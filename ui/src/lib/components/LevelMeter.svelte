<script lang="ts">
  import { t } from "../app.svelte";
  // Speech RMS sits around 0.05–0.25, so the bar scales the level by 4.
  let { level }: { level: number } = $props();
  const percent = $derived(Math.round(Math.max(0, Math.min(100, level * 400))));
</script>

<div class="meter" role="meter" aria-label={t("mic.level")} aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}>
  <div class="fill" style:width={`${percent}%`}></div>
</div>

<style>
  .meter { width: 100%; height: 8px; border-radius: var(--radius-sm); background: var(--surface-2); border: 1px solid var(--border); overflow: hidden; }
  .fill { height: 100%; background: var(--accent); transition: width 80ms linear; }
</style>
