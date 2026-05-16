use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use super::state::ConversationEntry;
use super::{Overlay, TuiApp};

pub(super) enum LoopSignal {
    Continue,
    Break,
}

impl TuiApp {
    pub(super) fn on_mouse_event(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.history.scroll_up(3),
            MouseEventKind::ScrollDown => self.history.scroll_down(3),
            _ => {}
        }
    }

    pub(super) async fn on_key_event(&mut self, key: KeyEvent) -> LoopSignal {
        if let Some(signal) = self.handle_approval_overlay_key(key) {
            return signal;
        }
        if let Some(signal) = self.handle_pager_overlay_key(key) {
            return signal;
        }
        if let Some(signal) = self.handle_help_overlay_key() {
            return signal;
        }
        if let Some(signal) = self.handle_backend_selector_overlay_key(key) {
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
                    self.history.push(ConversationEntry::ToolResult {
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
                    self.history.push(ConversationEntry::ToolResult {
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

    fn handle_pager_overlay_key(&mut self, key: KeyEvent) -> Option<LoopSignal> {
        let Overlay::Pager { ref mut scroll } = self.overlay else {
            return None;
        };
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Char('q'))
            | (KeyModifiers::NONE, KeyCode::Esc)
            | (KeyModifiers::CONTROL, KeyCode::Char('t')) => {
                self.overlay = Overlay::None;
            }
            (KeyModifiers::NONE, KeyCode::Char('j'))
            | (KeyModifiers::NONE, KeyCode::Down)
            | (KeyModifiers::NONE, KeyCode::PageDown) => {
                *scroll = scroll.saturating_add(5);
            }
            (KeyModifiers::NONE, KeyCode::Char('k'))
            | (KeyModifiers::NONE, KeyCode::Up)
            | (KeyModifiers::NONE, KeyCode::PageUp) => {
                *scroll = scroll.saturating_sub(5);
            }
            (KeyModifiers::NONE, KeyCode::Home) => {
                *scroll = 0;
            }
            (KeyModifiers::NONE, KeyCode::End) => {
                *scroll = usize::MAX;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(LoopSignal::Break),
            _ => {}
        }
        Some(LoopSignal::Continue)
    }

    fn handle_help_overlay_key(&mut self) -> Option<LoopSignal> {
        if self.overlay != Overlay::Help {
            return None;
        }
        self.overlay = Overlay::None;
        Some(LoopSignal::Continue)
    }

    fn handle_backend_selector_overlay_key(&mut self, key: KeyEvent) -> Option<LoopSignal> {
        let Overlay::BackendSelector { selected } = self.overlay else {
            return None;
        };

        let enabled = self.enabled_backends();
        let mut new_selected = selected;
        let mut should_close = false;
        let mut should_switch = None;

        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('b')) => {
                should_close = true;
            }
            (KeyModifiers::NONE, KeyCode::Up) => {
                new_selected = new_selected.saturating_sub(1);
            }
            (KeyModifiers::NONE, KeyCode::Down) => {
                new_selected = (new_selected + 1).min(enabled.len().saturating_sub(1));
            }
            (KeyModifiers::NONE, KeyCode::Enter) => {
                if let Some(&backend) = enabled.get(new_selected) {
                    should_switch = Some(backend);
                }
                should_close = true;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return Some(LoopSignal::Break),
            _ => {}
        }

        if should_close {
            self.overlay = Overlay::None;
        } else {
            self.overlay = Overlay::BackendSelector {
                selected: new_selected,
            };
        }

        if let Some(backend) = should_switch {
            self.switch_backend(backend);
        }

        Some(LoopSignal::Continue)
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
                    self.history.push(ConversationEntry::SystemNotice {
                        text: "Interrupted".into(),
                    });
                }
                self.overlay = Overlay::None;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('t')) => {
                self.overlay = Overlay::Pager { scroll: usize::MAX };
            }
            (KeyModifiers::NONE, KeyCode::Char('?')) => {
                self.overlay = Overlay::Help;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('b')) => {
                let enabled = self.enabled_backends();
                if enabled.len() > 1 {
                    let selected = enabled
                        .iter()
                        .position(|&b| b == self.active_backend)
                        .unwrap_or(0);
                    self.overlay = Overlay::BackendSelector { selected };
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
                self.history.entries.clear();
                self.history.scroll_to_bottom();
            }
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => match self.export_history() {
                Ok(path) => {
                    self.history.push(ConversationEntry::SystemNotice {
                        text: format!("Exported to {}", path.display()),
                    });
                }
                Err(err) => {
                    self.history.push(ConversationEntry::SystemNotice {
                        text: format!("Export failed: {}", err),
                    });
                }
            },
            (KeyModifiers::NONE, KeyCode::PageUp) => {
                self.history.scroll_up(10);
            }
            (KeyModifiers::NONE, KeyCode::PageDown) => {
                self.history.scroll_down(10);
            }
            (KeyModifiers::CONTROL, KeyCode::Char('d')) | (KeyModifiers::NONE, KeyCode::End) => {
                self.history.scroll_to_bottom();
            }
            (KeyModifiers::NONE, KeyCode::Tab) => {
                if self.running_turn && !self.composer.text.trim().is_empty() {
                    self.submit();
                }
            }
            _ => {
                let action = self.composer.handle_key(key);
                match action {
                    super::composer::ComposerAction::Submit => self.submit(),
                    super::composer::ComposerAction::ScrollHistory => {
                        if key.code == KeyCode::Up {
                            self.history.scroll_up(1);
                        } else {
                            self.history.scroll_down(1);
                        }
                    }
                    _ => {}
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
