<script lang="ts">
  import { app, errorText, saveConfig, t } from "../../lib/app.svelte";
  import Banner from "../../lib/components/Banner.svelte";

  interface Props {
    onnext: () => void;
    canNext?: boolean;
    form: string;
  }
  let { onnext, canNext = $bindable(false), form }: Props = $props();

  type Choice = "tr" | "en" | "system";

  let selected = $state<Choice>(current());
  let busy = $state(false);
  let error = $state<string | null>(null);
  let disposed = false;

  function current(): Choice {
    const language = app.config?.ui_language ?? null;
    if (language === null) return "system";
    return language.toLowerCase().startsWith("tr") ? "tr" : "en";
  }

  // Language names are shown in their own language (endonyms), so they need no translation.
  function endonym(code: "en" | "tr"): string {
    const name = new Intl.DisplayNames([code], { type: "language" }).of(code) ?? code;
    return name.charAt(0).toLocaleUpperCase(code) + name.slice(1);
  }

  const options = $derived<{ value: Choice; label: string }[]>([
    { value: "tr", label: endonym("tr") },
    { value: "en", label: endonym("en") },
    { value: "system", label: t("common.system_default") },
  ]);

  $effect(() => {
    canNext = !busy;
  });

  $effect(() => () => {
    disposed = true;
  });

  // Saving updates app.lang, so the whole wizard switches language at once.
  async function choose(value: Choice) {
    busy = true;
    error = null;
    try {
      await saveConfig((c) => {
        c.ui_language = value === "system" ? null : value;
      });
    } catch (e) {
      if (!disposed) {
        error = errorText(e);
        selected = current();
      }
    } finally {
      if (!disposed) busy = false;
    }
  }

  function submit(event: SubmitEvent) {
    event.preventDefault();
    if (canNext) onnext();
  }
</script>

<p class="muted">{t("setup.language.body")}</p>
<fieldset class="choices">
  <legend class="visually-hidden">{t("setup.language.title")}</legend>
  {#each options as option (option.value)}
    <label class="choice">
      <input type="radio" name="setup-language" value={option.value} bind:group={selected}
        onchange={() => void choose(option.value)} />
      <span class="choice-text"><span class="choice-title">{option.label}</span></span>
    </label>
  {/each}
</fieldset>
{#if error}<Banner tone="error">{error}</Banner>{/if}
<form id={form} onsubmit={submit}></form>
