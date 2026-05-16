use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, btree_map::Entry};
use std::path::PathBuf;

use crate::acp::{self, AcpBackend, AcpClient, AcpPromptOutput};
use crate::config::{
    EffectiveConfig, NimiaConfig, backend_process_env_with_context, config_path, configured_model,
    normalized_acp_command,
};
use crate::context::{ComposeInput, ContextEngine, WorkingMemoryBuffer};
use crate::runtime_event::{ErrorEvent, MemoryEvent, OutputEvent, RuntimeEvent, StateEvent};
use crate::skill::{SkillCache, SkillRegistry};
use crate::store::cache::{CacheStore, ExecutionStatus, request_hash};
use crate::store::ledger::SessionLedger;
use crate::store::memory::{
    MemoryFacet, MemoryInsert, MemoryScope, MemoryStore, MemoryType, RecallBuckets,
    RecallThresholds,
};
use crate::telemetry::metrics;
use crate::utils::summarize;

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
    /// Optional execution cache used for idempotency, replay, and joining in-flight prompts.
    cache_store: Option<CacheStore>,
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
        Self::new_for_session_cwd(config, show_native, timeout_ms, session_cwd.as_deref())
    }

    /// Build an engine and optionally reuse the latest ledger session for `session_cwd`.
    pub fn new_for_session_cwd(
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
            working_memory: WorkingMemoryBuffer::new(50),
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

    /// Wait for another process handling the same request hash, then reuse its output.
    ///
    /// This prevents duplicate backend work when TUI/daemon/CLI submit identical prompts at the
    /// same time. The wait uses exponential backoff and is capped by `acp_timeout_ms`.
    async fn wait_for_matching_running_execution(
        &self,
        backend: AcpBackend,
        request_hash: &str,
    ) -> Option<String> {
        let store = self.cache_store.as_ref()?.clone();
        let running = store
            .find_running_by_request_hash(&backend.to_string(), request_hash)
            .ok()??;
        let deadline =
            tokio::time::Instant::now() + tokio::time::Duration::from_millis(self.acp_timeout_ms);
        let mut poll_interval_ms: u64 = 50;
        loop {
            // A completed peer execution can be replayed as if this engine produced it.
            if let Ok(Some(record)) = store.get_execution(&running.execution_id) {
                if record.status == ExecutionStatus::Completed {
                    return store.output_text(&running.execution_id).ok().flatten();
                }
                // Failed/cancelled/stale executions are not safe to reuse.
                if record.status != ExecutionStatus::Running {
                    return None;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            // Start responsive, then back off to avoid hot-polling SQLite.
            tokio::time::sleep(tokio::time::Duration::from_millis(poll_interval_ms)).await;
            poll_interval_ms = (poll_interval_ms * 2).min(500);
        }
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
                match AcpClient::start(
                    backend,
                    cwd.clone(),
                    env,
                    Some(command),
                    mcp_servers,
                    session_options,
                    tool_whitelist,
                    show_native_protocol,
                    acp_timeout_ms,
                )
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

    /// Run a prompt and return only the final assistant text.
    pub async fn run_prompt_text(
        &mut self,
        backend: AcpBackend,
        cwd: PathBuf,
        prompt: &str,
    ) -> Result<String> {
        Ok(self
            .run_prompt_with_timing(backend, cwd, prompt)
            .await?
            .text)
    }

    /// Run a prompt and return text, runtime events, backend session id, and timing data.
    pub async fn run_prompt_with_timing(
        &mut self,
        backend: AcpBackend,
        cwd: PathBuf,
        prompt: &str,
    ) -> Result<AcpPromptOutput> {
        self.run_prompt_with_optional_execution_id(backend, cwd, prompt, None)
            .await
    }

    /// Run a prompt with an optional externally supplied execution id.
    ///
    /// The daemon uses `requested_execution_id` so callers can correlate persisted cache/events
    /// with their own request id. When it is `None`, the cache layer allocates the id.
    pub async fn run_prompt_with_optional_execution_id(
        &mut self,
        backend: AcpBackend,
        cwd: PathBuf,
        prompt: &str,
        requested_execution_id: Option<&str>,
    ) -> Result<AcpPromptOutput> {
        let request_hash = request_hash(&backend.to_string(), &cwd, prompt);
        tracing::debug!(backend = %backend, cwd = %cwd.display(), request_hash = %request_hash, "prompt requested");
        // Load skills before cache lookup because engine-run skills and memory writes have
        // side effects and must not be replayed from previous text output.
        let skills = SkillRegistry::load_cached(
            &cwd,
            self.effective_config.skill_roots(),
            &mut self.skill_registry_cache,
        );
        let matched_skill = skills.match_skill(backend, prompt);
        // Prompts with side effects or deterministic memory answers must execute fresh.
        let skip_replay = matched_skill.is_some()
            || is_memory_query(prompt)
            || !classify_memory_prompt(prompt).is_empty()
            || prompt.contains("iota_memory_write");
        if !skip_replay
            && let Some(output) = self.replay_completed_execution(backend, &request_hash)
        {
            self.record_cache_hit_metric();
            tracing::info!(backend = %backend, request_hash = %request_hash, "replaying completed execution");
            self.working_memory.push_turn(backend, prompt, &output);
            return Ok(AcpPromptOutput::synthetic(output));
        }
        if !skip_replay
            && let Some(output) = self
                .wait_for_matching_running_execution(backend, &request_hash)
                .await
        {
            self.record_cache_hit_metric();
            tracing::info!(backend = %backend, request_hash = %request_hash, "joined running execution");
            self.working_memory.push_turn(backend, prompt, &output);
            return Ok(AcpPromptOutput::synthetic(output));
        }
        let model = self
            .effective_config
            .backend_config(backend)
            .and_then(configured_model);
        // The ledger records the logical session first, then later records turns and backend ids.
        self.ensure_ledger_session(backend, &cwd, model.as_deref());
        // When switching from one backend to another, inject recent dialogue as handoff text.
        let handoff = self.prepare_backend_handoff(backend, &cwd);
        let execution_id = match self.cache_store.as_ref() {
            Some(store) => {
                // The cache store provides idempotency fencing for this request hash.
                match store.begin_execution_with_id(
                    &backend.to_string(),
                    &self.engine_session_id,
                    &request_hash,
                    requested_execution_id,
                ) {
                    Ok(execution_id) => Some(execution_id),
                    Err(_) => {
                        if !skip_replay
                            && let Some(output) = self
                                .wait_for_matching_running_execution(backend, &request_hash)
                                .await
                        {
                            self.record_cache_hit_metric();
                            self.working_memory.push_turn(backend, prompt, &output);
                            return Ok(AcpPromptOutput::synthetic(output));
                        }
                        None
                    }
                }
            }
            None => None,
        };
        self.record_cache_miss_metric();
        if let Some(ref eid) = execution_id {
            tracing::info!(execution_id = %eid, backend = %backend, session_id = %self.engine_session_id, "execution.started");
        }
        tracing::debug!(backend = %backend, execution_id = execution_id.as_deref(), "execution started");
        self.record_runtime_event(
            &execution_id,
            RuntimeEvent::State(StateEvent {
                state: "started".to_string(),
                detail: None,
            }),
        );

        // Keyword memory extraction happens before backend execution; write-only prompts can
        // complete locally without spending an ACP turn.
        let extracted_memories = if is_memory_query(prompt) || prompt.contains("iota_memory_write")
        {
            Vec::new()
        } else {
            self.extract_keyword_memories(backend, &cwd, prompt, execution_id.as_deref())
        };
        if !extracted_memories.is_empty() && is_memory_write_only_prompt(prompt) {
            let mut events = Vec::new();
            for memory_id in &extracted_memories {
                let event = RuntimeEvent::Memory(MemoryEvent {
                    action: "write".to_string(),
                    memory_id: Some(memory_id.clone()),
                    payload: serde_json::json!({"source":"engine-extract"}),
                });
                self.record_runtime_event(&execution_id, event.clone());
                events.push(event);
            }
            let text = format!("已记录 {} 条记忆。", extracted_memories.len());
            let output_event = RuntimeEvent::Output(OutputEvent {
                text: text.clone(),
                role: Some("engine".to_string()),
            });
            self.record_runtime_event(&execution_id, output_event.clone());
            events.push(output_event);
            self.mark_execution_finished(&execution_id, ExecutionStatus::Completed);
            self.record_ledger_turn(
                backend,
                execution_id.as_deref(),
                &request_hash,
                &text,
                ExecutionStatus::Completed.as_str(),
            );
            self.last_used_backend = Some(backend);
            self.working_memory.push_turn(backend, prompt, &text);
            let mut output = AcpPromptOutput::synthetic(text);
            output.execution_id = execution_id;
            output.events = events;
            return Ok(output);
        }

        if let Some(skill) = matched_skill {
            // Engine-run skills are local deterministic handlers. When they match, they replace
            // the external ACP backend call.
            if let Some(skill_output) =
                crate::skill::runner::run_engine_skill(skill, prompt).await?
            {
                let mut events = Vec::new();
                for event in skill_output.events {
                    self.record_runtime_event(&execution_id, event.clone());
                    events.push(event);
                }
                let output_event = RuntimeEvent::Output(OutputEvent {
                    text: skill_output.text.clone(),
                    role: Some("engine".to_string()),
                });
                self.record_runtime_event(&execution_id, output_event.clone());
                events.push(output_event);
                self.mark_execution_finished(&execution_id, ExecutionStatus::Completed);
                self.record_ledger_turn(
                    backend,
                    execution_id.as_deref(),
                    &request_hash,
                    &skill_output.text,
                    ExecutionStatus::Completed.as_str(),
                );
                self.last_used_backend = Some(backend);
                self.working_memory
                    .push_turn(backend, prompt, &skill_output.text);
                self.persist_turn_as_episodic_memory(
                    backend,
                    prompt,
                    &skill_output.text,
                    execution_id.as_deref(),
                );
                let mut output = AcpPromptOutput::synthetic(skill_output.text);
                output.execution_id = execution_id;
                output.events = events;
                return Ok(output);
            }
        }

        let memory = self.memory_store.as_ref().and_then(|store| {
            // Recall all configured memory buckets for the user/project/session capsule.
            let thresholds_cfg = self.effective_config.recall_thresholds();
            let thresholds = RecallThresholds {
                identity: thresholds_cfg.identity,
                preference: thresholds_cfg.preference,
                strategic: thresholds_cfg.strategic,
                domain: thresholds_cfg.domain,
                procedural: thresholds_cfg.procedural,
                episodic: thresholds_cfg.episodic,
            };
            let project_id = cwd.display().to_string();
            self.log_engine_event(
                execution_id.as_deref(),
                backend,
                "info",
                "memory.recall.started",
                serde_json::json!({
                    "user_id": "local-user",
                    "project_id": project_id.clone(),
                }),
            );
            tracing::info!(
                backend = %backend,
                execution_id = execution_id.as_deref().unwrap_or("-"),
                session_id = %self.engine_session_id,
                user_id = "local-user",
                project_id = %project_id,
                "engine memory recall started"
            );
            match store.recall_buckets_with_thresholds(
                "local-user",
                &project_id,
                &self.engine_session_id,
                thresholds,
            ) {
                Ok(buckets) => {
                    self.log_engine_event(
                        execution_id.as_deref(),
                        backend,
                        "info",
                        "memory.recall.completed",
                        serde_json::json!({
                            "identity_count": buckets.identity.len(),
                            "preference_count": buckets.preference.len(),
                            "strategic_count": buckets.strategic.len(),
                            "domain_count": buckets.domain.len(),
                            "procedural_count": buckets.procedural.len(),
                            "episodic_count": buckets.episodic.len(),
                        }),
                    );
                    tracing::info!(
                        backend = %backend,
                        execution_id = execution_id.as_deref().unwrap_or("-"),
                        session_id = %self.engine_session_id,
                        identity_count = buckets.identity.len(),
                        preference_count = buckets.preference.len(),
                        strategic_count = buckets.strategic.len(),
                        domain_count = buckets.domain.len(),
                        procedural_count = buckets.procedural.len(),
                        episodic_count = buckets.episodic.len(),
                        "engine memory recall completed"
                    );
                    Some(buckets)
                }
                Err(err) => {
                    self.log_engine_event(
                        execution_id.as_deref(),
                        backend,
                        "warn",
                        "memory.recall.failed",
                        serde_json::json!({"error": err.to_string()}),
                    );
                    tracing::warn!(
                        backend = %backend,
                        execution_id = execution_id.as_deref().unwrap_or("-"),
                        session_id = %self.engine_session_id,
                        error = %err,
                        "engine memory recall failed"
                    );
                    None
                }
            }
        });
        let memory_event = memory.as_ref().map(|buckets| {
            // Keep a structured event showing which memories were injected into the prompt.
            RuntimeEvent::Memory(MemoryEvent {
                action: "inject".to_string(),
                memory_id: None,
                payload: memory_inject_payload(buckets, self.context_engine.budgets().memory_chars),
            })
        });
        if let Some(event) = memory_event.clone() {
            self.log_engine_event(
                execution_id.as_deref(),
                backend,
                "info",
                "memory.inject",
                event_payload(&event),
            );
            tracing::info!(
                backend = %backend,
                execution_id = execution_id.as_deref().unwrap_or("-"),
                session_id = %self.engine_session_id,
                payload = %event_payload(&event),
                "engine memory inject event recorded"
            );
            self.record_runtime_event(&execution_id, event);
        }
        if let Some((buckets, text)) = memory.as_ref().and_then(|buckets| {
            deterministic_memory_answer(prompt, buckets).map(|text| (buckets, text))
        }) {
            // Memory queries can be answered from recall buckets without calling a backend.
            let mut events = Vec::new();
            if let Some(event) = memory_event.clone() {
                events.push(event);
            }
            let output_event = RuntimeEvent::Output(OutputEvent {
                text: text.clone(),
                role: Some("engine".to_string()),
            });
            self.record_runtime_event(&execution_id, output_event.clone());
            events.push(output_event);
            self.mark_execution_finished(&execution_id, ExecutionStatus::Completed);
            self.record_ledger_turn(
                backend,
                execution_id.as_deref(),
                &request_hash,
                &text,
                ExecutionStatus::Completed.as_str(),
            );
            self.last_used_backend = Some(backend);
            self.working_memory.push_turn(backend, prompt, &text);
            let mut output = AcpPromptOutput::synthetic(text);
            output.execution_id = execution_id;
            output.events = events;
            let _ = buckets;
            return Ok(output);
        }
        // compose_effective_prompt runs `git status` which is a blocking syscall.
        // Off-load it to the blocking thread pool to avoid stalling the tokio worker.
        let context_engine = self.context_engine.clone();
        let session_id_c = self.engine_session_id.clone();
        let model_c = model.clone();
        let skills_c = skills.clone();
        let working_memory_c = self.working_memory.clone();
        let handoff_c = handoff.clone();
        let prompt_c = prompt.to_string();
        let cwd_c = cwd.clone();
        let effective_prompt = tokio::task::spawn_blocking(move || {
            // ComposeInput borrows cloned values inside the blocking closure so git/status work
            // does not stall the async runtime worker.
            context_engine.compose_effective_prompt(ComposeInput {
                backend,
                cwd: &cwd_c,
                session_id: &session_id_c,
                model: model_c.as_deref(),
                prompt: &prompt_c,
                memory: memory.as_ref(),
                skills: Some(&skills_c),
                working_memory: &working_memory_c,
                handoff: handoff_c.as_deref(),
            })
        })
        .await
        .context("context composition task panicked")?;

        let client_started = self.ensure_acp_client(backend, cwd.clone()).await?;
        let key = AcpClientKey {
            backend,
            cwd: cwd.clone(),
        };
        let client = self
            .acp_clients
            .get_mut(&key)
            .context("ACP client missing after warm")?;
        let startup_timing = client.startup_timing();
        match client
            .prompt_with_cwd_timed_for_execution(&cwd, &effective_prompt, execution_id.as_deref())
            .await
        {
            Ok(mut output) => {
                // Normalize ACP output with engine-owned metadata before persisting events.
                output.execution_id = execution_id.clone();
                if let Some(event) = memory_event.clone() {
                    output.events.insert(0, event);
                }
                output.timing.client_started = client_started;
                output.timing.process_spawned = client_started;
                if client_started {
                    output.timing.process_spawn_ms = Some(startup_timing.process_spawn_ms);
                    output.timing.init_ms = Some(startup_timing.init_ms);
                }
                let has_output_event = output
                    .events
                    .iter()
                    .any(|event| matches!(event, RuntimeEvent::Output(_)));
                for event in output.events.iter().cloned() {
                    self.record_runtime_event(&execution_id, event);
                }
                if !has_output_event {
                    // Some backends only return final text. Synthesize an Output event so cache
                    // replay and event consumers see a consistent shape.
                    let output_text = &output.text;
                    tracing::info!(
                        execution_id = execution_id.as_deref(),
                        output_len = output_text.len(),
                        "output.final"
                    );
                    self.record_runtime_event(
                        &execution_id,
                        RuntimeEvent::Output(OutputEvent {
                            text: output_text.clone(),
                            role: Some("assistant".to_string()),
                        }),
                    );
                }
                if let Some(session_id) = output.backend_session_id.as_deref() {
                    // Preserve backend-native session ids for future backend-specific continuity.
                    self.persist_backend_session_id(backend, &cwd, session_id);
                }
                self.mark_execution_finished_with_timing(
                    &execution_id,
                    backend,
                    ExecutionStatus::Completed,
                    &output.timing,
                );
                tracing::info!(backend = %backend, execution_id = execution_id.as_deref(), total_ms = output.timing.total_ms, prompt_ms = output.timing.prompt_ms, "execution completed");
                self.record_ledger_turn(
                    backend,
                    execution_id.as_deref(),
                    &request_hash,
                    &output.text,
                    ExecutionStatus::Completed.as_str(),
                );
                self.last_used_backend = Some(backend);
                self.working_memory.push_turn(backend, prompt, &output.text);
                if !is_explicit_memory_tool_prompt(prompt) {
                    self.persist_turn_as_episodic_memory(
                        backend,
                        prompt,
                        &output.text,
                        execution_id.as_deref(),
                    );
                }
                Ok(output)
            }
            Err(err) => {
                self.record_runtime_event(
                    &execution_id,
                    RuntimeEvent::Error(ErrorEvent {
                        message: err.to_string(),
                        code: None,
                        data: None,
                    }),
                );
                self.mark_execution_finished(&execution_id, ExecutionStatus::Failed);
                tracing::warn!(backend = %backend, execution_id = execution_id.as_deref(), error = %err, "execution failed");
                self.record_ledger_turn(
                    backend,
                    execution_id.as_deref(),
                    &request_hash,
                    &err.to_string(),
                    ExecutionStatus::Failed.as_str(),
                );
                Err(err)
            }
        }
    }

    /// Return whether this engine already has a warm ACP client for `(backend, cwd)`.
    pub fn has_warm_client(&self, backend: AcpBackend, cwd: &PathBuf) -> bool {
        self.acp_clients.contains_key(&AcpClientKey {
            backend,
            cwd: cwd.clone(),
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

    /// Return cached output for a previously completed identical request.
    fn replay_completed_execution(
        &self,
        backend: AcpBackend,
        request_hash: &str,
    ) -> Option<String> {
        let store = self.cache_store.as_ref()?;
        let record = store
            .find_completed_by_request_hash(&backend.to_string(), request_hash)
            .ok()??;
        store.output_text(&record.execution_id).ok().flatten()
    }

    /// Persist the backend-native session id after ACP returns it.
    fn persist_backend_session_id(
        &self,
        backend: AcpBackend,
        cwd: &PathBuf,
        backend_session_id: &str,
    ) {
        if let Some(ledger) = &self.session_ledger_store {
            let _ = ledger.record_backend_session(
                &self.engine_session_id,
                &backend.to_string(),
                Some(backend_session_id),
                cwd,
            );
        }
    }

    /// Ensure the durable ledger has a session row and a backend-session row for this turn.
    fn ensure_ledger_session(&self, backend: AcpBackend, cwd: &PathBuf, model: Option<&str>) {
        if let Some(ledger) = &self.session_ledger_store {
            let _ = ledger.ensure_session(
                &self.engine_session_id,
                cwd,
                Some(&backend.to_string()),
                model,
            );
            let _ = ledger.record_backend_session(
                &self.engine_session_id,
                &backend.to_string(),
                None,
                cwd,
            );
        }
    }

    /// Build and persist a summary when the active conversation switches backend.
    fn prepare_backend_handoff(&mut self, backend: AcpBackend, cwd: &PathBuf) -> Option<String> {
        let previous = self
            .last_used_backend
            .filter(|previous| *previous != backend)?;
        let summary = self.working_memory.render(800);
        if summary.trim().is_empty() {
            return None;
        }
        if let Some(ledger) = &self.session_ledger_store {
            let _ = ledger.publish_handoff(
                &self.engine_session_id,
                Some(&previous.to_string()),
                Some(&backend.to_string()),
                cwd,
                &summary,
            );
        }
        if let Some(store) = &self.memory_store {
            let _ = store.insert(MemoryInsert {
                memory_type: MemoryType::Episodic,
                facet: None,
                scope: MemoryScope::Session,
                scope_id: self.engine_session_id.clone(),
                content: format!(
                    "Backend handoff from {} to {}:\n{}",
                    previous, backend, summary
                ),
                confidence: 0.85,
                source_backend: Some(previous.to_string()),
                source_session_id: Some(self.engine_session_id.clone()),
                source_execution_id: None,
                metadata_json: Some("{\"kind\":\"handoff\"}".to_string()),
                ttl_days: 7,
                supersedes: None,
            });
        }
        Some(summary)
    }

    /// Store the final result of a turn in the session ledger.
    fn record_ledger_turn(
        &self,
        backend: AcpBackend,
        execution_id: Option<&str>,
        request_hash: &str,
        output: &str,
        status: &str,
    ) {
        if let Some(ledger) = &self.session_ledger_store {
            let _ = ledger.record_turn(
                &self.engine_session_id,
                &backend.to_string(),
                execution_id,
                request_hash,
                &summarize(output, 500),
                status,
            );
        }
    }

    /// Persist or count the runtime side effects represented by one engine event.
    fn record_runtime_event(&self, execution_id: &Option<String>, event: RuntimeEvent) {
        // Output events are appended to the execution cache so completed turns can be replayed.
        if let (Some(eid), RuntimeEvent::Output(_)) = (execution_id.as_ref(), &event) {
            if let Some(store) = &self.cache_store {
                let _ = store.append_output(eid, &event);
            }
        }
        // Token usage is metric-only here; the raw event remains in the ACP output event list.
        if let RuntimeEvent::TokenUsage(ref tu) = event {
            let m = metrics::get();
            m.token_usage_count.add(1, &[]);
            if let Some(input) = tu.input_tokens {
                m.token_input.add(input, &[]);
            }
            if let Some(output) = tu.output_tokens {
                m.token_output.add(output, &[]);
            }
            if let Some(total) = tu.total_tokens {
                m.token_total.add(total, &[]);
            }
            tracing::info!(
                input_tokens = tu.input_tokens,
                output_tokens = tu.output_tokens,
                total_tokens = tu.total_tokens,
                "token.usage"
            );
        }
    }

    /// Emit a structured tracing event with a dynamic level.
    fn log_engine_event(
        &self,
        _execution_id: Option<&str>,
        backend: AcpBackend,
        level: &str,
        event: &str,
        fields: serde_json::Value,
    ) {
        match level {
            "error" => tracing::error!(backend = %backend, fields = %fields, "{}", event),
            "warn" => tracing::warn!(backend = %backend, fields = %fields, "{}", event),
            _ => tracing::info!(backend = %backend, fields = %fields, "{}", event),
        }
    }

    fn record_cache_hit_metric(&self) {
        metrics::get().cache_hit_count.add(1, &[]);
        tracing::info!("cache.hit");
    }

    fn record_cache_miss_metric(&self) {
        metrics::get().cache_miss_count.add(1, &[]);
        tracing::debug!("cache.miss");
    }

    fn record_active_sessions(&self) {
        // Keep this as a single hook for future OTel UpDownCounter session tracking.
    }

    /// Mark an execution row terminal when cache persistence is enabled.
    fn mark_execution_finished(&self, execution_id: &Option<String>, status: ExecutionStatus) {
        if let (Some(store), Some(execution_id)) = (&self.cache_store, execution_id) {
            let _ = store.finish_execution(execution_id, status);
        }
    }

    /// Mark an execution terminal and publish duration/count metrics.
    fn mark_execution_finished_with_timing(
        &self,
        execution_id: &Option<String>,
        backend: AcpBackend,
        status: ExecutionStatus,
        timing: &acp::AcpPromptTiming,
    ) {
        self.mark_execution_finished(execution_id, status.clone());
        let m = metrics::get();
        let backend_attr = opentelemetry::KeyValue::new("backend", backend.to_string());
        m.prompt_duration
            .record(timing.prompt_ms as f64 / 1000.0, &[backend_attr.clone()]);
        if let Some(init_ms) = timing.init_ms {
            m.init_duration
                .record(init_ms as f64 / 1000.0, &[backend_attr]);
        }
        let status_attr = opentelemetry::KeyValue::new("status", status.as_str().to_string());
        m.execution_count.add(1, &[status_attr]);
    }

    /// Extract simple keyword-based memories directly from user prompts.
    ///
    /// This is intentionally conservative: it only handles obvious memory statements and leaves
    /// richer memory writes to the `iota_memory_write` MCP tool.
    fn extract_keyword_memories(
        &self,
        backend: AcpBackend,
        cwd: &PathBuf,
        prompt: &str,
        execution_id: Option<&str>,
    ) -> Vec<String> {
        let Some(store) = &self.memory_store else {
            return Vec::new();
        };
        classify_memory_prompt(prompt)
            .into_iter()
            .filter_map(|classified| {
                let scope_id = classified.scope_id(cwd);
                self.log_engine_event(
                    execution_id,
                    backend,
                    "info",
                    "memory.write.call",
                    serde_json::json!({
                        "source": "engine-keyword",
                        "type": classified.memory_type.as_str(),
                        "facet": classified.facet.as_ref().map(MemoryFacet::as_str),
                        "scope": classified.scope.as_str(),
                        "scope_id": scope_id.clone(),
                        "confidence": classified.confidence,
                        "content_chars": prompt.trim().chars().count(),
                    }),
                );
                tracing::info!(
                    backend = %backend,
                    execution_id = execution_id.unwrap_or("-"),
                    session_id = %self.engine_session_id,
                    memory_type = %classified.memory_type.as_str(),
                    facet = classified.facet.as_ref().map(MemoryFacet::as_str).unwrap_or("-"),
                    scope = %classified.scope.as_str(),
                    scope_id = %scope_id,
                    source = "engine-keyword",
                    "engine structured memory write started"
                );
                store
                    .insert(MemoryInsert {
                        memory_type: classified.memory_type.clone(),
                        facet: classified.facet.clone(),
                        scope: classified.scope.clone(),
                        scope_id,
                        content: prompt.trim().to_string(),
                        confidence: classified.confidence,
                        source_backend: Some(backend.to_string()),
                        source_session_id: Some(self.engine_session_id.clone()),
                        source_execution_id: execution_id.map(str::to_string),
                        metadata_json: Some("{\"extraction\":\"engine-keyword\"}".to_string()),
                        ttl_days: classified.ttl_days,
                        supersedes: None,
                    })
                    .map(|id| {
                        self.log_engine_event(
                            execution_id,
                            backend,
                            "info",
                            "memory.write.result",
                            serde_json::json!({
                                "source": "engine-keyword",
                                "memory_id": id.clone(),
                                "ok": true,
                            }),
                        );
                        tracing::info!(
                            backend = %backend,
                            execution_id = execution_id.unwrap_or("-"),
                            session_id = %self.engine_session_id,
                            memory_id = %id,
                            source = "engine-keyword",
                            "engine structured memory write completed"
                        );
                        id
                    })
                    .map_err(|err| {
                        self.log_engine_event(
                            execution_id,
                            backend,
                            "warn",
                            "memory.write.result",
                            serde_json::json!({
                                "source": "engine-keyword",
                                "ok": false,
                                "error": err.to_string(),
                            }),
                        );
                        tracing::warn!(
                            backend = %backend,
                            execution_id = execution_id.unwrap_or("-"),
                            session_id = %self.engine_session_id,
                            error = %err,
                            source = "engine-keyword",
                            "engine structured memory write failed"
                        );
                        err
                    })
                    .ok()
            })
            .collect()
    }

    /// Persist a summarized prompt/output pair as session-scoped episodic memory.
    fn persist_turn_as_episodic_memory(
        &self,
        backend: AcpBackend,
        prompt: &str,
        output: &str,
        execution_id: Option<&str>,
    ) {
        let Some(store) = &self.memory_store else {
            return;
        };
        let content = format!(
            "Prompt: {}
Output: {}",
            summarize(prompt, 300),
            summarize(output, 500)
        );
        let content_chars = content.chars().count();
        self.log_engine_event(
            execution_id,
            backend,
            "info",
            "memory.write.call",
            serde_json::json!({
                "source": "engine-episodic",
                "type": "episodic",
                "scope": "session",
                "scope_id": self.engine_session_id.clone(),
                "confidence": 0.8,
                "content_chars": content_chars,
            }),
        );
        tracing::info!(
            backend = %backend,
            execution_id = execution_id.unwrap_or("-"),
            session_id = %self.engine_session_id,
            content_chars,
            source = "engine-episodic",
            "engine episodic memory write started"
        );
        match store.insert(MemoryInsert {
            memory_type: MemoryType::Episodic,
            facet: None,
            scope: MemoryScope::Session,
            scope_id: self.engine_session_id.clone(),
            content,
            confidence: 0.8,
            source_backend: Some(backend.to_string()),
            source_session_id: Some(self.engine_session_id.clone()),
            source_execution_id: execution_id.map(str::to_string),
            metadata_json: None,
            ttl_days: 7,
            supersedes: None,
        }) {
            Ok(id) => {
                self.log_engine_event(
                    execution_id,
                    backend,
                    "info",
                    "memory.write.result",
                    serde_json::json!({
                        "source": "engine-episodic",
                        "memory_id": id.clone(),
                        "ok": true,
                    }),
                );
                tracing::info!(
                    backend = %backend,
                    execution_id = execution_id.unwrap_or("-"),
                    session_id = %self.engine_session_id,
                    memory_id = %id,
                    source = "engine-episodic",
                    "engine episodic memory write completed"
                );
            }
            Err(err) => {
                self.log_engine_event(
                    execution_id,
                    backend,
                    "warn",
                    "memory.write.result",
                    serde_json::json!({
                        "source": "engine-episodic",
                        "ok": false,
                        "error": err.to_string(),
                    }),
                );
                tracing::warn!(
                    backend = %backend,
                    execution_id = execution_id.unwrap_or("-"),
                    session_id = %self.engine_session_id,
                    error = %err,
                    source = "engine-episodic",
                    "engine episodic memory write failed"
                );
            }
        }
        let keep = self.effective_config.episodic_compaction_keep();
        match store.compact_episodic_scope(MemoryScope::Session, &self.engine_session_id, keep) {
            Ok(deleted) => {
                self.log_engine_event(
                    execution_id,
                    backend,
                    "info",
                    "memory.compaction",
                    serde_json::json!({
                        "scope": "session",
                        "scope_id": self.engine_session_id.clone(),
                        "keep_latest": keep,
                        "deleted": deleted,
                        "ok": true,
                    }),
                );
                tracing::info!(
                    backend = %backend,
                    execution_id = execution_id.unwrap_or("-"),
                    session_id = %self.engine_session_id,
                    keep_latest = keep,
                    deleted,
                    "engine episodic memory compaction completed"
                );
            }
            Err(err) => {
                self.log_engine_event(
                    execution_id,
                    backend,
                    "warn",
                    "memory.compaction",
                    serde_json::json!({
                        "scope": "session",
                        "scope_id": self.engine_session_id.clone(),
                        "keep_latest": keep,
                        "ok": false,
                        "error": err.to_string(),
                    }),
                );
                tracing::warn!(
                    backend = %backend,
                    execution_id = execution_id.unwrap_or("-"),
                    session_id = %self.engine_session_id,
                    keep_latest = keep,
                    error = %err,
                    "engine episodic memory compaction failed"
                );
            }
        }
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

        AcpClient::start(
            backend,
            cwd,
            backend_process_env_with_context(
                backend,
                section,
                self.effective_config.backend_context_config(backend),
            ),
            Some(normalized_acp_command(backend, section, acp_config)),
            self.effective_config.context_mcp_servers(backend),
            self.effective_config.context_session_options(backend),
            self.effective_config.context_tool_whitelist(backend),
            self.show_native_protocol,
            self.acp_timeout_ms,
        )
        .await
    }
}

struct ClassifiedMemory {
    memory_type: MemoryType,
    facet: Option<MemoryFacet>,
    scope: MemoryScope,
    confidence: f64,
    ttl_days: i64,
}

impl ClassifiedMemory {
    fn scope_id(&self, cwd: &PathBuf) -> String {
        match self.scope {
            MemoryScope::User => "user-sympantos".to_string(),
            MemoryScope::Project => cwd
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("iota-sympantos")
                .to_string(),
            MemoryScope::Session => "session".to_string(),
            MemoryScope::Global => "global".to_string(),
        }
    }
}

fn classify_memory_prompt(prompt: &str) -> Vec<ClassifiedMemory> {
    let lower = prompt.to_lowercase();
    let mut memories = Vec::new();
    let is_procedure =
        prompt.contains("实验步骤") || lower.contains("steps:") || lower.contains("procedure");
    if prompt.contains("我叫") || lower.contains("my name") || lower.contains("i am") {
        memories.push(ClassifiedMemory {
            memory_type: MemoryType::Semantic,
            facet: Some(MemoryFacet::Identity),
            scope: MemoryScope::User,
            confidence: 0.95,
            ttl_days: 365,
        });
    }
    if prompt.contains("偏好") || lower.contains("prefer") || prompt.contains("报告格式") {
        memories.push(ClassifiedMemory {
            memory_type: MemoryType::Semantic,
            facet: Some(MemoryFacet::Preference),
            scope: MemoryScope::User,
            confidence: 0.92,
            ttl_days: 365,
        });
    }
    if prompt.contains("项目目标") || lower.contains("project goal") || lower.contains("q2") {
        memories.push(ClassifiedMemory {
            memory_type: MemoryType::Semantic,
            facet: Some(MemoryFacet::Strategic),
            scope: MemoryScope::Project,
            confidence: 0.90,
            ttl_days: 365,
        });
    }
    if !is_procedure
        && (prompt.contains("SQLite")
            || prompt.contains("SHA-256")
            || prompt.contains("存储层")
            || lower.contains("rust 实现"))
    {
        memories.push(ClassifiedMemory {
            memory_type: MemoryType::Semantic,
            facet: Some(MemoryFacet::Domain),
            scope: MemoryScope::Project,
            confidence: 0.90,
            ttl_days: 365,
        });
    }
    if is_procedure {
        memories.push(ClassifiedMemory {
            memory_type: MemoryType::Procedural,
            facet: None,
            scope: MemoryScope::Project,
            confidence: 0.88,
            ttl_days: 365,
        });
    }
    if prompt.contains("本轮") || prompt.contains("已通过") || lower.contains("this session") {
        memories.push(ClassifiedMemory {
            memory_type: MemoryType::Episodic,
            facet: None,
            scope: MemoryScope::Project,
            confidence: 0.82,
            ttl_days: 30,
        });
    }
    memories
}

fn is_memory_write_only_prompt(prompt: &str) -> bool {
    !classify_memory_prompt(prompt).is_empty()
        && !prompt.contains('？')
        && !prompt.contains('?')
        && !prompt.contains("请")
}

fn is_explicit_memory_tool_prompt(prompt: &str) -> bool {
    prompt.contains("iota_memory_write")
}

fn deterministic_memory_answer(prompt: &str, buckets: &RecallBuckets) -> Option<String> {
    if !is_memory_query(prompt) {
        return None;
    }
    let lower = prompt.to_lowercase();
    let mut lines = Vec::new();
    let all_info = prompt.contains("所有信息");
    if all_info {
        push_memory_lines(&mut lines, "身份", &buckets.identity);
        push_memory_lines(&mut lines, "偏好", &buckets.preference);
        push_memory_lines(&mut lines, "项目目标", &buckets.strategic);
        push_memory_lines(&mut lines, "技术事实", &buckets.domain);
        push_memory_lines(&mut lines, "实验步骤", &buckets.procedural);
        push_memory_lines(&mut lines, "历史经历", &buckets.episodic);
        return (!lines.is_empty()).then(|| lines.join("\n"));
    }
    if prompt.contains("谁") || lower.contains("who") || prompt.contains("了解") {
        push_memory_lines(&mut lines, "身份", &buckets.identity);
    }
    if prompt.contains("偏好") || prompt.contains("报告格式") || prompt.contains("语言") {
        push_memory_lines(&mut lines, "偏好", &buckets.preference);
    }
    if prompt.contains("目标") || prompt.contains("技术") || prompt.contains("实现") {
        push_memory_lines(&mut lines, "项目目标", &buckets.strategic);
        push_memory_lines(&mut lines, "技术事实", &buckets.domain);
    }
    if prompt.contains("步骤") || prompt.contains("发生") || prompt.contains("回顾") {
        push_memory_lines(&mut lines, "实验步骤", &buckets.procedural);
        push_memory_lines(&mut lines, "历史经历", &buckets.episodic);
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn is_memory_query(prompt: &str) -> bool {
    prompt.contains('？')
        || prompt.contains('?')
        || prompt.contains("请介绍")
        || prompt.contains("你知道")
        || prompt.contains("告诉我")
        || prompt.contains("回顾")
        || prompt.contains("列出")
        || prompt.contains("发生了什么")
}

fn push_memory_lines(
    lines: &mut Vec<String>,
    label: &str,
    records: &[crate::store::memory::MemoryRecord],
) {
    if records.is_empty() {
        return;
    }
    lines.push(format!("{}：", label));
    for record in records {
        lines.push(format!("- {}", record.content.trim()));
    }
}

fn event_payload(event: &RuntimeEvent) -> serde_json::Value {
    match event {
        RuntimeEvent::Memory(memory) => memory.payload.clone(),
        other => serde_json::json!({"event_type": other.event_type()}),
    }
}

fn memory_inject_payload(buckets: &RecallBuckets, memory_chars: usize) -> serde_json::Value {
    let total_chars = memory_total_chars(buckets);
    serde_json::json!({
        "identity": memory_bucket_summary(&buckets.identity),
        "preference": memory_bucket_summary(&buckets.preference),
        "strategic": memory_bucket_summary(&buckets.strategic),
        "domain": memory_bucket_summary(&buckets.domain),
        "procedural": memory_bucket_summary(&buckets.procedural),
        "episodic": memory_bucket_summary(&buckets.episodic),
        "budget": {
            "memory_chars": memory_chars,
            "total_chars": total_chars,
            "truncated": total_chars > memory_chars,
            "excluded_count": excluded_memory_count(buckets, memory_chars),
        }
    })
}

fn memory_total_chars(buckets: &RecallBuckets) -> usize {
    buckets
        .identity
        .iter()
        .chain(buckets.preference.iter())
        .chain(buckets.strategic.iter())
        .chain(buckets.domain.iter())
        .chain(buckets.procedural.iter())
        .chain(buckets.episodic.iter())
        .map(|record| record.content.chars().count())
        .sum()
}

fn excluded_memory_count(buckets: &RecallBuckets, memory_chars: usize) -> usize {
    let mut used = 0usize;
    let mut excluded = 0usize;
    for record in buckets
        .identity
        .iter()
        .chain(buckets.preference.iter())
        .chain(buckets.strategic.iter())
        .chain(buckets.domain.iter())
        .chain(buckets.procedural.iter())
        .chain(buckets.episodic.iter())
    {
        let len = record.content.chars().count();
        if used + len <= memory_chars {
            used += len;
        } else {
            excluded += 1;
        }
    }
    excluded
}

fn memory_bucket_summary(records: &[crate::store::memory::MemoryRecord]) -> serde_json::Value {
    serde_json::Value::Array(
        records
            .iter()
            .map(|record| {
                serde_json::json!({
                    "id": record.id,
                    "scope": record.scope,
                    "scope_id": record.scope_id,
                    "confidence": record.confidence,
                    "content": summarize(&record.content, 180),
                })
            })
            .collect(),
    )
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
