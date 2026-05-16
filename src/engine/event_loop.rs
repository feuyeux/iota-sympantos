use crate::acp::{self, AcpBackend};
use crate::runtime_event::RuntimeEvent;
use crate::store::cache::ExecutionStatus;
use crate::telemetry::metrics;

use super::IotaEngine;

impl IotaEngine {
    /// Persist or count the runtime side effects represented by one engine event.
    pub(super) fn record_runtime_event(&self, _execution_id: &Option<String>, event: RuntimeEvent) {
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
    pub(super) fn log_engine_event(
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

    /// Mark an execution row terminal when cache persistence is enabled.
    pub(super) fn mark_execution_finished(
        &self,
        execution_id: &Option<String>,
        status: ExecutionStatus,
    ) {
        if let (Some(store), Some(execution_id)) = (&self.cache_store, execution_id) {
            let _ = store.finish_execution(execution_id, status);
        }
    }

    /// Mark an execution terminal and publish duration/count metrics.
    pub(super) fn mark_execution_finished_with_timing(
        &self,
        execution_id: &Option<String>,
        backend: AcpBackend,
        status: ExecutionStatus,
        timing: &acp::AcpPromptTiming,
    ) {
        self.mark_execution_finished(execution_id, status.clone());
        let m = metrics::get();
        let backend_attr = opentelemetry::KeyValue::new("backend", backend.to_string());
        m.prompt_duration.record(
            timing.prompt_ms as f64 / 1000.0,
            std::slice::from_ref(&backend_attr),
        );
        if let Some(init_ms) = timing.init_ms {
            m.init_duration
                .record(init_ms as f64 / 1000.0, &[backend_attr]);
        }
        let status_attr = opentelemetry::KeyValue::new("status", status.as_str().to_string());
        m.execution_count.add(1, &[status_attr]);
    }
}

pub(super) fn event_payload(event: &RuntimeEvent) -> serde_json::Value {
    match event {
        RuntimeEvent::Memory(memory) => memory.payload.clone(),
        other => serde_json::json!({"event_type": other.event_type()}),
    }
}
