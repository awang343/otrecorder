use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Deserializer, Serialize};
use std::path::PathBuf;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing::{error, info, warn};

use crate::storage::{Bbox, LocationFilter, LocationRow, Storage};

#[derive(Clone)]
pub struct AppState {
    pub storage: Storage,
}

pub fn router(
    state: AppState,
    cors_any: bool,
    tiles_pmtiles: Option<PathBuf>,
    static_root: Option<PathBuf>,
) -> Router {
    let mut router = Router::new()
        .route("/api/health", get(health))
        .route("/api/stats", get(stats))
        .route("/api/users", get(users))
        .route("/api/users/:user/devices", get(devices_for_user))
        .route("/api/devices", get(devices_all))
        .route("/api/locations", get(locations))
        .route("/api/locations/latest", get(locations_latest))
        .route("/api/locations/bbox", get(locations_bbox))
        .route("/api/locations/near", get(locations_near))
        .route("/api/track.geojson", get(track_geojson))
        .route("/api/track.gpx", get(track_gpx))
        .with_state(state);

    if let Some(path) = tiles_pmtiles {
        if path.is_file() {
            info!(path = %path.display(), "serving pmtiles at /tiles/map.pmtiles");
            router = router.route_service("/tiles/map.pmtiles", ServeFile::new(path));
        } else {
            warn!(path = %path.display(), "tiles_pmtiles is set but file does not exist");
        }
    }

    if let Some(dir) = static_root {
        if dir.is_dir() {
            let index = dir.join("index.html");
            info!(path = %dir.display(), "serving frontend from {}", dir.display());
            let serve_dir = ServeDir::new(&dir).fallback(ServeFile::new(index));
            router = router.fallback_service(serve_dir);
        } else {
            warn!(path = %dir.display(), "static_root is set but directory does not exist");
        }
    }

    if cors_any {
        router = router.layer(CorsLayer::permissive());
    }
    router.layer(TraceLayer::new_for_http())
}

// ---------- handlers ----------

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

async fn stats(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let stats = blocking(move || s.storage.stats()).await?;
    Ok(Json(serde_json::to_value(stats).unwrap()))
}

async fn users(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = blocking(move || s.storage.list_users()).await?;
    Ok(Json(serde_json::json!({ "users": rows })))
}

async fn devices_for_user(
    State(s): State<AppState>,
    Path(user): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = blocking(move || s.storage.list_devices(Some(&user))).await?;
    Ok(Json(serde_json::json!({ "devices": rows })))
}

async fn devices_all(State(s): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = blocking(move || s.storage.list_devices(None)).await?;
    Ok(Json(serde_json::json!({ "devices": rows })))
}

#[derive(Debug, Deserialize, Default)]
pub struct LocationsQuery {
    pub user: Option<String>,
    pub device: Option<String>,
    #[serde(default, deserialize_with = "de_timestamp")]
    pub from: Option<i64>,
    #[serde(default, deserialize_with = "de_timestamp")]
    pub to: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    #[serde(default)]
    pub order: Order,
}

#[derive(Debug, Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Order {
    #[default]
    Asc,
    Desc,
}

impl LocationsQuery {
    fn into_filter(self) -> LocationFilter {
        LocationFilter {
            user: self.user,
            device: self.device,
            from_tst: self.from,
            to_tst: self.to,
            limit: self.limit,
            offset: self.offset,
            descending: matches!(self.order, Order::Desc),
        }
    }
}

async fn locations(
    State(s): State<AppState>,
    Query(q): Query<LocationsQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let filter = q.into_filter();
    let rows = blocking(move || s.storage.query_locations(&filter)).await?;
    Ok(Json(serde_json::json!({ "count": rows.len(), "locations": rows })))
}

#[derive(Debug, Deserialize, Default)]
pub struct LatestQuery {
    pub user: Option<String>,
    pub device: Option<String>,
}

async fn locations_latest(
    State(s): State<AppState>,
    Query(q): Query<LatestQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let rows = blocking(move || s.storage.latest_per_device(q.user.as_deref(), q.device.as_deref()))
        .await?;
    Ok(Json(serde_json::json!({ "count": rows.len(), "locations": rows })))
}

#[derive(Debug, Deserialize)]
pub struct BboxQuery {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
    pub user: Option<String>,
    pub device: Option<String>,
    #[serde(default, deserialize_with = "de_timestamp")]
    pub from: Option<i64>,
    #[serde(default, deserialize_with = "de_timestamp")]
    pub to: Option<i64>,
    pub limit: Option<i64>,
    #[serde(default)]
    pub order: Order,
}

async fn locations_bbox(
    State(s): State<AppState>,
    Query(q): Query<BboxQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let bbox = Bbox {
        min_lat: q.min_lat,
        max_lat: q.max_lat,
        min_lon: q.min_lon,
        max_lon: q.max_lon,
    };
    let filter = LocationFilter {
        user: q.user,
        device: q.device,
        from_tst: q.from,
        to_tst: q.to,
        limit: q.limit,
        offset: None,
        descending: matches!(q.order, Order::Desc),
    };
    let rows = blocking(move || s.storage.query_bbox(&bbox, &filter)).await?;
    Ok(Json(serde_json::json!({ "count": rows.len(), "locations": rows })))
}

#[derive(Debug, Deserialize)]
pub struct NearQuery {
    pub lat: f64,
    pub lon: f64,
    pub radius_m: f64,
    pub user: Option<String>,
    pub device: Option<String>,
    #[serde(default, deserialize_with = "de_timestamp")]
    pub from: Option<i64>,
    #[serde(default, deserialize_with = "de_timestamp")]
    pub to: Option<i64>,
    pub limit: Option<i64>,
}

async fn locations_near(
    State(s): State<AppState>,
    Query(q): Query<NearQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if q.radius_m <= 0.0 {
        return Err(ApiError::bad_request("radius_m must be > 0"));
    }
    let center = (q.lat, q.lon);
    let bbox = bbox_around(q.lat, q.lon, q.radius_m);
    let filter = LocationFilter {
        user: q.user,
        device: q.device,
        from_tst: q.from,
        to_tst: q.to,
        limit: q.limit.map(|n| n.saturating_mul(4)),
        offset: None,
        descending: false,
    };
    let radius = q.radius_m;
    let limit = q.limit.unwrap_or(1000).max(1) as usize;
    let rows: Vec<LocationRow> = blocking(move || s.storage.query_bbox(&bbox, &filter)).await?;
    let mut filtered: Vec<(f64, LocationRow)> = rows
        .into_iter()
        .filter_map(|r| {
            let d = haversine_meters(center.0, center.1, r.lat, r.lon);
            (d <= radius).then_some((d, r))
        })
        .collect();
    filtered.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    filtered.truncate(limit);
    let out: Vec<_> = filtered
        .into_iter()
        .map(|(d, r)| {
            let mut v = serde_json::to_value(&r).unwrap();
            v.as_object_mut().unwrap().insert(
                "distance_m".to_string(),
                serde_json::Value::from((d * 100.0).round() / 100.0),
            );
            v
        })
        .collect();
    Ok(Json(serde_json::json!({ "count": out.len(), "locations": out })))
}

#[derive(Debug, Deserialize)]
pub struct TrackQuery {
    pub user: String,
    pub device: String,
    #[serde(default, deserialize_with = "de_timestamp")]
    pub from: Option<i64>,
    #[serde(default, deserialize_with = "de_timestamp")]
    pub to: Option<i64>,
    pub limit: Option<i64>,
}

impl TrackQuery {
    fn filter(&self) -> LocationFilter {
        LocationFilter {
            user: Some(self.user.clone()),
            device: Some(self.device.clone()),
            from_tst: self.from,
            to_tst: self.to,
            limit: self.limit.or(Some(50_000)),
            offset: None,
            descending: false,
        }
    }
}

async fn track_geojson(
    State(s): State<AppState>,
    Query(q): Query<TrackQuery>,
) -> Result<Response, ApiError> {
    let filter = q.filter();
    let user = q.user.clone();
    let device = q.device.clone();
    let rows = blocking(move || s.storage.query_locations(&filter)).await?;

    let coordinates: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| match r.alt {
            Some(a) => serde_json::json!([r.lon, r.lat, a]),
            None => serde_json::json!([r.lon, r.lat]),
        })
        .collect();
    let timestamps: Vec<i64> = rows.iter().map(|r| r.tst).collect();
    let feature = serde_json::json!({
        "type": "Feature",
        "geometry": {
            "type": "LineString",
            "coordinates": coordinates,
        },
        "properties": {
            "user": user,
            "device": device,
            "count": rows.len(),
            "from": rows.first().map(|r| r.tst),
            "to": rows.last().map(|r| r.tst),
            "timestamps": timestamps,
        }
    });

    let body = serde_json::to_string(&feature).map_err(ApiError::internal)?;
    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/geo+json")],
        body,
    )
        .into_response())
}

async fn track_gpx(
    State(s): State<AppState>,
    Query(q): Query<TrackQuery>,
) -> Result<Response, ApiError> {
    let filter = q.filter();
    let user = q.user.clone();
    let device = q.device.clone();
    let rows = blocking(move || s.storage.query_locations(&filter)).await?;

    let mut xml = String::with_capacity(rows.len() * 128);
    xml.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    xml.push('\n');
    xml.push_str(r#"<gpx version="1.1" creator="otrecorder" xmlns="http://www.topografix.com/GPX/1/1">"#);
    xml.push('\n');
    xml.push_str("  <trk>\n    <name>");
    push_xml_escaped(&mut xml, &format!("{user}/{device}"));
    xml.push_str("</name>\n    <trkseg>\n");
    for r in &rows {
        xml.push_str(&format!(
            "      <trkpt lat=\"{:.7}\" lon=\"{:.7}\">\n",
            r.lat, r.lon
        ));
        if let Some(alt) = r.alt {
            xml.push_str(&format!("        <ele>{alt}</ele>\n"));
        }
        if let Some(dt) = chrono::DateTime::<chrono::Utc>::from_timestamp(r.tst, 0) {
            xml.push_str(&format!(
                "        <time>{}</time>\n",
                dt.format("%Y-%m-%dT%H:%M:%SZ")
            ));
        }
        if let Some(vel) = r.vel {
            xml.push_str(&format!(
                "        <extensions><speed>{vel}</speed></extensions>\n"
            ));
        }
        xml.push_str("      </trkpt>\n");
    }
    xml.push_str("    </trkseg>\n  </trk>\n</gpx>\n");

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/gpx+xml")],
        xml,
    )
        .into_response())
}

// ---------- helpers ----------

fn push_xml_escaped(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
}

fn haversine_meters(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6_371_000.0_f64;
    let to_rad = std::f64::consts::PI / 180.0;
    let dlat = (lat2 - lat1) * to_rad;
    let dlon = (lon2 - lon1) * to_rad;
    let a = (dlat / 2.0).sin().powi(2)
        + (lat1 * to_rad).cos() * (lat2 * to_rad).cos() * (dlon / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().asin()
}

fn bbox_around(lat: f64, lon: f64, radius_m: f64) -> Bbox {
    let lat_deg = radius_m / 111_111.0;
    let cos_lat = lat.to_radians().cos().max(0.000_001);
    let lon_deg = radius_m / (111_111.0 * cos_lat);
    Bbox {
        min_lat: lat - lat_deg,
        max_lat: lat + lat_deg,
        min_lon: lon - lon_deg,
        max_lon: lon + lon_deg,
    }
}

fn de_timestamp<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    let opt: Option<String> = Option::deserialize(d)?;
    match opt {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => {
            if let Ok(n) = s.parse::<i64>() {
                return Ok(Some(n));
            }
            chrono::DateTime::parse_from_rfc3339(&s)
                .map(|dt| Some(dt.timestamp()))
                .map_err(serde::de::Error::custom)
        }
    }
}

async fn blocking<F, T>(f: F) -> Result<T, ApiError>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError::internal(anyhow::anyhow!("join error: {e}")))?
        .map_err(ApiError::internal)
}

#[derive(Debug, Serialize)]
pub struct ApiError {
    #[serde(skip)]
    status: StatusCode,
    error: String,
}

impl ApiError {
    fn internal(e: impl std::fmt::Display) -> Self {
        let msg = e.to_string();
        error!(error = %msg, "internal error");
        ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error: msg,
        }
    }
    fn bad_request(msg: impl Into<String>) -> Self {
        ApiError {
            status: StatusCode::BAD_REQUEST,
            error: msg.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(serde_json::json!({ "error": self.error }))).into_response()
    }
}
