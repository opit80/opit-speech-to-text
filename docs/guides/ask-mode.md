# Ask mode in Opit Speech to Text

Ask is an optional feature of Opit Speech to Text that records a request, asks an AI agent,
and pastes its answer at the cursor or leaves it on the clipboard.

## Setup

Expand **Settings → Advanced → Ask**. Select one agent:

- **OpenAI-compatible chat:** set the chat endpoint and model. Save an API key if your service
  requires one. The Ask key uses its own `ask` reference in Windows Credential Manager and is
  separate from transcription and cleanup keys. Local endpoints can run without a key.
- **Claude Code CLI:** install `claude` on PATH and complete its authentication separately.
  Choose a model alias, such as `sonnet`. Opit uses print mode with JSON output.
- **Codex CLI:** install `codex` on PATH and complete its authentication separately. Set a model
  or leave it empty for the CLI default. Opit uses `exec` with JSON events and an explicit session
  id when resuming. A CLI version supporting these flags is required; unsupported flags fail closed.

The installed indicator resolves native executables and Windows `.cmd`/`.bat` shims on PATH.
Restart Opit after changing PATH. Only install CLI tools from a source you trust. Windows shims require a local working directory;
UNC working directories are rejected because cmd.exe cannot retain them.

The working directory defaults to your user home. A configured directory must exist and have an
absolute path. The timeout defaults to 180 seconds (valid range: 1–3600 seconds).
Timeout, conversation continuation minutes and request cleanup are no longer shown
in Settings; their saved values remain unchanged when the Ask form is saved.

Assign an optional Ask shortcut, or leave it empty and use **Ask…** in the tray menu. Shortcuts
cannot overlap recording or pause shortcuts; Escape is reserved for cancellation. Ask uses a
toggle gesture even when ordinary dictation uses push-to-talk: press again, use the ordinary
stop gesture, choose Stop, or let the recording limit finish. Pause/resume and the bounded
recording queue also apply. The overlay shows the agent and elapsed processing time.

The **Test agent** button sends only a fixed request for the word `OK`, without using the
microphone, clipboard, History or the current conversation. It may incur provider charges.

## CLI

```powershell
opit ask
# In a second terminal, or using the recording stop shortcut:
opit stop

opit ask --text "What is the PowerShell command to list open ports?"
```

Both commands wait for the answer, print it, and use the configured paste/copy policy. `--text`
skips microphone capture and transcription completely. Requests are bounded to 12 KiB. The
existing per-user named pipe carries the request to the running app. Avoid placing sensitive
requests in shell history: the app passes CLI-agent requests through stdin, but the initial
`opit ask --text` invocation is still visible to the caller's shell and process inspection.

## Permissions and cancellation

Restricted mode is the default. Claude has no tools, no loaded user/project setting sources,
no MCP servers, disabled hooks and disabled slash commands. Workspace mode makes only its
read/search/edit/write tools available, with edit acceptance enabled; shell tools stay denied.
Codex uses a read-only sandbox with shell, unified execution and JavaScript REPL tools disabled,
web search and startup hooks disabled and approvals set to never. User configuration and rules are not loaded.
Workspace mode changes Codex to its workspace-write sandbox and permits its normal shell tools.
It can execute commands and modify files in the working directory without confirmation.
Only enable broader permissions when you intend these effects and trust the directory.

Opit spawns an executable with an argument vector; the request and the original answer-style
instruction travel only through stdin. Model settings and session ids are validated. For a
Windows shim, `cmd.exe` is used with AutoRun and delayed expansion disabled; shell metacharacters
in the shim path are rejected. No request is interpolated into a shell command.

Each child starts suspended, is assigned to a Windows Job Object with kill-on-close enabled,
and then resumes. Timeout or Escape terminates the entire job, including descendants. The
queue waits for cancellation cleanup before proceeding from an active agent stage. Opit does
not display CLI stderr or provider response bodies in errors. Unsupported CLI versions return
a safe error; Opit never silently retries with weaker permissions. CLI execution is supported
on Windows only.

## Conversation, privacy and cost

Requests completed less than ten minutes ago continue by default. Chat retains at most four
request/answer pairs within a bounded byte budget, in RAM only. Claude resumes its explicit
session id; Codex uses `exec resume` with its explicit thread id. Expiration, configuration
changes and **New conversation** in the tray start a fresh conversation. A saved continue
window of zero starts fresh each time. Resetting does not delete CLI-owned session files.

Spoken audio first goes to your active transcription service, following its normal retry and
fallback policy. With optional request cleanup enabled, the request and glossary also go to
your configured cleanup service. Otherwise cleanup is skipped for Ask, even if it is enabled
for ordinary dictation. The recognized request goes to your chosen chat endpoint or CLI agent.
CLI tools use their own credentials, policies and storage and may retain conversations on disk.
The request includes an original instruction to answer briefly in the user's language, return
paste-ready text, and avoid Markdown fences unless code is requested. Opit does not automatically
read the clipboard or selected paragraph; include the necessary text in your request.

With History enabled, Opit stores the original recognized or typed request as raw text and the
answer as final text, marked `ask` with the agent name. The History request/answer toggle shows
both distinctly. Existing rows remain dictations after migration. Failed or cancelled Ask requests are not offered
as ordinary dictation retries. Disable History if you do
not want Opit to retain these. Save audio and the existing retention/cap settings still apply.
App logs and error messages contain neither request nor answer nor API keys.

Paste respects the existing foreground safety checks. Stopping from the tray or Opit's own
window leaves the answer on the clipboard. Copy-only mode never restores an old clipboard over
the answer. Answers are not passed through dictation rules or cleanup and do not get an added
trailing space. Provider token fees, transcription/cleanup fees and CLI subscription or API
charges are billed by the selected services. Conversation context can increase input token cost.
