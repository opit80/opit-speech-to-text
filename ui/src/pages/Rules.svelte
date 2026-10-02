<script lang="ts">
  import { tick, untrack } from "svelte";
  import { api } from "../lib/api";
  import { app, errorText, saveConfig, t } from "../lib/app.svelte";
  import type { MessageKey } from "../lib/i18n";
  import { budgetLevel, hasInlineComments } from "../lib/rules";
  import { createSerialQueue } from "../lib/serial";
  import { toast } from "../lib/toast.svelte";
  import { PROMPT_TOKEN_BUDGET, type BuiltPrompt, type PackInfo, type RuleKind, type RulePack, type RulesPreview, type RuleWarning } from "../lib/types";
  import Banner from "../lib/components/Banner.svelte";
  import Button from "../lib/components/Button.svelte";
  import Field from "../lib/components/Field.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import Switch from "../lib/components/Switch.svelte";
  import RulesTable from "./RulesTable.svelte";

  const kindKeys: Record<RuleKind, MessageKey> = {
    correction: "rules.kind.correction", replacement: "rules.kind.replacement", casing: "rules.kind.casing", numbers: "rules.kind.numbers",
  };
  const enqueueRender = createSerialQueue();
  let disposed = false;
  let budgetRequest = 0;
  let renderPending = 0;
  let sourceContext = "";
  let packs = $state<PackInfo[]>([]);
  let packsBusy = $state(true);
  let packsError = $state<string | null>(null);
  let configBusy = $state(false);
  let packsSaveError = $state<string | null>(null);
  let numbersError = $state<string | null>(null);
  let numbersRevision = $state(0);
  let contextText = $state("");
  let contextError = $state<string | null>(null);
  let budget = $state<BuiltPrompt | null>(null);
  let budgetBusy = $state(false);
  let budgetError = $state<string | null>(null);
  let yamlText = $state("");
  let savedText = $state("");
  let editorLoaded = $state(false);
  let editorBusy = $state(true);
  let editorError = $state<string | null>(null);
  let renderBusy = $state(false);
  let saveBusy = $state(false);
  let warnings = $state<RuleWarning[]>([]);
  let tab = $state<"table" | "yaml">("yaml");
  let tablePack = $state<RulePack | null>(null);
  let tableTab = $state<HTMLButtonElement>();
  let yamlTab = $state<HTMLButtonElement>();
  let tryText = $state("");
  let preview = $state<RulesPreview | null>(null);
  let previewBusy = $state(false);
  let previewError = $state<string | null>(null);
  let previewRevision = $state(0);

  const dirty = $derived(yamlText !== savedText);
  const editorLocked = $derived(!editorLoaded || editorBusy || saveBusy);
  const numbersView = $derived.by(() => { void numbersRevision; return { checked: app.config?.rules.numbers_as_words ?? false }; });

  async function setNumbers(checked: boolean) {
    if (configBusy || disposed) return;
    configBusy = true;
    numbersError = null;
    try {
      await saveConfig((draft) => { draft.rules.numbers_as_words = checked; });
      if (!disposed) toast(t("common.saved"));
    } catch (error) {
      if (!disposed) { numbersError = errorText(error); numbersRevision++; }
    } finally { if (!disposed) configBusy = false; }
  }

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

  async function loadEditor() {
    editorBusy = true;
    editorError = null;
    try {
      const loaded = await api.getUserRules();
      if (disposed) return;
      yamlText = loaded;
      savedText = loaded;
      editorLoaded = true;
      // Broken saved YAML remains editable in the raw view.
      const parsed = await api.parseUserRules(loaded);
      if (!disposed) { tablePack = parsed; tab = "table"; }
    } catch (error) {
      if (!disposed) { tab = "yaml"; editorError = errorText(error); }
    } finally {
      if (!disposed) editorBusy = false;
    }
  }

  $effect(() => {
    untrack(() => { void loadPacks(); void loadEditor(); });
    return () => { disposed = true; };
  });

  async function refreshBudget() {
    if (disposed) return;
    const request = ++budgetRequest;
    budgetBusy = true;
    budgetError = null;
    try {
      const next = await api.promptBudget();
      if (!disposed && request === budgetRequest) budget = next;
    } catch (error) {
      if (!disposed && request === budgetRequest) { budget = null; budgetError = errorText(error); }
    } finally {
      if (!disposed && request === budgetRequest) budgetBusy = false;
    }
  }

  // Refresh on every adopted config, including changes from other app surfaces.
  $effect(() => {
    const config = app.config;
    const context = config?.rules.prompt_context ?? "";
    untrack(() => {
      if (contextText === sourceContext) contextText = context;
      sourceContext = context;
      if (config) void refreshBudget();
    });
  });

  function packChecked(id: string) {
    // Reconcile a Switch's local state after a failed save.
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
        if (checked) enabled.add(id); else enabled.delete(id);
        draft.rules.enabled_packs = order.filter((packId) => enabled.has(packId));
      });
      if (!disposed) toast(t("common.saved"));
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
      await saveConfig((draft) => { draft.rules.prompt_context = context; });
      if (!disposed) toast(t("common.saved"));
    } catch (error) {
      if (!disposed) contextError = errorText(error);
    } finally {
      if (!disposed) configBusy = false;
    }
  }

  async function switchTab(next: "table" | "yaml", focus = false) {
    if (editorLocked || renderBusy || disposed) return;
    editorBusy = true;
    editorError = null;
    try {
      if (next === "table") {
        const parsed = await api.parseUserRules(yamlText);
        if (disposed) return;
        tablePack = parsed;
      }
      tab = next;
    } catch (error) {
      if (!disposed) { tab = "yaml"; editorError = errorText(error); }
    } finally {
      if (!disposed) {
        editorBusy = false;
        if (focus) {
          await tick();
          if (!disposed) (tab === "table" ? tableTab : yamlTab)?.focus();
        }
      }
    }
  }

  function tabKey(event: KeyboardEvent) {
    let next: "table" | "yaml";
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") next = tab === "table" ? "yaml" : "table";
    else if (event.key === "Home") next = "table";
    else if (event.key === "End") next = "yaml";
    else return;
    event.preventDefault();
    void switchTab(next, true);
  }

  async function editTable(pack: RulePack) {
    if (editorLocked || disposed) return;
    renderPending++;
    renderBusy = true;
    warnings = [];
    try {
      await enqueueRender(async () => {
        if (disposed) return;
        editorError = null;
        const rendered = await api.renderUserRules(pack, yamlText);
        if (!disposed) yamlText = rendered;
      });
    } catch (error) {
      if (!disposed) editorError = errorText(error);
    } finally {
      renderPending--;
      if (!disposed) renderBusy = renderPending > 0;
    }
  }

  async function saveRules() {
    if (!dirty || editorLocked || renderBusy || disposed) return;
    const submitted = yamlText;
    saveBusy = true;
    editorError = null;
    warnings = [];
    try {
      const skipped = await api.saveUserRules(submitted);
      if (disposed) return;
      warnings = skipped;
      savedText = submitted;
      previewRevision++;
      toast(t("rules.saved"));
      await refreshBudget();
    } catch (error) {
      if (!disposed) editorError = errorText(error);
    } finally {
      if (!disposed) saveBusy = false;
    }
  }

  async function revert() {
    if (editorLocked || renderBusy || disposed) return;
    editorBusy = true;
    editorError = null;
    try {
      // Always restore the raw source, even when the saved YAML is broken.
      yamlText = savedText;
      warnings = [];
      if (tab === "table") {
        const parsed = await api.parseUserRules(savedText);
        if (!disposed) tablePack = parsed;
      }
    } catch (error) {
      if (!disposed) { tab = "yaml"; editorError = errorText(error); }
    } finally {
      if (!disposed) editorBusy = false;
    }
  }

  $effect(() => {
    const text = tryText;
    const draft = dirty ? yamlText : null;
    const config = app.config;
    void previewRevision;
    let cancelled = false;
    preview = null;
    previewError = null;
    previewBusy = !!text.trim() && !!config;
    if (!text.trim() || !config) return;
    async function updatePreview() {
      try {
        const result = await api.rulesPreview(text, draft);
        if (!disposed && !cancelled) preview = result;
      } catch (error) {
        if (!disposed && !cancelled) previewError = errorText(error);
      } finally {
        if (!disposed && !cancelled) previewBusy = false;
      }
    }
    const timer = setTimeout(() => void updatePreview(), 300);
    return () => { cancelled = true; clearTimeout(timer); };
  });
</script>

<div>
  <PageHeader title={t("rules.title")} description={t("rules.description")} />
  <div class="rules-sections">
    <section class="card stack">
      <Switch label={t("rules.numbers")} hint={t("rules.numbers_hint")} checked={numbersView.checked}
        disabled={configBusy || !app.config} onchange={(checked) => void setNumbers(checked)} />
      {#if numbersError}<Banner tone="error">{numbersError}</Banner>{/if}
    </section>
    <section class="card stack" aria-labelledby="rules-packs" aria-busy={packsBusy || configBusy}>
      <h2 id="rules-packs">{t("rules.packs")}</h2>
      <div class="personal-row"><p>{t("rules.personal")}</p><p class="muted hint">{t("rules.personal_hint")}</p></div>
      {#if packsError}
        <Banner tone="error">{packsError}{#snippet action()}<Button busy={packsBusy} onclick={() => void loadPacks()}>{t("common.reload")}</Button>{/snippet}</Banner>
      {/if}
      {#if packsBusy}<p class="muted" role="status">{t("common.loading")}</p>{/if}
      {#each packs as pack (pack.id)}
        <Switch label={pack.name} hint={t("rules.pack_counts", { terms: pack.terms, corrections: pack.corrections, replacements: pack.replacements })}
          bind:checked={() => packChecked(pack.id), (checked) => void togglePack(pack.id, checked)}
          disabled={packsBusy || configBusy || !app.config} />
      {/each}
      {#if packsSaveError}<Banner tone="error">{packsSaveError}</Banner>{/if}
    </section>

    <section class="card stack" aria-labelledby="rules-try">
      <h2 id="rules-try">{t("rules.try")}</h2>
      <Field label={t("rules.try_input")} error={previewError}>
        {#snippet children(id)}
          <textarea {id} rows="3" bind:value={tryText} aria-invalid={!!previewError}
            aria-describedby={previewError ? `${id}-description` : undefined}></textarea>
        {/snippet}
      </Field>
      <div class="stack" aria-live="polite" aria-busy={previewBusy}>
        {#if previewBusy}<p class="muted" role="status">{t("common.loading")}</p>{/if}
        {#if preview}
          <h3>{t("rules.try_result")}</h3><p class="result-text">{preview.text}</p>
          {#if preview.hallucination}<Banner tone="warning">{t("rules.try_hallucination")}</Banner>{/if}
          {#if preview.hits.length}
            <h3>{t("rules.try_hits")}</h3>
            <ul>{#each preview.hits as hit, index (index)}<li><code>{hit.from}</code> → <code>{hit.to}</code> <span class="muted">({hit.rule.pack_id} · {t(kindKeys[hit.rule.kind])})</span></li>{/each}</ul>
          {/if}
          {#if preview.warnings.length}
            <Banner tone="warning"><p>{t("rules.skipped")}</p><ul>{#each preview.warnings as warning, index (index)}<li>{warning.pack_id}: {warning.message}</li>{/each}</ul></Banner>
          {/if}
        {/if}
      </div>
    </section>

    <details class="card advanced" open={!!contextError || !!budgetError}>
      <summary>{t("rules.advanced_prompt")}</summary>
      <div class="stack advanced-body">
      <Field label={t("rules.context")} hint={t("rules.context_hint")} error={contextError}>
        {#snippet children(id)}
          <textarea {id} rows="2" bind:value={contextText} disabled={configBusy || !app.config} onchange={() => void saveContext()}
            aria-invalid={!!contextError} aria-describedby={`${id}-description`}></textarea>
        {/snippet}
      </Field>
      <div class="stack" aria-busy={budgetBusy}>
        {#if budgetBusy}<p class="muted" role="status">{t("common.loading")}</p>{/if}
        {#if budgetError}
          <Banner tone="error">{budgetError}{#snippet action()}<Button busy={budgetBusy} onclick={() => void refreshBudget()}>{t("common.retry")}</Button>{/snippet}</Banner>
        {/if}
        {#if budget}
          <label class="hint" for="rules-budget">{t("rules.budget", { used: budget.estimated_tokens, max: PROMPT_TOKEN_BUDGET })}</label>
          <progress id="rules-budget" class={budgetLevel(budget.estimated_tokens)} max={PROMPT_TOKEN_BUDGET}
            value={Math.min(budget.estimated_tokens, PROMPT_TOKEN_BUDGET)}
            aria-valuetext={t("rules.budget", { used: budget.estimated_tokens, max: PROMPT_TOKEN_BUDGET })}></progress>
          <p class="prompt-text mono muted">{budget.prompt ?? t("rules.no_prompt")}</p>
          {#if budget.dropped_terms.length}<Banner tone="warning">{t("rules.dropped", { terms: budget.dropped_terms.join(", ") })}</Banner>{/if}
        {/if}
      </div>
    </div>
    </details>

    <details class="card advanced" open={!!editorError || dirty}>
      <summary>{t("rules.advanced_editor")}</summary>
      <div class="stack advanced-body">
      {#if dirty}<Banner tone="warning">{t("rules.unsaved")}</Banner>{/if}
      <div class="tabs" role="tablist" aria-label={t("rules.editor")}>
        <button type="button" id="rules-tab-table" role="tab" bind:this={tableTab} aria-selected={tab === "table"}
          aria-controls="rules-panel-table" tabindex={tab === "table" ? 0 : -1} disabled={editorLocked || renderBusy}
          onclick={() => void switchTab("table")} onkeydown={tabKey}>{t("rules.tab_table")}</button>
        <button type="button" id="rules-tab-yaml" role="tab" bind:this={yamlTab} aria-selected={tab === "yaml"}
          aria-controls="rules-panel-yaml" tabindex={tab === "yaml" ? 0 : -1} disabled={editorLocked || renderBusy}
          onclick={() => void switchTab("yaml")} onkeydown={tabKey}>{t("rules.tab_yaml")}</button>
      </div>
      {#if editorBusy}<p class="muted" role="status">{t("common.loading")}</p>{/if}
      <div id="rules-panel-table" role="tabpanel" aria-labelledby="rules-tab-table" hidden={tab !== "table"}>
        {#if tab === "table" && tablePack}
          <div class="stack">
            {#if hasInlineComments(yamlText)}<Banner tone="info">{t("rules.inline_comments")}</Banner>{/if}
            <fieldset disabled={editorLocked}><legend class="visually-hidden">{t("rules.editor")}</legend><RulesTable pack={tablePack} onchange={(pack) => void editTable(pack)} /></fieldset>
            {#if editorError}<Banner tone="error">{editorError}</Banner>{/if}
          </div>
        {/if}
      </div>
      <div id="rules-panel-yaml" role="tabpanel" aria-labelledby="rules-tab-yaml" hidden={tab !== "yaml"}>
        <Field label={t("rules.tab_yaml")} error={editorError}>
          {#snippet children(id)}
            <textarea {id} class="mono" rows="18" spellcheck="false" bind:value={yamlText} disabled={editorLocked}
              oninput={() => { editorError = null; warnings = []; }} aria-invalid={!!editorError}
              aria-describedby={editorError ? `${id}-description` : undefined}></textarea>
          {/snippet}
        </Field>
      </div>
      {#if !editorLoaded && !editorBusy}<div><Button onclick={() => void loadEditor()}>{t("common.reload")}</Button></div>{/if}
      {#if warnings.length}
        <Banner tone="warning"><p>{t("rules.skipped")}</p><ul>{#each warnings as warning, index (index)}<li>{warning.pack_id}: {warning.message}</li>{/each}</ul></Banner>
      {/if}
      <div class="row editor-actions">
        {#if dirty}<Button variant="ghost" disabled={editorLocked || renderBusy} onclick={() => void revert()}>{t("rules.revert")}</Button>{/if}
        <Button variant="primary" busy={saveBusy} disabled={!dirty || editorLocked || renderBusy} onclick={() => void saveRules()}>{t("common.save")}</Button>
      </div>
    </div>
    </details>


  </div>
</div>

<style>
  summary { cursor: pointer; font-weight: 600; }
  .advanced-body { margin-top: var(--space-4); }
  .rules-sections { display: flex; flex-direction: column; gap: var(--space-4); }
  .personal-row { border-bottom: 1px solid var(--border); padding-bottom: var(--space-3); }
  .hint { font-size: var(--text-sm); }
  textarea { width: 100%; }
  progress { width: 100%; height: var(--space-2); border: 0; border-radius: var(--radius-sm); overflow: hidden; }
  progress::-webkit-progress-bar { background: var(--surface-2); }
  progress::-webkit-progress-value { background: var(--success); }
  progress.warning::-webkit-progress-value { background: var(--warning); }
  progress.full::-webkit-progress-value { background: var(--danger); }
  .prompt-text { background: var(--surface-2); padding: var(--space-3); border-radius: var(--radius-sm); }
  .prompt-text, .result-text, li { white-space: pre-wrap; overflow-wrap: anywhere; }
  .tabs { display: flex; gap: var(--space-1); border-bottom: 1px solid var(--border); padding-bottom: var(--space-2); }
  .tabs button { border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface); padding: var(--space-2) var(--space-4); cursor: pointer; }
  .tabs button:hover { background: var(--surface-2); }
  .tabs button[aria-selected="true"] { background: var(--accent); border-color: var(--accent); color: var(--accent-text); }
  .tabs button:disabled { opacity: 0.6; cursor: default; }
  fieldset { border: 0; padding: 0; margin: 0; min-width: 0; }
  .editor-actions { justify-content: flex-end; }
  h3 { font-size: var(--text-md); }
  ul { padding-left: var(--space-5); margin: 0; }
</style>
