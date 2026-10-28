<script lang="ts">
  import { tick, untrack } from "svelte";
  import { api } from "../lib/api";
  import { errorText, t } from "../lib/app.svelte";
  import { initialChoices, splitChoices, type LearningChoice } from "../lib/learning";
  import { toast } from "../lib/toast.svelte";
  import type { RuleWarning, TermChange } from "../lib/types";
  import Banner from "../lib/components/Banner.svelte";
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import InfoTip from "../lib/components/InfoTip.svelte";
  import Select from "../lib/components/Select.svelte";

  // One offer per saved History edit. Every pair ends up applied or dismissed, so it is offered once.
  interface Props {
    suggestions: TermChange[] | null;
    /** After the offer was applied or dismissed and the dialog closed (e.g. to restore focus). */
    ondone?: () => void;
  }
  let { suggestions = $bindable(), ondone }: Props = $props();

  let open = $state(false);
  let choices = $state<LearningChoice[]>([]);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let warnings = $state<RuleWarning[]>([]);
  // The chosen items are saved; the choices are locked and a retry only repeats the dismissal.
  let applied = $state(false);
  // Everything is persisted; closing no longer dismisses anything.
  let settled = $state(false);
  let disposed = false;
  let list = $state<HTMLUListElement>();

  const choiceOptions = $derived([
    { value: "rule", label: t("learning.rule") },
    { value: "term", label: t("learning.term") },
    { value: "skip", label: t("learning.skip") },
  ]);

  $effect(() => {
    const offer = suggestions;
    untrack(() => {
      choices = offer ? initialChoices(offer) : [];
      error = null;
      warnings = [];
      applied = false;
      settled = false;
      busy = false;
      open = !!offer?.length;
    });
    // Start on the first choice rather than the info button, whose tooltip would cover the list.
    // (After showModal, which runs in the Dialog effect and moves focus itself.)
    if (offer?.length) {
      void tick().then(() => setTimeout(() => {
        if (!disposed && suggestions === offer) list?.querySelector("select")?.focus();
      }));
    }
  });

  $effect(() => () => {
    disposed = true;
    // Leaving the page with the offer still open counts as "Not now" (best effort).
    const offer = untrack(() => suggestions);
    if (!offer || untrack(() => settled)) return;
    const pending = untrack(() => applied) ? splitChoices(offer, untrack(() => choices)).dismiss : offer;
    if (pending.length) api.learningDismiss(pending).catch(() => {});
  });

  function finish(offer: TermChange[]) {
    if (suggestions !== offer) return;
    suggestions = null;
    open = false;
    ondone?.();
  }

  async function addSelected() {
    const offer = suggestions;
    if (!offer || busy || settled) return;
    const { apply, dismiss } = splitChoices(offer, choices);
    busy = true;
    error = null;
    try {
      if (apply.length && !applied) {
        const result = await api.learningApply(apply);
        if (disposed) return;
        warnings = result;
        applied = true;
      }
      if (dismiss.length) await api.learningDismiss(dismiss);
      if (disposed || suggestions !== offer) return;
      settled = true;
      if (apply.length) toast(t("learning.added"));
      if (!warnings.length) finish(offer);
    } catch (e) {
      if (!disposed && suggestions === offer) { error = errorText(e); open = true; }
    } finally {
      if (!disposed && suggestions === offer) busy = false;
    }
  }

  async function notNow() {
    const offer = suggestions;
    if (!offer || busy) return;
    const pending = applied ? splitChoices(offer, choices).dismiss : offer;
    if (settled || !pending.length) { finish(offer); return; }
    busy = true;
    error = null;
    try {
      await api.learningDismiss(pending);
      if (disposed || suggestions !== offer) return;
      settled = true;
      finish(offer);
    } catch (e) {
      // Keep the choices for a retry instead of claiming they were remembered.
      if (!disposed && suggestions === offer) { error = errorText(e); open = true; }
    } finally {
      if (!disposed && suggestions === offer) busy = false;
    }
  }

  // Escape (or any close) without a choice is "Not now"; a failure reopens the dialog with the error.
  function closed() {
    if (!suggestions) return;
    if (busy) { open = true; return; }
    void notNow();
  }
</script>

<Dialog bind:open title={t("learning.title")} onclose={closed}>
  <div class="stack">
    <p><InfoTip text={t("learning.hint")}>{t("learning.intro")}</InfoTip></p>
    <ul class="stack" bind:this={list}>
      {#each suggestions ?? [] as pair, index (index)}
        <li>
          <Select label={t("learning.pair", { heard: pair.heard, corrected: pair.corrected })} options={choiceOptions}
            value={choices[index] ?? "skip"} disabled={busy || applied}
            onchange={(value) => { choices[index] = value as LearningChoice; }} />
        </li>
      {/each}
    </ul>
    {#if error}<Banner tone="error">{error}</Banner>{/if}
    {#if warnings.length}
      <Banner tone="warning"><p>{t("correction.warnings")}</p><ul class="warnings">{#each warnings as warning, index (index)}<li>{warning.message}</li>{/each}</ul></Banner>
    {/if}
  </div>
  {#snippet actions()}
    {#if settled}
      <Button variant="primary" onclick={() => { if (suggestions) finish(suggestions); }}>{t("common.close")}</Button>
    {:else}
      <Button disabled={busy} onclick={() => void notNow()}>{t("learning.not_now")}</Button>
      <Button variant="primary" {busy} onclick={() => void addSelected()}>{t("learning.apply")}</Button>
    {/if}
  {/snippet}
</Dialog>

<style>
  ul { list-style: none; margin: 0; padding: 0; }
  li { overflow-wrap: anywhere; }
  .warnings { list-style: disc; padding-left: var(--space-5); margin-top: var(--space-2); }
</style>
