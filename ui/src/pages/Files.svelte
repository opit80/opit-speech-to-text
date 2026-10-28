<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import { app, errorText, t } from "../lib/app.svelte";
  import { onFileProgress } from "../lib/events";
  import { acceptFileSnapshot, fileRunning, mergeFileEvent } from "../lib/files";
  import { formatClock } from "../lib/format";
  import type { FileOutputFormat, FileState } from "../lib/types";
  import Banner from "../lib/components/Banner.svelte";
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import Field from "../lib/components/Field.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import Select from "../lib/components/Select.svelte";
  import Switch from "../lib/components/Switch.svelte";

  let job = $state<FileState>({ revision: -1, name: null, phase: "idle", progress: null, result: null, error: null,
    options: { language: null, cleanup: false, speakers: false }, unsaved: false });
  let loading = $state(true);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let notice = $state<string | null>(null);
  let language = $state("");
  let cleanup = $state(false);
  let speakers = $state(false);
  let prefixes = $state(false);
  let format = $state<FileOutputFormat>("txt");
  let preview = $state("");
  let rendering = $state(false);
  let discardOpen = $state(false);
  let disposed = false;
  let renderRequest = 0;
  const running = $derived(fileRunning(job.phase));
  const activeProfile = $derived(app.config?.profiles.find((p) => p.id === app.config?.active_profile_id));
  const canSpeakers = $derived(activeProfile?.provider === "elevenlabs");
  const formats = $derived([
    { value: "txt", label: t("files.txt") },
    { value: "srt", label: t("files.srt"), disabled: !job.result?.timestamps_available },
  ]);

  function accept(snapshot: FileState) {
    job = acceptFileSnapshot(job, snapshot);
    if (job.phase !== "idle") {
      language = job.options.language ?? "";
      cleanup = job.options.cleanup;
      speakers = job.options.speakers;
    }
  }

  async function load() {
    const snapshot = await api.fileState();
    if (!disposed) accept(snapshot);
  }

  onMount(() => {
    let stop: (() => void) | undefined;
    onFileProgress((event) => {
      if (disposed) return;
      const previous = job;
      job = mergeFileEvent(job, event);
      // Recover options/name as well when progress beats the start response.
      if (job !== previous) void load().catch((e: unknown) => { if (!disposed) error = errorText(e); });
    }).then(async (unlisten) => {
      if (disposed) { unlisten(); return; }
      stop = unlisten;
      await load();
      if (!disposed) loading = false;
    }).catch((e: unknown) => { if (!disposed) { error = errorText(e); loading = false; } });
    return () => { disposed = true; renderRequest++; stop?.(); };
  });

  $effect(() => {
    const result = job.result;
    const selected = format;
    const withPrefixes = prefixes;
    const withSpeakers = speakers && job.options.speakers;
    const request = ++renderRequest;
    preview = "";
    if (!result) { rendering = false; return; }
    if (selected === "srt" && !result.timestamps_available) { format = "txt"; return; }
    rendering = true;
    api.fileRender(selected, withPrefixes, withSpeakers).then((text) => {
      if (!disposed && request === renderRequest) preview = text;
    }).catch((e: unknown) => { if (!disposed && request === renderRequest) error = errorText(e); })
      .finally(() => { if (!disposed && request === renderRequest) rendering = false; });
  });

  async function act(work: () => Promise<void>) {
    busy = true; error = null; notice = null;
    try { await work(); } catch (e) { if (!disposed) error = errorText(e); }
    finally { if (!disposed) busy = false; }
  }

  function pick() { return act(async () => { const next = await api.filePick(); if (!disposed) accept(next); }); }
  function start() { return act(async () => {
    const next = await api.fileStart({ language: language.trim() || null, cleanup, speakers: speakers && canSpeakers });
    if (!disposed) accept(next);
  }); }
  function cancel() { return act(async () => { await api.fileCancel(); await load(); }); }
  // Saving or copying keeps the revision and the result; only `unsaved` changes, set in place so
  // the preview is not rendered again.
  function markHandled(revision: number) {
    if (!disposed && job.revision === revision) job.unsaved = false;
  }
  function save() { return act(async () => {
    const revision = job.revision;
    if (await api.fileSave(format, prefixes, speakers && job.options.speakers)) {
      markHandled(revision);
      notice = t("files.saved");
    }
  }); }
  function copy() { return act(async () => {
    const revision = job.revision;
    await navigator.clipboard.writeText(preview);
    notice = t("files.copied");
    if (!(await api.fileHandled(revision)).unsaved) markHandled(revision);
  }); }
  function discard() { return act(async () => {
    discardOpen = false;
    const next = await api.fileDiscard(job.revision);
    if (!disposed) accept(next);
  }); }
  // A result that was not saved or copied cannot be restored, so discarding it asks first.
  function askDiscard() { if (job.unsaved) discardOpen = true; else void discard(); }
</script>

<div class="files">
  <PageHeader title={t("files.title")} description={t("files.description")} />
  <p class="muted">{t("files.privacy")}</p>
  {#if error}<Banner tone="error">{error}</Banner>{/if}
  {#if job.error}<Banner tone="error">{errorText(job.error)}</Banner>{/if}
  {#if notice}<Banner tone="info">{notice}</Banner>{/if}
  {#if loading}<p class="muted" role="status">{t("common.loading")}</p>
  {:else}
    <div class="source">
      <Button disabled={running} {busy} onclick={pick}><Icon name="files" />{t("files.pick")}</Button>
      <span class="filename">{job.name ?? t("files.empty")}</span>
    </div>
    <p class="muted">{t("files.profile", { name: activeProfile?.name ?? t("files.profile_default") })}</p>
    <fieldset disabled={running || busy}>
      <legend>{t("files.options")}</legend>
      <Field label={t("files.language")} hint={t("files.language_hint")}>
        {#snippet children(id)}<input {id} aria-describedby={`${id}-description`} bind:value={language} placeholder={t("files.profile_default")} maxlength="12" />{/snippet}
      </Field>
      <Switch checked={cleanup} label={t("files.cleanup")} hint={t("files.cleanup_hint")} onchange={(value) => cleanup = value} />
      {#if canSpeakers}<Switch checked={speakers} label={t("files.speakers")} hint={t("files.speakers_hint")} onchange={(value) => speakers = value} />{/if}
    </fieldset>
    {#if running}
      <section class="job" aria-label={t("files.progress")} aria-busy="true">
        <p role="status">{job.phase === "decoding" ? t("files.decoding") : job.phase === "cancelling" ? t("files.cancelling") : t("files.chunk", { n: job.progress?.chunk ?? 0, total: job.progress?.chunks ?? 0, percent: job.progress?.percent ?? 0 })}</p>
        {#if job.progress}<progress aria-label={t("files.progress")} max="100" value={job.progress.percent}></progress>{/if}
        <Button disabled={job.phase === "cancelling"} {busy} onclick={cancel}>{t("common.cancel")}</Button>
      </section>
    {:else}
      <div><Button variant="primary" disabled={!job.name || !activeProfile} {busy} onclick={start}>{t("files.start")}</Button></div>
      {#if job.phase === "cancelled"}<p role="status">{t("files.cancelled")}</p>{/if}
    {/if}
    {#if job.result}
      <section class="result" aria-label={t("files.result")}>
        <h2>{t("files.result")}</h2>
        <p class="muted">{t("files.duration", { duration: formatClock(job.result.duration_ms) })}</p>
        {#if !job.result.timestamps_available}<Banner tone="info">{t("files.no_timestamps")}</Banner>{/if}
        {#if job.result.cleanup_failed}<Banner tone="warning">{t("files.cleanup_failed")}</Banner>{/if}
        <Select label={t("files.format")} value={format} options={formats} onchange={(value) => format = value as FileOutputFormat} />
        {#if format === "txt"}<Switch checked={prefixes} disabled={!job.result.timestamps_available} label={t("files.prefixes")} onchange={(value) => prefixes = value} />{/if}
        <Field label={t("files.preview")}>
          {#snippet children(id)}<textarea {id} readonly value={preview} rows="12" spellcheck="false" aria-busy={rendering}></textarea>{/snippet}
        </Field>
        <div class="actions">
          <Button disabled={rendering} {busy} onclick={save}>{t("files.save")}</Button>
          <Button disabled={rendering} {busy} onclick={copy}><Icon name="copy" />{t("common.copy")}</Button>
          <Button {busy} onclick={askDiscard}>{t("files.discard")}</Button>
        </div>
      </section>
    {/if}
  {/if}
</div>

<Dialog bind:open={discardOpen} title={t("files.discard_title")}>
  <p>{t("files.discard_body")}</p>
  {#snippet actions()}
    <Button onclick={() => discardOpen = false}>{t("common.cancel")}</Button>
    <Button variant="danger" {busy} onclick={discard}>{t("files.discard")}</Button>
  {/snippet}
</Dialog>

<style>
  .files, .result, fieldset { display: flex; flex-direction: column; gap: var(--space-4); }
  .files :global(header) { margin-bottom: 0; }
  .source, .actions { display: flex; align-items: center; gap: var(--space-3); flex-wrap: wrap; }
  .filename { overflow-wrap: anywhere; }
  fieldset { border: 0; padding: 0; margin: 0; min-width: 0; }
  legend { font-size: var(--text-lg); font-weight: 600; padding: 0; margin-bottom: var(--space-3); }
  .job, .result { border-top: 1px solid var(--border); padding-top: var(--space-4); }
  .job p { margin-bottom: var(--space-3); }
  progress { width: 100%; accent-color: var(--accent); margin-bottom: var(--space-3); }
  textarea { width: 100%; min-height: 12rem; resize: vertical; font-family: var(--font-mono); }
</style>
