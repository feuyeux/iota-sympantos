use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, btree_map::Entry};
use std::path::PathBuf;

use crate::acp::{self, AcpBackend, AcpClient, AcpPromptOutput};
use crate::config::{
    self, NimiaConfig, backend_config, backend_context_config, backend_process_env_with_context,
    config_path, configured_model, normalized_acp_command,
};
use crate::context::{ComposeInput, ContextEngine, DialogueBuffer};
use crate::runtime_event::{ErrorEvent, MemoryEvent, OutputEvent, RuntimeEvent, StateEvent};
use crate::skill::SkillRegistry;
use crate::store::events::EventStore;
use crate::store::ledger::SessionLedger;
use crate::store::memory::{
    MemoryFacet, MemoryInsert, MemoryScope, MemoryStore, MemoryType, RecallBuckets,
};
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
        let session_cwd = std::env::current_dir().ok();
        Self::new_for_session_cwd(config, show_native, timeout_ms, session_cwd.as_deref())
    }

    pub fn new_for_session_cwd(
        config: NimiaConfig,
        show_native: bool,
        timeout_ms: u64,
        session_cwd: Option<&std::path::Path>,
    ) -> Self {
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
        let session_id = session_cwd
            .and_then(|cwd| {
                session_ledger
                    .as_ref()
                    .and_then(|ledger| ledger.latest_session_for_cwd(cwd).ok().flatten())
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

            let backend_context = backend_context_config(&self.config, backend);
            let env = backend_process_env_with_context(backend, section, backend_context);
            let command = normalized_acp_command(backend, section, acp_config);
            let mcp_servers = config::context_mcp_servers(&self.config, backend);
            let session_options = config::context_session_options(&self.config, backend);
            let tool_whitelist = config::context_tool_whitelist(&self.config, backend);
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
                    session_options,
                    tool_whitelist,
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
        let request_hash = crate::store::events::request_hash(&backend.to_string(), &cwd, prompt);
        tracing::debug!(backend = %backend, cwd = %cwd.display(), request_hash = %request_hash, "prompt requested");
        let configured_roots = config::context_skill_roots(&self.config);
        let skills = SkillRegistry::load(&cwd, &configured_roots);
        let matched_skill = skills.match_skill(backend, prompt);
        let skip_replay = matched_skill.is_some()
            || is_memory_query(prompt)
            || !classify_memory_prompt(prompt).is_empty();
        if !skip_replay && let Some(output) = self.try_replay_completed(backend, &request_hash) {
            self.record_cache_hit();
            tracing::info!(backend = %backend, request_hash = %request_hash, "replaying completed execution");
            self.dialogue.push_turn(backend, prompt, &output);
            return Ok(AcpPromptOutput::synthetic(output));
        }
        if !skip_replay && let Some(output) = self.try_join_running(backend, &request_hash).await {
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
                        if !skip_replay
                            && let Some(output) =
                                self.try_join_running(backend, &request_hash).await
                        {
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

        let extracted_memories = if is_memory_query(prompt) {
            Vec::new()
        } else {
            self.extract_structured_memories(backend, &cwd, prompt, execution_id.as_deref())
        };
        if !extracted_memories.is_empty() && is_memory_write_only_prompt(prompt) {
            let mut events = Vec::new();
            for memory_id in &extracted_memories {
                let event = RuntimeEvent::Memory(MemoryEvent {
                    action: "write".to_string(),
                    memory_id: Some(memory_id.clone()),
                    payload: serde_json::json!({"source":"engine-extract"}),
                });
                self.record_event(&execution_id, event.clone());
                events.push(event);
            }
            let text = format!("已记录 {} 条记忆。", extracted_memories.len());
            let output_event = RuntimeEvent::Output(OutputEvent {
                text: text.clone(),
                role: Some("engine".to_string()),
            });
            self.record_event(&execution_id, output_event.clone());
            events.push(output_event);
            self.finish_execution(&execution_id, "completed");
            self.record_turn(
                backend,
                execution_id.as_deref(),
                &request_hash,
                &text,
                "completed",
            );
            self.active_backend = Some(backend);
            self.dialogue.push_turn(backend, prompt, &text);
            let mut output = AcpPromptOutput::synthetic(text);
            output.execution_id = execution_id;
            output.events = events;
            return Ok(output);
        }

        if let Some(skill) = matched_skill {
            if let Some(skill_output) =
                crate::skill::runner::run_engine_skill(skill, prompt).await?
            {
                let mut events = Vec::new();
                for event in skill_output.events {
                    self.record_event(&execution_id, event.clone());
                    events.push(event);
                }
                let output_event = RuntimeEvent::Output(OutputEvent {
                    text: skill_output.text.clone(),
                    role: Some("engine".to_string()),
                });
                self.record_event(&execution_id, output_event.clone());
                events.push(output_event);
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
                let mut output = AcpPromptOutput::synthetic(skill_output.text);
                output.execution_id = execution_id;
                output.events = events;
                return Ok(output);
            }
        }

        let memory = self.memory_store.as_ref().and_then(|store| {
            store
                .recall_buckets("local-user", &cwd.display().to_string(), &self.session_id)
                .ok()
        });
        let memory_event = memory.as_ref().map(|buckets| {
            RuntimeEvent::Memory(MemoryEvent {
                action: "inject".to_string(),
                memory_id: None,
                payload: memory_inject_payload(buckets, self.context_engine.budgets().memory_chars),
            })
        });
        if let Some(event) = memory_event.clone() {
            self.record_event(&execution_id, event);
        }
        if let (Some(buckets), Some(text)) = (
            memory.as_ref(),
            deterministic_memory_answer(prompt, memory.as_ref().unwrap()),
        ) {
            let mut events = Vec::new();
            if let Some(event) = memory_event.clone() {
                events.push(event);
            }
            let output_event = RuntimeEvent::Output(OutputEvent {
                text: text.clone(),
                role: Some("engine".to_string()),
            });
            self.record_event(&execution_id, output_event.clone());
            events.push(output_event);
            self.finish_execution(&execution_id, "completed");
            self.record_turn(
                backend,
                execution_id.as_deref(),
                &request_hash,
                &text,
                "completed",
            );
            self.active_backend = Some(backend);
            self.dialogue.push_turn(backend, prompt, &text);
            let mut output = AcpPromptOutput::synthetic(text);
            output.execution_id = execution_id;
            output.events = events;
            let _ = buckets;
            return Ok(output);
        }
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

    fn extract_structured_memories(
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
                store
                    .insert(MemoryInsert {
                        memory_type: classified.memory_type.clone(),
                        facet: classified.facet.clone(),
                        scope: classified.scope.clone(),
                        scope_id: classified.scope_id(cwd),
                        content: prompt.trim().to_string(),
                        confidence: classified.confidence,
                        source_backend: Some(backend.to_string()),
                        source_session_id: Some(self.session_id.clone()),
                        source_execution_id: execution_id.map(str::to_string),
                        metadata_json: Some("{\"extraction\":\"engine-keyword\"}".to_string()),
                        ttl_days: classified.ttl_days,
                        supersedes: None,
                    })
                    .ok()
            })
            .collect()
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
            backend_process_env_with_context(
                backend,
                section,
                backend_context_config(&self.config, backend),
            ),
            Some(normalized_acp_command(backend, section, acp_config)),
            config::context_mcp_servers(&self.config, backend),
            config::context_session_options(&self.config, backend),
            config::context_tool_whitelist(&self.config, backend),
            self.show_native,
            self.timeout_ms,
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
mod tests {
    use super::*;
    use crate::store::memory::MemoryRecord;

    #[test]
    fn memory_inject_payload_uses_configured_budget() {
        let buckets = RecallBuckets {
            identity: vec![memory_record("one")],
            preference: vec![memory_record("two")],
            ..Default::default()
        };
        let payload = memory_inject_payload(&buckets, 5);
        let budget = payload.get("budget").unwrap();

        assert_eq!(budget.get("memory_chars").and_then(|v| v.as_u64()), Some(5));
        assert_eq!(budget.get("total_chars").and_then(|v| v.as_u64()), Some(6));
        assert_eq!(
            budget.get("truncated").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            budget.get("excluded_count").and_then(|v| v.as_u64()),
            Some(1)
        );
    }

    fn memory_record(content: &str) -> MemoryRecord {
        MemoryRecord {
            id: uuid::Uuid::new_v4().to_string(),
            memory_type: "semantic".to_string(),
            facet: Some("identity".to_string()),
            scope: "user".to_string(),
            scope_id: "local-user".to_string(),
            content: content.to_string(),
            confidence: 1.0,
            created_at: 1,
            updated_at: 1,
            expires_at: 999,
        }
    }
}
