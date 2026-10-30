<script lang="ts">
  import { untrack } from "svelte";
  import { api } from "../../../lib/bridge/api";
  import { app, errorText, saveConfig, t } from "../../../lib/state/app.svelte";
  import type { PackInfo } from "../../../lib/bridge/types";
  import Banner from "../../../lib/components/Banner.svelte";
  import Button from "../../../lib/components/Button.svelte";
  import Field from "../../../lib/components/Field.svelte";
  import Switch from "../../../lib/components/Switch.svelte";

  interface Props {
    onnext: () => void;
    canNext?: boolean;
    form: string;
  }
  let { onnext, canNext = $bindable(false), form }: Props = $props();

  let packs = $state<PackInfo[]>([]);
  let packsBusy = $state(true);
  let packsError = $state<string | null>(null);
  let packsSaveError = $state<string | null>(null);
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

  async function loadPacks() {
    packsBusy = true;
    packsError = null;
    try {
      const loaded = await api.rulePacks();
      if (!disposed) packs = loaded;
    } catch (error) {
      if (!disposed) packsError = errorText(error);
    } finally {
      if (!disposed) packsBusy = false;
    }
  }

  $effect(() => {
    untrack(() => void loadPacks());
    return () => {
      disposed = true;
    };
  });

  function packChecked(id: string): boolean {
    // Re-read after a failed save, so the checkbox shows the stored state again.
    void configBusy;
    return app.config?.rules.enabled_packs.includes(id) ?? false;
  }

  async function togglePack(id: string, checked: boolean) {
    if (configBusy || disposed) return;
    configBusy = true;
    packsSaveError = null;
    try {
      const order = packs.map((pack) => pack.id);
      await saveConfig((draft) => {
        const enabled = new Set(draft.rules.enabled_packs);
        if (checked) enabled.add(id);
        else enabled.delete(id);
        draft.rules.enabled_packs = order.filter((packId) => enabled.has(packId));
      });
    } catch (error) {
      if (!disposed) packsSaveError = errorText(error);
    } finally {
      if (!disposed) configBusy = false;
    }
  }

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

<fieldset class="packs" aria-busy={packsBusy || configBusy}>
  <legend>{t("rules.packs")}</legend>
  {#if packsError}
    <Banner tone="error">
      {packsError}
      {#snippet action()}<Button busy={packsBusy} onclick={() => void loadPacks()}>{t("common.reload")}</Button>{/snippet}
    </Banner>
  {/if}
  {#if packsBusy}<p class="muted" role="status">{t("common.loading")}</p>{/if}
  {#each packs as pack (pack.id)}
    <Switch label={pack.name} info={t("rules.pack_counts", { terms: pack.terms, corrections: pack.corrections, replacements: pack.replacements })}
      disabled={configBusy || !app.config}
      bind:checked={() => packChecked(pack.id), (checked) => void togglePack(pack.id, checked)} />
  {/each}
  {#if packsSaveError}<Banner tone="error">{packsSaveError}</Banner>{/if}
</fieldset>

<Field label={`${t("rules.context")} (${t("common.optional")})`} info={t("rules.context_hint")} error={contextError}>
  {#snippet children(id, describedBy)}
    <textarea {id} rows="2" bind:value={contextText} disabled={configBusy || !app.config}
      onchange={() => void saveContext()} aria-invalid={!!contextError} aria-describedby={describedBy}></textarea>
  {/snippet}
</Field>

<Switch label={t("settings.autostart")} disabled={debugBuild || configBusy || !app.config}
  info={debugBuild ? t("settings.autostart_debug") : undefined}
  bind:checked={autostartChecked, (on) => void setAutostart(on)} />
{#if autostartError}<Banner tone="error">{autostartError}</Banner>{/if}
<form id={form} onsubmit={submit}></form>

<style>
  .packs { display: flex; flex-direction: column; gap: var(--space-2); margin: 0; padding: 0; border: 0; min-width: 0; }
  legend { font-weight: 600; padding: 0; margin-bottom: var(--space-2); }
  textarea { width: 100%; }
</style>
