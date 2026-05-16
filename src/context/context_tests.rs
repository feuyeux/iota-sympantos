use super::*;

#[test]
fn disabled_context_returns_prompt_unchanged() {
    let engine = ContextEngine {
        enabled: false,
        budgets: ContextBudgets::default(),
    };
    let working_memory = WorkingMemoryBuffer::new(2);
    let prompt = engine.compose_effective_prompt(ComposeInput {
        backend: AcpBackend::Codex,
        cwd: Path::new("."),
        session_id: "s",
        model: None,
        prompt: "ping",
        memory: None,
        skills: None,
        working_memory: &working_memory,
        handoff: None,
    });
    assert_eq!(prompt, "ping");
}

#[test]
fn enabled_context_wraps_prompt() {
    let engine = ContextEngine {
        enabled: true,
        budgets: ContextBudgets::default(),
    };
    let working_memory = WorkingMemoryBuffer::new(2);
    let prompt = engine.compose_effective_prompt(ComposeInput {
        backend: AcpBackend::Codex,
        cwd: Path::new("."),
        session_id: "s",
        model: Some("m"),
        prompt: "ping",
        memory: None,
        skills: None,
        working_memory: &working_memory,
        handoff: None,
    });
    assert!(prompt.contains("<iota-context>"));
    assert!(prompt.ends_with("ping"));
}

#[test]
fn working_memory_buffer_push_and_render() {
    let mut buf = WorkingMemoryBuffer::new(5);
    buf.push_turn(
        AcpBackend::Codex,
        "what is rust?",
        "Rust is a systems language.",
    );
    let rendered = buf.render(4000);
    assert!(rendered.contains("what is rust"));
    assert!(rendered.contains("systems language"));
}

#[test]
fn working_memory_buffer_evicts_oldest_when_full() {
    let mut buf = WorkingMemoryBuffer::new(2);
    buf.push_turn(AcpBackend::Codex, "turn one", "answer one");
    buf.push_turn(AcpBackend::Codex, "turn two", "answer two");
    buf.push_turn(AcpBackend::Codex, "turn three", "answer three");
    // Only turns 2 and 3 should be present.
    let rendered = buf.render(4000);
    assert!(!rendered.contains("turn one"));
    assert!(rendered.contains("turn two"));
    assert!(rendered.contains("turn three"));
}

#[test]
fn working_memory_buffer_budget_limits_output() {
    let mut buf = WorkingMemoryBuffer::new(10);
    for i in 0..10 {
        buf.push_turn(
            AcpBackend::Codex,
            &format!("question {}", i),
            &format!("answer {}", i),
        );
    }
    let small = buf.render(50);
    assert!(small.len() <= 50 + 200); // allow one line to be included
}

#[test]
fn context_with_model_includes_model_section() {
    let engine = ContextEngine {
        enabled: true,
        budgets: ContextBudgets::default(),
    };
    let working_memory = WorkingMemoryBuffer::new(2);
    let prompt = engine.compose_effective_prompt(ComposeInput {
        backend: AcpBackend::Codex,
        cwd: Path::new("."),
        session_id: "s",
        model: Some("gpt-4o"),
        prompt: "hi",
        memory: None,
        skills: None,
        working_memory: &working_memory,
        handoff: None,
    });
    assert!(prompt.contains("gpt-4o"));
}

#[test]
fn context_with_handoff_includes_handoff_section() {
    let engine = ContextEngine {
        enabled: true,
        budgets: ContextBudgets::default(),
    };
    let working_memory = WorkingMemoryBuffer::new(2);
    let prompt = engine.compose_effective_prompt(ComposeInput {
        backend: AcpBackend::Codex,
        cwd: Path::new("."),
        session_id: "s",
        model: None,
        prompt: "continue",
        memory: None,
        skills: None,
        working_memory: &working_memory,
        handoff: Some("Previous session: implemented auth module"),
    });
    assert!(prompt.contains("<handoff>"));
    assert!(prompt.contains("auth module"));
}
