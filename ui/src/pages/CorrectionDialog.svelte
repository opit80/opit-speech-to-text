<script lang="ts">
  import { tick, untrack } from "svelte";
  import { api } from "../lib/api";
  import { errorText, t } from "../lib/app.svelte";
  import { toast } from "../lib/toast.svelte";
  import type { CorrectionDraft, Dictation, RulesPreview, RuleWarning } from "../lib/types";
  import Banner from "../lib/components/Banner.svelte";
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import Field from "../lib/components/Field.svelte";

  interface Props { open: boolean; dictation: Dictation | null; variant: string; }
  let { open = $bindable(), dictation, variant }: Props = $props();
  let wrong = $state("");
  let correct = $state("");
  let correctInput = $state<HTMLInputElement>();
  let draft = $state<CorrectionDraft | null>(null);
  let preview = $state<RulesPreview | null>(null);
  let previewBusy = $state(false);
  let previewError = $state<string | null>(null);
  let saveBusy = $state(false);
  let saveError = $state<string | null>(null);
  let saveWarnings = $state<RuleWarning[]>([]);
  let draftWrong = $state("");
  let draftCorrect = $state("");
  let previewRequest = 0;
  let session = 0;

  const canSave = $derived(open && dictation !== null && draft !== null && preview !== null
    && draft.outcome !== "already_present" && draftWrong === wrong && draftCorrect === correct
    && !previewBusy && !previewError && !saveBusy && !saveWarnings.length);
  const afterParts = $derived.by(() => {
    if (!preview) return [];
    const targets = [...new Set(preview.hits.map((hit) => hit.to).filter(Boolean))].sort((a, b) => b.length - a.length);
    const parts: { text: string; changed: boolean }[] = [];
    let offset = 0;
    while (offset < preview.text.length) {
      let start = preview.text.length;
      let target = "";
      for (const candidate of targets) {
        const index = preview.text.indexOf(candidate, offset);
        if (index !== -1 && index < start) { start = index; target = candidate; }
      }
      if (start > offset) parts.push({ text: preview.text.slice(offset, start), changed: false });
      if (!target) break;
      parts.push({ text: target, changed: true });
      offset = start + target.length;
    }
    return parts;
  });

  $effect(() => {
    const isOpen = open;
    const initial = variant;
    const item = dictation;
    const currentSession = ++session;
    let disposed = false;
    untrack(() => {
      wrong = initial;
      correct = "";
      saveBusy = false;
      saveError = null;
      saveWarnings = [];
    });
    if (isOpen && item) {
      void tick().then(() => {
        if (!disposed && currentSession === session) correctInput?.focus();
      });
    }
    return () => { disposed = true; ++session; };
  });

  $effect(() => {
    const isOpen = open;
    const item = dictation;
    const canonical = correct;
    const selected = wrong;
    const request = ++previewRequest;
    let disposed = false;
    draft = null;
    preview = null;
    previewError = null;
    saveError = null;
    saveWarnings = [];
    previewBusy = !!(isOpen && item && canonical.trim() && selected.trim());
    if (!isOpen || !item || !canonical.trim() || !selected.trim()) return;
    const text = item.raw_text;
    async function updatePreview() {
      try {
        const nextDraft = await api.correctionDraft(canonical, selected);
        if (disposed || request !== previewRequest) return;
        const nextPreview = await api.rulesPreview(text, nextDraft.yaml);
        if (disposed || request !== previewRequest) return;
        draft = nextDraft;
        preview = nextPreview;
        draftCorrect = canonical;
        draftWrong = selected;
      } catch (error) {
        if (!disposed && request === previewRequest) previewError = errorText(error);
      } finally {
        if (!disposed && request === previewRequest) previewBusy = false;
      }
    }
    const timer = setTimeout(() => void updatePreview(), 300);
    return () => { disposed = true; clearTimeout(timer); };
  });

  async function save() {
    if (!canSave || !draft) return;
    const currentSession = session;
    saveBusy = true;
    saveError = null;
    try {
      const warnings = await api.saveUserRules(draft.yaml);
      if (!open || currentSession !== session) return;
      toast(t("correction.saved"));
      saveWarnings = warnings;
      if (!warnings.length) open = false;
    } catch (error) {
      if (open && currentSession === session) saveError = errorText(error);
    } finally {
      if (open && currentSession === session) saveBusy = false;
    }
  }
</script>

<Dialog bind:open title={t("correction.title")}>
  <div class="stack">
    <Field label={t("correction.wrong")} hint={t("correction.hint")}>
      {#snippet children(id)}
        <input {id} type="text" bind:value={wrong} disabled={saveBusy} aria-describedby={`${id}-description`} />
      {/snippet}
    </Field>
    <Field label={t("correction.right")} error={previewError}>
      {#snippet children(id)}
        <input {id} type="text" bind:this={correctInput} bind:value={correct} disabled={saveBusy}
          aria-invalid={!!previewError} aria-describedby={previewError ? `${id}-description` : undefined} />
      {/snippet}
    </Field>
    {#if previewBusy}<p class="muted" role="status">{t("common.loading")}</p>{/if}
    {#if draft?.outcome === "added_term"}<Banner tone="info">{t("correction.as_term")}</Banner>
    {:else if draft?.outcome === "already_present"}<Banner tone="info">{t("correction.exists")}</Banner>{/if}
    {#if dictation && preview}
      <div class="comparison stack" aria-live="polite">
        <div><h3>{t("correction.before")}</h3><p>{dictation.text}</p></div>
        <div><h3>{t("correction.after")}</h3><p>{#each afterParts as part, index (index)}{#if part.changed}<strong>{part.text}</strong>{:else}{part.text}{/if}{/each}</p></div>
      </div>
      {#if preview.warnings.length}
        <Banner tone="warning"><p>{t("correction.warnings")}</p><ul>{#each preview.warnings as warning, index (index)}<li>{warning.message}</li>{/each}</ul></Banner>
      {/if}
    {/if}
    {#if saveError}<Banner tone="error">{saveError}</Banner>{/if}
    {#if saveWarnings.length}
      <Banner tone="warning"><p>{t("correction.warnings")}</p><ul>{#each saveWarnings as warning, index (index)}<li>{warning.message}</li>{/each}</ul></Banner>
    {/if}
  </div>
  {#snippet actions()}
    <Button disabled={saveBusy} onclick={() => open = false}>{t("common.cancel")}</Button>
    <Button variant="primary" busy={saveBusy} disabled={!canSave} onclick={() => void save()}>{t("common.save")}</Button>
  {/snippet}
</Dialog>

<style>
  input { width: 100%; }
  .comparison { padding: var(--space-3); background: var(--surface-2); border-radius: var(--radius-sm); }
  h3 { font-size: var(--text-md); margin-bottom: var(--space-1); }
  .comparison p { white-space: pre-wrap; overflow-wrap: anywhere; }
  ul { padding-left: var(--space-5); margin: var(--space-2) 0 0; }
</style>
