# AI cleanup in Opit Speech to Text

AI cleanup is optional and off by default. It edits a transcript after the local
hallucination filter and deterministic rules. It removes meaningless fillers,
restarts and repetition, restores punctuation, and cautiously repairs obvious
recognition mistakes without translating or summarizing. Review important names,
numbers and facts: a model can still make mistakes.

## Set up

Open **Settings → AI cleanup**, or use the optional AI cleanup step after choosing
a transcription provider in first-run setup. Choose a service, enter its API key,
enable cleanup and save. **Skip AI cleanup** advances setup with cleanup off.
Each transcription profile also has **Apply AI cleanup**, on by default; the global
setting must be enabled as well.

Baseten is the recommended fast preset:

- Base URL: `https://inference.baseten.co/v1`
- Model: `deepseek-ai/DeepSeek-V4.1-Flash-Fast`
- Key page: <https://app.baseten.co/settings/api_keys>
- Authentication: `Authorization: Bearer <API key>`

OpenRouter, Google AI Studio, DeepSeek, OpenAI, Groq, local and custom presets are
also available. All endpoint and model values can be edited. Presets are starting
values, not promises of model availability or account access. Unknown model IDs
are accepted. Reasoning effort is omitted by default for compatibility and speed;
use the lowest value the chosen service supports (for example `none`) if needed.
The total timeout defaults to 15 seconds and can be set from 1 to 120 seconds.
Connection establishment has its own 3-second timeout. Cleanup makes one attempt,
without entering the transcription retry ladder.

**Test cleanup** sends a fixed short example, including when cleanup is off, and
shows before/after text. Entered keys are stored before testing or saving, then
cleared from the input. Testing does not enable cleanup or save other form edits.
Use **Save** (or **Next** in setup) to apply them.

## What is sent and retained

The configured cleanup endpoint receives the transcript after local rules, the
editing instructions, and a size-limited glossary/context from enabled built-in
packs, the personal pack and `rules.prompt_context`. No audio is sent to cleanup.
This is an additional request to a service that may differ from the speech
provider. Its processing, retention and pricing policies apply; text processing
may incur charges. API keys are stored separately from transcription keys in
Windows Credential Manager with a distinct reference per service preset, never in
`config.json`. The key is sent as a bearer
token. Disabling cleanup prevents cleanup requests; an eligible service connection
is warmed with `GET /models` while recording, without sending transcript or audio.

Turkish speech uses Turkish editing instructions. Other languages use English
instructions that explicitly preserve the original language. Provider-detected
language wins, followed by the successful transcription profile's language and
then the interface language. A plain-text custom system prompt replaces the
built-in instructions; the glossary is appended either way. Settings shows both
built-in prompts read-only and provides **Restore default prompt**. Overrides can
change model behavior, so retain the meaning, language and instruction-isolation
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
for a server without authentication. Use **Test cleanup** before enabling it.
Opit does not install, download or launch local models. With a loopback endpoint,
cleanup stays on this computer; the speech provider still receives audio unless
the transcription profile also uses a local server. Prefer HTTPS for remote
endpoints. Plain HTTP sends transcript and any bearer key without transport
encryption.
