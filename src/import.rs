use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::owntracks::Location;
use crate::storage::Storage;

#[derive(Debug, Default)]
pub struct ImportStats {
    pub files: u64,
    pub lines: u64,
    pub locations_inserted: u64,
    pub locations_duplicate: u64,
    pub messages_inserted: u64,
    pub skipped: u64,
}

pub fn import_rec_tree(
    storage: &Storage,
    root: &Path,
    user_filter: Option<&str>,
    device_filter: Option<&str>,
) -> Result<ImportStats> {
    let mut stats = ImportStats::default();
    let users = read_dir_names(root)
        .with_context(|| format!("read root {}", root.display()))?;
    for user in users {
        if let Some(u) = user_filter {
            if user != u {
                continue;
            }
        }
        let user_dir = root.join(&user);
        let devices = read_dir_names(&user_dir)
            .with_context(|| format!("read user dir {}", user_dir.display()))?;
        for device in devices {
            if let Some(d) = device_filter {
                if device != d {
                    continue;
                }
            }
            let device_dir = user_dir.join(&device);
            let rec_files = list_rec_files(&device_dir)
                .with_context(|| format!("read device dir {}", device_dir.display()))?;
            for path in rec_files {
                import_file(storage, &path, &user, &device, &mut stats)
                    .with_context(|| format!("import {}", path.display()))?;
                stats.files += 1;
            }
        }
    }
    Ok(stats)
}

fn import_file(
    storage: &Storage,
    path: &Path,
    user: &str,
    device: &str,
    stats: &mut ImportStats,
) -> Result<()> {
    let topic = format!("owntracks/{user}/{device}");
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut file_inserted = 0u64;
    let mut file_dups = 0u64;
    for line in reader.lines() {
        let line = line?;
        stats.lines += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((ts_str, payload)) = parse_rec_line(trimmed) else {
            warn!(path = %path.display(), line_no = stats.lines, "malformed line, skipping");
            stats.skipped += 1;
            continue;
        };
        let received_at = match chrono::DateTime::parse_from_rfc3339(ts_str) {
            Ok(dt) => dt.timestamp(),
            Err(e) => {
                warn!(path = %path.display(), ts = ts_str, error = %e, "bad timestamp, skipping");
                stats.skipped += 1;
                continue;
            }
        };
        let value: serde_json::Value = match serde_json::from_str(payload) {
            Ok(v) => v,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "non-JSON payload, skipping");
                stats.skipped += 1;
                continue;
            }
        };
        let msg_type = value.get("_type").and_then(|v| v.as_str()).map(str::to_string);
        match msg_type.as_deref() {
            Some("location") => match serde_json::from_value::<Location>(value) {
                Ok(loc) => {
                    let inserted = storage.insert_location(
                        &topic,
                        user,
                        device,
                        received_at,
                        &loc,
                        payload,
                    )?;
                    if inserted {
                        stats.locations_inserted += 1;
                        file_inserted += 1;
                    } else {
                        stats.locations_duplicate += 1;
                        file_dups += 1;
                    }
                }
                Err(e) => {
                    warn!(error = %e, "parse location failed, skipping");
                    stats.skipped += 1;
                }
            },
            Some(other) => {
                storage.insert_message(&topic, user, device, Some(other), received_at, payload)?;
                stats.messages_inserted += 1;
            }
            None => {
                storage.insert_message(&topic, user, device, None, received_at, payload)?;
                stats.messages_inserted += 1;
            }
        }
    }
    info!(
        file = %path.display(),
        inserted = file_inserted,
        duplicates = file_dups,
        "imported file"
    );
    Ok(())
}

/// rec line: `<ISO_TS>\t<TYPE_LABEL>\t<JSON_PAYLOAD>` (TYPE_LABEL may be space-padded).
fn parse_rec_line(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.splitn(3, '\t');
    let ts = parts.next()?;
    let _label = parts.next()?;
    let payload = parts.next()?;
    Some((ts, payload.trim()))
}

fn read_dir_names(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if let Some(name) = entry.file_name().to_str() {
            out.push(name.to_string());
        }
    }
    out.sort();
    Ok(out)
}

fn list_rec_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file()
            && path.extension().and_then(|s| s.to_str()) == Some("rec")
        {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}
