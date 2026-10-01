<script lang="ts">
  import { untrack } from "svelte";
  import { api } from "../../lib/api";
  import { app, errorText, t } from "../../lib/app.svelte";
  import { onHistoryAdded } from "../../lib/events";
  import { formatCombo } from "../../lib/hotkey";
  import type { Dictation, DictationState } from "../../lib/types";
  import Banner from "../../lib/components/Banner.svelte";
  import Button from "../../lib/components/Button.svelte";
  import Icon from "../../lib/components/Icon.svelte";
  import StatusBadge from "../../lib/components/StatusBadge.svelte";

  interface Props {
    onnext: () => void;
    /** On this last step: whether a dictation has worked (the frame shows "Finish" instead of "Finish without trying"). */
    canNext?: boolean;
    form: string;
  }
  let { onnext, canNext = $bindable(false), form }: Props = $props();

  let result = $state<Dictation | null>(null);
  let resultError = $state<string | null>(null);
  // History off: a dictation that went from pasting back to idle has finished.
  let finishedWithoutHistory = $state(false);
  let startedHere = $state(false);
  let actionBusy = $state(false);
  let actionError = $state<string | null>(null);
  let clickedHere = false;
  let previous: DictationState = untrack(() => app.status.state);
  let latestRequest = 0;
  let disposed = false;

  const busyState = $derived(app.status.state === "transcribing" || app.status.state === "pasting");
  const worked = $derived(result?.status === "ok" || finishedWithoutHistory);

  $effect(() => {
    canNext = worked;
  });

  async function loadLatest() {
    if (disposed) return;
    const request = ++latestRequest;
    resultError = null;
    try {
      const rows = await api.historyRecent(1, null);
      if (!disposed && request === latestRequest && rows.length) result = rows[0];
    } catch (error) {
      if (!disposed && request === latestRequest) resultError = errorText(error);
    }
  }

  $effect(() => {
    let unlisten: (() => void) | undefined;
    onHistoryAdded(() => void loadLatest()).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    }).catch((error: unknown) => {
      if (!disposed) resultError = errorText(error);
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  $effect(() => {
    const state = app.status.state;
    untrack(() => {
      if (state === "recording" && previous !== "recording") {
        // Whether this recording came from the button here (clipboard) or from the shortcut (typed).
        startedHere = clickedHere;
        clickedHere = false;
        finishedWithoutHistory = false;
      }
      if (previous === "pasting" && state === "idle" && app.config?.history.enabled === false) {
        finishedWithoutHistory = true;
      }
      previous = state;
    });
  });

  async function toggle() {
    if (actionBusy || disposed) return;
    actionBusy = true;
    actionError = null;
    clickedHere = app.status.state !== "recording";
    try {
      await api.toggleDictation();
    } catch (error) {
      clickedHere = false;
      if (!disposed) actionError = errorText(error);
    } finally {
      if (!disposed) actionBusy = false;
    }
  }

  function transcript(item: Dictation): string {
    if (item.status === "empty") return t("dictation.empty");
    if (item.status === "hallucination") return t("dictation.hallucination");
    return item.text;
  }

  function submit(event: SubmitEvent) {
    event.preventDefault();
    onnext();
  }
</script>

{#if app.config}
  <p>{t("setup.try.body", { combo: formatCombo(app.config.hotkey.keys, app.lang) })}</p>
  {#if !app.config.hotkey.enabled}<p class="muted">{t("home.shortcut_off")}</p>{/if}
{/if}

<div class="row">
  <div role="status"><StatusBadge status={app.status} /></div>
</div>

<div class="stack">
  <div class="row">
    <Button busy={actionBusy} disabled={busyState} onclick={() => void toggle()}>
      {#if busyState}{t("home.working")}
      {:else if app.status.state === "recording"}<Icon name="stop" />{t("home.stop")}
      {:else}<Icon name="mic" />{t("setup.try.here")}{/if}
    </Button>
  </div>
  <p class="muted note">{t("setup.try.here_note")}</p>
</div>

{#if actionError}<Banner tone="error">{actionError}</Banner>{/if}
{#if app.status.state === "error"}
  <Banner tone="error">
    {#if app.status.message}<p>{app.status.message}</p>{/if}
    <p>{t("setup.try.failed")}</p>
  </Banner>
{/if}

{#if result}
  <div class="result stack" role="status">
    {#if result.status === "ok"}<p class="worked"><Icon name="check" />{t("setup.try.worked")}</p>{/if}
    <p class="transcript" class:muted={result.status !== "ok"}>{transcript(result)}</p>
  </div>
{:else if finishedWithoutHistory}
  <div class="result" role="status">
    <p class="worked"><Icon name="check" />{startedHere ? t("setup.try.clipboard") : t("setup.try.worked")}</p>
  </div>
{/if}
{#if resultError}<Banner tone="error">{resultError}</Banner>{/if}
<form id={form} onsubmit={submit}></form>

<style>
  .note { font-size: var(--text-sm); }
  .result { padding: var(--space-3) var(--space-4); border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface-2); }
  .worked { display: inline-flex; align-items: center; gap: var(--space-2); color: var(--success); font-weight: 600; }
  .transcript { overflow-wrap: anywhere; white-space: pre-wrap; }
</style>
