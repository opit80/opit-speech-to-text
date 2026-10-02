// Dev-only browser entry: never imported by main.ts or included in the app build.
import { mount } from "svelte";
import BrowserPreview from "./BrowserPreview.svelte";
import "./app.css";
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { AppConfig, Dictation, DictationStatus } from "./lib/types";

if (!import.meta.env.DEV) throw new Error("Guide preview is only available in the development server");

const params = new URLSearchParams(location.search);
const config: AppConfig = {
  schema_version: 1,
  ui_language: params.get("lang") === "en" ? "en" : "tr",
  active_profile_id: "preview",
  profiles: [{
    id: "preview", name: "Groq", base_url: "https://api.groq.com/openai/v1", model: "whisper-large-v3",
    api_key_ref: null, language: "tr", audio_format: "flac", response_format: "verbose_json",
    send_prompt: true, send_keywords: false, apply_rules: true, fallback_profile_id: null,
  }],
  hotkey: { keys: ["RightCtrl", "RightShift"], mode: params.get("mode") === "ptt" ? "push_to_talk" : "toggle", enabled: true },
  recording: { microphone: null, max_seconds: 180 },
  paste: { restore_clipboard: false, trailing_space: true },
  history: { enabled: true, save_audio: false, audio_retention_days: 7 },
  rules: { enabled_packs: ["tr-core", "tr-tech"], prompt_context: "", numbers_as_words: false },
  ui: { start_in_tray: false, autostart: false, overlay_position: "right_center", sound_feedback: true, setup_done: true, check_updates: false },
};

const now = Date.now();
let rows: Dictation[] = Array.from({ length: params.get("empty") === "true" ? 0 : 9 }, (_, i) => ({
  id: 9 - i, created_at_ms: now - i * 3_600_000, profile_id: "preview",
  raw_text: "Toplantıdan önce github değişikliklerini kontrol edelim.",
  text: "Toplantıdan önce GitHub değişikliklerini kontrol edelim.",
  status: "ok", audio_ms: 12_000 + i * 1_000, latency_ms: 800 + i * 50, audio_path: null,
}));
let paused = params.get("paused") === "true";
let status: DictationStatus = { state: params.get("recording") === "true" ? "recording" : "idle", can_retry: false };
let completion: ReturnType<typeof setTimeout> | undefined;
async function changeStatus(state: DictationStatus["state"]) {
  status = { state, can_retry: false };
  await emit("dictation-status", status);
}
mockIPC(async (command, payload) => {
  switch (command) {
    case "app_info": return { version: "0.1.1", data_dir: "", log_dir: "", system_locale: "tr-TR", debug_build: true };
    case "get_config": return structuredClone(config);
    case "get_status": return structuredClone(status);
    case "toggle_dictation": {
      if (status.state === "recording") {
        await changeStatus("transcribing");
        completion = setTimeout(() => { void changeStatus("idle"); }, 900);
      } else if (status.state !== "transcribing" && status.state !== "pasting") {
        await changeStatus("recording");
      }
      return;
    }
    case "cancel_dictation": clearTimeout(completion); await changeStatus("cancelled"); return;
    case "get_hotkey_state": return { paused, error: null };
    case "set_hotkey_paused": paused = !!(payload as { paused: boolean }).paused; return { paused, error: null };
    case "save_config": {
      Object.assign(config, structuredClone((payload as { config: AppConfig }).config));
      return structuredClone(config);
    }
    case "set_hotkey_capture":
    case "validate_hotkey": return;
    case "history_recent": return rows.slice(0, (payload as {limit: number}).limit);
    case "history_search": return rows;
    case "history_delete": rows = rows.filter(row => row.id !== (payload as {id: number}).id); return;
    case "history_clear": rows = []; return;
    case "usage_stats": {
      const { sinceMs, untilMs } = payload as { sinceMs: number | null; untilMs: number };
      const selected = rows.filter(row => row.created_at_ms >= (sinceMs ?? -Infinity) && row.created_at_ms < untilMs);
      return { dictations: selected.length, successful: selected.length, audio_ms: selected.reduce((n,row) => n + row.audio_ms, 0), characters: selected.reduce((n,row) => n + Array.from(row.text).length, 0), average_latency_ms: selected.length ? selected.reduce((n,row) => n + row.latency_ms, 0) / selected.length : 0 };
    }
    case "parse_user_rules": return { schema: 1, id: "user", name: "User rules", language: "tr", terms: ["GitHub"], corrections: {}, replacements: [] };
    case "rules_preview": {
      const source = (payload as {text: string}).text;
      // A fixed example for visual review; Rust tests verify the real conversion.
      const text = config.rules.numbers_as_words ? source.replace(/\b12\b/g, "on iki") : source;
      return { text, hits: [], warnings: [], hallucination: false };
    }
    case "rule_packs": return [
      { id: "tr-core", name: "Türkçe imla", language: "tr", terms: 24, corrections: 8, replacements: 4, hallucinations: 6 },
      { id: "tr-tech", name: "Yazılım terimleri", language: "tr", terms: 48, corrections: 16, replacements: 3, hallucinations: 0 },
      { id: "fivem", name: "FiveM", language: "tr", terms: 32, corrections: 9, replacements: 2, hallucinations: 0 },
    ];
    case "get_update_state": return { kind: "idle" };
    case "take_startup_notices":
    case "list_microphones":
    case "profile_presets": return [];
    case "prompt_budget": return { prompt: null, included_terms: [], dropped_terms: [], estimated_tokens: 0 };
    case "get_user_rules": return "schema: 1\nid: user\nname: User rules\nlanguage: tr\n";
    default: throw { code: "unavailable", message: "Read-only browser preview", kind: null };
  }
}, { shouldMockEvents: true });

if (!location.hash) location.hash = "#/guide";
const target = document.getElementById("app");
if (target) mount(BrowserPreview, { target });
