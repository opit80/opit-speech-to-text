<script lang="ts">
  import { t } from "../app.svelte";
  import { progressPercent } from "../update";

  let { downloaded, total }: { downloaded: number; total: number | null } = $props();
  const percent = $derived(progressPercent(downloaded, total));
</script>

{#if percent === null}
  <progress aria-label={t("update.progress")}></progress>
{:else}
  <progress max="100" value={percent} aria-label={t("update.progress")}></progress>
{/if}

<style>
  progress { width: 100%; }
</style>
