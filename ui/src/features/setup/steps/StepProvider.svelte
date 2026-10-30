<script lang="ts">
  import { untrack } from "svelte";
  import { api } from "../../../lib/bridge/api";
  import { app, errorText, saveConfig, t } from "../../../lib/state/app.svelte";
  import type { MessageKey } from "../../../lib/i18n";
  import { presetProfile, profileProblems, uniqueProfileId, type ProfileProblem } from "../../profiles/profiles";
  import { toast } from "../../../lib/state/toast.svelte";
  import type { Profile } from "../../../lib/bridge/types";
  import Banner from "../../../lib/components/Banner.svelte";
  import Button from "../../../lib/components/Button.svelte";
  import Field from "../../../lib/components/Field.svelte";
  import Icon from "../../../lib/components/Icon.svelte";
  import Switch from "../../../lib/components/Switch.svelte";
  import LocalSettings from "../../profiles/LocalSettings.svelte";

  interface Props {
    onnext: () => void;
    canNext?: boolean;
    form: string;
  }
  let { onnext, canNext = $bindable(false), form }: Props = $props();

  type Choice = "groq" | "openai" | "elevenlabs" | "deepgram" | "local" | "custom";
  type FieldProblem = "name" | "base_url" | "model";

  const PRESET_IDS: string[] = ["groq", "openai", "elevenlabs", "deepgram", "local"];
  const KEY_URLS: Record<"groq" | "openai" | "elevenlabs" | "deepgram", string> = {
    groq: "https://console.groq.com/keys",
    openai: "https://platform.openai.com/api-keys",
    elevenlabs: "https://elevenlabs.io/app/settings/api-keys",
    deepgram: "https://console.deepgram.com",
  };
  const CHOICES: { value: Choice; title: MessageKey }[] = [
    { value: "groq", title: "profiles.preset.groq" },
    { value: "openai", title: "profiles.preset.openai" },
    { value: "elevenlabs", title: "profiles.preset.elevenlabs" },
    { value: "deepgram", title: "profiles.preset.deepgram" },
    { value: "local", title: "profiles.preset.local" },
    { value: "custom", title: "profiles.preset.custom" },
  ];
  const PROBLEM_KEYS: Record<FieldProblem, MessageKey> = {
    name: "profiles.problem.name",
    base_url: "profiles.problem.base_url",
    model: "profiles.problem.model",
  };

  // Coming back to this step starts from the active profile; an active custom profile is edited
  // in place instead of being added again.
  const startProfile = untrack(() => {
    const config = app.config;
    const active = config?.profiles.find((p) => p.id === config.active_profile_id);
    return active ? ($state.snapshot(active) as Profile) : null;
  });
  const editingCustom = startProfile && !PRESET_IDS.includes(startProfile.id) ? startProfile : null;

  function initialChoice(): Choice {
    if (startProfile?.provider === "local_whisper") return "local";
    if (editingCustom) return "custom";
    if (startProfile?.id === "elevenlabs") return "elevenlabs";
    if (startProfile?.id === "deepgram") return "deepgram";
    if (startProfile?.id === "openai") return "openai";
    return "groq";
  }

  let choice = $state<Choice>(initialChoice());
  let localModel = $state(startProfile?.provider === "local_whisper" ? startProfile.model : "ggml-large-v3-turbo-q5_0.bin");
  let presets = $state<Profile[]>([]);
  let presetsBusy = $state(true);
  let presetsError = $state<string | null>(null);
  let name = $state(editingCustom?.name ?? "");
  let baseUrl = $state(editingCustom?.base_url ?? "");
  let model = $state(editingCustom?.model ?? "");
  let needsKey = $state(editingCustom ? editingCustom.api_key_ref !== null : true);
  let touched = $state<Partial<Record<FieldProblem, boolean>>>({});
  let keyInput = $state("");
  let storedKey = $state(false);
  let keyError = $state<string | null>(null);
  let testBusy = $state(false);
  let testResult = $state<"ok" | "failed" | null>(null);
  let testError = $state<string | null>(null);
  let continueAnyway = $state(false);
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let copyError = $state<string | null>(null);
  let testRequest = 0;
  let disposed = false;

  /** The profile this step would store; null until the presets are loaded. */
  const profile = $derived.by((): Profile | null => {
    if (choice !== "custom") {
      // An existing Groq/OpenAI profile is used as it is; only a missing one is added from the preset.
      const chosen = choice === "local" && startProfile?.provider === "local_whisper" ? startProfile : presetProfile(choice, presets, app.config?.profiles ?? []);
      return chosen ? { ...($state.snapshot(chosen) as Profile), ...(choice === "local" ? {model:localModel,api_key_ref:null} : {}) } : null;
    }
    const template = editingCustom ?? presets.find((p) => !PRESET_IDS.includes(p.id));
    if (!template) return null;
    const p: Profile = {
      ...($state.snapshot(template) as Profile),
      name: name.trim(),
      base_url: baseUrl.trim(),
      model: model.trim(),
    };
    if (!editingCustom) {
      // The preset ids stay reserved, so a custom server never takes the place of Groq/OpenAI.
      const ids = [...(app.config?.profiles ?? []).map((other) => other.id), ...PRESET_IDS];
      p.id = uniqueProfileId(p.name, ids);
    }
    p.api_key_ref = needsKey ? (editingCustom?.api_key_ref ?? p.id) : null;
    return p;
  });

  const problems = $derived.by((): FieldProblem[] => {
    if (!profile || choice !== "custom") return [];
    return profileProblems(profile, app.config?.profiles ?? []).filter(
      (p: ProfileProblem): p is FieldProblem => p === "name" || p === "base_url" || p === "model",
    );
  });
  // An existing Groq/OpenAI profile may have been set up without a key; it is kept that way.
  const needsKeyNow = $derived(choice === "local" ? false : choice !== "custom" ? profile?.api_key_ref !== null : needsKey);
  // A new custom profile has no stored key yet, so only presets and an edited profile are checked.
  const keyRef = $derived(choice !== "custom" || editingCustom ? (profile?.api_key_ref ?? null) : null);
  const ready = $derived(
    profile !== null && problems.length === 0 && !saving && !testBusy &&
      (testResult === "ok" ||
        (testResult === "failed" && continueAnyway) ||
        (choice !== "custom" && storedKey && keyInput === "" && testResult === null)),
  );

  $effect(() => {
    canNext = ready;
  });

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
    return () => {
      disposed = true;
    };
  });

  $effect(() => {
    const ref = keyRef;
    let stale = false;
    storedKey = false;
    keyError = null;
    if (!ref) return;
    api.hasApiKey(ref)
      .then((has) => { if (!stale) storedKey = has; })
      .catch((error: unknown) => { if (!stale) keyError = errorText(error); });
    return () => { stale = true; };
  });

  // A connection result only describes the profile and key it was made with.
  $effect(() => {
    void JSON.stringify(profile);
    void keyInput;
    untrack(() => {
      testRequest++;
      testBusy = false;
      testResult = null;
      testError = null;
      continueAnyway = false;
    });
  });

  function problemFor(field: FieldProblem): string | null {
    return touched[field] && problems.includes(field) ? t(PROBLEM_KEYS[field]) : null;
  }

  function choose() {
    keyInput = "";
    saveError = null;
  }

  function setNeedsKey(on: boolean) {
    needsKey = on;
    if (!on) keyInput = "";
  }

  async function testConnection() {
    if (!profile || testBusy || saving || disposed) return;
    if (problems.length) {
      // Show what is missing instead of a disabled button that does not say why.
      touched = { name: true, base_url: true, model: true };
      return;
    }
    const request = ++testRequest;
    testBusy = true;
    testResult = null;
    testError = null;
    try {
      await api.testConnection(profile, profile.api_key_ref !== null && keyInput ? keyInput : null);
      if (!disposed && request === testRequest) testResult = "ok";
    } catch (error) {
      if (!disposed && request === testRequest) {
        testResult = "failed";
        testError = errorText(error);
      }
    } finally {
      if (!disposed && request === testRequest) testBusy = false;
    }
  }

  async function copyUrl(url: string) {
    copyError = null;
    try {
      await navigator.clipboard.writeText(url);
      if (!disposed) toast(t("common.copied"));
    } catch (error) {
      if (!disposed) copyError = errorText(error);
    }
  }

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!canNext || !profile || saving || disposed) return;
    const chosen: Profile = $state.snapshot(profile);
    const key = chosen.api_key_ref !== null && keyInput ? keyInput : null;
    // Only the custom form edits a profile; a chosen Groq/OpenAI profile that exists stays untouched.
    const edited = choice === "custom" || choice === "local";
    saving = true;
    saveError = null;
    try {
      if (key !== null && chosen.api_key_ref !== null) {
        await api.setApiKey(chosen.api_key_ref, key);
        if (disposed) return;
        keyInput = "";
        storedKey = true;
      }
      await saveConfig((c) => {
        const index = c.profiles.findIndex((p) => p.id === chosen.id);
        if (index < 0) c.profiles.push(chosen);
        else if (edited) c.profiles[index] = chosen;
        c.active_profile_id = chosen.id;
      });
      if (!disposed) onnext();
    } catch (error) {
      if (!disposed) saveError = errorText(error);
    } finally {
      if (!disposed) saving = false;
    }
  }
</script>

{#if presetsError}
  <Banner tone="error">
    {presetsError}
    {#snippet action()}<Button busy={presetsBusy} onclick={() => void loadPresets()}>{t("common.reload")}</Button>{/snippet}
  </Banner>
{/if}

<fieldset class="choices" disabled={saving} aria-busy={presetsBusy}>
  <legend class="visually-hidden">{t("setup.provider.title")}</legend>
  {#each CHOICES as item (item.value)}
    <label class="choice">
      <input type="radio" name="setup-provider" value={item.value} bind:group={choice} onchange={choose} />
      <span class="choice-text"><span class="choice-title">{t(item.title)}</span></span>
    </label>
  {/each}
</fieldset>

{#if choice === "local"}<LocalSettings bind:model={localModel} />{/if}
{#if choice === "custom"}
  <div class="stack custom">
    <Field label={t("profiles.name")} error={problemFor("name")}>
      {#snippet children(id, describedBy)}
        <input {id} type="text" bind:value={name} disabled={saving} autocomplete="off"
          onblur={() => (touched.name = true)} aria-invalid={!!problemFor("name")}
          aria-describedby={describedBy} />
      {/snippet}
    </Field>
    <Field label={t("profiles.base_url")} error={problemFor("base_url")}>
      {#snippet children(id, describedBy)}
        <input {id} type="url" class="mono" bind:value={baseUrl} disabled={saving} autocomplete="off"
          spellcheck="false" onblur={() => (touched.base_url = true)} aria-invalid={!!problemFor("base_url")}
          aria-describedby={describedBy} />
      {/snippet}
    </Field>
    <Field label={t("profiles.model")} error={problemFor("model")}>
      {#snippet children(id, describedBy)}
        <input {id} type="text" class="mono" bind:value={model} disabled={saving} autocomplete="off"
          spellcheck="false" onblur={() => (touched.model = true)} aria-invalid={!!problemFor("model")}
          aria-describedby={describedBy} />
      {/snippet}
    </Field>
    <Switch label={t("profiles.needs_key")} disabled={saving} bind:checked={() => needsKey, setNeedsKey} />
  </div>
{/if}

{#if needsKeyNow}
  {#if choice !== "custom" && choice !== "local"}
    {@const url = KEY_URLS[choice]}
    <div class="get-key">
      <p class="selectable">{t("setup.provider.get_key", { url })}</p>
      <Button variant="ghost" aria-label={t("common.copy")} title={t("common.copy")} onclick={() => void copyUrl(url)}>
        <Icon name="copy" />
      </Button>
    </div>
    {#if copyError}<p class="failed" role="alert">{copyError}</p>{/if}
  {/if}
  {#if storedKey}<Banner tone="info">{t("setup.provider.key_stored")}</Banner>{/if}
  <Field label={t("profiles.key")} error={keyError}>
    {#snippet children(id, describedBy)}
      <input {id} type="password" autocomplete="off" spellcheck="false" bind:value={keyInput} disabled={saving}
        placeholder={storedKey ? t("profiles.key_stored") : t("profiles.key_paste")}
        aria-invalid={!!keyError} aria-describedby={describedBy} />
    {/snippet}
  </Field>
{/if}

<div class="row">
  <Button busy={testBusy} disabled={!profile || saving} onclick={() => void testConnection()}>
    {t("profiles.test")}
  </Button>
  {#if testResult === "ok"}<p class="connected" role="status"><Icon name="check" />{t("profiles.connected")}</p>{/if}
  {#if testResult === "failed" && testError}<p class="failed" role="alert">{testError}</p>{/if}
</div>
{#if testResult === "failed"}
  <Switch label={t("setup.provider.continue_anyway")} bind:checked={continueAnyway} disabled={saving} />
{/if}

{#if saving}<p class="muted" role="status">{t("common.saving")}</p>{/if}
{#if saveError}<Banner tone="error">{saveError}</Banner>{/if}
<form id={form} onsubmit={submit}></form>

<style>
  input:not([type="radio"]) { width: 100%; }
  .custom { padding-top: var(--space-2); }
  .get-key { display: flex; align-items: center; gap: var(--space-2); }
  .selectable { user-select: text; overflow-wrap: anywhere; }
  .connected { display: inline-flex; align-items: center; gap: var(--space-1); color: var(--success); }
  .failed { color: var(--danger); overflow-wrap: anywhere; }
</style>
