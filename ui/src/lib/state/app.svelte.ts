// The one place the UI keeps live app state. Filled by commands at start-up, then by events.
import { api } from "../bridge/api";
import { describeError } from "../bridge/errors";
import { onConfigChanged, onFileProgress, onHotkeyState, onNavigate, onStatus, onUpdateState, type UnlistenFn } from "../bridge/events";
import { mergeFileWork } from "../../features/files/files";
import { resolveLang, translate, type MessageKey, type Params } from "../i18n";
import { parseRoute } from "../navigation/route";
import { navigate } from "../navigation/router.svelte";
import { createSerialQueue } from "../utils/serial";
import { checkUpdateState, keepInstallError } from "../../features/updates/update";
import type { AppConfig, AppInfo, DictationStatus, FileWork, HotkeyState, Lang, StartupNotice, StyleDefaults, UpdateState } from "../bridge/types";

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
  /** Why the last install failed; see `keepInstallError`. */
  updateError: string | null;
  /** The file job as far as update installation cares; see `fileBlocksUpdate`. */
  fileWork: FileWork;
  /** Built-in cleanup style names and prompts; null until `loadStyleDefaults` resolves. */
  styleDefaults: StyleDefaults[] | null;
}

export const app = $state<AppState>({
  info: null,
  config: null,
  status: { state: "idle", can_retry: false, waiting: 0, paused: false },
  hotkey: { paused: false, error: null },
  lang: resolveLang(null, navigator.language),
  notices: [],
  ready: false,
  loadError: null,
  update: { kind: "idle" },
  updateDismissed: null,
  updateError: null,
  fileWork: { revision: -1, phase: "idle", unsaved: false },
  styleDefaults: null,
});

let styleDefaultsRequest: Promise<void> | null = null;

/** Loads the built-in style defaults once (they never change at runtime); a failure allows a retry. */
export function loadStyleDefaults(): Promise<void> {
  styleDefaultsRequest ??= api.cleanupStyleDefaults().then(
    (defaults) => { app.styleDefaults = defaults; },
    (error: unknown) => { styleDefaultsRequest = null; throw error; },
  );
  return styleDefaultsRequest;
}

function setFileWork(next: FileWork): void {
  app.fileWork = mergeFileWork(app.fileWork, next);
}

let updateRevision = 0;

function setUpdate(state: UpdateState): void {
  updateRevision++;
  app.update = state;
  app.updateError = keepInstallError(app.updateError, state);
}

/** Preserve newer update events when the check command returns late. */
export function checkForUpdates(): Promise<void> {
  return checkUpdateState(api.checkForUpdates, () => updateRevision, setUpdate);
}

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
        setUpdate(u);
      }),
      onFileProgress(setFileWork),
    ]);
    for (const result of subscriptions) {
      if (result.status === "fulfilled") stops.push(result.value);
    }
    for (const result of subscriptions) {
      if (result.status === "rejected") throw result.reason;
    }
    const [info, config, status, hotkey, notices, update, file] = await Promise.all([
      api.appInfo(),
      api.getConfig(),
      api.getStatus(),
      api.getHotkeyState(),
      api.takeStartupNotices(),
      api.getUpdateState(),
      api.fileState(),
    ]);
    setFileWork(file);
    app.info = info;
    setConfig(configSeen && app.config ? app.config : config);
    if (!statusSeen) app.status = status;
    if (!hotkeySeen) app.hotkey = hotkey;
    if (!updateSeen) setUpdate(update);
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

/** Selects a cleanup style, queued with config saves so neither overwrites the other. */
export function setActiveStyle(id: string): Promise<AppConfig> {
  return enqueueSave(async () => {
    const saved = await api.setActiveStyle(id);
    setConfig(saved);
    return saved;
  });
}
