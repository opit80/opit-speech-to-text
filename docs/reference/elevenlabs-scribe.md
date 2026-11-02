# ElevenLabs Scribe v2 in Opit Speech to Text

Choose **Profiles > Add profile > ElevenLabs Scribe v2**, paste the API key, save,
then select **Make active**. The existing Groq/OpenAI profiles remain available.
The key is stored in Windows Credential Manager; config only stores its reference.

The preset uses `provider: "elevenlabs"`, endpoint `https://api.elevenlabs.io/v1`,
model `scribe_v2`, Turkish (`tr`), mono WAV and plain JSON. Old profiles without a
`provider` field keep the OpenAI-compatible adapter. The first-run wizard also offers Scribe.

Native requests use `POST /speech-to-text` and `xi-api-key`, with temperature 0,
diarization and audio event tags off, and verbatim transcription retained. Local
correction rules still run after transcription. Whisper prompt context is not sent;
no transcript editing or other paid add-ons are requested by default.

**Send keywords** is an explicit opt-in to ElevenLabs keyterms, currently a 20%
API surcharge. Unsupported terms are omitted (50 or more characters, more than
five words, or prohibited characters); at most 1,000 terms are sent. More than 100
terms can also trigger a minimum billable duration of 20 seconds per request.
The preset keeps this feature off. API availability and free credits depend on the account.

**Test connection** and warm-up only perform authenticated `GET /user`; they never
upload audio or spend transcription credits. A restricted API key may need the
User Read permission for this check. A key limited to Speech to Text can still
transcribe even when the account read check fails; warm-up failures are ignored.
A successful check verifies authentication, not Speech to Text permissions or quota.

The native adapter rejects HTTP redirects so API keys and audio stay at the
configured endpoint, and does not include API response bodies in errors. Offline tests
use synthetic WAV data, keys and transcripts with a local mock HTTP server. Real
dictation accuracy, account quota and latency must be checked through normal use.

API contracts: [Create transcript](https://elevenlabs.io/docs/api-reference/speech-to-text/convert)
and [Get user](https://elevenlabs.io/docs/api-reference/user/get).
