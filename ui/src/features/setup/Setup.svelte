<script lang="ts">
  import { tick, type Component } from "svelte";
  import { errorText, saveConfig, t } from "../../lib/state/app.svelte";
  import type { MessageKey } from "../../lib/i18n";
  import { navigate } from "../../lib/navigation/router.svelte";
  import Banner from "../../lib/components/Banner.svelte";
  import Button from "../../lib/components/Button.svelte";
  import InfoTip from "../../lib/components/InfoTip.svelte";
  import StepLanguage from "./steps/StepLanguage.svelte";
  import StepProvider from "./steps/StepProvider.svelte";
  import StepMicrophone from "./steps/StepMicrophone.svelte";
  import StepShortcut from "./steps/StepShortcut.svelte";
  import StepRules from "./steps/StepRules.svelte";
  import StepTry from "./steps/StepTry.svelte";

  /**
   * Steps save choices immediately or on Next. Skip preserves saved configuration and discards
   * unsaved drafts. Next submits the step's form `form`, which saves and calls `onnext`.
   * `canNext` enables Next; on the last step it means "the user has tried it" (Finish label).
   */
  interface StepProps {
    onnext: () => void;
    canNext?: boolean;
    form: string;
    onsavingchange?: (saving: boolean) => void;
  }

  /** `info` is shown in a tooltip on the step heading. */
  const STEPS: { title: MessageKey; info?: MessageKey; component: Component<StepProps, object, "canNext"> }[] = [
    { title: "setup.language.title", info: "setup.language.body", component: StepLanguage },
    { title: "setup.provider.title", info: "setup.provider.body", component: StepProvider },
    { title: "setup.mic.title", component: StepMicrophone },
    { title: "setup.shortcut.title", component: StepShortcut },
    { title: "setup.rules.title", info: "setup.rules.body", component: StepRules },
    { title: "setup.try.title", info: "setup.try.here_note", component: StepTry },
  ];
  const LAST = STEPS.length - 1;
  const FORM = "setup-step-form";

  let step = $state(0);
  let canNext = $state(false);
  let finishBusy = $state(false);
  let providerSaving = $state(false);
  let finishError = $state<string | null>(null);
  let heading = $state<HTMLHeadingElement>();
  let disposed = false;

  const Current = $derived(STEPS[step].component);
  const stepInfo = $derived(STEPS[step].info);

  $effect(() => () => {
    disposed = true;
  });

  async function go(next: number) {
    if (finishBusy || disposed) return;
    canNext = false;
    providerSaving = false;
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

  /** setup_done stays false until here, so a closed window brings the wizard back. */
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
    <h2 tabindex="-1" bind:this={heading}><InfoTip text={stepInfo ? t(stepInfo) : undefined}><span id="setup-step-title">{t(STEPS[step].title)}</span></InfoTip></h2>
    <Current onnext={advanceFrom(step)} bind:canNext form={FORM}
      onsavingchange={(saving) => { providerSaving = saving; }} />
  </section>

  {#if finishError}<Banner tone="error">{finishError}</Banner>{/if}

  <footer>
    {#if step < LAST}
      <Button variant="ghost" disabled={finishBusy || providerSaving}
        onclick={() => { if (!providerSaving) void go(step + 1); }}>{t("common.skip")}</Button>
    {:else}
      <span aria-hidden="true"></span>
    {/if}
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
  .setup :global(fieldset:disabled .choice) { opacity: 0.6; cursor: default; }
</style>
