use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
    pub tst: i64,
    #[serde(default)]
    pub acc: Option<f64>,
    #[serde(default)]
    pub alt: Option<f64>,
    #[serde(default)]
    pub vel: Option<f64>,
    #[serde(default)]
    pub cog: Option<f64>,
    #[serde(default)]
    pub batt: Option<f64>,
    #[serde(default)]
    pub bs: Option<i64>,
    #[serde(default, rename = "t")]
    pub trigger: Option<String>,
    #[serde(default)]
    pub tid: Option<String>,
    #[serde(default)]
    pub conn: Option<String>,
    #[serde(default)]
    pub vac: Option<f64>,
    #[serde(default, rename = "p")]
    pub pressure: Option<f64>,
}

/// Parse topic like "owntracks/<user>/<device>[/...]" into (user, device).
/// Falls back to ("", topic) when the topic doesn't match the canonical layout.
pub fn parse_topic(topic: &str) -> (String, String) {
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() >= 3 && parts[0].eq_ignore_ascii_case("owntracks") {
        (parts[1].to_string(), parts[2].to_string())
    } else {
        (String::new(), topic.to_string())
    }
}
