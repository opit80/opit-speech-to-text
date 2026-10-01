<script lang="ts">
  import { tick, untrack } from "svelte";
  import { api } from "../lib/api";
  import { app, errorText, saveConfig, t } from "../lib/app.svelte";
  import type { MessageKey } from "../lib/i18n";
  import { uniqueProfileId } from "../lib/profiles";
  import { toast } from "../lib/toast.svelte";
  import type { Profile } from "../lib/types";
  import Banner from "../lib/components/Banner.svelte";
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import ProfileForm, { KeySaveError } from "./ProfileForm.svelte";

  interface Editing {
    profile: Profile;
    isNew: boolean;
    session: number;
  }

  const presetKeys: Record<string, MessageKey> = { groq: "profiles.preset.groq", openai: "profiles.preset.openai" };
  let presets = $state<Profile[]>([]);
  let presetsBusy = $state(true);
  let presetsError = $state<string | null>(null);
  let keyState = $state<Record<string, boolean>>({});
  let keysError = $state<string | null>(null);
  let keysRevision = $state(0);
  let editing = $state<Editing | null>(null);
  let menuOpen = $state(false);
  let menu = $state<HTMLElement>();
  let activeBusy = $state<string | null>(null);
  let listError = $state<string | null>(null);
  let deleteTarget = $state<Profile | null>(null);
  let deleteOpen = $state(false);
  let deleteBusy = $state(false);
  let deleteError = $state<string | null>(null);
  let sessions = 0;
  let disposed = false;

  const profiles = $derived(app.config?.profiles ?? []);
  const activeId = $derived(app.config?.active_profile_id ?? "");

  async function loadPresets() {
    presetsBusy = true;
    presetsError = null;
    try {
      const loaded = await api.profilePresets();
      if (!disposed) presets = loaded;
    } catch (error) {
      if (!disposed) presetsError = errorText(error);
    } finally {
      if (!disposed) presetsBusy = false;
    }
  }

  $effect(() => {
    untrack(() => void loadPresets());
    return () => { disposed = true; };
  });

  // "No key" badges: re-checked on every adopted config and after the form closes.
  $effect(() => {
    void keysRevision;
    const refs = [...new Set(profiles.flatMap((p) => (p.api_key_ref ? [p.api_key_ref] : [])))];
    let stale = false;
    async function check() {
      try {
        const entries = await Promise.all(refs.map(async (ref) => [ref, await api.hasApiKey(ref)] as const));
        if (!stale && !disposed) { keyState = Object.fromEntries(entries); keysError = null; }
      } catch (error) {
        if (!stale && !disposed) keysError = errorText(error);
      }
    }
    void check();
    return () => { stale = true; };
  });

  function isPreset(id: string): boolean {
    return Object.hasOwn(presetKeys, id);
  }

  function presetLabel(preset: Profile): string {
    return t(isPreset(preset.id) ? presetKeys[preset.id] : "profiles.preset.custom");
  }

  function closeMenu(focusToggle: boolean) {
    menuOpen = false;
    if (focusToggle) void tick().then(() => menu?.querySelector<HTMLButtonElement>("button")?.focus());
  }

  function windowClick(event: MouseEvent) {
    if (menuOpen && !(event.target instanceof Node && menu?.contains(event.target))) closeMenu(false);
  }

  function windowKey(event: KeyboardEvent) {
    if (menuOpen && event.key === "Escape") closeMenu(true);
  }

  function startAdd(preset: Profile) {
    menuOpen = false;
    const ids = profiles.map((p) => p.id);
    const draft: Profile = $state.snapshot(preset);
    if (!isPreset(draft.id)) {
      // Custom server: the id and key name are derived from the name when it is saved.
      Object.assign(draft, { id: "", name: "", base_url: "", model: "", api_key_ref: "" });
    } else if (ids.includes(draft.id)) {
      // A second account of the same provider gets its own id and key.
      draft.id = uniqueProfileId(draft.name, ids);
      draft.api_key_ref = draft.id;
    }
    editing = { profile: draft, isNew: true, session: ++sessions };
  }

  function startEdit(profile: Profile) {
    editing = { profile: $state.snapshot(profile), isNew: false, session: ++sessions };
  }

  function closeForm() {
    editing = null;
    keysRevision++;
  }

  async function saveProfile(profile: Profile, key: string | null) {
    const current = editing;
    if (!current) return;
    const originalId = current.isNew ? null : current.profile.id;
    if (key !== null && profile.api_key_ref) {
      try {
        await api.setApiKey(profile.api_key_ref, key);
      } catch (error) {
        throw new KeySaveError(error);
      }
    }
    await saveConfig((config) => {
      const index = originalId === null ? -1 : config.profiles.findIndex((p) => p.id === originalId);
      if (index >= 0) config.profiles[index] = profile;
      else config.profiles.push(profile);
    });
    if (disposed) return;
    closeForm();
    toast(t("profiles.saved"));
  }

  async function makeActive(id: string) {
    if (activeBusy !== null || disposed) return;
    activeBusy = id;
    listError = null;
    try {
      await api.setActiveProfile(id);
      // The config-changed event updates the live config.
    } catch (error) {
      if (!disposed) listError = errorText(error);
    } finally {
      if (!disposed) activeBusy = null;
    }
  }

  function askDelete(profile: Profile) {
    deleteTarget = $state.snapshot(profile);
    deleteError = null;
    deleteOpen = true;
  }

  async function confirmDelete() {
    const target = deleteTarget;
    if (!target || deleteBusy || disposed) return;
    deleteBusy = true;
    deleteError = null;
    try {
      const saved = await saveConfig((config) => {
        // Never the active profile, even if it became active since the dialog opened.
        if (config.active_profile_id === target.id) return;
        config.profiles = config.profiles.filter((p) => p.id !== target.id);
      });
      if (!disposed) deleteOpen = false;
      const ref = target.api_key_ref;
      const removed = !saved.profiles.some((p) => p.id === target.id);
      if (removed && ref && !saved.profiles.some((p) => p.api_key_ref === ref)) {
        try {
          await api.deleteApiKey(ref);
        } catch (error) {
          if (!disposed) toast(errorText(error), "error");
        }
      }
    } catch (error) {
      if (!disposed) deleteError = errorText(error);
    } finally {
      if (!disposed) deleteBusy = false;
    }
  }
</script>

<svelte:window onclick={windowClick} onkeydown={windowKey} />

<div>
  <PageHeader title={t("profiles.title")} description={t("profiles.description")}>
    {#snippet actions()}
      {#if !editing}
        <div class="add" bind:this={menu}>
          <Button variant="primary" aria-expanded={menuOpen} aria-controls="profile-presets"
            busy={presetsBusy} disabled={!presets.length} onclick={() => menuOpen = !menuOpen}>
            <Icon name="plus" />{t("profiles.add")}
          </Button>
          <ul id="profile-presets" class="menu" hidden={!menuOpen}>
            {#each presets as preset (preset.id)}
              <li><button type="button" onclick={() => startAdd(preset)}>{presetLabel(preset)}</button></li>
            {/each}
          </ul>
        </div>
      {/if}
    {/snippet}
  </PageHeader>

  {#if editing}
    {#key editing.session}
      <ProfileForm profile={editing.profile} all={profiles} isNew={editing.isNew} onsave={saveProfile} oncancel={closeForm} />
    {/key}
  {:else}
    <div class="stack">
      {#if presetsError}
        <Banner tone="error">
          {presetsError}
          {#snippet action()}<Button busy={presetsBusy} onclick={() => void loadPresets()}>{t("common.retry")}</Button>{/snippet}
        </Banner>
      {/if}
      {#if keysError}<Banner tone="error">{keysError}</Banner>{/if}
      {#if listError}<Banner tone="error">{listError}</Banner>{/if}
      <ul class="profiles">
        {#each profiles as profile (profile.id)}
          {@const active = profile.id === activeId}
          {@const fallback = profiles.find((p) => p.id === profile.fallback_profile_id)}
          <li class="card stack">
            <div class="head">
              <h2>{profile.name}</h2>
              <div class="badges">
                {#if active}<span class="badge success">{t("profiles.active")}</span>{/if}
                {#if profile.api_key_ref && keyState[profile.api_key_ref] === false}
                  <span class="badge warning"><Icon name="key" size={12} />{t("profiles.no_key")}</span>
                {/if}
                {#if profile.base_url.trim().toLowerCase().startsWith("http://")}
                  <span class="badge warning"><Icon name="warning" size={12} />{t("profiles.insecure")}</span>
                {/if}
                {#if fallback}<span class="badge">{t("profiles.fallback_badge", { name: fallback.name })}</span>{/if}
              </div>
            </div>
            <div class="details">
              <p class="mono muted url">{profile.base_url}</p>
              <p class="mono">{profile.model}</p>
            </div>
            <div class="row">
              {#if !active}
                <Button busy={activeBusy === profile.id} disabled={activeBusy !== null} onclick={() => void makeActive(profile.id)}>
                  {t("profiles.make_active")}
                </Button>
              {/if}
              <Button onclick={() => startEdit(profile)}>{t("common.edit")}</Button>
              <Button variant="ghost" disabled={active} title={active ? t("profiles.cannot_delete_active") : undefined}
                onclick={() => askDelete(profile)}>
                <Icon name="trash" />{t("common.delete")}
              </Button>
            </div>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</div>

<Dialog bind:open={deleteOpen} title={t("profiles.delete_title", { name: deleteTarget?.name ?? "" })}>
  <div class="stack">
    <p>{t("profiles.delete_body")}</p>
    {#if deleteError}<Banner tone="error">{deleteError}</Banner>{/if}
  </div>
  {#snippet actions()}
    <Button disabled={deleteBusy} onclick={() => deleteOpen = false}>{t("common.cancel")}</Button>
    <Button variant="danger" busy={deleteBusy} onclick={() => void confirmDelete()}>{t("common.delete")}</Button>
  {/snippet}
</Dialog>

<style>
  .add { position: relative; }
  .menu { position: absolute; right: 0; top: calc(100% + var(--space-1)); z-index: 5; min-width: 260px; margin: 0; padding: var(--space-1); list-style: none; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius); box-shadow: var(--shadow); }
  .menu[hidden] { display: none; }
  .menu button { display: block; width: 100%; padding: var(--space-2) var(--space-3); border: 0; border-radius: var(--radius-sm); background: transparent; text-align: left; cursor: pointer; }
  .menu button:hover { background: var(--surface-2); }
  .profiles { display: flex; flex-direction: column; gap: var(--space-3); margin: 0; padding: 0; list-style: none; }
  .head { display: flex; align-items: center; justify-content: space-between; gap: var(--space-3); flex-wrap: wrap; }
  .badges { display: flex; gap: var(--space-2); flex-wrap: wrap; }
  .badge { display: inline-flex; align-items: center; gap: var(--space-1); padding: 2px var(--space-2); border-radius: var(--radius-sm); background: var(--surface-2); border: 1px solid var(--border); font-size: var(--text-sm); }
  .badge.success { background: var(--success-bg); border-color: var(--success-bg); }
  .badge.warning { background: var(--warning-bg); border-color: var(--warning-bg); }
  .details { display: flex; flex-direction: column; gap: var(--space-1); }
  .url { overflow-wrap: anywhere; }
</style>
