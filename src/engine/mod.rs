use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, btree_map::Entry};
use std::path::{Path, PathBuf};

use crate::acp::{self, AcpBackend, AcpClient, AcpClientStartOptions};
use crate::config::{
    EffectiveConfig, NimiaConfig, backend_process_env_with_context, config_path,
    normalized_acp_command,
};
use crate::context::{ContextEngine, WorkingMemoryBuffer};
use crate::memory::MemoryStore;
use crate::skill::SkillCache;
use crate::store::cache::CacheStore;
use crate::store::ledger::SessionLedger;
use crate::store::observability::ObservabilityStore;

mod memory_ops;
mod prompt;
mod session_ledger;
mod telemetry;

#[cfg(test)]
use crate::memory::RecallBuckets;
#[cfg(test)]
use memory_ops::memory_inject_payload;

/// Unique key for one reusable ACP process.
///
/// A backend can be used from multiple directories, and each `(backend, cwd)` pair needs
/// its own ACP session because backend-side context, permissions, and MCP state are cwd-scoped.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct AcpClientKey {
    backend: AcpBackend,
    cwd: PathBuf,
}

pub struct IotaEngine {
    /// Fully resolved runtime configuration derived from `~/.i6/nimia.yaml`.
    effective_config: EffectiveConfig,
    /// Reusable ACP client processes, keyed by backend and working directory.
    acp_clients: BTreeMap<AcpClientKey, AcpClient>,
    /// Whether raw ACP protocol messages should be surfaced for local debugging.
    show_native_protocol: bool,
    /// Timeout applied to ACP line reads, process initialization, and cache-join waits.
    acp_timeout_ms: u64,
    /// Builds the final prompt capsule with memory, skills, working memory, git context, and handoff.
    context_engine: ContextEngine,
    /// Optional persistent memory database. `None` keeps the engine usable without memory.
    memory_store: Option<MemoryStore>,
    /// Optional execution store used for turn lifecycle persistence.
    cache_store: Option<CacheStore>,
    /// Optional observability store used for runtime token usage events.
    observability_store: Option<ObservabilityStore>,
    /// Recent prompt/output turns used to compose context and backend handoff summaries.
    working_memory: WorkingMemoryBuffer,
    /// Stable logical session id shared across backend turns in this cwd.
    engine_session_id: String,
    /// Optional ledger for durable session, turn, and backend handoff records.
    session_ledger_store: Option<SessionLedger>,
    /// Last backend that completed a turn. Used to decide whether a handoff summary is needed.
    last_used_backend: Option<AcpBackend>,
    /// In-memory cache for loaded skill manifests to avoid repeated filesystem scans.
    skill_registry_cache: SkillCache,
}

impl IotaEngine {
    /// Build an engine bound to the process current directory when a ledger session exists.
    pub fn new(config: NimiaConfig, show_native: bool, timeout_ms: u64) -> Self {
        let session_cwd = std::env::current_dir().ok();
        Self::create_session(config, show_native, timeout_ms, session_cwd.as_deref())
    }

    /// Build an engine and optionally reuse the latest ledger session for `session_cwd`.
    pub fn create_session(
        config: NimiaConfig,
        show_native: bool,
        timeout_ms: u64,
        session_cwd: Option<&std::path::Path>,
    ) -> Self {
        let effective_config = EffectiveConfig::from_config(&config);
        let context_engine = ContextEngine::from_config(Some(effective_config.context_engine()));
        let embedding_cfg = effective_config.embedding_config();
        let memory_store = effective_config
            .memory_db_path()
            .and_then(|path| MemoryStore::open_with_embedding(path, embedding_cfg).ok());
        let cache_store = CacheStore::default_path()
            .ok()
            .and_then(|path| CacheStore::open(&path).ok());
        let observability_store = ObservabilityStore::default_path()
            .ok()
            .and_then(|path| ObservabilityStore::open(&path).ok());
        let session_ledger_store = SessionLedger::default_path()
            .ok()
            .and_then(|path| SessionLedger::open(&path).ok());
        // Reuse the latest session for this cwd when available so daemon/TUI restarts preserve
        // continuity; otherwise create a fresh session id.
        let engine_session_id = session_cwd
            .and_then(|cwd| {
                session_ledger_store
                    .as_ref()
                    .and_then(|ledger| ledger.latest_session_for_cwd(cwd).ok().flatten())
            })
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        Self {
            effective_config,
            acp_clients: BTreeMap::new(),
            show_native_protocol: show_native,
            acp_timeout_ms: timeout_ms,
            context_engine,
            memory_store,
            cache_store,
            observability_store,
            working_memory: WorkingMemoryBuffer::new(20),
            engine_session_id,
            session_ledger_store,
            last_used_backend: None,
            skill_registry_cache: SkillCache::default(),
        }
    }

    /// Attach or clear the streaming output channel for all currently open ACP clients.
    ///
    /// The TUI installs a sender before a turn starts, receives incremental `session/update`
    /// text chunks through it, and clears the sender after the turn to stop forwarding output.
    pub fn set_stream_output_sender(&mut self, tx: Option<tokio::sync::mpsc::Sender<String>>) {
        for client in self.acp_clients.values_mut() {
            client.set_stream_sender(tx.clone());
        }
    }

    #[allow(dead_code)]
    pub async fn shutdown_open_clients_in_place(&mut self) {
        self.shutdown_open_clients().await;
    }

    /// Start ACP clients for every enabled backend in `cwd` and keep them in the client pool.
    pub async fn warm_all_enabled_backends(&mut self, cwd: PathBuf) -> Result<usize> {
        let mut handles = Vec::new();
        for backend in acp::ALL_BACKENDS {
            // Do not start duplicate child processes for a backend/cwd pair already in the pool.
            let key = AcpClientKey {
                backend,
                cwd: cwd.clone(),
            };
            if self.acp_clients.contains_key(&key) {
                continue;
            }
            // Skip disabled or incomplete backend sections so one bad backend does not prevent
            // warming the others.
            let Some(section) = self.effective_config.backend_config(backend) else {
                continue;
            };
            if !section.enabled {
                continue;
            }
            let Some(acp_config) = section.acp.as_ref() else {
                eprintln!("Skipping {}: missing acp config", backend);
                continue;
            };
            if acp_config.command.trim().is_empty() {
                eprintln!("Skipping {}: missing acp.command", backend);
                continue;
            }

            let backend_context = self.effective_config.backend_context_config(backend);
            let env = backend_process_env_with_context(backend, section, backend_context);
            let command = normalized_acp_command(backend, section, acp_config);
            let mcp_servers = self.effective_config.context_mcp_servers(backend);
            let session_options = self.effective_config.context_session_options(backend);
            let tool_whitelist = self.effective_config.context_tool_whitelist(backend);
            let cwd = cwd.clone();
            let show_native_protocol = self.show_native_protocol;
            let acp_timeout_ms = self.acp_timeout_ms;
            handles.push(tokio::spawn(async move {
                // Start each backend concurrently; benchmark warmup should be bounded by the
                // slowest backend, not by the sum of all startup times.
                match AcpClient::start(AcpClientStartOptions {
                    backend,
                    cwd: cwd.clone(),
                    env,
                    command_override: Some(command),
                    mcp_servers,
                    session_options,
                    tool_whitelist,
                    show_native: show_native_protocol,
                    timeout_ms: acp_timeout_ms,
                })
                .await
                {
                    Ok(client) => Some((AcpClientKey { backend, cwd }, client)),
                    Err(err) => {
                        eprintln!("Failed to warm {}: {}", backend, err);
                        None
                    }
                }
            }));
        }

        for handle in handles {
            // Failed warmups are logged inside the task and omitted from the pool.
            if let Ok(Some((key, client))) = handle.await {
                self.acp_clients.insert(key, client);
            }
        }
        self.record_active_sessions();
        Ok(self.acp_clients.len())
    }

    /// Return whether this engine already has a warm ACP client for `(backend, cwd)`.
    pub fn has_warm_client(&self, backend: AcpBackend, cwd: &Path) -> bool {
        self.acp_clients.contains_key(&AcpClientKey {
            backend,
            cwd: cwd.to_path_buf(),
        })
    }

    /// Start one backend client for `cwd` if it is not already present.
    ///
    /// Returns `true` when a new process was started and `false` when an existing client was reused.
    pub async fn warm_backend(&mut self, backend: AcpBackend, cwd: PathBuf) -> Result<bool> {
        self.ensure_acp_client(backend, cwd).await
    }

    /// Consume the engine and shut down every open ACP child process.
    pub async fn shutdown(mut self) {
        while let Some((_, client)) = self.acp_clients.pop_first() {
            client.shutdown().await;
        }
        self.record_active_sessions();
    }

    /// Number of currently open ACP client processes.
    pub fn open_client_count(&self) -> usize {
        self.acp_clients.len()
    }

    /// Update the ACP timeout for this engine and all already running clients.
    pub fn set_acp_timeout_ms(&mut self, timeout_ms: u64) {
        self.acp_timeout_ms = timeout_ms;
        for client in self.acp_clients.values_mut() {
            client.set_timeout_ms(timeout_ms);
        }
    }

    /// Shut down all open ACP clients while keeping the engine object reusable.
    pub async fn shutdown_open_clients(&mut self) {
        while let Some((_, client)) = self.acp_clients.pop_first() {
            client.shutdown().await;
        }
        self.record_active_sessions();
    }

    /// Ensure the ACP client pool contains a process for `(backend, cwd)`.
    async fn ensure_acp_client(&mut self, backend: AcpBackend, cwd: PathBuf) -> Result<bool> {
        let key = AcpClientKey {
            backend,
            cwd: cwd.clone(),
        };
        if self.acp_clients.contains_key(&key) {
            return Ok(false);
        }
        let client = self.start_acp_client(backend, cwd.clone()).await?;
        match self.acp_clients.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(client);
                self.record_active_sessions();
            }
            Entry::Occupied(_) => {}
        }
        Ok(true)
    }

    /// Validate backend config and launch one ACP child process.
    async fn start_acp_client(&self, backend: AcpBackend, cwd: PathBuf) -> Result<AcpClient> {
        let path = config_path()?;
        let section = self
            .effective_config
            .backend_config(backend)
            .with_context(|| {
                format!(
                    "Missing backend section for {} in {}",
                    backend,
                    path.display()
                )
            })?;
        if !section.enabled {
            bail!("Backend {} is disabled in {}", backend, path.display());
        }
        let acp_config = section.acp.as_ref().with_context(|| {
            format!(
                "Missing acp config for backend {} in {}",
                backend,
                path.display()
            )
        })?;
        if acp_config.command.trim().is_empty() {
            bail!(
                "Missing acp.command for backend {} in {}",
                backend,
                path.display()
            );
        }

        AcpClient::start(AcpClientStartOptions {
            backend,
            cwd,
            env: backend_process_env_with_context(
                backend,
                section,
                self.effective_config.backend_context_config(backend),
            ),
            command_override: Some(normalized_acp_command(backend, section, acp_config)),
            mcp_servers: self.effective_config.context_mcp_servers(backend),
            session_options: self.effective_config.context_session_options(backend),
            tool_whitelist: self.effective_config.context_tool_whitelist(backend),
            show_native: self.show_native_protocol,
            timeout_ms: self.acp_timeout_ms,
        })
        .await
    }

    fn record_active_sessions(&self) {
        // Keep this as a single hook for future OTel UpDownCounter session tracking.
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
