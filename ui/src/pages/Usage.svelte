<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import { app, errorText, t } from "../lib/app.svelte";
  import { onHistoryAdded } from "../lib/events";
  import { formatClock, formatMilliseconds, formatNumber } from "../lib/format";
  import { usageBounds, type UsagePeriod } from "../lib/usage";
  import type { UsageStats } from "../lib/types";
  import Banner from "../lib/components/Banner.svelte";
  import Button from "../lib/components/Button.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import Select from "../lib/components/Select.svelte";

  let period = $state<UsagePeriod>("all");
  let stats = $state<UsageStats | null>(null);
  let busy = $state(true);
  let error = $state<string | null>(null);
  let disposed = false;
  let request = 0;
  const options = $derived([
    { value: "today", label: t("usage.today") },
    { value: "week", label: t("usage.week") },
    { value: "all", label: t("usage.all") },
  ]);

  async function load() {
    const current = ++request;
    busy = true;
    error = null;
    stats = null;
    const bounds = usageBounds(period);
    try {
      const result = await api.usageStats(bounds.sinceMs, bounds.untilMs);
      if (!disposed && current === request) stats = result;
    } catch (e) {
      if (!disposed && current === request) error = errorText(e);
    } finally { if (!disposed && current === request) busy = false; }
  }

  onMount(() => {
    let stop: (() => void) | undefined;
    onHistoryAdded(() => void load()).then((unlisten) => {
      if (disposed) unlisten();
      else { stop = unlisten; void load(); }
    }).catch((e: unknown) => { if (!disposed) { error = errorText(e); busy = false; } });
    return () => { disposed = true; stop?.(); };
  });
</script>

<div class="usage">
  <PageHeader title={t("usage.title")} description={t("usage.description")} />
  <div class="toolbar"><Select label={t("usage.period")} value={period} {options} onchange={(v) => { period = v as UsagePeriod; void load(); }} /><Button busy={busy} onclick={() => void load()}><Icon name="retry" />{t("settings.refresh")}</Button></div>
  {#if !app.config?.history.enabled}<Banner tone="info">{t("usage.history_off")} <a href="#/settings">{t("guide.open_settings")}</a></Banner>{/if}
  {#if error}<Banner tone="error">{error}</Banner>{/if}
  <section aria-label={t("usage.summary")} aria-busy={busy}>
    {#if busy}<p class="muted" role="status">{t("common.loading")}</p>
    {:else if stats && stats.dictations > 0}
      <dl class="metrics">
        <div><dt>{t("usage.dictations")}</dt><dd>{formatNumber(stats.dictations, app.lang)}</dd><p>{t("usage.successful", { count: formatNumber(stats.successful, app.lang) })}</p></div>
        <div><dt>{t("usage.duration")}</dt><dd class="mono">{formatClock(stats.audio_ms)}</dd><p>{t("usage.duration_hint")}</p></div>
        <div><dt>{t("usage.characters")}</dt><dd>{formatNumber(stats.characters, app.lang)}</dd><p>{t("usage.characters_hint")}</p></div>
        <div><dt>{t("usage.latency")}</dt><dd>{stats.successful ? formatMilliseconds(stats.average_latency_ms, app.lang) : t("usage.no_result")}</dd><p>{t("usage.latency_hint")}</p></div>
      </dl>
    {:else if stats}<div class="empty"><Icon name="mic" size={28} /><h2>{t("usage.empty_title")}</h2><p class="muted">{t("usage.empty_body")}</p><a href="#/guide">{t("nav.guide")}</a></div>{/if}
  </section>
  <p class="note muted">{t("usage.retained_note")}</p>
</div>

<style>
  .usage { display: flex; flex-direction: column; gap: var(--space-4); }
  .usage :global(header) { margin-bottom: 0; }
  .toolbar { display: flex; align-items: flex-end; gap: var(--space-3); flex-wrap: wrap; }
  .metrics { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); margin: 0; border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; background: var(--surface); }
  .metrics > div { padding: var(--space-5); border-bottom: 1px solid var(--border); }
  .metrics > div:nth-child(odd) { border-right: 1px solid var(--border); }
  .metrics > div:nth-last-child(-n+2) { border-bottom: 0; }
  dt { color: var(--text-muted); }
  dd { font-size: var(--text-2xl); font-weight: 600; margin: var(--space-2) 0; }
  .metrics p, .note { font-size: var(--text-sm); color: var(--text-muted); }
  a { color: var(--accent); text-underline-offset: 3px; }
  .empty { display: flex; flex-direction: column; align-items: center; gap: var(--space-3); text-align: center; padding: var(--space-6); border: 1px solid var(--border); border-radius: var(--radius); }
  .empty :global(svg) { color: var(--accent); }
  @media (max-width: 600px) { .metrics { grid-template-columns: 1fr; } .metrics > div:nth-child(odd) { border-right: 0; } .metrics > div:nth-last-child(2) { border-bottom: 1px solid var(--border); } }
</style>
