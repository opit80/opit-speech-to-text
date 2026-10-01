// TypeScript mirrors of the Rust payloads (serde output). Keep in sync with
// crates/core (config, provider, history, rules) and crates/app (commands, app_core, controller).

export type Lang = "en" | "tr";

export type ErrorKind =
  | "missing_key" | "invalid_key" | "too_large" | "rate_limited" | "server" | "network" | "timeout"
  | "bad_response" | "rejected" | "audio" | "microphone" | "credentials" | "paste";

export type DictationState = "idle" | "recording" | "transcribing" | "pasting" | "cancelled" | "error";

export interface DictationStatus {
  state: DictationState;
  can_retry: boolean;
  /** Only when state === "error". */
  kind?: ErrorKind;
  /** Only when state === "error"; already localized by Rust. */
  message?: string;
}

export type AudioFormat = "flac" | "wav";
export type ResponseFormat = "verbose_json" | "json";

export interface Profile {
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
  fallback_profile_id: string | null;
}

export type HotkeyMode = "toggle" | "push_to_talk";
export type OverlayPosition = "right_center" | "top_center" | "bottom_center" | "left_center";

export interface AppConfig {
  schema_version: number;
  ui_language: string | null;
  active_profile_id: string;
  profiles: Profile[];
  hotkey: { keys: string[]; mode: HotkeyMode; enabled: boolean };
  recording: { microphone: string | null; max_seconds: number };
  paste: { restore_clipboard: boolean; trailing_space: boolean };
  history: { enabled: boolean; save_audio: boolean; audio_retention_days: number };
  rules: { enabled_packs: string[]; prompt_context: string };
  ui: {
    start_in_tray: boolean;
    autostart: boolean;
    overlay_position: OverlayPosition;
    sound_feedback: boolean;
    setup_done: boolean;
  };
}

export interface AppInfo {
  version: string;
  data_dir: string;
  log_dir: string;
  system_locale: string | null;
  debug_build: boolean;
}

export type TranscriptStatus = "ok" | "empty" | "hallucination";

export interface Dictation {
  id: number;
  created_at_ms: number;
  profile_id: string;
  raw_text: string;
  text: string;
  status: TranscriptStatus;
  audio_ms: number;
  latency_ms: number;
  audio_path: string | null;
}

export type RuleKind = "correction" | "replacement" | "casing";
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
  | "config" | "rules" | "history" | "secrets" | "provider" | "invalid_input" | "unavailable";
export interface CommandError {
  code: CommandErrorCode;
  message: string;
  kind: ErrorKind | null;
}

export type MicTestEvent = { kind: "level"; value: number } | { kind: "failed"; message: string };

/** Must match `PROMPT_TOKEN_BUDGET` in crates/core/src/rules/prompt.rs. */
export const PROMPT_TOKEN_BUDGET = 224;
