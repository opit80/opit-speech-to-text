<script lang="ts">
  import AskSettings from "../lib/components/AskSettings.svelte";
  import CleanupSettings from "../lib/components/CleanupSettings.svelte";
  import CleanupStyles from "../lib/components/CleanupStyles.svelte";
  import { untrack } from "svelte";
  import { api } from "../lib/api";
  import { app, checkForUpdates, errorText, saveConfig, t } from "../lib/app.svelte";
  import type { MessageKey } from "../lib/i18n";
  import { toast } from "../lib/toast.svelte";
  import type { AppConfig, HotkeyMode, MediaMode, OverlayPosition, PasteAfter, PasteMethod, Theme } from "../lib/types";
  import { duckPercentRange, parseNumberInput } from "../lib/number-input";
  import Banner from "../lib/components/Banner.svelte";
  import Button from "../lib/components/Button.svelte";
  import Field from "../lib/components/Field.svelte";
  import HotkeyInput from "../lib/components/HotkeyInput.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import InfoTip from "../lib/components/InfoTip.svelte";
  import MicLevelTest from "../lib/components/MicLevelTest.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import Select from "../lib/components/Select.svelte";
  import Switch from "../lib/components/Switch.svelte";
  import UpdateInstallButton from "../lib/components/UpdateInstallButton.svelte";
  import UpdateProgress from "../lib/components/UpdateProgress.svelte";
  import { statusMessage } from "../lib/update";
  import { speechDetectionDefaults } from "../lib/speech-detection";

  let errors = $state<Record<string, string | null>>({});
  // Bumped after a failed change: the controls re-read the live value and drop what the user picked.
  let revision = $state(0);
  // Text of a focused number input; null when it shows the live value.
  let drafts = $state<Record<string, string | null>>({});
  let mics = $state<string[]>([]);
  let micsBusy = $state(false);
  let micsError = $state<string | null>(null);
  let pauseBusy = $state(false);
  let micRequest = 0;
  let disposed = false;
  let resetBusy = $state(false);

  async function resetSpeechDetection() {
    resetBusy = true;
    await save("speech_reset", (c) => { Object.assign(c.recording, speechDetectionDefaults); });
    for (const field of Object.keys(speechDetectionDefaults)) {
      drafts[field] = null;
      errors[field] = null;
    }
    resetBusy = false;
  }

  const config = $derived(app.config);

  async function save(field: string, mutate: (c: AppConfig) => void) {
    errors[field] = null;
    try {
      await saveConfig(mutate);
      toast(t("common.saved"));
    } catch (e) {
      if (disposed) return;
      errors[field] = errorText(e);
      revision++;
    }
  }

  // ---- General
  // Language names are shown in their own language (endonyms), so they need no translation.
  function endonym(code: "en" | "tr"): string {
    const name = new Intl.DisplayNames([code], { type: "language" }).of(code) ?? code;
    return name.charAt(0).toLocaleUpperCase(code) + name.slice(1);
  }
  const languageOptions = $derived([
    { value: "system", label: t("settings.language_system") },
    { value: "en", label: endonym("en") },
    { value: "tr", label: endonym("tr") },
  ]);
  const themeOptions = $derived([
    { value: "system", label: t("settings.theme_system") },
    { value: "light", label: t("settings.theme_light") },
    { value: "dark", label: t("settings.theme_dark") },
  ]);

  function languageValue(c: AppConfig): string {
    if (c.ui_language === null) return "system";
    return c.ui_language.toLowerCase().startsWith("tr") ? "tr" : "en";
  }

  // What the controls show: always the live values. A new object is built after every config
  // change and after a failed change (revision), so a control drops what the user picked and
  // shows the live value again. (Passing `view.x` keeps the prop unmemoized; a memoized equal
  // value would not reach a control that holds the user's pick.)
  const view = $derived.by(() => {
    void revision;
    const c = app.config;
    if (!c) return null;
    return {
      language: languageValue(c),
      theme: c.ui.theme,
      autostart: c.ui.autostart,
      startInTray: c.ui.start_in_tray,
      sound: c.ui.sound_feedback,
      mode: c.hotkey.mode as string,
      hotkeyEnabled: c.hotkey.enabled,
      paused: app.hotkey.paused,
      microphone: c.recording.microphone ?? "",
      pasteMethod: c.paste.method as string,
      pasteAfter: c.paste.after as string,
      restoreClipboard: c.paste.restore_clipboard,
      trailingSpace: c.paste.trailing_space,
      overlay: c.ui.overlay_position as string,
      mediaEnabled: c.media.enabled,
      mediaMode: c.media.mode as string,
      historyEnabled: c.history.enabled,
      saveAudio: c.history.save_audio,
      checkUpdates: c.ui.check_updates,
    };
  });

  // ---- Shortcut
  async function setPaused(paused: boolean) {
    pauseBusy = true;
    errors.pause = null;
    try {
      app.hotkey = await api.setHotkeyPaused(paused);
    } catch (e) {
      if (!disposed) {
        errors.pause = errorText(e);
        revision++;
      }
    } finally {
      if (!disposed) pauseBusy = false;
    }
  }

  // ---- Microphone
  async function loadMics() {
    if (disposed) return;
    const request = ++micRequest;
    micsBusy = true;
    micsError = null;
    try {
      const list = await api.listMicrophones();
      if (!disposed && request === micRequest) mics = [...new Set(list)];
    } catch (e) {
      if (!disposed && request === micRequest) micsError = errorText(e);
    } finally {
      if (!disposed && request === micRequest) micsBusy = false;
    }
  }

  $effect(() => {
    untrack(() => void loadMics());
    return () => {
      disposed = true;
    };
  });

  const micOptions = $derived.by(() => {
    const options = [{ value: "", label: t("common.system_default") }, ...mics.map((m) => ({ value: m, label: m }))];
    const configured = config?.recording.microphone ?? null;
    if (configured !== null && !mics.includes(configured)) {
      options.push({ value: configured, label: t("settings.mic_missing", { name: configured }) });
    }
    return options;
  });

  // ---- Number inputs: keep the typed text while focused, save on change, then show what Rust stored.
  function numberText(field: string, value: number): string {
    void revision;
    return drafts[field] ?? String(Number(value.toFixed(2)));
  }

  async function commitNumber(
    field: string,
    input: HTMLInputElement,
    min: number,
    max: number,
    step: number,
    read: (c: AppConfig) => number,
    write: (c: AppConfig, n: number) => void,
  ) {
    const value = parseNumberInput(input.value, min, max, step);
    if (value !== null) await save(field, (c) => write(c, value));
    if (disposed || !app.config) return;
    const stored = String(Number(read(app.config).toFixed(2)));
    input.value = stored;
    if (drafts[field] != null) drafts[field] = stored;
  }

  // ---- Updates
  const debugBuild = $derived(app.info?.debug_build ?? false);
  const updateStatus = $derived(statusMessage(app.update));
  const installing = $derived(app.update.kind === "installing" ? app.update : null);

  async function checkNow() {
    errors.update_check = null;
    try {
      await checkForUpdates();
    } catch (e) {
      if (!disposed) errors.update_check = errorText(e);
    }
  }

  // ---- About
  async function copyPath(field: string, path: string) {
    errors[field] = null;
    try {
      await navigator.clipboard.writeText(path);
      toast(t("common.copied"));
    } catch (e) {
      if (!disposed) errors[field] = errorText(e);
    }
  }

  const overlayKeys: Record<OverlayPosition, MessageKey> = {
    right_center: "settings.overlay.right_center",
    top_center: "settings.overlay.top_center",
    bottom_center: "settings.overlay.bottom_center",
    left_center: "settings.overlay.left_center",
  };
  const overlayOptions = $derived(
    (Object.keys(overlayKeys) as OverlayPosition[]).map((value) => ({ value, label: t(overlayKeys[value]) })),
  );
  const pasteMethodKeys: Record<PasteMethod, MessageKey> = {
    ctrl_v: "settings.paste_method.ctrl_v",
    shift_insert: "settings.paste_method.shift_insert",
    ctrl_shift_v: "settings.paste_method.ctrl_shift_v",
    type: "settings.paste_method.type",
  };
  const pasteMethodOptions = $derived(
    (Object.keys(pasteMethodKeys) as PasteMethod[]).map((value) => ({ value, label: t(pasteMethodKeys[value]) })),
  );
  const pasteAfterKeys: Record<PasteAfter, MessageKey> = {
    none: "settings.paste_after.none",
    enter: "settings.paste_after.enter",
    ctrl_enter: "settings.paste_after.ctrl_enter",
  };
  const pasteAfterOptions = $derived(
    (Object.keys(pasteAfterKeys) as PasteAfter[]).map((value) => ({ value, label: t(pasteAfterKeys[value]) })),
  );
  const mediaModeKeys: Record<MediaMode, MessageKey> = {
    pause: "settings.media_mode.pause",
    duck: "settings.media_mode.duck",
  };
  const mediaModeOptions = $derived(
    (Object.keys(mediaModeKeys) as MediaMode[]).map((value) => ({ value, label: t(mediaModeKeys[value]) })),
  );
  const modeOptions = $derived([
    { value: "toggle", label: t("settings.mode_toggle") },
    { value: "push_to_talk", label: t("settings.mode_ptt") },
  ]);
</script>

{#snippet fieldError(field: string)}
  {#if errors[field]}<p class="error" role="alert">{errors[field]}</p>{/if}
{/snippet}

{#snippet numberInput(field: string, label: string, info: string | undefined, min: number, max: number, step: number, disabled: boolean, read: (c: AppConfig) => number, write: (c: AppConfig, n: number) => void)}
  {#if config}
    <Field {label} {info} error={errors[field]}>
      {#snippet children(id, describedBy)}
        <input {id} class="number" type="number" {min} {max} {step} inputmode="decimal" {disabled}
          value={numberText(field, read(config))}
          aria-describedby={describedBy}
          onfocus={(e) => (drafts[field] = e.currentTarget.value)}
          onblur={() => (drafts[field] = null)}
          onchange={(e) => void commitNumber(field, e.currentTarget, min, max, step, read, write)} />
      {/snippet}
    </Field>
  {/if}
{/snippet}

<div>
  <PageHeader title={t("settings.title")} />
  {#if config && view}
    <div class="sections">
      <details class="card settings-group" open={["language", "theme", "autostart", "start_in_tray", "sound"].some((field) => !!errors[field])}>
        <summary>{t("settings.general")}</summary>
        <div class="stack group-body">
        <Select label={t("settings.language")} options={languageOptions} value={view.language}
          onchange={(v) => void save("language", (c) => { c.ui_language = v === "system" ? null : v; })} />
        {@render fieldError("language")}
        <Select label={t("settings.theme")} options={themeOptions} value={view.theme}
          onchange={(v) => void save("theme", (c) => { c.ui.theme = v as Theme; })} />
        {@render fieldError("theme")}
        <Switch label={t("settings.autostart")} checked={view.autostart}
          disabled={app.info?.debug_build ?? false}
          info={app.info?.debug_build ? t("settings.autostart_debug") : undefined}
          onchange={(v) => void save("autostart", (c) => { c.ui.autostart = v; })} />
        {@render fieldError("autostart")}
        <Switch label={t("settings.start_in_tray")}
          checked={view.startInTray}
          onchange={(v) => void save("start_in_tray", (c) => { c.ui.start_in_tray = v; })} />
        {@render fieldError("start_in_tray")}
        <Switch label={t("settings.sound")} info={t("settings.sound_hint")} checked={view.sound}
          onchange={(v) => void save("sound", (c) => { c.ui.sound_feedback = v; })} />
        {@render fieldError("sound")}
      </div>
      </details>

      <section class="card stack" aria-labelledby="settings-shortcut">
        <h2 id="settings-shortcut">{t("settings.shortcut")}</h2>
        {#if app.hotkey.error}<Banner tone="error">{t("notice.hotkey_failed", { reason: app.hotkey.error })}</Banner>{/if}
        <HotkeyInput keys={config.hotkey.keys} target="dictation"
          onchange={(keys) => save("hotkey_keys", (c) => { c.hotkey.keys = keys; })} />
        {@render fieldError("hotkey_keys")}
        <HotkeyInput keys={config.hotkey.pause_keys} optional target="pause" label={t("settings.pause_shortcut")} info={t("settings.pause_shortcut_note")}
          onchange={(keys) => save("pause_keys", (c) => { c.hotkey.pause_keys = keys; })} />
        {@render fieldError("pause_keys")}
        <HotkeyInput keys={config.hotkey.style_cycle_keys} optional target="style_cycle" label={t("settings.style_cycle_shortcut")}
          info={t("settings.style_cycle_shortcut_note")}
          onchange={(keys) => save("style_cycle_keys", (c) => { c.hotkey.style_cycle_keys = keys; })} />
        {@render fieldError("style_cycle_keys")}
        <Select label={t("settings.mode")} options={modeOptions} value={view.mode}
          info={config.hotkey.keys.includes("Escape") ? t("guide.cancel_from_home") : config.hotkey.mode === "toggle" ? t("settings.mode_toggle_hint") : undefined}
          onchange={(v) => void save("mode", (c) => { c.hotkey.mode = v as HotkeyMode; })} />
        {@render fieldError("mode")}
        <Switch label={t("settings.shortcut_enabled")} checked={view.hotkeyEnabled}
          onchange={(v) => void save("hotkey_enabled", (c) => { c.hotkey.enabled = v; })} />
        {@render fieldError("hotkey_enabled")}
        <Switch label={t("settings.pause")} checked={view.paused} disabled={pauseBusy}
          onchange={(v) => void setPaused(v)} />
        {@render fieldError("pause")}
      </section>

      <section class="card stack" aria-labelledby="settings-microphone">
        <h2 id="settings-microphone">{t("settings.microphone")}</h2>
        <div class="mic-row">
          <div class="grow">
            <Select label={t("settings.mic")} options={micOptions} value={view.microphone}
              onchange={(v) => void save("mic", (c) => { c.recording.microphone = v === "" ? null : v; })} />
          </div>
          <Button variant="ghost" busy={micsBusy} aria-label={t("settings.refresh")} title={t("settings.refresh")}
            onclick={() => void loadMics()}>
            <Icon name="retry" />
          </Button>
        </div>
        {@render fieldError("mic")}
        {#if micsError}<Banner tone="error">{micsError}</Banner>{/if}
        <MicLevelTest device={config.recording.microphone} />
        {@render numberInput("max_seconds", t("settings.max_seconds"), t("settings.max_seconds_hint"), 10, 600, 10, false,
          (c) => c.recording.max_seconds, (c, n) => { c.recording.max_seconds = n; })}
      </section>

      <details class="card settings-group" open={["media_enabled", "media_mode", "duck_volume"].some((field) => !!errors[field])}>
        <summary>{t("settings.media")}</summary>
        <div class="stack group-body">
          <Switch label={t("settings.media_enabled")} info={t("settings.media_enabled_hint")} checked={view.mediaEnabled}
            onchange={(v) => void save("media_enabled", (c) => { c.media.enabled = v; })} />
          {@render fieldError("media_enabled")}
          <Select label={t("settings.media_mode")} options={mediaModeOptions} value={view.mediaMode}
            disabled={!config.media.enabled}
            info={config.media.mode === "duck" ? t("settings.media_mode_duck_hint") : t("settings.media_mode_pause_hint")}
            onchange={(v) => void save("media_mode", (c) => { c.media.mode = v as MediaMode; })} />
          {@render fieldError("media_mode")}
          {#if config.media.mode === "duck"}
            {@render numberInput("duck_volume", t("settings.media_duck_volume"), t("settings.media_duck_volume_hint"),
              duckPercentRange.min, duckPercentRange.max, duckPercentRange.step, !config.media.enabled,
              (c) => c.media.duck_volume_percent, (c, n) => { c.media.duck_volume_percent = n; })}
          {/if}
        </div>
      </details>

      <details class="card settings-group" open={["speech_floor_db", "noise_margin_db", "min_speech_seconds", "speech_reset"].some((field) => !!errors[field])}>
        <summary>{t("settings.speech_detection")}</summary>
        <div class="stack group-body">
          {@render numberInput("speech_floor_db", t("settings.speech_floor"), t("settings.speech_floor_hint"), -90, -6, 0.01, resetBusy,
            (c) => c.recording.speech_floor_db, (c, n) => { c.recording.speech_floor_db = n; })}
          {@render numberInput("noise_margin_db", t("settings.noise_margin"), t("settings.noise_margin_hint"), 0, 30, 0.01, resetBusy,
            (c) => c.recording.noise_margin_db, (c, n) => { c.recording.noise_margin_db = n; })}
          {@render numberInput("min_speech_seconds", t("settings.min_speech"), t("settings.min_speech_hint"), 0.03, 5, 0.01, resetBusy,
            (c) => c.recording.min_speech_seconds, (c, n) => { c.recording.min_speech_seconds = n; })}
          <div class="row"><Button variant="ghost" busy={resetBusy} onclick={() => void resetSpeechDetection()}>{t("settings.speech_reset")}</Button></div>
          {@render fieldError("speech_reset")}
        </div>
      </details>

      <details class="card settings-group" open={["paste_method", "paste_after", "restore_clipboard", "trailing_space", "overlay"].some((field) => !!errors[field])}>
        <summary>{t("settings.pasting")}</summary>
        <div class="stack group-body">
        <Select label={t("settings.paste_method")} options={pasteMethodOptions} value={view.pasteMethod}
          info={config.paste.method === "type" ? t("settings.paste_method_type_hint") : t("settings.paste_method_hint")}
          onchange={(v) => void save("paste_method", (c) => { c.paste.method = v as PasteMethod; })} />
        {@render fieldError("paste_method")}
        <Select label={t("settings.paste_after")} options={pasteAfterOptions} value={view.pasteAfter}
          info={t("settings.paste_after_hint")}
          onchange={(v) => void save("paste_after", (c) => { c.paste.after = v as PasteAfter; })} />
        {@render fieldError("paste_after")}
        <Switch label={t("settings.restore_clipboard")}
          info={config.paste.method === "type" ? t("settings.restore_clipboard_type") : t("settings.restore_clipboard_hint")}
          disabled={config.paste.method === "type"}
          checked={view.restoreClipboard}
          onchange={(v) => void save("restore_clipboard", (c) => { c.paste.restore_clipboard = v; })} />
        {@render fieldError("restore_clipboard")}
        <Switch label={t("settings.trailing_space")} checked={view.trailingSpace}
          onchange={(v) => void save("trailing_space", (c) => { c.paste.trailing_space = v; })} />
        {@render fieldError("trailing_space")}
        <Select label={t("settings.overlay")} options={overlayOptions} value={view.overlay}
          onchange={(v) => void save("overlay", (c) => { c.ui.overlay_position = v as OverlayPosition; })} />
        {@render fieldError("overlay")}
      </div>
      </details>

      <details class="card settings-group" open={["history_enabled", "save_audio", "retention", "max_entries"].some((field) => !!errors[field])}>
        <summary>{t("settings.history")}</summary>
        <div class="stack group-body">
        <Switch label={t("settings.history_enabled")} checked={view.historyEnabled}
          onchange={(v) => void save("history_enabled", (c) => { c.history.enabled = v; })} />
        {@render fieldError("history_enabled")}
        {@render numberInput("max_entries", t("settings.max_entries"), t("settings.max_entries_hint"), 0, 100000, 1,
          !config.history.enabled,
          (c) => c.history.max_entries, (c, n) => { c.history.max_entries = n; })}
        <Switch label={t("settings.save_audio")} info={t("settings.save_audio_hint")}
          checked={view.saveAudio} disabled={!config.history.enabled}
          onchange={(v) => void save("save_audio", (c) => { c.history.save_audio = v; })} />
        {@render fieldError("save_audio")}
        {@render numberInput("retention", t("settings.retention"), undefined, 1, 365, 1,
          !config.history.enabled || !config.history.save_audio,
          (c) => c.history.audio_retention_days, (c, n) => { c.history.audio_retention_days = n; })}
      </div>
      </details>

      <details class="card settings-group">
        <summary><InfoTip text={t("ask.description")}>{t("ask.title")}</InfoTip></summary>
        <div class="stack group-body"><AskSettings /></div>
      </details>

      <details class="card settings-group">
        <summary>{t("cleanup.title")}</summary>
        <div class="stack group-body"><CleanupSettings /><CleanupStyles /></div>
      </details>

      <details class="card settings-group" open={installing !== null || !!errors.update_check}>
        <summary>{t("settings.updates")}</summary>
        <div class="stack group-body">
        <Switch label={t("settings.check_updates")} checked={view.checkUpdates} disabled={debugBuild}
          info={debugBuild ? t("update.debug") : t("settings.check_updates_hint")}
          onchange={(v) => void save("check_updates", (c) => { c.ui.check_updates = v; })} />
        {@render fieldError("check_updates")}
        <p role="status">{t(updateStatus.key, updateStatus.params)}</p>
        {#if installing}<UpdateProgress downloaded={installing.downloaded} total={installing.total} />{/if}
        <div class="row">
          <Button busy={app.update.kind === "checking"} disabled={debugBuild || installing !== null}
            onclick={() => void checkNow()}>{t("update.check_now")}</Button>
          {#if app.update.kind === "available"}<UpdateInstallButton />{/if}
        </div>
        {@render fieldError("update_check")}
      </div>
      </details>

      <details class="card settings-group" open={!!errors.copy_data || !!errors.copy_log || !!errors.copy_cli}>
        <summary>{t("settings.about")}</summary>
        <div class="stack group-body">
        {#if app.info}
          <p>{t("settings.version", { version: app.info.version })}</p>
          <dl class="paths">
            {#each [
              { field: "copy_data", label: t("settings.data_dir"), info: t("settings.keys_note"), path: app.info.data_dir },
              { field: "copy_log", label: t("settings.log_dir"), info: t("settings.logs_note"), path: app.info.log_dir },
              { field: "copy_cli", label: t("settings.cli_path"), info: undefined, path: app.info.cli_path },
            ] as item (item.field)}
              <div>
                <dt><InfoTip text={item.info}><span id={`settings-${item.field}`}>{item.label}</span></InfoTip></dt>
                <dd>
                  <code>{item.path}</code>
                  <Button variant="ghost" aria-label={t("common.copy")} title={t("common.copy")}
                    aria-describedby={`settings-${item.field}`}
                    onclick={() => void copyPath(item.field, item.path)}>
                    <Icon name="copy" />
                  </Button>
                  {@render fieldError(item.field)}
                </dd>
              </div>
            {/each}
          </dl>
        {/if}
      </div>
      </details>
    </div>
  {/if}
</div>

<style>
  summary { cursor: pointer; font-weight: 600; }
  .group-body { margin-top: var(--space-4); }
  .sections { display: flex; flex-direction: column; gap: var(--space-4); }
  .error { color: var(--danger); font-size: var(--text-sm); }
  .mic-row { display: flex; align-items: flex-end; gap: var(--space-2); }
  .grow { flex: 1; min-width: 0; }
  .number { width: 120px; }
  .paths { display: flex; flex-direction: column; gap: var(--space-3); margin: 0; }
  dt { font-weight: 600; }
  dd { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); margin: 0; }
  dd .error { flex-basis: 100%; }
  dd code { flex: 1; min-width: 0; overflow-wrap: anywhere; padding: var(--space-1) var(--space-2); background: var(--surface-2); border-radius: var(--radius-sm); }
</style>
