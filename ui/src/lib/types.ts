// TypeScript mirrors of the Rust payloads (serde output). Keep in sync with
// crates/core (config, provider, history, rules) and crates/app (commands, app_core, controller).

export type Lang = "en" | "tr";
export type Theme = "system" | "light" | "dark";

export type ErrorKind =
  | "missing_key" | "invalid_key" | "too_large" | "rate_limited" | "server" | "network" | "timeout"
  | "bad_response" | "rejected" | "audio" | "microphone" | "credentials" | "paste" | "cancelled";

export type DictationState = "idle" | "recording" | "transcribing" | "cleaning" | "asking" | "pasting" | "cancelled" | "error";

export interface DictationStatus {
  state: DictationState;
  can_retry: boolean;
  waiting: number;
  paused: boolean;
  retry_wait?: { kind: ErrorKind; seconds: number; attempt: number; total: number };
  /** Only when state === "error". */
  kind?: ErrorKind;
  /** Only when state === "error"; already localized by Rust. */
  message?: string;
}

export type AudioFormat = "flac" | "wav";
export type ResponseFormat = "verbose_json" | "json";
export type ProviderKind = "openai_compatible" | "elevenlabs" | "local_whisper" | "deepgram";

export interface LocalConfig {
  build: string;
  threads: number;
  gpu: boolean;
  idle_minutes: number;
  preload: boolean;
  models_folder: string | null;
}
export interface LocalArtifact { id: string; file: string; url: string; size: number; sha256: string }
export interface LocalProgress { id: string; received: number; total: number }
export interface LocalState {
  builds: LocalArtifact[];
  models: LocalArtifact[];
  installed: string[];
  progress: LocalProgress | null;
  running_model: string | null;
  running_build: string | null;
  recommended: string;
  hardware: { ram_bytes: number; vram_mb: number; nvidia: boolean; physical_cores: number };
  folder: string;
}

export interface Profile {
  provider: ProviderKind;
  id: string;
  name: string;
  base_url: string;
  model: string;
  api_key_ref: string | null;
  language: string;
  audio_format: AudioFormat;
  response_format: ResponseFormat;
  send_prompt: boolean;
  send_keywords: boolean;
  apply_rules: boolean;
  apply_cleanup: boolean;
  fallback_profile_id: string | null;
}

export type HotkeyMode = "toggle" | "push_to_talk";
export type OverlayPosition = "right_center" | "top_center" | "bottom_center" | "left_center";

/** How the text is inserted: clipboard + a paste shortcut, or typed as keystrokes ("type"). */
export type PasteMethod = "ctrl_v" | "shift_insert" | "ctrl_shift_v" | "type";
/** Key sent only after the text was actually inserted. */
export type PasteAfter = "none" | "enter" | "ctrl_enter";

export interface PasteConfig {
  method: PasteMethod;
  after: PasteAfter;
  restore_clipboard: boolean;
  trailing_space: boolean;
}

/** "pause": pause media sessions that are playing and resume exactly those; "duck": lower other apps' volume. */
export type MediaMode = "pause" | "duck";

/** What happens to other audio while a recording runs. */
export interface MediaConfig {
  enabled: boolean;
  mode: MediaMode;
  /** 0–100: the volume other apps are lowered to, as a percent of their own level ("duck" only). */
  duck_volume_percent: number;
}

/** One per-app rule (crates/core/src/app_rules.rs `AppRule`); null fields keep the global setting. */
export interface AppRule {
  id: string;
  enabled: boolean;
  /** Process file name such as `code.exe`, stored case-folded; no path or wildcard. */
  exe: string;
  /** Case-insensitive part of the window title; "" matches any window of the app. */
  title_contains: string;
  style_id: string | null;
  profile_id: string | null;
  paste_method: PasteMethod | null;
  /** "none" turns the global after-key off; null keeps it. */
  paste_after: PasteAfter | null;
}

/** The first enabled matching rule (in list order) applies when a dictation starts. */
export interface AppRulesConfig {
  enabled: boolean;
  rules: AppRule[];
}

/** `list_running_apps`: visible windows of other apps, one per file name, sorted. Titles are private. */
export interface RunningApp {
  exe: string;
  title: string;
}

/** Dictate-and-translate; uses the AI cleanup connection. */
export interface TranslateConfig {
  /** One of the `translate_languages()` codes; Rust stores anything else as "en". */
  target_language: string;
  /** null uses `cleanup.model`; Rust stores a blank string as null. */
  model: string | null;
}

/** `translate_languages()`: the supported target languages in their fixed order. */
export interface TranslateLanguage {
  code: string;
  name_en: string;
  name_tr: string;
}

export interface AppConfig {
  local: LocalConfig;
  ask: AskConfig;
  cleanup: CleanupConfig;
  schema_version: number;
  ui_language: string | null;
  active_profile_id: string;
  profiles: Profile[];
  hotkey: {
    keys: string[];
    pause_keys: string[];
    ask_keys: string[];
    /** Cycles through the cleanup styles with `in_cycle`; empty disables it. */
    style_cycle_keys: string[];
    /** Voice edit of the selected text (or new text at the cursor); empty disables it. */
    edit_keys: string[];
    /** Dictate, then paste the text translated into `translate.target_language`; empty disables it. */
    translate_keys: string[];
    mode: HotkeyMode;
    enabled: boolean;
  };
  recording: {
    microphone: string | null;
    max_seconds: number;
    speech_floor_db: number;
    noise_margin_db: number;
    min_speech_seconds: number;
    /** Preview partial text in the native overlay when the effective profile can stream; nothing is typed early. */
    live_text: boolean;
  };
  paste: PasteConfig;
  translate: TranslateConfig;
  media: MediaConfig;
  app_rules: AppRulesConfig;
  /** Only whether suggestions are offered after a History edit; nothing is added without confirmation. */
  learning: LearningConfig;
  history: { enabled: boolean; save_audio: boolean; audio_retention_days: number; max_entries: number };
  rules: { enabled_packs: string[]; prompt_context: string; numbers_as_words: boolean };
  ui: {
    theme: Theme;
    start_in_tray: boolean;
    autostart: boolean;
    overlay_position: OverlayPosition;
    sound_feedback: boolean;
    setup_done: boolean;
    check_updates: boolean;
  };
}

export interface AppInfo {
  cli_path: string;
  version: string;
  data_dir: string;
  log_dir: string;
  system_locale: string | null;
  debug_build: boolean;
}

export type TranscriptStatus = "ok" | "empty" | "hallucination" | "failed";

/** "edit" = voice edit of the selected text, "translate" = dictate and translate. */
export type DictationMode = "dictation" | "ask" | "edit" | "translate";

export interface Dictation {
  mode: DictationMode;
  agent: string | null;
  id: number;
  created_at_ms: number;
  profile_id: string;
  cleanup_status: "ok" | "failed" | "skipped" | null;
  cleanup_model: string | null;
  /** The cleanup style chosen for the dictation; null for Ask and older rows. May name a deleted style. */
  cleanup_style_id: string | null;
  /** The original recognition (Ask: the request); unchanged by a History edit. */
  raw_text: string;
  /** Editable from History (`history_update_text`). */
  text: string;
  status: TranscriptStatus;
  audio_ms: number;
  latency_ms: number;
  audio_path: string | null;
  error_code: ErrorKind | null;
  language: string | null;
  /** Edit: the original selection (null when new text was written). Translate: the source transcript. Else null. */
  source_text: string | null;
  /** Edit: the spoken instruction. Else null. */
  instruction: string | null;
}

export interface LearningConfig {
  enabled: boolean;
}

/** One suggested dictionary addition from a History edit (each side: 1–4 words). */
export interface TermChange {
  heard: string;
  corrected: string;
}

/** "rule": a correction rule heard → corrected; "term": the corrected spelling as a personal term. */
export type LearningTarget = "rule" | "term";

/** A `learning_apply` item (Rust flattens `TermChange` into it). */
export interface LearningItem extends TermChange {
  target: LearningTarget;
}

/** `history_update_text`: the saved row and at most eight suggestions (none when learning is off). */
export interface HistoryUpdate {
  dictation: Dictation;
  suggestions: TermChange[];
}

export type RuleKind = "correction" | "replacement" | "casing" | "numbers";

export interface UsageStats {
  dictations: number;
  successful: number;
  audio_ms: number;
  characters: number;
  average_latency_ms: number;
}
export interface RuleHit {
  rule: { pack_id: string; kind: RuleKind; index: number };
  from: string;
  to: string;
}
export interface RuleWarning {
  pack_id: string;
  message: string;
}
export interface RulesPreview {
  text: string;
  hits: RuleHit[];
  warnings: RuleWarning[];
  hallucination: boolean;
}
export interface BuiltPrompt {
  prompt: string | null;
  included_terms: string[];
  dropped_terms: string[];
  estimated_tokens: number;
}

export interface Replacement {
  from: string;
  to: string;
  case_sensitive?: boolean;
  regex?: boolean;
}
/** Empty lists/maps are omitted by Rust, so they are optional here. */
export interface RulePack {
  schema: number;
  id: string;
  name: string;
  language: string;
  terms?: string[];
  corrections?: Record<string, string[]>;
  replacements?: Replacement[];
  hallucinations?: string[];
}
export interface PackInfo {
  id: string;
  name: string;
  language: string;
  terms: number;
  corrections: number;
  replacements: number;
  hallucinations: number;
}

export type CorrectionOutcome = "added_variant" | "added_term" | "already_present";
export interface CorrectionDraft {
  yaml: string;
  outcome: CorrectionOutcome;
}

export interface HotkeyState {
  paused: boolean;
  error: string | null;
}

export type StartupNotice =
  | { kind: "config_reset"; backup: string | null; reason: string }
  | { kind: "user_rules_broken"; reason: string };

export type CommandErrorCode =
  | "config" | "rules" | "history" | "secrets" | "provider" | "invalid_input" | "unavailable" | "update" | "learning"
  | "file_busy" | "file_pick" | "file_result" | "file_timestamps" | "file_language" | "file_speakers"
  | "file_decode" | "file_limit" | "file_cancelled" | "file_worker" | "file_save" | "file_blocked_by_update"
  | "update_blocked_by_dictation" | "update_blocked_by_file_job";
export interface CommandError {
  code: CommandErrorCode;
  message: string;
  kind: ErrorKind | null;
}

export interface UpdateInfo {
  version: string;
  current_version: string;
  notes: string | null;
}

/** Mirrors `updates::UpdateState` (crates/app/src/updates.rs). */
export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "up_to_date" }
  | { kind: "available"; info: UpdateInfo }
  | { kind: "installing"; info: UpdateInfo; downloaded: number; total: number | null }
  | { kind: "check_failed"; message: string };

export type MicTestEvent = { kind: "level"; value: number } | { kind: "failed"; message: string };

/** Must match `PROMPT_TOKEN_BUDGET` in crates/core/src/rules/prompt.rs. */
export const PROMPT_TOKEN_BUDGET = 224;

export type BuiltinStyleId = "raw" | "clean" | "formal" | "casual" | "prompt";

/** A cleanup style (crates/core/src/cleanup/styles.rs `CleanupStyle`). */
export interface CleanupStyle {
  /** A built-in id, or `custom-` + ASCII `[a-z0-9-]` (at most 48 characters in total). */
  id: string;
  /** Unchanged built-ins store the English default name; that name is shown localized. */
  name: string;
  builtin: boolean;
  /** "" selects the built-in default for the dictation's language. */
  prompt: string;
  /** null inherits `cleanup.model`. */
  model: string | null;
  in_cycle: boolean;
  hotkey: string[];
}

/** `cleanup_style_defaults`: the five built-ins in their fixed order. */
export interface StyleDefaults {
  id: BuiltinStyleId;
  names: Record<Lang, string>;
  prompts: Record<Lang, string>;
}

/** `validate_hotkey` targets. */
export type HotkeyTarget = "dictation" | "pause" | "ask" | "style_cycle" | "edit" | "translate" | `style:${string}`;

export interface CleanupConfig {
  enabled: boolean;
  base_url: string;
  model: string;
  api_key_ref: string | null;
  /** Legacy; Rust migrates it into the clean style. The UI edits `styles[].prompt` instead. */
  prompt_override: string;
  timeout_seconds: number;
  reasoning_effort: string | null;
  active_style_id: string;
  styles: CleanupStyle[];
}
export interface CleanupPreset { id: string; name: string; key_url: string; config: CleanupConfig }
export interface CleanupTestResult { before: string; after: string }

export interface FileOptions { language: string | null; cleanup: boolean; speakers: boolean }
export type FilePhase = "idle" | "decoding" | "transcribing" | "cancelling" | "completed" | "cancelled" | "failed";
export interface FileProgress { chunk: number; chunks: number; percent: number }
export interface FileSegment { start_ms: number; end_ms: number; text: string; speaker: string | null }
export interface FileResult { text: string; segments: FileSegment[]; timestamps_available: boolean; duration_ms: number; cleanup_failed: boolean }
export interface FileState {
  revision: number; name: string | null; phase: FilePhase; progress: FileProgress | null;
  result: FileResult | null; error: CommandError | null; options: FileOptions;
  /** A completed result that exists only in memory: not saved, copied or discarded yet. */
  unsaved: boolean;
}
export type FileEvent = Pick<FileState, "revision" | "phase" | "progress" | "unsaved">;
/** What the shell tracks about the file job: enough to know whether an update may install. */
export type FileWork = Pick<FileState, "revision" | "phase" | "unsaved">;
export type FileOutputFormat = "txt" | "srt";

export type AskAgent = "chat" | "claude" | "codex";
export interface AskConfig {
  agent: AskAgent;
  base_url: string;
  chat_model: string;
  api_key_ref: string | null;
  claude_model: string;
  codex_model: string;
  permission: "restricted" | "workspace";
  working_dir: string;
  timeout_seconds: number;
  output: "paste" | "copy";
  continue_minutes: number;
  cleanup_request: boolean;
}
export interface AgentStatus { agent: AskAgent; installed: boolean; }
