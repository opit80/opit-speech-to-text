<script lang="ts">
  import { untrack } from "svelte";
  import { app, errorText, saveConfig, t } from "../../../lib/state/app.svelte";
  import Banner from "../../../lib/components/Banner.svelte";
  import Field from "../../../lib/components/Field.svelte";
  import Switch from "../../../lib/components/Switch.svelte";

  interface Props {
    onnext: () => void;
    canNext?: boolean;
    form: string;
  }
  let { onnext, canNext = $bindable(false), form }: Props = $props();

  let configBusy = $state(false);
  let contextText = $state(untrack(() => app.config?.rules.prompt_context ?? ""));
  let contextError = $state<string | null>(null);
  let autostartError = $state<string | null>(null);
  let disposed = false;

  const debugBuild = $derived(app.info?.debug_build ?? false);

  // Every choice here is saved as it is made and has a working default.
  $effect(() => {
    canNext = true;
  });

  $effect(() => () => { disposed = true; });

  async function saveContext() {
    if (configBusy || disposed || contextText === app.config?.rules.prompt_context) return;
    const context = contextText;
    configBusy = true;
    contextError = null;
    try {
      await saveConfig((draft) => {
        draft.rules.prompt_context = context;
      });
    } catch (error) {
      if (!disposed) contextError = errorText(error);
    } finally {
      if (!disposed) configBusy = false;
    }
  }

  function autostartChecked(): boolean {
    void configBusy;
    return app.config?.ui.autostart ?? false;
  }

  async function setAutostart(on: boolean) {
    if (configBusy || disposed) return;
    configBusy = true;
    autostartError = null;
    try {
      await saveConfig((draft) => {
        draft.ui.autostart = on;
      });
    } catch (error) {
      if (!disposed) autostartError = errorText(error);
    } finally {
      if (!disposed) configBusy = false;
    }
  }

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (canNext) onnext();
  }
</script>

<Field label={`${t("rules.context")} (${t("common.optional")})`} info={t("rules.context_hint")} error={contextError}>
  {#snippet children(id, describedBy)}
    <textarea {id} rows="2" bind:value={contextText} disabled={configBusy || !app.config}
      onchange={() => void saveContext()} aria-invalid={!!contextError} aria-describedby={describedBy}></textarea>
  {/snippet}
</Field>

<Switch label={t(app.capabilities?.platform === "linux" ? "settings.autostart_linux" : "settings.autostart")} disabled={debugBuild || configBusy || !app.config}
  info={debugBuild ? t("settings.autostart_debug") : undefined}
  bind:checked={autostartChecked, (on) => void setAutostart(on)} />
{#if autostartError}<Banner tone="error">{autostartError}</Banner>{/if}
<form id={form} onsubmit={submit}></form>

<style>
  textarea { width: 100%; }
</style>
