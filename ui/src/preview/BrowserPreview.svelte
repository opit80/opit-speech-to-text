<script lang="ts">
  import App from "../App.svelte";
</script>

<div class="preview-shell">
  <App />
</div>

<style>
  .preview-shell { height: 100vh; display: flex; flex-direction: column; }
  .preview-shell :global(.shell) { height: auto; min-height: 0; flex: 1; }
</style>
