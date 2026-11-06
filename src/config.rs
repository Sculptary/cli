use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_HOST: &str = "https://sculptary.com";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub hosts: std::collections::BTreeMap<String, HostConfig>,
    #[serde(default)]
    pub default_host: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostConfig {
    pub username: String,
    pub token: String,
}

pub fn path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("SCULPTARY_CONFIG") {
        return Ok(PathBuf::from(p));
    }
    Ok(dirs::config_dir().context("no config directory on this system")?.join("sculptary").join("config.toml"))
}

impl Config {
    pub fn load() -> Result<Self> {
        let p = path()?;
        if !p.exists() {
            return Ok(Self::default());
        }
        toml::from_str(&std::fs::read_to_string(&p)?).with_context(|| format!("invalid config at {}", p.display()))
    }

    pub fn save(&self) -> Result<()> {
        let p = path()?;
        std::fs::create_dir_all(p.parent().unwrap())?;
        let text = toml::to_string_pretty(self)?;
        // Write with owner-only permissions: the file holds access tokens.
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(&p)?;
            f.write_all(text.as_bytes())?;
        }
        #[cfg(not(unix))]
        std::fs::write(&p, text)?;
        Ok(())
    }

    pub fn host(&self, explicit: Option<&str>) -> String {
        normalize(explicit.or(self.default_host.as_deref()).unwrap_or(DEFAULT_HOST))
    }

    pub fn credentials(&self, host: &str) -> Option<&HostConfig> {
        self.hosts.get(&normalize(host))
    }
}

pub fn normalize(host: &str) -> String {
    let h = host.trim().trim_end_matches('/');
    if h.starts_with("http://") || h.starts_with("https://") { h.to_string() } else { format!("https://{h}") }
}
