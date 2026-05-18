use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{Event as CEvent, EventStream, KeyEventKind};
use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::acp::{AcpBackend, AcpPromptOutput};

use super::events::LoopSignal;
use super::state::{ConversationEntry, ObservabilityMeta};
use super::{ApprovalRequest, Terminal, TuiApp, TurnMessage, scrollback};

pub(super) async fn run_loop(
    terminal: &mut Terminal,
    app: &mut TuiApp,
    mut approval_rx: mpsc::Receiver<ApprovalRequest>,
) -> Result<()> {
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(80));
    let mut events = EventStream::new();

    // Frame rate limiter - skip redraw if we drew less than MIN_FRAME_MS ago.
    const MIN_FRAME_MS: u64 = 8; // ~120 fps cap
    let mut last_draw = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_millis(MIN_FRAME_MS))
        .unwrap_or(std::time::Instant::now());

    // engine_result carries the completed engine response back to the loop.
    // We use a one-shot channel so the engine future can be driven without
    // holding a mutable borrow on `app` across the terminal.draw() call.
    let (engine_tx, mut engine_rx) = mpsc::channel::<Result<(AcpBackend, AcpPromptOutput)>>(1);

    // pending_prompt is set by the submit path.
    let mut pending_prompt: Option<(AcpBackend, PathBuf, String)> = None;

    loop {
        app.run_loop_flush_pending_scrollback(terminal);
        run_loop_draw_if_due(terminal, app, &mut last_draw, MIN_FRAME_MS)?;
        app.run_loop_spawn_pending_prompt_if_any(&mut pending_prompt, &engine_tx);

        tokio::select! {
            _ = tick.tick() => {
                app.tick_count += 1;
            }

            // Streaming output chunks - drain all available, then redraw.
            Some(chunk) = app.stream_rx.recv(), if app.running_turn => {
                app.run_loop_handle_stream_chunk(chunk, &mut last_draw, MIN_FRAME_MS);
            }

            // Incoming approval requests from the ACP layer.
            Some(req) = approval_rx.recv() => {
                app.run_loop_handle_approval_request(req);
            }

            // Collect engine result.
            Some(result) = engine_rx.recv() => {
                app.run_loop_handle_engine_result(result);
            }

            // Pick up the internal submit signal from the channel.
            Some(msg) = app.turn_rx.recv() => {
                let TurnMessage::Prompt { backend, cwd, text } = msg;
                pending_prompt = Some((backend, cwd, text));
            }

            maybe_event = events.next() => {
                let Some(Ok(event)) = maybe_event else { break };
                if matches!(app.run_loop_handle_terminal_event(event).await, LoopSignal::Break) {
                    break;
                }
            }
        }
    }

    app.run_loop_teardown_turn_and_engine().await;
    Ok(())
}

fn run_loop_draw_if_due(
    terminal: &mut Terminal,
    app: &TuiApp,
    last_draw: &mut std::time::Instant,
    min_frame_ms: u64,
) -> Result<()> {
    let now = std::time::Instant::now();
    if now.duration_since(*last_draw).as_millis() as u64 >= min_frame_ms {
        terminal.draw(|f| app.render(f))?;
        *last_draw = now;
    }
    Ok(())
}

impl TuiApp {
    fn run_loop_spawn_pending_prompt_if_any(
        &mut self,
        pending_prompt: &mut Option<(AcpBackend, PathBuf, String)>,
        engine_tx: &mpsc::Sender<Result<(AcpBackend, AcpPromptOutput)>>,
    ) {
        let Some((backend, cwd, prompt)) = pending_prompt.take() else {
            return;
        };
        self.streaming_text.clear();
        self.streaming_version
            .set(self.streaming_version.get().wrapping_add(1));
        self.streaming_backend = Some(backend);
        let engine_arc = self.engine.clone();
        let stream_tx = self.stream_tx.clone();
        let engine_tx2 = engine_tx.clone();
        self.turn_task = Some(tokio::spawn(async move {
            let mut engine = engine_arc.lock().await;
            engine.set_stream_output_sender(Some(stream_tx));
            let result = engine.run_with_timing(backend, cwd, &prompt).await;
            engine.set_stream_output_sender(None);
            let _ = engine_tx2
                .send(result.map(|output| (backend, output)))
                .await;
        }));
    }

    fn run_loop_handle_stream_chunk(
        &mut self,
        chunk: String,
        last_draw: &mut std::time::Instant,
        min_frame_ms: u64,
    ) {
        self.streaming_text.push_str(&chunk);
        while let Ok(c) = self.stream_rx.try_recv() {
            self.streaming_text.push_str(&c);
        }
        self.streaming_version
            .set(self.streaming_version.get().wrapping_add(1));
        *last_draw = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_millis(min_frame_ms))
            .unwrap_or(std::time::Instant::now());
    }

    fn run_loop_handle_approval_request(&mut self, req: ApprovalRequest) {
        let tool_name = req.tool_name.clone();
        self.overlay = super::Overlay::None;
        self.pending_approval = Some(req);
        self.record_entry(ConversationEntry::SystemNotice {
            text: format!("Approval requested: {}", tool_name),
        });
    }

    fn run_loop_handle_engine_result(&mut self, result: Result<(AcpBackend, AcpPromptOutput)>) {
        self.turn_task = None;
        match result {
            Ok((backend, output)) => {
                let observability = observability_from_output(&output);
                self.latest_observability = Some(observability.clone());
                self.record_entry(ConversationEntry::AssistantMessage {
                    backend,
                    text: output.text,
                    observability: Some(observability),
                });
            }
            Err(err) => {
                self.record_entry(ConversationEntry::SystemNotice {
                    text: format!("Error: {}", err),
                });
            }
        }
        self.run_loop_reset_turn_state();

        if let Some(queued) = self.queued_prompt.take() {
            self.record_queued_prompt_delta(-1);
            self.record_entry(ConversationEntry::UserMessage {
                text: queued.clone(),
            });
            self.running_turn = true;
            self.turn_started_at = Some(std::time::Instant::now());
            self.send_turn_prompt(self.active_backend, self.cwd.clone(), queued);
        }
    }

    fn run_loop_reset_turn_state(&mut self) {
        self.running_turn = false;
        self.streaming_text.clear();
        self.streaming_version
            .set(self.streaming_version.get().wrapping_add(1));
        self.streaming_backend = None;
        self.turn_started_at = None;
    }

    async fn run_loop_teardown_turn_and_engine(&mut self) {
        if let Some(handle) = self.turn_task.take() {
            handle.abort();
        }
        self.engine.lock().await.shutdown_open_clients().await;
    }

    async fn run_loop_handle_terminal_event(&mut self, event: CEvent) -> LoopSignal {
        match event {
            CEvent::Key(key) if key.kind == KeyEventKind::Press => self.on_key_event(key).await,
            CEvent::Key(_) => LoopSignal::Continue,
            CEvent::Resize(_, _) => LoopSignal::Continue,
            _ => LoopSignal::Continue,
        }
    }

    fn run_loop_flush_pending_scrollback(&mut self, terminal: &mut Terminal) {
        if self.help_requested {
            let _ = scrollback::insert_help(terminal);
            self.help_requested = false;
        }
        for entry in self.pending_scrollback_entries.drain(..) {
            let _ = scrollback::insert_entry(terminal, &entry);
        }
    }
}

fn observability_from_output(output: &AcpPromptOutput) -> ObservabilityMeta {
    let token_usage = output.events.iter().rev().find_map(|event| match event {
        crate::runtime_event::RuntimeEvent::TokenUsage(usage) => Some(usage),
        _ => None,
    });
    ObservabilityMeta {
        execution_id: output.execution_id.clone(),
        total_ms: Some(output.timing.total_ms),
        prompt_ms: Some(output.timing.prompt_ms),
        input_tokens: token_usage.and_then(|usage| usage.input_tokens),
        cache_tokens: token_usage.and_then(|usage| usage.cache_tokens),
        cache_read_input_tokens: token_usage.and_then(|usage| usage.cache_read_input_tokens),
        cache_creation_input_tokens: token_usage
            .and_then(|usage| usage.cache_creation_input_tokens),
        output_tokens: token_usage.and_then(|usage| usage.output_tokens),
        thinking_tokens: token_usage.and_then(|usage| usage.thinking_tokens),
        total_tokens: token_usage.and_then(|usage| usage.total_tokens),
        provider_reported_total_tokens: token_usage
            .and_then(|usage| usage.provider_reported_total_tokens),
        normalized_total_tokens: token_usage.and_then(|usage| usage.normalized_total_tokens),
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::oneshot;

    use crate::config::NimiaConfig;

    use super::*;
    use crate::tui::Overlay;

    #[test]
    fn approval_request_closes_existing_overlay_so_prompt_is_visible() {
        let mut app = TuiApp::new(NimiaConfig::default()).unwrap();
        let (reply, _rx) = oneshot::channel();
        app.overlay = Overlay::QuitConfirm;

        app.run_loop_handle_approval_request(ApprovalRequest {
            tool_name: "shell".to_string(),
            params: serde_json::Value::Null,
            reply,
        });

        assert_eq!(app.overlay, Overlay::None);
        assert!(app.pending_approval.is_some());
    }
}
