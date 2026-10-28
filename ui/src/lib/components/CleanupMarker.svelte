<script lang="ts">
  import { t } from "../app.svelte";
  import type { Dictation } from "../types";
  let { status, model }: { status: Dictation["cleanup_status"]; model: string | null } = $props();
</script>

{#if status === "ok" || status === "failed"}
  <span class="marker" class:warning={status === "failed"} title={model ?? undefined}>
    {t(status === "ok" ? "cleanup.cleaned" : "cleanup.failed")}
  </span>
{/if}

<style>
  .marker { display: inline-block; font-size: var(--text-sm); color: var(--success); }
  .warning { color: var(--warning); }
</style>
