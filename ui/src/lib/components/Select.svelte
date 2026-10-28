<script lang="ts">
  import Field from "./Field.svelte";
  interface Props {
    value: string;
    options: { value: string; label: string; disabled?: boolean }[];
    label: string;
    /** Extra information, shown in a tooltip on the label. */
    info?: string | null;
    disabled?: boolean;
    onchange?: (value: string) => void;
  }
  let { value = $bindable(), options, label, info, disabled = false, onchange }: Props = $props();
</script>

<Field {label} {info}>
  {#snippet children(id, describedBy)}
    <select {id} bind:value {disabled} aria-describedby={describedBy}
      onchange={(event) => { value = event.currentTarget.value; onchange?.(value); }}>
      {#each options as option (option.value)}<option value={option.value} disabled={option.disabled}>{option.label}</option>{/each}
    </select>
  {/snippet}
</Field>

<style>
  select { width: 100%; }
  select:disabled { opacity: 0.6; }
</style>
