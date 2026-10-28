<script lang="ts">
  import type { Snippet } from "svelte";
  import InfoTip from "./InfoTip.svelte";
  interface Props { title: string; info?: string; actions?: Snippet; }
  let { title, info, actions }: Props = $props();
</script>

<header>
  <h1><InfoTip text={info}>{title}</InfoTip></h1>
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</header>

<style>
  header { display: flex; align-items: flex-start; justify-content: space-between; gap: var(--space-4); margin-bottom: var(--space-5); }
  .actions { display: flex; align-items: center; gap: var(--space-2); flex-shrink: 0; }
</style>
