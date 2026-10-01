<script lang="ts">
  import type { Snippet } from "svelte";
  interface Props { title: string; description?: string; actions?: Snippet; }
  let { title, description, actions }: Props = $props();
</script>

<header>
  <div><h1>{title}</h1>{#if description}<p>{description}</p>{/if}</div>
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</header>

<style>
  header { display: flex; align-items: flex-start; justify-content: space-between; gap: var(--space-4); margin-bottom: var(--space-5); }
  p { color: var(--text-muted); margin-top: var(--space-2); max-width: 70ch; }
  .actions { display: flex; align-items: center; gap: var(--space-2); flex-shrink: 0; }
</style>
