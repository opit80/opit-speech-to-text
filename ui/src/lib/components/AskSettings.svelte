<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { api } from "../api";
  import { app, errorText, saveConfig, t } from "../app.svelte";
  import { toast } from "../toast.svelte";
  import type { AskConfig, AgentStatus, AskAgent } from "../types";
  import Banner from "./Banner.svelte";
  import Button from "./Button.svelte";
  import Field from "./Field.svelte";
  import HotkeyInput from "./HotkeyInput.svelte";
  import Select from "./Select.svelte";
  import Switch from "./Switch.svelte";

  let draft = $state<AskConfig>(untrack(() => structuredClone($state.snapshot(app.config!.ask))));
  let keys = $state<string[]>(untrack(() => [...app.config!.hotkey.ask_keys]));
  let statuses = $state<AgentStatus[]>([]);
  let loaded = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let result = $state<string | null>(null);
  let keyInput = $state("");
  let disposed = false;
  const agents = $derived((["chat", "claude", "codex"] as AskAgent[]).map((agent) => ({
    value: agent, label: t(`ask.agent_${agent}`),
  })));
  const permissions = $derived([
    { value: "restricted", label: t("ask.restricted") },
    { value: "workspace", label: t("ask.workspace") },
  ]);
  const outputs = $derived([
    { value: "paste", label: t("ask.paste") }, { value: "copy", label: t("ask.copy") },
  ]);
  const installed = $derived(draft.agent === "chat" || statuses.some((s) => s.agent === draft.agent && s.installed));

  onMount(() => {
    void load();
    return () => { disposed = true; keyInput = ""; };
  });
  async function load() {
    try { const data = await api.askStatus(); if (!disposed) { statuses = data; loaded = true; } }
    catch (e) { if (!disposed) error = errorText(e); }
  }
  async function storeKey() {
    if (draft.agent === "chat" && draft.api_key_ref && keyInput.trim()) {
      const key = keyInput; keyInput = "";
      await api.setApiKey(draft.api_key_ref, key);
    }
  }
  async function save() {
    busy = true; error = null; result = null;
    try {
      await storeKey();
      const config = $state.snapshot(draft);
      const askKeys = [...keys];
      await saveConfig((c) => { c.ask = config; c.hotkey.ask_keys = askKeys; });
      if (!disposed) toast(t("common.saved"));
    } catch (e) { if (!disposed) error = errorText(e); }
    finally { if (!disposed) busy = false; }
  }
  async function test() {
    busy = true; error = null; result = null;
    try {
      await storeKey();
      const answer = await api.testAsk($state.snapshot(draft));
      if (!disposed) result = answer;
    } catch (e) { if (!disposed) error = errorText(e); }
    finally { if (!disposed) busy = false; }
  }
</script>

<form class="stack" onsubmit={(event) => { event.preventDefault(); void save(); }}>
  <fieldset class="stack" disabled={busy}>
    <HotkeyInput {keys} optional target="ask" label={t("ask.shortcut")} info={t("ask.shortcut_hint")} onchange={(value) => { keys = value; }} />
    <Select label={t("ask.agent")} info={t("ask.privacy")} value={draft.agent} options={agents} onchange={(value) => { draft.agent = value as AskAgent; result = null; }} />
    <p class="muted" role="status">{!loaded ? t("common.loading") : installed ? t("ask.available") : t("ask.missing")}</p>
    {#if draft.agent === "chat"}
      <Field label={t("profiles.base_url")}>
        {#snippet children(id)}<input {id} type="url" bind:value={draft.base_url} required autocomplete="off" spellcheck="false" />{/snippet}
      </Field>
      <Field label={t("profiles.model")}>
        {#snippet children(id)}<input {id} bind:value={draft.chat_model} required autocomplete="off" spellcheck="false" />{/snippet}
      </Field>
      <Switch label={t("profiles.needs_key")} checked={draft.api_key_ref !== null} onchange={(on) => { draft.api_key_ref = on ? "ask" : null; keyInput = ""; }} />
      {#if draft.api_key_ref}
        <Field label={t("profiles.key")} info={t("ask.key_hint")}>
          {#snippet children(id, describedBy)}<input {id} aria-describedby={describedBy} type="password" bind:value={keyInput} autocomplete="new-password" />{/snippet}
        </Field>
      {/if}
    {:else if draft.agent === "claude"}
      <Field label={t("ask.claude_model")}>
        {#snippet children(id)}<input {id} bind:value={draft.claude_model} required autocomplete="off" spellcheck="false" />{/snippet}
      </Field>
    {:else}
      <Field label={t("ask.codex_model")} info={t("ask.codex_model_hint")}>
        {#snippet children(id, describedBy)}<input {id} aria-describedby={describedBy} bind:value={draft.codex_model} autocomplete="off" spellcheck="false" />{/snippet}
      </Field>
    {/if}
    {#if draft.agent !== "chat"}
      <Select label={t("ask.permission")} info={t("ask.restricted_hint")} value={draft.permission} options={permissions} onchange={(value) => draft.permission = value as AskConfig["permission"]} />
      {#if draft.permission === "workspace"}<Banner tone="warning">{t("ask.permission_warning")}</Banner>{/if}
    {/if}
    <Field label={t("ask.directory")} info={t("ask.directory_hint")}>
      {#snippet children(id, describedBy)}<input {id} aria-describedby={describedBy} bind:value={draft.working_dir} autocomplete="off" spellcheck="false" />{/snippet}
    </Field>
    <Field label={t("ask.timeout")}>
      {#snippet children(id)}<input {id} type="number" min="1" max="3600" step="1" bind:value={draft.timeout_seconds} required />{/snippet}
    </Field>
    <Field label={t("ask.continue")} info={t("ask.continue_hint")}>
      {#snippet children(id, describedBy)}<input {id} aria-describedby={describedBy} type="number" min="0" max="1440" step="1" bind:value={draft.continue_minutes} required />{/snippet}
    </Field>
    <Select label={t("ask.output")} value={draft.output} options={outputs} onchange={(value) => draft.output = value as AskConfig["output"]} />
    <Switch label={t("ask.cleanup")} info={t("ask.cleanup_hint")} bind:checked={draft.cleanup_request} />
  </fieldset>
  {#if error}<Banner tone="error">{error}</Banner>{/if}
  {#if result}<p class="answer" role="status">{result}</p>{/if}
  <div class="row">
    <Button busy={busy} disabled={!loaded || !installed} onclick={() => void test()}>{t("ask.test")}</Button>
    <Button type="submit" variant="primary" disabled={busy}>{t("common.save")}</Button>
  </div>
</form>

<style>
  fieldset { border: 0; padding: 0; margin: 0; min-width: 0; }
  input { width: 100%; }
  .answer { user-select: text; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
