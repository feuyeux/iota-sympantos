//! Engine pool for the daemon service.
//!
//! [`EnginePool`] maintains one [`IotaEngine`] per cwd so
//! ACP subprocess connections are reused across CLI invocations and backend handoff state is shared.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::config::NimiaConfig;
use crate::engine::IotaEngine;

/// Composite key used to bucket engines by working directory.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct EngineKey {
    pub cwd: PathBuf,
}

/// Holds one [`IotaEngine`] per working directory.
///
/// Wrapped in `Arc<Mutex<EnginePool>>` by the daemon server loop.
pub(crate) struct EnginePool {
    pub config: NimiaConfig,
    pub show_native: bool,
    pub timeout_ms: u64,
    pub engines: BTreeMap<EngineKey, Arc<Mutex<IotaEngine>>>,
}

impl EnginePool {
    pub fn new(config: NimiaConfig, show_native: bool, timeout_ms: u64) -> Self {
        Self {
            config,
            show_native,
            timeout_ms,
            engines: BTreeMap::new(),
        }
    }

    /// Return (or create) the engine for the given working directory.
    pub fn engine_for(&mut self, cwd: PathBuf) -> Arc<Mutex<IotaEngine>> {
        let key = EngineKey { cwd: cwd.clone() };
        let timeout_ms = self.timeout_ms;
        self.engines
            .entry(key)
            .or_insert_with(|| {
                Arc::new(Mutex::new(IotaEngine::create_session(
                    self.config.clone(),
                    self.show_native,
                    timeout_ms,
                    Some(&cwd),
                )))
            })
            .clone()
    }

    pub fn all_engines(&self) -> Vec<Arc<Mutex<IotaEngine>>> {
        self.engines.values().cloned().collect()
    }

    pub fn config(&self) -> NimiaConfig {
        self.config.clone()
    }

    pub fn replace_config(&mut self, config: NimiaConfig) {
        self.config = config;
        self.engines.clear();
    }
}
