<script lang="ts">
  import { t } from "../app.svelte";
  import type { MessageKey } from "../i18n";
  import type { DictationState, DictationStatus } from "../types";
  let { status }: { status: DictationStatus } = $props();
  const keys: Record<DictationState, MessageKey> = {
    idle: "status.idle", recording: "status.recording", transcribing: "status.transcribing",
    pasting: "status.pasting", cancelled: "status.cancelled", error: "status.error",
  };
</script>

<span class="badge {status.state}" title={status.state === "error" ? status.message : undefined}>
  <span class="dot" aria-hidden="true"></span>{t(keys[status.state])}
</span>

<style>
  .badge { display: inline-flex; align-items: center; gap: var(--space-2); }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--text-muted); }
  .idle .dot { background: var(--success); }
  .recording .dot { background: var(--danger); animation: pulse 1.4s ease-in-out infinite; }
  .transcribing .dot, .pasting .dot { background: var(--accent); }
  .error .dot { background: var(--danger); }
  @keyframes pulse { 50% { opacity: 0.4; } }
</style>
