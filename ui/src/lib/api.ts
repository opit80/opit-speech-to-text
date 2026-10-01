// Typed wrappers for every Rust command. Argument names are camelCase (Tauri maps them).
import { invoke } from "@tauri-apps/api/core";
import type {
  AppConfig, AppInfo, BuiltPrompt, CorrectionDraft, Dictation, DictationStatus, HotkeyState, PackInfo,
  Profile, RulePack, RulesPreview, RuleWarning, StartupNotice,
} from "./types";

export const api = {
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
  micTestStart: (device: string | null) => invoke<string>("mic_test_start", { device }),
  micTestStop: () => invoke<void>("mic_test_stop"),

  hasApiKey: (keyRef: string) => invoke<boolean>("has_api_key", { keyRef }),
  setApiKey: (keyRef: string, key: string) => invoke<void>("set_api_key", { keyRef, key }),
  deleteApiKey: (keyRef: string) => invoke<void>("delete_api_key", { keyRef }),
  testConnection: (profile: Profile, apiKey: string | null) =>
    invoke<void>("test_connection", { profile, apiKey }),
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
  historySearch: (query: string, limit: number) => invoke<Dictation[]>("history_search", { query, limit }),
  historyDelete: (id: number) => invoke<void>("history_delete", { id }),
  historyClear: () => invoke<void>("history_clear"),
  historyAudio: (id: number) => invoke<ArrayBuffer>("history_audio", { id }),

  getHotkeyState: () => invoke<HotkeyState>("get_hotkey_state"),
  setHotkeyPaused: (paused: boolean) => invoke<HotkeyState>("set_hotkey_paused", { paused }),
  setHotkeyCapture: (active: boolean) => invoke<void>("set_hotkey_capture", { active }),
  validateHotkey: (keys: string[]) => invoke<void>("validate_hotkey", { keys }),
};
