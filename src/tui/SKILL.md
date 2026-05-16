# tui — Interactive Terminal UI

ratatui-based interactive chat interface with markdown rendering, multi-line composer, and backend selector.

## Layout

```
┌─ header ──────────────────── 1 row ─┐
│  magenta bg · cwd · active backend  │
├─ history ──────────────── fill rows ─┤
│  scrollable conversation transcript  │
├─ composer ─────────────── 3+ rows ──┤
│  multi-line input with cursor        │
├─ status ───────────────── 1 row ────┤
│  backend·model  /  key hints         │
└─────────────────────────────────────┘
```

## Features

- Multi-line input (Shift+Enter), Unicode grapheme cursor
- Kill buffer (Ctrl+K/Y), word delete (Ctrl+U/W), word motion (Alt+B/F)
- Ctrl+R incremental history search
- Markdown rendering (pulldown-cmark)
- Approval overlay for tool permission requests
- Ctrl+T full-screen pager, ? help overlay
- Tab queue (buffer input while backend is running)
- ~120 FPS frame limiter, mouse capture

## Sub-modules

| Module | Purpose |
|--------|---------|
| `composer` | Multi-line input with kill-buffer, word motion, history search |
| `events` | Keyboard/mouse event handling and dispatch |
| `loop_runtime` | Main event loop, async engine turn, frame timing |
| `markdown` | pulldown-cmark rendering to ratatui spans |
| `render` | Widget layout and drawing |
| `state` | TUI application state |
| `status_bar` | Bottom status bar (backend·model, key hints) |
| `terminal_lifecycle` | Terminal setup/restore RAII guard |
| `theme` | ratatui color theme (magenta accent) |
