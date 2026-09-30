# Measuring accuracy with `opit-eval`

`opit-eval` sends a folder of recordings to a provider and reports word error rate (WER) and
term accuracy, with the rule layer **off** (raw provider output) and **on** (prompt + rules).
Use it to pick a default model and to check that a rule actually helps before keeping it.

## Build a dataset

1. Create a folder outside the repository (or `eval-data/`, which is git-ignored).
2. Record short clips the way you really dictate: 5–30 s each, your usual microphone,
   mixed Turkish and technical terms. Any WAV works (the tool resamples to 16 kHz mono).
3. Next to each `clip.wav`, write `clip.txt` with exactly what you said, spelled the way you
   want it pasted (`Claude Code'u GitHub'a pushla`).
4. 20–50 clips give a useful signal; include the terms your rule packs care about.

Recordings of your voice are personal data. Never commit them.

## Run

```sh
# Groq (default preset), rules on and off, built-in packs tr-core + tr-tech
GROQ_API_KEY=gsk_... cargo run --release -p opit-eval -- --dir eval-data

# Add FiveM terms, your personal pack and a prompt context
cargo run --release -p opit-eval -- --dir eval-data \
  --packs tr-core,tr-tech,fivem \
  --user-rules "$APPDATA/opit-speech-to-text/rules/user.yaml" \
  --context "Türkçe yazılım geliştirme ve FiveM sunucusu üzerine konuşma."

# Compare models
cargo run --release -p opit-eval -- --dir eval-data --model whisper-large-v3-turbo
OPENAI_API_KEY=sk-... cargo run --release -p opit-eval -- --dir eval-data --preset openai

# A self-hosted OpenAI-compatible server
cargo run --release -p opit-eval -- --dir eval-data --preset custom \
  --base-url http://192.168.1.10:8888/v1 --model large-v3
```

## Read the table

| Column | Meaning |
|---|---|
| WER off / on | Word error rate after case folding and punctuation removal; lower is better |
| terms off / on | Term occurrences spelled exactly as in the packs / occurrences in the reference |
| latency on | Request time in ms (median in the total row) |

A rule earns its place when "on" beats "off" on WER or terms without making another clip worse.
If a rule never fires on your dataset under the current model, delete it.
