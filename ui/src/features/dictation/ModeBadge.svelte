<script lang="ts">
  import { t } from "../../lib/state/app.svelte";
  import type { DictationMode } from "../../lib/bridge/types";
  import { modeBadgeKey } from "./voice-modes";
  /** `model`: the row's effective AI model, shown as the tooltip. */
  let { mode, model = null }: { mode: DictationMode; model?: string | null } = $props();
  const key = $derived(modeBadgeKey(mode));
</script>

{#if key}<span class="mode" title={model ?? undefined}>{t(key)}</span>{/if}

<style>
  .mode { display: inline-block; font-size: var(--text-sm); font-weight: 600; color: var(--text); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 0 var(--space-2); }
</style>
