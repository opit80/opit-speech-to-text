<script lang="ts">
  import CleanupSettings from "../../lib/components/CleanupSettings.svelte";
  let { onnext, canNext = $bindable(false), form }: { onnext: () => void; canNext?: boolean; form: string } = $props();
</script>

<CleanupSettings wizard formId={form} onsaved={onnext} bind:canNext />
