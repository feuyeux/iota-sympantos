use std::collections::VecDeque;

use crate::acp::AcpBackend;

#[derive(Debug, Clone, Default)]
pub struct ObservabilityMeta {
    pub execution_id: Option<String>,
    pub total_ms: Option<u64>,
    pub prompt_ms: Option<u64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

/// A single entry in the conversation history.
#[derive(Debug, Clone)]
pub enum ConversationEntry {
    UserMessage {
        text: String,
    },
    AssistantMessage {
        backend: AcpBackend,
        text: String,
        observability: Option<ObservabilityMeta>,
    },
    SystemNotice {
        text: String,
    },
    ToolResult {
        name: String,
        ok: bool,
        text: String,
    },
}

/// Scrollable conversation history, capped at `max_entries`.
pub struct HistoryState {
    pub entries: VecDeque<ConversationEntry>,
    max_entries: usize,
    /// How many rendered rows are scrolled up from the bottom (0 = stick to bottom).
    pub scroll_offset: usize,
}

impl HistoryState {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            max_entries,
            scroll_offset: 0,
        }
    }

    pub fn push(&mut self, entry: ConversationEntry) {
        self.entries.push_back(entry);
        while self.entries.len() > self.max_entries {
            self.entries.pop_front();
        }
        // If user was at the bottom, keep them there.
        if self.scroll_offset == 0 {
            // stay at bottom — nothing to do
        }
    }

    pub fn scroll_up(&mut self, rows: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(rows);
    }

    pub fn scroll_down(&mut self, rows: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(rows);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }
}
