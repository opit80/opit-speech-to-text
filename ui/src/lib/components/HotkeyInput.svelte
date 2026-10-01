<script lang="ts">
  import { tick } from "svelte";
  import { api } from "../api";
  import { app, errorText, t } from "../app.svelte";
  import { ComboRecorder, comboProblem, formatCombo, keyNameFromCode, type ComboProblem } from "../hotkey";
  import type { MessageKey } from "../i18n";
  import Button from "./Button.svelte";
  import Field from "./Field.svelte";

  interface Props {
    keys: string[];
    onchange: (keys: string[]) => void | Promise<void>;
  }
  let { keys, onchange }: Props = $props();

  const DEFAULT_KEYS = ["RightCtrl", "RightShift"];
  const PROBLEM_KEYS: Record<ComboProblem, MessageKey> = {
    empty: "settings.combo.empty",
    typing_only: "settings.combo.typing_only",
    single_modifier: "settings.combo.single_modifier",
    too_many: "settings.combo.too_many",
  };
  const recorder = new ComboRecorder();
  let box = $state<HTMLInputElement>();
  let starting = $state(false);
  let capturing = $state(false);
  let checking = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);
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
    await tick();
    box?.focus();
  }

  /** Ends the capture for any reason and resumes the hook. */
  function stopCapture() {
    if (!capturing) return;
    capturing = false;
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
      await api.validateHotkey(combo);
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
    if (event.code === "Escape") {
      event.preventDefault();
      stopCapture();
      return;
    }
    if (keyNameFromCode(event.code) === null) return; // Tab still moves focus (and cancels).
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
    void apply([...DEFAULT_KEYS]);
  }

  // Leaving the page mid-capture resumes the hook (Rust also expires the capture after 30 s).
  $effect(() => () => {
    disposed = true;
    if (capturing) {
      capturing = false;
      void api.setHotkeyCapture(false).catch(() => {});
    }
  });
</script>

<Field label={t("settings.shortcut")} hint={t("settings.shortcut_note")} {error}>
  {#snippet children(id)}
    <div class="row">
      <input {id} bind:this={box} class="combo" class:capturing readonly
        value={capturing ? t("settings.press_keys") : formatCombo(keys, app.lang)}
        aria-describedby={`${id}-description`}
        onkeydown={keydown} onkeyup={keyup} onblur={stopCapture} />
      <Button busy={starting || checking || saving} onclick={() => void startCapture()}>
        {t("settings.change")}
      </Button>
      <Button variant="ghost" disabled={saving} onclick={reset}>{t("settings.reset_default")}</Button>
    </div>
  {/snippet}
</Field>

<style>
  .combo { flex: 1; min-width: 180px; font-weight: 600; cursor: default; }
  .combo.capturing { border-color: var(--accent); font-weight: 400; color: var(--text-muted); }
</style>
