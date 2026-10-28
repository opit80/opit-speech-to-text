<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../api";
  import { app, errorText, saveConfig, t } from "../app.svelte";
  import { onLocalProgress, type UnlistenFn } from "../events";
  import type { LocalConfig, LocalState } from "../types";
  import type { MessageKey } from "../i18n";
  import { localInteger, localStatus } from "../local";
  import Banner from "./Banner.svelte";
  import Button from "./Button.svelte";
  import Field from "./Field.svelte";
  import Select from "./Select.svelte";
  import Switch from "./Switch.svelte";

  let { model = $bindable() }: { model: string } = $props();
  let snapshot = $state<LocalState | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let folder = $state("");
  let disposed = false;
  let refreshing = false;
  const config = $derived(app.config?.local);
  const modelInstalled = $derived(snapshot?.installed.includes(model) ?? false);
  const buildInstalled = $derived(snapshot?.installed.includes(config?.build ?? "blas") ?? false);
  const status = $derived(localStatus(snapshot,model,config?.build ?? "blas"));

  function saveNumber(key: "threads" | "idle_minutes", value: string) {
    const n = localInteger(value);
    if (n === null) { error = t("local.invalid_number"); return; }
    void update(key,n);
  }
  function size(bytes: number): string {
    return new Intl.NumberFormat(app.lang === "tr" ? "tr-TR" : "en-US", {style:"unit",unit:"megabyte",maximumFractionDigits:1}).format(bytes/1000000);
  }

  function localError(value: unknown): string {
    const code = (value as { code?: string })?.code;
    const key = code ? `error.code.${code}` : "";
    return code && ["local_folder", "local_hash", "local_server_missing", "local_not_installed", "local_cancelled", "local_busy", "local_readiness", "local"].includes(code)
      ? t(key as MessageKey) : errorText(value);
  }
  async function refresh() {
    if (refreshing || disposed) return;
    refreshing = true;
    try { const next = await api.localState(); if (!disposed) snapshot = next; }
    catch (e) { if (!disposed) error = localError(e); }
    finally { refreshing = false; }
  }
  onMount(() => {
    folder = app.config?.local.models_folder ?? "";
    void refresh();
    let off: UnlistenFn | undefined;
    void onLocalProgress((progress) => { if (snapshot) snapshot.progress = progress; })
      .then((unlisten) => { if (disposed) unlisten(); else off = unlisten; }).catch((e) => { if (!disposed) error = localError(e); });
    const timer = setInterval(() => void refresh(), 1500);
    return () => { disposed = true; off?.(); clearInterval(timer); };
  });
  async function update<K extends keyof LocalConfig>(key: K, value: LocalConfig[K]) {
    error = null;
    try { await saveConfig((c) => { c.local[key] = value; }); await refresh(); }
    catch (e) { if (!disposed) error = localError(e); }
  }
  async function run(id: string, remove = false) {
    if (busy) return;
    busy = true; error = null;
    try { if (remove) await api.localDelete(id); else await api.localDownload(id); }
    catch (e) { if (!disposed) error = localError(e); }
    finally { if (!disposed) { busy = false; await refresh(); } }
  }
  async function cancel() {
    try { await api.localCancel(); } catch (e) { if (!disposed) error = localError(e); }
  }
  const buildLabels: Record<string, MessageKey> = { blas: "local.build_blas", cpu: "local.build_cpu", cuda12: "local.build_cuda12", cuda11: "local.build_cuda11" };
</script>

<section class="stack" aria-label={t("profiles.preset.local")}>
  <p role="status">{t(status)}</p>
  {#if error}<Banner tone="error">{error}</Banner>{/if}
  {#if config}
    <Select label={t("local.model")} info={t("local.hint")} value={model} disabled={busy || !!snapshot?.progress}
      options={(snapshot?.models ?? []).map((m) => ({ value: m.id, label: `${m.id.replace(/^ggml-/, "").replace(/\.bin$/, "")} · ${size(m.size)}${m.id === snapshot?.recommended ? ` · ${t("local.recommended")}` : ""}` }))}
      onchange={(value) => { model = value; }} />
    <div class="row">
      <Button disabled={busy || !!snapshot?.progress} onclick={() => void run(model, modelInstalled)}>{t(modelInstalled ? "local.delete_model" : "local.download_model")}</Button>
    </div>
    <Select label={t("local.build")} value={config.build} disabled={busy || !!snapshot?.progress}
      options={(snapshot?.builds ?? []).map((b) => ({ value: b.id, label: `${t(buildLabels[b.id])} · ${size(b.size)}` }))}
      onchange={(value) => void update("build", value)} />
    <div class="row">
      <Button disabled={busy || !!snapshot?.progress} onclick={() => void run(config.build, buildInstalled)}>{t(buildInstalled ? "local.delete_build" : "local.download_build")}</Button>
    </div>
    {#if snapshot?.progress}
      <div class="stack" role="status">
        <label for="local-download">{t("local.progress",{file:snapshot.progress.id,received:size(snapshot.progress.received),total:size(snapshot.progress.total)})}</label>
        <progress id="local-download" max={snapshot.progress.total} value={snapshot.progress.received}></progress>
        <Button onclick={() => void cancel()}>{t("common.cancel")}</Button>
      </div>
    {/if}
    <details>
      <summary>{t("local.advanced")}</summary>
      <div class="stack advanced">
        <Field label={t("local.threads")} info={t("local.threads_hint")}>
          {#snippet children(id, describedBy)}<input {id} aria-describedby={describedBy} type="number" min="0" max="65535" value={config.threads} onchange={(e) => saveNumber("threads", e.currentTarget.value)} />{/snippet}
        </Field>
        <Switch label={t("local.gpu")} info={t("local.gpu_hint")} checked={config.gpu} onchange={(value) => void update("gpu", value)} />
        <Field label={t("local.idle")} info={t("local.idle_hint")}>
          {#snippet children(id, describedBy)}<input {id} aria-describedby={describedBy} type="number" min="0" max="65535" value={config.idle_minutes} onchange={(e) => saveNumber("idle_minutes", e.currentTarget.value)} />{/snippet}
        </Field>
        <Switch label={t("local.preload")} checked={config.preload} onchange={(value) => void update("preload", value)} />
        <Field label={t("local.folder")} info={t("local.folder_hint")}>
          {#snippet children(id, describedBy)}<input {id} aria-describedby={describedBy} type="text" bind:value={folder} spellcheck="false" />{/snippet}
        </Field>
        <Button disabled={busy || !!snapshot?.progress} onclick={() => void update("models_folder", folder.trim() || null)}>{t("local.save_folder")}</Button>
        {#if snapshot}<p class="muted path">{snapshot.folder}</p>{/if}
      </div>
    </details>
  {/if}
</section>

<style>
  progress { width: 100%; accent-color: var(--accent); }
  .advanced { padding-top: var(--space-3); }
  .path { overflow-wrap: anywhere; }
  summary { cursor: pointer; }
  input { width: 100%; }
</style>
