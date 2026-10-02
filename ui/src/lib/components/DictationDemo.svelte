<script lang="ts">
  import { t } from "../app.svelte";
  import { DEMO_STEPS, nextDemoStep, type DemoStep } from "../guide-demo";
  import type { MessageKey } from "../i18n";
  import Button from "./Button.svelte";
  import Icon from "./Icon.svelte";

  let step = $state<DemoStep>("cursor");
  const labels: Record<DemoStep, MessageKey> = {
    cursor: "guide.demo.cursor", speak: "guide.demo.speak",
    process: "guide.demo.process", result: "guide.demo.result",
  };
  const bodies: Record<DemoStep, MessageKey> = {
    cursor: "guide.demo.cursor_body", speak: "guide.demo.speak_body",
    process: "guide.demo.process_body", result: "guide.demo.result_body",
  };
  const actions: Record<DemoStep, MessageKey> = {
    cursor: "guide.demo.start", speak: "guide.demo.stop",
    process: "guide.demo.paste", result: "guide.demo.restart",
  };
  const heights = [12, 20, 32, 18, 40, 28, 48, 24, 36, 16, 44, 30, 20, 38, 26, 48, 18, 34, 24, 12];
</script>

<section class="demo" aria-labelledby="guide-demo-title">
  <div class="demo-heading">
    <div><p class="eyebrow">{t("guide.demo.eyebrow")}</p><h2 id="guide-demo-title">{t("guide.demo.title")}</h2></div>
    <span class="sample-label"><Icon name="play" size={12} />{t("guide.demo.sample")}</span>
  </div>
  <ol class="steps" aria-label={t("guide.demo.steps")}>
    {#each DEMO_STEPS as item, index (item)}
      <li>
        <button type="button" class:chosen={item === step} aria-current={item === step ? "step" : undefined} onclick={() => (step = item)}>
          <span class="step-number" aria-hidden="true">{index + 1}</span><span>{t(labels[item])}</span>
        </button>
      </li>
    {/each}
  </ol>
  <div class="stage" class:listening={step === "speak"}>
    <div class="stage-caption"><Icon name={step === "result" ? "check" : step === "speak" ? "mic" : "wand"} />{t(labels[step])}</div>
    <div class="scene">
      {#if step === "speak"}
        <div class="wave" aria-hidden="true">
          {#each heights as height, i}<span style:height={`${height}px`} style:animation-delay={`${i * -0.09}s`}></span>{/each}
        </div>
        <p class="spoken">{t("guide.demo.sentence")}</p>
      {:else if step === "process"}
        <div class="conversion"><span class="muted">{t("guide.demo.before")}</span><s>{t("guide.demo.raw_term")}</s><Icon name="wand" size={20} /><strong>{t("guide.demo.correct_term")}</strong></div>
        <p class="muted">{t("guide.demo.rule_note")}</p>
      {:else}
        <div class="editor">
          <div class="editor-bar"><Icon name="rules" size={14} /><span>{t("guide.demo.editor")}</span></div>
          <p class="editor-text">{#if step === "result"}{t("guide.demo.sentence")}{:else}<span class="placeholder">{t("guide.demo.placeholder")}</span>{/if}<span class="caret" aria-hidden="true"></span></p>
        </div>
      {/if}
    </div>
    <p class="instruction" role="status" aria-live="polite">{t(bodies[step])}</p>
  </div>
  <div class="demo-footer">
    <p class="muted">{t("guide.demo.safe")}</p>
    <Button variant="primary" onclick={() => (step = nextDemoStep(step))}><Icon name={step === "result" ? "retry" : step === "speak" ? "stop" : "play"} />{t(actions[step])}</Button>
  </div>
</section>

<style>
  .demo { border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); overflow: hidden; }
  .demo-heading { display: flex; justify-content: space-between; align-items: center; gap: var(--space-3); padding: var(--space-5); flex-wrap: wrap; }
  .eyebrow { font-size: var(--text-sm); font-weight: 600; color: var(--accent); margin-bottom: var(--space-1); }
  h2 { font-size: var(--text-xl); }
  .sample-label { display: inline-flex; align-items: center; gap: var(--space-2); color: var(--text-muted); font-size: var(--text-sm); }
  .steps { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); list-style: none; padding: 0 var(--space-5) var(--space-4); margin: 0; gap: var(--space-2); }
  .steps button { display: flex; align-items: center; gap: var(--space-2); width: 100%; padding: var(--space-2); border: 1px solid transparent; border-radius: var(--radius-sm); background: transparent; color: var(--text-muted); text-align: left; cursor: pointer; font-size: var(--text-sm); }
  .steps button:hover { background: var(--surface-2); }
  .steps .chosen { background: var(--info-bg); color: var(--accent); border-color: var(--accent); font-weight: 600; }
  .step-number { font-family: var(--font-mono); }
  .stage { background: var(--surface-2); padding: var(--space-5); border-top: 1px solid var(--border); border-bottom: 1px solid var(--border); }
  .stage-caption { display: flex; align-items: center; justify-content: center; gap: var(--space-2); color: var(--accent); font-size: var(--text-sm); font-weight: 600; }
  .scene { min-height: 156px; display: flex; flex-direction: column; justify-content: center; align-items: center; gap: var(--space-3); text-align: center; }
  .editor { width: min(100%, 460px); border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); text-align: left; }
  .editor-bar { display: flex; align-items: center; gap: var(--space-2); border-bottom: 1px solid var(--border); padding: var(--space-2) var(--space-4); font-size: var(--text-sm); color: var(--text-muted); }
  .editor-text { padding: var(--space-5) var(--space-4); min-height: 90px; font-size: var(--text-lg); overflow-wrap: anywhere; }
  .placeholder { color: var(--text-muted); }
  .caret { display: inline-block; height: 1.1em; width: 2px; background: var(--accent); vertical-align: text-bottom; margin-left: var(--space-1); }
  .instruction { max-width: 60ch; margin: 0 auto; text-align: center; color: var(--text-muted); min-height: 42px; }
  .wave { display: flex; align-items: center; height: 52px; gap: var(--space-1); }
  .wave span { width: 4px; border-radius: var(--radius-sm); background: var(--accent); animation: pulse 0.9s ease-in-out infinite alternate; }
  .spoken { font-size: var(--text-xl); font-weight: 600; max-width: 40ch; }
  .conversion { display: flex; align-items: center; justify-content: center; flex-wrap: wrap; gap: var(--space-3); font-size: var(--text-xl); }
  .conversion > span { font-size: var(--text-sm); }
  .conversion strong { color: var(--success); }
  .conversion :global(svg) { color: var(--accent); }
  .demo-footer { display: flex; justify-content: space-between; align-items: center; gap: var(--space-4); padding: var(--space-4) var(--space-5); }
  .demo-footer p { font-size: var(--text-sm); max-width: 40ch; }
  .demo-footer :global(button) { flex-shrink: 0; }
  @keyframes pulse { from { transform: scaleY(0.45); } to { transform: scaleY(1); } }
  @media (max-width: 680px) { .steps { grid-template-columns: repeat(2, minmax(0, 1fr)); } .demo-footer { align-items: flex-start; flex-direction: column; } .stage, .demo-heading, .demo-footer { padding: var(--space-4); } .steps { padding-left: var(--space-4); padding-right: var(--space-4); } }
</style>
