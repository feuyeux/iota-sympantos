use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, btree_map::Entry};
use std::path::{Path, PathBuf};

use crate::acp::{self, AcpBackend, AcpClient, AcpClientStartOptions};
use crate::config::{
    EffectiveConfig, NimiaConfig, backend_process_env_with_context, config_path,
    normalized_acp_command,
};
use crate::context::{ContextEngine, WorkingMemoryBuffer};
use crate::daemon::{
    DesktopContextBudgetsSnapshot, DesktopContextSection, DesktopRuntimeContextSnapshot,
};
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
    /// Optional live text stream sender used by TUI/desktop turns.
    stream_output_sender: Option<tokio::sync::mpsc::Sender<String>>,
    /// Optional live runtime event sender used by desktop inspectors.
    stream_event_sender: Option<tokio::sync::mpsc::Sender<crate::runtime_event::RuntimeEvent>>,
    /// Last actual context capsule sent to a backend in this process. Not persisted.
    pub recent_runtime_context: Option<DesktopRuntimeContextSnapshot>,
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
        let mut engine = Self {
            effective_config,
            acp_clients: BTreeMap::new(),
            show_native_protocol: show_native,
            acp_timeout_ms: timeout_ms,
            context_engine,
            memory_store,
            cache_store,
            observability_store,
            working_memory: WorkingMemoryBuffer::new(20),
            engine_session_id: uuid::Uuid::new_v4().to_string(),
            session_ledger_store,
            last_used_backend: None,
            skill_registry_cache: SkillCache::default(),
            stream_output_sender: None,
            stream_event_sender: None,
            recent_runtime_context: None,
        };
        engine.resume_session_state(session_cwd);
        engine
    }

    /// Load the resumed session active backend and memory turns into the engine.
    pub fn resume_session_state(&mut self, session_cwd: Option<&Path>) {
        let mut is_resumed = false;
        let resumed_session_id = if let Some(cwd) = session_cwd {
            if let Some(ref ledger) = self.session_ledger_store {
                let res = ledger.latest_session_for_cwd(cwd).ok().flatten();
                if res.is_some() {
                    is_resumed = true;
                }
                res
            } else {
                None
            }
        } else {
            None
        };

        if let Some(session_id) = resumed_session_id {
            self.engine_session_id = session_id;
        }

        if is_resumed {
            if let Some(ref ledger) = self.session_ledger_store {
                if let Ok(Some(summary)) = ledger.summary(&self.engine_session_id) {
                    if let Some(backend_str) = summary.active_backend {
                        if let Ok(backend) = AcpBackend::parse(&backend_str) {
                            self.last_used_backend = Some(backend);
                        }
                    }
                }
            }
            if let Some(ref mem_store) = self.memory_store {
                if let Ok(turns) = mem_store.get_session_turns(&self.engine_session_id) {
                    self.working_memory = WorkingMemoryBuffer::new(20);
                    for (backend_str, content) in turns {
                        if let Ok(backend) = AcpBackend::parse(&backend_str) {
                            if let Some(rest) = content.strip_prefix("Prompt: ") {
                                if let Some(idx) = rest.find("\nOutput: ") {
                                    let prompt_summary = rest[..idx].to_string();
                                    let output_summary = rest[idx + 9..].to_string();
                                    self.working_memory.push_turn_from_resume(
                                        backend,
                                        prompt_summary,
                                        output_summary,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Attach or clear the streaming output channel for all currently open ACP clients.
    ///
    /// The TUI installs a sender before a turn starts, receives incremental `session/update`
    /// text chunks through it, and clears the sender after the turn to stop forwarding output.
    pub fn set_stream_output_sender(&mut self, tx: Option<tokio::sync::mpsc::Sender<String>>) {
        self.stream_output_sender = tx.clone();
        for client in self.acp_clients.values_mut() {
            client.set_stream_sender(tx.clone());
        }
    }

    /// Attach or clear a live runtime event channel for all currently open and future ACP clients.
    pub fn set_stream_event_sender(
        &mut self,
        tx: Option<tokio::sync::mpsc::Sender<crate::runtime_event::RuntimeEvent>>,
    ) {
        self.stream_event_sender = tx.clone();
        for client in self.acp_clients.values_mut() {
            client.set_event_sender(tx.clone());
        }
    }

    /// Get reference to the underlying memory store when available.
    pub fn memory_store(&self) -> Option<&MemoryStore> {
        self.memory_store.as_ref()
    }

    /// Get reference to the effective configuration.
    pub fn effective_config(&self) -> &EffectiveConfig {
        &self.effective_config
    }

    /// Get the current engine session ID.
    pub fn engine_session_id(&self) -> &str {
        &self.engine_session_id
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

    /// Update the engine configuration and shut down all currently open ACP clients.
    pub async fn update_config(&mut self, config: crate::config::NimiaConfig) {
        self.effective_config = crate::config::EffectiveConfig::from_config(&config);
        self.context_engine = crate::context::ContextEngine::from_config(Some(
            self.effective_config.context_engine(),
        ));
        let embedding_cfg = self.effective_config.embedding_config();
        if let Some(path) = self.effective_config.memory_db_path() {
            self.memory_store =
                crate::memory::MemoryStore::open_with_embedding(path, embedding_cfg).ok();
        } else {
            self.memory_store = None;
        }
        self.shutdown_open_clients().await;
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
        let mut client = self.start_acp_client(backend, cwd.clone()).await?;
        client.set_stream_sender(self.stream_output_sender.clone());
        client.set_event_sender(self.stream_event_sender.clone());
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

    pub fn recent_runtime_context_snapshot(&self) -> Option<DesktopRuntimeContextSnapshot> {
        self.recent_runtime_context.clone()
    }

    pub fn context_engine_budgets(&self) -> crate::context::ContextBudgets {
        self.context_engine.budgets()
    }

    pub fn context_engine_enabled(&self) -> bool {
        self.context_engine.enabled
    }

    pub fn capture_runtime_context_snapshot(
        &mut self,
        turn_id: String,
        backend: AcpBackend,
        cwd: PathBuf,
        model: Option<String>,
        capsule_text: String,
    ) {
        self.recent_runtime_context = Some(DesktopRuntimeContextSnapshot {
            turn_id,
            backend: backend.to_string(),
            cwd,
            session_id: self.engine_session_id.clone(),
            model,
            created_at: crate::utils::now_ts(),
            sections: parse_context_sections(&capsule_text),
            capsule_text,
            budgets: DesktopContextBudgetsSnapshot::from(self.context_engine.budgets()),
        });
    }
}

impl From<crate::context::ContextBudgets> for DesktopContextBudgetsSnapshot {
    fn from(value: crate::context::ContextBudgets) -> Self {
        Self {
            memory_chars: value.memory_chars,
            skills_chars: value.skills_chars,
            working_memory_chars: value.working_memory_chars,
            workspace_chars: value.workspace_chars,
            handoff_chars: value.handoff_chars,
        }
    }
}

fn parse_context_sections(capsule: &str) -> Vec<DesktopContextSection> {
    let Some(start) = capsule.find("<iota-context>") else {
        return Vec::new();
    };
    let Some(end) = capsule.find("</iota-context>") else {
        return Vec::new();
    };
    let body = &capsule[start..end + "</iota-context>".len()];
    let names = [
        "memory-tools",
        "model",
        "skills",
        "session",
        "handoff",
        "working-memory",
        "workspace",
    ];
    let mut sections = names
        .iter()
        .filter_map(|name| {
            let open = format!("<{}>", name);
            let close = format!("</{}>", name);
            let section_start = body.find(&open)? + open.len();
            let section_end = body[section_start..].find(&close)? + section_start;
            let text = body[section_start..section_end].trim();
            Some(DesktopContextSection {
                name: (*name).to_string(),
                chars: text.len(),
                preview: crate::utils::summarize(text, 180),
            })
        })
        .collect::<Vec<_>>();
    if let Some(memory_section) = parse_memory_context_section(body) {
        sections.push(memory_section);
    }
    sections
}

fn parse_memory_context_section(body: &str) -> Option<DesktopContextSection> {
    let mut rest = body;
    let mut content = Vec::new();
    while let Some(open_start) = rest.find("<memory") {
        let after_open_start = &rest[open_start..];
        let Some(open_end) = after_open_start.find('>') else {
            break;
        };
        let tag_suffix = &after_open_start["<memory".len()..open_end];
        if !tag_suffix.is_empty() && !tag_suffix.starts_with(char::is_whitespace) {
            rest = &after_open_start[open_end + 1..];
            continue;
        }
        let content_start = open_start + open_end + 1;
        let Some(close_start_relative) = rest[content_start..].find("</memory>") else {
            break;
        };
        let close_start = content_start + close_start_relative;
        let text = rest[content_start..close_start].trim();
        if !text.is_empty() {
            content.push(text.to_string());
        }
        let after_close = close_start + "</memory>".len();
        rest = &rest[after_close..];
    }
    if content.is_empty() {
        return None;
    }
    let joined = content.join("\n\n");
    Some(DesktopContextSection {
        name: "memory".to_string(),
        chars: joined.len(),
        preview: crate::utils::summarize(&joined, 180),
    })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
