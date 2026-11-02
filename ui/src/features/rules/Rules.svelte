<script lang="ts">
  import { tick, untrack } from "svelte";
  import { api } from "../../lib/bridge/api";
  import { app, errorText, saveConfig, t } from "../../lib/state/app.svelte";
  import { onRulesChanged } from "../../lib/bridge/events";
  import type { MessageKey } from "../../lib/i18n";
  import { budgetLevel, hasInlineComments } from "./rules";
  import { createSerialQueue } from "../../lib/utils/serial";
  import { toast } from "../../lib/state/toast.svelte";
  import { PROMPT_TOKEN_BUDGET, type BuiltPrompt, type RuleKind, type RulePack, type RulesPreview, type RuleWarning } from "../../lib/bridge/types";
  import Banner from "../../lib/components/Banner.svelte";
  import Button from "../../lib/components/Button.svelte";
  import Field from "../../lib/components/Field.svelte";
  import PageHeader from "../../lib/components/PageHeader.svelte";
  import Switch from "../../lib/components/Switch.svelte";
  import RulesTable from "./RulesTable.svelte";

  const kindKeys: Record<RuleKind, MessageKey> = {
    correction: "rules.kind.correction", replacement: "rules.kind.replacement", casing: "rules.kind.casing", numbers: "rules.kind.numbers",
  };
  const enqueueRender = createSerialQueue();
  let disposed = false;
  let budgetRequest = 0;
  let renderPending = 0;
  let sourceContext = "";
  let configBusy = $state(false);
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
  // Personal rules were saved elsewhere while this editor had unsaved changes.
  let changedElsewhere = $state(false);
  // A rules-changed event that arrived while the editor was busy; handled once it is idle.
  let rulesChangePending = false;
  let syncRequest = 0;

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
      if (!disposed) { editorBusy = false; settleRulesChange(); }
    }
  }

  /**
   * Compares the saved rules with what this editor last loaded or saved. New rules replace a clean
   * editor (keeping the tab); with an unsaved draft only a notice is shown unless `discardDraft`.
   * The check itself does not lock the editor, so typing is never interrupted.
   */
  async function syncEditor(discardDraft = false) {
    if (disposed) return;
    const request = ++syncRequest;
    try {
      const loaded = await api.getUserRules();
      if (disposed || request !== syncRequest) return;
      if (editorBusy || saveBusy || renderBusy) { rulesChangePending = true; return; }
      if (!discardDraft && loaded === savedText) return;
      if (!discardDraft && dirty) { changedElsewhere = true; return; }
      await adoptRules(loaded);
    } catch (error) {
      if (!disposed && request === syncRequest) editorError = errorText(error);
    }
  }

  async function adoptRules(loaded: string) {
    editorBusy = true;
    editorError = null;
    try {
      yamlText = loaded;
      savedText = loaded;
      editorLoaded = true;
      warnings = [];
      changedElsewhere = false;
      if (tab === "table") {
        const parsed = await api.parseUserRules(loaded);
        if (!disposed) tablePack = parsed;
      }
    } catch (error) {
      if (!disposed) { tab = "yaml"; editorError = errorText(error); }
    } finally {
      if (!disposed) { editorBusy = false; settleRulesChange(); }
    }
  }

  // Saved rules changed (this page, a History correction or a learning confirmation): refresh what
  // depends on them, but never drop an unsaved draft.
  function rulesChanged() {
    if (disposed) return;
    previewRevision++;
    void refreshBudget();
    if (editorBusy || saveBusy || renderBusy) { rulesChangePending = true; return; }
    if (editorLoaded) void syncEditor();
  }

  function settleRulesChange() {
    if (!rulesChangePending || editorBusy || saveBusy || renderBusy) return;
    rulesChangePending = false;
    rulesChanged();
  }

  $effect(() => {
    untrack(() => { void loadEditor(); });
    let stopRules: (() => void) | undefined;
    onRulesChanged(rulesChanged).then((stop) => {
      if (disposed) stop();
      else stopRules = stop;
    }).catch((error: unknown) => {
      if (!disposed) editorError = errorText(error);
    });
    return () => { disposed = true; stopRules?.(); };
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
        settleRulesChange();
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
      if (!disposed) { renderBusy = renderPending > 0; settleRulesChange(); }
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
      changedElsewhere = false;
      previewRevision++;
      toast(t("rules.saved"));
      await refreshBudget();
    } catch (error) {
      if (!disposed) editorError = errorText(error);
    } finally {
      if (!disposed) { saveBusy = false; settleRulesChange(); }
    }
  }

  async function revert() {
    if (editorLocked || renderBusy || disposed) return;
    // The saved copy here is stale: discard the draft by reading the current rules.
    if (changedElsewhere) { await syncEditor(true); return; }
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
      if (!disposed) { editorBusy = false; settleRulesChange(); }
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
  <PageHeader title={t("rules.title")} />
  <div class="rules-sections">
    <section class="card stack">
      <Switch label={t("rules.numbers")} info={t("rules.numbers_hint")} checked={numbersView.checked}
        disabled={configBusy || !app.config} onchange={(checked) => void setNumbers(checked)} />
      {#if numbersError}<Banner tone="error">{numbersError}</Banner>{/if}
    </section>
    <details class="card advanced" open>
      <summary>{t("rules.personal")}</summary>
      <div class="stack advanced-body">
      {#if changedElsewhere}
        <Banner tone="warning">{t("rules.changed_elsewhere")}
          {#snippet action()}<Button disabled={editorLocked || renderBusy} onclick={() => void syncEditor(true)}>{t("common.reload")}</Button>{/snippet}
        </Banner>
      {:else if dirty}<Banner tone="warning">{t("rules.unsaved")}</Banner>{/if}
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
          {#snippet children(id, describedBy)}
            <textarea {id} class="mono" rows="18" spellcheck="false" bind:value={yamlText} disabled={editorLocked}
              oninput={() => { editorError = null; warnings = []; }} aria-invalid={!!editorError}
              aria-describedby={describedBy}></textarea>
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

    <details class="card advanced" open={!!previewError}>
      <summary>{t("rules.try")}</summary>
      <div class="stack advanced-body">
      <Field label={t("rules.try_input")} error={previewError}>
        {#snippet children(id, describedBy)}
          <textarea {id} rows="3" bind:value={tryText} aria-invalid={!!previewError}
            aria-describedby={describedBy}></textarea>
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
      </div>
    </details>

    <details class="card advanced" open={!!contextError || !!budgetError}>
      <summary>{t("rules.advanced_prompt")}</summary>
      <div class="stack advanced-body">
      <Field label={t("rules.context")} info={t("rules.context_hint")} error={contextError}>
        {#snippet children(id, describedBy)}
          <textarea {id} rows="2" bind:value={contextText} disabled={configBusy || !app.config} onchange={() => void saveContext()}
            aria-invalid={!!contextError} aria-describedby={describedBy}></textarea>
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

  </div>
</div>

<style>
  summary { cursor: pointer; font-weight: 600; }
  .advanced-body { margin-top: var(--space-4); }
  .rules-sections { display: flex; flex-direction: column; gap: var(--space-4); }
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
