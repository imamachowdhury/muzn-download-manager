//! User settings: stored as one JSON value, defaults for anything missing.

use std::path::PathBuf;

use mdm_engine::{EngineConfig, Proxy, MAX_CONNECTIONS};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

/// How downloads reach the network.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ProxySetting {
    /// The OS / environment proxy.
    System,
    /// Direct.
    None,
    /// This proxy URL.
    Manual {
        /// e.g. `http://127.0.0.1:8080`.
        url: String,
    },
}

/// Everything the user can change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Where new downloads go.
    pub download_dir: PathBuf,
    /// Connections per download (1–32).
    pub max_connections: u8,
    /// Downloads running at once (1–10).
    pub max_parallel: u8,
    /// Custom User-Agent; `None` = the engine's.
    pub user_agent: Option<String>,
    /// Proxy policy.
    pub proxy: ProxySetting,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: PathBuf::new(),
            max_connections: 8,
            max_parallel: 3,
            user_agent: None,
            proxy: ProxySetting::System,
        }
    }
}

impl Settings {
    /// Clamp the numbers into range and refuse unusable values.
    pub fn validated(mut self) -> Result<Settings> {
        if self.download_dir.as_os_str().is_empty() {
            return Err(CoreError::Settings("the download folder is not set".into()));
        }
        self.max_connections = self.max_connections.clamp(1, MAX_CONNECTIONS);
        self.max_parallel = self.max_parallel.clamp(1, 10);
        if let ProxySetting::Manual { url } = &self.proxy {
            url::Url::parse(url).map_err(|e| CoreError::Settings(format!("proxy URL: {e}")))?;
        }
        Ok(self)
    }

    /// The engine configuration these settings describe.
    pub fn engine_config(&self) -> Result<EngineConfig> {
        let mut cfg = EngineConfig {
            max_connections: self.max_connections,
            ..EngineConfig::default()
        };
        if let Some(ua) = &self.user_agent {
            cfg.user_agent = ua.clone();
        }
        cfg.proxy = match &self.proxy {
            ProxySetting::System => Proxy::System,
            ProxySetting::None => Proxy::None,
            ProxySetting::Manual { url } => Proxy::Manual(
                url::Url::parse(url).map_err(|e| CoreError::Settings(format!("proxy URL: {e}")))?,
            ),
        };
        Ok(cfg)
    }
}
