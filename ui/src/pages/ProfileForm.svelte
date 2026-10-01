<script module lang="ts">
  /** Thrown by `onsave` when the API key could not be stored; nothing was saved then. */
  export class KeySaveError extends Error {
    source: unknown;
    constructor(source: unknown) {
      super("the API key could not be stored");
      this.source = source;
    }
  }
</script>

<script lang="ts">
  import { tick, untrack } from "svelte";
  import { api } from "../lib/api";
  import { app, errorText, t } from "../lib/app.svelte";
  import type { MessageKey } from "../lib/i18n";
  import { LANGUAGES, profileProblems, uniqueProfileId, type ProfileProblem } from "../lib/profiles";
  import type { Profile } from "../lib/types";
  import Banner from "../lib/components/Banner.svelte";
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import Field from "../lib/components/Field.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Select from "../lib/components/Select.svelte";
  import Switch from "../lib/components/Switch.svelte";

  interface Props {
    profile: Profile;
    all: Profile[];
    isNew: boolean;
    onsave: (p: Profile, key: string | null) => Promise<void>;
    oncancel: () => void;
  }
  let { profile, all, isNew, onsave, oncancel }: Props = $props();

  const problemKeys: Record<ProfileProblem, MessageKey> = {
    name: "profiles.problem.name",
    base_url: "profiles.problem.base_url",
    model: "profiles.problem.model",
    fallback_self: "profiles.problem.fallback_self",
    fallback_missing: "profiles.problem.fallback_missing",
  };

  // The form edits a local draft; `original` is the version it started from.
  let original = $state<Profile>(untrack(() => $state.snapshot(profile)));
  let draft = $state<Profile>(untrack(() => $state.snapshot(profile)));
  let touched = $state<Record<string, boolean>>({});
  let keyInput = $state("");
  let keyError = $state<string | null>(null);
  let storedKey = $state(false);
  let saveBusy = $state(false);
  let saveError = $state<string | null>(null);
  let testBusy = $state(false);
  let testOk = $state(false);
  let testError = $state<string | null>(null);
  let removeOpen = $state(false);
  let removeBusy = $state(false);
  let removeError = $state<string | null>(null);
  let nameInput = $state<HTMLInputElement>();
  let disposed = false;
  let testRequest = 0;

  /** A new custom profile gets its id (and key name) from its name when it is saved. */
  const derivesId = untrack(() => isNew && !profile.id);
  const problems = $derived(profileProblems(draft, all));
  const needsKey = $derived(draft.api_key_ref !== null);
  const insecure = $derived(draft.base_url.trim().toLowerCase().startsWith("http://"));
  const live = $derived(app.config?.profiles.find((p) => p.id === original.id) ?? null);
  const changedElsewhere = $derived(!isNew && !saveBusy && live !== null && canonical(live) !== canonical(original));
  const languageNames = $derived(displayNames(app.lang));
  const languageOptions = $derived.by(() => {
    const codes: string[] = [...LANGUAGES];
    if (!codes.includes(draft.language)) codes.push(draft.language);
    return codes.map((code) => ({
      value: code,
      label: code === "auto" ? t("profiles.language_auto") : `${languageNames?.of(code) ?? code} (${code})`,
    }));
  });
  const fallbackOptions = $derived.by(() => {
    const others = all.filter((p) => p.id !== draft.id).map((p) => ({ value: p.id, label: p.name }));
    const current = draft.fallback_profile_id;
    if (current && !others.some((o) => o.value === current)) others.push({ value: current, label: current });
    return [{ value: "", label: t("common.none") }, ...others];
  });

  function canonical(p: Profile): string {
    return JSON.stringify(p, Object.keys(p).sort());
  }

  function displayNames(lang: string): Intl.DisplayNames | null {
    try {
      return new Intl.DisplayNames([lang], { type: "language" });
    } catch {
      return null;
    }
  }

  function problemFor(...kinds: ProfileProblem[]): string | null {
    const found = problems.find((p) => kinds.includes(p));
    if (!found) return null;
    // A new profile shows a field's problem only once the user has left that field.
    if (isNew && !touched[found] && found !== "fallback_self" && found !== "fallback_missing") return null;
    return t(problemKeys[found]);
  }

  function touch(field: ProfileProblem) {
    touched[field] = true;
  }

  /** The draft as it would be stored: a new custom profile gets its id and key name here. */
  function resolved(): Profile {
    const p: Profile = $state.snapshot(draft);
    p.name = p.name.trim();
    p.base_url = p.base_url.trim();
    p.model = p.model.trim();
    if (derivesId) {
      const otherIds = all.map((other) => other.id);
      p.id = uniqueProfileId(p.name, otherIds);
      if (p.api_key_ref !== null) p.api_key_ref = p.id;
    }
    return p;
  }

  function setNeedsKey(on: boolean) {
    if (on) draft.api_key_ref = original.api_key_ref ?? draft.id;
    else {
      draft.api_key_ref = null;
      keyInput = "";
      keyError = null;
    }
  }

  function reload() {
    if (!live) return;
    const next: Profile = $state.snapshot(live);
    original = next;
    draft = $state.snapshot(next);
    touched = {};
  }

  $effect(() => {
    void tick().then(() => { if (!disposed) nameInput?.focus(); });
    return () => { disposed = true; };
  });

  // Whether a key is stored under this profile's key name (none yet for an unsaved custom profile).
  $effect(() => {
    const ref = draft.api_key_ref;
    let stale = false;
    storedKey = false;
    if (!ref) return;
    api.hasApiKey(ref)
      .then((has) => { if (!stale) storedKey = has; })
      .catch((error: unknown) => { if (!stale) keyError = errorText(error); });
    return () => { stale = true; };
  });

  // A connection result only describes the draft it was made with.
  $effect(() => {
    void JSON.stringify(draft);
    void keyInput;
    untrack(() => {
      testRequest++;
      testBusy = false;
      testOk = false;
      testError = null;
    });
  });

  async function testConnection() {
    if (testBusy || disposed) return;
    const request = ++testRequest;
    testBusy = true;
    testOk = false;
    testError = null;
    try {
      await api.testConnection(resolved(), needsKey && keyInput ? keyInput : null);
      if (!disposed && request === testRequest) testOk = true;
    } catch (error) {
      if (!disposed && request === testRequest) testError = errorText(error);
    } finally {
      if (!disposed && request === testRequest) testBusy = false;
    }
  }

  async function save() {
    if (saveBusy || problems.length || disposed) return;
    saveBusy = true;
    saveError = null;
    keyError = null;
    try {
      const p = resolved();
      await onsave(p, p.api_key_ref !== null && keyInput ? keyInput : null);
      keyInput = "";
    } catch (error) {
      if (disposed) return;
      if (error instanceof KeySaveError) keyError = errorText(error.source);
      else saveError = errorText(error);
    } finally {
      if (!disposed) saveBusy = false;
    }
  }

  async function removeKey() {
    const ref = draft.api_key_ref;
    if (!ref || removeBusy || disposed) return;
    removeBusy = true;
    removeError = null;
    try {
      await api.deleteApiKey(ref);
      if (disposed) return;
      storedKey = false;
      removeOpen = false;
    } catch (error) {
      if (!disposed) removeError = errorText(error);
    } finally {
      if (!disposed) removeBusy = false;
    }
  }
</script>

<form class="card stack" novalidate aria-labelledby="profile-form-title" aria-busy={saveBusy}
  onsubmit={(event) => { event.preventDefault(); void save(); }}>
  <h2 id="profile-form-title">{isNew ? t("profiles.add") : original.name}</h2>

  {#if changedElsewhere}
    <Banner tone="warning">
      {t("profiles.changed_elsewhere")}
      {#snippet action()}<Button onclick={reload}>{t("common.reload")}</Button>{/snippet}
    </Banner>
  {/if}

  <Field label={t("profiles.name")} error={problemFor("name")}>
    {#snippet children(id)}
      <input {id} type="text" bind:this={nameInput} bind:value={draft.name} onblur={() => touch("name")}
        autocomplete="off" aria-invalid={!!problemFor("name")}
        aria-describedby={problemFor("name") ? `${id}-description` : undefined} />
    {/snippet}
  </Field>

  <Field label={t("profiles.base_url")} error={problemFor("base_url")}>
    {#snippet children(id)}
      <input {id} type="url" class="mono" bind:value={draft.base_url} onblur={() => touch("base_url")}
        autocomplete="off" spellcheck="false" aria-invalid={!!problemFor("base_url")}
        aria-describedby={[insecure ? `${id}-warning` : "", problemFor("base_url") ? `${id}-description` : ""].filter(Boolean).join(" ") || undefined} />
      {#if insecure}
        <p id={`${id}-warning`} class="warning"><Icon name="warning" />{t("profiles.insecure_warning")}</p>
      {/if}
    {/snippet}
  </Field>

  <Field label={t("profiles.model")} error={problemFor("model")}>
    {#snippet children(id)}
      <input {id} type="text" class="mono" bind:value={draft.model} onblur={() => touch("model")}
        autocomplete="off" spellcheck="false" aria-invalid={!!problemFor("model")}
        aria-describedby={problemFor("model") ? `${id}-description` : undefined} />
    {/snippet}
  </Field>

  <div class="grid">
    <Select label={t("profiles.language")} options={languageOptions}
      bind:value={() => draft.language, (value) => { draft.language = value; }} />
    <Select label={t("profiles.audio_format")}
      options={[{ value: "flac", label: "FLAC" }, { value: "wav", label: "WAV" }]}
      bind:value={() => draft.audio_format, (value) => { draft.audio_format = value === "wav" ? "wav" : "flac"; }} />
  </div>
  <Select label={t("profiles.response_format")} hint={t("profiles.response_format_hint")}
    options={[{ value: "verbose_json", label: "verbose_json" }, { value: "json", label: "json" }]}
    bind:value={() => draft.response_format, (value) => { draft.response_format = value === "verbose_json" ? "verbose_json" : "json"; }} />

  <Switch label={t("profiles.send_prompt")} bind:checked={draft.send_prompt} />
  <Switch label={t("profiles.send_keywords")} hint={t("profiles.send_keywords_hint")} bind:checked={draft.send_keywords} />
  <Switch label={t("profiles.apply_rules")} hint={t("profiles.apply_rules_hint")} bind:checked={draft.apply_rules} />

  <Field label={t("profiles.fallback")} hint={t("profiles.fallback_hint")} error={problemFor("fallback_self", "fallback_missing")}>
    {#snippet children(id)}
      <select {id} aria-describedby={`${id}-description`} aria-invalid={!!problemFor("fallback_self", "fallback_missing")}
        bind:value={() => draft.fallback_profile_id ?? "", (value) => { draft.fallback_profile_id = value || null; }}>
        {#each fallbackOptions as option (option.value)}<option value={option.value}>{option.label}</option>{/each}
      </select>
    {/snippet}
  </Field>

  <section class="key stack" aria-labelledby="profile-key-title">
    <h3 id="profile-key-title" class="visually-hidden">{t("profiles.key")}</h3>
    <Switch label={t("profiles.needs_key")} bind:checked={() => needsKey, setNeedsKey} />
    {#if needsKey}
      <Field label={t("profiles.key")} error={keyError}>
        {#snippet children(id)}
          <input {id} type="password" autocomplete="off" spellcheck="false" bind:value={keyInput}
            placeholder={storedKey ? t("profiles.key_stored") : t("profiles.key_paste")}
            aria-invalid={!!keyError} aria-describedby={keyError ? `${id}-description` : undefined} />
        {/snippet}
      </Field>
      {#if storedKey}
        <div><button type="button" class="link" onclick={() => { removeError = null; removeOpen = true; }}>{t("profiles.remove_key")}</button></div>
      {/if}
    {/if}
  </section>

  <div class="row">
    <Button busy={testBusy} disabled={saveBusy} onclick={() => void testConnection()}>{t("profiles.test")}</Button>
    {#if testOk}<p class="connected" role="status"><Icon name="check" />{t("profiles.connected")}</p>{/if}
    {#if testError}<p class="failed" role="alert">{testError}</p>{/if}
  </div>

  {#if saveError}<Banner tone="error">{saveError}</Banner>{/if}

  <div class="actions">
    <Button disabled={saveBusy} onclick={oncancel}>{t("common.cancel")}</Button>
    <Button type="submit" variant="primary" busy={saveBusy} disabled={problems.length > 0}>{t("common.save")}</Button>
  </div>
</form>

<Dialog bind:open={removeOpen} title={t("profiles.remove_key_title")}>
  {#if removeError}<Banner tone="error">{removeError}</Banner>{/if}
  {#snippet actions()}
    <Button disabled={removeBusy} onclick={() => removeOpen = false}>{t("common.cancel")}</Button>
    <Button variant="danger" busy={removeBusy} onclick={() => void removeKey()}>{t("common.remove")}</Button>
  {/snippet}
</Dialog>

<style>
  input, select { width: 100%; }
  .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); gap: var(--space-3); }
  .warning { display: flex; align-items: center; gap: var(--space-2); color: var(--warning); font-size: var(--text-sm); }
  .key { padding-top: var(--space-3); border-top: 1px solid var(--border); }
  .link { padding: 0; border: 0; background: transparent; color: var(--accent); text-decoration: underline; text-underline-offset: 2px; cursor: pointer; }
  .link:hover { color: var(--accent-hover); }
  .connected { display: inline-flex; align-items: center; gap: var(--space-1); color: var(--success); }
  .failed { color: var(--danger); overflow-wrap: anywhere; }
  .actions { display: flex; justify-content: flex-end; gap: var(--space-2); padding-top: var(--space-3); border-top: 1px solid var(--border); }
</style>
