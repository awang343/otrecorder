use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub mqtt: MqttConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub http: HttpConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MqttConfig {
    #[serde(default)]
    pub enabled: BoolDefaultTrue,
    #[serde(default = "default_mqtt_host")]
    pub host: String,
    #[serde(default = "default_mqtt_port")]
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default = "default_topic")]
    pub topic: String,
    #[serde(default = "default_client_id")]
    pub client_id: String,
    #[serde(default)]
    pub tls: bool,
    #[serde(default)]
    pub ca_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_db_path")]
    pub db_path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HttpConfig {
    #[serde(default)]
    pub enabled: BoolDefaultTrue,
    #[serde(default = "default_http_bind")]
    pub bind: String,
    #[serde(default)]
    pub cors_any_origin: bool,
}

/// Wrapper that defaults to `true` (serde Default trait gives `false` for bool).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(transparent)]
pub struct BoolDefaultTrue(pub bool);
impl Default for BoolDefaultTrue {
    fn default() -> Self {
        BoolDefaultTrue(true)
    }
}
impl From<BoolDefaultTrue> for bool {
    fn from(b: BoolDefaultTrue) -> bool {
        b.0
    }
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            enabled: BoolDefaultTrue::default(),
            host: default_mqtt_host(),
            port: default_mqtt_port(),
            username: None,
            password: None,
            topic: default_topic(),
            client_id: default_client_id(),
            tls: false,
            ca_file: None,
        }
    }
}
impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            db_path: default_db_path(),
        }
    }
}
impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            enabled: BoolDefaultTrue::default(),
            bind: default_http_bind(),
            cors_any_origin: false,
        }
    }
}

fn default_mqtt_host() -> String {
    "localhost".into()
}
fn default_mqtt_port() -> u16 {
    1883
}
fn default_topic() -> String {
    "owntracks/#".into()
}
fn default_client_id() -> String {
    "owntracks-recorder".into()
}
fn default_http_bind() -> String {
    "127.0.0.1:8080".into()
}
fn default_db_path() -> PathBuf {
    data_dir().join("owntracks.db")
}

pub fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("otrecorder")
        .join("config.toml")
}

fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("otrecorder")
}

const TEMPLATE: &str = r#"# otrecorder config

[mqtt]
enabled  = true
host     = "localhost"
port     = 1883
# username = "your-mqtt-user"
# password = "your-mqtt-pass"
topic    = "owntracks/#"
client_id = "owntracks-recorder"
tls      = false
# ca_file is only needed for self-signed or private CAs. Public CAs like
# Let's Encrypt are trusted via the system root store automatically.
# ca_file  = "/path/to/ca.crt"

[storage]
# Path is expanded for ~ and $HOME. Parent directory is created if missing.
# db_path = "~/.local/share/otrecorder/owntracks.db"

[http]
enabled = true
bind    = "127.0.0.1:8080"
cors_any_origin = false
"#;

pub fn load(path: &Path) -> Result<Config> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read config file {}", path.display()))?;
    let cfg: Config =
        toml::from_str(&raw).with_context(|| format!("parse config file {}", path.display()))?;
    Ok(cfg)
}

/// If the config file does not exist, write a commented template and return
/// the path the user should edit. Returns `Ok(true)` when a template was created.
pub fn ensure_template(path: &Path) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create config dir {}", parent.display()))?;
    }
    std::fs::write(path, TEMPLATE)
        .with_context(|| format!("write config template {}", path.display()))?;
    Ok(true)
}

/// Expand a leading `~/` or `$HOME` in a path.
pub fn expand_path(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    if let Some(rest) = s.strip_prefix("$HOME/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    p.to_path_buf()
}
