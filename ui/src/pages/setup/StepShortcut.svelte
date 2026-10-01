<script lang="ts">
  import { app, errorText, saveConfig, t } from "../../lib/app.svelte";
  import type { HotkeyMode } from "../../lib/types";
  import Banner from "../../lib/components/Banner.svelte";
  import HotkeyInput from "../../lib/components/HotkeyInput.svelte";
  import Select from "../../lib/components/Select.svelte";

  interface Props {
    onnext: () => void;
    canNext?: boolean;
    form: string;
  }
  let { onnext, canNext = $bindable(false), form }: Props = $props();

  let modeBusy = $state(false);
  let modeError = $state<string | null>(null);
  let disposed = false;

  // The default shortcut works for most people, so this step never blocks.
  $effect(() => {
    canNext = true;
  });

  $effect(() => () => {
    disposed = true;
  });

  const modeOptions = $derived([
    { value: "toggle", label: t("settings.mode_toggle") },
    { value: "push_to_talk", label: t("settings.mode_ptt") },
  ]);

  function selectedMode(): string {
    // Re-read after a failed save, so the list shows the stored mode again.
    void modeBusy;
    return app.config?.hotkey.mode ?? "toggle";
  }

  async function setMode(value: string) {
    if (disposed || value === selectedMode()) return;
    const mode: HotkeyMode = value === "push_to_talk" ? "push_to_talk" : "toggle";
    modeBusy = true;
    modeError = null;
    try {
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
    await saveConfig((c) => {
      c.hotkey.keys = keys;
    });
  }

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (canNext) onnext();
  }
</script>

<p class="muted">{t("setup.shortcut.body")}</p>
{#if app.hotkey.error}
  <Banner tone="warning">
    <p>{app.hotkey.error}</p>
    <p>{t("setup.shortcut.tray")}</p>
  </Banner>
{/if}
{#if app.config}
  <HotkeyInput keys={app.config.hotkey.keys} onchange={setKeys} />
  <Select label={t("settings.mode")} options={modeOptions} disabled={modeBusy}
    hint={t(app.config.hotkey.mode === "toggle" ? "settings.mode_toggle_hint" : "settings.mode_ptt_hint")}
    bind:value={selectedMode, (value) => void setMode(value)} />
  {#if modeError}<Banner tone="error">{modeError}</Banner>{/if}
{/if}
<form id={form} onsubmit={submit}></form>
