<script lang="ts">
  import { api } from "../api";
  import { app, errorText, t } from "../app.svelte";
  import { dictationBusy } from "../update";
  import Button from "./Button.svelte";

  let busy = $state(false);
  const dictating = $derived(dictationBusy(app.status.state));
  const debugBuild = $derived(app.info?.debug_build ?? false);

  async function install() {
    busy = true;
    app.updateError = null;
    try {
      await api.installUpdate(); // on success the app exits before this returns
    } catch (e) {
      // Shared: by now `installing` has replaced this button with a new one.
      app.updateError = errorText(e);
    } finally {
      busy = false;
    }
  }
</script>

<span class="install">
  <Button variant="primary" {busy} disabled={dictating || debugBuild || app.update.kind !== "available"}
    title={t("update.install_note")} onclick={() => void install()}>{t("update.install")}</Button>
  {#if dictating}<span class="muted">{t("update.busy")}</span>{/if}
  {#if app.updateError}<span class="error" role="alert">{app.updateError}</span>{/if}
</span>

<style>
  .install { display: inline-flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; }
  .error { color: var(--danger); font-size: var(--text-sm); }
</style>
