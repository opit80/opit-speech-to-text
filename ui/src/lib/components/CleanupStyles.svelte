<script lang="ts">
  import { onMount, tick } from "svelte";
  import { app, errorText, loadStyleDefaults, saveConfig, setActiveStyle, t } from "../app.svelte";
  import { formatCombo } from "../hotkey";
  import {
    defaultStylePrompt, deleteStyle, isBuiltinStyle, newStyle, newStyleId, prepareStyle, styleDisplayName, styleProblem,
    upsertStyle, type StyleProblem,
  } from "../styles";
  import { toast } from "../toast.svelte";
  import type { AppConfig, CleanupStyle } from "../types";
  import Banner from "./Banner.svelte";
  import Button from "./Button.svelte";
  import Dialog from "./Dialog.svelte";
  import Field from "./Field.svelte";
  import HotkeyInput from "./HotkeyInput.svelte";
  import InfoTip from "./InfoTip.svelte";
  import Switch from "./Switch.svelte";

  const uid = $props.id();
  const formId = `${uid}-form`;
  let list = $state<HTMLElement>();
  let switching = $state(false);
  let error = $state<string | null>(null);
  let defaultsError = $state<string | null>(null);

  // The edit dialog's form draft; null while it is closed (which also ends any shortcut capture).
  let draft = $state<CleanupStyle | null>(null);
  let creating = $state(false);
  let editOpen = $state(false);
  let saving = $state(false);
  let problem = $state<StyleProblem | null>(null);
  let dialogError = $state<string | null>(null);

  let deleting = $state<CleanupStyle | null>(null);
  let deleteOpen = $state(false);
  let deleteBusy = $state(false);
  let deleteError = $state<string | null>(null);
  let disposed = false;

  const styles = $derived(app.config?.cleanup.styles ?? []);
  const active = $derived(app.config?.cleanup.active_style_id ?? "clean");
  const nameOf = (style: Pick<CleanupStyle, "id" | "name">) => styleDisplayName(style, app.styleDefaults, app.lang);
  const draftDefaultPrompt = $derived(draft ? defaultStylePrompt(draft.id, app.styleDefaults, app.lang) : "");
  const draftBuiltin = $derived(!!draft && isBuiltinStyle(draft.id));
  const editTitle = $derived.by(() => {
    if (!draft || creating) return t("styles.add_title");
    const saved = styles.find((s) => s.id === draft?.id);
    return t("styles.edit_title", { name: saved ? nameOf(saved) : draft.name });
  });
  // The saved settings with this draft in place, so the shortcut check sees a new style too.
  const validationConfig = $derived.by(() => {
    if (!app.config || !draft) return undefined;
    const config = $state.snapshot(app.config) as AppConfig;
    config.cleanup.styles = upsertStyle(config.cleanup.styles, $state.snapshot(draft));
    return config;
  });

  onMount(() => {
    void loadDefaults();
    return () => { disposed = true; };
  });

  async function loadDefaults() {
    defaultsError = null;
    try { await loadStyleDefaults(); }
    catch (e) { if (!disposed) defaultsError = errorText(e); }
  }

  /** After a failed switch, show the live selection again. */
  function syncRadios() {
    for (const input of list?.querySelectorAll<HTMLInputElement>("input[type=radio]") ?? []) {
      input.checked = input.value === app.config?.cleanup.active_style_id;
    }
  }

  async function select(id: string) {
    if (switching || id === active) return;
    switching = true;
    error = null;
    try {
      await setActiveStyle(id);
    } catch (e) {
      if (!disposed) { error = errorText(e); syncRadios(); }
    } finally {
      if (!disposed) switching = false;
    }
  }

  function edit(style: CleanupStyle) {
    draft = { ...$state.snapshot(style), name: nameOf(style), hotkey: [...style.hotkey] };
    creating = false;
    openDialog();
  }

  function add() {
    draft = newStyle(newStyleId(styles.map((s) => s.id)));
    creating = true;
    openDialog();
  }

  function openDialog() {
    problem = null;
    dialogError = null;
    editOpen = true;
  }

  async function saveStyle() {
    if (!draft || saving) return;
    const style = prepareStyle($state.snapshot(draft), app.styleDefaults, app.lang);
    problem = styleProblem(style);
    if (problem) {
      await tick();
      document.querySelector<HTMLElement>(`#${formId} [aria-invalid="true"]`)?.focus();
      return;
    }
    saving = true;
    dialogError = null;
    try {
      await saveConfig((c) => { c.cleanup.styles = upsertStyle(c.cleanup.styles, style); });
      if (!disposed) { editOpen = false; toast(t("common.saved")); }
    } catch (e) {
      if (!disposed) dialogError = errorText(e);
    } finally {
      if (!disposed) saving = false;
    }
  }

  function askDelete(style: CleanupStyle) {
    deleting = style;
    deleteError = null;
    deleteOpen = true;
  }

  async function confirmDelete() {
    if (!deleting || deleteBusy) return;
    const id = deleting.id;
    deleteBusy = true;
    deleteError = null;
    try {
      await saveConfig((c) => { Object.assign(c.cleanup, deleteStyle(c.cleanup, id)); });
      if (!disposed) { deleteOpen = false; toast(t("common.saved")); }
    } catch (e) {
      if (!disposed) deleteError = errorText(e);
    } finally {
      if (!disposed) deleteBusy = false;
    }
  }

  const fallbackName = $derived(nameOf(styles.find((s) => s.id === "clean") ?? { id: "clean", name: "Clean" }));
</script>

<section class="stack" aria-labelledby={`${uid}-title`}>
  <h3 id={`${uid}-title`}><InfoTip text={t("styles.hint")}>{t("styles.title")}</InfoTip></h3>
  {#if defaultsError}
    <Banner tone="error">{defaultsError}
      {#snippet action()}<Button onclick={() => void loadDefaults()}>{t("common.reload")}</Button>{/snippet}
    </Banner>
  {/if}
  <fieldset class="styles" aria-busy={switching}>
    <legend class="visually-hidden">{t("styles.active")}</legend>
    <ul bind:this={list}>
      {#each styles as style (style.id)}
        {@const name = nameOf(style)}
        <li>
          <label class="choice">
            <input type="radio" name={`${uid}-active`} value={style.id} checked={style.id === active} disabled={switching}
              onchange={() => void select(style.id)} />
            <span class="name">{name}</span>
          </label>
          <span class="meta muted">
            {#if style.hotkey.length}<span>{formatCombo(style.hotkey, app.lang)}</span>{/if}
            {#if !style.in_cycle}<span>{t("styles.not_in_cycle")}</span>{/if}
          </span>
          <span class="actions">
            <Button variant="ghost" aria-label={t("styles.edit_named", { name })} onclick={() => edit(style)}>{t("common.edit")}</Button>
            {#if !style.builtin}
              <Button variant="ghost" aria-label={t("styles.delete_named", { name })} onclick={() => askDelete(style)}>{t("common.delete")}</Button>
            {/if}
          </span>
        </li>
      {/each}
    </ul>
  </fieldset>
  {#if error}<Banner tone="error">{error}</Banner>{/if}
  <div class="row"><Button onclick={add}>{t("styles.add")}</Button></div>
</section>

<Dialog bind:open={editOpen} title={editTitle} onclose={() => { draft = null; }}>
  {#if draft}
    <form id={formId} class="stack" novalidate onsubmit={(e) => { e.preventDefault(); void saveStyle(); }}>
      <fieldset class="stack" disabled={saving}>
        <Field label={t("styles.name")} error={problem === "name" ? t("styles.name_required") : null}>
          {#snippet children(id, describedBy)}<input {id} bind:value={draft!.name} aria-describedby={describedBy}
            aria-invalid={problem === "name"} required={!draftBuiltin} maxlength="80" autocomplete="off" />{/snippet}
        </Field>
        {#if draft.id !== "raw"}
          <Field label={t("styles.prompt")} info={draftBuiltin ? t("styles.prompt_hint") : t("styles.prompt_hint_custom")}
            error={problem === "prompt" ? t("styles.prompt_required") : null}>
            {#snippet children(id, describedBy)}<textarea {id} rows="7" bind:value={draft!.prompt} aria-describedby={describedBy}
              aria-invalid={problem === "prompt"} required={!draftBuiltin}
              placeholder={draftBuiltin ? draftDefaultPrompt : undefined}></textarea>{/snippet}
          </Field>
          {#if draftDefaultPrompt}
            <div class="row">
              {#if draft.prompt.trim()}
                {#if draftBuiltin}<Button variant="ghost" onclick={() => { if (draft) draft.prompt = ""; }}>{t("cleanup.restore")}</Button>{/if}
              {:else}
                <Button variant="ghost" onclick={() => { if (draft) draft.prompt = draftDefaultPrompt; }}>{t("styles.use_default")}</Button>
              {/if}
            </div>
          {/if}
          <Field label={t("styles.model")} info={t("styles.model_hint")}>
            {#snippet children(id, describedBy)}<input {id} value={draft!.model ?? ""} aria-describedby={describedBy}
              placeholder={app.config?.cleanup.model ?? ""} autocomplete="off" spellcheck="false"
              oninput={(e) => { if (draft) draft.model = e.currentTarget.value; }} />{/snippet}
          </Field>
        {/if}
        <Switch label={t("styles.in_cycle")} bind:checked={draft.in_cycle} />
        <HotkeyInput keys={draft.hotkey} optional target={`style:${draft.id}`} config={validationConfig}
          label={t("styles.shortcut")} info={t("styles.shortcut_hint")}
          onchange={(keys) => { if (draft) draft.hotkey = keys; }} />
      </fieldset>
      {#if dialogError}<Banner tone="error">{dialogError}</Banner>{/if}
    </form>
  {/if}
  {#snippet actions()}
    <Button disabled={saving} onclick={() => { editOpen = false; }}>{t("common.cancel")}</Button>
    <Button type="submit" form={formId} variant="primary" busy={saving}>{t("common.save")}</Button>
  {/snippet}
</Dialog>

<Dialog bind:open={deleteOpen} title={t("styles.delete_title")} onclose={() => { deleting = null; }}>
  <div class="stack">
    {#if deleting}<p>{t("styles.delete_body", { name: nameOf(deleting), fallback: fallbackName })}</p>{/if}
    {#if deleteError}<Banner tone="error">{deleteError}</Banner>{/if}
  </div>
  {#snippet actions()}
    <Button disabled={deleteBusy} onclick={() => { deleteOpen = false; }}>{t("common.cancel")}</Button>
    <Button variant="danger" busy={deleteBusy} onclick={() => void confirmDelete()}>{t("common.delete")}</Button>
  {/snippet}
</Dialog>

<style>
  section { border-top: 1px solid var(--border); padding-top: var(--space-4); }
  h3 { font-size: var(--text-md); font-weight: 600; }
  fieldset { border: 0; margin: 0; padding: 0; min-width: 0; }
  ul { list-style: none; margin: 0; padding: 0; }
  li { display: flex; align-items: center; gap: var(--space-3); padding: var(--space-1) 0; flex-wrap: wrap; }
  li + li { border-top: 1px solid var(--border); }
  .choice { display: flex; align-items: center; gap: var(--space-2); flex: 1; min-width: 0; cursor: pointer; padding: var(--space-1) 0; }
  .choice input { margin: 0; min-height: 0; accent-color: var(--accent); }
  .name { overflow-wrap: anywhere; }
  .meta { display: flex; gap: var(--space-2); font-size: var(--text-sm); }
  .actions { display: flex; gap: var(--space-1); }
  input:not([type="radio"]), textarea { width: 100%; }
</style>
