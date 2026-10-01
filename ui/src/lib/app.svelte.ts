// The one place the UI keeps live app state. Filled by commands at start-up, then by events.
import { api } from "./api";
import { describeError } from "./errors";
import { onConfigChanged, onHotkeyState, onNavigate, onStatus, onUpdateState, type UnlistenFn } from "./events";
import { resolveLang, translate, type MessageKey, type Params } from "./i18n";
import { parseRoute } from "./route";
import { navigate } from "./router.svelte";
import { createSerialQueue } from "./serial";
import type { AppConfig, AppInfo, DictationStatus, HotkeyState, Lang, StartupNotice, UpdateState } from "./types";

interface AppState {
  info: AppInfo | null;
  config: AppConfig | null;
  status: DictationStatus;
  hotkey: HotkeyState;
  lang: Lang;
  notices: StartupNotice[];
  ready: boolean;
  loadError: string | null;
  update: UpdateState;
  updateDismissed: string | null;
}

export const app = $state<AppState>({
  info: null,
  config: null,
  status: { state: "idle", can_retry: false },
  hotkey: { paused: false, error: null },
  lang: resolveLang(null, navigator.language),
  notices: [],
  ready: false,
  loadError: null,
  update: { kind: "idle" },
  updateDismissed: null,
});

/** Reactive in templates: it reads `app.lang`. */
export function t(key: MessageKey, params?: Params): string {
  return translate(app.lang, key, params);
}

export function errorText(error: unknown): string {
  return describeError(error, app.lang);
}

function setConfig(config: AppConfig): void {
  app.config = config;
  app.lang = resolveLang(config.ui_language, app.info?.system_locale);
  document.documentElement.lang = app.lang;
}

/** Subscribes first (so nothing is missed), then loads. Returns the unsubscribe function. */
export async function initApp(): Promise<() => void> {
  let statusSeen = false;
  let configSeen = false;
  let hotkeySeen = false;
  let updateSeen = false;
  const stops: UnlistenFn[] = [];
  const stop = () => stops.splice(0).forEach((unlisten) => unlisten());
  try {
    const subscriptions = await Promise.allSettled([
      onStatus((s) => {
        statusSeen = true;
        app.status = s;
      }),
      onConfigChanged((config) => {
        configSeen = true;
        setConfig(config);
      }),
      onHotkeyState((h) => {
        hotkeySeen = true;
        app.hotkey = h;
      }),
      onNavigate((route) => navigate(parseRoute(`#/${route}`))),
      onUpdateState((u) => {
        updateSeen = true;
        app.update = u;
      }),
    ]);
    for (const result of subscriptions) {
      if (result.status === "fulfilled") stops.push(result.value);
    }
    for (const result of subscriptions) {
      if (result.status === "rejected") throw result.reason;
    }
    const [info, config, status, hotkey, notices, update] = await Promise.all([
      api.appInfo(),
      api.getConfig(),
      api.getStatus(),
      api.getHotkeyState(),
      api.takeStartupNotices(),
      api.getUpdateState(),
    ]);
    app.info = info;
    setConfig(configSeen && app.config ? app.config : config);
    if (!statusSeen) app.status = status;
    if (!hotkeySeen) app.hotkey = hotkey;
    if (!updateSeen) app.update = update;
    app.notices = notices;
    app.ready = true;
  } catch (error) {
    app.loadError = describeError(error, app.lang);
    stop();
  }
  return stop;
}

const enqueueSave = createSerialQueue();

/** Queues a save using the latest config when it runs, then adopts what Rust stored. */
export function saveConfig(mutate: (draft: AppConfig) => void): Promise<AppConfig> {
  return enqueueSave(async () => {
    if (!app.config) throw new Error("config not loaded");
    const draft = $state.snapshot(app.config) as AppConfig;
    mutate(draft);
    const saved = await api.saveConfig(draft);
    setConfig(saved);
    return saved;
  });
}
