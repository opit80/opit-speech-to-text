<script lang="ts">
  import { app, t } from "../../lib/state/app.svelte";
  import { historyStyleName } from "./styles";
  import type { Dictation } from "../../lib/bridge/types";
  let { status, model, styleId = null }: {
    status: Dictation["cleanup_status"]; model: string | null;
    /** The row's `cleanup_style_id`; named only when it is not the default clean style. */
    styleId?: string | null;
  } = $props();
  const style = $derived(historyStyleName(styleId, app.config?.cleanup.styles ?? [], app.styleDefaults, app.lang));
  const label = $derived(t(status === "ok" ? "cleanup.cleaned" : "cleanup.failed"));
</script>

{#if status === "ok" || status === "failed"}
  <span class="marker" class:warning={status === "failed"} title={model ?? undefined}>
    {style ? t("cleanup.with_style", { status: label, style }) : label}
  </span>
{/if}

<style>
  .marker { display: inline-block; font-size: var(--text-sm); color: var(--success); }
  .warning { color: var(--warning); }
</style>
