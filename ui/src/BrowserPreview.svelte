<script lang="ts">
  import App from "./App.svelte";
  import { t } from "./lib/app.svelte";
</script>

<div class="preview-shell">
  <div class="preview-notice" role="status">{t("preview.notice")}</div>
  <App />
</div>

<style>
  .preview-notice { padding: var(--space-2) var(--space-4); color: var(--text-muted); background: var(--info-bg); border-bottom: 1px solid var(--border); font-size: var(--text-sm); }
  .preview-shell { height: 100vh; display: flex; flex-direction: column; }
  .preview-shell :global(.shell) { height: auto; min-height: 0; flex: 1; }
</style>
