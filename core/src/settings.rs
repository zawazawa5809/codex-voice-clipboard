//! Persisted settings. Never holds transcript text (spec 1.7).

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub auto_copy: bool,
    pub delay_seconds: f64,
    pub risk_notice_seen: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { auto_copy: true, delay_seconds: 2.0, risk_notice_seen: false }
    }
}

impl Settings {
    /// Missing or unreadable settings fall back to defaults; the delay is normalised.
    pub fn load(path: &Path) -> Self {
        let mut settings: Settings = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        settings.delay_seconds = normalize_delay(settings.delay_seconds);
        settings
    }

    /// Write-then-rename so a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut temp = path.as_os_str().to_owned();
        temp.push(".tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&temp, path)
    }
}

/// 0–60 seconds in 0.1 steps; anything unusable becomes the 2-second default.
pub fn normalize_delay(seconds: f64) -> f64 {
    if !seconds.is_finite() {
        return 2.0;
    }
    (seconds.clamp(0.0, 60.0) * 10.0).round() / 10.0
}
