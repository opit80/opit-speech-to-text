<script lang="ts">
  import { toasts } from "../toast.svelte";
</script>

<div class="toasts" aria-live="polite" aria-atomic="false">
  {#each toasts as item (item.id)}
    <div class="toast" class:error={item.tone === "error"}>{item.text}</div>
  {/each}
</div>

<style>
  .toasts { position: fixed; right: var(--space-5); bottom: var(--space-5); display: flex; flex-direction: column; gap: var(--space-2); max-width: min(400px, calc(100vw - var(--space-6))); pointer-events: none; z-index: 10; }
  .toast { background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius); box-shadow: var(--shadow); padding: var(--space-3) var(--space-4); overflow-wrap: anywhere; }
  .error { background: var(--danger-bg); }
</style>
