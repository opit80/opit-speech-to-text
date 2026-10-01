<script lang="ts">
  import { api } from "../lib/api";
  import { app, errorText, t } from "../lib/app.svelte";
  import { onHistoryAdded } from "../lib/events";
  import { formatDateTime, formatSeconds } from "../lib/format";
  import { formatCombo } from "../lib/hotkey";
  import { navigate } from "../lib/router.svelte";
  import { toast } from "../lib/toast.svelte";
  import type { Dictation } from "../lib/types";
  import Banner from "../lib/components/Banner.svelte";
  import Button from "../lib/components/Button.svelte";
  import Icon from "../lib/components/Icon.svelte";
  import PageHeader from "../lib/components/PageHeader.svelte";
  import Select from "../lib/components/Select.svelte";
  import StatusBadge from "../lib/components/StatusBadge.svelte";

  let recent = $state<Dictation[]>([]);
  let listBusy = $state(true);
  let listError = $state<string | null>(null);
  let missingKey = $state(false);
  let keyBusy = $state(false);
  let keyError = $state<string | null>(null);
  let actionBusy = $state(false);
  let actionError = $state<string | null>(null);
  let profileBusy = $state(false);
  let profileError = $state<string | null>(null);
  let copyingId = $state<number | null>(null);
  let copyError = $state<string | null>(null);
  let disposed = false;
  let listRequest = 0;

  const active = $derived(app.config?.profiles.find((p) => p.id === app.config?.active_profile_id) ?? null);
  const busyState = $derived(app.status.state === "transcribing" || app.status.state === "pasting");
  const keyProblem = $derived(
    app.status.kind === "missing_key" || app.status.kind === "invalid_key" || app.status.kind === "credentials",
  );

  async function loadRecent() {
    if (disposed) return;
    const request = ++listRequest;
    listBusy = true;
    listError = null;
    try {
      const rows = await api.historyRecent(8, null);
      if (!disposed && request === listRequest) recent = rows;
    } catch (error) {
      if (!disposed && request === listRequest) listError = errorText(error);
    } finally {
      if (!disposed && request === listRequest) listBusy = false;
    }
  }

  $effect(() => {
    let unlisten: (() => void) | undefined;
    // Subscribe before the first read, so a completed dictation cannot fall into a gap.
    onHistoryAdded(() => void loadRecent()).then((stop) => {
      if (disposed) stop();
      else {
        unlisten = stop;
        void loadRecent();
      }
    }).catch((error: unknown) => {
      if (!disposed) {
        listError = errorText(error);
        listBusy = false;
      }
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  $effect(() => {
    const ref = active?.api_key_ref ?? null;
    let keyDisposed = false;
    missingKey = false;
    keyError = null;
    keyBusy = !!ref;
    if (!ref) return;
    async function checkKey(keyRef: string) {
      try {
        const has = await api.hasApiKey(keyRef);
        if (!keyDisposed) missingKey = !has;
      } catch (error) {
        if (!keyDisposed) keyError = errorText(error);
      } finally {
        if (!keyDisposed) keyBusy = false;
      }
    }
    void checkKey(ref);
    return () => { keyDisposed = true; };
  });

  async function runDictation(action: () => Promise<void>) {
    if (actionBusy || profileBusy || disposed) return;
    actionBusy = true;
    actionError = null;
    try {
      await action();
    } catch (error) {
      if (!disposed) actionError = errorText(error);
    } finally {
      if (!disposed) actionBusy = false;
    }
  }

  function selectedProfile() {
    // Reconcile the native selection after a rejected save, even if config did not change.
    void profileBusy;
    return app.config?.active_profile_id ?? "";
  }

  async function setProfile(id: string) {
    if (profileBusy || actionBusy || disposed || app.status.state !== "idle" || id === app.config?.active_profile_id) return;
    profileBusy = true;
    profileError = null;
    try {
      await api.setActiveProfile(id);
      // The config-changed event owns the live config, including changes from the tray.
    } catch (error) {
      if (!disposed) profileError = errorText(error);
    } finally {
      if (!disposed) profileBusy = false;
    }
  }

  async function copy(item: Dictation) {
    if (copyingId !== null || disposed) return;
    copyingId = item.id;
    copyError = null;
    try {
      await navigator.clipboard.writeText(item.text);
      if (!disposed) toast(t("common.copied"));
    } catch (error) {
      if (!disposed) copyError = errorText(error);
    } finally {
      if (!disposed) copyingId = null;
    }
  }
</script>

<div>
  <PageHeader title={t("home.title")} />
  <div class="home-cards">
    <section class="card status-card stack" aria-label={t("home.title")}>
      <div role="status"><StatusBadge status={app.status} /></div>
      <div class="row">
        <div class="primary-action">
          <Button variant="primary" busy={actionBusy} disabled={busyState || profileBusy}
            onclick={() => void runDictation(api.toggleDictation)}>
            {#if busyState}{t("home.working")}
            {:else if app.status.state === "recording"}<Icon name="stop" />{t("home.stop")}
            {:else}<Icon name="mic" />{t("home.start")}{/if}
          </Button>
        </div>
        {#if app.status.state === "recording" || busyState}
          <Button variant="ghost" disabled={actionBusy} onclick={() => void runDictation(api.cancelDictation)}>
            {t("home.cancel")}
          </Button>
        {/if}
      </div>
      {#if app.status.state === "error"}
        <Banner tone="error">
          {app.status.message}
          {#snippet action()}
            <div class="row">
              {#if app.status.can_retry}
                <Button disabled={actionBusy || profileBusy} onclick={() => void runDictation(api.retryDictation)}>
                  {t("common.retry")}
                </Button>
              {/if}
              {#if keyProblem}
                <Button onclick={() => navigate("profiles")}>{t("home.open_profiles")}</Button>
              {/if}
            </div>
          {/snippet}
        </Banner>
      {/if}
      {#if actionError}<Banner tone="error">{actionError}</Banner>{/if}
      {#if app.config}
        <p class="muted">
          {app.config.hotkey.enabled ? t("home.shortcut", {
            combo: formatCombo(app.config.hotkey.keys, app.lang),
            mode: t(app.config.hotkey.mode === "toggle" ? "home.mode.toggle" : "home.mode.push_to_talk"),
          }) : t("home.shortcut_off")}
        </p>
      {/if}
      <p class="muted own-window">{t("home.own_window")}</p>
    </section>

    <section class="card stack" aria-label={t("home.profile")} aria-busy={profileBusy || keyBusy}>
      <Select label={t("home.profile")}
        bind:value={selectedProfile, (id) => void setProfile(id)}
        options={app.config?.profiles.map((profile) => ({ value: profile.id, label: profile.name })) ?? []}
        disabled={!app.config || app.status.state !== "idle" || profileBusy || actionBusy} />
      {#if profileError}<Banner tone="error">{profileError}</Banner>{/if}
      {#if keyError}<Banner tone="error">{keyError}</Banner>{/if}
      {#if missingKey}
        <Banner tone="warning">
          {t("home.no_key")}
          {#snippet action()}
            <Button onclick={() => navigate("profiles")}>{t("home.add_key")}</Button>
          {/snippet}
        </Banner>
      {/if}
    </section>

    <section class="card stack" aria-labelledby="home-recent">
      <div class="recent-header">
        <h2 id="home-recent">{t("home.recent")}</h2>
        <a href="#/history">{t("home.show_all")}</a>
      </div>
      {#if app.config?.history.enabled === false}
        <p class="muted">{t("home.history_off")} <a href="#/settings">{t("home.open_settings")}</a></p>
      {:else}
        {#if listError}<Banner tone="error">{listError}</Banner>{/if}
        {#if copyError}<Banner tone="error">{copyError}</Banner>{/if}
        <div aria-busy={listBusy}>
          {#if listBusy && recent.length === 0}
            <p class="muted" role="status">{t("common.loading")}</p>
          {:else if recent.length === 0 && !listError}
            <p class="muted">{t("home.empty")}</p>
          {:else if recent.length}
            <ul class="recent-list">
              {#each recent as item (item.id)}
                <li>
                  <div class="dictation-content">
                    <div class="metadata muted">
                      <time datetime={new Date(item.created_at_ms).toISOString()}>{formatDateTime(item.created_at_ms, app.lang)}</time>
                      <span>{formatSeconds(item.latency_ms, app.lang)}</span>
                    </div>
                    <p class="transcript" class:muted={item.status !== "ok"}>
                      {item.status === "empty" ? t("dictation.empty") : item.status === "hallucination" ? t("dictation.hallucination") : item.text}
                    </p>
                  </div>
                  <Button variant="ghost" aria-label={t("common.copy")} title={t("common.copy")}
                    busy={copyingId === item.id} disabled={copyingId !== null} onclick={() => void copy(item)}>
                    <Icon name="copy" />
                  </Button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {/if}
    </section>
  </div>
</div>

<style>
  .home-cards { display: flex; flex-direction: column; gap: var(--space-4); }
  .status-card { padding: var(--space-5); }
  .primary-action :global(button) { min-height: 48px; min-width: 200px; font-size: var(--text-lg); padding: var(--space-3) var(--space-4); }
  .own-window { font-size: var(--text-sm); }
  .recent-header { display: flex; align-items: baseline; justify-content: space-between; gap: var(--space-3); flex-wrap: wrap; }
  a { color: var(--accent); text-underline-offset: 2px; }
  a:hover { color: var(--accent-hover); }
  .recent-list { list-style: none; padding: 0; margin: 0; }
  li { display: flex; align-items: center; gap: var(--space-3); padding: var(--space-3) 0; }
  li + li { border-top: 1px solid var(--border); }
  .dictation-content { flex: 1; min-width: 0; }
  .metadata { display: flex; justify-content: space-between; gap: var(--space-3); flex-wrap: wrap; font-size: var(--text-sm); margin-bottom: var(--space-1); }
  .transcript { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; overflow: hidden; overflow-wrap: anywhere; white-space: pre-wrap; }
</style>
