<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { api } from "../../lib/bridge/api";
  import { app, errorText, saveConfig, t } from "../../lib/state/app.svelte";
  import { cleanupEnabledAfterTest, cleanupSaveNeedsTest, sameCleanupConnection, cleanupPreset, cleanupShowsConnection, cleanupValid, withCleanupPreset, withLiveStyles } from "./cleanup";
  import { toast } from "../../lib/state/toast.svelte";
  import type { CleanupConfig, CleanupPreset, CleanupTestResult } from "../../lib/bridge/types";
  import Banner from "../../lib/components/Banner.svelte";
  import Button from "../../lib/components/Button.svelte";
  import Field from "../../lib/components/Field.svelte";
  import Select from "../../lib/components/Select.svelte";
  import Switch from "../../lib/components/Switch.svelte";

  let { onkeysaved }: {
    /** Called after a key is stored, including through the test action. */
    onkeysaved?: () => void;
  } = $props();
  // A form draft, never an independent live config. Saving uses the shared serialized chain.
  let draft = $state<CleanupConfig>(untrack(() => structuredClone($state.snapshot(app.config!.cleanup))));
  let presets = $state<CleanupPreset[]>([]);
  let loaded = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let keyInput = $state("");
  let storedKey = $state(false);
  let result = $state<CleanupTestResult | null>(null);
  let resultFor = $state("");
  let disposed = false;
  let keyRequest = 0;
  const valid = $derived(!!draft && cleanupValid(draft));
  const selected = $derived(draft ? cleanupPreset(draft, presets) : "baseten");
  const options = $derived(presets.map((p) => ({ value: p.id, label: p.id === "baseten" ? t("cleanup.baseten") : p.id === "local" ? t("cleanup.local") : p.id === "custom" ? t("cleanup.custom") : p.name })));
  const showConnection = $derived(cleanupShowsConnection(draft, presets));
  const keyUrl = $derived(presets.find((p) => p.id === selected)?.key_url ?? "");
  // The test uses the active style, so switching it hides an earlier result too.
  const signature = $derived(JSON.stringify([draft, app.config?.cleanup.active_style_id]));

  $effect(() => {
    const ref = draft?.api_key_ref;
    const request = ++keyRequest;
    storedKey = false;
    if (ref) api.hasApiKey(ref).then((has) => { if (!disposed && request === keyRequest) storedKey = has; })
      .catch((e: unknown) => { if (!disposed && request === keyRequest) error = errorText(e); });
  });

  onMount(() => {
    void load();
    return () => { disposed = true; keyInput = ""; };
  });

  async function load() {
    error = null;
    try {
      const data = await api.cleanupPresets();
      if (!disposed) { presets = data; loaded = true; }
    } catch (e) { if (!disposed) error = errorText(e); }
  }

  function choose(id: string) {
    const preset = presets.find((p) => p.id === id);
    if (!preset || !draft) return;
    draft = withCleanupPreset(draft, preset.config);
    keyInput = "";
    error = null;
    result = null;
  }

  async function storeKey(config: CleanupConfig) {
    if (config.api_key_ref && keyInput.trim()) {
      await api.setApiKey(config.api_key_ref, keyInput);
      keyInput = "";
      storedKey = true;
      onkeysaved?.();
      return true;
    }
    return false;
  }

  async function save() { await verify(true); }
  async function test() { await verify(false); }

  async function verify(saveSettings: boolean) {
    if (!draft || busy || !valid || !app.config) return;
    busy = true;
    error = null;
    result = null;
    const config = $state.snapshot(draft);
    const wasEnabled = app.config.cleanup.enabled;
    try {
      const keySaved = await storeKey(config);
      if (disposed) return;
      // The Test action also stores a newly entered key. Persist its connection so any
      // successful auto-enable applies to the connection that was actually tested.
      const persist = saveSettings || keySaved;
      const credentialSaved = persist && cleanupSaveNeedsTest(config, keySaved, wasEnabled);
      if (persist) {
        const saved = await saveConfig((c) => { c.cleanup = withLiveStyles(config, c.cleanup); });
        if (!disposed) draft = structuredClone(saved.cleanup);
      }
      if (disposed || !app.config) return;
      if (!saveSettings || credentialSaved) {
        const tested = withLiveStyles(config, $state.snapshot(app.config.cleanup));
        const outcome = tested.active_style_id === "raw" ? "skipped" : "passed";
        // Raw mode returns a local no-op result; it provides no connection proof.
        const answer = outcome === "skipped" && saveSettings ? null : await api.testCleanup(tested);
        if (cleanupEnabledAfterTest(false, credentialSaved, outcome)) {
          const saved = await saveConfig((c) => {
            if (sameCleanupConnection(c.cleanup, tested)) {
              c.cleanup.enabled = cleanupEnabledAfterTest(c.cleanup.enabled, credentialSaved, outcome);
            }
          });
          if (!disposed) draft = structuredClone(saved.cleanup);
        }
        if (!disposed && answer && sameCleanupConnection(draft, tested)) {
          result = answer;
          resultFor = JSON.stringify([$state.snapshot(draft), tested.active_style_id]);
        }
      }
      if (!disposed && persist) toast(t("common.saved"));
    } catch (e) { if (!disposed) error = errorText(e); }
    finally { if (!disposed) busy = false; }
  }
</script>

{#if !loaded}
  <p class="muted" role="status">{t("common.loading")}</p>
  {#if error}<Banner tone="error">{error}<Button onclick={() => void load()}>{t("common.reload")}</Button></Banner>{/if}
{/if}
{#if draft && loaded}
  <form class="stack" onsubmit={(e) => { e.preventDefault(); void save(); }}>
    <fieldset class="stack" disabled={busy}>
      <Switch label={t("cleanup.enable")} info={t("cleanup.privacy")} bind:checked={draft.enabled} />
      <Select label={t("cleanup.preset")} value={selected} {options} onchange={choose} />
      {#if keyUrl}<p class="muted selectable">{t("cleanup.key_page", { url: keyUrl })}</p>{/if}
      {#if showConnection}
        <Field label={t("profiles.base_url")}>
          {#snippet children(id)}<input {id} type="url" bind:value={draft.base_url} autocomplete="off" spellcheck="false" required />{/snippet}
        </Field>
        <Field label={t("profiles.model")}>
          {#snippet children(id)}<input {id} bind:value={draft.model} autocomplete="off" spellcheck="false" required />{/snippet}
        </Field>
        <Switch label={t("profiles.needs_key")} checked={draft.api_key_ref !== null}
          onchange={(on) => { draft.api_key_ref = on ? presets.find((p) => p.id === selected)?.config.api_key_ref ?? "cleanup-custom" : null; keyInput = ""; }} />
      {/if}
      {#if draft.api_key_ref !== null}
        <Field label={t("profiles.key")}>
          {#snippet children(id)}<input {id} type="password" autocomplete="off" spellcheck="false" bind:value={keyInput}
            placeholder={storedKey ? t("profiles.key_stored") : t("profiles.key_paste")} />{/snippet}
        </Field>
      {/if}
      {#if showConnection || draft.reasoning_effort}
        <Field label={t("cleanup.timeout")}>
          {#snippet children(id)}<input {id} type="number" min="1" max="120" step="1" bind:value={draft.timeout_seconds} required />{/snippet}
        </Field>
        <Field label={t("cleanup.reasoning")} info={t("cleanup.reasoning_hint")}>
          {#snippet children(id, describedBy)}<input {id} value={draft.reasoning_effort ?? ""} aria-describedby={describedBy}
            onchange={(e) => { if (draft) draft.reasoning_effort = e.currentTarget.value.trim() || null; }} />{/snippet}
        </Field>
      {/if}
    </fieldset>
    {#if !valid}<Banner tone="warning">{t("cleanup.invalid")}</Banner>{/if}
    {#if error}<Banner tone="error">{error}</Banner>{/if}
    {#if result && resultFor === signature && !keyInput}
      <div class="stack result" role="status">
        <p><strong>{t("cleanup.before")}</strong></p><p class="selectable">{result.before}</p>
        <p><strong>{t("cleanup.after")}</strong></p><p class="selectable">{result.after}</p>
      </div>
    {/if}
    <p class="muted">{t("cleanup.save_test_hint")}</p>
    <div class="row">
      <Button busy={busy} disabled={!valid} onclick={() => void test()}>{t("cleanup.test")}</Button>
      <Button type="submit" variant="primary" disabled={!valid || busy}>{t("common.save")}</Button>
    </div>
  </form>
{/if}

<style>
  fieldset { border: 0; margin: 0; padding: 0; min-width: 0; }
  input { width: 100%; }
  .result { border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space-3); background: var(--surface-2); }
  .selectable { user-select: text; overflow-wrap: anywhere; white-space: pre-wrap; }
</style>
