<script lang="ts">
  import type { Snippet } from "svelte";
  interface Props {
    open: boolean;
    title: string;
    children: Snippet;
    actions?: Snippet;
    onclose?: () => void;
  }
  let { open = $bindable(), title, children, actions, onclose }: Props = $props();
  let dialog = $state<HTMLDialogElement>();
  const id = $props.id();

  $effect(() => {
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    else if (!open && dialog.open) dialog.close();
  });

  function closed() {
    open = false;
    onclose?.();
  }
</script>

<dialog bind:this={dialog} aria-labelledby={id} onclose={closed}>
  <h2 {id}>{title}</h2>
  <div class="body">{@render children()}</div>
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</dialog>

<style>
  dialog { width: min(520px, calc(100% - var(--space-6))); max-height: calc(100% - var(--space-6)); padding: var(--space-5); background: var(--surface); color: var(--text); border: 1px solid var(--border); border-radius: var(--radius); box-shadow: var(--shadow); }
  dialog::backdrop { background: var(--text); opacity: 0.35; }
  .body { margin-top: var(--space-4); }
  .actions { display: flex; justify-content: flex-end; gap: var(--space-2); margin-top: var(--space-5); }
</style>
