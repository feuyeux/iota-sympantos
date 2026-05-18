# iota-sympantos — slash commands

Type `/` at the start of a message in the iota composer to use a slash command.
Commands must be on a single line. Text after the command name is treated as
arguments.

All five backends are supported:

| Backend | Reference |
| --- | --- |
| Claude Code | <https://docs.anthropic.com/en/docs/claude-code/slash-commands> |
| Codex | <https://openai.github.io/codex/reference/slash-commands> |
| Gemini CLI | <https://google-gemini.github.io/gemini-cli/docs/cli/commands.html> |
| Hermes Agent | <https://nousresearch.com/hermes-agent/docs/slash-commands> |
| OpenCode | <https://opencode.ai/docs/slash-commands> |

## Local commands

These commands are handled by iota. They work identically on all five backends.

| Command | Aliases | What it does |
| --- | --- | --- |
| `/?` | — | Show iota TUI help. On Claude Code, Codex, and OpenCode, `/help` is treated the same as `/?`. |
| `/backend` | `/backends` | List enabled backends. |
| `/backend <name>` | — | Switch to a backend by name or alias. |
| `/claude` | — | Switch to Claude Code. |
| `/codex` | — | Switch to Codex. |
| `/gemini` | — | Switch to Gemini. |
| `/hermes` | — | Switch to Hermes. |
| `/opencode` | — | Switch to OpenCode. |
| `/clear` | `/new`, `/reset` | Clear the visible transcript. |
| `/model` | `/models` | Show the model configured for the active backend. |
| `/status` | `/stats`, `/usage`, `/about`, `/profile` | Show the active backend and model. |
| `/export` | `/save` | Save the current transcript to a file. |
| `/quit` | `/exit` | Open the quit confirmation prompt. |
| `/q` | — | Quit confirmation (OpenCode session only). |

Backend switching respects the enabled backends in `~/.i6/nimia.yaml`. If the
target backend is disabled, iota shows a notice and stays on the current backend.

## Provider passthrough commands

For commands that belong to a specific backend, iota sends the slash text
directly to that backend via ACP. Aliases are resolved to their canonical name
before forwarding — for example `/compress` is sent as `/compact` — so that the
backend's own slash handler always receives the name it registers against.

**Only backends with confirmed ACP slash handling are listed.** Claude Code and
Codex do **not** intercept slash commands from the ACP `session/prompt` text:
Claude Code's ACP server passes the text to the LLM, which then explains the
command as prose rather than executing it; Codex's ACP server shows the same
behaviour in testing. Custom slash commands (e.g. `.claude/commands/*.md`) still
reach those backends via the dynamic passthrough below.

| Command | Aliases | Supported backends |
| --- | --- | --- |
| `/help` | — | Hermes, Gemini |
| `/init` | — | Gemini |
| `/compact` | `/compress` | Hermes, OpenCode |
| `/memory` | — | Gemini |
| `/queue` | — | Hermes |
| `/steer` | — | Hermes |
| `/tools` | — | Hermes |
| `/rollback` | `/restore` | Gemini |
| `/extensions` | — | Gemini |

## Dynamic passthrough

Any slash command that iota does not recognise — including commands not in the
table above — is forwarded to the active backend as a plain prompt. The backend
receives the text and handles it according to its own logic. This makes backend
extension surfaces available without iota needing an explicit entry for each one:

- Claude Code custom slash commands (from `.claude/commands/` or bundled skills)
- Codex skills and inline commands
- Hermes quick commands
- Gemini file-based and MCP-backed custom commands
- OpenCode project commands

For example, `/my-command arg1 arg2` is recorded as a user message and submitted
to the active backend as text.

## Examples

### `/help` — backend-native on Hermes and Gemini

On **Hermes** and **Gemini**, `/help` is forwarded via ACP to the backend's own
command handler, which returns its native help output.

On **Claude Code**, **Codex**, and **OpenCode**, `/help` cannot be triggered
through the ACP text prompt (it is a TUI-level command only). Instead, iota
shows its own built-in help overlay — the same as `/?`.

```
/help
```

### `/compact` — passthrough command, Hermes and OpenCode

`/compact` (alias `/compress`) compresses the context history of the active
session. Confirmed ACP handling on Hermes and OpenCode.

```
/compact
```

With an optional instruction:

```
/compact focus on the last task only
```

iota resolves the alias to the canonical name before forwarding, so typing
`/compress` sends `/compact` to the backend. On Claude Code and Codex the ACP
layer does not intercept slash commands from text prompts — the LLM receives the
text and explains the command instead of executing it. On Gemini, `/compact` has
no registered ACP handler and is treated as plain text by the LLM.
