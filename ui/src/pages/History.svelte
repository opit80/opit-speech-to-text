<script lang="ts">
  import { api } from "../lib/api";
  import { app, errorText, t } from "../lib/app.svelte";
  import { onHistoryAdded } from "../lib/events";
  import { formatClock, formatDateTime, formatSeconds } from "../lib/format";
  import { expandToWords } from "../lib/selection";
  import { toast } from "../lib/toast.svelte";
  import type { Dictation } from "../lib/types";
  import Banner from "../lib/components/Banner.svelte";
  import Button from "../lib/components/Button.svelte";
  import Dialog from "../lib/components/Dialog.svelte";
  import Field from "../lib/components/Field.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import CorrectionDialog from "./CorrectionDialog.svelte";

  let query = $state("");
  let rows = $state<Dictation[]>([]);
  let listBusy = $state(true);
  let listError = $state<string | null>(null);
  let liveError = $state<string | null>(null);
  let moreBusy = $state(false);
  let moreError = $state<string | null>(null);
  let hasMore = $state(false);
  let copyingId = $state<number | null>(null);
  let copyError = $state<string | null>(null);
  let audioBusy = $state(false);
  let audioError = $state<string | null>(null);
  let audio = $state<HTMLAudioElement>();
  let audioId: number | null = null;
  let audioUrl = $state<string | null>(null);
  let audioRequest = 0;
  let deleteOpen = $state(false);
  let deleteTarget = $state<Dictation | null>(null);
  let clearOpen = $state(false);
  let clearCount = $state(0);
  let mutationBusy = $state(false);
  let mutationError = $state<string | null>(null);
  let correctionOpen = $state(false);
  let correctionTarget = $state<Dictation | null>(null);
  let variant = $state("");
  let disposed = false;
  let generation = 0;
  let listRequest = 0;
  let refreshPending = false;

  function merge(newRows: Dictation[], oldRows: Dictation[]): Dictation[] {
    return [...new Map([...oldRows, ...newRows].map((row) => [row.id, row])).values()]
      .sort((a, b) => b.id - a.id);
  }

  async function load(search: string, epoch: number, live = false) {
    if (disposed || mutationBusy) return;
    const request = ++listRequest;
    listBusy = true;
    listError = null;
    try {
      const page = search ? await api.historySearch(search, 100) : await api.historyRecent(50, null);
      if (disposed || epoch !== generation || request !== listRequest) return;
      if (!live || rows.length === 0) hasMore = !search && page.length === 50;
      rows = live ? merge(page, rows) : page;
    } catch (error) {
      if (!disposed && epoch === generation && request === listRequest) listError = errorText(error);
    } finally {
      if (!disposed && epoch === generation && request === listRequest) listBusy = false;
    }
  }

  $effect(() => {
    const search = query;
    const epoch = ++generation;
    rows = [];
    hasMore = false;
    moreBusy = false;
    moreError = null;
    listBusy = true;
    listError = null;
    const timer = setTimeout(() => void load(search, epoch, !search), 250);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    let unlisten: (() => void) | undefined;
    onHistoryAdded(() => {
      if (query || disposed) return;
      if (mutationBusy) refreshPending = true;
      else void load("", generation, true);
    }).then((stop) => {
      if (disposed) stop();
      else {
        unlisten = stop;
        // Catch any dictation completed while the subscription was being established.
        if (!query) void load("", generation, true);
      }
    }).catch((error: unknown) => {
      if (!disposed) liveError = errorText(error);
    });
    return () => {
      disposed = true;
      unlisten?.();
      stopAudio();
    };
  });

  async function loadMore() {
    if (disposed || query || listBusy || moreBusy || mutationBusy || !hasMore || !rows.length) return;
    const epoch = generation;
    moreBusy = true;
    moreError = null;
    try {
      const page = await api.historyRecent(50, rows[rows.length - 1].id);
      if (!disposed && epoch === generation) {
        rows = merge(page, rows);
        hasMore = page.length === 50;
      }
    } catch (error) {
      if (!disposed && epoch === generation) moreError = errorText(error);
    } finally {
      if (!disposed && epoch === generation) moreBusy = false;
    }
  }

  async function copy(row: Dictation) {
    if (disposed || copyingId !== null) return;
    copyingId = row.id;
    copyError = null;
    try {
      await navigator.clipboard.writeText(row.text);
      if (!disposed) toast(t("common.copied"));
    } catch (error) {
      if (!disposed) copyError = errorText(error);
    } finally {
      if (!disposed) copyingId = null;
    }
  }

  function stopAudio() {
    ++audioRequest;
    audio?.pause();
    audio?.removeAttribute("src");
    audio?.load();
    if (audioUrl) URL.revokeObjectURL(audioUrl);
    audioUrl = null;
    audioId = null;
    audioBusy = false;
  }

  function playbackError(error: unknown) {
    if (disposed) return;
    audioError = errorText(error);
    toast(audioError, "error");
  }

  async function play(row: Dictation) {
    if (disposed || mutationBusy || !audio) return;
    stopAudio();
    const request = audioRequest;
    audioId = row.id;
    audioBusy = true;
    audioError = null;
    try {
      const bytes = await api.historyAudio(row.id);
      if (disposed || request !== audioRequest) return;
      audioUrl = URL.createObjectURL(new Blob([bytes], { type: "audio/wav" }));
      audio.src = audioUrl;
      await audio.play();
    } catch (error) {
      if (!disposed && request === audioRequest) playbackError(error);
    } finally {
      if (!disposed && request === audioRequest) audioBusy = false;
    }
  }

  function addCorrection(row: Dictation, button: HTMLElement) {
    const text = button.closest("li")?.querySelector("[data-transcript]");
    const selection = window.getSelection();
    variant = "";
    if (text && selection && !selection.isCollapsed && selection.anchorNode && selection.focusNode
      && text.contains(selection.anchorNode) && text.contains(selection.focusNode)) {
      // A range can end on the element itself at either side of its single text node.
      const start = selection.anchorNode === text ? (selection.anchorOffset ? row.text.length : 0) : selection.anchorOffset;
      const end = selection.focusNode === text ? (selection.focusOffset ? row.text.length : 0) : selection.focusOffset;
      const range = expandToWords(row.text, start, end);
      variant = row.text.slice(range.start, range.end);
    }
    correctionTarget = row;
    correctionOpen = true;
  }

  async function remove(clear: boolean) {
    if (disposed || mutationBusy || (!clear && !deleteTarget)) return;
    const target = deleteTarget;
    mutationBusy = true;
    mutationError = null;
    ++generation;
    ++listRequest;
    listBusy = false;
    moreBusy = false;
    try {
      if (clear) await api.historyClear();
      else if (target) await api.historyDelete(target.id);
      if (disposed) return;
      if (clear) {
        rows = [];
        hasMore = false;
        clearOpen = false;
        stopAudio();
        toast(t("history.cleared"));
      } else if (target) {
        rows = rows.filter((row) => row.id !== target.id);
        deleteOpen = false;
        if (audioId === target.id) stopAudio();
      }
    } catch (error) {
      if (!disposed) mutationError = errorText(error);
    } finally {
      if (!disposed) {
        mutationBusy = false;
        if (refreshPending && !query) void load("", generation, true);
        refreshPending = false;
      }
    }
  }
</script>

<div>
  <PageHeader title={t("history.title")}>
    {#snippet actions()}
      <Button variant="danger" disabled={mutationBusy || listBusy} onclick={() => {
        clearCount = rows.length; mutationError = null; clearOpen = true;
      }}>{t("history.clear")}</Button>
    {/snippet}
  </PageHeader>
  <div class="stack">
    <Field label={t("history.search_label")}>
      {#snippet children(id)}
        <input {id} type="search" bind:value={query} placeholder={t("history.search")} disabled={mutationBusy}
          onkeydown={(event) => { if (event.key === "Escape") { event.preventDefault(); query = ""; } }} />
      {/snippet}
    </Field>
    {#if app.config?.history.enabled === false}<Banner tone="info">{t("history.off")}</Banner>{/if}
    <section class="card stack" aria-label={t("history.title")} aria-busy={listBusy}>
      {#if listError}
        <Banner tone="error">{listError}
          {#snippet action()}<Button disabled={mutationBusy} busy={listBusy} onclick={() => void load(query, generation, !query)}>{t("common.retry")}</Button>{/snippet}
        </Banner>
      {/if}
      {#if copyError}<Banner tone="error">{copyError}</Banner>{/if}
      {#if liveError}<Banner tone="error">{liveError}</Banner>{/if}
      {#if audioError}<Banner tone="error">{audioError}</Banner>{/if}
      {#if mutationError && !deleteOpen && !clearOpen}<Banner tone="error">{mutationError}</Banner>{/if}
      {#if listBusy && !rows.length}<p class="muted" role="status">{t("common.loading")}</p>
      {:else if !rows.length && !listError}<p class="muted">{query ? t("history.no_results", { query }) : t("history.empty")}</p>{/if}
      <ul>
        {#each rows as row (row.id)}
          <li>
            <div class="metadata muted">
              <time datetime={new Date(row.created_at_ms).toISOString()}>{formatDateTime(row.created_at_ms, app.lang)}</time>
              <span>{row.profile_id}</span>
              <span>{t("history.latency", { seconds: formatSeconds(row.latency_ms, app.lang) })}</span>
              <span>{t("history.audio_length", { duration: formatClock(row.audio_ms) })}</span>
            </div>
            <!-- A caret enables keyboard selection; beforeinput keeps the single text node read-only. -->
            <div class="transcript" data-transcript contenteditable="plaintext-only" role="textbox" aria-readonly="true"
              aria-label={t("history.transcript")} onbeforeinput={(event) => event.preventDefault()}>{row.text}</div>
            {#if row.status !== "ok"}<p class="muted">{t(row.status === "empty" ? "dictation.empty" : "dictation.hallucination")}</p>{/if}
            {#if row.raw_text !== row.text}
              <details><summary>{t("history.show_original")}</summary><p class="transcript">{row.raw_text}</p></details>
            {/if}
            <div class="row actions">
              <Button variant="ghost" title={t("common.copy")} aria-label={t("common.copy")}
                busy={copyingId === row.id} disabled={copyingId !== null} onclick={() => void copy(row)}><Icon name="copy" /></Button>
              {#if row.audio_path}
                <Button variant="ghost" title={t("history.play")} aria-label={t("history.play")}
                  disabled={mutationBusy} onclick={() => void play(row)}><Icon name="play" /></Button>
              {/if}
              <Button variant="ghost" title={t("history.add_correction")} aria-label={t("history.add_correction")}
                disabled={mutationBusy} onpointerdown={(event) => event.preventDefault()}
                onclick={(event) => addCorrection(row, event.currentTarget)}><Icon name="wand" /></Button>
              <Button variant="ghost" title={t("common.delete")} aria-label={t("common.delete")} disabled={mutationBusy}
                onclick={() => { deleteTarget = row; mutationError = null; deleteOpen = true; }}><Icon name="trash" /></Button>
            </div>
          </li>
        {/each}
      </ul>
      {#if audioBusy}<p class="muted" role="status">{t("common.loading")}</p>{/if}
      <audio bind:this={audio} controls hidden={!audioUrl} aria-label={t("history.play")} onerror={() => {
        if (audioUrl) playbackError({ code: "provider", kind: "audio", message: "" });
      }}></audio>
      {#if moreError}<Banner tone="error">{moreError}</Banner>{/if}
      {#if !query && hasMore}
        <div><Button busy={moreBusy} disabled={listBusy || mutationBusy} onclick={() => void loadMore()}>{t("history.load_more")}</Button></div>
      {/if}
    </section>
  </div>
</div>

<Dialog bind:open={deleteOpen} title={t("history.delete_title")}>
  <div class="stack"><p>{t("history.delete_body")}</p>{#if mutationError}<Banner tone="error">{mutationError}</Banner>{/if}</div>
  {#snippet actions()}
    <Button disabled={mutationBusy} onclick={() => deleteOpen = false}>{t("common.cancel")}</Button>
    <Button variant="danger" busy={mutationBusy} onclick={() => void remove(false)}>{t("common.delete")}</Button>
  {/snippet}
</Dialog>
<Dialog bind:open={clearOpen} title={t("history.clear_title")}>
  <div class="stack">
    <p>{t("history.shown_count", { count: clearCount })}</p><p>{t("history.clear_body")}</p>
    {#if mutationError}<Banner tone="error">{mutationError}</Banner>{/if}
  </div>
  {#snippet actions()}
    <Button disabled={mutationBusy} onclick={() => clearOpen = false}>{t("common.cancel")}</Button>
    <Button variant="danger" busy={mutationBusy} onclick={() => void remove(true)}>{t("history.clear")}</Button>
  {/snippet}
</Dialog>
<CorrectionDialog bind:open={correctionOpen} dictation={correctionTarget} {variant} />

<style>
  input { width: 100%; }
  ul { list-style: none; margin: 0; padding: 0; }
  li { padding: var(--space-4) 0; }
  li:first-child { padding-top: 0; }
  li + li { border-top: 1px solid var(--border); }
  .metadata { display: flex; gap: var(--space-3); flex-wrap: wrap; font-size: var(--text-sm); margin-bottom: var(--space-2); }
  .transcript { white-space: pre-wrap; overflow-wrap: anywhere; }
  details { margin-top: var(--space-2); }
  summary { cursor: pointer; color: var(--text-muted); }
  details p { margin-top: var(--space-2); }
  .actions { justify-content: flex-end; margin-top: var(--space-2); }
  audio { width: 100%; }
</style>
