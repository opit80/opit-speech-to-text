<script lang="ts">
  import Field from "./Field.svelte";
  interface Props {
    value: string;
    options: { value: string; label: string; disabled?: boolean }[];
    label: string;
    hint?: string;
    disabled?: boolean;
    onchange?: (value: string) => void;
  }
  let { value = $bindable(), options, label, hint, disabled = false, onchange }: Props = $props();
</script>

<Field {label} {hint}>
  {#snippet children(id)}
    <select {id} bind:value {disabled} aria-describedby={hint ? `${id}-description` : undefined}
      onchange={(event) => { value = event.currentTarget.value; onchange?.(value); }}>
      {#each options as option (option.value)}<option value={option.value} disabled={option.disabled}>{option.label}</option>{/each}
    </select>
  {/snippet}
</Field>

<style>
  select { width: 100%; }
  select:disabled { opacity: 0.6; }
</style>
