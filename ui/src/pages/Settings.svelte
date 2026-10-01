<script lang="ts">
  import { untrack } from "svelte";
  import { api } from "../lib/api";
  import { app, errorText, saveConfig, t } from "../lib/app.svelte";
  import type { MessageKey } from "../lib/i18n";
  import { toast } from "../lib/toast.svelte";
  import type { AppConfig, HotkeyMode, OverlayPosition } from "../lib/types";
  import Banner from "../lib/components/Banner.svelte";
  import Button from "../lib/components/Button.svelte";
  import Field from "../lib/components/Field.svelte";
  import HotkeyInput from "../lib/components/HotkeyInput.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import MicLevelTest from "../lib/components/MicLevelTest.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import Select from "../lib/components/Select.svelte";
  import Switch from "../lib/components/Switch.svelte";
  import UpdateInstallButton from "../lib/components/UpdateInstallButton.svelte";
  import UpdateProgress from "../lib/components/UpdateProgress.svelte";
  import { statusMessage } from "../lib/update";

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
      autostart: c.ui.autostart,
      startInTray: c.ui.start_in_tray,
      sound: c.ui.sound_feedback,
      mode: c.hotkey.mode as string,
      hotkeyEnabled: c.hotkey.enabled,
      paused: app.hotkey.paused,
      microphone: c.recording.microphone ?? "",
      restoreClipboard: c.paste.restore_clipboard,
      trailingSpace: c.paste.trailing_space,
      overlay: c.ui.overlay_position as string,
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
    return drafts[field] ?? String(value);
  }

  async function commitNumber(
    field: string,
    input: HTMLInputElement,
    min: number,
    max: number,
    read: (c: AppConfig) => number,
    write: (c: AppConfig, n: number) => void,
  ) {
    const n = Number(input.value);
    if (input.value.trim() !== "" && Number.isFinite(n)) {
      const value = Math.min(max, Math.max(min, Math.round(n)));
      await save(field, (c) => write(c, value));
    }
    if (disposed || !app.config) return;
    const stored = String(read(app.config));
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
      app.update = await api.checkForUpdates();
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
  const modeOptions = $derived([
    { value: "toggle", label: t("settings.mode_toggle") },
    { value: "push_to_talk", label: t("settings.mode_ptt") },
  ]);
</script>

{#snippet fieldError(field: string)}
  {#if errors[field]}<p class="error" role="alert">{errors[field]}</p>{/if}
{/snippet}

{#snippet numberInput(field: string, label: string, hint: string | undefined, min: number, max: number, step: number, disabled: boolean, read: (c: AppConfig) => number, write: (c: AppConfig, n: number) => void)}
  {#if config}
    <Field {label} {hint} error={errors[field]}>
      {#snippet children(id)}
        <input {id} class="number" type="number" {min} {max} {step} inputmode="numeric" {disabled}
          value={numberText(field, read(config))}
          aria-describedby={hint || errors[field] ? `${id}-description` : undefined}
          onfocus={(e) => (drafts[field] = e.currentTarget.value)}
          onblur={() => (drafts[field] = null)}
          onchange={(e) => void commitNumber(field, e.currentTarget, min, max, read, write)} />
      {/snippet}
    </Field>
  {/if}
{/snippet}

<div>
  <PageHeader title={t("settings.title")} />
  {#if config && view}
    <div class="sections">
      <section class="card stack" aria-labelledby="settings-general">
        <h2 id="settings-general">{t("settings.general")}</h2>
        <Select label={t("settings.language")} options={languageOptions} value={view.language}
          onchange={(v) => void save("language", (c) => { c.ui_language = v === "system" ? null : v; })} />
        {@render fieldError("language")}
        <Switch label={t("settings.autostart")} checked={view.autostart}
          disabled={app.info?.debug_build ?? false}
          hint={app.info?.debug_build ? t("settings.autostart_debug") : undefined}
          onchange={(v) => void save("autostart", (c) => { c.ui.autostart = v; })} />
        {@render fieldError("autostart")}
        <Switch label={t("settings.start_in_tray")} hint={t("settings.start_in_tray_hint")}
          checked={view.startInTray}
          onchange={(v) => void save("start_in_tray", (c) => { c.ui.start_in_tray = v; })} />
        {@render fieldError("start_in_tray")}
        <Switch label={t("settings.sound")} hint={t("settings.sound_hint")} checked={view.sound}
          onchange={(v) => void save("sound", (c) => { c.ui.sound_feedback = v; })} />
        {@render fieldError("sound")}
      </section>

      <section class="card stack" aria-labelledby="settings-shortcut">
        <h2 id="settings-shortcut">{t("settings.shortcut")}</h2>
        {#if app.hotkey.error}<Banner tone="error">{t("notice.hotkey_failed", { reason: app.hotkey.error })}</Banner>{/if}
        <HotkeyInput keys={config.hotkey.keys}
          onchange={(keys) => save("hotkey_keys", (c) => { c.hotkey.keys = keys; })} />
        {@render fieldError("hotkey_keys")}
        <Select label={t("settings.mode")} options={modeOptions} value={view.mode}
          hint={t(config.hotkey.mode === "toggle" ? "settings.mode_toggle_hint" : "settings.mode_ptt_hint")}
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

      <section class="card stack" aria-labelledby="settings-pasting">
        <h2 id="settings-pasting">{t("settings.pasting")}</h2>
        <Switch label={t("settings.restore_clipboard")} hint={t("settings.restore_clipboard_hint")}
          checked={view.restoreClipboard}
          onchange={(v) => void save("restore_clipboard", (c) => { c.paste.restore_clipboard = v; })} />
        {@render fieldError("restore_clipboard")}
        <Switch label={t("settings.trailing_space")} checked={view.trailingSpace}
          onchange={(v) => void save("trailing_space", (c) => { c.paste.trailing_space = v; })} />
        {@render fieldError("trailing_space")}
        <Select label={t("settings.overlay")} options={overlayOptions} value={view.overlay}
          onchange={(v) => void save("overlay", (c) => { c.ui.overlay_position = v as OverlayPosition; })} />
        {@render fieldError("overlay")}
      </section>

      <section class="card stack" aria-labelledby="settings-history">
        <h2 id="settings-history">{t("settings.history")}</h2>
        <Switch label={t("settings.history_enabled")} checked={view.historyEnabled}
          onchange={(v) => void save("history_enabled", (c) => { c.history.enabled = v; })} />
        {@render fieldError("history_enabled")}
        <Switch label={t("settings.save_audio")} hint={t("settings.save_audio_hint")}
          checked={view.saveAudio} disabled={!config.history.enabled}
          onchange={(v) => void save("save_audio", (c) => { c.history.save_audio = v; })} />
        {@render fieldError("save_audio")}
        {@render numberInput("retention", t("settings.retention"), undefined, 1, 365, 1,
          !config.history.enabled || !config.history.save_audio,
          (c) => c.history.audio_retention_days, (c, n) => { c.history.audio_retention_days = n; })}
      </section>

      <section class="card stack" aria-labelledby="settings-updates">
        <h2 id="settings-updates">{t("settings.updates")}</h2>
        <Switch label={t("settings.check_updates")} checked={view.checkUpdates} disabled={debugBuild}
          hint={debugBuild ? t("update.debug") : t("settings.check_updates_hint")}
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
      </section>

      <section class="card stack" aria-labelledby="settings-about">
        <h2 id="settings-about">{t("settings.about")}</h2>
        {#if app.info}
          <p>{t("settings.version", { version: app.info.version })}</p>
          <dl class="paths">
            {#each [
              { field: "copy_data", label: t("settings.data_dir"), path: app.info.data_dir },
              { field: "copy_log", label: t("settings.log_dir"), path: app.info.log_dir },
            ] as item (item.field)}
              <div>
                <dt id={`settings-${item.field}`}>{item.label}</dt>
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
        <p class="muted">{t("settings.keys_note")}</p>
        <p class="muted">{t("settings.logs_note")}</p>
      </section>
    </div>
  {/if}
</div>

<style>
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
