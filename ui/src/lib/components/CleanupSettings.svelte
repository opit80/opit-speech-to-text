<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { api } from "../api";
  import { app, errorText, saveConfig, t } from "../app.svelte";
  import { cleanupPreset, cleanupValid } from "../cleanup";
  import { toast } from "../toast.svelte";
  import type { CleanupConfig, CleanupPreset, CleanupTestResult } from "../types";
  import Banner from "./Banner.svelte";
  import Button from "./Button.svelte";
  import Field from "./Field.svelte";
  import Select from "./Select.svelte";
  import Switch from "./Switch.svelte";

  let { formId = "cleanup-settings", wizard = false, onsaved, canNext = $bindable(false) }: {
    formId?: string; wizard?: boolean; onsaved?: () => void; canNext?: boolean;
  } = $props();
  // A form draft, never an independent live config. Saving uses the shared serialized chain.
  let draft = $state<CleanupConfig>(untrack(() => structuredClone($state.snapshot(app.config!.cleanup))));
  let presets = $state<CleanupPreset[]>([]);
  let prompts = $state<Record<"en" | "tr", string> | null>(null);
  let loaded = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let keyInput = $state("");
  let storedKey = $state(false);
  let result = $state<CleanupTestResult | null>(null);
  let resultFor = $state("");
  let promptLanguage = $state<"en" | "tr">(untrack(() => app.lang));
  let disposed = false;
  let keyRequest = 0;
  const valid = $derived(!!draft && cleanupValid(draft));
  const selected = $derived(draft ? cleanupPreset(draft, presets) : "baseten");
  const options = $derived(presets.map((p) => ({ value: p.id, label: p.id === "baseten" ? t("cleanup.baseten") : p.id === "local" ? t("cleanup.local") : p.id === "custom" ? t("cleanup.custom") : p.name })));
  const keyUrl = $derived(presets.find((p) => p.id === selected)?.key_url ?? "");
  const signature = $derived(JSON.stringify(draft));

  $effect(() => { canNext = loaded && valid && !busy; });
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
      const builtins = await api.cleanupPrompts();
      if (!disposed) { presets = data; prompts = builtins; loaded = true; }
    } catch (e) { if (!disposed) error = errorText(e); }
  }

  function choose(id: string) {
    const preset = presets.find((p) => p.id === id);
    if (!preset || !draft) return;
    // Presets are reactive proxies, which structuredClone rejects; snapshot first.
    draft = { ...structuredClone($state.snapshot(preset.config)), enabled: draft.enabled, prompt_override: draft.prompt_override };
    keyInput = "";
    error = null;
    result = null;
  }

  async function storeKey(config: CleanupConfig) {
    if (config.api_key_ref && keyInput.trim()) {
      await api.setApiKey(config.api_key_ref, keyInput);
      keyInput = "";
      storedKey = true;
    }
  }

  async function save(skip = false) {
    if (!draft || busy || (!skip && !valid)) return;
    busy = true;
    error = null;
    const config = $state.snapshot(draft);
    if (skip) config.enabled = false;
    try {
      if (!skip) await storeKey(config);
      if (disposed) return;
      await saveConfig((c) => { c.cleanup = config; });
      if (!disposed) { draft = config; toast(t("common.saved")); onsaved?.(); }
    } catch (e) { if (!disposed) error = errorText(e); }
    finally { if (!disposed) busy = false; }
  }

  async function test() {
    if (!draft || busy || !valid) return;
    busy = true;
    error = null;
    result = null;
    const config = $state.snapshot(draft);
    try {
      await storeKey(config);
      if (disposed) return;
      const answer = await api.testCleanup(config);
      if (!disposed) { result = answer; resultFor = JSON.stringify(config); }
    } catch (e) { if (!disposed) error = errorText(e); }
    finally { if (!disposed) busy = false; }
  }
</script>

{#if !loaded}
  <p class="muted" role="status">{t("common.loading")}</p>
  {#if error}<Banner tone="error">{error}<Button onclick={() => void load()}>{t("common.reload")}</Button></Banner>{/if}
{/if}
{#if draft && loaded}
  <form id={formId} class="stack" onsubmit={(e) => { e.preventDefault(); void save(); }}>
    <fieldset class="stack" disabled={busy}>
      <Switch label={t("cleanup.enable")} info={t("cleanup.privacy")} bind:checked={draft.enabled} />
      <Select label={t("cleanup.preset")} value={selected} {options} onchange={choose} />
      {#if keyUrl}<p class="muted selectable">{t("cleanup.key_page", { url: keyUrl })}</p>{/if}
      <Field label={t("profiles.base_url")}>
        {#snippet children(id)}<input {id} type="url" bind:value={draft.base_url} autocomplete="off" spellcheck="false" required />{/snippet}
      </Field>
      <Field label={t("profiles.model")}>
        {#snippet children(id)}<input {id} bind:value={draft.model} autocomplete="off" spellcheck="false" required />{/snippet}
      </Field>
      <Switch label={t("profiles.needs_key")} checked={draft.api_key_ref !== null}
        onchange={(on) => { draft.api_key_ref = on ? presets.find((p) => p.id === selected)?.config.api_key_ref ?? "cleanup-custom" : null; keyInput = ""; }} />
      {#if draft.api_key_ref !== null}
        <Field label={t("profiles.key")}>
          {#snippet children(id)}<input {id} type="password" autocomplete="off" spellcheck="false" bind:value={keyInput}
            placeholder={storedKey ? t("profiles.key_stored") : t("profiles.key_paste")} />{/snippet}
        </Field>
      {/if}
      <Field label={t("cleanup.timeout")}>
        {#snippet children(id)}<input {id} type="number" min="1" max="120" step="1" bind:value={draft.timeout_seconds} required />{/snippet}
      </Field>
      <Field label={t("cleanup.reasoning")} info={t("cleanup.reasoning_hint")}>
        {#snippet children(id, describedBy)}<input {id} value={draft.reasoning_effort ?? ""} aria-describedby={describedBy}
          onchange={(e) => { if (draft) draft.reasoning_effort = e.currentTarget.value.trim() || null; }} />{/snippet}
      </Field>
      <details>
        <summary>{t("cleanup.prompt")}</summary>
        <div class="stack prompt-fields">
          <Select label={t("cleanup.prompt_language")} bind:value={promptLanguage}
            options={[{ value: "tr", label: t("cleanup.turkish") }, { value: "en", label: t("cleanup.other_languages") }]} />
          <Field label={t("cleanup.builtin_prompt")} info={t("cleanup.language_hint")}>
            {#snippet children(id, describedBy)}<textarea {id} readonly rows="8" value={prompts?.[promptLanguage] ?? ""} aria-describedby={describedBy}></textarea>{/snippet}
          </Field>
          <Field label={t("cleanup.override")} info={t("cleanup.override_hint")}>
            {#snippet children(id, describedBy)}<textarea {id} rows="5" bind:value={draft.prompt_override} aria-describedby={describedBy}></textarea>{/snippet}
          </Field>
          <Button onclick={() => { if (draft) draft.prompt_override = ""; }}>{t("cleanup.restore")}</Button>
        </div>
      </details>
    </fieldset>
    {#if !valid}<Banner tone="warning">{t("cleanup.invalid")}</Banner>{/if}
    {#if error}<Banner tone="error">{error}</Banner>{/if}
    {#if result && resultFor === signature && !keyInput}
      <div class="stack result" role="status">
        <p><strong>{t("cleanup.before")}</strong></p><p class="selectable">{result.before}</p>
        <p><strong>{t("cleanup.after")}</strong></p><p class="selectable">{result.after}</p>
      </div>
    {/if}
    <div class="row">
      <Button busy={busy} disabled={!valid} onclick={() => void test()}>{t("cleanup.test")}</Button>
      {#if !wizard}<Button type="submit" variant="primary" disabled={!valid || busy}>{t("common.save")}</Button>{/if}
      {#if wizard}<Button variant="ghost" disabled={busy} onclick={() => void save(true)}>{t("cleanup.skip")}</Button>{/if}
    </div>
  </form>
{/if}

<style>
  fieldset { border: 0; margin: 0; padding: 0; min-width: 0; }
  input, textarea { width: 100%; }
  textarea { resize: vertical; }
  .prompt-fields { padding-top: var(--space-3); }
  .result { border: 1px solid var(--border); border-radius: var(--radius); padding: var(--space-3); background: var(--surface-2); }
  .selectable { user-select: text; overflow-wrap: anywhere; white-space: pre-wrap; }
</style>
