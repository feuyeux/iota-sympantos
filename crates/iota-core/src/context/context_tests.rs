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
        mcp_tools_available: false,
        workspace: None,
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
        mcp_tools_available: false,
        workspace: None,
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
    // Only turns 2 and 3 should be present, in chronological order.
    let rendered = buf.render(4000);
    assert!(!rendered.contains("turn one"));
    assert!(rendered.contains("turn two"));
    assert!(rendered.contains("turn three"));
    let pos2 = rendered.find("turn two").unwrap();
    let pos3 = rendered.find("turn three").unwrap();
    assert!(pos2 < pos3);
}

#[test]
fn working_memory_buffer_renders_chronologically() {
    let mut buf = WorkingMemoryBuffer::new(5);
    buf.push_turn(AcpBackend::Codex, "first", "one");
    buf.push_turn(AcpBackend::Codex, "second", "two");
    buf.push_turn(AcpBackend::Codex, "third", "three");
    let rendered = buf.render(4000);
    let idx_first = rendered.find("first").unwrap();
    let idx_second = rendered.find("second").unwrap();
    let idx_third = rendered.find("third").unwrap();
    assert!(idx_first < idx_second);
    assert!(idx_second < idx_third);
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
        mcp_tools_available: false,
        workspace: None,
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
        mcp_tools_available: false,
        workspace: None,
    });
    assert!(prompt.contains("<handoff>"));
    assert!(prompt.contains("auth module"));
}

#[test]
fn memory_tools_points_model_to_core_memory_taxonomy_skill() {
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
        prompt: "remember durable memory",
        memory: None,
        skills: None,
        working_memory: &working_memory,
        handoff: None,
        mcp_tools_available: true,
        workspace: None,
    });

    assert!(prompt.contains("iota-memory-taxonomy"));
    assert!(prompt.contains("iota_skill_load"));
    assert!(prompt.contains("iota_memory_write"));
    assert!(prompt.contains("Classification rules live only in `iota-memory-taxonomy`"));
}

#[test]
fn memory_tools_do_not_advertise_missing_mcp_tools() {
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
        prompt: "remember durable memory",
        memory: None,
        skills: None,
        working_memory: &working_memory,
        handoff: None,
        mcp_tools_available: false,
        workspace: None,
    });

    assert!(prompt.contains("Persistent memory MCP tools are not available"));
    assert!(!prompt.contains("iota_memory_write"));
    assert!(!prompt.contains("iota_skill_load"));
}

#[test]
fn minimal_context_still_includes_memory_write_contract() {
    let engine = ContextEngine {
        enabled: true,
        budgets: ContextBudgets::default(),
    };
    let working_memory = WorkingMemoryBuffer::new(2);
    let prompt = engine.compose_effective_prompt(ComposeInput {
        backend: AcpBackend::Hermes,
        cwd: Path::new("."),
        session_id: "s",
        model: None,
        prompt: "ping",
        memory: None,
        skills: None,
        working_memory: &working_memory,
        handoff: None,
        mcp_tools_available: true,
        workspace: None,
    });

    assert!(prompt.contains("<memory-tools>"));
    assert!(prompt.contains("iota_memory_write"));
    assert!(prompt.contains("iota_kanban_create_task"));
    assert!(prompt.contains("Do not say that information was remembered"));
}
