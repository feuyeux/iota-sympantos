use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::state::ConversationEntry;
use super::{Overlay, TuiApp};

pub(super) enum LoopSignal {
    Continue,
    Break,
}

impl TuiApp {
    pub(super) async fn on_key_event(&mut self, key: KeyEvent) -> LoopSignal {
        if let Some(signal) = self.handle_approval_overlay_key(key) {
            return signal;
        }
        if let Some(signal) = self.handle_quit_confirm_overlay_key(key) {
            return signal;
        }

        let signal = self.handle_global_key(key).await;
        if matches!(signal, LoopSignal::Break) {
            return signal;
        }

        self.post_key_housekeeping();
        LoopSignal::Continue
    }

    fn handle_approval_overlay_key(&mut self, key: KeyEvent) -> Option<LoopSignal> {
        self.pending_approval.as_ref()?;
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => Some(LoopSignal::Break),
            (KeyModifiers::NONE, KeyCode::Char('y')) | (KeyModifiers::NONE, KeyCode::Char('Y')) => {
                if let Some(req) = self.pending_approval.take() {
                    let _ = req.reply.send(true);
                    self.record_entry(ConversationEntry::ToolResult {
                        name: req.tool_name,
                        ok: true,
                        text: "approved".to_string(),
                    });
                }
                Some(LoopSignal::Continue)
            }
            (KeyModifiers::NONE, KeyCode::Char('n'))
            | (KeyModifiers::NONE, KeyCode::Char('N'))
            | (KeyModifiers::NONE, KeyCode::Esc) => {
                if let Some(req) = self.pending_approval.take() {
                    let _ = req.reply.send(false);
                    self.record_entry(ConversationEntry::ToolResult {
                        name: req.tool_name,
                        ok: false,
                        text: "denied".to_string(),
                    });
                }
                Some(LoopSignal::Continue)
            }
            _ => Some(LoopSignal::Continue),
        }
    }

    fn handle_quit_confirm_overlay_key(&mut self, key: KeyEvent) -> Option<LoopSignal> {
        if self.overlay != Overlay::QuitConfirm {
            return None;
        }
        if matches!(
            (key.modifiers, key.code),
            (KeyModifiers::CONTROL, KeyCode::Char('c'))
        ) {
            return Some(LoopSignal::Break);
        }
        self.overlay = Overlay::None;
        self.quit_confirm_tick = None;
        Some(LoopSignal::Continue)
    }

    async fn handle_global_key(&mut self, key: KeyEvent) -> LoopSignal {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                if self.quit_confirm_tick.is_some() {
                    return LoopSignal::Break;
                }
                self.quit_confirm_tick = Some(self.tick_count);
                self.overlay = Overlay::QuitConfirm;
            }
            (KeyModifiers::NONE, KeyCode::Esc) => {
                if self.running_turn {
                    if let Some(handle) = self.turn_task.take() {
                        handle.abort();
                    }
                    self.engine.lock().await.shutdown_open_clients().await;
                    self.running_turn = false;
                    self.streaming_text.clear();
                    self.streaming_version
                        .set(self.streaming_version.get().wrapping_add(1));
                    self.streaming_backend = None;
                    self.turn_started_at = None;
                    self.record_entry(ConversationEntry::SystemNotice {
                        text: "Interrupted".into(),
                    });
                }
                self.overlay = Overlay::None;
            }
            (KeyModifiers::NONE, KeyCode::Char('?')) => {
                self.help_requested = true;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('b')) => {
                self.cycle_backend();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
                self.history.entries.clear();
                self.pending_scrollback_entries.clear();
                self.record_entry(ConversationEntry::SystemNotice {
                    text: "Transcript cache cleared (terminal scrollback preserved)".into(),
                });
            }
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => match self.export_history() {
                Ok(path) => {
                    self.record_entry(ConversationEntry::SystemNotice {
                        text: format!("Exported to {}", path.display()),
                    });
                }
                Err(err) => {
                    self.record_entry(ConversationEntry::SystemNotice {
                        text: format!("Export failed: {}", err),
                    });
                }
            },
            (KeyModifiers::NONE, KeyCode::Tab) => {
                if self.running_turn && !self.composer.text.trim().is_empty() {
                    self.submit();
                }
            }
            _ => {
                let action = self.composer.handle_key(key);
                if matches!(action, super::input::ComposerAction::Submit) {
                    self.submit();
                }
            }
        }
        LoopSignal::Continue
    }

    fn post_key_housekeeping(&mut self) {
        if self.overlay != Overlay::QuitConfirm {
            self.quit_confirm_tick = None;
        }
        if let Some(t) = self.quit_confirm_tick
            && self.tick_count.saturating_sub(t) > 25
        {
            self.overlay = Overlay::None;
            self.quit_confirm_tick = None;
        }
    }
}
