<script lang="ts">
  import type { Snippet } from "svelte";
  import InfoTip from "./InfoTip.svelte";

  interface Props {
    label: string;
    /** Extra information, shown in a tooltip on the label. */
    info?: string | null;
    error?: string | null;
    id?: string;
    /** Receives the control's id and the value for its aria-describedby (info and error, if any). */
    children: Snippet<[string, string | undefined]>;
  }
  const generatedId = $props.id();
  let { label, info, error, id = generatedId, children }: Props = $props();
  const describedBy = $derived([info ? `${id}-info` : "", error ? `${id}-error` : ""].filter(Boolean).join(" ") || undefined);
</script>

<div class="field">
  <InfoTip text={info} id={`${id}-info`}><label for={id}>{label}</label></InfoTip>
  {@render children(id, describedBy)}
  {#if error}<p id={`${id}-error`} class="error" role="alert">{error}</p>{/if}
</div>

<style>
  .field { display: flex; flex-direction: column; gap: var(--space-1); }
  label { font-weight: 600; }
  .error { color: var(--danger); font-size: var(--text-sm); }
</style>
