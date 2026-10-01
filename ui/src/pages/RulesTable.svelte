<script lang="ts">
  import { t } from "../lib/app.svelte";
  import type { Replacement, RulePack } from "../lib/types";
  import Button from "../lib/components/Button.svelte";
  import Field from "../lib/components/Field.svelte";

  interface Props { pack: RulePack; onchange: (pack: RulePack) => void; }
  let { pack, onchange }: Props = $props();
  let nextId = 0;
  let terms = $state<{ id: number; text: string }[]>([]);
  let corrections = $state<{ id: number; canonical: string; variants: string }[]>([]);
  let replacements = $state<(Replacement & { id: number })[]>([]);
  let hallucinations = $state<{ id: number; text: string }[]>([]);

  // Keep blank rows locally; the pack sent to Rust contains only completed rules.
  $effect(() => {
    terms = (pack.terms ?? []).map((text) => ({ id: nextId++, text }));
    corrections = Object.entries(pack.corrections ?? {}).map(([canonical, variants]) => ({
      id: nextId++, canonical, variants: variants.join(", "),
    }));
    replacements = (pack.replacements ?? []).map((row) => ({ ...row, id: nextId++ }));
    hallucinations = (pack.hallucinations ?? []).map((text) => ({ id: nextId++, text }));
  });

  function changed() {
    const next: RulePack = { schema: pack.schema, id: pack.id, name: pack.name, language: pack.language };
    const cleanTerms = terms.map((row) => row.text.trim()).filter(Boolean);
    const cleanPhrases = hallucinations.map((row) => row.text.trim()).filter(Boolean);
    const cleanCorrections = new Map<string, string[]>();
    for (const row of corrections) {
      const canonical = row.canonical.trim();
      const variants = row.variants.split(",").map((text) => text.trim()).filter(Boolean);
      if (canonical && variants.length) {
        cleanCorrections.set(canonical, [...(cleanCorrections.get(canonical) ?? []), ...variants]);
      }
    }
    const cleanReplacements = replacements.filter((row) => row.from.trim()).map((row) => ({
      from: row.from, to: row.to, case_sensitive: !!row.case_sensitive, regex: !!row.regex,
    }));
    if (cleanTerms.length) next.terms = cleanTerms;
    if (cleanCorrections.size) next.corrections = Object.fromEntries(cleanCorrections);
    if (cleanReplacements.length) next.replacements = cleanReplacements;
    if (cleanPhrases.length) next.hallucinations = cleanPhrases;
    onchange(next);
  }

  function commitOnEnter(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.isComposing) {
      event.preventDefault();
      (event.currentTarget as HTMLInputElement).blur();
    }
  }

  function move(index: number, direction: -1 | 1) {
    const destination = index + direction;
    if (destination < 0 || destination >= replacements.length) return;
    [replacements[index], replacements[destination]] = [replacements[destination], replacements[index]];
    changed();
  }
</script>

<div class="table-sections">
  <section class="stack" aria-label={t("rules.terms")}>
    <h3>{t("rules.terms")}</h3>
    {#each terms as row (row.id)}
      <div class="edit-row">
        <Field label={t("rules.terms")}>
          {#snippet children(id)}<input {id} type="text" bind:value={row.text} onchange={changed} onkeydown={commitOnEnter} />{/snippet}
        </Field>
        <Button variant="ghost" onclick={() => { terms = terms.filter((item) => item.id !== row.id); changed(); }}>{t("common.remove")}</Button>
      </div>
    {/each}
    <div><Button onclick={() => { terms.push({ id: nextId++, text: "" }); changed(); }}>{t("rules.add_term")}</Button></div>
  </section>

  <section class="stack" aria-label={t("rules.corrections")}>
    <h3>{t("rules.corrections")}</h3>
    {#each corrections as row (row.id)}
      <div class="edit-row">
        <Field label={t("rules.correct")}>
          {#snippet children(id)}<input {id} type="text" bind:value={row.canonical} onchange={changed} onkeydown={commitOnEnter} />{/snippet}
        </Field>
        <Field label={t("rules.variants")}>
          {#snippet children(id)}<input {id} type="text" bind:value={row.variants} onchange={changed} onkeydown={commitOnEnter} />{/snippet}
        </Field>
        <Button variant="ghost" onclick={() => { corrections = corrections.filter((item) => item.id !== row.id); changed(); }}>{t("common.remove")}</Button>
      </div>
    {/each}
    <div><Button onclick={() => { corrections.push({ id: nextId++, canonical: "", variants: "" }); changed(); }}>{t("rules.add_correction")}</Button></div>
  </section>

  <section class="stack" aria-label={t("rules.replacements")}>
    <h3>{t("rules.replacements")}</h3>
    {#each replacements as row, index (row.id)}
      <div class="replacement-row stack">
        <div class="edit-row">
          <Field label={t("rules.from")}>
            {#snippet children(id)}<input {id} type="text" bind:value={row.from} onchange={changed} onkeydown={commitOnEnter} />{/snippet}
          </Field>
          <Field label={t("rules.to")}>
            {#snippet children(id)}<input {id} type="text" bind:value={row.to} onchange={changed} onkeydown={commitOnEnter} />{/snippet}
          </Field>
        </div>
        <div class="row replacement-options">
          <label><input type="checkbox" bind:checked={row.case_sensitive} onchange={changed} />{t("rules.case_sensitive")}</label>
          <label><input type="checkbox" bind:checked={row.regex} onchange={changed} />{t("rules.regex")}</label>
          <div class="row order-actions">
            <Button variant="ghost" disabled={index === 0} onclick={() => move(index, -1)}>{t("rules.move_up")}</Button>
            <Button variant="ghost" disabled={index === replacements.length - 1} onclick={() => move(index, 1)}>{t("rules.move_down")}</Button>
            <Button variant="ghost" onclick={() => { replacements = replacements.filter((item) => item.id !== row.id); changed(); }}>{t("common.remove")}</Button>
          </div>
        </div>
      </div>
    {/each}
    <div><Button onclick={() => { replacements.push({ id: nextId++, from: "", to: "", case_sensitive: false, regex: false }); changed(); }}>{t("rules.add_replacement")}</Button></div>
  </section>

  <section class="stack" aria-label={t("rules.hallucinations")}>
    <h3>{t("rules.hallucinations")}</h3>
    <p class="muted hint">{t("rules.hallucinations_hint")}</p>
    {#each hallucinations as row (row.id)}
      <div class="edit-row">
        <Field label={t("rules.hallucinations")}>
          {#snippet children(id)}<input {id} type="text" bind:value={row.text} onchange={changed} onkeydown={commitOnEnter} />{/snippet}
        </Field>
        <Button variant="ghost" onclick={() => { hallucinations = hallucinations.filter((item) => item.id !== row.id); changed(); }}>{t("common.remove")}</Button>
      </div>
    {/each}
    <div><Button onclick={() => { hallucinations.push({ id: nextId++, text: "" }); changed(); }}>{t("rules.add_phrase")}</Button></div>
  </section>
</div>

<style>
  .table-sections { display: flex; flex-direction: column; gap: var(--space-5); }
  section + section { border-top: 1px solid var(--border); padding-top: var(--space-4); }
  h3 { font-size: var(--text-md); }
  .hint { font-size: var(--text-sm); }
  .edit-row { display: flex; align-items: flex-end; gap: var(--space-2); flex-wrap: wrap; }
  .edit-row :global(.field) { flex: 1 1 160px; min-width: 0; }
  input[type="text"] { width: 100%; }
  label { display: inline-flex; align-items: center; gap: var(--space-2); }
  input[type="checkbox"] { min-height: 0; margin: 0; accent-color: var(--accent); }
  .replacement-row + .replacement-row { border-top: 1px solid var(--border); padding-top: var(--space-3); }
  .replacement-options { gap: var(--space-3); }
  .order-actions { margin-left: auto; }
</style>
