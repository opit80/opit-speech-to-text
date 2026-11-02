<script lang="ts">
  import InfoTip from "./InfoTip.svelte";

  interface Props {
    checked: boolean;
    label: string;
    /** Extra information, shown in a tooltip on the label. */
    info?: string | null;
    disabled?: boolean;
    /** Accessible name when the visible label alone is ambiguous; it must start with the label. */
    ariaLabel?: string;
    onchange?: (checked: boolean) => void;
  }
  let { checked = $bindable(), label, info, disabled = false, ariaLabel, onchange }: Props = $props();
  const id = $props.id();
</script>

<div class="switch" class:disabled>
  <input {id} type="checkbox" role="switch" bind:checked {disabled} aria-label={ariaLabel}
    aria-describedby={info ? `${id}-info` : undefined}
    onchange={(event) => { checked = event.currentTarget.checked; onchange?.(checked); }} />
  <span class="track" aria-hidden="true"></span>
  <InfoTip text={info} id={`${id}-info`}><label for={id}>{label}</label></InfoTip>
</div>

<style>
  .switch { display: flex; position: relative; align-items: center; gap: var(--space-3); }
  input { position: absolute; width: 40px; height: 22px; opacity: 0; margin: 0; min-height: 0; cursor: pointer; }
  .track { flex: 0 0 40px; height: 22px; border: 1px solid var(--text-muted); border-radius: 22px; background: var(--surface-2); padding: 3px; }
  .track::before { content: ""; display: block; width: 14px; height: 14px; border-radius: 50%; background: var(--text-muted); transition: transform 150ms ease-out; }
  input:checked + .track { background: var(--accent); border-color: var(--accent); }
  input:checked + .track::before { transform: translateX(18px); background: var(--accent-text); }
  input:focus-visible + .track { outline: 2px solid var(--focus); outline-offset: 2px; }
  label { cursor: pointer; }
  .disabled { opacity: 0.6; }
  .disabled input, .disabled label { cursor: default; }
</style>
