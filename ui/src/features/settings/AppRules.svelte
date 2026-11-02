<script lang="ts">
  import { onMount, tick } from "svelte";
  import { api } from "../../lib/bridge/api";
  import { app, errorText, loadStyleDefaults, saveConfig, t } from "../../lib/state/app.svelte";
  import {
    deleteRule, exeProblem, foldExe, moveRule, newRule, newRuleId, PASTE_AFTER_KEYS, PASTE_METHOD_KEYS, prepareRule,
    ruleSummary, upsertRule, type ExeProblem, type SummaryPart,
  } from "./app-rules";
  import { styleDisplayName } from "../dictation/styles";
  import { toast } from "../../lib/state/toast.svelte";
  import type { AppRule, PasteAfter, PasteMethod, RunningApp } from "../../lib/bridge/types";
  import Banner from "../../lib/components/Banner.svelte";
  import Button from "../../lib/components/Button.svelte";
  import Dialog from "../../lib/components/Dialog.svelte";
  import Field from "../../lib/components/Field.svelte";
  import Icon from "../../lib/components/Icon.svelte";
  import Select from "../../lib/components/Select.svelte";
  import Switch from "../../lib/components/Switch.svelte";

  const TITLE_CHARS = 60;
  const uid = $props.id();
  const formId = `${uid}-form`;
  // Bumped after a failed change: the switches re-read the live value and drop what the user picked.
  let revision = $state(0);
  let error = $state<string | null>(null);
  let disposed = false;

  // The edit dialog's form draft; null while it is closed.
  let draft = $state<AppRule | null>(null);
  let creating = $state(false);
  let editOpen = $state(false);
  let saving = $state(false);
  let problem = $state<ExeProblem | null>(null);
  let dialogError = $state<string | null>(null);

  // Running apps for the picker. Window titles are private: kept only while the dialog is open.
  let running = $state<RunningApp[]>([]);
  let runningBusy = $state(false);
  let runningError = $state<string | null>(null);
  let runningRequest = 0;

  let deleting = $state<AppRule | null>(null);
  let deleteOpen = $state(false);
  let deleteBusy = $state(false);
  let deleteError = $state<string | null>(null);

  // Fresh objects after every config change and failed change (see Settings.svelte `view`).
  const view = $derived.by(() => {
    void revision;
    const rules = app.config?.app_rules;
    return rules ? { enabled: rules.enabled, rules: rules.rules.map((r) => ({ ...r })) } : null;
  });

  onMount(() => {
    // Only for localized style names; without them stored names are shown.
    loadStyleDefaults().catch(() => {});
    return () => { disposed = true; };
  });

  const styles = $derived(app.config?.cleanup.styles ?? []);
  const profiles = $derived(app.config?.profiles ?? []);

  function styleName(id: string): string {
    const style = styles.find((s) => s.id === id);
    return style ? styleDisplayName(style, app.styleDefaults, app.lang) : id;
  }
  function profileName(id: string): string {
    return profiles.find((p) => p.id === id)?.name ?? id;
  }

  /** Two rules can share an app; the title filter tells their controls apart for screen readers. */
  function ruleName(rule: AppRule): string {
    return rule.title_contains ? t("app_rules.rule_titled", { app: rule.exe, title: rule.title_contains }) : rule.exe;
  }

  function partText(part: SummaryPart): string {
    switch (part.key) {
      case "app_rules.summary.style": return t(part.key, { value: styleName(part.value) });
      case "app_rules.summary.profile": return t(part.key, { value: profileName(part.value) });
      case "app_rules.summary.paste_method": return t(part.key, { value: t(PASTE_METHOD_KEYS[part.value as PasteMethod]) });
      case "app_rules.summary.paste_after": return t(part.key, { value: t(PASTE_AFTER_KEYS[part.value as PasteAfter]) });
      default: return t(part.key, { value: part.value });
    }
  }

  async function save(mutate: (rules: AppRule[]) => AppRule[], enabled?: boolean): Promise<boolean> {
    error = null;
    try {
      await saveConfig((c) => {
        c.app_rules.rules = mutate(c.app_rules.rules);
        if (enabled !== undefined) c.app_rules.enabled = enabled;
      });
      toast(t("common.saved"));
      return true;
    } catch (e) {
      if (!disposed) { error = errorText(e); revision++; }
      return false;
    }
  }

  function setRuleEnabled(id: string, enabled: boolean) {
    void save((rules) => rules.map((r) => (r.id === id ? { ...r, enabled } : r)));
  }

  async function move(id: string, delta: -1 | 1) {
    if (!(await save((rules) => moveRule(rules, id, delta))) || disposed) return;
    await tick();
    // Keep keyboard focus on the moved rule; at the top or bottom the other arrow is the one left.
    const same = document.getElementById(`${uid}-${id}-${delta < 0 ? "up" : "down"}`) as HTMLButtonElement | null;
    const other = document.getElementById(`${uid}-${id}-${delta < 0 ? "down" : "up"}`) as HTMLButtonElement | null;
    (same && !same.disabled ? same : other)?.focus();
  }

  // ---- Edit dialog
  const editTitle = $derived.by(() => {
    if (!draft || creating) return t("app_rules.add_title");
    const saved = app.config?.app_rules.rules.find((r) => r.id === draft?.id);
    return t("app_rules.edit_title", { app: saved?.exe ?? draft.exe });
  });

  function add() {
    draft = newRule(newRuleId(app.config?.app_rules.rules.map((r) => r.id) ?? []));
    creating = true;
    void openDialog();
  }

  function edit(rule: AppRule) {
    draft = { ...rule };
    creating = false;
    void openDialog();
  }

  async function openDialog() {
    problem = null;
    dialogError = null;
    editOpen = true;
    void loadRunning();
    // Start in the file name field rather than on the first info button (its tooltip would cover the form).
    await tick();
    document.getElementById(`${uid}-exe`)?.focus();
  }

  function closeDialog() {
    // Escape while saving would hide the outcome (and a save error): keep the dialog open until it ends.
    if (saving) { editOpen = true; return; }
    draft = null;
    runningRequest++;
    running = [];
    runningBusy = false;
    runningError = null;
  }

  async function loadRunning() {
    const request = ++runningRequest;
    runningBusy = true;
    runningError = null;
    try {
      const list = await api.listRunningApps();
      if (!disposed && request === runningRequest) running = list;
    } catch (e) {
      if (!disposed && request === runningRequest) runningError = errorText(e);
    } finally {
      if (!disposed && request === runningRequest) runningBusy = false;
    }
  }

  function shortTitle(title: string): string {
    const chars = Array.from(title);
    return chars.length > TITLE_CHARS ? `${chars.slice(0, TITLE_CHARS - 1).join("")}…` : title;
  }

  const pickedApp = $derived(draft ? running.find((a) => a.exe === foldExe(draft!.exe.trim())) ?? null : null);
  const runningOptions = $derived([
    { value: "", label: t("app_rules.running_choose"), disabled: true },
    ...running.map((a) => ({
      value: a.exe,
      label: a.title ? t("app_rules.running_option", { exe: a.exe, title: shortTitle(a.title) }) : a.exe,
    })),
  ]);

  function pick(exe: string) {
    if (draft && exe) { draft.exe = exe; problem = null; }
  }

  const global = $derived(app.config);
  const styleOptions = $derived([
    { value: "", label: t("app_rules.keep_global", { value: global ? styleName(global.cleanup.active_style_id) : "" }) },
    ...styles.map((s) => ({ value: s.id, label: styleDisplayName(s, app.styleDefaults, app.lang) })),
  ]);
  const profileOptions = $derived([
    { value: "", label: t("app_rules.keep_global", { value: global ? profileName(global.active_profile_id) : "" }) },
    ...profiles.map((p) => ({ value: p.id, label: p.name })),
  ]);
  const pasteMethodOptions = $derived([
    { value: "", label: t("app_rules.keep_global", { value: global ? t(PASTE_METHOD_KEYS[global.paste.method]) : "" }) },
    ...(Object.keys(PASTE_METHOD_KEYS) as PasteMethod[]).map((value) => ({ value, label: t(PASTE_METHOD_KEYS[value]) })),
  ]);
  const pasteAfterOptions = $derived([
    { value: "", label: t("app_rules.keep_global", { value: global ? t(PASTE_AFTER_KEYS[global.paste.after]) : "" }) },
    ...(Object.keys(PASTE_AFTER_KEYS) as PasteAfter[]).map((value) => ({ value, label: t(PASTE_AFTER_KEYS[value]) })),
  ]);

  async function saveRule() {
    if (!draft || saving) return;
    const rule = prepareRule($state.snapshot(draft) as AppRule);
    problem = exeProblem(rule.exe, app.capabilities?.platform ?? "windows");
    if (problem) {
      await tick();
      document.querySelector<HTMLElement>(`#${formId} [aria-invalid="true"]`)?.focus();
      return;
    }
    saving = true;
    dialogError = null;
    try {
      await saveConfig((c) => { c.app_rules.rules = upsertRule(c.app_rules.rules, rule); });
      if (!disposed) { editOpen = false; toast(t("common.saved")); }
    } catch (e) {
      if (!disposed) dialogError = errorText(e);
    } finally {
      if (!disposed) saving = false;
    }
  }

  // ---- Delete
  function askDelete(rule: AppRule) {
    deleting = rule;
    deleteError = null;
    deleteOpen = true;
  }

  async function confirmDelete() {
    if (!deleting || deleteBusy) return;
    const id = deleting.id;
    deleteBusy = true;
    deleteError = null;
    try {
      await saveConfig((c) => { c.app_rules.rules = deleteRule(c.app_rules.rules, id); });
      if (disposed) return;
      deleteOpen = false;
      toast(t("common.saved"));
      await tick();
      document.getElementById(`${uid}-add`)?.focus();
    } catch (e) {
      if (!disposed) deleteError = errorText(e);
    } finally {
      if (!disposed) deleteBusy = false;
    }
  }
</script>

{#if view}
  {#if !view.enabled}
  <Switch label={t("app_rules.enabled")} info={t("app_rules.enabled_hint")} checked={view.enabled}
    onchange={(v) => void save((rules) => rules, v)} />
  {/if}
  {#if view.rules.length}
    <ul aria-label={t("app_rules.list")}>
      {#each view.rules as rule, index (rule.id)}
        {@const name = ruleName(rule)}
        <li>
          <div class="main">
            <Switch label={rule.exe} ariaLabel={name} checked={rule.enabled} onchange={(v) => setRuleEnabled(rule.id, v)} />
            <span class="meta muted">
              {#each ruleSummary(rule) as part (part.key)}<span>{partText(part)}</span>{/each}
            </span>
          </div>
          <span class="actions">
            <Button id={`${uid}-${rule.id}-up`} variant="ghost" disabled={index === 0}
              aria-label={t("app_rules.move_up_named", { app: name })} title={t("app_rules.move_up_named", { app: name })}
              onclick={() => void move(rule.id, -1)}><Icon name="up" /></Button>
            <Button id={`${uid}-${rule.id}-down`} variant="ghost" disabled={index === view.rules.length - 1}
              aria-label={t("app_rules.move_down_named", { app: name })} title={t("app_rules.move_down_named", { app: name })}
              onclick={() => void move(rule.id, 1)}><Icon name="down" /></Button>
            <Button variant="ghost" aria-label={t("app_rules.edit_named", { app: name })} onclick={() => edit(rule)}>{t("common.edit")}</Button>
            <Button variant="ghost" aria-label={t("app_rules.delete_named", { app: name })} onclick={() => askDelete(rule)}>{t("common.delete")}</Button>
          </span>
        </li>
      {/each}
    </ul>
  {:else}
    <p class="muted">{t("app_rules.empty")}</p>
  {/if}
  {#if error}<Banner tone="error">{error}</Banner>{/if}
  <div class="row"><Button id={`${uid}-add`} onclick={add}>{t("app_rules.add")}</Button></div>
{/if}

<Dialog bind:open={editOpen} title={editTitle} onclose={closeDialog}>
  {#if draft}
    <form id={formId} class="stack" novalidate onsubmit={(e) => { e.preventDefault(); void saveRule(); }}>
      <fieldset class="stack" disabled={saving}>
        <div class="picker">
          <div class="grow">
            <Select label={t("app_rules.running")} info={t("app_rules.running_hint")} options={runningOptions}
              value={pickedApp?.exe ?? ""} disabled={runningBusy} onchange={pick} />
          </div>
          <Button variant="ghost" busy={runningBusy} aria-label={t("settings.refresh")} title={t("settings.refresh")}
            onclick={() => void loadRunning()}><Icon name="retry" /></Button>
        </div>
        {#if runningError}<Banner tone="error">{runningError}</Banner>{/if}
        <Field id={`${uid}-exe`} label={t("app_rules.exe")} info={t("app_rules.exe_hint")}
          error={problem === "required" ? t("app_rules.exe_required") : problem === "invalid" ? t("app_rules.exe_invalid") : null}>
          {#snippet children(id, describedBy)}<input {id} bind:value={draft!.exe} aria-describedby={describedBy}
            aria-invalid={problem !== null} required maxlength="260" autocomplete="off" spellcheck="false" />{/snippet}
        </Field>
        <Field label={t("app_rules.title_contains")} info={t("app_rules.title_contains_hint")}>
          {#snippet children(id, describedBy)}<input {id} bind:value={draft!.title_contains} aria-describedby={describedBy}
            autocomplete="off" spellcheck="false" />{/snippet}
        </Field>
        {#if pickedApp?.title && pickedApp.title !== draft.title_contains}
          <div class="row">
            <Button variant="ghost" onclick={() => { if (draft && pickedApp) draft.title_contains = pickedApp.title; }}>{t("app_rules.use_title")}</Button>
          </div>
        {/if}
        <Select label={t("app_rules.style")} info={t("app_rules.style_hint")} options={styleOptions} value={draft.style_id ?? ""}
          onchange={(v) => { if (draft) draft.style_id = v || null; }} />
        <Select label={t("app_rules.profile")} options={profileOptions} value={draft.profile_id ?? ""}
          onchange={(v) => { if (draft) draft.profile_id = v || null; }} />
        <Select label={t("app_rules.paste_method")} options={pasteMethodOptions} value={draft.paste_method ?? ""}
          onchange={(v) => { if (draft) draft.paste_method = (v || null) as PasteMethod | null; }} />
        <Select label={t("app_rules.paste_after")} options={pasteAfterOptions} value={draft.paste_after ?? ""}
          onchange={(v) => { if (draft) draft.paste_after = (v || null) as PasteAfter | null; }} />
      </fieldset>
      {#if dialogError}<Banner tone="error">{dialogError}</Banner>{/if}
    </form>
  {/if}
  {#snippet actions()}
    <Button disabled={saving} onclick={() => { editOpen = false; }}>{t("common.cancel")}</Button>
    <Button type="submit" form={formId} variant="primary" busy={saving}>{t("common.save")}</Button>
  {/snippet}
</Dialog>

<Dialog bind:open={deleteOpen} title={t("app_rules.delete_title")} onclose={() => { if (deleteBusy) deleteOpen = true; else deleting = null; }}>
  <div class="stack">
    {#if deleting}<p>{t("app_rules.delete_body", { app: ruleName(deleting) })}</p>{/if}
    {#if deleteError}<Banner tone="error">{deleteError}</Banner>{/if}
  </div>
  {#snippet actions()}
    <Button disabled={deleteBusy} onclick={() => { deleteOpen = false; }}>{t("common.cancel")}</Button>
    <Button variant="danger" busy={deleteBusy} onclick={() => void confirmDelete()}>{t("common.delete")}</Button>
  {/snippet}
</Dialog>

<style>
  fieldset { border: 0; margin: 0; padding: 0; min-width: 0; }
  ul { list-style: none; margin: 0; padding: 0; }
  li { display: flex; align-items: center; gap: var(--space-3); padding: var(--space-2) 0; flex-wrap: wrap; }
  li + li { border-top: 1px solid var(--border); }
  .main { display: flex; flex-direction: column; gap: var(--space-1); flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .meta { display: flex; flex-wrap: wrap; column-gap: var(--space-3); font-size: var(--text-sm); padding-left: calc(40px + var(--space-3)); }
  .actions { display: flex; gap: var(--space-1); }
  .picker { display: flex; align-items: flex-end; gap: var(--space-2); }
  .grow { flex: 1; min-width: 0; }
  input { width: 100%; }
</style>
