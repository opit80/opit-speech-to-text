<script lang="ts">
  import { revealFieldErrors } from "../../lib/utils/reveal-field-errors";
  import { advancedShouldOpen } from "./advanced";
  import AppRules from "./AppRules.svelte";
  import { PASTE_AFTER_KEYS, PASTE_METHOD_KEYS } from "./app-rules";
  import AskSettings from "./AskSettings.svelte";
  import CleanupSettings from "./CleanupSettings.svelte";
  import CleanupStyles from "./CleanupStyles.svelte";
  import { tick, untrack } from "svelte";
  import { api } from "../../lib/bridge/api";
  import { app, checkForUpdates, errorText, saveConfig, t } from "../../lib/state/app.svelte";
  import type { MessageKey } from "../../lib/i18n";
  import { toast } from "../../lib/state/toast.svelte";
  import type { AppConfig, HotkeyMode, MediaMode, OverlayPosition, PasteAfter, PasteMethod, Theme, TranslateLanguage } from "../../lib/bridge/types";
  import { duckPercentRange, parseNumberInput } from "../../lib/utils/number-input";
  import Banner from "../../lib/components/Banner.svelte";
  import Button from "../../lib/components/Button.svelte";
  import Field from "../../lib/components/Field.svelte";
  import HotkeyInput from "../dictation/HotkeyInput.svelte";
  import Icon from "../../lib/components/Icon.svelte";
  import InfoTip from "../../lib/components/InfoTip.svelte";
  import MicLevelTest from "../dictation/MicLevelTest.svelte";
  import PageHeader from "../../lib/components/PageHeader.svelte";
  import Select from "../../lib/components/Select.svelte";
  import Switch from "../../lib/components/Switch.svelte";
  import UpdateInstallButton from "../updates/UpdateInstallButton.svelte";
  import UpdateProgress from "../updates/UpdateProgress.svelte";
  import { statusMessage } from "../updates/update";
  import { speechDetectionCustomized, speechDetectionDefaults } from "../dictation/speech-detection";
  import { cleanupConnectionReady, languageOptions as translateLanguageOptions, voiceModesInUse } from "../dictation/voice-modes";

  let shortcutError = $state<string | null>(null);
  let errors = $state<Record<string, string | null>>({});
  // Bumped after a failed change: the controls re-read the live value and drop what the user picked.
  let revision = $state(0);
  // Text of a focused number input; null when it shows the live value.
  let drafts = $state<Record<string, string | null>>({});
  let mics = $state<string[]>([]);
  let micsBusy = $state(false);
  let micsError = $state<string | null>(null);
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
      mode: app.capabilities?.platform === "linux" && !app.capabilities.shortcut_release ? "toggle" : c.hotkey.mode as string,
      hotkeyEnabled: c.hotkey.enabled,
      microphone: c.recording.microphone ?? "",
      liveText: c.recording.live_text,
      pasteMethod: c.paste.method as string,
      pasteAfter: c.paste.after as string,
      restoreClipboard: c.paste.restore_clipboard,
      trailingSpace: c.paste.trailing_space,
      overlay: c.ui.overlay_position as string,
      mediaEnabled: c.media.enabled,
      mediaMode: c.media.mode as string,
      historyEnabled: c.history.enabled,
      saveAudio: c.history.save_audio,
      learning: c.learning.enabled,
      checkUpdates: c.ui.check_updates,
      translateLanguage: c.translate.target_language,
      translateModel: c.translate.model ?? "",
    };
  });

  // ---- Voice edit and translation
  let languages = $state<TranslateLanguage[]>([]);
  // Whether the cleanup connection's key is stored; null until checked (no hint before that).
  let cleanupKey = $state<{ ref: string; stored: boolean } | null>(null);
  let cleanupGroup = $state<HTMLDetailsElement>();
  let keyRequest = 0;
  // Bumped when AI cleanup stores a key, so the hint is re-checked without a config change.
  let keySaved = $state(0);
  const translateOptions = $derived(translateLanguageOptions(languages, app.lang, config?.translate.target_language ?? "en"));

  async function loadLanguages() {
    errors.translate_languages = null;
    try {
      const list = await api.translateLanguages();
      if (!disposed) languages = list;
    } catch (e) {
      if (!disposed) errors.translate_languages = errorText(e);
    }
  }

  // Memoized, so only a change of the cleanup connection (not every settings save) re-runs the check.
  const keyCheck = $derived.by(() => {
    const c = app.config?.cleanup;
    return c ? JSON.stringify([c.api_key_ref, c.base_url, c.model]) : "";
  });

  $effect(() => {
    void keyCheck;
    void keySaved;
    const ref = untrack(() => app.config?.cleanup.api_key_ref ?? null);
    const request = ++keyRequest;
    if (!ref) return;
    untrack(() => {
      api.hasApiKey(ref)
        .then((stored) => { if (!disposed && request === keyRequest) cleanupKey = { ref, stored }; })
        .catch(() => {});
    });
  });

  const cleanupMissing = $derived.by(() => {
    if (!config || !voiceModesInUse(config)) return false;
    const ref = config.cleanup.api_key_ref;
    if (ref !== null && cleanupKey?.ref !== ref) return false;
    return !cleanupConnectionReady(config.cleanup, cleanupKey?.stored ?? false);
  });

  /** Blank uses the cleanup model; afterwards the input shows what Rust stored. */
  async function saveTranslateModel(input: HTMLInputElement) {
    const model = input.value.trim() || null;
    await save("translate_model", (c) => { c.translate.model = model; });
    if (!disposed && app.config) input.value = app.config.translate.model ?? "";
  }

  async function openCleanup() {
    if (!cleanupGroup) return;
    cleanupGroup.open = true;
    await tick();
    const summary = cleanupGroup.querySelector("summary");
    summary?.scrollIntoView({ block: "start" });
    summary?.focus();
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
    untrack(() => { void loadMics(); void loadLanguages(); });
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

  // ---- Live text: whether the active profile can stream; null until known (no note before that).
  let activeStreams = $state<boolean | null>(null);
  let streamRequest = 0;

  // Memoized, so other settings saves do not re-run the check.
  const streamKey = $derived(JSON.stringify([app.config?.active_profile_id ?? null, app.config?.profiles ?? []]));

  // Re-checked when the active profile or the profiles change; a failed check shows no note.
  $effect(() => {
    void streamKey;
    const id = untrack(() => app.config?.active_profile_id ?? null);
    const request = ++streamRequest;
    untrack(() => {
      activeStreams = null;
      if (!id) return;
      api.streamingSupported(id)
        .then((supported) => { if (!disposed && request === streamRequest) activeStreams = supported; })
        .catch(() => {});
    });
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
  const pasteMethodOptions = $derived(
    (Object.keys(PASTE_METHOD_KEYS) as PasteMethod[])
      .filter((value) => app.capabilities?.platform !== "linux" || value !== "type" || view?.pasteMethod === "type")
      .map((value) => ({ value, label: t(PASTE_METHOD_KEYS[value]) })),
  );
  const pasteAfterOptions = $derived(
    (Object.keys(PASTE_AFTER_KEYS) as PasteAfter[]).map((value) => ({ value, label: t(PASTE_AFTER_KEYS[value]) })),
  );
  const mediaModeKeys: Record<MediaMode, MessageKey> = {
    pause: "settings.media_mode.pause",
    duck: "settings.media_mode.duck",
  };
  const mediaModeOptions = $derived(
    (Object.keys(mediaModeKeys) as MediaMode[])
      .map((value) => ({ value, label: t(mediaModeKeys[value]) })),
  );
  const modeOptions = $derived([
    { value: "toggle", label: t("settings.mode_toggle") },
    ...((app.capabilities?.platform !== "linux" || app.capabilities.shortcut_release) ? [{ value: "push_to_talk", label: t("settings.mode_ptt") }] : []),
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

<div use:revealFieldErrors>
  <PageHeader title={t("settings.title")} />
  {#if config && view}
    <div class="sections">
      <section class="card stack" aria-labelledby="settings-dictation">
        <h2 id="settings-dictation">{t("settings.dictation")}</h2>
        {#if app.capabilities?.platform === "linux"}<Banner tone="info">{t("settings.linux_limits")}</Banner>{/if}
        <HotkeyInput keys={config.hotkey.keys} target="dictation" onerrorchange={(error) => { shortcutError = error; }}
          onchange={(keys) => save("hotkey_keys", (c) => { c.hotkey.keys = keys; })} />
        {@render fieldError("hotkey_keys")}
        <Select label={t("settings.mode")} options={modeOptions} value={view.mode}
          info={app.capabilities?.platform === "linux" ? t("settings.mode_toggle_hint_linux") : config.hotkey.keys.includes("Escape") ? t("settings.cancel_from_home") : config.hotkey.mode === "toggle" ? t("settings.mode_toggle_hint") : undefined}
          onchange={(v) => void save("mode", (c) => { c.hotkey.mode = v as HotkeyMode; })} />
        {@render fieldError("mode")}
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
        <Select label={t("settings.paste_method")} options={pasteMethodOptions} value={view.pasteMethod}
          info={config.paste.method === "type" ? t("settings.paste_method_type_hint") : t("settings.paste_method_hint")}
          onchange={(v) => void save("paste_method", (c) => { c.paste.method = v as PasteMethod; })} />
        {@render fieldError("paste_method")}
      </section>

      <section class="card stack" aria-labelledby="settings-general">
        <h2 id="settings-general">{t("settings.general")}</h2>
        <Select label={t("settings.language")} options={languageOptions} value={view.language}
          onchange={(v) => void save("language", (c) => { c.ui_language = v === "system" ? null : v; })} />
        {@render fieldError("language")}
        <Select label={t("settings.theme")} options={themeOptions} value={view.theme}
          onchange={(v) => void save("theme", (c) => { c.ui.theme = v as Theme; })} />
        {@render fieldError("theme")}
        <Switch label={t(app.capabilities?.platform === "linux" ? "settings.autostart_linux" : "settings.autostart")} checked={view.autostart}
          disabled={app.info?.debug_build ?? false}
          info={app.info?.debug_build ? t("settings.autostart_debug") : undefined}
          onchange={(v) => void save("autostart", (c) => { c.ui.autostart = v; })} />
        {@render fieldError("autostart")}
        {#if app.capabilities?.tray !== false}
          <Switch label={t("settings.start_in_tray")}
            checked={view.startInTray}
            onchange={(v) => void save("start_in_tray", (c) => { c.ui.start_in_tray = v; })} />
          {@render fieldError("start_in_tray")}
        {/if}
        <Switch label={t("settings.sound")} checked={view.sound}
          onchange={(v) => void save("sound", (c) => { c.ui.sound_feedback = v; })} />
        {@render fieldError("sound")}
        <div class="media-row">
          <div class="stack">
            <Switch label={t("settings.media_enabled")} checked={view.mediaEnabled}
              onchange={(v) => void save("media_enabled", (c) => { c.media.enabled = v; })} />
            {@render fieldError("media_enabled")}
          </div>
        </div>
      </section>

      <section class="card stack" aria-labelledby="settings-history">
        <h2 id="settings-history">{t("settings.history")}</h2>
        <Switch label={t("settings.history_enabled")} checked={view.historyEnabled}
          onchange={(v) => void save("history_enabled", (c) => { c.history.enabled = v; })} />
        {@render fieldError("history_enabled")}
        <Switch label={t("settings.save_audio")} info={t("settings.save_audio_hint")}
          checked={view.saveAudio} disabled={!config.history.enabled}
          onchange={(v) => void save("save_audio", (c) => { c.history.save_audio = v; })} />
        {@render fieldError("save_audio")}
        {#if config.history.save_audio}
        {@render numberInput("retention", t("settings.retention"), undefined, 1, 365, 1,
          !config.history.enabled || !config.history.save_audio,
          (c) => c.history.audio_retention_days, (c, n) => { c.history.audio_retention_days = n; })}
        {/if}
      </section>

      <details class="card settings-group" bind:this={cleanupGroup}>
        <summary>{t("cleanup.title")}</summary>
        <div class="stack group-body">
          <CleanupSettings onkeysaved={() => { keySaved++; }} /><CleanupStyles />
        </div>
      </details>

      <section class="card stack" aria-labelledby="settings-updates">
        <h2 id="settings-updates">{t("settings.updates_about")}</h2>
        {#if app.info}<p>{t("settings.version", { version: app.info.version })}</p>{/if}
        {#if app.capabilities?.updates === false}<p>{t("settings.updates_linux")}</p>{:else}
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
        {/if}
      </section>

      <details class="card settings-group" open={advancedShouldOpen(errors, config.hotkey.enabled, app.hotkey.error ?? shortcutError, app.capabilities?.platform === "linux")}>
        <summary>{t("settings.advanced")}</summary>
        <div class="stack group-body">
          <section class="stack" aria-labelledby="settings-extra-shortcuts">
            <h3 id="settings-extra-shortcuts">{t("settings.extra_shortcuts")}</h3>
            {#if app.capabilities?.platform === "linux"}
              <HotkeyInput keys={config.hotkey.cancel_keys ?? []} optional target="cancel" label={t("settings.cancel_shortcut")}
                info={t("settings.cancel_shortcut_note")} onchange={(keys) => save("cancel_keys", (c) => { c.hotkey.cancel_keys = keys; })} />
              {@render fieldError("cancel_keys")}
            {/if}
            <HotkeyInput keys={config.hotkey.pause_keys} optional target="pause" label={t("settings.pause_shortcut")} info={t("settings.pause_shortcut_note")}
              onchange={(keys) => save("pause_keys", (c) => { c.hotkey.pause_keys = keys; })} />
            {@render fieldError("pause_keys")}
            <HotkeyInput keys={config.hotkey.style_cycle_keys} optional target="style_cycle" label={t("settings.style_cycle_shortcut")}
              info={t("settings.style_cycle_shortcut_note")}
              onchange={(keys) => save("style_cycle_keys", (c) => { c.hotkey.style_cycle_keys = keys; })} />
            {@render fieldError("style_cycle_keys")}
            <HotkeyInput keys={config.hotkey.edit_keys} optional target="edit" label={t("settings.edit_shortcut")}
              info={t("settings.edit_shortcut_note")} clearLabel={t("settings.shortcut_off")}
              onchange={(keys) => save("edit_keys", (c) => { c.hotkey.edit_keys = keys; })} />
            {@render fieldError("edit_keys")}
            <HotkeyInput keys={config.hotkey.translate_keys} optional target="translate" label={t("settings.translate_shortcut")}
              info={t("settings.translate_shortcut_note")} clearLabel={t("settings.shortcut_off")}
              onchange={(keys) => save("translate_keys", (c) => { c.hotkey.translate_keys = keys; })} />
            {@render fieldError("translate_keys")}
            {#if config.hotkey.translate_keys.length}
              <Select label={t("settings.translate_language")} options={translateOptions} value={view.translateLanguage}
                onchange={(v) => void save("translate_language", (c) => { c.translate.target_language = v; })} />
              {@render fieldError("translate_language")}
              {@render fieldError("translate_languages")}
            {/if}
            {#if config.translate.model !== null}
              <Field label={t("translate.model")} info={t("translate.model_hint")} error={errors.translate_model}>
                {#snippet children(id, describedBy)}
                  <input {id} class="wide" value={view.translateModel} aria-describedby={describedBy} placeholder={config.cleanup.model}
                    autocomplete="off" spellcheck="false"
                    onchange={(e) => void saveTranslateModel(e.currentTarget)} />
                {/snippet}
              </Field>
            {/if}
            {#if cleanupMissing}
              <Banner tone="warning">{t("settings.voice_modes_setup")}
                {#snippet action()}<Button onclick={() => void openCleanup()}>{t("settings.voice_modes_open_cleanup")}</Button>{/snippet}
              </Banner>
            {/if}
            <Switch label={t("settings.shortcut_enabled")} checked={view.hotkeyEnabled}
              onchange={(v) => void save("hotkey_enabled", (c) => { c.hotkey.enabled = v; })} />
            {@render fieldError("hotkey_enabled")}
          </section>
          <section class="stack" aria-labelledby="settings-recording-media">
            <h3 id="settings-recording-media">{t("settings.recording_media")}</h3>
            {@render numberInput("max_seconds", t("settings.max_seconds"), t("settings.max_seconds_hint"), 10, 600, 10, false,
              (c) => c.recording.max_seconds, (c, n) => { c.recording.max_seconds = n; })}
            <Switch label={t("settings.live_text")} checked={view.liveText}
              info={config.recording.live_text && activeStreams === false ? t("settings.live_text_unsupported") : t("settings.live_text_hint")}
              onchange={(v) => void save("live_text", (c) => { c.recording.live_text = v; })} />
            {@render fieldError("live_text")}
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
          </section>
          <section class="stack" aria-labelledby="settings-insertion-overlay">
            <h3 id="settings-insertion-overlay">{t("settings.insertion_overlay")}</h3>
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
            <Select label={t("settings.overlay")} options={overlayOptions} value={view.overlay} disabled={app.capabilities?.overlay === false}
              onchange={(v) => void save("overlay", (c) => { c.ui.overlay_position = v as OverlayPosition; })} />
            {@render fieldError("overlay")}
            {#if app.capabilities?.platform === "linux" && !app.capabilities.overlay}<p class="muted">{t("settings.no_overlay_linux")}</p>{/if}
          </section>
          <section class="stack" aria-labelledby="settings-history-limits">
            <h3 id="settings-history-limits">{t("settings.history_limits")}</h3>
            {@render numberInput("max_entries", t("settings.max_entries"), t("settings.max_entries_hint"), 0, 100000, 1,
              !config.history.enabled,
              (c) => c.history.max_entries, (c, n) => { c.history.max_entries = n; })}
            <Switch label={t("settings.learning")} info={t("settings.learning_hint")} checked={view.learning}
              onchange={(v) => void save("learning", (c) => { c.learning.enabled = v; })} />
            {@render fieldError("learning")}
          </section>
          <details>
            <summary><InfoTip text={t("ask.description")}>{t("ask.title")}</InfoTip></summary>
            <div class="stack group-body"><AskSettings /></div>
          </details>
          {#if app.capabilities?.per_app_rules !== false}
            <details>
              <summary>{t("app_rules.title")}</summary>
              <div class="stack group-body"><AppRules /></div>
            </details>
          {/if}
          <section class="stack" aria-labelledby="settings-folders">
            <h3 id="settings-folders">{t("settings.folders")}</h3>
            {#if app.info}
              <dl class="paths">
                {#each [
                  { field: "copy_data", label: t("settings.data_dir"), info: t(app.capabilities?.platform === "linux" ? "settings.keys_note_linux" : "settings.keys_note"), path: app.info.data_dir },
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
          </section>
          {#if speechDetectionCustomized(config.recording)}
            <section class="stack" aria-labelledby="settings-speech">
              <h3 id="settings-speech">{t("settings.speech_detection")}</h3>
              <div class="row"><Button variant="ghost" busy={resetBusy} onclick={() => void resetSpeechDetection()}>{t("settings.speech_reset")}</Button></div>
              {@render fieldError("speech_reset")}
            </section>
          {/if}
        </div>
      </details>
    </div>
  {/if}
</div>

<style>
  summary { cursor: pointer; font-weight: 600; }
  h3 { font-size: var(--text-md); }
  .group-body > section + section { border-top: 1px solid var(--border); padding-top: var(--space-4); }
  .group-body { margin-top: var(--space-4); }
  .sections { display: flex; flex-direction: column; gap: var(--space-4); }
  .error { color: var(--danger); font-size: var(--text-sm); }
  .media-row { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-3); }
  .mic-row { display: flex; align-items: flex-end; gap: var(--space-2); }
  .grow { flex: 1; min-width: 0; }
  .number { width: 120px; }
  .wide { width: 100%; }
  .paths { display: flex; flex-direction: column; gap: var(--space-3); margin: 0; }
  dt { font-weight: 600; }
  dd { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); margin: 0; }
  dd .error { flex-basis: 100%; }
  dd code { flex: 1; min-width: 0; overflow-wrap: anywhere; padding: var(--space-1) var(--space-2); background: var(--surface-2); border-radius: var(--radius-sm); }
</style>
