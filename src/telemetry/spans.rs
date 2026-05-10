use opentelemetry::{
    Context, KeyValue, global,
    trace::{Span, SpanKind, Status, TraceContextExt, Tracer},
};

pub struct ExecutionSpan {
    context: Context,
}

impl ExecutionSpan {
    pub fn context(&self) -> Context {
        self.context.clone()
    }

    pub fn in_scope<T>(&self, f: impl FnOnce() -> T) -> T {
        let _guard = self.context.clone().attach();
        f()
    }

    pub fn set_attribute(&self, attribute: KeyValue) {
        self.context.span().set_attribute(attribute);
    }

    pub fn end_ok(&self) {
        self.context.span().set_status(Status::Ok);
        self.context.span().end();
    }

    pub fn end_error(&self, message: &str) {
        self.context
            .span()
            .set_status(Status::error(message.to_string()));
        self.context.span().end();
    }
}

pub fn start_execution_span(
    execution_id: &str,
    session_id: &str,
    backend: &str,
    request_hash: &str,
) -> ExecutionSpan {
    let tracer = global::tracer("iota");
    let span = tracer
        .span_builder("execution")
        .with_kind(SpanKind::Internal)
        .with_attributes(vec![
            KeyValue::new("iota.execution.id", execution_id.to_string()),
            KeyValue::new("iota.session.id", session_id.to_string()),
            KeyValue::new("iota.backend", backend.to_string()),
            KeyValue::new("iota.request.hash", request_hash.to_string()),
        ])
        .start(&tracer);
    ExecutionSpan {
        context: Context::current_with_span(span),
    }
}

pub fn start_phase_span(name: &str) -> opentelemetry::global::BoxedSpan {
    let tracer = global::tracer("iota");
    tracer
        .span_builder(name.to_string())
        .with_kind(SpanKind::Internal)
        .start_with_context(&tracer, &Context::current())
}

pub fn start_tool_span(tool_name: &str, tool_call_id: &str) -> opentelemetry::global::BoxedSpan {
    let tracer = global::tracer("iota");
    tracer
        .span_builder(format!("tool_call: {}", tool_name))
        .with_kind(SpanKind::Internal)
        .with_attributes(vec![
            KeyValue::new("iota.tool.name", tool_name.to_string()),
            KeyValue::new("iota.tool.call_id", tool_call_id.to_string()),
        ])
        .start_with_context(&tracer, &Context::current())
}

pub fn start_memory_span(operation: &str) -> opentelemetry::global::BoxedSpan {
    let tracer = global::tracer("iota");
    tracer
        .span_builder(format!("memory.{}", operation))
        .with_kind(SpanKind::Internal)
        .with_attributes(vec![KeyValue::new(
            "iota.memory.operation",
            operation.to_string(),
        )])
        .start_with_context(&tracer, &Context::current())
}

pub fn start_approval_span(tool_name: &str) -> opentelemetry::global::BoxedSpan {
    let tracer = global::tracer("iota");
    tracer
        .span_builder("approval")
        .with_kind(SpanKind::Internal)
        .with_attributes(vec![KeyValue::new("iota.tool.name", tool_name.to_string())])
        .start_with_context(&tracer, &Context::current())
}

pub fn end_span_ok(span: &mut impl Span) {
    span.set_status(Status::Ok);
    span.end();
}

pub fn end_span_error(span: &mut impl Span, message: &str) {
    span.set_status(Status::error(message.to_string()));
    span.end();
}
