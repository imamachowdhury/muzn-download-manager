//! The engine: one shared HTTP client and its settings.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::error::EngineError;

/// How outbound connections are routed.
#[derive(Clone, Debug)]
pub enum Proxy {
    /// Honour the OS / environment proxy settings (reqwest default).
    System,
    /// Connect directly, ignoring any proxy.
    None,
    /// Use this proxy for every request.
    Manual(url::Url),
}

/// Engine-wide settings.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// Connections per download (1–32).
    pub max_connections: u8,
    /// `User-Agent` sent on every request.
    pub user_agent: String,
    /// Proxy policy.
    pub proxy: Proxy,
    /// TCP connect timeout.
    pub connect_timeout: Duration,
    /// First retry delay; doubles per attempt up to `segment::RETRY_CAP`. Tests shorten it.
    pub retry_base_delay: Duration,
    /// No bytes for this long = the connection is dead; reconnect.
    pub stall_timeout: Duration,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            max_connections: 8,
            user_agent: format!("MuznDownloadManager/{}", crate::VERSION),
            proxy: Proxy::System,
            connect_timeout: Duration::from_secs(20),
            retry_base_delay: Duration::from_secs(1),
            stall_timeout: Duration::from_secs(30),
        }
    }
}

/// A configured engine. Cheap to clone; all clones share one connection pool.
#[derive(Clone)]
pub struct Engine {
    /// The HTTP client used by all download operations.
    pub(crate) client: reqwest::Client,
    /// The engine configuration.
    pub(crate) cfg: EngineConfig,
    /// Part file paths claimed by a currently-running download, shared by
    /// every clone. `Engine::start` uses this so two live downloads of the
    /// same name never write into one `.mdm.part`.
    pub(crate) live_parts: Arc<Mutex<HashSet<PathBuf>>>,
}

impl Engine {
    /// Build the shared client.
    pub fn new(cfg: EngineConfig) -> Result<Engine, EngineError> {
        Ok(Engine {
            client: build_client(&cfg)?,
            cfg,
            live_parts: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    /// A new engine with `cfg` that shares this engine's registry of live
    /// part files, so downloads started before a settings change keep their
    /// claims.
    pub fn reconfigured(&self, cfg: EngineConfig) -> Result<Engine, EngineError> {
        Ok(Engine {
            client: build_client(&cfg)?,
            cfg,
            live_parts: self.live_parts.clone(),
        })
    }

    /// The settings this engine was built with.
    pub fn config(&self) -> &EngineConfig {
        &self.cfg
    }
}

/// The one HTTP client recipe, shared by `new` and `reconfigured`.
fn build_client(cfg: &EngineConfig) -> Result<reqwest::Client, EngineError> {
    let mut b = reqwest::Client::builder()
        .user_agent(cfg.user_agent.clone())
        .connect_timeout(cfg.connect_timeout)
        .redirect(reqwest::redirect::Policy::limited(10))
        .no_gzip()
        .no_brotli()
        .no_deflate();
    b = match &cfg.proxy {
        Proxy::System => b,
        Proxy::None => b.no_proxy(),
        Proxy::Manual(u) => b.proxy(reqwest::Proxy::all(u.as_str())?),
    };
    Ok(b.build()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let c = EngineConfig::default();
        assert_eq!(c.max_connections, 8);
        assert!(c.user_agent.starts_with("MuznDownloadManager/"));
        assert!(matches!(c.proxy, Proxy::System));
    }

    #[test]
    fn engine_builds_with_manual_proxy() {
        let cfg = EngineConfig {
            proxy: Proxy::Manual(url::Url::parse("http://127.0.0.1:8080").unwrap()),
            ..Default::default()
        };
        assert!(Engine::new(cfg).is_ok());
    }
}
