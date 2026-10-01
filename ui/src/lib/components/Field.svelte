<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    label: string;
    hint?: string;
    error?: string | null;
    id?: string;
    /** Set the control's id and use `${id}-description` for aria-describedby when hint/error exists. */
    children: Snippet<[string]>;
  }
  const generatedId = $props.id();
  let { label, hint, error, id = generatedId, children }: Props = $props();
</script>

<div class="field">
  <label for={id}>{label}</label>
  {@render children(id)}
  {#if hint || error}
    <div id={`${id}-description`} class="description">
      {#if hint}<p class="hint">{hint}</p>{/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
    </div>
  {/if}
</div>

<style>
  .field { display: flex; flex-direction: column; gap: var(--space-1); }
  label { font-weight: 600; }
  .description { display: flex; flex-direction: column; gap: var(--space-1); font-size: var(--text-sm); }
  .hint { color: var(--text-muted); }
  .error { color: var(--danger); }
</style>
