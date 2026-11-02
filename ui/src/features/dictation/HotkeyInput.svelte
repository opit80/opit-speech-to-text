<script lang="ts">
  import { tick } from "svelte";
  import { api } from "../../lib/bridge/api";
  import { onHotkeyCaptureIntercepted, type UnlistenFn } from "../../lib/bridge/events";
  import { app, errorText, t } from "../../lib/state/app.svelte";
  import { ComboRecorder, comboProblem, defaultKeys, formatCombo, formatLinuxCombo, parseComboText, LINUX_COMBOS, keyNameFromCode, SELECTABLE_KEYS, type ComboProblem } from "./hotkey";
  import { CUSTOM_COMBO, linuxComboChoice, linuxComboText } from "./linux-shortcut-picker";
  import { LinuxShortcutCapture, LINUX_CAPTURE_TIMEOUT_MS, type CaptureCancelReason, type CaptureResult } from "./linux-shortcut-capture";
  import type { MessageKey } from "../../lib/i18n";
  import type { AppConfig, HotkeyTarget, LinuxShortcutState } from "../../lib/bridge/types";
  import Button from "../../lib/components/Button.svelte";
  import Field from "../../lib/components/Field.svelte";
  import InfoTip from "../../lib/components/InfoTip.svelte";
  import Select from "../../lib/components/Select.svelte";

  interface Props {
    keys: string[];
    label?: string;
    /** Tooltip text on the label; the main shortcut's note by default. */
    info?: string;
    optional?: boolean;
    /** Label of the clear button of an optional shortcut; "Use tray only" by default. */
    clearLabel?: string;
    /** Also checks conflicts with the other shortcuts (against `config`, else the saved settings). */
    target?: HotkeyTarget;
    /** An unsaved config draft to check conflicts against. */
    config?: AppConfig;
    onerrorchange?: (error: string | null) => void;
    onchange: (keys: string[]) => void | Promise<void>;
  }
  let { keys, onchange, label, info, optional = false, clearLabel, target, config, onerrorchange }: Props = $props();

  async function validate(combo: string[]) {
    const problem = comboProblem(combo, app.capabilities?.platform);
    if (problem) throw new Error(t(PROBLEM_KEYS[problem]));
    await api.validateHotkey(combo, target, config ? $state.snapshot(config) as AppConfig : undefined);
  }

  const PROBLEM_KEYS: Record<ComboProblem, MessageKey> = {
    empty: "settings.combo.empty",
    shape: "settings.combo.shape",
    typing: "settings.combo.typing",
  };
  const linux = $derived(app.capabilities?.platform === "linux");
  let text = $state("");
  const linuxComboOptions = $derived([
    ...LINUX_COMBOS.map((combo) => ({ value: combo, label: combo })),
    { value: CUSTOM_COMBO, label: t("settings.shortcut_custom") },
  ]);
  let registry = $state<LinuxShortcutState | null>(null);
  let registryLoading = $state(false);
  let registryError = $state<string | null>(null);
  let registryGeneration = 0;
  const registryBackend = $derived(registry?.backend ?? app.capabilities?.shortcut_backend);
  const desktopRegistry = $derived(registryBackend === "kde" || registryBackend === "gnome");
  $effect(() => { text = formatLinuxCombo(keys); });
  $effect(() => {
    if (linux && !config) void readRegistry(target ?? "dictation", [...keys]);
  });

  async function readRegistry(action: HotkeyTarget, combo: string[]) {
    const generation = ++registryGeneration;
    registryLoading = true;
    registryError = null;
    try {
      const state = await api.linuxShortcutRegistry(action, combo, "read");
      if (!disposed && generation === registryGeneration) registry = state;
    } catch {
      if (!disposed && generation === registryGeneration) registryError = t("settings.registry_error");
    } finally {
      if (!disposed && generation === registryGeneration) registryLoading = false;
    }
  }

  async function installLinux() {
    if (saving || checking) return;
    const combo = parseComboText(text);
    if (!combo) { error = t("settings.combo.syntax"); return; }
    if (optional && combo.length === 0) { await removeLinux(); return; }
    error = null;
    checking = true;
    try {
      await validate(combo);
      await onchange(combo);
      if (disposed) return;
      if (!config && desktopRegistry) {
        await tick();
        ++registryGeneration;
        registryLoading = false;
        registry = await api.linuxShortcutRegistry(target ?? "dictation", combo, "install");
        registryError = null;
      }
    } catch (e) { if (!disposed) error = errorText(e); }
    finally { if (!disposed) checking = false; }
  }

  async function removeLinux() {
    if (saving) return;
    error = null;
    saving = true;
    try {
      let removed: LinuxShortcutState | null = null;
      if (!config && desktopRegistry) {
        ++registryGeneration;
        removed = await api.linuxShortcutRegistry(target ?? "dictation", [...keys], "remove");
        registryError = null;
      }
      if (optional) await onchange([]);
      await tick();
      if (disposed) return;
      // A config change starts a read effect; keep the removal's login warning until refresh.
      ++registryGeneration;
      registryLoading = false;
      if (removed) registry = removed;
      text = optional ? "" : formatLinuxCombo(keys);
    } catch (e) { if (!disposed) error = errorText(e); }
    finally { if (!disposed) saving = false; }
  }

  const recorder = new ComboRecorder();
  const linuxCapture = new LinuxShortcutCapture(recorder);
  let captureStatus = $state<MessageKey | null>(null);
  let captureUnlisten: UnlistenFn | undefined;
  let box = $state<HTMLInputElement>();
  let starting = $state(false);
  let capturing = $state(false);
  let checking = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  $effect(() => { onerrorchange?.(error); });
  let chosenKey = $state("F13");
  let selectedKeys = $state<string[]>([]);
  const keyOptions = $derived(SELECTABLE_KEYS.map((key) => ({ value: key, label: formatCombo([key], app.lang, app.capabilities?.platform) })));
  let expiry: ReturnType<typeof setTimeout> | undefined;
  // Bumped whenever a capture ends, so a late validation result of that capture is ignored.
  let session = 0;
  let disposed = false;

  function releaseCaptureListener() {
    captureUnlisten?.();
    captureUnlisten = undefined;
  }

  function linuxCaptureResult(result: CaptureResult) {
    if (result.kind === "refused") { error = t(PROBLEM_KEYS[result.problem]); return; }
    if (result.kind !== "recorded" && result.kind !== "cancelled") return;
    text = result.text;
    error = null;
    captureStatus = result.kind === "cancelled" ? result.status : "settings.capture_recorded_linux";
    if (capturing) stopCapture();
    else session++; // Cancelled while the listener or backend was still starting.
    releaseCaptureListener();
  }

  function cancelLinuxCapture(reason: CaptureCancelReason) {
    linuxCaptureResult(linuxCapture.cancel(reason));
  }

  async function startLinuxCapture() {
    if (starting || capturing || saving || checking || disposed) return;
    error = null;
    captureStatus = null;
    starting = true;
    linuxCapture.start(text);
    const current = ++session;
    try {
      // Subscribe before suppressing relays so even an immediate interception is reported.
      const unlisten = await onHotkeyCaptureIntercepted(() => cancelLinuxCapture("desktop"));
      if (disposed || current !== session) { unlisten(); return; }
      captureUnlisten = unlisten;
      await api.setHotkeyCapture(true);
      if (disposed || current !== session) {
        await api.setHotkeyCapture(false);
        return;
      }
      recorder.reset();
      capturing = true;
      expiry = setTimeout(() => cancelLinuxCapture("timeout"), LINUX_CAPTURE_TIMEOUT_MS);
      await tick();
      if (!disposed && current === session) box?.focus();
    } catch (e) {
      if (!disposed && current === session) {
        cancelLinuxCapture("cancel");
        captureStatus = null;
        error = errorText(e);
      }
      // A failed response must not leave a successfully enabled backend suppressed.
      void api.setHotkeyCapture(false).catch(() => {});
    } finally {
      if (!disposed) starting = false;
    }
  }

  function linuxKeydown(event: KeyboardEvent) {
    if (!capturing) return;
    event.preventDefault();
    event.stopPropagation();
    linuxCaptureResult(linuxCapture.keydown(event));
  }

  async function startCapture() {
    if (linux || starting || capturing || disposed) return;
    error = null;
    starting = true;
    try {
      // Pauses the global hook, so pressing the current shortcut does not start a dictation.
      await api.setHotkeyCapture(true);
    } catch (e) {
      if (!disposed) {
        error = errorText(e);
        starting = false;
      }
      return;
    }
    if (disposed) {
      void api.setHotkeyCapture(false).catch(() => {});
      return;
    }
    starting = false;
    session++;
    recorder.reset();
    capturing = true;
    expiry = setTimeout(stopCapture, 30_000);
    await tick();
    box?.focus();
  }

  /** Ends the capture for any reason and resumes the hook. */
  function stopCapture() {
    if (!capturing) return;
    capturing = false;
    clearTimeout(expiry);
    checking = false;
    session++;
    recorder.reset();
    void api.setHotkeyCapture(false).catch((e: unknown) => {
      if (!disposed) error = errorText(e);
    });
  }

  async function finish(combo: string[]) {
    if (checking) return;
    const problem = comboProblem(combo, app.capabilities?.platform);
    if (problem) {
      error = t(PROBLEM_KEYS[problem]);
      return;
    }
    const current = session;
    error = null;
    checking = true;
    try {
      await validate(combo);
    } catch (e) {
      if (current === session && !disposed) {
        error = errorText(e);
        checking = false;
      }
      return;
    }
    if (current !== session || disposed) return;
    stopCapture();
    await apply(combo);
  }

  async function apply(combo: string[]) {
    saving = true;
    try {
      await onchange(combo);
    } catch (e) {
      if (!disposed) error = errorText(e);
    } finally {
      if (!disposed) saving = false;
    }
  }

  function keydown(event: KeyboardEvent) {
    if (!capturing) return;
    if (keyNameFromCode(event.code) === null) return;
    event.preventDefault();
    if (event.repeat) return;
    recorder.down(event.code);
  }

  function keyup(event: KeyboardEvent) {
    if (!capturing || keyNameFromCode(event.code) === null) return;
    event.preventDefault();
    const combo = recorder.up(event.code);
    if (combo) void finish(combo);
  }

  function reset() {
    stopCapture();
    error = null;
    void apply(optional ? [] : defaultKeys(app.capabilities?.platform));
  }

  async function useSelectedKey() {
    if (saving || starting || checking) return;
    stopCapture();
    error = null;
    checking = true;
    const combo = selectedKeys.length ? [...selectedKeys] : [chosenKey];
    try { await validate(combo); if (!disposed) await apply(combo); }
    catch (e) { if (!disposed) error = errorText(e); }
    finally { if (!disposed) checking = false; }
  }

  // Leaving the page mid-capture resumes the hook (Rust also expires the capture after 30 s).
  $effect(() => () => {
    disposed = true;
    if (linux) { session++; releaseCaptureListener(); }
    clearTimeout(expiry);
    if (capturing) {
      capturing = false;
      void api.setHotkeyCapture(false).catch(() => {});
    }
  });
</script>

<svelte:window onblur={() => { if (linux) cancelLinuxCapture("blur"); }} />

{#if linux}
  <div class="linux-shortcut">
    <Select label={t("settings.shortcut_presets")} options={linuxComboOptions}
      value={linuxComboChoice(text)} disabled={saving || checking || starting || capturing}
      onchange={(choice) => { text = linuxComboText(choice); error = null; captureStatus = null; }} />
    <Field label={label ?? t("settings.shortcut")} info={info ?? t("settings.shortcut_note_linux")} error={error ?? registryError}>
      {#snippet children(id, describedBy)}
        <div class="row linux-combo-row">
          <input {id} bind:this={box} class="combo" class:capturing value={capturing ? t("settings.press_keys_linux") : text}
            readonly={starting || capturing} placeholder={t("settings.shortcut_placeholder")} autocomplete="off" spellcheck="false"
            disabled={saving || checking} aria-describedby={describedBy} aria-invalid={!!error}
            onkeydown={linuxKeydown} onblur={() => cancelLinuxCapture("blur")}
            oninput={(event) => { text = event.currentTarget.value; error = null; captureStatus = null; }} />
          <Button busy={starting} disabled={saving || checking || capturing} onclick={() => void startLinuxCapture()}>
            {t("settings.record_shortcut")}
          </Button>
          {#if capturing}<Button variant="ghost" onclick={() => cancelLinuxCapture("cancel")}>{t("settings.capture_cancel")}</Button>{/if}
          <Button busy={checking} disabled={saving || starting || capturing} onclick={() => void installLinux()}>
            {t(!config && desktopRegistry ? "settings.registry_install" : "settings.apply_shortcut")}
          </Button>
          {#if desktopRegistry || optional}
            <Button variant="ghost" busy={saving} disabled={checking || starting || capturing} onclick={() => void removeLinux()}>{t("settings.registry_remove")}</Button>
          {/if}
        </div>
        <div class="registry-status" id={`${id}-registry`} role="status" aria-live="polite">
          {#if capturing}<p class="muted">{t("settings.press_keys_linux")}</p>
          {:else if captureStatus}<p class="muted">{t(captureStatus)}</p>{/if}
          {#if config}
            <p class="muted">{t("settings.registry_save_first")}</p>
          {:else}
            {#if registryLoading}<p class="muted">{t("common.loading")}</p>
            {:else if registry}
              {#if desktopRegistry}
                <p class="muted">{registry.registered ? t("settings.registry_registered", { combo: registry.registered }) : t("settings.registry_empty")}</p>
              {:else if registry.backend === "x11"}
                <p class="muted">{t("settings.registry_x11")}</p>
              {:else}
                <p class="muted">{t("settings.registry_manual")}</p>
              {/if}
              {#if registry.after_login}<p class="muted">{t("settings.registry_login")}</p>{/if}
              {#if registry.conflict}<p class="error">{t("settings.registry_conflict")}</p>{/if}
              {#if registry.backend === "manual"}
                <Field label={t("settings.registry_command")}>
                  {#snippet children(commandId)}<input id={commandId} class="command" readonly value={registry?.command ?? ""} />{/snippet}
                </Field>
              {/if}
            {/if}
          {/if}
        </div>
        {#if !config}
          <Button variant="ghost" disabled={saving || checking || registryLoading || starting || capturing}
            onclick={() => void readRegistry(target ?? "dictation", [...keys])}>{t("settings.registry_refresh")}</Button>
        {/if}
      {/snippet}
    </Field>
  </div>
{:else}
<Field label={label ?? t("settings.shortcut")} info={info ?? t("settings.shortcut_note")} {error}>
  {#snippet children(id, describedBy)}
    <div class="row">
      <input {id} bind:this={box} class="combo" class:capturing readonly
        value={capturing ? t("settings.press_keys") : formatCombo(keys, app.lang, app.capabilities?.platform)}
        aria-describedby={describedBy}
        onkeydown={keydown} onkeyup={keyup} onblur={stopCapture} />
      <Button busy={starting || checking || saving} onclick={() => void startCapture()}>
        {t("settings.change")}
      </Button>
      <Button variant="ghost" disabled={saving} onclick={reset}>{optional ? clearLabel ?? t("settings.pause_clear") : t("settings.reset_default")}</Button>
      {#if capturing}<Button variant="ghost" onclick={stopCapture}>{t("settings.capture_cancel")}</Button>{/if}
    </div>
  {/snippet}
</Field>
<details>
  <summary><InfoTip text={t("settings.shortcut_details")}>{t("settings.select_keys")}</InfoTip></summary>
  <div class="picker"><Select label={t("settings.key_choice")} options={keyOptions} bind:value={chosenKey} disabled={saving || checking} /><Button disabled={saving || checking} onclick={() => { if (!selectedKeys.includes(chosenKey)) selectedKeys = [...selectedKeys, chosenKey]; }}>{t("settings.add_key")}</Button></div>
  <div class="picker">
    {#each selectedKeys as key (key)}<Button variant="ghost" disabled={saving || checking} aria-label={t("settings.remove_key", { key: formatCombo([key], app.lang, app.capabilities?.platform) })} onclick={() => { selectedKeys = selectedKeys.filter((item) => item !== key); }}>{formatCombo([key], app.lang, app.capabilities?.platform)} ×</Button>{/each}
    <Button disabled={saving || starting || checking} onclick={() => void useSelectedKey()}>{t("settings.apply_shortcut")}</Button>
  </div>
</details>

{/if}

<style>
  .linux-shortcut { display: flex; flex-direction: column; gap: var(--space-3); min-width: 0; }
  .linux-combo-row .combo { flex: 1 1 180px; min-width: 0; max-width: 100%; cursor: text; }
  .combo { flex: 1; min-width: 180px; font-weight: 600; cursor: default; }
  .command { width: 100%; font-family: var(--font-mono); }
  .registry-status { min-width: 0; overflow-wrap: anywhere; }
  .error { color: var(--danger); }
  .combo.capturing { border-color: var(--accent); font-weight: 400; color: var(--text-muted); }
  summary { cursor: pointer; color: var(--accent); font-size: var(--text-sm); }
  .picker { display: flex; align-items: flex-end; gap: var(--space-2); flex-wrap: wrap; margin-top: var(--space-3); }
</style>
