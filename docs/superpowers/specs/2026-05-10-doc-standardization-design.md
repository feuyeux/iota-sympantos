# Doc standardization design

Date: 2026-05-10

## Goal

Establish a clear, enforceable language and style standard for all project documentation.
Translation of existing Chinese content is a secondary task that follows from the standard — not the goal itself.

---

## Standard

### Language rule

All documentation files, except those explicitly suffixed `-zh`, must be written in English.

This applies to: body text, headings, table contents, code block comments, list items, and image alt text.

**Exception — demo data:** User-facing prompt examples or command-line inputs that contain Chinese strings (e.g., `iota run codex "你的 prompt"`) are treated as data, not documentation prose. They are preserved as-is.

### Style conventions

The reference baseline is `doc/observability.md` — the repository's existing English document.

| Element | Convention |
|---------|------------|
| Headings | Sentence case (`## Core features`, not `## Core Features`) |
| Code block comments | English, matching the inline style of the surrounding code (`# comment`) |
| Table headers | English |
| List items | Capitalize first word; no trailing period unless the item is a full sentence |
| Inline code | Unchanged (paths, identifiers, flags stay as-is) |

### File naming

File names are not changed as part of this standard. The `-zh` suffix convention is the only naming rule in scope.

---

## Scope

### Protected files (do not modify)

| File | Reason |
|------|--------|
| `doc/README-zh.md` | Intentional Chinese-language companion document |

### Files that must conform to the standard

| File | Current state |
|------|---------------|
| `README.md` | Chinese — translate to English |
| `AGENTS.md` | Chinese — translate to English |
| `doc/architecture.md` | Chinese — translate to English |
| `doc/code-call-chains.md` | Chinese — translate to English |
| `doc/debugging.md` | Chinese — translate to English |
| `doc/observability.md` | Already English — no action needed |
| `gefsi/exp01-memory.md` | Mixed, Chinese-dominant — translate to English |
| `gefsi/exp02-skill-fun.md` | Mixed, Chinese-dominant — translate to English |
| `gefsi/exp03-acp-runtime.md` | Mixed, Chinese-dominant — translate to English |

---

## Enforcement

New documents added to the repository must follow this standard from the start.
The `-zh` suffix must be used for any intentional Chinese-language variant.

---

## Implementation order

1. Translate `README.md` — highest visibility, sets tone for the rest
2. Translate `AGENTS.md` — affects agent behavior and contributor onboarding
3. Translate `doc/` files — architecture, call chains, debugging (observability already done)
4. Translate `gefsi/` experiment reports — historical records, lower urgency
