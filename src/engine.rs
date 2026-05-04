use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, btree_map::Entry};
use std::path::PathBuf;

use crate::acp::{self, AcpBackend, AcpClient, AcpPromptOutput};
use crate::config::{
    self, NimiaConfig, backend_config, backend_process_env, config_path, configured_model,
    normalized_acp_command,
};
use crate::context::{ComposeInput, ContextEngine, DialogueBuffer};
use crate::event_store::{self, EventStore};
use crate::memory::{MemoryFacet, MemoryInsert, MemoryScope, MemoryStore, MemoryType};
use crate::runtime_event::{ErrorEvent, OutputEvent, RuntimeEvent, StateEvent};
use crate::session_ledger::SessionLedger;
use crate::skill_runner;
use crate::skills::SkillRegistry;
use crate::utils::summarize;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ClientKey {
    backend: AcpBackend,
    cwd: PathBuf,
}

pub struct IotaEngine {
    config: NimiaConfig,
    clients: BTreeMap<ClientKey, AcpClient>,
    show_native: bool,
    timeout_ms: u64,
    context_engine: ContextEngine,
    memory_store: Option<MemoryStore>,
    event_store: Option<EventStore>,
    dialogue: DialogueBuffer,
    session_id: String,
    session_ledger: Option<SessionLedger>,
    active_backend: Option<AcpBackend>,
}

impl IotaEngine {
    pub fn new(config: NimiaConfig, show_native: bool, timeout_ms: u64) -> Self {
        let context_engine = ContextEngine::from_config(config.context_engine.as_ref());
        let memory_store = config::context_memory_db_path(&config)
            .ok()
            .and_then(|path| MemoryStore::open(&path).ok());
        let event_store = EventStore::default_path()
            .ok()
            .and_then(|path| EventStore::open(&path).ok());
        let session_ledger = SessionLedger::default_path()
            .ok()
            .and_then(|path| SessionLedger::open(&path).ok());
        let session_id = std::env::current_dir()
            .ok()
            .and_then(|cwd| {
                session_ledger
                    .as_ref()
                    .and_then(|ledger| ledger.latest_session_for_cwd(&cwd).ok().flatten())
            })
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        Self {
            config,
            clients: BTreeMap::new(),
            show_native,
            timeout_ms,
            context_engine,
            memory_store,
            event_store,
            dialogue: DialogueBuffer::new(50),
            session_id,
            session_ledger,
            active_backend: None,
        }
    }

    #[allow(dead_code)]
    pub fn config(&self) -> &NimiaConfig {
        &self.config
    }

    /// Set an output-streaming sender on all currently open ACP clients.
    /// New chunks from `session/update` events are forwarded to `tx` as they arrive.
    pub fn set_stream_sender(&mut self, tx: Option<tokio::sync::mpsc::Sender<String>>) {
        for client in self.clients.values_mut() {
            client.stream_tx = tx.clone();
        }
    }

    #[allow(dead_code)]
    pub async fn shutdown_in_place(&mut self) {
        self.shutdown_all_clients().await;
    }

    async fn try_join_running(&self, backend: AcpBackend, request_hash: &str) -> Option<String> {
        let store = self.event_store.as_ref()?.clone();
        let running = store
            .find_running_by_request_hash(&backend.to_string(), request_hash)
            .ok()??;
        let deadline =
            tokio::time::Instant::now() + tokio::time::Duration::from_millis(self.timeout_ms);
        // Poll with exponential back-off (50 ms → 500 ms cap) to avoid busy-wait
        // while still picking up completion within a reasonable latency budget.
        let mut poll_interval_ms: u64 = 50;
        loop {
            if let Ok(Some(record)) = store.get_execution(&running.execution_id) {
                if record.status == "completed" {
                    return store.output_text(&running.execution_id).ok().flatten();
                }
                if record.status != "running" {
                    return None;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return None;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(poll_interval_ms)).await;
            poll_interval_ms = (poll_interval_ms * 2).min(500);
        }
    }

    pub async fn warm_enabled_backends_in_cwd(&mut self, cwd: PathBuf) -> Result<usize> {
        let mut handles = Vec::new();
        for backend in acp::ALL_BACKENDS {
            let key = ClientKey {
                backend,
                cwd: cwd.clone(),
            };
            if self.clients.contains_key(&key) {
                continue;
            }
            let Some(section) = backend_config(&self.config, backend) else {
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

            let env = backend_process_env(backend, section);
            let command = normalized_acp_command(backend, section, acp_config);
            let mcp_servers = config::context_mcp_servers(&self.config, backend);
            let cwd = cwd.clone();
            let show_native = self.show_native;
            let timeout_ms = self.timeout_ms;
            handles.push(tokio::spawn(async move {
                match AcpClient::start(
                    backend,
                    cwd.clone(),
                    env,
                    Some(command),
                    mcp_servers,
                    show_native,
                    timeout_ms,
                )
                .await
                {
                    Ok(client) => Some((ClientKey { backend, cwd }, client)),
                    Err(err) => {
                        eprintln!("Failed to warm {}: {}", backend, err);
                        None
                    }
                }
            }));
        }

        for handle in handles {
            if let Ok(Some((key, client))) = handle.await {
                self.clients.insert(key, client);
            }
        }
        self.record_active_sessions();
        Ok(self.clients.len())
    }

    pub async fn prompt_in_cwd(
        &mut self,
        backend: AcpBackend,
        cwd: PathBuf,
        prompt: &str,
    ) -> Result<String> {
        Ok(self.prompt_in_cwd_timed(backend, cwd, prompt).await?.text)
    }

    pub async fn prompt_in_cwd_timed(
        &mut self,
        backend: AcpBackend,
        cwd: PathBuf,
        prompt: &str,
    ) -> Result<AcpPromptOutput> {
        self.prompt_in_cwd_timed_with_execution_id(backend, cwd, prompt, None)
            .await
    }

    pub async fn prompt_in_cwd_timed_with_execution_id(
        &mut self,
        backend: AcpBackend,
        cwd: PathBuf,
        prompt: &str,
        requested_execution_id: Option<&str>,
    ) -> Result<AcpPromptOutput> {
        let request_hash = event_store::request_hash(&backend.to_string(), &cwd, prompt);
        tracing::debug!(backend = %backend, cwd = %cwd.display(), request_hash = %request_hash, "prompt requested");
        if let Some(output) = self.try_replay_completed(backend, &request_hash) {
            self.record_cache_hit();
            tracing::info!(backend = %backend, request_hash = %request_hash, "replaying completed execution");
            self.dialogue.push_turn(backend, prompt, &output);
            return Ok(AcpPromptOutput::synthetic(output));
        }
        if let Some(output) = self.try_join_running(backend, &request_hash).await {
            self.record_cache_hit();
            tracing::info!(backend = %backend, request_hash = %request_hash, "joined running execution");
            self.dialogue.push_turn(backend, prompt, &output);
            return Ok(AcpPromptOutput::synthetic(output));
        }
        let model = backend_config(&self.config, backend).and_then(configured_model);
        self.ensure_session_ledger(backend, &cwd, model.as_deref());
        let handoff = self.prepare_handoff(backend, &cwd);
        let execution_id = match self.event_store.as_ref() {
            Some(store) => {
                match store.begin_execution_with_id(
                    &backend.to_string(),
                    &self.session_id,
                    &request_hash,
                    requested_execution_id,
                ) {
                    Ok(execution_id) => Some(execution_id),
                    Err(_) => {
                        if let Some(output) = self.try_join_running(backend, &request_hash).await {
                            self.record_cache_hit();
                            self.dialogue.push_turn(backend, prompt, &output);
                            return Ok(AcpPromptOutput::synthetic(output));
                        }
                        None
                    }
                }
            }
            None => None,
        };
        self.record_cache_miss();
        tracing::debug!(backend = %backend, execution_id = execution_id.as_deref(), "execution started");
        self.record_event(
            &execution_id,
            RuntimeEvent::State(StateEvent {
                state: "started".to_string(),
                detail: None,
            }),
        );

        let configured_roots = config::context_skill_roots(&self.config);
        let skills = SkillRegistry::load(&cwd, &configured_roots);
        if let Some(skill) = skills.match_skill(backend, prompt) {
            if let Some(skill_output) = skill_runner::run_engine_skill(skill, prompt).await? {
                for event in skill_output.events {
                    self.record_event(&execution_id, event);
                }
                self.record_event(
                    &execution_id,
                    RuntimeEvent::Output(OutputEvent {
                        text: skill_output.text.clone(),
                        role: Some("engine".to_string()),
                    }),
                );
                self.finish_execution(&execution_id, "completed");
                self.record_turn(
                    backend,
                    execution_id.as_deref(),
                    &request_hash,
                    &skill_output.text,
                    "completed",
                );
                self.active_backend = Some(backend);
                self.dialogue.push_turn(backend, prompt, &skill_output.text);
                self.write_episodic_memory(
                    backend,
                    prompt,
                    &skill_output.text,
                    execution_id.as_deref(),
                );
                self.extract_explicit_memory(
                    backend,
                    &cwd,
                    prompt,
                    &skill_output.text,
                    execution_id.as_deref(),
                );
                return Ok(AcpPromptOutput::synthetic(skill_output.text));
            }
        }

        let memory = self.memory_store.as_ref().and_then(|store| {
            store
                .recall_buckets("local-user", &cwd.display().to_string(), &self.session_id)
                .ok()
        });
        // compose_effective_prompt runs `git status` which is a blocking syscall.
        // Off-load it to the blocking thread pool to avoid stalling the tokio worker.
        let context_engine = self.context_engine.clone();
        let session_id_c = self.session_id.clone();
        let model_c = model.clone();
        let skills_c = skills.clone();
        let dialogue_c = self.dialogue.clone();
        let handoff_c = handoff.clone();
        let prompt_c = prompt.to_string();
        let cwd_c = cwd.clone();
        let effective_prompt = tokio::task::spawn_blocking(move || {
            context_engine.compose_effective_prompt(ComposeInput {
                backend,
                cwd: &cwd_c,
                session_id: &session_id_c,
                model: model_c.as_deref(),
                prompt: &prompt_c,
                memory: memory.as_ref(),
                skills: Some(&skills_c),
                dialogue: &dialogue_c,
                handoff: handoff_c.as_deref(),
            })
        })
        .await
        .context("context composition task panicked")?;

        let client_started = self.ensure_client(backend, cwd.clone()).await?;
        let key = ClientKey {
            backend,
            cwd: cwd.clone(),
        };
        let client = self
            .clients
            .get_mut(&key)
            .context("ACP client missing after warm")?;
        let startup_timing = client.startup_timing();
        match client
            .prompt_with_cwd_timed_for_execution(&cwd, &effective_prompt, execution_id.as_deref())
            .await
        {
            Ok(mut output) => {
                output.execution_id = execution_id.clone();
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
                    self.record_event(&execution_id, event);
                }
                if !has_output_event {
                    self.record_event(
                        &execution_id,
                        RuntimeEvent::Output(OutputEvent {
                            text: output.text.clone(),
                            role: Some("assistant".to_string()),
                        }),
                    );
                }
                if let Some(session_id) = output.backend_session_id.as_deref() {
                    self.record_backend_session_id(backend, &cwd, session_id);
                }
                self.finish_execution_with_timing(&execution_id, "completed", &output.timing);
                tracing::info!(backend = %backend, execution_id = execution_id.as_deref(), total_ms = output.timing.total_ms, prompt_ms = output.timing.prompt_ms, "execution completed");
                self.record_turn(
                    backend,
                    execution_id.as_deref(),
                    &request_hash,
                    &output.text,
                    "completed",
                );
                self.active_backend = Some(backend);
                self.dialogue.push_turn(backend, prompt, &output.text);
                self.write_episodic_memory(backend, prompt, &output.text, execution_id.as_deref());
                self.extract_explicit_memory(
                    backend,
                    &cwd,
                    prompt,
                    &output.text,
                    execution_id.as_deref(),
                );
                Ok(output)
            }
            Err(err) => {
                self.record_event(
                    &execution_id,
                    RuntimeEvent::Error(ErrorEvent {
                        message: err.to_string(),
                        code: None,
                        data: None,
                    }),
                );
                self.finish_execution(&execution_id, "failed");
                tracing::warn!(backend = %backend, execution_id = execution_id.as_deref(), error = %err, "execution failed");
                self.record_turn(
                    backend,
                    execution_id.as_deref(),
                    &request_hash,
                    &err.to_string(),
                    "failed",
                );
                Err(err)
            }
        }
    }

    pub fn is_warmed_in_cwd(&self, backend: AcpBackend, cwd: &PathBuf) -> bool {
        self.clients.contains_key(&ClientKey {
            backend,
            cwd: cwd.clone(),
        })
    }

    pub async fn warm_backend_in_cwd(&mut self, backend: AcpBackend, cwd: PathBuf) -> Result<bool> {
        self.ensure_client(backend, cwd).await
    }

    pub async fn shutdown(mut self) {
        while let Some((_, client)) = self.clients.pop_first() {
            client.shutdown().await;
        }
        self.record_active_sessions();
    }

    pub fn clients_count(&self) -> usize {
        self.clients.len()
    }

    pub fn set_timeout_ms(&mut self, timeout_ms: u64) {
        self.timeout_ms = timeout_ms;
        for client in self.clients.values_mut() {
            client.set_timeout_ms(timeout_ms);
        }
    }

    pub async fn shutdown_all_clients(&mut self) {
        while let Some((_, client)) = self.clients.pop_first() {
            client.shutdown().await;
        }
        self.record_active_sessions();
    }

    fn try_replay_completed(&self, backend: AcpBackend, request_hash: &str) -> Option<String> {
        let store = self.event_store.as_ref()?;
        let record = store
            .find_completed_by_request_hash(&backend.to_string(), request_hash)
            .ok()??;
        store.output_text(&record.execution_id).ok().flatten()
    }

    fn record_backend_session_id(
        &self,
        backend: AcpBackend,
        cwd: &PathBuf,
        backend_session_id: &str,
    ) {
        if let Some(ledger) = &self.session_ledger {
            let _ = ledger.record_backend_session(
                &self.session_id,
                &backend.to_string(),
                Some(backend_session_id),
                cwd,
            );
        }
    }

    fn ensure_session_ledger(&self, backend: AcpBackend, cwd: &PathBuf, model: Option<&str>) {
        if let Some(ledger) = &self.session_ledger {
            let _ = ledger.ensure_session(&self.session_id, cwd, Some(&backend.to_string()), model);
            let _ =
                ledger.record_backend_session(&self.session_id, &backend.to_string(), None, cwd);
        }
    }

    fn prepare_handoff(&mut self, backend: AcpBackend, cwd: &PathBuf) -> Option<String> {
        let previous = self
            .active_backend
            .filter(|previous| *previous != backend)?;
        let summary = self.dialogue.render(800);
        if summary.trim().is_empty() {
            return None;
        }
        if let Some(ledger) = &self.session_ledger {
            let _ = ledger.publish_handoff(
                &self.session_id,
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
                scope_id: self.session_id.clone(),
                content: format!(
                    "Backend handoff from {} to {}:\n{}",
                    previous, backend, summary
                ),
                confidence: 0.85,
                source_backend: Some(previous.to_string()),
                source_session_id: Some(self.session_id.clone()),
                source_execution_id: None,
                metadata_json: Some("{\"kind\":\"handoff\"}".to_string()),
                ttl_days: 7,
                supersedes: None,
            });
        }
        Some(summary)
    }

    fn record_turn(
        &self,
        backend: AcpBackend,
        execution_id: Option<&str>,
        request_hash: &str,
        output: &str,
        status: &str,
    ) {
        if let Some(ledger) = &self.session_ledger {
            let _ = ledger.record_turn(
                &self.session_id,
                &backend.to_string(),
                execution_id,
                request_hash,
                &summarize(output, 500),
                status,
            );
        }
    }

    fn record_event(&self, execution_id: &Option<String>, event: RuntimeEvent) {
        if let (Some(store), Some(execution_id)) = (&self.event_store, execution_id) {
            tracing::debug!(execution_id = %execution_id, event_type = event.event_type(), "recording runtime event");
            let _ = store.append_event(execution_id, &event);
        }
    }

    fn record_cache_hit(&self) {
        if let Some(store) = &self.event_store {
            let _ = store.record_cache_hit();
        }
    }

    fn record_cache_miss(&self) {
        if let Some(store) = &self.event_store {
            let _ = store.record_cache_miss();
        }
    }

    fn record_active_sessions(&self) {
        if let Some(store) = &self.event_store {
            let _ = store.set_active_sessions(self.clients.len() as u64);
        }
    }

    fn finish_execution(&self, execution_id: &Option<String>, status: &str) {
        if let (Some(store), Some(execution_id)) = (&self.event_store, execution_id) {
            let _ = store.finish_execution(execution_id, status);
        }
    }

    fn finish_execution_with_timing(
        &self,
        execution_id: &Option<String>,
        status: &str,
        timing: &acp::AcpPromptTiming,
    ) {
        if let (Some(store), Some(execution_id)) = (&self.event_store, execution_id) {
            let _ = store.record_timing(execution_id, timing);
            let _ = store.finish_execution(execution_id, status);
        }
    }

    fn extract_explicit_memory(
        &self,
        backend: AcpBackend,
        cwd: &PathBuf,
        prompt: &str,
        _output: &str,
        execution_id: Option<&str>,
    ) {
        let Some(store) = &self.memory_store else {
            return;
        };
        let lower = prompt.to_lowercase();
        let explicit = ["remember", "save this", "记住", "保存"]
            .iter()
            .any(|needle| lower.contains(needle));
        if !explicit {
            return;
        }
        // Strip the trigger keywords to derive content.
        // NOTE: This is keyword-based extraction and is intentionally imprecise.
        // Phrases such as "remember to clean up" will store "to clean up" as a
        // memory item.  The confidence is set below 1.0 to reflect this
        // uncertainty.  A future implementation should delegate extraction to
        // the LLM itself for more accurate structured results.
        let content = prompt
            .replace("remember", "")
            .replace("save this", "")
            .replace("记住", "")
            .replace("保存", "")
            .trim()
            .to_string();
        if content.is_empty() {
            return;
        }
        // Facet is also heuristic: keyword matches on common English/Chinese
        // terms.  Misclassification is possible; confidence reflects this.
        let (facet, scope) =
            if lower.contains("prefer") || lower.contains("偏好") || lower.contains("喜欢") {
                (MemoryFacet::Preference, MemoryScope::User)
            } else if lower.contains("i am") || lower.contains("我是") || lower.contains("my name")
            {
                (MemoryFacet::Identity, MemoryScope::User)
            } else if lower.contains("project") || lower.contains("项目") {
                (MemoryFacet::Strategic, MemoryScope::Project)
            } else {
                (MemoryFacet::Domain, MemoryScope::Project)
            };
        let scope_id = match scope {
            MemoryScope::User => "local-user".to_string(),
            MemoryScope::Project => cwd.display().to_string(),
            MemoryScope::Session => self.session_id.clone(),
            MemoryScope::Global => "global".to_string(),
        };
        let _ = store.insert(MemoryInsert {
            memory_type: MemoryType::Semantic,
            facet: Some(facet),
            scope,
            scope_id,
            content,
            // 0.70 rather than 0.90: heuristic extraction is often imprecise.
            confidence: 0.70,
            source_backend: Some(backend.to_string()),
            source_session_id: Some(self.session_id.clone()),
            source_execution_id: execution_id.map(str::to_string),
            metadata_json: Some("{\"extraction\":\"explicit-keyword\"}".to_string()),
            ttl_days: 365,
            supersedes: None,
        });
    }

    fn write_episodic_memory(
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
        let _ = store.insert(MemoryInsert {
            memory_type: MemoryType::Episodic,
            facet: None,
            scope: MemoryScope::Session,
            scope_id: self.session_id.clone(),
            content,
            confidence: 0.8,
            source_backend: Some(backend.to_string()),
            source_session_id: Some(self.session_id.clone()),
            source_execution_id: execution_id.map(str::to_string),
            metadata_json: None,
            ttl_days: 7,
            supersedes: None,
        });
    }

    async fn ensure_client(&mut self, backend: AcpBackend, cwd: PathBuf) -> Result<bool> {
        let key = ClientKey {
            backend,
            cwd: cwd.clone(),
        };
        if self.clients.contains_key(&key) {
            return Ok(false);
        }
        let client = self.start_client(backend, cwd.clone()).await?;
        match self.clients.entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(client);
                self.record_active_sessions();
            }
            Entry::Occupied(_) => {}
        }
        Ok(true)
    }

    async fn start_client(&self, backend: AcpBackend, cwd: PathBuf) -> Result<AcpClient> {
        let path = config_path()?;
        let section = backend_config(&self.config, backend).with_context(|| {
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
            backend_process_env(backend, section),
            Some(normalized_acp_command(backend, section, acp_config)),
            config::context_mcp_servers(&self.config, backend),
            self.show_native,
            self.timeout_ms,
        )
        .await
    }
}
