<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";

  interface Props extends HTMLButtonAttributes {
    variant?: "primary" | "secondary" | "danger" | "ghost";
    busy?: boolean;
    children: Snippet;
  }
  let { variant = "secondary", busy = false, disabled = false, type = "button", children, ...rest }: Props = $props();
</script>

<button {...rest} {type} class={variant} disabled={disabled || busy} aria-busy={busy}>
  <span class="spinner" class:active={busy} aria-hidden="true"></span>
  <span class="label">{@render children()}</span>
</button>

<style>
  button {
    display: inline-grid;
    grid-template-columns: 12px auto 12px;
    align-items: center;
    gap: var(--space-2);
    min-height: 32px;
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    cursor: pointer;
  }
  button::after { content: ""; }
  .label { display: inline-flex; align-items: center; justify-content: center; gap: var(--space-2); }
  button:hover { background: var(--surface-2); }
  button:active { transform: translateY(1px); }
  .primary { background: var(--accent); color: var(--accent-text); border-color: var(--accent); }
  .primary:hover { background: var(--accent-hover); border-color: var(--accent-hover); }
  .danger { background: var(--danger); color: var(--accent-text); border-color: var(--danger); }
  .danger:hover { background: var(--danger-hover); border-color: var(--danger-hover); }
  .ghost { background: transparent; border-color: transparent; }
  button:disabled { opacity: 0.6; cursor: default; transform: none; }
  .spinner { width: 12px; height: 12px; border: 2px solid currentColor; border-right-color: transparent; border-radius: 50%; visibility: hidden; }
  .spinner.active { visibility: visible; animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
</style>
