<script lang="ts">
  interface Props {
    checked: boolean;
    label: string;
    hint?: string;
    disabled?: boolean;
    onchange?: (checked: boolean) => void;
  }
  let { checked = $bindable(), label, hint, disabled = false, onchange }: Props = $props();
  const id = $props.id();
</script>

<label class="switch" class:disabled>
  <input type="checkbox" role="switch" bind:checked {disabled}
    aria-describedby={hint ? `${id}-hint` : undefined}
    onchange={(event) => { checked = event.currentTarget.checked; onchange?.(checked); }} />
  <span class="track" aria-hidden="true"></span>
  <span class="text">
    <span>{label}</span>
    {#if hint}<span id={`${id}-hint`} class="hint">{hint}</span>{/if}
  </span>
</label>

<style>
  .switch { display: flex; position: relative; align-items: center; gap: var(--space-3); cursor: pointer; }
  input { position: absolute; width: 40px; height: 22px; opacity: 0; margin: 0; min-height: 0; }
  .track { flex: 0 0 40px; height: 22px; border: 1px solid var(--text-muted); border-radius: 22px; background: var(--surface-2); padding: 3px; }
  .track::before { content: ""; display: block; width: 14px; height: 14px; border-radius: 50%; background: var(--text-muted); transition: transform 150ms ease-out; }
  input:checked + .track { background: var(--accent); border-color: var(--accent); }
  input:checked + .track::before { transform: translateX(18px); background: var(--accent-text); }
  input:focus-visible + .track { outline: 2px solid var(--focus); outline-offset: 2px; }
  .text { display: flex; flex-direction: column; gap: var(--space-1); }
  .hint { color: var(--text-muted); font-size: var(--text-sm); }
  .disabled { opacity: 0.6; cursor: default; }
</style>
