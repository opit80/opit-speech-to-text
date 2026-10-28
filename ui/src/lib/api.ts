// Typed wrappers for every Rust command. Argument names are camelCase (Tauri maps them).
import { invoke } from "@tauri-apps/api/core";
import type {
  AskConfig, AgentStatus, CleanupConfig, CleanupPreset, CleanupTestResult, AppConfig, AppInfo, BuiltPrompt, CorrectionDraft, Dictation, DictationStatus, HotkeyState, PackInfo,
  Profile, RulePack, RulesPreview, RuleWarning, StartupNotice, UpdateState, UsageStats,
  FileOptions, FileState, FileOutputFormat, HotkeyTarget, LocalState, RunningApp, StyleDefaults, TranslateLanguage,
  HistoryUpdate, LearningItem, TermChange,
} from "./types";

export const api = {
  localState: () => invoke<LocalState>("local_state"),
  localDownload: (id: string) => invoke<void>("local_download", { id }),
  localCancel: () => invoke<void>("local_cancel"),
  localDelete: (id: string) => invoke<void>("local_delete", { id }),
  askStatus: () => invoke<AgentStatus[]>("ask_status"),
  testAsk: (config: AskConfig) => invoke<string>("test_ask", { config }),
  filePick: () => invoke<FileState>("file_pick"),
  fileState: () => invoke<FileState>("file_state"),
  fileStart: (options: FileOptions) => invoke<FileState>("file_start", { options }),
  fileCancel: () => invoke<void>("file_cancel"),
  fileRender: (format: FileOutputFormat, prefixes: boolean, speakers: boolean) =>
    invoke<string>("file_render", { format, prefixes, speakers }),
  fileSave: (format: FileOutputFormat, prefixes: boolean, speakers: boolean) =>
    invoke<boolean>("file_save", { format, prefixes, speakers }),
  fileHandled: (revision: number) => invoke<FileState>("file_handled", { revision }),
  fileDiscard: (revision: number) => invoke<FileState>("file_discard", { revision }),
  appInfo: () => invoke<AppInfo>("app_info"),
  getStatus: () => invoke<DictationStatus>("get_status"),
  toggleDictation: () => invoke<void>("toggle_dictation"),
  cancelDictation: () => invoke<void>("cancel_dictation"),
  retryDictation: () => invoke<void>("retry_dictation"),

  getConfig: () => invoke<AppConfig>("get_config"),
  saveConfig: (config: AppConfig) => invoke<AppConfig>("save_config", { config }),
  setActiveProfile: (id: string) => invoke<AppConfig>("set_active_profile", { id }),
  takeStartupNotices: () => invoke<StartupNotice[]>("take_startup_notices"),

  listMicrophones: () => invoke<string[]>("list_microphones"),
  /** For the per-app rule picker only; titles are private and never persisted automatically. */
  listRunningApps: () => invoke<RunningApp[]>("list_running_apps"),
  micTestStart: (device: string | null) => invoke<string>("mic_test_start", { device }),
  micTestStop: () => invoke<void>("mic_test_stop"),

  hasApiKey: (keyRef: string) => invoke<boolean>("has_api_key", { keyRef }),
  setApiKey: (keyRef: string, key: string) => invoke<void>("set_api_key", { keyRef, key }),
  deleteApiKey: (keyRef: string) => invoke<void>("delete_api_key", { keyRef }),
  testConnection: (profile: Profile, apiKey: string | null) =>
    invoke<void>("test_connection", { profile, apiKey }),
  cleanupPresets: () => invoke<CleanupPreset[]>("cleanup_presets"),
  cleanupPrompts: () => invoke<Record<"en" | "tr", string>>("cleanup_prompts"),
  cleanupStyleDefaults: () => invoke<StyleDefaults[]>("cleanup_style_defaults"),
  /** Use `setActiveStyle` in app.svelte.ts, which serializes it with config saves. */
  setActiveStyle: (id: string) => invoke<AppConfig>("set_active_style", { id }),
  testCleanup: (config: CleanupConfig) => invoke<CleanupTestResult>("test_cleanup", { config }),
  /** Supported translation target languages with English and Turkish names; no I/O. */
  translateLanguages: () => invoke<TranslateLanguage[]>("translate_languages"),
  profilePresets: () => invoke<Profile[]>("profile_presets"),

  getUserRules: () => invoke<string>("get_user_rules"),
  saveUserRules: (yaml: string) => invoke<RuleWarning[]>("save_user_rules", { yaml }),
  rulesPreview: (text: string, draftYaml: string | null) =>
    invoke<RulesPreview>("rules_preview", { text, draftYaml }),
  promptBudget: () => invoke<BuiltPrompt>("prompt_budget"),
  rulePacks: () => invoke<PackInfo[]>("rule_packs"),
  parseUserRules: (yaml: string) => invoke<RulePack>("parse_user_rules", { yaml }),
  renderUserRules: (pack: RulePack, previousYaml: string) =>
    invoke<string>("render_user_rules", { pack, previousYaml }),
  correctionDraft: (canonical: string, variant: string) =>
    invoke<CorrectionDraft>("correction_draft", { canonical, variant }),

  historyRecent: (limit: number, beforeId: number | null) =>
    invoke<Dictation[]>("history_recent", { limit, beforeId }),
  usageStats: (sinceMs: number | null, untilMs: number) => invoke<UsageStats>("usage_stats", { sinceMs, untilMs }),
  historySearch: (query: string, limit: number) => invoke<Dictation[]>("history_search", { query, limit }),
  historyDelete: (id: number) => invoke<void>("history_delete", { id }),
  historyClear: () => invoke<void>("history_clear"),
  historyAudio: (id: number) => invoke<ArrayBuffer>("history_audio", { id }),
  historyRetry: (id: number) => invoke<Dictation>("history_retry", { id }),
  /** Saves an edited transcript (`raw_text` keeps the original) and returns dictionary suggestions. */
  historyUpdateText: (id: number, text: string) => invoke<HistoryUpdate>("history_update_text", { id, text }),
  /** Adds confirmed suggestions (1–8) to the personal rules; `rules-changed` follows even on failure. */
  learningApply: (items: LearningItem[]) => invoke<RuleWarning[]>("learning_apply", { items }),
  /** Remembers rejected suggestions (1–8) so they are never offered again. */
  learningDismiss: (items: TermChange[]) => invoke<void>("learning_dismiss", { items }),

  getHotkeyState: () => invoke<HotkeyState>("get_hotkey_state"),
  setHotkeyPaused: (paused: boolean) => invoke<HotkeyState>("set_hotkey_paused", { paused }),
  setHotkeyCapture: (active: boolean) => invoke<void>("set_hotkey_capture", { active }),
  /** Without a target: syntax only. With one: also conflicts, checked against `config` or the saved settings. */
  validateHotkey: (keys: string[], target?: HotkeyTarget, config?: AppConfig) =>
    invoke<void>("validate_hotkey", { keys, target, config }),

  getUpdateState: () => invoke<UpdateState>("get_update_state"),
  checkForUpdates: () => invoke<UpdateState>("check_for_updates"),
  /** On success the app exits and the installer runs, so this may never settle. */
  installUpdate: () => invoke<void>("install_update"),
};
