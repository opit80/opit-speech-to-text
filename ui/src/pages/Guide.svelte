<script lang="ts">
  import { app, t } from "../lib/app.svelte";
  import { formatCombo } from "../lib/hotkey";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import DictationDemo from "../lib/components/DictationDemo.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import Banner from "../lib/components/Banner.svelte";

  const mode = $derived(app.config?.hotkey.mode ?? "toggle");
  const shortcut = $derived(app.config ? formatCombo(app.config.hotkey.keys, app.lang) : t("common.loading"));
  const unavailable = $derived(app.config?.hotkey.enabled === false || app.hotkey.paused || !!app.hotkey.error);
</script>

<div class="guide">
  <PageHeader title={t("guide.title")} description={t("guide.description")} />

  <section class="intro" aria-labelledby="guide-intro">
    <div class="intro-copy"><p class="eyebrow">{t("guide.eyebrow")}</p><h2 id="guide-intro">{t("guide.headline")}</h2><p class="muted">{t("guide.intro")}</p></div>
    <div class="shortcut"><Icon name="key" size={20} /><span>{t("guide.your_shortcut")}</span><kbd>{shortcut}</kbd><a href="#/settings">{t("guide.change_shortcut")}</a></div>
  </section>
  {#if unavailable}<Banner tone="warning">{t("guide.shortcut_unavailable")} <a href="#/settings">{t("guide.open_settings")}</a></Banner>{/if}

  <DictationDemo />

  <section aria-labelledby="guide-modes">
    <div class="section-heading"><h2 id="guide-modes">{t("guide.modes_title")}</h2><a href="#/settings">{t("guide.change_mode")}</a></div>
    <div class="modes">
      <div class="mode" class:current={mode === "toggle"}><div class="mode-heading"><Icon name="key" /><h3>{t("settings.mode_toggle")}</h3>{#if mode === "toggle"}<span class="badge">{t("guide.current")}</span>{/if}</div><p>{t("guide.toggle_body")}</p><p class="mode-hint muted">{t("guide.toggle_hint")}</p></div>
      <div class="mode" class:current={mode === "push_to_talk"}><div class="mode-heading"><Icon name="mic" /><h3>{t("settings.mode_ptt")}</h3>{#if mode === "push_to_talk"}<span class="badge">{t("guide.current")}</span>{/if}</div><p>{t("guide.ptt_body")}</p><p class="mode-hint muted">{t("guide.ptt_hint")}</p></div>
    </div>
    <p class="cancel-note"><kbd>Esc</kbd><span>{t(app.config?.hotkey.keys.includes("Escape") ? "guide.cancel_from_home" : "guide.cancel")}</span></p>
  </section>

  <section aria-labelledby="guide-ready">
    <h2 id="guide-ready">{t("guide.ready_title")}</h2>
    <ol class="readiness">
      <li><span class="number">01</span><div><h3>{t("guide.provider_title")}</h3><p>{t("guide.provider_body")}</p></div><a href="#/profiles">{t("nav.profiles")}<span aria-hidden="true">↗</span></a></li>
      <li><span class="number">02</span><div><h3>{t("guide.mic_title")}</h3><p>{t("guide.mic_body")}</p></div><a href="#/settings">{t("nav.settings")}<span aria-hidden="true">↗</span></a></li>
      <li><span class="number">03</span><div><h3>{t("guide.first_title")}</h3><p>{t("guide.first_body", { combo: shortcut })}</p></div><a href="#/home">{t("nav.home")}<span aria-hidden="true">↗</span></a></li>
    </ol>
  </section>

  <section class="tips" aria-labelledby="guide-tips">
    <h2 id="guide-tips">{t("guide.tips_title")}</h2>
    <details><summary>{t("guide.clipboard_title")}</summary><p>{t("guide.clipboard_body")}</p></details>
    <details><summary>{t("guide.correction_title")}</summary><p>{t("guide.correction_body")}</p><a href="#/history">{t("nav.history")} ↗</a><a href="#/rules">{t("nav.rules")} ↗</a></details>
    <details><summary>{t("guide.tray_title")}</summary><p>{t("guide.tray_body")}</p></details>
    <details><summary>{t("guide.privacy_title")}</summary><p>{t("guide.privacy_body")}</p><a href="#/settings">{t("guide.open_settings")} ↗</a></details>
  </section>
  <footer><Icon name="mic" /><p>{t("guide.footer")}</p></footer>
</div>

<style>
  .guide { display: flex; flex-direction: column; gap: var(--space-6); padding-bottom: var(--space-4); }
  .guide :global(header) { margin-bottom: 0; }
  .intro { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: var(--space-5); align-items: center; }
  .eyebrow { color: var(--accent); font-size: var(--text-sm); font-weight: 600; margin-bottom: var(--space-2); }
  .intro h2 { font-size: var(--text-2xl); letter-spacing: -0.5px; margin-bottom: var(--space-3); max-width: 18ch; }
  .intro-copy > p:last-child { max-width: 46ch; }
  .shortcut { display: flex; flex-direction: column; align-items: flex-start; gap: var(--space-2); padding: var(--space-4); background: var(--info-bg); border: 1px solid var(--border); border-radius: var(--radius); max-width: 280px; }
  .shortcut :global(svg) { color: var(--accent); }
  .shortcut > span { font-size: var(--text-sm); color: var(--text-muted); }
  kbd { font-family: var(--font-mono); font-size: var(--text-md); font-weight: 600; overflow-wrap: anywhere; }
  a { color: var(--accent); text-underline-offset: 3px; font-size: var(--text-sm); }
  a:hover { color: var(--accent-hover); }
  .section-heading { display: flex; justify-content: space-between; align-items: baseline; gap: var(--space-3); flex-wrap: wrap; margin-bottom: var(--space-4); }
  .modes { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--space-3); }
  .mode { padding: var(--space-4); border: 1px solid var(--border); border-radius: var(--radius); }
  .mode.current { background: var(--surface); border-color: var(--accent); }
  .mode-heading { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); margin-bottom: var(--space-3); }
  h3 { font-size: var(--text-md); font-weight: 600; }
  .badge { color: var(--accent); background: var(--info-bg); border-radius: var(--radius-sm); padding: 2px var(--space-2); font-size: var(--text-sm); }
  .mode-hint { font-size: var(--text-sm); margin-top: var(--space-2); }
  .cancel-note { display: flex; align-items: baseline; gap: var(--space-2); color: var(--text-muted); font-size: var(--text-sm); margin-top: var(--space-3); }
  .cancel-note kbd { border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 2px var(--space-2); background: var(--surface); color: var(--text); font-size: var(--text-sm); }
  .readiness { list-style: none; padding: 0; margin: var(--space-3) 0 0; }
  .readiness li { display: grid; grid-template-columns: 28px minmax(0, 1fr) auto; gap: var(--space-4); align-items: baseline; padding: var(--space-4) 0; border-bottom: 1px solid var(--border); }
  .number { font-family: var(--font-mono); font-size: var(--text-sm); color: var(--accent); }
  .readiness p { color: var(--text-muted); margin-top: var(--space-1); max-width: 60ch; }
  .readiness a { display: inline-flex; gap: var(--space-2); }
  .tips h2 { margin-bottom: var(--space-3); }
  details { border-bottom: 1px solid var(--border); }
  summary { cursor: pointer; padding: var(--space-4) 0; font-weight: 600; }
  details p { color: var(--text-muted); padding-bottom: var(--space-4); max-width: 70ch; }
  details a { display: inline-block; margin: 0 var(--space-4) var(--space-4) 0; }
  footer { display: flex; align-items: center; gap: var(--space-2); color: var(--text-muted); font-size: var(--text-sm); }
  @media (max-width: 760px) { .intro { grid-template-columns: 1fr; } .shortcut { max-width: none; } .modes { grid-template-columns: 1fr; } }
  @media (max-width: 560px) { .readiness li { grid-template-columns: 24px minmax(0, 1fr); gap: var(--space-2); } .readiness a { grid-column: 2; } }
</style>
