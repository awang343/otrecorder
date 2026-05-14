mod api;
mod config;
mod export;
mod mqtt;
mod owntracks;
mod storage;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tokio::sync::oneshot;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "otrecorder",
    about = "OwnTracks MQTT recorder with HTTP API + Parquet export",
    version
)]
struct Cli {
    /// Path to config file (default: $XDG_CONFIG_HOME/otrecorder/config.toml).
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run the recorder and HTTP API (default).
    Run,
    /// Export locations to Parquet.
    Export(ExportArgs),
    /// Print the loaded config (after merging with the file at --config).
    ConfigShow,
}

#[derive(clap::Args, Debug)]
struct ExportArgs {
    /// Output file path (e.g. locations.parquet).
    #[arg(long)]
    out: PathBuf,
    /// Filter by user.
    #[arg(long)]
    user: Option<String>,
    /// Filter by device.
    #[arg(long)]
    device: Option<String>,
    /// Lower bound on `tst` (unix seconds or RFC3339).
    #[arg(long)]
    from: Option<String>,
    /// Upper bound on `tst` (unix seconds or RFC3339).
    #[arg(long)]
    to: Option<String>,
    /// Parquet row group / batch size.
    #[arg(long, default_value_t = 8192)]
    batch_size: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let cfg_path = cli.config.unwrap_or_else(config::default_config_path);

    if config::ensure_template(&cfg_path)? {
        eprintln!(
            "Wrote default config template to {}. Edit it and re-run.",
            cfg_path.display()
        );
        return Ok(());
    }

    let mut cfg = config::load(&cfg_path)
        .with_context(|| format!("loading config from {}", cfg_path.display()))?;
    cfg.storage.db_path = config::expand_path(&cfg.storage.db_path);
    if let Some(ca) = cfg.mqtt.ca_file.clone() {
        cfg.mqtt.ca_file = Some(config::expand_path(&ca));
    }

    match cli.cmd.unwrap_or(Cmd::Run) {
        Cmd::Run => run(cfg).await,
        Cmd::Export(args) => run_export(cfg, args),
        Cmd::ConfigShow => {
            println!("{:#?}", cfg);
            Ok(())
        }
    }
}

async fn run(cfg: config::Config) -> Result<()> {
    info!(
        broker = %format!("{}:{}", cfg.mqtt.host, cfg.mqtt.port),
        topic = %cfg.mqtt.topic,
        db = %cfg.storage.db_path.display(),
        http_bind = %cfg.http.bind,
        mqtt_enabled = bool::from(cfg.mqtt.enabled),
        http_enabled = bool::from(cfg.http.enabled),
        "starting otrecorder",
    );

    let storage = storage::Storage::open(&cfg.storage.db_path)?;

    let (mqtt_shutdown_tx, mqtt_shutdown_rx) = oneshot::channel::<()>();
    let mqtt_handle: Option<tokio::task::JoinHandle<Result<()>>> = if bool::from(cfg.mqtt.enabled) {
        let storage_for_mqtt = storage.clone();
        let mqtt_cfg = cfg.mqtt.clone();
        Some(tokio::spawn(async move {
            mqtt::run(mqtt_cfg, storage_for_mqtt, mqtt_shutdown_rx).await
        }))
    } else {
        info!("mqtt disabled in config; skipping recorder loop");
        None
    };

    let (http_shutdown_tx, http_shutdown_rx) = oneshot::channel::<()>();
    let http_handle: Option<tokio::task::JoinHandle<Result<()>>> = if bool::from(cfg.http.enabled) {
        let storage_for_http = storage.clone();
        let bind = cfg.http.bind.clone();
        let cors = cfg.http.cors_any_origin;
        Some(tokio::spawn(async move {
            serve_http(storage_for_http, &bind, cors, http_shutdown_rx).await
        }))
    } else {
        info!("http disabled in config; skipping api");
        None
    };

    tokio::signal::ctrl_c()
        .await
        .context("install ctrl-c handler")?;
    info!("ctrl-c; shutting down");
    let _ = mqtt_shutdown_tx.send(());
    let _ = http_shutdown_tx.send(());

    if let Some(h) = mqtt_handle {
        if let Err(e) = h.await {
            warn!(error = %e, "mqtt task join error");
        }
    }
    if let Some(h) = http_handle {
        if let Err(e) = h.await {
            warn!(error = %e, "http task join error");
        }
    }
    Ok(())
}

async fn serve_http(
    storage: storage::Storage,
    bind: &str,
    cors_any_origin: bool,
    shutdown: oneshot::Receiver<()>,
) -> Result<()> {
    let app = api::router(api::AppState { storage }, cors_any_origin);
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("bind {bind}"))?;
    info!(addr = %bind, "http api listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown.await;
        })
        .await
        .context("axum serve")?;
    Ok(())
}

fn run_export(cfg: config::Config, args: ExportArgs) -> Result<()> {
    let storage = storage::Storage::open(&cfg.storage.db_path)?;
    let filter = storage::LocationFilter {
        user: args.user,
        device: args.device,
        from_tst: args.from.as_deref().map(parse_timestamp).transpose()?,
        to_tst: args.to.as_deref().map(parse_timestamp).transpose()?,
        limit: None,
        offset: None,
        descending: false,
    };
    let count = export::export_parquet(&storage, &args.out, &filter, args.batch_size)?;
    println!(
        "exported {count} rows to {} ({})",
        args.out.display(),
        humanize_bytes(file_size(&args.out).unwrap_or(0))
    );
    Ok(())
}

fn parse_timestamp(s: &str) -> Result<i64> {
    if let Ok(n) = s.parse::<i64>() {
        return Ok(n);
    }
    let dt = chrono::DateTime::parse_from_rfc3339(s)
        .with_context(|| format!("parse timestamp {s:?} (expected unix seconds or RFC3339)"))?;
    Ok(dt.timestamp())
}

fn file_size(p: &std::path::Path) -> Option<u64> {
    std::fs::metadata(p).ok().map(|m| m.len())
}

fn humanize_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = n as f64;
    let mut idx = 0;
    while size >= 1024.0 && idx < UNITS.len() - 1 {
        size /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{} {}", n, UNITS[0])
    } else {
        format!("{:.2} {}", size, UNITS[idx])
    }
}
