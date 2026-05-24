use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::acp::{AcpBackend, AcpPromptOutput};
use crate::config::configured_model;
use crate::context::ComposeInput;
use crate::runtime_event::{ErrorEvent, MemoryEvent, OutputEvent, RuntimeEvent, StateEvent};
use crate::skill::SkillRegistry;
use crate::store::cache::{ExecutionStatus, request_hash};

use super::IotaEngine;
use super::memory_ops::{
    deterministic_memory_answer, is_explicit_memory_tool_prompt, is_memory_query,
    is_memory_write_only_prompt, memory_inject_payload,
};
use super::telemetry::event_payload;

impl IotaEngine {
    /// Run a prompt and return only the final assistant text.
    pub async fn run_prompt_text(
        &mut self,
        backend: AcpBackend,
        cwd: PathBuf,
        prompt: &str,
    ) -> Result<String> {
        Ok(self.run_with_timing(backend, cwd, prompt).await?.text)
    }

    /// Run a prompt and return text, runtime events, backend session id, and timing data.
    pub async fn run_with_timing(
        &mut self,
        backend: AcpBackend,
        cwd: PathBuf,
        prompt: &str,
    ) -> Result<AcpPromptOutput> {
        self.run(backend, cwd, prompt, None).await
    }

    /// Run a prompt with an optional externally supplied execution id.
    ///
    /// The daemon uses `requested_execution_id` so callers can correlate persisted cache/events
    /// with their own request id. When it is `None`, the cache layer allocates the id.
    #[tracing::instrument(
        skip(self, prompt),
        fields(
            acp.backend = %backend,
            cwd = %cwd.display(),
            session.id = %self.engine_session_id,
            acp.model = tracing::field::Empty,
            execution.id = tracing::field::Empty,
            request.hash = tracing::field::Empty,
        )
    )]
    pub async fn run(
        &mut self,
        backend: AcpBackend,
        cwd: PathBuf,
        prompt: &str,
        requested_execution_id: Option<&str>,
    ) -> Result<AcpPromptOutput> {
        let request_hash = request_hash(&backend.to_string(), &cwd, prompt);
        tracing::Span::current().record("request.hash", &request_hash);
        tracing::info!("prompt.requested");

        let skills = SkillRegistry::load_cached(
            &cwd,
            self.effective_config.skill_roots(),
            &mut self.skill_registry_cache,
        );
        let matched_skill = skills.match_skill(backend, prompt);
        let model = self
            .effective_config
            .backend_config(backend)
            .and_then(configured_model);
        if let Some(ref m) = model {
            tracing::Span::current().record("acp.model", m);
        }

        // The ledger records the logical session first, then later records turns and backend ids.
        self.ensure_ledger_session(backend, &cwd, model.as_deref());
        // When switching from one backend to another, inject recent dialogue as handoff text.
        let handoff = self.prepare_backend_handoff(backend, &cwd);
        let execution_id = self
            .cache_store
            .as_ref()
            .map(|store| {
                store.begin_execution_with_id(
                    &backend.to_string(),
                    &self.engine_session_id,
                    &request_hash,
                    requested_execution_id,
                )
            })
            .transpose()?;

        if let Some(ref eid) = execution_id {
            tracing::Span::current().record("execution.id", eid);
            tracing::info!("execution.started");
        }
        self.record_runtime_event(
            &execution_id,
            backend,
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
                self.record_runtime_event(&execution_id, backend, event.clone());
                events.push(event);
            }
            let text = format!("已记录 {} 条记忆。", extracted_memories.len());
            let output_event = RuntimeEvent::Output(OutputEvent {
                text: text.clone(),
                role: Some("engine".to_string()),
            });
            self.record_runtime_event(&execution_id, backend, output_event.clone());
            events.push(output_event);
            return Ok(self.finalize_local_turn(
                backend,
                &execution_id,
                &request_hash,
                prompt,
                text,
                events,
            ));
        }

        if let Some(skill) = matched_skill {
            // Engine-run skills are local deterministic handlers. When they match, they replace
            // the external ACP backend call.
            if let Some(skill_output) =
                crate::skill::runner::run_engine_skill(skill, prompt).await?
            {
                let mut events = Vec::new();
                for event in skill_output.events {
                    self.record_runtime_event(&execution_id, backend, event.clone());
                    events.push(event);
                }
                let output_event = RuntimeEvent::Output(OutputEvent {
                    text: skill_output.text.clone(),
                    role: Some("engine".to_string()),
                });
                self.record_runtime_event(&execution_id, backend, output_event.clone());
                events.push(output_event);
                self.persist_turn_as_episodic_memory(
                    backend,
                    prompt,
                    &skill_output.text,
                    execution_id.as_deref(),
                );
                return Ok(self.finalize_local_turn(
                    backend,
                    &execution_id,
                    &request_hash,
                    prompt,
                    skill_output.text,
                    events,
                ));
            }
        }

        // Run memory recall and git status concurrently — both are blocking I/O
        // operations that are independent of each other.
        let memory_store_c = self.memory_store.clone();
        let thresholds = *self.effective_config.recall_thresholds();
        let project_id = cwd.display().to_string();
        let session_id_for_recall = self.engine_session_id.clone();
        let cwd_for_git = cwd.clone();

        let memory_task = tokio::task::spawn_blocking(move || {
            memory_store_c.as_ref().and_then(|store| {
                store
                    .recall_buckets_with_thresholds(
                        "local-user",
                        &project_id,
                        &session_id_for_recall,
                        thresholds,
                    )
                    .ok()
            })
        });
        let workspace_task =
            tokio::task::spawn_blocking(move || crate::context::render_workspace(&cwd_for_git));

        self.log_engine_event(
            execution_id.as_deref(),
            backend,
            "info",
            "memory.recall.started",
            serde_json::json!({
                "user_id": "local-user",
                "project_id": cwd.display().to_string(),
            }),
        );
        tracing::info!(
            user_id = "local-user",
            project_id = %cwd.display(),
            "memory.recall.started"
        );

        // Await both concurrently.
        let (memory_result, workspace_result) = tokio::join!(memory_task, workspace_task);
        let memory = memory_result.unwrap_or(None);
        let workspace_str = workspace_result.unwrap_or_default();

        if let Some(ref buckets) = memory {
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
                identity_count = buckets.identity.len(),
                preference_count = buckets.preference.len(),
                strategic_count = buckets.strategic.len(),
                domain_count = buckets.domain.len(),
                procedural_count = buckets.procedural.len(),
                episodic_count = buckets.episodic.len(),
                "memory.recall.completed"
            );
        }

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
                payload = %event_payload(&event),
                "memory.inject"
            );
            self.record_runtime_event(&execution_id, backend, event);
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
            self.record_runtime_event(&execution_id, backend, output_event.clone());
            events.push(output_event);
            let _ = buckets;
            return Ok(self.finalize_local_turn(
                backend,
                &execution_id,
                &request_hash,
                prompt,
                text,
                events,
            ));
        }
        // Compose the effective prompt — git status is already pre-computed so this
        // is now a pure CPU operation (no blocking I/O).
        let context_engine = self.context_engine.clone();
        let session_id_c = self.engine_session_id.clone();
        let model_c = model.clone();
        let skills_c = skills.clone();
        let working_memory_c = self.working_memory.clone();
        let handoff_c = handoff.clone();
        let prompt_c = prompt.to_string();
        let cwd_c = cwd.clone();
        let effective_prompt = context_engine.compose_effective_prompt(ComposeInput {
            backend,
            cwd: &cwd_c,
            session_id: &session_id_c,
            model: model_c.as_deref(),
            prompt: &prompt_c,
            memory: memory.as_ref(),
            skills: Some(&skills_c),
            working_memory: &working_memory_c,
            handoff: handoff_c.as_deref(),
            workspace: Some(&workspace_str),
        });

        if effective_prompt.contains("<iota-context>") {
            self.capture_runtime_context_snapshot(
                execution_id
                    .clone()
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                backend,
                cwd.clone(),
                model.clone(),
                effective_prompt.clone(),
            );
        }

        let client_started = self.ensure_acp_client(backend, cwd.clone()).await?;
        let key = super::AcpClientKey {
            backend,
            cwd: cwd.clone(),
        };
        let client = self
            .acp_clients
            .get_mut(&key)
            .context("ACP client missing after warm")?;
        let startup_timing = client.startup_timing();
        match client
            .execute(&cwd, &effective_prompt, execution_id.as_deref())
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
                    self.record_runtime_event(&execution_id, backend, event);
                }
                if !has_output_event {
                    // Some backends only return final text. Synthesize an Output event so
                    // event consumers see a consistent shape.
                    let output_text = &output.text;
                    tracing::info!(
                        execution_id = execution_id.as_deref(),
                        output_len = output_text.len(),
                        "output.final"
                    );
                    self.record_runtime_event(
                        &execution_id,
                        backend,
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
                tracing::info!(
                    total_ms = output.timing.total_ms,
                    prompt_ms = output.timing.prompt_ms,
                    "execution.completed"
                );
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
                    backend,
                    RuntimeEvent::Error(ErrorEvent {
                        message: err.to_string(),
                        code: None,
                        data: None,
                    }),
                );
                self.mark_execution_finished(&execution_id, ExecutionStatus::Failed);
                tracing::warn!(error = %err, "execution.failed");
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

    fn finalize_local_turn(
        &mut self,
        backend: AcpBackend,
        execution_id: &Option<String>,
        request_hash: &str,
        prompt: &str,
        text: String,
        events: Vec<RuntimeEvent>,
    ) -> AcpPromptOutput {
        self.mark_execution_finished(execution_id, ExecutionStatus::Completed);
        self.record_ledger_turn(
            backend,
            execution_id.as_deref(),
            request_hash,
            &text,
            ExecutionStatus::Completed.as_str(),
        );
        self.last_used_backend = Some(backend);
        self.working_memory.push_turn(backend, prompt, &text);
        let mut output = AcpPromptOutput::synthetic(text);
        output.execution_id = execution_id.clone();
        output.events = events;
        output
    }
}
