<script lang="ts">
  import { app, errorText, hotkeyErrorText, saveConfig, t } from "../../../lib/state/app.svelte";
  import type { HotkeyMode } from "../../../lib/bridge/types";
  import Banner from "../../../lib/components/Banner.svelte";
  import HotkeyInput from "../../dictation/HotkeyInput.svelte";
  import { api } from "../../../lib/bridge/api";
  import { shortcutTestAllowsNext } from "../shortcut-test";
  import Button from "../../../lib/components/Button.svelte";
  import Select from "../../../lib/components/Select.svelte";

  interface Props {
    onnext: () => void;
    canNext?: boolean;
    form: string;
  }
  let { onnext, canNext = $bindable(false), form }: Props = $props();

  let modeBusy = $state(false);
  let modeError = $state<string | null>(null);
  let disposed = false;

  let testBusy = $state(false);
  let testActive = $state(false);
  let testConfirmed = $state(false);
  let testError = $state<string | null>(null);
  let verifiedKeys = $state("");
  const keySignature = $derived(JSON.stringify(app.config?.hotkey.keys ?? []));
  const linux = $derived(app.capabilities?.platform === "linux");

  $effect(() => {
    canNext = shortcutTestAllowsNext(app.capabilities?.platform, keySignature, verifiedKeys, testConfirmed);
  });

  async function stopTest() {
    testActive = false;
    if (linux) await api.setShortcutTest(false);
  }

  async function startTest() {
    if (testBusy || disposed) return;
    testBusy = true;
    testError = null;
    testConfirmed = false;
    verifiedKeys = keySignature;
    try {
      const state = await api.setShortcutTest(true);
      if (disposed) { await api.setShortcutTest(false); return; }
      testActive = state.active;
      await api.probeShortcutCommand();
      const next = await api.shortcutTestState();
      if (!disposed && next.generation === state.generation) testConfirmed = next.confirmed;
    } catch (error) {
      if (!disposed) testError = errorText(error);
    } finally {
      await stopTest().catch(() => {});
      if (!disposed) testBusy = false;
    }
  }

  $effect(() => () => {
    disposed = true;
    if (linux && testActive) void api.setShortcutTest(false).catch(() => {});
  });

  const modeOptions = $derived([
    { value: "toggle", label: t("settings.mode_toggle") },
    ...(!linux || app.capabilities?.shortcut_release ? [{ value: "push_to_talk", label: t("settings.mode_ptt") }] : []),
  ]);

  function selectedMode(): string {
    // Re-read after a failed save, so the list shows the stored mode again.
    void modeBusy;
    return linux && !app.capabilities?.shortcut_release ? "toggle" : app.config?.hotkey.mode ?? "toggle";
  }

  async function setMode(value: string) {
    if (disposed || value === selectedMode()) return;
    const mode: HotkeyMode = value === "push_to_talk" ? "push_to_talk" : "toggle";
    modeBusy = true;
    modeError = null;
    try {
      if (linux) { testConfirmed = false; await stopTest(); }
      await saveConfig((c) => {
        c.hotkey.mode = mode;
      });
    } catch (error) {
      if (!disposed) modeError = errorText(error);
    } finally {
      if (!disposed) modeBusy = false;
    }
  }

  // HotkeyInput shows a rejected save next to the shortcut itself.
  async function setKeys(keys: string[]) {
    if (linux) { testConfirmed = false; await stopTest(); }
    await saveConfig((c) => {
      c.hotkey.keys = keys;
    });
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!canNext || testBusy) return;
    try {
      if (linux) await stopTest();
      if (!disposed) onnext();
    } catch (error) {
      if (!disposed) testError = errorText(error);
    }
  }
</script>

{#if app.hotkey.error}
  <Banner tone="warning">
    <p>{hotkeyErrorText(app.hotkey.error)}</p>
    <p>{t("setup.shortcut.tray")}</p>
  </Banner>
{/if}
{#if app.config}
  <HotkeyInput keys={app.config.hotkey.keys} target="dictation" onchange={setKeys} />
  <Select label={t("settings.mode")} options={modeOptions} disabled={modeBusy}
    info={linux ? t("settings.mode_toggle_hint_linux") : app.config.hotkey.mode === "toggle" ? t("settings.mode_toggle_hint") : undefined}
    bind:value={selectedMode, (value) => void setMode(value)} />
  {#if modeError}<Banner tone="error">{modeError}</Banner>{/if}
{/if}
{#if linux}
  <div class="stack">
    <p>{t("setup.shortcut.verify_body")}</p>
    <p class="muted">{t("setup.shortcut.cancel_linux")}</p>
    {#if !app.capabilities?.shortcut_release}<p class="muted">{t("setup.shortcut.toggle_only")}</p>{/if}
    <div class="row">
      <Button busy={testBusy} disabled={modeBusy} onclick={() => void startTest()}>{t("setup.shortcut.verify_start")}</Button>
    </div>
    {#if testActive && !testConfirmed}<p role="status">{t("setup.shortcut.verify_wait")}</p>{/if}
    {#if testConfirmed}<p role="status">{t("setup.shortcut.verify_done")}</p>{/if}
    {#if testError}<Banner tone="error">{testError}</Banner>{/if}
  </div>
{/if}
<form id={form} onsubmit={submit}></form>
