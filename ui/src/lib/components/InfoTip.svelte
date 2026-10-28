<script lang="ts">
  import { tick, type Snippet } from "svelte";
  import { t } from "../app.svelte";
  import { placeTooltip, type TooltipPlacement } from "../tooltip";
  import Icon from "./Icon.svelte";

  interface Props {
    /** The info text. Without it, only `children` is rendered. */
    text?: string | null;
    /** Id of the tooltip content, so a related control can also reference it with aria-describedby. */
    id?: string;
    /** The label or heading text the info belongs to. Keep `<label for>` elements inside it. */
    children: Snippet;
  }
  const generatedId = $props.id();
  let { text, id = `${generatedId}-tip`, children }: Props = $props();

  const SHOW_DELAY_MS = 150;
  const HIDE_DELAY_MS = 120;
  let open = $state(false);
  let placement = $state<TooltipPlacement | null>(null);
  let wrapper = $state<HTMLSpanElement>();
  let tip = $state<HTMLSpanElement>();
  let focused = false;
  let timer: ReturnType<typeof setTimeout> | undefined;

  function show() {
    clearTimeout(timer);
    if (!open) {
      placement = null;
      open = true;
    }
  }

  function hide() {
    clearTimeout(timer);
    open = false;
  }

  function later(action: () => void, delay: number) {
    clearTimeout(timer);
    timer = setTimeout(action, delay);
  }

  /** Fixed positioning keeps the box out of scroll containers' and dialogs' clipping. */
  async function place() {
    await tick();
    if (!open || !wrapper || !tip) return;
    const size = tip.getBoundingClientRect();
    placement = placeTooltip(wrapper.getBoundingClientRect(), { width: size.width, height: size.height },
      { width: window.innerWidth, height: window.innerHeight });
  }

  $effect(() => {
    if (!open) return;
    void place();
    const reposition = () => void place();
    window.addEventListener("scroll", reposition, true);
    window.addEventListener("resize", reposition);
    return () => {
      window.removeEventListener("scroll", reposition, true);
      window.removeEventListener("resize", reposition);
    };
  });

  $effect(() => () => clearTimeout(timer));

  function keydown(event: KeyboardEvent) {
    if (!open || event.key !== "Escape") return;
    // Only the tooltip closes, not a dialog around it.
    event.preventDefault();
    event.stopPropagation();
    hide();
  }

  /**
   * Hovering the text or the cue opens the box (touch uses the cue's click instead). The box is a
   * child, so moving the pointer onto it keeps it open. The wrapper itself is not interactive:
   * the cue button is the keyboard-reachable trigger.
   */
  function hoverable(node: HTMLElement) {
    const enter = (event: PointerEvent) => { if (event.pointerType !== "touch") later(show, SHOW_DELAY_MS); };
    const leave = () => { if (!focused) later(hide, HIDE_DELAY_MS); };
    node.addEventListener("pointerenter", enter);
    node.addEventListener("pointerleave", leave);
    return () => {
      node.removeEventListener("pointerenter", enter);
      node.removeEventListener("pointerleave", leave);
    };
  }

  /** Clicks on the box must not toggle a surrounding `<summary>` or reach other handlers. */
  function swallowClicks(node: HTMLElement) {
    const stop = (event: Event) => { event.preventDefault(); event.stopPropagation(); };
    node.addEventListener("click", stop);
    return () => node.removeEventListener("click", stop);
  }
</script>

<svelte:window onkeydowncapture={keydown} />

{#if text}
  <span class="infotip" bind:this={wrapper} {@attach hoverable}>
    {@render children()}
    <button type="button" class="cue" aria-label={t("common.more_info")} aria-describedby={id}
      onfocus={() => { focused = true; show(); }}
      onblur={() => { focused = false; hide(); }}
      onclick={(event) => { event.preventDefault(); event.stopPropagation(); show(); }}>
      <Icon name="info" size={14} />
    </button>
    <span {id} role="tooltip" class="tip" class:placed={placement !== null} hidden={!open} bind:this={tip}
      style:left={placement ? `${placement.left}px` : undefined} style:top={placement ? `${placement.top}px` : undefined}
      {@attach swallowClicks}>{text}</span>
  </span>
{:else}
  {@render children()}
{/if}

<style>
  .infotip { display: inline-flex; align-items: center; gap: var(--space-1); width: fit-content; max-width: 100%; }
  .cue { display: inline-flex; align-items: center; justify-content: center; flex-shrink: 0; width: 24px; height: 24px; margin: -4px 0; padding: 0; border: 0; border-radius: 50%; background: transparent; color: var(--text-muted); cursor: help; }
  .infotip:hover .cue, .cue:focus-visible { color: var(--accent); }
  .tip { position: fixed; left: 0; top: 0; z-index: 1000; width: max-content; max-width: min(22rem, calc(100vw - 16px)); padding: var(--space-2) var(--space-3); background: var(--surface); color: var(--text); border: 1px solid var(--border); border-radius: var(--radius-sm); box-shadow: var(--shadow); font: 400 var(--text-sm) / 1.45 var(--font); letter-spacing: normal; text-align: left; text-transform: none; white-space: normal; overflow-wrap: break-word; cursor: default; visibility: hidden; }
  .tip.placed { visibility: visible; }
  .tip[hidden] { display: none; }
</style>
