use std::path::Path;

use crate::acp::AcpBackend;
use crate::memory::{MemoryInsert, MemoryScope, MemoryType};
use crate::utils::summarize;

use super::IotaEngine;

impl IotaEngine {
    /// Persist the backend-native session id after ACP returns it.
    pub(super) fn persist_backend_session_id(
        &self,
        backend: AcpBackend,
        cwd: &Path,
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
    pub(super) fn ensure_ledger_session(
        &self,
        backend: AcpBackend,
        cwd: &Path,
        model: Option<&str>,
    ) {
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
    pub(super) fn prepare_backend_handoff(
        &mut self,
        backend: AcpBackend,
        cwd: &Path,
    ) -> Option<String> {
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
    pub(super) fn record_ledger_turn(
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
}
