<script lang="ts">
  import type { Snippet } from "svelte";
  import { t } from "../app.svelte";
  import type { MessageKey } from "../i18n";
  import type { DictationState, DictationStatus, HotkeyMode } from "../types";
  import Button from "./Button.svelte";
  import Icon from "./Icon.svelte";
  import StatusBadge from "./StatusBadge.svelte";

  interface Props {
    status: DictationStatus;
    busy: boolean;
    disabled: boolean;
    profileName?: string;
    shortcut: string;
    shortcutEnabled: boolean;
    mode: HotkeyMode;
    ontoggle: () => void;
    oncancel: () => void;
    feedback?: Snippet;
    feedbackVisible?: boolean;
  }
  let { status, busy, disabled, profileName, shortcut, shortcutEnabled, mode, ontoggle, oncancel, feedback, feedbackVisible = false }: Props = $props();
  const recording = $derived(status.state === "recording");
  const working = $derived(status.state === "transcribing" || status.state === "pasting");
  const titles: Record<DictationState, MessageKey> = {
    idle: "home.control.ready_title", recording: "home.control.recording_title", transcribing: "home.control.processing_title",
    pasting: "status.pasting", cancelled: "status.cancelled", error: "status.error",
  };
  const bodyKey = $derived(recording ? "home.control.recording_body" : working ? "home.control.processing_body" : "home.control.ready_body");
  const actionKey = $derived(working || busy ? "home.working" : recording ? "home.stop" : "home.start");
  const wave = [10, 16, 24, 12, 30, 20, 34, 18, 26, 14, 22, 10];
</script>

<section class="recorder" class:recording aria-labelledby="record-title">
  <div class="recorder-top">
    <span class="section-label"><Icon name="mic" size={14} />{t("home.control.section")}</span>
    <div class="state" role="status"><StatusBadge {status} /></div>
  </div>
  <div class="controls">
    <div class="mic-control">
      <Button variant="ghost" disabled={working || busy || disabled} aria-busy={working || busy} aria-label={t(actionKey)} onclick={ontoggle}>
        <span class="mic-orb"><Icon name={recording ? "stop" : working ? "wand" : "mic"} size={30} /></span>
        <span class="control-label">{t(actionKey)}</span>
      </Button>
    </div>
    <div class="copy">
      <h2 id="record-title">{t(titles[status.state])}</h2>
      <p>{t(bodyKey)}</p>
      {#if recording}
        <div class="wave" aria-hidden="true">{#each wave as height, i}<span style:height={`${height}px`} style:animation-delay={`${i * -0.11}s`}></span>{/each}</div>
      {:else if profileName}<span class="provider"><Icon name="profiles" size={12} />{t("home.control.provider", { name: profileName })}</span>{/if}
    </div>
    {#if recording || working}<div class="cancel"><Button variant="ghost" disabled={busy || disabled} aria-label={t("home.cancel")} onclick={oncancel}><Icon name="x" /></Button></div>{/if}
  </div>
  {#if feedback && feedbackVisible}<div class="feedback">{@render feedback()}</div>{/if}
  <div class="recorder-bottom">
    <a class="shortcut" href="#/settings" aria-label={t("guide.change_shortcut")}><Icon name="key" size={14} /><span>{t("home.control.shortcut")}</span>{#if shortcutEnabled}<kbd>{shortcut}</kbd>{:else}<span>{t("home.shortcut_off")}</span>{/if}</a>
    <span class="mode">{t(mode === "push_to_talk" ? "home.control.mode_ptt" : "home.control.mode_toggle")}</span>
  </div>
</section>

<style>
  .recorder { border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); overflow: hidden; }
  .recorder-top { display: flex; align-items: center; justify-content: space-between; gap: var(--space-3); padding: var(--space-4) var(--space-5) 0; }
  .section-label { display: inline-flex; align-items: center; gap: var(--space-2); color: var(--text-muted); font-size: var(--text-sm); font-weight: 600; }
  .state { display: inline-flex; align-items: center; gap: var(--space-2); color: var(--text-muted); font-size: var(--text-sm); }
  .feedback { padding: 0 var(--space-5) var(--space-4); }
  .controls { display: grid; grid-template-columns: 104px minmax(0, 1fr) auto; align-items: center; gap: var(--space-5); padding: var(--space-5); min-height: 148px; }
  .mic-control { display: flex; flex-direction: column; align-items: center; gap: var(--space-2); }
  .mic-control :global(button) { width: 104px; min-height: 104px; padding: 0; background: transparent; transition: transform 160ms ease-out; }
  .mic-control :global(.label) { flex-direction: column; }
  .mic-orb { width: 76px; height: 76px; display: flex; align-items: center; justify-content: center; border-radius: 50%; background: var(--accent); color: var(--accent-text); }
  .recording .mic-orb { background: var(--danger); }
  .mic-control :global(button:hover) { background: transparent; transform: scale(1.04); }
  .mic-control :global(button:active) { transform: scale(0.96); }
  .mic-control :global(button:disabled) { transform: none; }
  .control-label { color: var(--text-muted); font-size: var(--text-sm); text-align: center; }
  h2 { font-size: var(--text-2xl); font-weight: 600; letter-spacing: -0.5px; margin-bottom: var(--space-2); }
  .copy p { max-width: 46ch; color: var(--text-muted); }
  .provider { display: inline-flex; align-items: center; gap: var(--space-1); margin-top: var(--space-3); color: var(--text-muted); font-size: var(--text-sm); }
  .wave { display: flex; align-items: center; gap: var(--space-1); height: 34px; margin-top: var(--space-2); }
  .wave span { width: 3px; background: var(--danger); border-radius: var(--radius-sm); animation: level 700ms ease-in-out infinite alternate; }
  .recorder-bottom { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: var(--space-2); padding: var(--space-3) var(--space-5); background: var(--surface-2); border-top: 1px solid var(--border); }
  .shortcut { display: inline-flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); color: var(--text-muted); text-decoration: none; font-size: var(--text-sm); }
  .shortcut:hover { color: var(--accent); }
  kbd { color: var(--text); font: 600 var(--text-sm) var(--font-mono); padding: var(--space-1) var(--space-2); border: 1px solid var(--border); border-radius: var(--radius-sm); background: var(--surface); }
  .mode { color: var(--text-muted); font-size: var(--text-sm); }
  @keyframes level { from { transform: scaleY(0.45); } to { transform: scaleY(1); } }
  @media (max-width: 680px) { .controls { grid-template-columns: 80px minmax(0, 1fr); gap: var(--space-3); padding: var(--space-4); } .cancel { grid-column: 2; } h2 { font-size: var(--text-xl); } .mic-control :global(button) { width: 80px; min-height: 96px; } .mic-orb { width: 64px; height: 64px; } .recorder-top, .recorder-bottom { padding-left: var(--space-4); padding-right: var(--space-4); } }
</style>
