<script lang="ts">
  import { untrack } from "svelte";
  import { api } from "../../lib/api";
  import { app, errorText, saveConfig, t } from "../../lib/app.svelte";
  import type { PackInfo } from "../../lib/types";
  import Banner from "../../lib/components/Banner.svelte";
  import Button from "../../lib/components/Button.svelte";
  import Field from "../../lib/components/Field.svelte";
  import Switch from "../../lib/components/Switch.svelte";

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

<p class="muted">{t("setup.rules.body")}</p>

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
    <label class="pack">
      <input type="checkbox" disabled={configBusy || !app.config}
        aria-describedby={`setup-pack-${pack.id}`}
        bind:checked={() => packChecked(pack.id), (checked) => void togglePack(pack.id, checked)} />
      <span class="pack-text">
        <span>{pack.name}</span>
        <span id={`setup-pack-${pack.id}`} class="hint">
          {t("rules.pack_counts", { terms: pack.terms, corrections: pack.corrections, replacements: pack.replacements })}
        </span>
      </span>
    </label>
  {/each}
  {#if packsSaveError}<Banner tone="error">{packsSaveError}</Banner>{/if}
</fieldset>

<Field label={`${t("rules.context")} (${t("common.optional")})`} hint={t("rules.context_hint")} error={contextError}>
  {#snippet children(id)}
    <textarea {id} rows="2" bind:value={contextText} disabled={configBusy || !app.config}
      onchange={() => void saveContext()} aria-invalid={!!contextError} aria-describedby={`${id}-description`}></textarea>
  {/snippet}
</Field>

<Switch label={t("settings.autostart")} disabled={debugBuild || configBusy || !app.config}
  hint={debugBuild ? t("settings.autostart_debug") : undefined}
  bind:checked={autostartChecked, (on) => void setAutostart(on)} />
{#if autostartError}<Banner tone="error">{autostartError}</Banner>{/if}
<form id={form} onsubmit={submit}></form>

<style>
  .packs { display: flex; flex-direction: column; gap: var(--space-2); margin: 0; padding: 0; border: 0; min-width: 0; }
  legend { font-weight: 600; padding: 0; margin-bottom: var(--space-2); }
  .pack { display: flex; align-items: flex-start; gap: var(--space-3); cursor: pointer; }
  .pack input { min-height: 0; margin: 3px 0 0; accent-color: var(--accent); }
  .pack-text { display: flex; flex-direction: column; gap: var(--space-1); }
  .hint { color: var(--text-muted); font-size: var(--text-sm); }
  textarea { width: 100%; }
</style>
