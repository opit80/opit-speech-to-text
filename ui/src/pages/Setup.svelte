<script lang="ts">
  import { tick, type Component } from "svelte";
  import { errorText, saveConfig, t } from "../lib/app.svelte";
  import type { MessageKey } from "../lib/i18n";
  import { navigate } from "../lib/router.svelte";
  import Banner from "../lib/components/Banner.svelte";
  import Button from "../lib/components/Button.svelte";
  import StepLanguage from "./setup/StepLanguage.svelte";
  import StepProvider from "./setup/StepProvider.svelte";
  import StepMicrophone from "./setup/StepMicrophone.svelte";
  import StepShortcut from "./setup/StepShortcut.svelte";
  import StepRules from "./setup/StepRules.svelte";
  import StepTry from "./setup/StepTry.svelte";

  /**
   * Every step saves its own choices as they are made. The footer's Next button submits the
   * step's (empty) form `form`; the step does its Next work there and then calls `onnext`.
   * `canNext` enables Next; on the last step it means "the user has tried it" (Finish label).
   */
  interface StepProps {
    onnext: () => void;
    canNext?: boolean;
    form: string;
  }

  const STEPS: { title: MessageKey; component: Component<StepProps, object, "canNext"> }[] = [
    { title: "setup.language.title", component: StepLanguage },
    { title: "setup.provider.title", component: StepProvider },
    { title: "setup.mic.title", component: StepMicrophone },
    { title: "setup.shortcut.title", component: StepShortcut },
    { title: "setup.rules.title", component: StepRules },
    { title: "setup.try.title", component: StepTry },
  ];
  const LAST = STEPS.length - 1;
  const FORM = "setup-step-form";

  let step = $state(0);
  let canNext = $state(false);
  let finishBusy = $state(false);
  let finishError = $state<string | null>(null);
  let heading = $state<HTMLHeadingElement>();
  let disposed = false;

  const Current = $derived(STEPS[step].component);

  $effect(() => () => {
    disposed = true;
  });

  async function go(next: number) {
    canNext = false;
    step = next;
    await tick();
    if (!disposed) heading?.focus();
  }

  /** A step can only advance the wizard while it is the current one. */
  function advanceFrom(from: number): () => void {
    return () => {
      if (disposed || step !== from) return;
      if (from < LAST) void go(from + 1);
      else void finish();
    };
  }

  /** Finish and Skip: setup_done stays false until here, so a closed window brings the wizard back. */
  async function finish() {
    if (finishBusy || disposed) return;
    finishBusy = true;
    finishError = null;
    try {
      await saveConfig((c) => {
        c.ui.setup_done = true;
      });
      if (!disposed) navigate("home");
    } catch (error) {
      if (!disposed) finishError = errorText(error);
    } finally {
      if (!disposed) finishBusy = false;
    }
  }
</script>

<main class="setup">
  <header class="stack">
    <p class="brand">{t("app.name")}</p>
    <h1>{t("setup.title")}</h1>
  </header>

  <div class="progress">
    <p class="muted">{t("setup.step_of", { n: step + 1, total: STEPS.length })}</p>
    <ol class="steps">
      {#each STEPS as item, i (item.title)}
        <li aria-current={i === step ? "step" : undefined} class:done={i < step}>{t(item.title)}</li>
      {/each}
    </ol>
  </div>

  <section class="card stack step" aria-labelledby="setup-step-title">
    <h2 id="setup-step-title" tabindex="-1" bind:this={heading}>{t(STEPS[step].title)}</h2>
    <Current onnext={advanceFrom(step)} bind:canNext form={FORM} />
  </section>

  {#if finishError}<Banner tone="error">{finishError}</Banner>{/if}

  <footer>
    <Button variant="ghost" disabled={finishBusy} onclick={() => void finish()}>{t("setup.skip")}</Button>
    <div class="row">
      <Button disabled={step === 0 || finishBusy} onclick={() => void go(step - 1)}>{t("common.back")}</Button>
      <Button type="submit" form={FORM} variant="primary" busy={finishBusy} disabled={step < LAST && !canNext}>
        {step < LAST ? t("common.next") : canNext ? t("setup.finish") : t("setup.finish_untried")}
      </Button>
    </div>
  </footer>
</main>

<style>
  .setup { display: flex; flex-direction: column; gap: var(--space-5); max-width: var(--content); min-height: 100vh; margin: 0 auto; padding: var(--space-6) var(--space-5); }
  .brand { color: var(--text-muted); font-weight: 600; }
  .progress { display: flex; flex-direction: column; gap: var(--space-2); }
  .steps { display: flex; flex-wrap: wrap; gap: var(--space-2) var(--space-4); margin: 0; padding: 0; list-style: none; color: var(--text-muted); }
  .steps li.done { color: var(--text); }
  .steps li[aria-current="step"] { color: var(--text); font-weight: 600; }
  .step { padding: var(--space-5); }
  h2:focus { outline: none; }
  h2:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
  footer { display: flex; align-items: center; justify-content: space-between; gap: var(--space-3); flex-wrap: wrap; margin-top: auto; }

  /* Radio cards shared by the steps. */
  .setup :global(.choices) { display: flex; flex-direction: column; gap: var(--space-2); margin: 0; padding: 0; border: 0; min-width: 0; }
  .setup :global(.choice) { display: flex; align-items: flex-start; gap: var(--space-3); padding: var(--space-3) var(--space-4); border: 1px solid var(--border); border-radius: var(--radius); background: var(--surface); cursor: pointer; }
  .setup :global(.choice:hover) { background: var(--surface-2); }
  .setup :global(.choice:has(input:checked)) { border-color: var(--accent); box-shadow: inset 0 0 0 1px var(--accent); }
  .setup :global(.choice input) { min-height: 0; margin: 3px 0 0; accent-color: var(--accent); }
  .setup :global(.choice-text) { display: flex; flex-direction: column; gap: var(--space-1); min-width: 0; }
  .setup :global(.choice-title) { font-size: var(--text-lg); font-weight: 600; }
  .setup :global(.choice-hint) { color: var(--text-muted); font-size: var(--text-sm); }
  .setup :global(fieldset:disabled .choice) { opacity: 0.6; cursor: default; }
</style>
