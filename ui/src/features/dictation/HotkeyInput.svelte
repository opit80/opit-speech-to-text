<script lang="ts">
  import { tick } from "svelte";
  import { api } from "../../lib/bridge/api";
  import { app, errorText, t } from "../../lib/state/app.svelte";
  import { ComboRecorder, comboProblem, formatCombo, keyNameFromCode, SELECTABLE_KEYS, type ComboProblem } from "./hotkey";
  import type { MessageKey } from "../../lib/i18n";
  import type { AppConfig, HotkeyTarget } from "../../lib/bridge/types";
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
    onchange: (keys: string[]) => void | Promise<void>;
  }
  let { keys, onchange, label, info, optional = false, clearLabel, target, config }: Props = $props();

  const validate = (combo: string[]) => api.validateHotkey(combo, target, config ? $state.snapshot(config) as AppConfig : undefined);

  const DEFAULT_KEYS = ["RightCtrl", "RightShift"];
  const PROBLEM_KEYS: Record<ComboProblem, MessageKey> = {
    empty: "settings.combo.empty",
  };
  const recorder = new ComboRecorder();
  let box = $state<HTMLInputElement>();
  let starting = $state(false);
  let capturing = $state(false);
  let checking = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let chosenKey = $state("F13");
  let selectedKeys = $state<string[]>([]);
  const keyOptions = $derived(SELECTABLE_KEYS.map((key) => ({ value: key, label: formatCombo([key], app.lang) })));
  let expiry: ReturnType<typeof setTimeout> | undefined;
  // Bumped whenever a capture ends, so a late validation result of that capture is ignored.
  let session = 0;
  let disposed = false;

  async function startCapture() {
    if (starting || capturing || disposed) return;
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
    const problem = comboProblem(combo);
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
    void apply(optional ? [] : [...DEFAULT_KEYS]);
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
    clearTimeout(expiry);
    if (capturing) {
      capturing = false;
      void api.setHotkeyCapture(false).catch(() => {});
    }
  });
</script>

<Field label={label ?? t("settings.shortcut")} info={info ?? t("settings.shortcut_note")} {error}>
  {#snippet children(id, describedBy)}
    <div class="row">
      <input {id} bind:this={box} class="combo" class:capturing readonly
        value={capturing ? t("settings.press_keys") : formatCombo(keys, app.lang)}
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
    {#each selectedKeys as key (key)}<Button variant="ghost" disabled={saving || checking} aria-label={t("settings.remove_key", { key: formatCombo([key], app.lang) })} onclick={() => { selectedKeys = selectedKeys.filter((item) => item !== key); }}>{formatCombo([key], app.lang)} ×</Button>{/each}
    <Button disabled={saving || starting || checking} onclick={() => void useSelectedKey()}>{t("settings.apply_shortcut")}</Button>
  </div>
</details>

<style>
  .combo { flex: 1; min-width: 180px; font-weight: 600; cursor: default; }
  .combo.capturing { border-color: var(--accent); font-weight: 400; color: var(--text-muted); }
  summary { cursor: pointer; color: var(--accent); font-size: var(--text-sm); }
  .picker { display: flex; align-items: flex-end; gap: var(--space-2); flex-wrap: wrap; margin-top: var(--space-3); }
</style>
