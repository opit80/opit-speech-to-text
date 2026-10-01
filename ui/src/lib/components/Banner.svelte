<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "../app.svelte";
  import Icon from "./Icon.svelte";
  interface Props {
    tone: "info" | "warning" | "error" | "success";
    children: Snippet;
    action?: Snippet;
    ondismiss?: () => void;
  }
  let { tone, children, action, ondismiss }: Props = $props();
</script>

<div class="banner {tone}" role={tone === "warning" || tone === "error" ? "alert" : "status"}>
  <div class="message">{@render children()}</div>
  {#if action}<div class="action">{@render action()}</div>{/if}
  {#if ondismiss}
    <button type="button" class="dismiss" onclick={ondismiss} aria-label={t("common.dismiss")}><Icon name="x" /></button>
  {/if}
</div>

<style>
  .banner { display: flex; align-items: center; gap: var(--space-3); padding: var(--space-3) var(--space-4); border-radius: var(--radius); color: var(--text); }
  .info { background: var(--info-bg); }
  .warning { background: var(--warning-bg); }
  .error { background: var(--danger-bg); }
  .success { background: var(--success-bg); }
  .message { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .action { flex-shrink: 0; }
  .dismiss { display: inline-flex; padding: var(--space-1); border: 0; border-radius: var(--radius-sm); background: transparent; cursor: pointer; }
  .dismiss:hover { background: var(--surface-2); }
</style>
