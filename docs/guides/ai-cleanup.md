# AI cleanup in Opit Speech to Text

AI cleanup is optional and off by default. It edits a transcript after the local
hallucination filter and deterministic rules. It removes meaningless fillers,
restarts and repetition, restores punctuation, and cautiously repairs obvious
recognition mistakes without translating or summarizing. Review important names,
numbers and facts: a model can still make mistakes.

## Set up

Expand **Settings → AI cleanup**, choose a service, enter its API key and choose **Save**
(or **Test cleanup**). Saving a newly entered key runs a connection test in the same step; if the
test succeeds, cleanup is turned on for you. A key-free connection (for example a local server
with **Needs API key** off) is tested and turned on the same way when you save it. A failed test
leaves the **Enable AI cleanup** switch as it was, and the Raw style skips the test, so it never
turns cleanup on. Saving or testing again with an already stored key does not turn cleanup back
on after you switched it off. To turn cleanup off, switch **Enable AI cleanup** off and choose
**Save**; that save skips the automatic test, including for a key-free connection.
The first-run wizard has no cleanup step and preserves existing cleanup settings.
Each transcription profile also has **Apply AI cleanup** under **More**, on by default; the global
setting must be enabled as well.

Baseten is the recommended fast preset:

- Base URL: `https://inference.baseten.co/v1`
- Model: `deepseek-ai/DeepSeek-V4.1-Flash-Fast`
- Key page: <https://app.baseten.co/settings/api_keys>
- Authentication: `Authorization: Bearer <API key>`

OpenRouter, Google AI Studio, DeepSeek, OpenAI, Groq, local and custom presets are
also available. URL, model and the key requirement appear for Custom and Local,
or when a saved key reference differs from the matched preset. Timeout and reasoning
also appear when reasoning is set. To customize a hosted service, choose Custom and
enter its endpoint and model. Hidden values remain saved and continue to apply;
changing service does not reset timeout or reasoning. Presets are starting values,
not promises of model availability or account access. Unknown model IDs
are accepted. The Google AI Studio preset uses `gemini-3.5-flash-lite` at
`https://generativelanguage.googleapis.com/v1beta/openai`; saved model selections
are kept unchanged. If the cleanup endpoint returns HTTP 404, Opit reports that
the model was not found or is unavailable and asks you to choose another model.
Reasoning effort is omitted by default for compatibility and speed;
use the lowest value the chosen service supports (for example `none`) if needed.
The total timeout is 8 seconds for fresh configurations; legacy configurations without
a saved timeout retain 15 seconds. Its valid range is 1–120 seconds.
Connection establishment has its own 3-second timeout. Cleanup makes one attempt,
without entering the transcription retry ladder.

**Test cleanup** sends a fixed short example, including when cleanup is off, and
shows before/after text. Entered keys are stored before testing or saving, then
cleared from the input. When **Test cleanup** stores a newly entered key, it also saves the
connection it tested and, if the test passes, turns cleanup on. A test without a new key changes
nothing: use **Save** to apply other form edits.

## What is sent and retained

The configured cleanup endpoint receives the transcript after local rules, the
editing instructions, and a size-limited glossary/context from personal rules
and `rules.prompt_context`. No audio is sent to cleanup.
This is an additional request to a service that may differ from the speech
provider. Its processing, retention and pricing policies apply; text processing
may incur charges. API keys are stored separately from transcription keys, with a distinct reference per
service preset, in Windows Credential Manager or, on Linux, the Secret Service; never in
`config.json`. The key is sent as a bearer
token. Disabling cleanup prevents cleanup requests; an eligible service connection
is warmed with `GET /models` while recording, without sending transcript or audio.

Turkish speech uses Turkish editing instructions. Other languages use English
instructions that explicitly preserve the original language. Provider-detected
language wins, followed by the successful transcription profile's language and
then the interface language. A plain-text custom system prompt replaces the
built-in instructions; the glossary is appended either way. Settings lists cleanup styles
with add and edit actions. Choose the active style on Home or from the tray. Each style editor puts its name and prompt first, with
model, cycling and shortcut options under **More**. Built-in styles provide
**Restore default prompt**. Overrides can change model behavior, so retain the meaning, language and instruction-isolation
requirements if writing your own prompt.

Empty, hallucinated and very short outputs (two words or fewer) skip cleanup.
After successful cleanup, a final term-casing pass preserves glossary spellings
such as `iOS` and `ox_lib` when local rules are enabled. Failed, empty, truncated
or excessively long answers keep the rules-applied transcript and show an amber
overlay with the failure kind. Escape during cleanup cancels the dictation before
paste. Cleanup time contributes to history latency.

When history is enabled, it keeps the speech provider's raw transcript and final
text, plus `cleanup_status` (`ok`, `failed`, `skipped`) and the attempted model.
Old rows have null cleanup metadata. Home and History indicate successful/failed
cleanup. History switches between raw and final text; copying uses the displayed
version. The pre-cleanup rules-applied intermediate text is not stored separately.

## Local model

Choose the local preset for an existing Ollama server at
`http://127.0.0.1:11434/v1`; select an installed chat-capable model (the preset
suggests `qwen3:4b`). For an existing llama.cpp server, change the base URL to
`http://127.0.0.1:8080/v1` and enter its served model ID. Turn **Needs API key** off
for a server without authentication. Saving the connection tests it and turns cleanup on
only if the test succeeds.
Opit does not install, download or launch local models. With a loopback endpoint,
cleanup stays on this computer; the speech provider still receives audio unless
the transcription profile also uses a local server. Prefer HTTPS for remote
endpoints. Plain HTTP sends transcript and any bearer key without transport
encryption.
