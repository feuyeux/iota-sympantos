# Doc standardization implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring all non-`-zh` documentation into conformance with the language and style standard defined in `docs/superpowers/specs/2026-05-10-doc-standardization-design.md`.

**Architecture:** Each file is translated and reformatted independently, committed per file. No content is added or removed — this is a standardization pass, not a rewrite. The reference style baseline is `doc/observability.md`.

**Tech Stack:** Markdown, git

---

## Style rules (carry these into every task)

- **Language:** English throughout (body, headings, tables, code comments, list items)
- **Headings:** Sentence case (`## Core features`, not `## Core Features`)
- **Code block comments:** English, `# comment` style
- **List items:** Capitalize first word; no trailing period unless full sentence
- **Demo data exception:** Chinese strings inside command examples (e.g. `iota run codex "你的 prompt"`) are preserved as-is — they are data, not prose
- **Do not add, remove, or restructure content** — only translate and reformat to match the style rules

---

## File map

| File | Action |
|------|--------|
| `README.md` | Translate to English, apply style rules |
| `AGENTS.md` | Translate to English, apply style rules |
| `doc/architecture.md` | Translate to English, apply style rules |
| `doc/code-call-chains.md` | Translate to English, apply style rules |
| `doc/debugging.md` | Translate to English, apply style rules |
| `doc/observability.md` | Skip — already English baseline |
| `gefsi/exp01-memory.md` | Translate to English, apply style rules |
| `gefsi/exp02-skill-fun.md` | Translate to English, apply style rules |
| `gefsi/exp03-acp-runtime.md` | Translate to English, apply style rules |
| `doc/README-zh.md` | Skip — protected Chinese companion |

---

## Task 1: Standardize `README.md`

**Files:**
- Modify: `README.md`

The README is the highest-visibility document. Translate all prose to English. Apply Sentence case to all headings. Translate table headers and content. Preserve Chinese strings inside code block commands (demo data).

- [ ] **Step 1: Read the current file**

Open `README.md` and read it fully before making any changes.

- [ ] **Step 2: Rewrite the file in English**

Replace the full contents with an English version. Use `doc/observability.md` as the style reference. Key sections to carry over faithfully:
  - Top tagline (one-liner description)
  - Core features table
  - Architecture table + image reference
  - Documentation table
  - Feature lab table (gefsi experiments)
  - Quick start: build, config, run, observability sections
  - All code blocks — translate inline comments, preserve command content and Chinese demo strings

Headings:
```
# iota sympantos
## Core features
## Architecture
## Documentation
## Feature lab
## Quick start
### Build
### Configuration
### Running
### Observability
```

- [ ] **Step 3: Verify**

Read the file back and check:
- No Chinese prose remains (Chinese in code strings is fine)
- All headings are Sentence case
- Table headers are English
- Code block comments (if any) are English

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: translate README.md to English"
```

---

## Task 2: Standardize `AGENTS.md`

**Files:**
- Modify: `AGENTS.md`

AGENTS.md contains agent behavior instructions and contributor guidelines. Translate all prose. Apply style rules. Preserve code identifiers, file paths, and command examples exactly.

- [ ] **Step 1: Read the current file**

Open `AGENTS.md` and read it fully.

- [ ] **Step 2: Rewrite the file in English**

Translate all prose, headings, and table content to English. Apply Sentence case to headings. Translate any inline comments in code blocks. Do not change the structure, section order, or any code/command content.

- [ ] **Step 3: Verify**

- No Chinese prose remains
- Headings are Sentence case
- Code blocks, file paths, identifiers are unchanged

- [ ] **Step 4: Commit**

```bash
git add AGENTS.md
git commit -m "docs: translate AGENTS.md to English"
```

---

## Task 3: Standardize `doc/architecture.md`

**Files:**
- Modify: `doc/architecture.md`

Architecture document (~423 lines). Translate all prose, section headings, table content, and ASCII diagram labels. Preserve all code identifiers, module names, and file paths exactly.

- [ ] **Step 1: Read the current file**

Open `doc/architecture.md` and read it fully.

- [ ] **Step 2: Rewrite the file in English**

Translate all prose and table content. Apply Sentence case to headings. For ASCII diagrams or inline diagram labels that contain Chinese, translate those labels to English. Preserve module names (`engine.rs`, `acp/`, etc.) verbatim.

Cross-link references in the file (`[code-call-chains.md](code-call-chains.md)` etc.) — keep the link text and target unchanged.

- [ ] **Step 3: Verify**

- No Chinese prose remains
- Headings are Sentence case
- All file paths, module names, and identifiers are unchanged
- Cross-document links are intact

- [ ] **Step 4: Commit**

```bash
git add doc/architecture.md
git commit -m "docs: translate doc/architecture.md to English"
```

---

## Task 4: Standardize `doc/code-call-chains.md`

**Files:**
- Modify: `doc/code-call-chains.md`

Largest doc file (~1027 lines). Contains call chain diagrams in text/ASCII format with Chinese labels. Translate all prose, headings, table content, and diagram labels. Preserve function names, module paths, and code identifiers exactly.

- [ ] **Step 1: Read the current file**

Open `doc/code-call-chains.md` and read it fully. Pay attention to the call chain blocks — they mix code identifiers (keep) with Chinese description labels (translate).

- [ ] **Step 2: Rewrite the file in English**

Translate all prose, section headings, and inline Chinese labels in call chain diagrams. Apply Sentence case to headings. For call chain blocks like:

```
cli::run()
  -> telemetry::init()   # 初始化 OTel
  -> match first arg:
       "run"   -> ACP prompt 路径
```

Translate the Chinese comments/labels:
```
cli::run()
  -> telemetry::init()   # initialize OTel
  -> match first arg:
       "run"   -> ACP prompt path
```

- [ ] **Step 3: Verify**

- No Chinese prose or labels remain
- All function names, module paths, type names are unchanged
- Headings are Sentence case
- Cross-document links are intact

- [ ] **Step 4: Commit**

```bash
git add doc/code-call-chains.md
git commit -m "docs: translate doc/code-call-chains.md to English"
```

---

## Task 5: Standardize `doc/debugging.md`

**Files:**
- Modify: `doc/debugging.md`

Debugging guide (~136 lines). Translate all prose, headings, and table content. Preserve VS Code configuration keys, extension IDs, and command examples exactly.

- [ ] **Step 1: Read the current file**

Open `doc/debugging.md` and read it fully.

- [ ] **Step 2: Rewrite the file in English**

Translate all prose and table content. Apply Sentence case to headings. Translate table rows that describe launch configurations (the config name column may stay in English if already English; translate the description column). Preserve all JSON/YAML config values, extension IDs, and CLI commands verbatim.

- [ ] **Step 3: Verify**

- No Chinese prose remains
- Headings are Sentence case
- VS Code config keys, extension IDs, CLI commands unchanged

- [ ] **Step 4: Commit**

```bash
git add doc/debugging.md
git commit -m "docs: translate doc/debugging.md to English"
```

---

## Task 6: Standardize `gefsi/exp01-memory.md`

**Files:**
- Modify: `gefsi/exp01-memory.md`

Historical experiment report (~716 lines). Mixed language — English status note at top, Chinese body. Translate all Chinese prose to English. The existing English status note at the top is already standard — keep it, do not rewrite it.

- [ ] **Step 1: Read the current file**

Open `gefsi/exp01-memory.md` and read it fully. Note which sections are already English (the `Status note:` block) and which are Chinese.

- [ ] **Step 2: Rewrite the file in English**

Translate all Chinese prose, headings, table headers and content, list items, and code block comments. Apply Sentence case to headings. Preserve:
- The existing English `Status note:` paragraph verbatim
- All SQL queries, CLI commands, and code blocks (translate inline comments only)
- Chinese strings inside commands that are demo data (e.g. experiment prompt strings)

For the metadata table at the top:
```markdown
| Field | Value |
|-------|-------|
| Experiment ID | exp01-memory |
| Date | 2026-05-07 |
...
```
Translate the field names (left column) to English; values stay as-is.

- [ ] **Step 3: Verify**

- No Chinese prose remains
- Headings are Sentence case
- SQL and CLI commands unchanged
- Status note at top is intact

- [ ] **Step 4: Commit**

```bash
git add gefsi/exp01-memory.md
git commit -m "docs: translate gefsi/exp01-memory.md to English"
```

---

## Task 7: Standardize `gefsi/exp02-skill-fun.md`

**Files:**
- Modify: `gefsi/exp02-skill-fun.md`

Experiment report (~305 lines). Same pattern as exp01.

- [ ] **Step 1: Read the current file**

Open `gefsi/exp02-skill-fun.md` and read it fully.

- [ ] **Step 2: Rewrite the file in English**

Apply the same approach as Task 6:
- Translate all Chinese prose, headings, table content
- Apply Sentence case to headings
- Preserve the existing English `Status note:` paragraph verbatim
- Preserve all code/command content; translate only inline Chinese comments

- [ ] **Step 3: Verify**

- No Chinese prose remains
- Headings are Sentence case
- Code blocks and commands unchanged

- [ ] **Step 4: Commit**

```bash
git add gefsi/exp02-skill-fun.md
git commit -m "docs: translate gefsi/exp02-skill-fun.md to English"
```

---

## Task 8: Standardize `gefsi/exp03-acp-runtime.md`

**Files:**
- Modify: `gefsi/exp03-acp-runtime.md`

Experiment report (~315 lines). Same pattern as exp01/02.

- [ ] **Step 1: Read the current file**

Open `gefsi/exp03-acp-runtime.md` and read it fully.

- [ ] **Step 2: Rewrite the file in English**

Apply the same approach as Tasks 6 and 7.

- [ ] **Step 3: Verify**

- No Chinese prose remains
- Headings are Sentence case
- Code blocks and commands unchanged

- [ ] **Step 4: Commit**

```bash
git add gefsi/exp03-acp-runtime.md
git commit -m "docs: translate gefsi/exp03-acp-runtime.md to English"
```

---

## Task 9: Final conformance check

**Files:** All modified files

- [ ] **Step 1: Scan all modified files for remaining Chinese prose**

```bash
# Check for CJK characters across all modified docs
grep -rn --include="*.md" -P "[\x{4e00}-\x{9fff}]" \
  README.md AGENTS.md doc/architecture.md doc/code-call-chains.md \
  doc/debugging.md gefsi/exp01-memory.md gefsi/exp02-skill-fun.md \
  gefsi/exp03-acp-runtime.md
```

Any matches that appear inside backtick code spans or code blocks (demo data) are expected and acceptable. Any matches in prose are a bug — fix them.

- [ ] **Step 2: Check heading case**

Scan each file's headings. Every `#`/`##`/`###` line must be Sentence case. Fix any Title Case headings found.

- [ ] **Step 3: Commit final fixes (if any)**

```bash
git add -p  # stage only the conformance fixes
git commit -m "docs: fix remaining standardization issues"
```
