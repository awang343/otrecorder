use anyhow::{Context, Result};
use rumqttc::{AsyncClient, Event, EventLoop, Incoming, MqttOptions, QoS, TlsConfiguration, Transport};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use tracing::{debug, error, info, warn};

use crate::config::MqttConfig;
use crate::owntracks::{self, Location};
use crate::storage::Storage;

pub async fn run(cfg: MqttConfig, storage: Storage, mut shutdown: oneshot::Receiver<()>) -> Result<()> {
    let mut opts = MqttOptions::new(&cfg.client_id, &cfg.host, cfg.port);
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_clean_session(false);
    opts.set_max_packet_size(1024 * 1024, 1024 * 1024);

    if let Some(user) = cfg.username.as_deref() {
        let pass = cfg.password.as_deref().unwrap_or("");
        opts.set_credentials(user, pass);
    }

    if cfg.tls {
        let tls = match cfg.ca_file.as_deref() {
            Some(ca_path) => {
                let ca = std::fs::read(ca_path)
                    .with_context(|| format!("read ca file {:?}", ca_path))?;
                TlsConfiguration::Simple {
                    ca,
                    alpn: None,
                    client_auth: None,
                }
            }
            None => {
                let mut roots = rustls::RootCertStore::empty();
                let certs = rustls_native_certs::load_native_certs()
                    .context("load native root certificates")?;
                for cert in certs {
                    roots
                        .add(cert)
                        .context("add system cert to root store")?;
                }
                let client_config = rustls::ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth();
                TlsConfiguration::Rustls(Arc::new(client_config))
            }
        };
        opts.set_transport(Transport::Tls(tls));
    }

    let (client, mut eventloop) = AsyncClient::new(opts, 256);
    client
        .subscribe(&cfg.topic, QoS::AtLeastOnce)
        .await
        .context("initial subscribe")?;
    info!(topic = %cfg.topic, "subscribed");

    let topic = cfg.topic.clone();
    let client_for_loop = client.clone();
    let storage_for_loop = storage.clone();
    let event_task = tokio::spawn(async move {
        event_loop(&mut eventloop, storage_for_loop, client_for_loop, topic).await;
    });

    tokio::select! {
        _ = &mut shutdown => {
            info!("recorder shutdown requested");
            let _ = client.disconnect().await;
        }
        _ = event_task => {
            warn!("mqtt event loop exited");
        }
    }
    Ok(())
}

async fn event_loop(
    eventloop: &mut EventLoop,
    storage: Storage,
    client: AsyncClient,
    topic: String,
) {
    loop {
        match eventloop.poll().await {
            Ok(Event::Incoming(Incoming::Publish(p))) => {
                let t = p.topic.clone();
                let payload = String::from_utf8_lossy(&p.payload).into_owned();
                if let Err(e) = handle_publish(&storage, &t, &payload) {
                    error!(topic = %t, error = %e, "handle publish failed");
                }
            }
            Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                info!("connected to broker");
                if let Err(e) = client.subscribe(&topic, QoS::AtLeastOnce).await {
                    warn!(error = %e, "re-subscribe failed");
                }
            }
            Ok(Event::Incoming(Incoming::Disconnect)) => {
                info!("broker sent disconnect");
            }
            Ok(other) => {
                debug!(?other, "event");
            }
            Err(e) => {
                warn!(error = %e, "mqtt connection error; reconnecting");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

fn handle_publish(storage: &Storage, topic: &str, payload: &str) -> Result<()> {
    let (user, device) = owntracks::parse_topic(topic);
    let received_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let value: serde_json::Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(e) => {
            warn!(topic = %topic, error = %e, "non-JSON payload, skipping");
            return Ok(());
        }
    };

    let msg_type = value
        .get("_type")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    match msg_type.as_deref() {
        Some("location") => {
            let loc: Location = serde_json::from_value(value).context("parse location")?;
            let inserted =
                storage.insert_location(topic, &user, &device, received_at, &loc, payload)?;
            if inserted {
                info!(user, device, lat = loc.lat, lon = loc.lon, tst = loc.tst, "recorded");
            } else {
                debug!(user, device, tst = loc.tst, "duplicate ignored");
            }
        }
        Some(other) => {
            storage.insert_message(topic, &user, &device, Some(other), received_at, payload)?;
            debug!(user, device, msg_type = other, "stored message");
        }
        None => {
            storage.insert_message(topic, &user, &device, None, received_at, payload)?;
            debug!(user, device, "stored untyped message");
        }
    }
    Ok(())
}
