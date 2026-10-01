<script lang="ts">
  import { untrack } from "svelte";
  import { api } from "../api";
  import { errorText, t } from "../app.svelte";
  import { onMicTest, type UnlistenFn } from "../events";
  import type { MicTestEvent } from "../types";
  import Banner from "./Banner.svelte";
  import Button from "./Button.svelte";
  import Icon from "./Icon.svelte";
  import LevelMeter from "./LevelMeter.svelte";

  let { device }: { device: string | null } = $props();

  const UI_LIMIT_MS = 15_000;
  type Verdict = "silent" | "quiet" | "good";

  let starting = $state(false);
  let running = $state(false);
  let shown = $state(0);
  let using = $state<string | null>(null);
  let fellBack = $state(false);
  let verdict = $state<Verdict | null>(null);
  let error = $state<string | null>(null);
  let peak = 0;
  // Every start, stop and unmount bumps this; results of an older start are ignored, which also
  // hides the "the microphone test was stopped" rejection that follows our own stop.
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let subscription: Promise<UnlistenFn> | undefined;
  let disposed = false;

  function handle(event: MicTestEvent) {
    if (disposed || !(starting || running)) return;
    if (event.kind === "level") {
      shown = Math.max(event.value, shown * 0.85);
      peak = Math.max(peak, event.value);
    } else {
      error = errorText({ code: "unavailable", message: event.message, kind: "microphone" });
      halt();
      void api.micTestStop().catch(() => {});
    }
  }

  /** Ends the current test locally; returns whether it had been running (so a verdict applies). */
  function halt(): boolean {
    const wasRunning = running;
    generation++;
    clearTimeout(timer);
    timer = undefined;
    starting = false;
    running = false;
    shown = 0;
    return wasRunning;
  }

  async function start() {
    if (starting || running || disposed) return;
    const gen = ++generation;
    error = null;
    verdict = null;
    using = null;
    fellBack = false;
    shown = 0;
    peak = 0;
    starting = true;
    try {
      await subscription;
      if (gen !== generation || disposed) return;
      const opened = await api.micTestStart(device);
      if (gen !== generation || disposed) return;
      using = opened;
      fellBack = device !== null && opened !== device;
      starting = false;
      running = true;
      timer = setTimeout(() => {
        if (gen === generation) void stop();
      }, UI_LIMIT_MS);
    } catch (e) {
      if (gen === generation && !disposed) {
        error = errorText(e);
        starting = false;
      }
    }
  }

  async function stop() {
    if (!starting && !running) return;
    const wasRunning = halt();
    if (wasRunning) verdict = peak < 0.01 ? "silent" : peak < 0.05 ? "quiet" : "good";
    try {
      await api.micTestStop();
    } catch (e) {
      if (!disposed) error = errorText(e);
    }
  }

  $effect(() => {
    let unlisten: UnlistenFn | undefined;
    subscription = onMicTest(handle);
    subscription.then((stopListening) => {
      if (disposed) stopListening();
      else unlisten = stopListening;
    }).catch(() => {
      // start() awaits the same promise and shows the error.
    });
    return () => {
      disposed = true;
      unlisten?.();
      if (starting || running) {
        halt();
        void api.micTestStop().catch(() => {});
      }
    };
  });

  // A different microphone was chosen: the running test no longer measures it.
  $effect(() => {
    void device;
    untrack(() => void stop());
  });

  const verdictKey = { silent: "mic.silent", quiet: "mic.quiet", good: "mic.good" } as const;
</script>

<div class="stack mic-test">
  <div class="row">
    <!-- One button for both actions, so keyboard focus stays on it. -->
    <Button busy={starting} onclick={() => void (running ? stop() : start())}>
      {#if running}<Icon name="stop" />{t("mic.stop")}{:else}<Icon name="mic" />{t("mic.test")}{/if}
    </Button>
    {#if running}<span class="muted" role="status">{t("mic.speak")}</span>{/if}
  </div>
  {#if running || starting}<LevelMeter level={shown} />{/if}
  {#if using}<p class="muted using">{t("mic.using", { device: using })}</p>{/if}
  {#if fellBack}<Banner tone="warning">{t("mic.fell_back")}</Banner>{/if}
  {#if verdict}
    <Banner tone={verdict === "good" ? "success" : "warning"}>{t(verdictKey[verdict])}</Banner>
  {/if}
  {#if error}<Banner tone="error">{error}</Banner>{/if}
</div>

<style>
  .using { font-size: var(--text-sm); overflow-wrap: anywhere; }
</style>
