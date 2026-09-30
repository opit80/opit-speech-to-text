<script lang="ts">
  // Plan 2 skeleton: proves the invoke/event wiring. Plan 3 replaces this with the real pages.
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

  interface DictationStatus {
    state: "idle" | "recording" | "transcribing" | "pasting" | "cancelled" | "error";
    can_retry: boolean;
    kind?: string;
    message?: string;
  }

  interface AppInfo {
    version: string;
    data_dir: string;
    log_dir: string;
  }

  let status = $state<DictationStatus | null>(null);
  let info = $state<AppInfo | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    let disposed = false;
    const unlisten = listen<DictationStatus>("dictation-status", (event) => {
      status = event.payload;
    });
    Promise.all([invoke<DictationStatus>("get_status"), invoke<AppInfo>("app_info")])
      .then(([s, i]) => {
        if (disposed) return;
        status ??= s;
        info = i;
      })
      .catch((e: unknown) => {
        error = String(e);
      });
    return () => {
      disposed = true;
      void unlisten.then((stop) => stop());
    };
  });

  function toggle() {
    invoke("toggle_dictation").catch((e: unknown) => {
      error = String(e);
    });
  }
</script>

<main>
  <h1>Opit Speech to Text</h1>
  {#if error}
    <p class="error">{error}</p>
  {/if}
  {#if status}
    <p>State: <strong>{status.state}</strong>{status.message ? ` — ${status.message}` : ""}</p>
    <button type="button" onclick={toggle}>{status.state === "recording" ? "Stop" : "Start"} dictation</button>
  {/if}
  {#if info}
    <p class="muted">v{info.version} · data: {info.data_dir}</p>
  {/if}
  <p class="muted">The full interface arrives in a later version. Settings live in config.json in the data folder.</p>
</main>

<style>
  main {
    font-family: system-ui, sans-serif;
    padding: 1.5rem;
  }
  .error {
    color: #b91c1c;
  }
  .muted {
    color: #6b7280;
    font-size: 0.875rem;
  }
</style>
