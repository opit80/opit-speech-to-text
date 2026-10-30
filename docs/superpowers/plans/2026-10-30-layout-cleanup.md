# Layout Cleanup Implementation Plan

> **For agentic workers:** Tasks are executed by separate workers, one task at a time, each followed by a read-only review worker. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Execute layout move batches 1–11 without changing Windows behaviour, public contracts, installed-copy identity or test coverage; leave explicit seams for the separately planned Linux port.
**Architecture:** Move existing code mechanically into concern-owned modules, retaining compatibility facades where callers depend on them. A platform factory becomes the only startup backend constructor, while process supervision, hardware probing and local runtime metadata get explicit ownership. Documentation and UI imports move with their consumers; no Linux backend, capabilities API or UI redesign is implemented here.
**Tech Stack:** Rust 2024 workspace (minimum Rust 1.90), Tauri 2, Win32, cpal, Tokio, Svelte 5, TypeScript, Vite, Vitest, GitHub Actions.
**Spec:** docs/superpowers/specs/2026-10-30-layout-and-linux-design.md

## Global Constraints

- Layout batches change no behaviour and are not mixed with feature work.
- Layout batches keep today's Linux exclusion of the app crate until the port makes it compile.
- Windows cannot be built on the owner's Linux machine here. CI on Windows is the gate for each batch.
- **Workspace roots stay:** `crates/`, `ui/`, `rules/`, `scripts/`, `crates/app/windows/` and crate names do not change.
- No tag, release or publication in this work, and no version bump. Pushes go to the feature branch only.
- Non-goal: Rewriting git history.
- Installed-copy invariants that must not change: product name, identifier `io.github.opit80.opit-speech-to-text`, executable names, `APP_DIR_NAME`, credential service and suffix, autostart value, updater endpoint and public key, asset naming, signing key, and the `$UpdateMode` guards.
- Code, comments, docs and commit messages are in **English**.
- **Never log or put in error messages** transcript text or API keys.
- **No Tauri or Windows crates**; must build and test on Linux.
- Every user-facing UI string exists in both `ui/src/lib/i18n/en.ts` and `tr.ts` (parity is tested) — no literal user text in `.svelte` files.
- No new UI dependencies without a reason. CSP stays `default-src 'self'` (+ `media-src 'self' blob:`); no network requests from the UI.
- Commands reject with `CommandError { code, message, kind }`; keep `ui/src/lib/types.ts` / `api.ts` in sync with Rust command names and argument names (JS uses camelCase args).
- Blocking work (file I/O, keyring, SQLite, rule compilation) never runs on the Tauri main thread: such commands are `async` + `spawn_blocking`.
- UI config writes go through the serialized `saveConfig(mutate)` chain in `ui/src/lib/app.svelte.ts`; components never keep their own copy of `config` except a form draft.
- Pages must load their data on mount (closing the window destroys the WebView) and tolerate a dictation already running.
- UI: only components from `ui/src/lib/components/` and tokens from `ui/src/app.css` (no raw colours), native `<dialog>` + `showModal()`, every input labelled, keyboard reachable, WCAG AA in light and dark.
- The machine is often low on memory: run builds/tests one at a time, not in parallel.
- `hooks.nsh` / `uninstall.rs` "delete my data" must never run during an update or a silent reinstall — keep the `$UpdateMode` guard and its tests.
- Workers' sandbox blocks loopback sockets, so tests that bind sockets are run outside it; this is expected.
- **UI move before the owner's UI work,** to avoid conflicts. The owner holds UI edits until that batch lands.

## Review Focus

1. **Native insertion targets and clipboard restoration lose or misdirect a dictation.** Expected: stop-time target safety, FIFO ordering and restore-only-if-unchanged remain identical; a tray/UI stop stays clipboard-only. Fakes cannot prove native focus, UIPI or clipboard formats. Task 7 owns a concrete Windows before/after desktop check with a normal editor, an elevated editor and clipboard changes during transcription; preserve existing defects as known issues, never add the future Linux recovery feature in this move.
2. **An installed copy loses its update trust or data during startup/update/uninstall.** Expected: identity, signing trust, storage paths, credential refs, autostart command and all destructive guards are unchanged. Text guard tests cannot prove installer execution. Tasks 3 and 4 own byte comparisons of configuration/hooks and a disposable Windows update/uninstall check at final acceptance; never run cleanup against the owner's installation.
3. **A moved process runner leaves an agent, hardware probe or local server alive.** Expected: suspended Windows launch enters its Job Object before execution; timeout/cancel/shutdown reap the whole tree before joining pipes. Synthetic test children cannot cover real agent/server executables and inheritance. Task 2 owns an actual Windows process-tree check with disposable synthetic children plus code comparison of every production launch site; Task 11 checks the local-server launch and shutdown wiring.
4. **UI imports pass pure tests but a reopened WebView or browser preview fails.** Expected: every route mounts, setup and feature components use the same state singleton, listeners clean up, and preview mocks never enter production. Vitest uses a Node environment and does not mount Svelte pages. Task 12 owns production/preview entry graph checks and a browser route/reload matrix in both languages/themes, plus the owner's Windows close/reopen-during-dictation pass.
5. **Documentation cleanup deletes an unresolved acceptance item or turns a snapshot into a support claim.** Expected: pending checks remain actionable after old plans disappear; archived publication evidence remains historical; Linux status makes no support claim. Task 1 owns a source-to-destination findings/checklist ledger and a local-link check; Task 12 repeats it after archiving the active spec and plan.
---

## Execution and evidence conventions

There are 12 tasks/commits: batches 1–5, batch 6a (tests), batch 6b (responsibilities), then batches 7–11. Execute in that order on a feature branch. Workers commit only their task's explicit paths, never push, merge, tag or release. The orchestrator obtains CI on the exact commit after the read-only review; the next task waits for Windows/Linux/UI checks to pass. No network or implementation is part of writing this plan.

All source line ranges below refer to the inspected input at `d792d9d` (parent `81f4072`). Use item names when prior tasks have shifted line numbers. Source attribute counts below are inspection aids, not claimed executable results. Record real numeric discovery counts before each move using the commands below; save evidence outside the tracked tree, for example `/tmp/opit-layout-task-N/`. Never compare a Linux count to a Windows count.

For every task, the worker receives its task text and the repository's current `AGENTS.md`. Each task therefore repeats its checks and essential seam contracts. A gate requiring Windows is an orchestrator/CI handoff, not a claim that Linux compiled the app. Do not add a GNU cross-target check: no working cross-build is configured, and `crates/app/build.rs` emits MSVC `/MANIFEST:*` linker arguments for every Windows target. Linux cannot compile `crates/app` in this layout phase.

Discovery protocol for every task moving Rust tests: before editing, record `cargo test -p <package> --lib -- --list` on each runnable platform, and integration discovery with `cargo test -p <package> --tests -- --list` where relevant. Count lines ending `: test` per binary and record ignored tests separately with `-- --list --ignored`. Afterward repeat, map only the module-prefix changes explicitly allowed by the task, and compare the entire sorted names and counts, not just a filtered suite's exit code. Expected: no missing or duplicate test, same ignored set; a filtered `running 0 tests` fails the gate. New verification tests, if needed, must be reported separately so they cannot hide a lost moved test.

The sequential portable gate is `cargo fmt --all --check`, `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`, and `cargo test --workspace --exclude opit-speech-to-text`. Run full socket-binding tests outside the worker sandbox via the orchestrator. Inside the sandbox, discovery and individual socket-free suites may run; do not disable, ignore or delete tests to manufacture a green run. Socket users include `core` provider/files unit tests, `cleanup_http`, `cleanup_styles_http`, `pipeline_http`, eval `run_eval`, app Ask/provider/controller/local tests and local `free_port`; Windows named pipes also need a suitable runner. Windows CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. UI gate: `(cd ui && npm run check && npm test && npm run build)`; expect 0 errors AND 0 warnings, all tests passed, successful Vite build. Use installed dependencies; if missing, the orchestrator prepares them without altering tracked lockfiles. Run all builds/tests sequentially.

## Task 1 — Batch 1: curate documentation without removing execution instructions

**Files:**

Move exactly:

| From | To |
|---|---|
| `docs/ai-cleanup.md` | `docs/guides/ai-cleanup.md` |
| `docs/ask-mode.md` | `docs/guides/ask-mode.md` |
| `docs/cli.md` | `docs/guides/cli.md` |
| `docs/eval.md` | `docs/guides/eval.md` |
| `docs/file-transcription.md` | `docs/guides/file-transcription.md` |
| `docs/local-transcription.md` | `docs/guides/local-transcription.md` |
| `docs/app-rules.md` | `docs/reference/app-rules.md` |
| `docs/cleanup-styles.md` | `docs/reference/cleanup-styles.md` |
| `docs/dictation-upgrades.md` | `docs/reference/dictation-upgrades.md` |
| `docs/elevenlabs-scribe.md` | `docs/reference/elevenlabs-scribe.md` |
| `docs/learning-dictionary.md` | `docs/reference/learning-dictionary.md` |
| `docs/live-text.md` | `docs/reference/live-text.md` |
| `docs/media-control.md` | `docs/reference/media-control.md` |
| `docs/paste-methods.md` | `docs/reference/paste-methods.md` |
| `docs/voice-modes.md` | `docs/reference/voice-modes.md` |
| `docs/RELEASE-CHECKLIST.md` | `docs/release/RELEASE-CHECKLIST.md` |
| `docs/signpath-application.md` | `docs/release/signpath-application.md` |
| `docs/FIRST-RELEASE.md` | `docs/archive/FIRST-RELEASE.md` |
| `docs/PRE-RELEASE-REPORT.md` | `docs/archive/PRE-RELEASE-REPORT.md` |
| `docs/PUBLICATION-REPORT.md` | `docs/archive/PUBLICATION-REPORT.md` |

Create: `docs/README.md`, `docs/development/known-issues.md`, `docs/development/linux-status.md`. Track this input plan at `docs/superpowers/plans/2026-10-30-layout-cleanup.md` if the orchestration preparation has not already done so.

Delete after extraction: `docs/superpowers/plans/2026-09-30-plan-1-core-and-eval.md`, `docs/superpowers/plans/2026-09-30-plan-2-app-shell.md`, `docs/superpowers/plans/2026-10-01-plan-3-ui.md`, `docs/superpowers/plans/2026-10-01-plan-4-release.md`, `docs/superpowers/plans/README.md`, tracked `.claude/settings.json` only.

Modify: `AGENTS.md`, `README.md` (broken links only), `CONTRIBUTING.md` (checklist link only), `.github/workflows/release.yml` (checklist comment only), `docs/releases/0.1.3.md` (publication-report reference only), `ui/src/guide-preview.ts`, `ui/src/lib/app-rules.ts`, `ui/src/lib/learning.ts`, `ui/src/lib/styles.ts`, `ui/src/lib/voice-modes.ts` (documentation-reference comments only). Moved docs may have link/evidence-reference edits. Inspect/preserve `.github/workflows/ci.yml`; it already has the required trigger and needs no edit.

**Interfaces:**

Consumes: the four completed plans and their index, both specs, `AGENTS.md`, publication/pre-release evidence, current source/tests. Produces: `docs/README.md` as the public local index; `docs/development/known-issues.md` as the preserved deferred-work/manual-acceptance contract; an empty-of-claims Linux status skeleton. Both specs and this plan remain at their original `docs/superpowers/` paths through Task 11. Task 12 alone moves the specs to `docs/design/` and this plan to `docs/archive/`. No Rust/JS export or behaviour changes.

- [ ] Record `git status --short` and the current commit; preserve unrelated changes. Read `AGENTS.md`, both specs, all four old plan outcome/manual sections and `docs/superpowers/plans/README.md` before deleting anything. Keep this plan and both specs available at their original paths. Do not reinterpret old implementation code blocks as current code.
- [ ] Create the exact document groups above. Mark the three archived release documents as dated snapshots; preserve useful public evidence and existing limitations. Replace references to ignored `.superpowers/package2-workspace-tests.log`, `.superpowers/package4-tests.log`, `target/release-readiness/` evidence and the old absolute installer build location with dated summaries of what was checked and what was not; do not invent downloadable logs. Keep useful build-command examples distinct from claims that private outputs are available.
- [ ] Extract still-open findings into `known-issues.md` with source plan/task, current code evidence, status, and an actionable reproduction/check. Preserve a per-source-item disposition ledger: retained, superseded (cite current code/test), or historical-only. Carry the real-provider eval baseline; Turkish regex folding and eval malformed-WAV/report escaping candidates; hanging paste, clipboard formats/delayed rendering, controller blocking, overlay stray mouse-up, quit/log/restore and window-open races; config/learning/command ordering and keyring threading; UI load/resume/key lookup, History refresh/edit/playback/search, wizard/profile key lifecycle, microphone/capture races, Rules save/YAML and accessibility/contrast checks. Do not fix these in the layout task. Classify old stale-generation timer gaps as superseded by the named current timer tests, and old uninstaller cancellation/update UI/signature/draft findings using `tauri_conf.rs`, `ui/src/lib/update.test.ts`, release-helper tests and the pre-release report. Do not preserve resolved entries as confirmed current defects.
- [ ] Preserve open manual passes as checkboxes with prerequisites and expected results: Plan 1 Task 16 Step 15 (personal dataset/key); Plan 2 Task 15 Steps 2–7 (real mic/key, toggle/lone Ctrl/Escape/push-to-talk, silence, stop-time safety/UIPI, fallback/recovery, clipboard preservation, missing mic/key/network, tray/window/single instance, five short dictations' median latency <1.5 s, tray RAM <40 MB, config/rules recovery, history/log privacy, release-only autostart); Plan 3 Task 12 Step 2 (wizard under two minutes, all pages, real correction, keys/fallback, capture/mic, close/reopen, tray pause, keyboard and both themes); Plan 4 clean Windows install/update/restart/uninstall/data-retention and pending SignPath integration. Use current settings: restore is opt-in, autostart requires `setup_done`, patient retries and microphone salvage exist. Retain original targets as pending acceptance criteria, never measured results. Link to `docs/release/RELEASE-CHECKLIST.md`; carry modern feature-specific pending passes from the moved reference documents without asserting they passed.
- [ ] Create `linux-status.md` containing only a title and empty sections: Scope, Spike evidence, Desktop/session runs, Packages, Known limits, Pending checks. Use empty table headers if useful; enter no tested distro, support matrix, package availability or success claim.
- [ ] Replace AGENTS status/history with current working rules and a current layout table, including existing `crates/ipc` and `crates/cli`. Preserve secrets, blocking, serialization, i18n, platform purity, test commands, Windows CI and installation guards. Keep the active plan/spec links until Task 12; link current known issues instead of the deleted plan index. Only link substitutions are allowed in root `README.md`; do not rewrite its description/table prose, product claims or preview instructions.
- [ ] Fix every documentation move reference: root README's checklist (two occurrences), FIRST-RELEASE and eval links; CONTRIBUTING's checklist; release workflow's checklist comment; release notes' PUBLICATION reference; archived reports' links to each other, checklist and `../releases/0.1.3.md`; FIRST-RELEASE's repo-root prose paths; AGENTS's brief/index/publication/eval/layout paths; self-paths in reference documents; the five listed UI documentation comments. The two retained specs still mention the old checklist: list that as an intentional pending link repair owned by Task 12, rather than moving/editing them early. A future proposed code-signing-policy filename is not a broken existing file.
- [ ] Review Focus 5 check: compare the extraction ledger with every deferred/manual item in the original plans via `git show HEAD:<old-path>` before removing them. Every unresolved item must have a retained destination; dated publication claims must remain dated. Check local Markdown targets with a dependency-free local path/fragment scan (ignore external URLs and fenced examples); expect zero unintended dangling local targets. `git diff -- README.md` must contain only old-to-new path/link-label substitutions. Confirm `.claude/settings.local.json` remains untouched and ignored.
- [ ] Verify on Linux sequentially: `cargo fmt --all --check`; `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside sandbox: `cargo test --workspace --exclude opit-speech-to-text`; `(cd ui && npm run check && npm test && npm run build)`; `node --test scripts/release/*.test.mjs`. Expect zero failures, zero Rust warnings, UI 0 errors/0 warnings and a successful build. No tests move in this task; compare discovered suites/counts with the pre-task baseline. Windows cannot compile here: request orchestrator Windows CI `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` on this commit. Preserve `[main, "feat/**"]` and the app exclusion in CI; the spec's proposed trigger addition is already implemented by `d792d9d`. Require an actual Windows baseline before Task 2; do not guess that `81f4072` compiled.
- [ ] Required batch gate (sequential): Linux worker runs `cargo fmt --all --check` and `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside the socket-restricted sandbox runs `cargo test --workspace --exclude opit-speech-to-text`. Windows CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; Linux cannot compile `crates/app` here, so do not substitute a cross-target check. Run `(cd ui && npm run check && npm test && npm run build)` on Linux; expect 0 errors AND 0 warnings, all tests passing, a successful Vite build and no unexplained change from the recorded before-counts. Record all evidence outside tracked files. Preserve behaviour and public facades; socket-binding tests are never silently skipped. After this commit, a read-only review and orchestrator CI on this exact commit must pass before the next task; workers never push.
- [ ] Commit only these exact paths; inspect staged names against the move/create/modify/delete list:

  ```sh
  git add -A -- \
    docs/ai-cleanup.md docs/ask-mode.md docs/cli.md docs/eval.md \
    docs/file-transcription.md docs/local-transcription.md docs/app-rules.md docs/cleanup-styles.md \
    docs/dictation-upgrades.md docs/elevenlabs-scribe.md docs/learning-dictionary.md docs/live-text.md \
    docs/media-control.md docs/paste-methods.md docs/voice-modes.md docs/RELEASE-CHECKLIST.md \
    docs/signpath-application.md docs/FIRST-RELEASE.md docs/PRE-RELEASE-REPORT.md docs/PUBLICATION-REPORT.md \
    docs/guides/ai-cleanup.md docs/guides/ask-mode.md docs/guides/cli.md docs/guides/eval.md \
    docs/guides/file-transcription.md docs/guides/local-transcription.md docs/reference/app-rules.md docs/reference/cleanup-styles.md \
    docs/reference/dictation-upgrades.md docs/reference/elevenlabs-scribe.md docs/reference/learning-dictionary.md docs/reference/live-text.md \
    docs/reference/media-control.md docs/reference/paste-methods.md docs/reference/voice-modes.md docs/release/RELEASE-CHECKLIST.md \
    docs/release/signpath-application.md docs/archive/FIRST-RELEASE.md docs/archive/PRE-RELEASE-REPORT.md docs/archive/PUBLICATION-REPORT.md \
    docs/README.md docs/development/known-issues.md docs/development/linux-status.md docs/superpowers/plans/2026-09-30-plan-1-core-and-eval.md \
    docs/superpowers/plans/2026-09-30-plan-2-app-shell.md docs/superpowers/plans/2026-10-01-plan-3-ui.md docs/superpowers/plans/2026-10-01-plan-4-release.md docs/superpowers/plans/README.md \
    docs/superpowers/plans/2026-10-30-layout-cleanup.md AGENTS.md README.md CONTRIBUTING.md \
    .github/workflows/release.yml docs/releases/0.1.3.md .claude/settings.json ui/src/guide-preview.ts \
    ui/src/lib/app-rules.ts ui/src/lib/learning.ts ui/src/lib/styles.ts ui/src/lib/voice-modes.ts
  git diff --cached --check
  git commit -m 'docs: organize project documentation and preserve pending checks'
  ```

## Task 2 — Batch 2: shared process supervision and Windows hardware probing

**Files:**

Move/split: `crates/app/src/ask/process.rs` → `crates/app/src/platform/process.rs` (portable orchestration and five Windows-only tests), plus Create `crates/app/src/platform/windows/process.rs` (native Job Object). Split `crates/app/src/local/hardware.rs` into its retained portable cache/model helpers plus Create `crates/app/src/platform/windows/hardware.rs` (native detection and two tests).

Modify: `crates/app/src/ask/mod.rs`, `crates/app/src/ask/cli.rs`, `crates/app/src/platform/mod.rs`, `crates/app/src/platform/windows/mod.rs`, `crates/app/src/local/hardware.rs`, `crates/app/src/local/server.rs`.

**Interfaces:**

Consumes: `controller::ErrorKind`, `opit_core::ask::MAX_ANSWER`, `ask::cli::{is_shim, arguments}`, `local::hardware::{Hardware, auto_threads}`. Produces: `platform::process::run` with its unchanged arguments/result; `pub(crate) platform::process::Tree` re-exported from `platform::windows::process::Tree` on Windows; `Tree::{new, attach, stop}` and Drop/Send semantics unchanged. `platform::windows::hardware::detect() -> Option<local::hardware::Hardware>` becomes the Windows detection function; `local::hardware::detect` remains the dispatcher with today's non-Windows fallback. `Hardware`, `HardwareCache`, `recommendation` remain under `local::hardware` until Task 11.

- [ ] Record status and Windows discovery via orchestrator: `cargo test -p opit-speech-to-text --lib -- --list` and `-- --list --ignored`. Record numeric counts/names for `ask::process::tests` (five source tests) and `local::hardware::tests` (four on Windows). Keep evidence in `/tmp/opit-layout-task-2/`; compare after moving with the prefix mapping below.
- [ ] Move `run` and `build_command` (original process lines 11–100) and all five original tests (181–305) to `platform/process.rs`. Move Windows `Tree`, its unsafe Send, inherent impl and Drop (102–162) to `platform/windows/process.rs`; use a Windows-gated re-export in shared `process.rs`. Keep the existing rejecting non-Windows `Tree` implementation (164–179) in shared `process.rs`: no Unix process support yet. Preserve suspended creation, no-window flags, kill/wait order, output bounds, stdin-only content, cancellation and sanitized error mapping.
- [ ] Make `ask::cli` and its `is_shim` visible only as far as needed (`pub(crate)` module/function; `arguments` accessible to moved tests). Replace shared runner's `super::cli::is_shim` with `crate::ask::cli::is_shim` and test `super::super::cli::arguments` with `crate::ask::cli::arguments`. Remove `ask::process`; import `crate::platform::process` in Ask. Update hardware/server `crate::ask::process::Tree` consumers to `crate::platform::process::Tree`.
- [ ] Move hardware `CIM_TIMEOUT`, `NVIDIA_SMI_TIMEOUT`, `probe`, Windows `detect` body, `blocked_probe_child` and `hardware_probe_times_out_and_reaps_a_blocked_child` to `platform/windows/hardware.rs`. Retain `Hardware`, `HardwareCache`, `CacheState`, `RETRY_AFTER`, `fallback`, `auto_threads`, `recommendation`, two portable cache/recommendation tests and `detect` dispatcher in `local/hardware.rs`. Expose `local::hardware` and `auto_threads` as `pub(crate)` only where the native module needs them. Native `detect` calls the shared `auto_threads`; no new hardware algorithm or failure policy.
- [ ] Repair embedded test paths: `ask::process::tests::fake_child` → `platform::process::tests::fake_child`; `ask::process::tests::fake_grandchild` → `platform::process::tests::fake_grandchild`; `local::hardware::tests::blocked_probe_child` → `platform::windows::hardware::tests::blocked_probe_child`. These are executable argv, not comments. Move each test exactly once and keep its original cfg/attributes.
- [ ] Review Focus 3 check: compare all three production callers (`ask/mod.rs`, hardware `probe`, local server `Process::launch`) with pre-task code for flags → Tree creation → spawn → attach/resume → failure kill/wait → stop/Drop. On Windows orchestrator run `cargo test -p opit-speech-to-text --lib platform::process::tests -- --nocapture` (five tests, no zero-test success) and `cargo test -p opit-speech-to-text --lib platform::windows::hardware::tests -- --nocapture` (two tests). Observe the disposable child/grandchild PIDs terminate on timeout and cancellation; never launch a real agent with real workspace permissions for a move check. A real installed agent/server remains part of final owner acceptance; do not claim these synthetic checks prove it.
- [ ] Linux here: `cargo fmt --all --check`, `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside sandbox: `cargo test --workspace --exclude opit-speech-to-text`. Windows CI: full workspace clippy/tests; Linux cannot compile the app. Compare Windows before/after complete discovered names/counts with only the three module-prefix changes above; shared cache tests remain two, the combined process/hardware suites remain nine tests (seven change module prefixes). Run loopback-binding app/provider tests only on orchestrator/CI, never suppress them.
- [ ] Required batch gate (sequential): Linux worker runs `cargo fmt --all --check` and `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside the socket-restricted sandbox runs `cargo test --workspace --exclude opit-speech-to-text`. Windows CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; Linux cannot compile `crates/app` here, so do not substitute a cross-target check. Run `(cd ui && npm run check && npm test && npm run build)` on Linux; expect 0 errors AND 0 warnings, all tests passing, a successful Vite build and no unexplained change from the recorded before-counts. Record all evidence outside tracked files. Preserve behaviour and public facades; socket-binding tests are never silently skipped. After this commit, a read-only review and orchestrator CI on this exact commit must pass before the next task; workers never push.
- [ ] Commit:

  ```sh
  git add -A -- \
    crates/app/src/ask/process.rs crates/app/src/ask/mod.rs crates/app/src/ask/cli.rs crates/app/src/platform/process.rs \
    crates/app/src/platform/windows/process.rs crates/app/src/platform/windows/hardware.rs crates/app/src/platform/mod.rs crates/app/src/platform/windows/mod.rs \
    crates/app/src/local/hardware.rs crates/app/src/local/server.rs
  git diff --cached --check
  git commit -m 'refactor(platform): centralize process supervision and Windows hardware probes'
  ```

## Task 3 — Batch 3: platform factory, uninstall isolation and shared microphone source

**Files:**

Move: `crates/app/src/uninstall.rs` → `crates/app/src/platform/windows/uninstall.rs`; `crates/app/src/platform/windows/microphone.rs` → `crates/app/src/platform/microphone.rs`.

Create: `crates/app/src/platform/factory.rs`, `crates/app/src/platform/windows/factory.rs`.

Modify: `crates/app/src/lib.rs`, `crates/app/src/platform/mod.rs`, `crates/app/src/platform/windows/mod.rs`, `AGENTS.md` (layout paths only), `docs/reference/live-text.md` (uninstall/microphone physical paths), `docs/development/known-issues.md` (pending native acceptance).

**Interfaces:**

Consumes: `app_core::Platform`, `platform::{OverlaySink, Paster, Sounds}`, `opit_core::config::AppConfig`, existing Windows backends/fallbacks. Produces: `platform::factory::PlatformServices { platform: app_core::Platform, paster: Arc<dyn Paster>, sounds: Arc<dyn Sounds> }`, Windows-gated `platform::factory::create(config: &AppConfig, overlay_sink: OverlaySink) -> PlatformServices` dispatching to `platform::windows::factory::create`. Preserve `platform::windows::CpalMicrophone` and `platform::windows::microphone` through re-exports of the new `platform::microphone` module/type, and preserve Windows-gated `opit_app::uninstall` via `pub use platform::windows::uninstall`. These aliases keep existing integration tests compiling. `app_core::Platform` stays intact for Task 5.

- [ ] Record Windows app test names/counts, ignored set and `cargo test -p opit-speech-to-text --tests -- --list`. The microphone source has ten tests (nine automatic plus one ignored real-mic smoke); uninstall has five. Snapshot `crates/app/tauri.conf.json`, `crates/app/windows/hooks.nsh`, credential service/suffix, `APP_DIR_NAME`, autostart `VALUE_NAME`/command and updater settings as bytes/hash metadata without reading private key files.
- [ ] Move the complete cpal file, including `CpalMicrophone`, `CpalCapture`, `SampleBuffer`, `Opened`, `Attempt`, `LevelMeter`, all impls/helpers (`capture_thread`, `plan_attempts`, `open`, `default_device`, `device_name`, `order_device_names`, `build_stream`, `build_typed`, `append_mono`, `is_fatal`) and inline tests to `platform/microphone.rs`. Declare the shared source under `#[cfg(windows)]` for now, preserving the existing Windows-targeted cpal dependency. Moving a portable source does not enable the Linux app or add ALSA dependencies. Preserve the Windows module/type aliases without compiling the file twice.
- [ ] Move the complete uninstall file with `DELETE_CREDENTIALS_FLAG`, `credential_refs`, `delete_credentials`, `run`, `custom_local_folder` and all five tests under `platform/windows/`. Keep its startup dispatcher before paths/logging/plugins/windows/tray creation and behind `#[cfg(windows)]`. Keep the root uninstall alias; `crates/app/tests/tauri_conf.rs` must still compile its `opit_app::uninstall::DELETE_CREDENTIALS_FLAG` access.
- [ ] Extract only the backend-construction block from `lib.rs::setup` into the factory: WinOverlay and NoOverlay error fallback; KeyringStore and UnavailableSecrets; DebugAutostart versus RegistryAutostart/NoAutostart; WinHotkey, CpalMicrophone, WinForegroundApp, Windows media, WinSounds including `warm_up`, and WinPaster. Package the existing seven-field `Platform` plus paster/sounds into `PlatformServices`. Startup calls `create(&config, controller.overlay_sink())` once and clones the same Arc instances into AppCore and controller Env. History, providers, settings, local loading callback, tasks and startup policy stay in startup. Do not create Linux stubs/fallback success or change cfg/dependencies to pretend the app compiles there.
- [ ] Review Focus 2 check: `git diff` must show no changes to Tauri config, NSIS hooks, build resources, identities/keys, credential namespace, autostart values or storage strings. Verify `--delete-credentials` still exits without logging or recreating data directories by tracing the actual entry call order. Have the orchestrator run Windows `cargo test -p opit-speech-to-text --test tauri_conf` (seven existing tests) and all uninstall tests; final clean-VM acceptance must exercise unticked/ticked uninstall, cancelled uninstall, silent reinstall and update, with disposable credentials/data. No destructive run against the owner's installation.
- [ ] Linux here: `cargo fmt --all --check`, `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside sandbox: `cargo test --workspace --exclude opit-speech-to-text`. Windows CI runs full workspace clippy/tests; app cannot compile on Linux here. Compare Windows discovery with `platform::windows::microphone::tests` → `platform::microphone::tests`, `uninstall::tests` → `platform::windows::uninstall::tests`; counts/ignored sets unchanged, aliases do not duplicate discovery. Fix live-text reference paths for the moved uninstall and microphone files. Keep real-mic smoke ignored pending owner hardware run; socket tests run outside sandbox.
- [ ] Required batch gate (sequential): Linux worker runs `cargo fmt --all --check` and `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside the socket-restricted sandbox runs `cargo test --workspace --exclude opit-speech-to-text`. Windows CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; Linux cannot compile `crates/app` here, so do not substitute a cross-target check. Run `(cd ui && npm run check && npm test && npm run build)` on Linux; expect 0 errors AND 0 warnings, all tests passing, a successful Vite build and no unexplained change from the recorded before-counts. Record all evidence outside tracked files. Preserve behaviour and public facades; socket-binding tests are never silently skipped. After this commit, a read-only review and orchestrator CI on this exact commit must pass before the next task; workers never push.
- [ ] Commit:

  ```sh
  git add -A -- \
    crates/app/src/uninstall.rs crates/app/src/platform/windows/uninstall.rs crates/app/src/platform/windows/microphone.rs crates/app/src/platform/microphone.rs \
    crates/app/src/platform/factory.rs crates/app/src/platform/windows/factory.rs crates/app/src/platform/mod.rs crates/app/src/platform/windows/mod.rs \
    crates/app/src/lib.rs AGENTS.md docs/reference/live-text.md docs/development/known-issues.md
  git diff --cached --check
  git commit -m 'refactor(platform): add backend factory and isolate Windows cleanup'
  ```

## Task 4 — Batch 4: shell lifecycle and presentation

**Files:**

Move complete files, including inline tests: `crates/app/src/startup.rs` → `crates/app/src/shell/startup.rs`; `crates/app/src/logging.rs` → `crates/app/src/shell/logging.rs`; `crates/app/src/events.rs` → `crates/app/src/shell/events.rs`; `crates/app/src/i18n.rs` → `crates/app/src/shell/i18n.rs`; `crates/app/src/tray.rs` → `crates/app/src/shell/tray.rs`; `crates/app/src/tray_menu.rs` → `crates/app/src/shell/tray_menu.rs`; `crates/app/src/window.rs` → `crates/app/src/shell/window.rs`; `crates/app/src/updates.rs` → `crates/app/src/shell/updates.rs`.

Create: `crates/app/src/shell/mod.rs`. Modify: `crates/app/src/lib.rs`, `AGENTS.md` (layout/native-i18n path), `docs/reference/app-rules.md`, `docs/reference/live-text.md`, `docs/reference/voice-modes.md`, `docs/reference/cleanup-styles.md` (implementation path references), `docs/development/known-issues.md` (pending native acceptance), `ui/src/lib/events.ts`, `ui/src/lib/types.ts` (Rust-source comments only).

**Interfaces:**

Consumes: `platform::factory::{create, PlatformServices}`, `app_core::{AppCore, Platform}`, `controller::{channel, run, Env}`, existing root module aliases. Produces: `shell::{run, AUTOSTART_FLAG}`, `pub(crate) shell::now_ms`, and `shell::{startup, logging, events, i18n, tray, tray_menu, window, updates}`. Retain root `run`/`AUTOSTART_FLAG` re-exports, crate-visible root `now_ms`, and public root aliases for all eight moved modules. Keep `startup::{Paths, StartupNotice, LoadedConfig, DATA_DIR_ENV, load_config, move_config_aside, load_user_pack}`, `i18n::{Lang, resolve, system_locale}`, `logging::{init, install_panic_hook, LogInitError}`, events constants/functions/`TauriEvents`, `tray::{create, refresh, TRAY_ID}`, `tray_menu::{TrayCommand, TrayItem, build, parse}`, `window::{show_main, build, MAIN_WINDOW}`, `updates::{UpdateInfo, UpdateState, UpdateMachine, UpdateService, current, check, install, spawn_auto_check}` unchanged. Existing callers and integration tests continue using the root paths.

- [ ] Record Windows app library/integration discovery names and numeric counts. Relevant source suites: startup six, i18n seven, tray_menu four, updates eight; logging integration one. Save before manifests and identity/config/hook hashes outside the tree.
- [ ] Move all eight files whole. Move `AUTOSTART_FLAG`, `run`, `setup`, `now_ms` and their required imports from `lib.rs` into `shell/mod.rs`; root lib contains module declarations and compatibility re-exports. Keep `tauri::generate_context!()` valid in the shell module; the macro still uses the app crate manifest/config, not a new relative config. Use explicit `crate::{app_core, commands, controller, file_jobs, ipc, platform}` imports where the old root body relied on unqualified module names, and a Windows-gated `use crate::uninstall` for the pre-start cleanup branch. Existing fully qualified history/providers/settings imports keep their crate paths.
- [ ] Preserve single-instance as first plugin, updater/dialog plugins, command handler registry, managed-state-before-callback ordering, start-hidden policy, event names/payloads, main-thread tray scheduling and background window construction, shutdown guard and media/local termination. Preserve root `opit_app::run()` for `main.rs`, `opit_app::logging` for `logging_init.rs`, and root aliases used by AppCore/controller/uninstall/commands/file_jobs/settings. No mass caller rewrite is necessary.
- [ ] Keep `shell/i18n.rs` inline tests in that same file. Its existing `include_str!("i18n.rs")` now resolves to `shell/i18n.rs` and needs no string edit; do not externalize the tests or change it to `../i18n.rs`. Verify on Windows with `cargo test -p opit-speech-to-text --lib shell::i18n::tests::native_strings_preserve_utf8_and_ask_wording -- --exact` (one test). Update AGENTS and listed source/doc comments to the new physical paths.
- [ ] Path audit: integration `include_str!("../tauri.conf.json")`, `include_str!("../windows/hooks.nsh")` and `include_str!("../../../scripts/build-cli.mjs")` remain correct because neither integration tests nor their inputs move. `build.rs` references `windows/tests.manifest`, `../../target/cli-bundle` and `opit.exe` remain unchanged; `tauri.conf.json` resource and NSIS-hook paths remain unchanged. Review Focus 2: compare their bytes and the entry order against before; confirm logger guard remains alive for the full `run`, uninstall is handled first and update hold/shutdown wiring is identical. Queue the clean-VM install/update/restart/data pass in known issues; no claim of Windows desktop execution from Linux.
- [ ] Linux here: `cargo fmt --all --check`, `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside sandbox `cargo test --workspace --exclude opit-speech-to-text`. Windows CI full workspace clippy/tests plus `cargo test -p opit-speech-to-text --test logging_init` and `--test tauri_conf`; app cannot compile on Linux. Compare full before/after test names/counts with only eight module prefixes gaining `shell::`; integration names/counts unchanged, all 25 moved inline tests still discovered. Socket suites remain an orchestrator/CI responsibility.
- [ ] Required batch gate (sequential): Linux worker runs `cargo fmt --all --check` and `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside the socket-restricted sandbox runs `cargo test --workspace --exclude opit-speech-to-text`. Windows CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; Linux cannot compile `crates/app` here, so do not substitute a cross-target check. Run `(cd ui && npm run check && npm test && npm run build)` on Linux; expect 0 errors AND 0 warnings, all tests passing, a successful Vite build and no unexplained change from the recorded before-counts. Record all evidence outside tracked files. Preserve behaviour and public facades; socket-binding tests are never silently skipped. After this commit, a read-only review and orchestrator CI on this exact commit must pass before the next task; workers never push.
- [ ] Commit:

  ```sh
  git add -A -- \
    crates/app/src/lib.rs crates/app/src/startup.rs crates/app/src/logging.rs crates/app/src/events.rs \
    crates/app/src/i18n.rs crates/app/src/tray.rs crates/app/src/tray_menu.rs crates/app/src/window.rs \
    crates/app/src/updates.rs crates/app/src/shell/mod.rs crates/app/src/shell/startup.rs crates/app/src/shell/logging.rs \
    crates/app/src/shell/events.rs crates/app/src/shell/i18n.rs crates/app/src/shell/tray.rs crates/app/src/shell/tray_menu.rs \
    crates/app/src/shell/window.rs crates/app/src/shell/updates.rs AGENTS.md docs/reference/app-rules.md \
    docs/reference/live-text.md docs/reference/voice-modes.md docs/reference/cleanup-styles.md docs/development/known-issues.md \
    ui/src/lib/events.ts ui/src/lib/types.ts
  git diff --cached --check
  git commit -m 'refactor(shell): group Tauri lifecycle and presentation modules'
  ```

## Task 5 — Batch 5: split AppCore by concern behind its public facade

**Files:**

Move/split `crates/app/src/app_core.rs` → Create `crates/app/src/app_core/mod.rs`, `crates/app/src/app_core/types.rs`, `crates/app/src/app_core/config.rs`, `crates/app/src/app_core/credentials.rs`, `crates/app/src/app_core/input.rs`, `crates/app/src/app_core/rules.rs`, `crates/app/src/app_core/history.rs`, `crates/app/src/app_core/tests/mod.rs`. Delete the old flat file after moving every item. Modify `AGENTS.md`, `docs/reference/app-rules.md`, `docs/reference/live-text.md`, `docs/reference/cleanup-styles.md`, `docs/reference/voice-modes.md` (physical AppCore paths only).

**Interfaces:**

Consumes: root aliases `startup::{Paths, StartupNotice}`, `settings::{Settings, SettingsHandle}`, platform traits, `controller::{ControllerHandle, DictationStatus, ErrorKind}`, `HistoryService`, `LocalService`. Produces: unchanged `app_core::AppCore` in `app_core/mod.rs`, with all original fields/constructor/method signatures; `pub use types::{CommandError, RulesPreview, CorrectionDraft, HistoryUpdate, HotkeyState, MicTestEvent, MicTestSink, Platform, MAX_HISTORY_TEXT_BYTES, MIC_TEST_LIMIT, HOTKEY_CAPTURE_LIMIT, USER_PACK_TEMPLATE}` and `pub use config::autostart_wanted`. `app_core::Platform` continues to satisfy `platform::factory::PlatformServices`; `commands`, tray, IPC and file jobs keep importing the facade. No module-private implementation type becomes an external API.

- [ ] Record Windows `cargo test -p opit-speech-to-text --lib -- --list` and ignored set before editing. Capture every `app_core::tests::` name and count (55 source tests); some use wiremock and must execute on orchestrator/CI outside the sandbox. Retain the tests under that same logical path to make manifest comparison exact.
- [ ] Move `CommandError` and all its impls/conversions; `RulesPreview`, `CorrectionDraft`, `HistoryUpdate`, `HotkeyState`, `MicTestEvent`, `MicTestSink`, `Platform`; and the four public constants listed in Interfaces into `types.rs` (original lines 56–163, excluding the private startup-duration constant). Keep serialization/error strings unchanged. Move `AppCore` storage (166–189), `new` (199–229), `status`, `take_notices`, `startup_overlay` (235–266) and private `STARTUP_NOTICE_HIDE` to `mod.rs`. Declare private concern modules and the `#[cfg(test)] mod tests`.
- [ ] `config.rs`: move free `autostart_wanted`; AppCore methods `config`, `rebuild`, `save_config`, `update_config`, `save_locked`, `move_bad_config_aside`, `set_active_profile`, `set_active_style`, `persist_active_style`, `system_locale`, `apply_autostart`. `input.rs`: move free `auxiliary_shortcuts`, `shortcut_mappings_changed`; methods `shortcut_error`, `validate_shortcut`, `apply_hotkey`, `sync_pause`, `set_hotkey_paused`, `hotkey_state`, `mic_test_start`, `mic_test_start_for`, `mic_test_stop`, `stop_mic_test_if`, `set_hotkey_capture`, `set_hotkey_capture_for`, `validate_hotkey`, `microphones`.
- [ ] `credentials.rs`: move `has_api_key`, `set_api_key`, `delete_api_key`, `test_connection`, `cleanup_test_client`. `rules.rs`: move `parse_user_rules`, `render_user_rules`, `correction_draft`, `user_rules_yaml`, `save_user_rules`, `save_user_rules_locked`, `learning_apply`, `learning_dismiss`, `validate_learning_count`, `learning_error`, `rules_preview`, `prompt_budget`. `history.rs`: move private `history` plus `history_update_text`, `history_recent`, `usage_stats`, `history_search`, `history_delete`, `history_clear`, `history_audio`, `history_retry_audio`. Move methods as inherent `impl AppCore` blocks, not a new trait/service object.
- [ ] Move the entire original inline tests body (883–2005) to `tests/mod.rs`, retaining fixtures and helpers (`Fixture`, `fixture`, `fixture_with`, locked-bad-config helpers, learning helpers) in that module; remove only its old enclosing brace pair. No test rename or deletion. Use `super::*` plus explicit concern-helper imports as needed; the tests' Windows OpenOptions/share-mode helper remains Windows-only by the existing app boundary.
- [ ] Rust privacy check: parent-owned AppCore fields remain private and are accessible to descendants. Expose free `input::shortcut_mappings_changed` as `pub(super)` and import it in config; keep `auxiliary_shortcuts` private to input. Mark only cross-concern helper methods `rebuild`, `save_locked`, `shortcut_error`, `sync_pause`, `save_user_rules_locked`, `learning_error` as `pub(super)` where sibling callers need access; keep `mic_test_start_for` and `set_hotkey_capture_for` at their existing crate visibility. Import test-only free helpers explicitly rather than making broad public re-exports. Preserve lock lifetimes/order, normalize/save side effects, generation timers, error sanitization and API-key input trimming. The pre-existing async `test_connection` keyring call is documented deferred work, not silently repaired in this mechanical split.
- [ ] Run Windows `cargo test -p opit-speech-to-text --lib app_core::tests` on orchestrator/CI; expect all 55 original tests discovered/passed and exact before/after names, including locked-config protection, concurrent learning, timer generation, autostart/wizard and sanitized errors. Linux here: `cargo fmt --all --check`, `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside sandbox: `cargo test --workspace --exclude opit-speech-to-text`. Windows CI full workspace clippy/tests; app cannot compile here on Linux. No loopback-binding suite runs inside the worker sandbox. Update listed documentation paths; no public `app_core::AppCore` caller needs a new import.
- [ ] Required batch gate (sequential): Linux worker runs `cargo fmt --all --check` and `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside the socket-restricted sandbox runs `cargo test --workspace --exclude opit-speech-to-text`. Windows CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; Linux cannot compile `crates/app` here, so do not substitute a cross-target check. Run `(cd ui && npm run check && npm test && npm run build)` on Linux; expect 0 errors AND 0 warnings, all tests passing, a successful Vite build and no unexplained change from the recorded before-counts. Record all evidence outside tracked files. Preserve behaviour and public facades; socket-binding tests are never silently skipped. After this commit, a read-only review and orchestrator CI on this exact commit must pass before the next task; workers never push.
- [ ] Commit:

  ```sh
  git add -A -- \
    crates/app/src/app_core.rs crates/app/src/app_core/mod.rs crates/app/src/app_core/types.rs crates/app/src/app_core/config.rs \
    crates/app/src/app_core/credentials.rs crates/app/src/app_core/input.rs crates/app/src/app_core/rules.rs crates/app/src/app_core/history.rs \
    crates/app/src/app_core/tests/mod.rs AGENTS.md docs/reference/app-rules.md docs/reference/live-text.md \
    docs/reference/cleanup-styles.md docs/reference/voice-modes.md
  git diff --cached --check
  git commit -m 'refactor(app): split AppCore by concern behind its facade'
  ```

## Task 6 — Batch 6a: move controller tests before touching responsibilities

**Files:**

Move: `crates/app/src/controller/tests.rs` → `crates/app/src/controller/tests/mod.rs`; `crates/app/src/controller/live_tests.rs` → `crates/app/src/controller/tests/live_tests.rs`.

Modify: `docs/reference/cleanup-styles.md`, `docs/reference/app-rules.md`, `docs/reference/live-text.md`, `docs/reference/voice-modes.md` (controller test source paths only). Inspect `crates/app/src/controller/mod.rs`; its existing `#[cfg(test)] mod tests;` needs no edit.

**Interfaces:**

Consumes: unchanged `controller::{Controller, Msg, Env, channel, run, meter}`, `controller::live::{LiveRecording, LiveResult}`, existing fake providers/platform services. Produces: unchanged logical test modules `controller::tests` and `controller::tests::live_tests`, shared `Harness`, `harness`, `settings`, `TestProviders`, `ScriptedClient`, `FakeHistory`, `FakeEvents`, style/delivery gates and helpers inside their original test module. Tests remain private; Task 7 can change implementation ownership without rewriting scenario bodies.

- [ ] Ask orchestrator to record Windows `cargo test -p opit-speech-to-text --lib -- --list` plus ignored set and a before run of `cargo test -p opit-speech-to-text --lib controller::` outside the sandbox. Record the numeric `controller::` count and full names. Source inspection: 90 tests in `tests.rs`, ten in `live_tests.rs`, one inline live helper test and two status tests = 103 controller tests; use runnable discovery as authority rather than this arithmetic if cfg changes.
- [ ] Move `tests.rs` whole to `tests/mod.rs`; all scenario functions, imports, harness/provider/history/event structs, gate impls, `captured_logs`, `CaptureWriter`, `wait_for`, and every attribute move unchanged. Move `live_tests.rs` whole alongside it. Remove `#[path = "live_tests.rs"]` and use ordinary `mod live_tests;` in `tests/mod.rs`; because the file is now a module root, Rust resolves the sibling normally. Keep its logical name `live_tests`, so no scenario test name changes. `use super::*` in each file retains the same parent module meaning.
- [ ] Do not split/change production actor methods or rename tests in this commit. Verify no `controller/tests.rs`, `controller/live_tests.rs` or obsolete `#[path]` remains in live reference paths; update all four listed reference documents to `controller/tests/mod.rs`, and live-text's streaming test reference to `controller/tests/live_tests.rs`. Historical original-plan mentions inside this plan are evidence, not live imports.
- [ ] Windows orchestrator/CI: repeat discovery and `cargo test -p opit-speech-to-text --lib controller::`; expect identical full names, same numeric 103 baseline if unchanged, zero duplicates/missing tests and zero failures. This is discovery preservation, not new behavioural coverage. Loopback-binding cleanup/Ask/voice tests require the orchestrator outside the worker sandbox. Linux here: `cargo fmt --all --check`, `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator portable `cargo test --workspace --exclude opit-speech-to-text`; Windows CI full workspace clippy/tests. App cannot compile on Linux here.
- [ ] Required batch gate (sequential): Linux worker runs `cargo fmt --all --check` and `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside the socket-restricted sandbox runs `cargo test --workspace --exclude opit-speech-to-text`. Windows CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; Linux cannot compile `crates/app` here, so do not substitute a cross-target check. Run `(cd ui && npm run check && npm test && npm run build)` on Linux; expect 0 errors AND 0 warnings, all tests passing, a successful Vite build and no unexplained change from the recorded before-counts. Record all evidence outside tracked files. Preserve behaviour and public facades; socket-binding tests are never silently skipped. After this commit, a read-only review and orchestrator CI on this exact commit must pass before the next task; workers never push.
- [ ] Commit:

  ```sh
  git add -A -- \
    crates/app/src/controller/tests.rs crates/app/src/controller/live_tests.rs crates/app/src/controller/tests/mod.rs crates/app/src/controller/tests/live_tests.rs \
    docs/reference/cleanup-styles.md docs/reference/app-rules.md docs/reference/live-text.md docs/reference/voice-modes.md
  git diff --cached --check
  git commit -m 'refactor(controller): relocate tests without changing discovery'
  ```

## Task 7 — Batch 6b: controller responsibilities without changing the actor

**Files:**

Create: `crates/app/src/controller/handle.rs`, `crates/app/src/controller/recording.rs`, `crates/app/src/controller/transcription.rs`, `crates/app/src/controller/delivery.rs`.

Modify: `crates/app/src/controller/mod.rs`, `crates/app/src/controller/status.rs`, `crates/app/src/controller/tests/mod.rs` (private implementation imports only), `docs/reference/live-text.md`, `docs/reference/voice-modes.md`, `docs/reference/cleanup-styles.md` (implementation path descriptions), `docs/development/known-issues.md` (native insertion acceptance evidence/status).

**Interfaces:**

Consumes: the unchanged 103-test controller suite; platform traits/fakes; `Providers`, `HistorySink`, `UiEvents`, `Env`, `SettingsHandle`. Produces: facade re-exports `controller::{ControllerHandle, Inbox, ControlAction, HoldRefusal, WorkGuard, UpdateHold, channel}` from private `handle`; `controller::{DictationState, DictationStatus, ErrorKind, meter}` from `status`; root `run` unchanged. `Msg`, actor storage/types and provider/event/history traits remain in `controller/mod.rs`. `recording`, `transcription`, `delivery` contain inherent `impl<P: Providers> Controller<P>` blocks, with `pub(super)` only for cross-module members. External imports used by AppCore, shell, IPC, file jobs and updates stay valid.

- [ ] Record Windows library discovery/counts/ignored set and the unchanged `controller::` list before edits. Preserve Task 6's logical test names. Read `controller/mod.rs`, `live.rs`, `status.rs` and the tests before splitting; all line ranges below refer to the original 2481-line actor file.
- [ ] `handle.rs`: move `ControllerHandle`, private `WorkState`, `HoldRefusal`, `WorkGuard`/Drop, `Inbox`, `ControlAction`, `UpdateHold`/Drop, `channel`, and the entire `impl ControllerHandle` (original 222–278, 299–318, 354–442). Make `ControllerHandle.tx`, `.status` and `Inbox.0` `pub(super)` so parent `run`/actor can access them; keep work-state details private to handle. Retain `TextReply`/Drop in the parent alongside job state. Re-export public handle items once; do not compile duplicated copies.
- [ ] `recording.rs`: move methods `ask`, `toggle`, `voice_shortcut`, `start_voice`, `voice_ready`, `on_hotkey`, `select_style`, `queue_style`, `style_saved`, `style_settings`, `style_changed`, `start`, `activate_recording`, `dictation_settings`, `stop`, `on_capture`, `arm_limit`, `pause_resume`; free `effective_paste`, `effective_style`. Keep their bodies, start-time snapshots, queue bounds, live start/stop, media guard lifetimes, style writer generations and selection safety unchanged.
- [ ] `transcription.rs`: move `clients`, `retry`, `begin_transcription`, `retry_history`, `on_salvaged`, `on_transcribed`; free `wait_for_retry`, `prepare_off_thread`; `CleanupLog` and its impls/Drop. Keep ordinary cleanup skip versus Ask/voice cancellation, monitored-worker panic reporting, retry ladder, in-memory prepared audio, live-to-batch fallback and history-off behaviour unchanged. Import `delivery::{save_transcription, HistoryContext}` and recording snapshot helpers with explicit module paths. Tests access the moved private `CleanupLog` via `super::transcription::CleanupLog`; only the type and its `new`/`finished` methods need `pub(super)` beyond their module.
- [ ] `delivery.rs`: move `finish_transcription`, `on_pasted`, `record`, `cancel`; `HistoryContext` and `save_transcription`. Give `HistoryContext` and its `style_id`/`voice` fields `pub(super)` visibility for transcription. Preserve blocking paste execution, no restore until completion, history write/replace ordering, TextReply terminal cancellation, trailing space handling, no auto-paste for history retry, and preservation of pending queued jobs. This is not the Linux session-result-recovery feature.
- [ ] `status.rs`: retain existing enums/serialization/conversions/two tests; append controller presentation methods `show_progress`, `fail`, `fail_with_message`, `finish_info`, `play`, `show_recording`, `show`, `set_state`, plus free `view`, `listening`, public `meter`. Add required parent imports, not duplicate state definitions. Keep `live.rs` and its inline test unchanged. Parent `mod.rs` retains all remaining state types (`Clients`, `VoiceMode`, `VoiceContext`, `VoicePrepared`, `Preparing`, `VoiceStartError`, `Job`, `Input`, `ActiveRecording`, `Pending`, `Phase`, `StyleWrite`, `PendingStyle`, `Controller`), `Msg`, `Providers`, `SetupError`/impl, `HistorySink`, `UiEvents`, `Env`, constants `PASTED_HIDE`, `INFO_HIDE`, `CANCEL_HIDE`, `WARNING_HIDE`, `ERROR_HIDE`, `ERROR_WITH_BUTTON_HIDE`, `MAX_WAITING`, `run`, methods `serve`, `handle`, `is_idle`, `control`, `held_for_update`, `pump`, and `now_ms`. Every original item now has one stated owner.
- [ ] Mark the moved methods called from parent/siblings `pub(super)`; leave purely local methods private. Parent-owned fields/types are visible to child modules without becoming public. Tests import moved free helpers explicitly where needed; retain `controller::tests::*` names, including the level-meter test. No task changes message variants, trait signatures, actor ordering, await points, channels, lock boundaries, Drop or abort semantics.
- [ ] Review Focus 1 check owned here: Windows owner/orchestrator performs before/after dictation into a normal editor using shortcut stop, then tray/UI stop (clipboard-only); start in a normal editor and finish by limit while an elevated editor has focus (UIPI, text available via manual paste); enable restore, replace clipboard during transcription and confirm the user's newer clipboard survives; queue two jobs and confirm paste/restore ordering. Repeat with History off, checking no new transcript files/rows. Use disposable data and synthetic non-private text. Record baseline versus after and retain pre-existing defects in known issues; do not claim fake `Paster` tests prove native behaviour. If Windows desktop is unavailable, keep this manual pass explicitly pending rather than claiming it passed.
- [ ] Windows orchestrator/CI: `cargo test -p opit-speech-to-text --lib controller::`; discovery before/after must match exactly (103 source tests; no test relocation in this commit). Run socket-binding cleanup/voice/Ask scenarios outside the worker sandbox. Linux here: `cargo fmt --all --check`, `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator portable `cargo test --workspace --exclude opit-speech-to-text`; Windows CI full workspace clippy/tests. App cannot compile on Linux. Review mechanically moved bodies against their pre-task versions; document only ownership/import/privacy changes.
- [ ] Required batch gate (sequential): Linux worker runs `cargo fmt --all --check` and `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside the socket-restricted sandbox runs `cargo test --workspace --exclude opit-speech-to-text`. Windows CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; Linux cannot compile `crates/app` here, so do not substitute a cross-target check. Run `(cd ui && npm run check && npm test && npm run build)` on Linux; expect 0 errors AND 0 warnings, all tests passing, a successful Vite build and no unexplained change from the recorded before-counts. Record all evidence outside tracked files. Preserve behaviour and public facades; socket-binding tests are never silently skipped. After this commit, a read-only review and orchestrator CI on this exact commit must pass before the next task; workers never push.
- [ ] Commit:

  ```sh
  git add -A -- \
    crates/app/src/controller/mod.rs crates/app/src/controller/status.rs crates/app/src/controller/handle.rs crates/app/src/controller/recording.rs \
    crates/app/src/controller/transcription.rs crates/app/src/controller/delivery.rs crates/app/src/controller/tests/mod.rs docs/reference/live-text.md \
    docs/reference/voice-modes.md docs/reference/cleanup-styles.md docs/development/known-issues.md
  git diff --cached --check
  git commit -m 'refactor(controller): separate handle capture transcription and delivery'
  ```

## Task 8 — Batch 7: core cleanup concern modules and unchanged facade

**Files:**

Create: `crates/core/src/cleanup/config.rs`, `crates/core/src/cleanup/prompts.rs`, `crates/core/src/cleanup/response.rs`, `crates/core/src/cleanup/transport.rs`. Modify `crates/core/src/cleanup/mod.rs`; retain `crates/core/src/cleanup/styles.rs` without splitting. Modify `docs/reference/cleanup-styles.md` (cleanup ownership reference only).

**Interfaces:**

Consumes: `opit_core::pipeline::{Transcript, TranscriptStatus}`, `provider::Profile`, `rules::RuleSet`, existing cleanup/style consumers throughout core/app. Produces unchanged facade `opit_core::cleanup::{CleanupConfig, CleanupPreset, CleanupStatus, CleanupError, CleanupStyle, CleanupClient, CleanupStats, ChatMessage, KEY_REF, EN_PROMPT, TR_PROMPT, presets, prompt_language, default_prompt, system_prompt, strip_answer, strip_answer_with_limit, eligible, apply, apply_with_stats}` and crate-visible `cleanup::legacy_config`. Keep `cleanup::styles::{StyleDefaults, BUILTIN_IDS, builtin_name, builtin_prompt, defaults, builtin_styles, valid_id}` and inherent CleanupConfig style methods working. Shared private helpers get `pub(super)` only; no Tauri/Windows/OS crate dependency enters core.

- [ ] Record Linux and Windows discovery separately: `cargo test -p opit-core --lib -- --list`, `cargo test -p opit-core --test cleanup_http -- --list`, `cargo test -p opit-core --test cleanup_styles_http -- --list`; ignored lists separately. Five inline `cleanup::tests` and five `cleanup::styles::tests` exist in inspected source. Record actual numeric integration counts rather than copying old release totals.
- [ ] `config.rs`: move `KEY_REF`, `CleanupConfig`/Default, `legacy_timeout_seconds`, `legacy_config`, `CleanupPreset`, `presets`, `CleanupStatus`/impl, `CleanupError`. Retain serde defaults, timeout migration and safe error messages exactly. `prompts.rs`: move `EN_FRAMING`, `TR_FRAMING`, `EN_PROMPT`, `TR_PROMPT`, `prompt_language`, `default_prompt`, `system_prompt`, `effective_style`. `effective_style` is `pub(super)` for transport. Keep `styles.rs` importing `super::{CleanupConfig, ...}` through the facade so its inherent impl remains on the same type.
- [ ] `response.rs`: move `strip_answer`, `editing_limit`, `TranscriptFrame`/impl, `strip_transcript_answer`, `has_word_overlap`, `is_transcript_meta_reply`, `references_transcript`, `prompt_leak_markers`, `transcript_meta_kinds`, `strip_answer_with_limit`. Grant `pub(super)` to only helpers transport/tests use (`editing_limit`, `TranscriptFrame`/`for_input` and opening/closing fields, `strip_transcript_answer`, `has_word_overlap`, `is_transcript_meta_reply`, `prompt_leak_markers`). Keep Unicode limits, delimiter selection, instruction-family detection and overlap guards byte-for-byte in their moved bodies.
- [ ] `transport.rs`: move `ChatMessage`, `CleanupClient` and its full inherent impl, `CleanupStats` and impl, `network_error`. Methods moving together are `new`, `http_client`, `request`, `request_with`, `warm_up`, `clean`, `output_budget`, `with_reasoning_headroom`, `custom_instructions`, `allows_expansion`, `output_limit`, `reasoning_effort`, `clean_with_stats`, `clean_subtitles`, `clean_inner`, `complete`, `complete_mode`, `complete_mode_with_stats`, `send_chat`, `send_hedged`, `send_once`. Keep total deadlines, fresh-pool hedge behaviour, status mapping, bounded body reads, reasoning budgets and cancellation untouched. Preserve crate-visible `clean_subtitles` for file jobs. Existing tests need `pub(super)` access to `output_limit`/`output_budget`; do not expose them publicly.
- [ ] `mod.rs`: retain `eligible`, `apply`, `apply_with_stats`, the original five inline tests, `pub mod styles`, and explicit public re-exports from the four concern modules; re-export `legacy_config` as `pub(crate)`. Import only the private helpers required by inline tests, cfg-gated if unused outside tests. Keep existing public import paths such as `crate::cleanup::CleanupConfig`, app `opit_core::cleanup::CleanupClient`, voice modes `strip_answer_with_limit`, and `styles::CleanupStyle`. No function body rewriting or new network operation.
- [ ] Linux here: `cargo fmt --all --check`; `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; `cargo test -p opit-core --lib cleanup::` (ten existing socket-free tests, same names/counts). Orchestrator outside sandbox: `cargo test -p opit-core --test cleanup_http`, `cargo test -p opit-core --test cleanup_styles_http`, `cargo test --workspace --exclude opit-speech-to-text`; expect zero failures and unchanged integration counts. Windows CI full workspace clippy/tests validates app consumers; Linux still cannot compile app. Compare discovered full names, ignored sets and counts exactly; public facade prevents test-prefix churn.
- [ ] Required batch gate (sequential): Linux worker runs `cargo fmt --all --check` and `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside the socket-restricted sandbox runs `cargo test --workspace --exclude opit-speech-to-text`. Windows CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; Linux cannot compile `crates/app` here, so do not substitute a cross-target check. Run `(cd ui && npm run check && npm test && npm run build)` on Linux; expect 0 errors AND 0 warnings, all tests passing, a successful Vite build and no unexplained change from the recorded before-counts. Record all evidence outside tracked files. Preserve behaviour and public facades; socket-binding tests are never silently skipped. After this commit, a read-only review and orchestrator CI on this exact commit must pass before the next task; workers never push.
- [ ] Commit:

  ```sh
  git add -A -- \
    crates/core/src/cleanup/mod.rs crates/core/src/cleanup/config.rs crates/core/src/cleanup/prompts.rs crates/core/src/cleanup/response.rs \
    crates/core/src/cleanup/transport.rs docs/reference/cleanup-styles.md
  git diff --cached --check
  git commit -m 'refactor(core): split cleanup behind the existing public facade'
  ```

## Task 9 — Batch 8: commands by domain with one invoke registry

**Files:**

Move/split `crates/app/src/commands.rs` → Create `crates/app/src/commands/mod.rs`, `crates/app/src/commands/app.rs`, `crates/app/src/commands/dictation.rs`, `crates/app/src/commands/config.rs`, `crates/app/src/commands/profiles.rs`, `crates/app/src/commands/credentials.rs`, `crates/app/src/commands/cleanup.rs`, `crates/app/src/commands/ask.rs`, `crates/app/src/commands/input.rs`, `crates/app/src/commands/rules.rs`, `crates/app/src/commands/history.rs`, `crates/app/src/commands/local.rs`, `crates/app/src/commands/files.rs`, `crates/app/src/commands/updates.rs`. Delete the old flat file after extraction. Modify `AGENTS.md`, `docs/reference/app-rules.md`, `docs/reference/live-text.md`, `docs/reference/cleanup-styles.md`, `docs/reference/voice-modes.md` (physical command paths only).

**Interfaces:**

Consumes: unchanged `app_core::{AppCore, CommandError, CorrectionDraft, HistoryUpdate, HotkeyState, MicTestSink, RulesPreview}`, root shell aliases, controller, file_jobs and local services. Produces unchanged `commands::handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static`, `commands::AppInfo` re-exported from `commands::app`. Parent-scoped `Core<'a> = State<'a, Arc<AppCore>>`, `Result<T>` and `blocking<T,F>` remain shared. Every JS invoke name, camelCase argument and serde payload remains unchanged; Tauri generated wrapper macros must be resolved at their defining module paths.

- [ ] Before editing, record the ordered 64-name registry from `tauri::generate_handler!` and every command signature/attribute; independently compare against `ui/src/lib/api.ts` (its Task 12 move has not happened). Record Windows app library/integration test counts; no existing tests live in the flat commands file.
- [ ] Move handlers using this exhaustive ownership map, without changing signatures, bodies, async markers or `#[tauri::command]` attributes:

  | File under `crates/app/src/commands/` | Items |
  |---|---|
  | `app.rs` | `AppInfo`, `app_info`, `take_startup_notices` |
  | `dictation.rs` | `get_status`, `toggle_dictation`, `cancel_dictation`, `retry_dictation` |
  | `config.rs` | `get_config`, `save_config`, `set_active_profile`, `set_active_style` |
  | `profiles.rs` | `profile_presets`, `streaming_supported`, `translate_languages` |
  | `credentials.rs` | `has_api_key`, `set_api_key`, `delete_api_key`, `test_connection` |
  | `cleanup.rs` | `cleanup_style_defaults`, `cleanup_presets`, `cleanup_prompts`, `CleanupTestResult` (parent-visible for macro expansion), `test_cleanup` |
  | `ask.rs` | `ask_status`, `test_ask` |
  | `input.rs` | `list_microphones`, `list_running_apps`, `mic_test_start`, `mic_test_stop`, `get_hotkey_state`, `set_hotkey_paused`, `set_hotkey_capture`, `validate_hotkey` |
  | `rules.rs` | `get_user_rules`, `save_user_rules`, `rules_preview`, `prompt_budget`, `rule_packs`, `parse_user_rules`, `render_user_rules`, `correction_draft`, `learning_apply`, `learning_dismiss` |
  | `history.rs` | `history_update_text`, `history_recent`, `usage_stats`, `history_search`, `history_delete`, `history_clear`, `history_audio`, `history_retry` |
  | `local.rs` | `local_state`, `local_download`, `local_cancel`, `local_delete` |
  | `files.rs` | `file_pick`, `file_state`, `file_start`, `file_cancel`, `file_handled`, `file_discard`, `file_render`, `file_save` |
  | `updates.rs` | `get_update_state`, `check_for_updates`, `install_update` |

- [ ] `mod.rs` holds only module declarations, shared aliases/helper, `pub use app::AppInfo`, and one `handler` with `tauri::generate_handler![app::app_info, ...]` entries in the original order. Use module-qualified paths for both handler and generated Tauri wrappers; a function-only re-export is insufficient for generated `__cmd__*` macros. Make handlers and `CleanupTestResult` `pub(super)` so parent registry expansion can name the function and its return type; retain Tauri's generated wrapper-macro visibility, without exporting new IPC. In child `updates.rs` avoid shadowing itself: import `crate::updates as update_service` and qualify service calls; analogous type/result/module names must remain unambiguous.
- [ ] Compare the pre/post registry as ordered `(wire name, argument names/types, result type, async/blocking placement)` records; expected 64 identical commands, none omitted/duplicated. Keep `history_audio`'s binary `tauri::ipc::Response`, file revision/unsaved guards and events, key input handling, `CommandError` shape and config-event ordering. No capabilities command or Linux UI wording change. Run Windows CI compilation to validate Tauri macro paths; source regex checks alone cannot establish macro correctness.
- [ ] Linux here: `cargo fmt --all --check`, `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside sandbox `cargo test --workspace --exclude opit-speech-to-text`; `(cd ui && npm run check && npm test && npm run build)` with 0 errors/0 warnings and unchanged test counts. Windows CI full workspace clippy/tests validates all handlers; app cannot compile on Linux here. Full app discovered names/counts stay unchanged; no tests move. Socket-binding tests remain orchestrator/CI only.
- [ ] Required batch gate (sequential): Linux worker runs `cargo fmt --all --check` and `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside the socket-restricted sandbox runs `cargo test --workspace --exclude opit-speech-to-text`. Windows CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; Linux cannot compile `crates/app` here, so do not substitute a cross-target check. Run `(cd ui && npm run check && npm test && npm run build)` on Linux; expect 0 errors AND 0 warnings, all tests passing, a successful Vite build and no unexplained change from the recorded before-counts. Record all evidence outside tracked files. Preserve behaviour and public facades; socket-binding tests are never silently skipped. After this commit, a read-only review and orchestrator CI on this exact commit must pass before the next task; workers never push.
- [ ] Commit:

  ```sh
  git add -A -- \
    crates/app/src/commands.rs crates/app/src/commands/mod.rs crates/app/src/commands/app.rs crates/app/src/commands/dictation.rs \
    crates/app/src/commands/config.rs crates/app/src/commands/profiles.rs crates/app/src/commands/credentials.rs crates/app/src/commands/cleanup.rs \
    crates/app/src/commands/ask.rs crates/app/src/commands/input.rs crates/app/src/commands/rules.rs crates/app/src/commands/history.rs \
    crates/app/src/commands/local.rs crates/app/src/commands/files.rs crates/app/src/commands/updates.rs AGENTS.md \
    docs/reference/app-rules.md docs/reference/live-text.md docs/reference/cleanup-styles.md docs/reference/voice-modes.md
  git diff --cached --check
  git commit -m 'refactor(commands): group invoke handlers by domain'
  ```

## Task 10 — Batch 9: IPC protocol, framing and transport

**Files:**

Create: `crates/ipc/src/protocol.rs`, `crates/ipc/src/framing.rs`, `crates/ipc/src/transport/mod.rs`. Move: `crates/ipc/src/windows.rs` → `crates/ipc/src/transport/windows.rs`. Modify: `crates/ipc/src/lib.rs`. No crate manifest, CLI or app dispatcher changes are needed.

**Interfaces:**

Consumes: existing `opit_ipc` root contracts used by `crates/cli/src/main.rs` and `crates/app/src/ipc.rs`. Produces: root `pub use protocol::{VERSION, MAX_REQUEST, MAX_RESPONSE, Request, Command, Format, Response, Outcome, Error, decode_request}` and `pub use framing::{read_frame, write_frame}`, `pub mod transport`, and `#[cfg(windows)] pub use transport::windows`. Preserve `opit_ipc::windows::{pipe_name, create_server, call, call_at}` exactly. `transport/mod.rs` declares only the Windows backend for now; the Linux port later adds Unix transport, not this task.

- [ ] Record Linux `cargo test -p opit-ipc -- --list` and Windows equivalent before editing, including ignored set. Inspected source: three root tests; four Windows transport tests, including one ignored real pipe roundtrip. Run per-platform before/after manifests; no cross-platform count comparison.
- [ ] Move protocol constants/types and their complete impls, `decode_request`, `roundtrip_and_versions`, `unknown_fields_verbs_and_invalid_arguments_are_safe` to `protocol.rs`. Move only `read_frame`, `write_frame`, `framing_limits_and_missing_newline` to `framing.rs`. Move Windows file whole, retaining identity, ACL, anti-squatting, server authentication, session name, timeout, no-mutation-retry and all tests. Re-export once; Windows implementation can keep `use crate::{...}` through the facade.
- [ ] Compare wire serialization and safety checks with the pre-task source: version 1, exact limits including newline, deny-unknown-fields behaviour, unit-variant extra-field rejection, transcript-free errors, authenticated-before-write client order, owner/session SDDL and first-pipe semantics. Keep CLI/app imports unchanged using `opit_ipc::windows`; do not add a Unix socket or enable Linux IPC.
- [ ] Linux here: `cargo fmt --all --check`, `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`, `cargo test -p opit-ipc` (three automatic socket-free tests). Orchestrator outside sandbox: `cargo test --workspace --exclude opit-speech-to-text`. Windows CI full workspace clippy/tests plus `cargo test -p opit-ipc`; ignored `cargo test -p opit-ipc transport::windows::tests::private_pipe_roundtrip -- --exact --ignored` is an explicit Windows orchestrator check, expected one passed. Map `tests::{roundtrip_and_versions, unknown_fields_verbs_and_invalid_arguments_are_safe}` → `protocol::tests::*`, `tests::framing_limits_and_missing_newline` → `framing::tests::*`, `windows::tests::*` → `transport::windows::tests::*`; same counts/ignored set. Linux still cannot compile the app.
- [ ] Required batch gate (sequential): Linux worker runs `cargo fmt --all --check` and `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside the socket-restricted sandbox runs `cargo test --workspace --exclude opit-speech-to-text`. Windows CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; Linux cannot compile `crates/app` here, so do not substitute a cross-target check. Run `(cd ui && npm run check && npm test && npm run build)` on Linux; expect 0 errors AND 0 warnings, all tests passing, a successful Vite build and no unexplained change from the recorded before-counts. Record all evidence outside tracked files. Preserve behaviour and public facades; socket-binding tests are never silently skipped. After this commit, a read-only review and orchestrator CI on this exact commit must pass before the next task; workers never push.
- [ ] Commit:

  ```sh
  git add -A -- \
    crates/ipc/src/lib.rs crates/ipc/src/protocol.rs crates/ipc/src/framing.rs crates/ipc/src/windows.rs \
    crates/ipc/src/transport/mod.rs crates/ipc/src/transport/windows.rs
  git diff --cached --check
  git commit -m 'refactor(ipc): separate protocol framing and Windows transport'
  ```

## Task 11 — Batch 10: local model runtime separation

**Files:**

Move: `crates/app/src/local/hardware.rs` → `crates/app/src/local/runtime/hardware.rs`; `crates/app/src/local/server.rs` → `crates/app/src/local/runtime/server.rs`.

Create: `crates/app/src/local/runtime/mod.rs`, `crates/app/src/local/runtime/windows.rs`. Modify: `crates/app/src/local/mod.rs`, `crates/app/src/local/catalogue.rs`, `crates/app/src/local/download.rs`, `crates/app/src/platform/windows/hardware.rs` (hardware type/helper import only).

**Interfaces:**

Consumes: `platform::process::Tree`, `platform::windows::hardware::detect`, `local::{LocalError, Result}`, `local::catalogue::{Artifact, MODELS}`, current LocalService. Produces: crate-visible `local::runtime::{BUILDS, extract, find_server}`, `local::runtime::hardware::{Hardware, HardwareCache, RETRY_AFTER, detect, recommendation, auto_threads}`, `local::runtime::server::{Process, RestartBudget, free_port, idle_expired, readiness}`. `local::catalogue::BUILDS` remains a public re-export of the runtime metadata for existing consumers. Preserve `local::{LocalService, Snapshot, Progress, LocalError, validate_config, storage_root}`, all command payloads, and download verification functions. No Linux local models or runtime are added. `runtime/windows.rs` contains portable-to-compile Windows metadata/archive policy with no native imports; it remains available to today's tests without claiming Linux runtime support.

- [ ] Record Windows app discovery/counts before editing, especially current local suites (after Task 2: portable hardware two, local server three, download five, LocalService eight = 18 automatic source tests; two native hardware tests stay in platform/windows). Capture existing build/model artifact arrays for byte comparison. Socket tests (`free_port`, wiremock LocalService tests) execute only on orchestrator/CI.
- [ ] Move portable hardware cache/model recommendation file whole into runtime, updating its Windows detection delegation import. Move server whole into runtime with `Process`, `RestartBudget`, `free_port`, `idle_expired`, `readiness`, their impls/Drop and three tests. Their `use super::{LocalError, Result}` must now resolve via runtime's parent imports or change explicitly to `crate::local::{LocalError, Result}`. Update `platform/windows/hardware.rs` to import the moved Hardware/auto_threads path; native hardware tests stay where Task 2 placed them, so their `--exact` helper argv must not change again.
- [ ] Split `catalogue.rs`: retain `Artifact` and the complete `MODELS` constant; move `BUILDS` (all four pinned Windows ZIP artifacts, original lines 11–40) to `runtime/windows.rs` and re-export via runtime and the old public catalogue path. Preserve IDs, filenames, URLs, byte sizes, SHA-256 values and order verbatim. Metadata remains usable in today's tests; putting Windows-only metadata in this file does not make Linux local models a supported feature.
- [ ] Split `download.rs`: retain `Source`, `allowed`, `verify`, `verify_cancellable`, `Part`/Drop, `fetch`, `publish`, and tests `publication_is_atomic_and_hash_failure_or_cancel_removes_partial_files`, `hashes_and_size_must_match`, `host_allowlist_is_https_and_source_specific`. Move `safe_name`, `extract`, `find_server` (original lines 147–267) and tests `archives_reject_links_missing_server_and_cancel_without_leaving_an_install`, `archives_reject_traversal_and_find_nested_server_with_dll` to `runtime/windows.rs`. Keep Windows reserved-name/ASCII/path validation, extraction bounds, symlink refusal, DLL placement, case-insensitive executable lookup and failed-install cleanup intact. Transfer `Write` imports with extraction; hash verification retains `Read`. Export existing runtime functions without adding a generalized filename or archive policy.
- [ ] `runtime/mod.rs` declares crate-visible hardware and parent-visible server/windows modules and re-exports `BUILDS`, `extract`, `find_server` to its parent. Keep `safe_name` private in `runtime/windows.rs`; its tests are in that same module and no external consumer needs it. In `local/mod.rs`, replace old hardware/server modules with runtime imports; switch `download::extract`/`download::find_server` to `runtime::extract`/`runtime::find_server`. Leave validation/storage policy and LocalService's current orchestration intact: preload, idle shutdown, one-restart budget, current config snapshots, health checks, closed-state guard and download progress. Keep `catalogue_is_complete_and_pinned` under `local::tests` through the facade.
- [ ] Review Focus 3 check: inspect actual `LocalService::endpoint` → runtime `Process::launch` → `platform::process::Tree`, and shell exit → `LocalService::shutdown` → Process Drop. Confirm no branch starts or publishes a process after `closed` becomes true; same executable/current_dir/argv, no content in argv/logs. On Windows orchestrator rerun `shutdown_finishes_while_initialization_is_blocked_and_prevents_late_launch`, `idle_shutdown_skips_inflight_requests_and_exit_always_kills_fake_process`, restart/readiness and both archive tests; expect each selected test actually discovered. Compare moved metadata and archive code mechanically; never download a model or start a real local server solely for this layout change.
- [ ] Linux here: `cargo fmt --all --check`, `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside sandbox `cargo test --workspace --exclude opit-speech-to-text`. Windows CI full workspace clippy/tests, plus orchestrator `cargo test -p opit-speech-to-text --lib local::`. Compare full app manifests mapping `local::hardware::tests` → `local::runtime::hardware::tests`, `local::server::tests` → `local::runtime::server::tests`, just the two archive test names from `local::download::tests` → `local::runtime::windows::tests`; other names/counts/ignored sets unchanged. App cannot compile on Linux here, including newly grouped portable files; no test is disabled to sidestep sockets.
- [ ] Required batch gate (sequential): Linux worker runs `cargo fmt --all --check` and `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`; orchestrator outside the socket-restricted sandbox runs `cargo test --workspace --exclude opit-speech-to-text`. Windows CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; Linux cannot compile `crates/app` here, so do not substitute a cross-target check. Run `(cd ui && npm run check && npm test && npm run build)` on Linux; expect 0 errors AND 0 warnings, all tests passing, a successful Vite build and no unexplained change from the recorded before-counts. Record all evidence outside tracked files. Preserve behaviour and public facades; socket-binding tests are never silently skipped. After this commit, a read-only review and orchestrator CI on this exact commit must pass before the next task; workers never push.
- [ ] Commit:

  ```sh
  git add -A -- \
    crates/app/src/local/mod.rs crates/app/src/local/catalogue.rs crates/app/src/local/download.rs crates/app/src/local/hardware.rs \
    crates/app/src/local/server.rs crates/app/src/local/runtime/mod.rs crates/app/src/local/runtime/hardware.rs crates/app/src/local/runtime/server.rs \
    crates/app/src/local/runtime/windows.rs crates/app/src/platform/windows/hardware.rs
  git diff --cached --check
  git commit -m 'refactor(local): separate model logic from runtime metadata and discovery'
  ```

## Task 12 — Batch 11: UI feature folders, preview and final documentation handoff

**Files:**

Move whole files according to this exhaustive table. Tests move beside their helper modules; every exported function/type and every Svelte component body stays intact.

| From | To |
|---|---|
| `ui/src/pages/Home.svelte` | `ui/src/features/home/Home.svelte` |
| `ui/src/pages/History.svelte` | `ui/src/features/history/History.svelte` |
| `ui/src/pages/CorrectionDialog.svelte` | `ui/src/features/history/CorrectionDialog.svelte` |
| `ui/src/pages/LearningDialog.svelte` | `ui/src/features/history/LearningDialog.svelte` |
| `ui/src/pages/Profiles.svelte` | `ui/src/features/profiles/Profiles.svelte` |
| `ui/src/pages/ProfileForm.svelte` | `ui/src/features/profiles/ProfileForm.svelte` |
| `ui/src/pages/Rules.svelte` | `ui/src/features/rules/Rules.svelte` |
| `ui/src/pages/RulesTable.svelte` | `ui/src/features/rules/RulesTable.svelte` |
| `ui/src/pages/Settings.svelte` | `ui/src/features/settings/Settings.svelte` |
| `ui/src/pages/Setup.svelte` | `ui/src/features/setup/Setup.svelte` |
| `ui/src/pages/Files.svelte` | `ui/src/features/files/Files.svelte` |
| `ui/src/pages/Guide.svelte` | `ui/src/features/guide/Guide.svelte` |
| `ui/src/pages/Usage.svelte` | `ui/src/features/usage/Usage.svelte` |
| `ui/src/pages/setup/StepCleanup.svelte` | `ui/src/features/setup/steps/StepCleanup.svelte` |
| `ui/src/pages/setup/StepLanguage.svelte` | `ui/src/features/setup/steps/StepLanguage.svelte` |
| `ui/src/pages/setup/StepMicrophone.svelte` | `ui/src/features/setup/steps/StepMicrophone.svelte` |
| `ui/src/pages/setup/StepProvider.svelte` | `ui/src/features/setup/steps/StepProvider.svelte` |
| `ui/src/pages/setup/StepRules.svelte` | `ui/src/features/setup/steps/StepRules.svelte` |
| `ui/src/pages/setup/StepShortcut.svelte` | `ui/src/features/setup/steps/StepShortcut.svelte` |
| `ui/src/pages/setup/StepTry.svelte` | `ui/src/features/setup/steps/StepTry.svelte` |
| `ui/src/lib/components/DictationControl.svelte` | `ui/src/features/dictation/DictationControl.svelte` |
| `ui/src/lib/components/HotkeyInput.svelte` | `ui/src/features/dictation/HotkeyInput.svelte` |
| `ui/src/lib/components/MicLevelTest.svelte` | `ui/src/features/dictation/MicLevelTest.svelte` |
| `ui/src/lib/components/LevelMeter.svelte` | `ui/src/features/dictation/LevelMeter.svelte` |
| `ui/src/lib/components/StatusBadge.svelte` | `ui/src/features/dictation/StatusBadge.svelte` |
| `ui/src/lib/components/CleanupMarker.svelte` | `ui/src/features/dictation/CleanupMarker.svelte` |
| `ui/src/lib/components/ModeBadge.svelte` | `ui/src/features/dictation/ModeBadge.svelte` |
| `ui/src/lib/components/AppRules.svelte` | `ui/src/features/settings/AppRules.svelte` |
| `ui/src/lib/components/AskSettings.svelte` | `ui/src/features/settings/AskSettings.svelte` |
| `ui/src/lib/components/CleanupSettings.svelte` | `ui/src/features/settings/CleanupSettings.svelte` |
| `ui/src/lib/components/CleanupStyles.svelte` | `ui/src/features/settings/CleanupStyles.svelte` |
| `ui/src/lib/components/LocalSettings.svelte` | `ui/src/features/profiles/LocalSettings.svelte` |
| `ui/src/lib/components/DictationDemo.svelte` | `ui/src/features/guide/DictationDemo.svelte` |
| `ui/src/lib/components/UpdateInstallButton.svelte` | `ui/src/features/updates/UpdateInstallButton.svelte` |
| `ui/src/lib/components/UpdateProgress.svelte` | `ui/src/features/updates/UpdateProgress.svelte` |
| `ui/src/lib/api.ts` | `ui/src/lib/bridge/api.ts` |
| `ui/src/lib/events.ts` | `ui/src/lib/bridge/events.ts` |
| `ui/src/lib/types.ts` | `ui/src/lib/bridge/types.ts` |
| `ui/src/lib/errors.ts` | `ui/src/lib/bridge/errors.ts` |
| `ui/src/lib/errors.test.ts` | `ui/src/lib/bridge/errors.test.ts` |
| `ui/src/lib/app.svelte.ts` | `ui/src/lib/state/app.svelte.ts` |
| `ui/src/lib/toast.svelte.ts` | `ui/src/lib/state/toast.svelte.ts` |
| `ui/src/lib/route.ts` | `ui/src/lib/navigation/route.ts` |
| `ui/src/lib/route.test.ts` | `ui/src/lib/navigation/route.test.ts` |
| `ui/src/lib/router.svelte.ts` | `ui/src/lib/navigation/router.svelte.ts` |
| `ui/src/lib/format.ts` | `ui/src/lib/utils/format.ts` |
| `ui/src/lib/format.test.ts` | `ui/src/lib/utils/format.test.ts` |
| `ui/src/lib/number-input.ts` | `ui/src/lib/utils/number-input.ts` |
| `ui/src/lib/number-input.test.ts` | `ui/src/lib/utils/number-input.test.ts` |
| `ui/src/lib/serial.ts` | `ui/src/lib/utils/serial.ts` |
| `ui/src/lib/serial.test.ts` | `ui/src/lib/utils/serial.test.ts` |
| `ui/src/lib/theme.ts` | `ui/src/lib/utils/theme.ts` |
| `ui/src/lib/theme.test.ts` | `ui/src/lib/utils/theme.test.ts` |
| `ui/src/lib/tooltip.ts` | `ui/src/lib/utils/tooltip.ts` |
| `ui/src/lib/tooltip.test.ts` | `ui/src/lib/utils/tooltip.test.ts` |
| `ui/src/lib/hotkey.ts` | `ui/src/features/dictation/hotkey.ts` |
| `ui/src/lib/hotkey.test.ts` | `ui/src/features/dictation/hotkey.test.ts` |
| `ui/src/lib/voice-modes.ts` | `ui/src/features/dictation/voice-modes.ts` |
| `ui/src/lib/voice-modes.test.ts` | `ui/src/features/dictation/voice-modes.test.ts` |
| `ui/src/lib/styles.ts` | `ui/src/features/dictation/styles.ts` |
| `ui/src/lib/styles.test.ts` | `ui/src/features/dictation/styles.test.ts` |
| `ui/src/lib/speech-detection.ts` | `ui/src/features/dictation/speech-detection.ts` |
| `ui/src/lib/speech-detection.test.ts` | `ui/src/features/dictation/speech-detection.test.ts` |
| `ui/src/lib/history.ts` | `ui/src/features/history/history.ts` |
| `ui/src/lib/history.test.ts` | `ui/src/features/history/history.test.ts` |
| `ui/src/lib/learning.ts` | `ui/src/features/history/learning.ts` |
| `ui/src/lib/learning.test.ts` | `ui/src/features/history/learning.test.ts` |
| `ui/src/lib/selection.ts` | `ui/src/features/history/selection.ts` |
| `ui/src/lib/selection.test.ts` | `ui/src/features/history/selection.test.ts` |
| `ui/src/lib/profiles.ts` | `ui/src/features/profiles/profiles.ts` |
| `ui/src/lib/profiles.test.ts` | `ui/src/features/profiles/profiles.test.ts` |
| `ui/src/lib/local.ts` | `ui/src/features/profiles/local.ts` |
| `ui/src/lib/local.test.ts` | `ui/src/features/profiles/local.test.ts` |
| `ui/src/lib/rules.ts` | `ui/src/features/rules/rules.ts` |
| `ui/src/lib/rules.test.ts` | `ui/src/features/rules/rules.test.ts` |
| `ui/src/lib/app-rules.ts` | `ui/src/features/settings/app-rules.ts` |
| `ui/src/lib/app-rules.test.ts` | `ui/src/features/settings/app-rules.test.ts` |
| `ui/src/lib/cleanup.ts` | `ui/src/features/settings/cleanup.ts` |
| `ui/src/lib/cleanup.test.ts` | `ui/src/features/settings/cleanup.test.ts` |
| `ui/src/lib/files.ts` | `ui/src/features/files/files.ts` |
| `ui/src/lib/files.test.ts` | `ui/src/features/files/files.test.ts` |
| `ui/src/lib/guide-demo.ts` | `ui/src/features/guide/guide-demo.ts` |
| `ui/src/lib/guide-demo.test.ts` | `ui/src/features/guide/guide-demo.test.ts` |
| `ui/src/lib/usage.ts` | `ui/src/features/usage/usage.ts` |
| `ui/src/lib/usage.test.ts` | `ui/src/features/usage/usage.test.ts` |
| `ui/src/lib/update.ts` | `ui/src/features/updates/update.ts` |
| `ui/src/lib/update.test.ts` | `ui/src/features/updates/update.test.ts` |
| `ui/src/guide-preview.ts` | `ui/src/preview/guide-preview.ts` |
| `ui/src/BrowserPreview.svelte` | `ui/src/preview/BrowserPreview.svelte` |
| `docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md` | `docs/design/2026-09-30-opit-speech-to-text-design.md` |
| `docs/superpowers/specs/2026-10-30-layout-and-linux-design.md` | `docs/design/2026-10-30-layout-and-linux-design.md` |
| `docs/superpowers/plans/2026-10-30-layout-cleanup.md` | `docs/archive/2026-10-30-layout-cleanup.md` |

Modify (imports/source-path references only, plus the final documentation index/rules changes below):

- `ui/src/App.svelte`
- `ui/src/lib/i18n/index.ts`
- `ui/guide-preview.html`
- `ui/src/lib/components/Banner.svelte`
- `ui/src/lib/components/Button.svelte`
- `ui/src/lib/components/Dialog.svelte`
- `ui/src/lib/components/Field.svelte`
- `ui/src/lib/components/Icon.svelte`
- `ui/src/lib/components/InfoTip.svelte`
- `ui/src/lib/components/PageHeader.svelte`
- `ui/src/lib/components/Select.svelte`
- `ui/src/lib/components/Switch.svelte`
- `ui/src/lib/components/Toasts.svelte`
- `AGENTS.md`
- `README.md`
- `docs/README.md`
- `docs/development/known-issues.md`
- `docs/reference/app-rules.md`
- `docs/reference/cleanup-styles.md`
- `docs/reference/live-text.md`
- `docs/reference/voice-modes.md`

No edits to `ui/src/app.css`, `ui/src/main.ts`, `ui/src/lib/i18n/en.ts`, `ui/src/lib/i18n/tr.ts`, `ui/vite.config.ts`, package manifests or lockfiles are needed. In particular, `Settings.svelte` moves whole and is not split. Do not move/split rule packs, scripts, Rust config/history, overlay or paster.

**Interfaces:**

Consumes: current Rust command/event/serde contracts and the UI's existing exports. Produces the following exact import contracts (all other exports of moved whole files remain intact too):

| Module path relative to `ui/src/` | Required exports/consumers |
|---|---|
| `lib/bridge/api.ts` | `api`, all existing typed methods and camelCase invoke args; used by pages, state and feature components |
| `lib/bridge/events.ts` | `onStatus`, `onHistoryAdded`, `onHistoryUpdated`, `onRulesChanged`, `onConfigChanged`, `onNavigate`, `onHotkeyState`, `onMicTest`, `onUpdateState`, `onFileProgress`, `onLocalProgress`, `UnlistenFn` |
| `lib/bridge/types.ts` | all current exports, including `AppConfig`, `AppInfo`, `CommandError`, `DictationStatus`, `Dictation`, `CleanupStyle`, `FileState`, `FileWork`, `UpdateState`, `LocalState`, `PROMPT_TOKEN_BUDGET`; no payload reshaping |
| `lib/bridge/errors.ts` | `describeError`, `isCommandError` |
| `lib/state/app.svelte.ts` | singleton `app`, `loadStyleDefaults`, `checkForUpdates`, `t`, `errorText`, `initApp`, serialized `saveConfig`, `setActiveStyle` |
| `lib/state/toast.svelte.ts` | `Toast`, `toasts`, `toast` |
| `lib/navigation/route.ts`, `router.svelte.ts` | `ROUTES`, `Route`, `parseRoute`, `routeHash`; `router`, `navigate` |
| `lib/i18n/index.ts` | unchanged `MessageKey`, `Params`, `resolveLang`, `translate`, `placeholders`, `hasKey`; types import points to bridge |
| `lib/utils/format.ts`, `number-input.ts`, `serial.ts`, `theme.ts`, `tooltip.ts` | existing format functions; `duckPercentRange`, `parseNumberInput`; `createSerialQueue`; `resolveTheme`, `watchTheme`; `AnchorRect`, `BoxSize`, `TooltipPlacement`, `placeTooltip` |
| `features/dictation/hotkey.ts` | `ComboRecorder`, `SELECTABLE_KEYS`, `keyNameFromCode`, `sortCombo`, `comboProblem`, `formatCombo`, `ComboProblem` |
| `features/dictation/voice-modes.ts` | `languageOptions`, `isVoiceMode`, `modeBadgeKey`, `canRetryRow`, `originalOf`, `cleanupConnectionReady`, `voiceModesInUse` |
| `features/dictation/styles.ts` | `BUILTIN_STYLE_IDS`, `isBuiltinStyle`, `newStyleId`, `newStyle`, `styleDisplayName`, `storedStyleName`, `historyStyleName`, `defaultStylePrompt`, `styleProblem`, `prepareStyle`, `upsertStyle`, `deleteStyle`, `StyleProblem` |
| `features/dictation/speech-detection.ts` | `speechDetectionDefaults`, `rmsToDb` |
| `features/history/history.ts`, `learning.ts`, `selection.ts` | `HISTORY_PAGE`, `refreshWindow`, `ListToken`, `sameList`, `refreshHistory`; existing edit/learning helpers and `MAX_HISTORY_TEXT_BYTES`; `expandToWords` |
| `features/profiles/profiles.ts`, `local.ts`, `ProfileForm.svelte` | preset/profile validation, `LANGUAGES`, `PROVIDER_KEYS`, live-text helpers; `localInteger`, `localStatus`; default component and named `KeySaveError` |
| `features/rules/rules.ts` | `hasInlineComments`, `budgetLevel` |
| `features/settings/app-rules.ts`, `cleanup.ts` | per-app rule helpers/constants (`PASTE_METHOD_KEYS`, `PASTE_AFTER_KEYS`, `newRuleId`, `newRule`, `foldExe`, `exeProblem`, `prepareRule`, `moveRule`, `upsertRule`, `deleteRule`, `ruleSummary`, `ExeProblem`, `SummaryPart`); `cleanupValid`, `cleanupPreset`, `withLiveStyles` |
| `features/files/files.ts` | `fileRunning`, `mergeFileEvent`, `acceptFileSnapshot`, `mergeFileWork`, `fileBlocksUpdate` |
| `features/guide/guide-demo.ts`, `features/usage/usage.ts` | `DEMO_STEPS`, `DemoStep`, `nextDemoStep`; `UsagePeriod`, `usageBounds` |
| `features/updates/update.ts` | `checkUpdateState`, `bannerVersion`, `progressPercent`, `statusMessage`, `dictationBusy`, `keepInstallError` |
| `preview/guide-preview.ts`, `preview/BrowserPreview.svelte` | dev-only bootstrap/mockIPC, default preview component; `ui/guide-preview.html` stays at the same public URL |

Root `App.svelte` still switches the same routes, now importing feature page components. Primitives remaining in `lib/components/` are Banner, Button, Dialog, Field, Icon, InfoTip, PageHeader, Select, Switch and Toasts. Domain components live in features and may import primitives; primitives may import shared state/utilities but must not become a second state/bootstrap implementation. Shared feature helpers are imported by exact module paths; add no barrel or compatibility copy that duplicates rune state. Linux-specific capabilities, labels and validators remain for the separate port plan.

- [ ] Record status, then the baseline UI gate sequentially: `(cd ui && npm run check)`, `(cd ui && npm test -- --reporter=json)` (save machine-readable suite/test names/counts under `/tmp/opit-layout-task-12/`), `(cd ui && npm run build)`. Expect 0 errors AND 0 warnings and successful build; record numeric tests and test-file counts, not historical plan numbers. Record Rust discovery as well; no Rust tests move here. Use installed dependencies; orchestrator prepares missing ones without lockfile edits.
- [ ] Apply every file move in the table mechanically, preserving all component props, snippets, module-context exports (especially `ProfileForm`'s `KeySaveError`), lifecycle handlers, local form drafts and styles. Move the seven setup steps into `features/setup/steps/`; update Setup's imports. Do not split Settings or i18n dictionaries, change UI copy, redesign components, add dependencies, generalize `.exe` validation, add Super labels or capabilities.
- [ ] Rewrite every relative import to its actual new target, including imports in all moved tests, remaining primitive components, App, i18n index and preview. Notable cross-feature edges: Home → history labels/voice helpers and dictation control; History → dictation mode/style labels; settings → dictation hotkey/mic, profile local settings and updates; setup → profile helpers/local settings, settings cleanup and dictation mic/hotkey; state → files merge and updates state helpers; updates → files work guard; voice-mode helper → settings cleanup validation. Keep a single `lib/state/app.svelte.ts` and `lib/navigation/router.svelte.ts` instance. Preserve `saveConfig` serialization and on-mount reloading/unlisten logic exactly.
- [ ] Repair preview-specific paths: HTML script `/src/guide-preview.ts` → `/src/preview/guide-preview.ts`; preview bootstrap CSS import `./app.css` → `../app.css`; BrowserPreview's App import `./App.svelte` → `../App.svelte`; rewrite bridge/helper/state imports in both preview files. Keep `guide-preview.html` at the UI root and its URL/README usage unchanged. Keep `import.meta.env.DEV` guard and mockIPC registration before mount; production `main.ts` still imports App directly, never preview. `vite.config.ts` still discovers `src/**/*.test.ts` and builds the default index entry; tsconfig already includes features/preview, so no discovery narrowing is needed.
- [ ] Update AGENTS's actual final tree and relocated shared UI paths: bridge API/types/events, state `app.svelte.ts`, shell native strings, feature pages/components, primitive components and docs. Preserve the rule intent of using existing components/tokens while permitting the feature components this spec explicitly requires. Fix only broken paths in root README: original design brief → `docs/design/2026-09-30-opit-speech-to-text-design.md`; no prose/claim rewriting. Fix current reference documents' physical UI/component/helper paths and docs index/known-issues references.
- [ ] Only now move both specs to `docs/design/` and this plan to `docs/archive/` using the table. In the relocated layout spec and original brief, repair `docs/RELEASE-CHECKLIST.md` → `docs/release/RELEASE-CHECKLIST.md`; leave design decisions/status/history intact. Update this plan's **Spec** pointer to `docs/design/2026-10-30-layout-and-linux-design.md` and all live incoming plan/spec links in AGENTS, docs index and known issues. Historical old-path descriptions/move tables in this archived plan remain historical. Do not delete the active spec or this plan, and do not archive any future Linux plan prematurely.
- [ ] Review Focus 4 check: after `npm run check`, `npm test` and `npm run build` pass, orchestrator starts `(cd ui && npm run dev -- --host 127.0.0.1 --port 5199)` outside the socket-restricted worker sandbox. In a browser visit `http://127.0.0.1:5199/guide-preview.html#/guide`; all requested relative module URLs must return 200, with no import/console errors. Visit `#/home`, `#/history`, `#/profiles`, `#/rules`, `#/settings`, `#/files`, `#/usage`, `#/guide`, and `?setup=true#/setup`; walk all seven steps. Repeat with `?lang=en` and default Turkish, both light/dark via existing Settings, and `?recording=true#/home` plus reload. Exercise dictation control, settings nested controls, rule/profile dialogs and file preview, using synthetic preview data only. Expect same rendering/actions as before, no live provider/file/process operations. Inspect the production `ui/dist` JS import graph for `mockIPC`, `shouldMockEvents`, `Guide preview is only available`, `BrowserPreview`: none may be reachable from index. Preserve the DEV guard and default production input rather than bundling preview to make the check pass. Owner Windows acceptance additionally closes/reopens the real WebView while recording and checks live status/listeners; browser mocks do not prove that pass.
- [ ] Verification: Linux here runs `cargo fmt --all --check`, `cargo clippy --workspace --exclude opit-speech-to-text --all-targets -- -D warnings`, then `(cd ui && npm run check && npm test && npm run build)` sequentially; expect 0 errors/0 warnings, same test count/file count and names after mapping only the source-file moves. Compare JSON manifests by test full name plus mapped filepath; no test/glob is dropped or duplicated. Orchestrator outside sandbox runs `cargo test --workspace --exclude opit-speech-to-text`; Windows CI runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Linux cannot compile app in this phase. All socket tests and preview server/browser work are explicitly outside the worker sandbox. Release helpers are unchanged; run `node --test scripts/release/*.test.mjs` as final integrity check, with no asset publication.
- [ ] Review Focus 5 check: run the local Markdown target/fragment scan after final moves, with zero unintended broken targets. Check all active incoming references using `rg -n 'docs/superpowers|docs/(RELEASE-CHECKLIST|FIRST-RELEASE|PUBLICATION-REPORT|eval)\.md|src/pages|src/lib/(api|events|types|app\.svelte)|src/guide-preview|src/BrowserPreview' AGENTS.md README.md CONTRIBUTING.md docs ui .github`; classify archived historical move descriptions as intentional, repair all live links/imports. Confirm Linux status is still an empty-of-claims skeleton. Review root README diff against the task's parent: path substitutions only. All batches 1–11 are now represented; local models/Unix backends/packaging/Linux UI remain unimplemented by design, for the post-spike plan.
- [ ] Prepare the exact commit/diff, before/after manifests, prior batches' Windows/Linux/UI CI evidence and explicit pending desktop acceptance for the post-commit read-only review. The orchestrator must obtain Windows/Linux/UI CI for this final commit before declaring the batch complete. Confirm no product/version/identity, updater key/endpoint, asset filename, data directory, credential suffix, autostart value, NSIS guard, behaviour, API or privacy change. Inspect `git diff --cached --name-status` against the exhaustive move/modify table. Commit:

  ```sh
  git add -A -- \
    ui/src/pages/Home.svelte ui/src/features/home/Home.svelte ui/src/pages/History.svelte ui/src/features/history/History.svelte \
    ui/src/pages/CorrectionDialog.svelte ui/src/features/history/CorrectionDialog.svelte ui/src/pages/LearningDialog.svelte ui/src/features/history/LearningDialog.svelte \
    ui/src/pages/Profiles.svelte ui/src/features/profiles/Profiles.svelte ui/src/pages/ProfileForm.svelte ui/src/features/profiles/ProfileForm.svelte \
    ui/src/pages/Rules.svelte ui/src/features/rules/Rules.svelte ui/src/pages/RulesTable.svelte ui/src/features/rules/RulesTable.svelte \
    ui/src/pages/Settings.svelte ui/src/features/settings/Settings.svelte ui/src/pages/Setup.svelte ui/src/features/setup/Setup.svelte \
    ui/src/pages/Files.svelte ui/src/features/files/Files.svelte ui/src/pages/Guide.svelte ui/src/features/guide/Guide.svelte \
    ui/src/pages/Usage.svelte ui/src/features/usage/Usage.svelte ui/src/pages/setup/StepCleanup.svelte ui/src/features/setup/steps/StepCleanup.svelte \
    ui/src/pages/setup/StepLanguage.svelte ui/src/features/setup/steps/StepLanguage.svelte ui/src/pages/setup/StepMicrophone.svelte ui/src/features/setup/steps/StepMicrophone.svelte \
    ui/src/pages/setup/StepProvider.svelte ui/src/features/setup/steps/StepProvider.svelte ui/src/pages/setup/StepRules.svelte ui/src/features/setup/steps/StepRules.svelte \
    ui/src/pages/setup/StepShortcut.svelte ui/src/features/setup/steps/StepShortcut.svelte ui/src/pages/setup/StepTry.svelte ui/src/features/setup/steps/StepTry.svelte \
    ui/src/lib/components/DictationControl.svelte ui/src/features/dictation/DictationControl.svelte ui/src/lib/components/HotkeyInput.svelte ui/src/features/dictation/HotkeyInput.svelte \
    ui/src/lib/components/MicLevelTest.svelte ui/src/features/dictation/MicLevelTest.svelte ui/src/lib/components/LevelMeter.svelte ui/src/features/dictation/LevelMeter.svelte \
    ui/src/lib/components/StatusBadge.svelte ui/src/features/dictation/StatusBadge.svelte ui/src/lib/components/CleanupMarker.svelte ui/src/features/dictation/CleanupMarker.svelte \
    ui/src/lib/components/ModeBadge.svelte ui/src/features/dictation/ModeBadge.svelte ui/src/lib/components/AppRules.svelte ui/src/features/settings/AppRules.svelte \
    ui/src/lib/components/AskSettings.svelte ui/src/features/settings/AskSettings.svelte ui/src/lib/components/CleanupSettings.svelte ui/src/features/settings/CleanupSettings.svelte \
    ui/src/lib/components/CleanupStyles.svelte ui/src/features/settings/CleanupStyles.svelte ui/src/lib/components/LocalSettings.svelte ui/src/features/profiles/LocalSettings.svelte \
    ui/src/lib/components/DictationDemo.svelte ui/src/features/guide/DictationDemo.svelte ui/src/lib/components/UpdateInstallButton.svelte ui/src/features/updates/UpdateInstallButton.svelte \
    ui/src/lib/components/UpdateProgress.svelte ui/src/features/updates/UpdateProgress.svelte ui/src/lib/api.ts ui/src/lib/bridge/api.ts \
    ui/src/lib/events.ts ui/src/lib/bridge/events.ts ui/src/lib/types.ts ui/src/lib/bridge/types.ts \
    ui/src/lib/errors.ts ui/src/lib/bridge/errors.ts ui/src/lib/errors.test.ts ui/src/lib/bridge/errors.test.ts \
    ui/src/lib/app.svelte.ts ui/src/lib/state/app.svelte.ts ui/src/lib/toast.svelte.ts ui/src/lib/state/toast.svelte.ts \
    ui/src/lib/route.ts ui/src/lib/navigation/route.ts ui/src/lib/route.test.ts ui/src/lib/navigation/route.test.ts \
    ui/src/lib/router.svelte.ts ui/src/lib/navigation/router.svelte.ts ui/src/lib/format.ts ui/src/lib/utils/format.ts \
    ui/src/lib/format.test.ts ui/src/lib/utils/format.test.ts ui/src/lib/number-input.ts ui/src/lib/utils/number-input.ts \
    ui/src/lib/number-input.test.ts ui/src/lib/utils/number-input.test.ts ui/src/lib/serial.ts ui/src/lib/utils/serial.ts \
    ui/src/lib/serial.test.ts ui/src/lib/utils/serial.test.ts ui/src/lib/theme.ts ui/src/lib/utils/theme.ts \
    ui/src/lib/theme.test.ts ui/src/lib/utils/theme.test.ts ui/src/lib/tooltip.ts ui/src/lib/utils/tooltip.ts \
    ui/src/lib/tooltip.test.ts ui/src/lib/utils/tooltip.test.ts ui/src/lib/hotkey.ts ui/src/features/dictation/hotkey.ts \
    ui/src/lib/hotkey.test.ts ui/src/features/dictation/hotkey.test.ts ui/src/lib/voice-modes.ts ui/src/features/dictation/voice-modes.ts \
    ui/src/lib/voice-modes.test.ts ui/src/features/dictation/voice-modes.test.ts ui/src/lib/styles.ts ui/src/features/dictation/styles.ts \
    ui/src/lib/styles.test.ts ui/src/features/dictation/styles.test.ts ui/src/lib/speech-detection.ts ui/src/features/dictation/speech-detection.ts \
    ui/src/lib/speech-detection.test.ts ui/src/features/dictation/speech-detection.test.ts ui/src/lib/history.ts ui/src/features/history/history.ts \
    ui/src/lib/history.test.ts ui/src/features/history/history.test.ts ui/src/lib/learning.ts ui/src/features/history/learning.ts \
    ui/src/lib/learning.test.ts ui/src/features/history/learning.test.ts ui/src/lib/selection.ts ui/src/features/history/selection.ts \
    ui/src/lib/selection.test.ts ui/src/features/history/selection.test.ts ui/src/lib/profiles.ts ui/src/features/profiles/profiles.ts \
    ui/src/lib/profiles.test.ts ui/src/features/profiles/profiles.test.ts ui/src/lib/local.ts ui/src/features/profiles/local.ts \
    ui/src/lib/local.test.ts ui/src/features/profiles/local.test.ts ui/src/lib/rules.ts ui/src/features/rules/rules.ts \
    ui/src/lib/rules.test.ts ui/src/features/rules/rules.test.ts ui/src/lib/app-rules.ts ui/src/features/settings/app-rules.ts \
    ui/src/lib/app-rules.test.ts ui/src/features/settings/app-rules.test.ts ui/src/lib/cleanup.ts ui/src/features/settings/cleanup.ts \
    ui/src/lib/cleanup.test.ts ui/src/features/settings/cleanup.test.ts ui/src/lib/files.ts ui/src/features/files/files.ts \
    ui/src/lib/files.test.ts ui/src/features/files/files.test.ts ui/src/lib/guide-demo.ts ui/src/features/guide/guide-demo.ts \
    ui/src/lib/guide-demo.test.ts ui/src/features/guide/guide-demo.test.ts ui/src/lib/usage.ts ui/src/features/usage/usage.ts \
    ui/src/lib/usage.test.ts ui/src/features/usage/usage.test.ts ui/src/lib/update.ts ui/src/features/updates/update.ts \
    ui/src/lib/update.test.ts ui/src/features/updates/update.test.ts ui/src/guide-preview.ts ui/src/preview/guide-preview.ts \
    ui/src/BrowserPreview.svelte ui/src/preview/BrowserPreview.svelte docs/superpowers/specs/2026-09-30-opit-speech-to-text-design.md docs/design/2026-09-30-opit-speech-to-text-design.md \
    docs/superpowers/specs/2026-10-30-layout-and-linux-design.md docs/design/2026-10-30-layout-and-linux-design.md docs/superpowers/plans/2026-10-30-layout-cleanup.md docs/archive/2026-10-30-layout-cleanup.md \
    ui/src/App.svelte ui/src/lib/i18n/index.ts ui/guide-preview.html ui/src/lib/components/Banner.svelte \
    ui/src/lib/components/Button.svelte ui/src/lib/components/Dialog.svelte ui/src/lib/components/Field.svelte ui/src/lib/components/Icon.svelte \
    ui/src/lib/components/InfoTip.svelte ui/src/lib/components/PageHeader.svelte ui/src/lib/components/Select.svelte ui/src/lib/components/Switch.svelte \
    ui/src/lib/components/Toasts.svelte AGENTS.md README.md docs/README.md \
    docs/development/known-issues.md docs/reference/app-rules.md docs/reference/cleanup-styles.md docs/reference/live-text.md \
    docs/reference/voice-modes.md
  git diff --cached --check
  git commit -m 'refactor(ui): organize features shared modules and browser preview'
  ```

## Plan self-review and completion criteria

- [ ] Coverage: Tasks 1–5 map to batches 1–5; Tasks 6/7 to controller batch 6; Tasks 8–12 to batches 7–11. Each task is one commit, with explicit files, consumed/produced interfaces, verification ownership and an exact staging command; no push step is delegated to workers.
- [ ] Names: `app_core::AppCore`/`Platform`, root shell aliases, `opit_ipc::windows` and `opit_core::cleanup` facade stay valid. Controller test modules stay logically named; embedded process/hardware `--exact` argv is updated; inline `include_str!("i18n.rs")` stays beside the moved source. Resource/build script paths and preview entry are accounted for.
- [ ] Scope: no Linux implementation/spike/packaging/capabilities, redesign, version bump, key rotation, Windows policy change or unsolicited bug fixes. Root README changes are link repairs only; active spec/plan survive until final task and are then retained at their stated final paths.
- [ ] Proof: all moved suites have recorded runnable before-counts, after-counts and complete name comparisons on the same platform. Linux app limitations, outside-sandbox socket tests and pending native desktop acceptance are explicit. No code bodies or unresolved implementation choices are embedded in this plan.
- [ ] Writing deliverable only: before handing this plan to the user, `git diff --check` is clean and `git status --short` contains exactly `?? docs/superpowers/plans/2026-10-30-layout-cleanup.md`. Do not execute the implementation commits while authoring this file.
