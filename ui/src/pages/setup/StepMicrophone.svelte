<script lang="ts">
  import { untrack } from "svelte";
  import { api } from "../../lib/api";
  import { app, errorText, saveConfig, t } from "../../lib/app.svelte";
  import Banner from "../../lib/components/Banner.svelte";
  import Button from "../../lib/components/Button.svelte";
  import Icon from "../../lib/components/Icon.svelte";
  import MicLevelTest from "../../lib/components/MicLevelTest.svelte";
  import Select from "../../lib/components/Select.svelte";

  interface Props {
    onnext: () => void;
    canNext?: boolean;
    form: string;
  }
  let { onnext, canNext = $bindable(false), form }: Props = $props();

  let mics = $state<string[]>([]);
  let micsBusy = $state(false);
  let micsError = $state<string | null>(null);
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let micRequest = 0;
  let disposed = false;

  // Choosing a microphone is optional: the system default works.
  $effect(() => {
    canNext = true;
  });

  async function loadMics() {
    if (disposed) return;
    const request = ++micRequest;
    micsBusy = true;
    micsError = null;
    try {
      const list = await api.listMicrophones();
      if (!disposed && request === micRequest) mics = [...new Set(list)];
    } catch (error) {
      if (!disposed && request === micRequest) micsError = errorText(error);
    } finally {
      if (!disposed && request === micRequest) micsBusy = false;
    }
  }

  $effect(() => {
    untrack(() => void loadMics());
    return () => {
      disposed = true;
    };
  });

  const options = $derived.by(() => {
    const list = [{ value: "", label: t("common.system_default") }, ...mics.map((m) => ({ value: m, label: m }))];
    const configured = app.config?.recording.microphone ?? null;
    if (configured !== null && !mics.includes(configured)) {
      list.push({ value: configured, label: t("settings.mic_missing", { name: configured }) });
    }
    return list;
  });

  function selectedMic(): string {
    // Re-read after a failed save, so the list shows the stored choice again.
    void saving;
    return app.config?.recording.microphone ?? "";
  }

  async function setMic(value: string) {
    if (disposed || value === selectedMic()) return;
    saving = true;
    saveError = null;
    try {
      await saveConfig((c) => {
        c.recording.microphone = value === "" ? null : value;
      });
    } catch (error) {
      if (!disposed) saveError = errorText(error);
    } finally {
      if (!disposed) saving = false;
    }
  }

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (canNext) onnext();
  }
</script>

<div class="mic-row">
  <div class="grow">
    <Select label={t("settings.mic")} {options} disabled={saving}
      bind:value={selectedMic, (value) => void setMic(value)} />
  </div>
  <Button variant="ghost" busy={micsBusy} aria-label={t("settings.refresh")} title={t("settings.refresh")}
    onclick={() => void loadMics()}>
    <Icon name="retry" />
  </Button>
</div>
{#if saveError}<Banner tone="error">{saveError}</Banner>{/if}
{#if micsError}<Banner tone="error">{micsError}</Banner>{/if}
<p class="muted">{t("setup.mic.body")}</p>
<MicLevelTest device={app.config?.recording.microphone ?? null} />
<form id={form} onsubmit={submit}></form>

<style>
  .mic-row { display: flex; align-items: flex-end; gap: var(--space-2); }
  .grow { flex: 1; min-width: 0; }
</style>
