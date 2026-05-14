use anyhow::{Context, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, params_from_iter, types::Value};
use serde::Serialize;
use std::path::Path;

use crate::owntracks::Location;

#[derive(Clone)]
pub struct Storage {
    pool: Pool<SqliteConnectionManager>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct LocationRow {
    pub id: i64,
    pub topic: String,
    pub user: String,
    pub device: String,
    pub tst: i64,
    pub received_at: i64,
    pub lat: f64,
    pub lon: f64,
    pub acc: Option<f64>,
    pub alt: Option<f64>,
    pub vel: Option<f64>,
    pub cog: Option<f64>,
    pub batt: Option<f64>,
    pub bs: Option<i64>,
    pub trigger: Option<String>,
    pub tid: Option<String>,
    pub conn: Option<String>,
    pub vac: Option<f64>,
    pub pressure: Option<f64>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct UserSummary {
    pub user: String,
    pub device_count: i64,
    pub location_count: i64,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct DeviceSummary {
    pub user: String,
    pub device: String,
    pub location_count: i64,
    pub first_tst: Option<i64>,
    pub last_tst: Option<i64>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Stats {
    pub total_locations: i64,
    pub total_messages: i64,
    pub oldest_tst: Option<i64>,
    pub newest_tst: Option<i64>,
    pub user_count: i64,
    pub device_count: i64,
    pub db_size_bytes: i64,
}

#[derive(Debug, Default, Clone)]
pub struct LocationFilter {
    pub user: Option<String>,
    pub device: Option<String>,
    pub from_tst: Option<i64>,
    pub to_tst: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub descending: bool,
}

#[derive(Debug, Clone)]
pub struct Bbox {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
}

impl Storage {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create db parent dir {}", parent.display()))?;
            }
        }
        // journal_mode is database-file-scoped and persists once set.
        // Apply it on a standalone connection before the pool exists so two
        // pool connections never race on the WAL-setup exclusive lock.
        {
            let conn = rusqlite::Connection::open(path)
                .with_context(|| format!("open {} to set WAL", path.display()))?;
            let mode: String = conn
                .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
                .context("set journal_mode = WAL")?;
            anyhow::ensure!(
                mode.eq_ignore_ascii_case("wal"),
                "failed to enable WAL mode (got {mode}); is the db on a filesystem that supports it?"
            );
        }
        let manager = SqliteConnectionManager::file(path).with_init(|c| {
            c.execute_batch(
                "PRAGMA synchronous = NORMAL;
                 PRAGMA temp_store = MEMORY;
                 PRAGMA foreign_keys = ON;
                 PRAGMA busy_timeout = 5000;",
            )
        });
        let pool = Pool::builder().max_size(8).build(manager)?;
        let conn = pool.get()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Storage { pool })
    }

    fn conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>> {
        self.pool.get().context("acquire sqlite connection")
    }

    pub fn insert_location(
        &self,
        topic: &str,
        user: &str,
        device: &str,
        received_at: i64,
        loc: &Location,
        raw: &str,
    ) -> Result<bool> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let changed = tx.execute(
            "INSERT OR IGNORE INTO locations (
                topic, user, device, tst, received_at, lat, lon,
                acc, alt, vel, cog, batt, bs, trigger_kind, tid, conn, vac, pressure, raw
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19
             )",
            params![
                topic,
                user,
                device,
                loc.tst,
                received_at,
                loc.lat,
                loc.lon,
                loc.acc,
                loc.alt,
                loc.vel,
                loc.cog,
                loc.batt,
                loc.bs,
                loc.trigger,
                loc.tid,
                loc.conn,
                loc.vac,
                loc.pressure,
                raw,
            ],
        )?;
        if changed > 0 {
            let id = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO locations_rtree (id, min_lat, max_lat, min_lon, max_lon)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, loc.lat, loc.lat, loc.lon, loc.lon],
            )?;
        }
        tx.commit()?;
        Ok(changed > 0)
    }

    pub fn insert_message(
        &self,
        topic: &str,
        user: &str,
        device: &str,
        msg_type: Option<&str>,
        received_at: i64,
        payload: &str,
    ) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO messages (topic, user, device, type, received_at, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![topic, user, device, msg_type, received_at, payload],
        )?;
        Ok(())
    }

    pub fn list_users(&self) -> Result<Vec<UserSummary>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT user, COUNT(DISTINCT device) AS dc, COUNT(*) AS lc
             FROM locations GROUP BY user ORDER BY user",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(UserSummary {
                    user: r.get(0)?,
                    device_count: r.get(1)?,
                    location_count: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn list_devices(&self, user: Option<&str>) -> Result<Vec<DeviceSummary>> {
        let conn = self.conn()?;
        let (sql, params): (&str, Vec<Value>) = match user {
            Some(u) => (
                "SELECT user, device, COUNT(*), MIN(tst), MAX(tst)
                 FROM locations WHERE user = ?1
                 GROUP BY user, device ORDER BY device",
                vec![Value::Text(u.to_string())],
            ),
            None => (
                "SELECT user, device, COUNT(*), MIN(tst), MAX(tst)
                 FROM locations GROUP BY user, device ORDER BY user, device",
                vec![],
            ),
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map(params_from_iter(params), |r| {
                Ok(DeviceSummary {
                    user: r.get(0)?,
                    device: r.get(1)?,
                    location_count: r.get(2)?,
                    first_tst: r.get(3)?,
                    last_tst: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn stats(&self) -> Result<Stats> {
        let conn = self.conn()?;
        let (total_locations, oldest, newest): (i64, Option<i64>, Option<i64>) = conn
            .query_row(
                "SELECT COUNT(*), MIN(tst), MAX(tst) FROM locations",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or((0, None, None));
        let total_messages: i64 = conn
            .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
            .unwrap_or(0);
        let user_count: i64 = conn
            .query_row("SELECT COUNT(DISTINCT user) FROM locations", [], |r| r.get(0))
            .unwrap_or(0);
        let device_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM (SELECT 1 FROM locations GROUP BY user, device)",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let db_size_bytes: i64 = conn
            .query_row(
                "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        Ok(Stats {
            total_locations,
            total_messages,
            oldest_tst: oldest,
            newest_tst: newest,
            user_count,
            device_count,
            db_size_bytes,
        })
    }

    pub fn query_locations(&self, filter: &LocationFilter) -> Result<Vec<LocationRow>> {
        let mut sql = String::from("SELECT ");
        sql.push_str(SELECT_COLS);
        sql.push_str(" FROM locations");
        let mut where_clauses: Vec<&str> = Vec::new();
        let mut params: Vec<Value> = Vec::new();

        if filter.user.is_some() {
            where_clauses.push("user = ?");
            params.push(Value::Text(filter.user.clone().unwrap()));
        }
        if filter.device.is_some() {
            where_clauses.push("device = ?");
            params.push(Value::Text(filter.device.clone().unwrap()));
        }
        if let Some(from) = filter.from_tst {
            where_clauses.push("tst >= ?");
            params.push(Value::Integer(from));
        }
        if let Some(to) = filter.to_tst {
            where_clauses.push("tst <= ?");
            params.push(Value::Integer(to));
        }
        if !where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clauses.join(" AND "));
        }
        sql.push_str(if filter.descending {
            " ORDER BY tst DESC"
        } else {
            " ORDER BY tst ASC"
        });
        let limit = filter.limit.unwrap_or(1000).clamp(1, 100_000);
        sql.push_str(" LIMIT ?");
        params.push(Value::Integer(limit));
        if let Some(off) = filter.offset {
            sql.push_str(" OFFSET ?");
            params.push(Value::Integer(off.max(0)));
        }

        let conn = self.conn()?;
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params), row_to_location)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn latest_per_device(
        &self,
        user: Option<&str>,
        device: Option<&str>,
    ) -> Result<Vec<LocationRow>> {
        let mut sql = format!(
            "SELECT {cols} FROM locations l
             JOIN (
                 SELECT user, device, MAX(tst) AS max_tst
                 FROM locations
                 {where_clause}
                 GROUP BY user, device
             ) m ON m.user = l.user AND m.device = l.device AND m.max_tst = l.tst",
            cols = SELECT_COLS_WITH_PREFIX,
            where_clause = match (user, device) {
                (Some(_), Some(_)) => "WHERE user = ?1 AND device = ?2",
                (Some(_), None) => "WHERE user = ?1",
                _ => "",
            }
        );
        sql.push_str(" ORDER BY l.user, l.device");
        let mut params: Vec<Value> = Vec::new();
        if let Some(u) = user {
            params.push(Value::Text(u.to_string()));
        }
        if let Some(d) = device {
            params.push(Value::Text(d.to_string()));
        }
        let conn = self.conn()?;
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params), row_to_location)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn query_bbox(
        &self,
        bbox: &Bbox,
        filter: &LocationFilter,
    ) -> Result<Vec<LocationRow>> {
        let mut sql = format!(
            "SELECT {cols} FROM locations l
             JOIN locations_rtree r ON r.id = l.id
             WHERE r.min_lat >= ?1 AND r.max_lat <= ?2
               AND r.min_lon >= ?3 AND r.max_lon <= ?4",
            cols = SELECT_COLS_WITH_PREFIX,
        );
        let mut params: Vec<Value> = vec![
            Value::Real(bbox.min_lat),
            Value::Real(bbox.max_lat),
            Value::Real(bbox.min_lon),
            Value::Real(bbox.max_lon),
        ];
        if let Some(u) = &filter.user {
            sql.push_str(" AND l.user = ?");
            params.push(Value::Text(u.clone()));
        }
        if let Some(d) = &filter.device {
            sql.push_str(" AND l.device = ?");
            params.push(Value::Text(d.clone()));
        }
        if let Some(from) = filter.from_tst {
            sql.push_str(" AND l.tst >= ?");
            params.push(Value::Integer(from));
        }
        if let Some(to) = filter.to_tst {
            sql.push_str(" AND l.tst <= ?");
            params.push(Value::Integer(to));
        }
        sql.push_str(if filter.descending {
            " ORDER BY l.tst DESC"
        } else {
            " ORDER BY l.tst ASC"
        });
        let limit = filter.limit.unwrap_or(10_000).clamp(1, 100_000);
        sql.push_str(" LIMIT ?");
        params.push(Value::Integer(limit));

        let conn = self.conn()?;
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(params), row_to_location)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Stream locations for export. The callback is invoked with each row.
    pub fn stream_locations<F>(&self, filter: &LocationFilter, mut sink: F) -> Result<u64>
    where
        F: FnMut(LocationRow) -> Result<()>,
    {
        let mut sql = String::from("SELECT ");
        sql.push_str(SELECT_COLS);
        sql.push_str(" FROM locations");
        let mut where_clauses: Vec<&str> = Vec::new();
        let mut params: Vec<Value> = Vec::new();
        if let Some(u) = &filter.user {
            where_clauses.push("user = ?");
            params.push(Value::Text(u.clone()));
        }
        if let Some(d) = &filter.device {
            where_clauses.push("device = ?");
            params.push(Value::Text(d.clone()));
        }
        if let Some(from) = filter.from_tst {
            where_clauses.push("tst >= ?");
            params.push(Value::Integer(from));
        }
        if let Some(to) = filter.to_tst {
            where_clauses.push("tst <= ?");
            params.push(Value::Integer(to));
        }
        if !where_clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&where_clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY user, device, tst");

        let conn = self.conn()?;
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(params_from_iter(params))?;
        let mut count: u64 = 0;
        while let Some(r) = rows.next()? {
            sink(row_to_location(r)?)?;
            count += 1;
        }
        Ok(count)
    }

}

fn row_to_location(r: &rusqlite::Row<'_>) -> rusqlite::Result<LocationRow> {
    Ok(LocationRow {
        id: r.get(0)?,
        topic: r.get(1)?,
        user: r.get(2)?,
        device: r.get(3)?,
        tst: r.get(4)?,
        received_at: r.get(5)?,
        lat: r.get(6)?,
        lon: r.get(7)?,
        acc: r.get(8)?,
        alt: r.get(9)?,
        vel: r.get(10)?,
        cog: r.get(11)?,
        batt: r.get(12)?,
        bs: r.get(13)?,
        trigger: r.get(14)?,
        tid: r.get(15)?,
        conn: r.get(16)?,
        vac: r.get(17)?,
        pressure: r.get(18)?,
    })
}

const SELECT_COLS: &str = "id, topic, user, device, tst, received_at, lat, lon, acc, alt, vel, cog, batt, bs, trigger_kind, tid, conn, vac, pressure";
const SELECT_COLS_WITH_PREFIX: &str = "l.id, l.topic, l.user, l.device, l.tst, l.received_at, l.lat, l.lon, l.acc, l.alt, l.vel, l.cog, l.batt, l.bs, l.trigger_kind, l.tid, l.conn, l.vac, l.pressure";

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS locations (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    topic         TEXT    NOT NULL,
    user          TEXT    NOT NULL,
    device        TEXT    NOT NULL,
    tst           INTEGER NOT NULL,
    received_at   INTEGER NOT NULL,
    lat           REAL    NOT NULL,
    lon           REAL    NOT NULL,
    acc           REAL,
    alt           REAL,
    vel           REAL,
    cog           REAL,
    batt          REAL,
    bs            INTEGER,
    trigger_kind  TEXT,
    tid           TEXT,
    conn          TEXT,
    vac           REAL,
    pressure      REAL,
    raw           TEXT    NOT NULL,
    UNIQUE(user, device, tst)
);

CREATE INDEX IF NOT EXISTS idx_locations_user_device_tst ON locations(user, device, tst);
CREATE INDEX IF NOT EXISTS idx_locations_tst            ON locations(tst);

CREATE VIRTUAL TABLE IF NOT EXISTS locations_rtree USING rtree(
    id,
    min_lat, max_lat,
    min_lon, max_lon
);

CREATE TABLE IF NOT EXISTS messages (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    topic        TEXT    NOT NULL,
    user         TEXT    NOT NULL,
    device       TEXT    NOT NULL,
    type         TEXT,
    received_at  INTEGER NOT NULL,
    payload      TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_messages_user_device_received ON messages(user, device, received_at);
CREATE INDEX IF NOT EXISTS idx_messages_type                 ON messages(type);
"#;
