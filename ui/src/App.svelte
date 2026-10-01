<script lang="ts">
  import { app, errorText, initApp, t } from "./lib/app.svelte";
  import { navigate, router } from "./lib/router.svelte";
  import type { Route } from "./lib/route";
  import type { MessageKey } from "./lib/i18n";
  import { api } from "./lib/api";
  import Banner from "./lib/components/Banner.svelte";
  import Button from "./lib/components/Button.svelte";
  import Icon from "./lib/components/Icon.svelte";
  import Toasts from "./lib/components/Toasts.svelte";
  import { bannerVersion } from "./lib/update";
  import UpdateInstallButton from "./lib/components/UpdateInstallButton.svelte";
  import UpdateProgress from "./lib/components/UpdateProgress.svelte";
  import Home from "./pages/Home.svelte";
  import History from "./pages/History.svelte";
  import Rules from "./pages/Rules.svelte";
  import Profiles from "./pages/Profiles.svelte";
  import Settings from "./pages/Settings.svelte";
  import Setup from "./pages/Setup.svelte";

  const NAV: { route: Route; icon: "home" | "history" | "rules" | "profiles" | "settings" }[] = [
    { route: "home", icon: "home" },
    { route: "history", icon: "history" },
    { route: "rules", icon: "rules" },
    { route: "profiles", icon: "profiles" },
    { route: "settings", icon: "settings" },
  ];

  const navKeys: Record<Route, MessageKey> = {
    home: "nav.home", history: "nav.history", rules: "nav.rules",
    profiles: "nav.profiles", settings: "nav.settings", setup: "nav.setup",
  };

  function navKey(route: Route): MessageKey {
    return navKeys[route];
  }

  const updateVersion = $derived(bannerVersion(app.update, app.updateDismissed));
  const installing = $derived(app.update.kind === "installing" ? app.update : null);

  let resumeBusy = $state(false);
  let resumeError = $state<string | null>(null);

  async function resumeHotkey() {
    resumeBusy = true;
    resumeError = null;
    try {
      app.hotkey = await api.setHotkeyPaused(false);
    } catch (error) {
      resumeError = errorText(error);
    } finally {
      resumeBusy = false;
    }
  }

  $effect(() => {
    let stop: (() => void) | undefined;
    let disposed = false;
    initApp().then((s) => (disposed ? s() : (stop = s)));
    return () => {
      disposed = true;
      stop?.();
    };
  });

  // The wizard runs until it is finished or skipped.
  $effect(() => {
    if (app.ready && app.config && !app.config.ui.setup_done && router.route !== "setup") navigate("setup");
  });

  function dismissNotice(index: number) {
    app.notices.splice(index, 1);
  }
</script>

{#if app.loadError}
  <main class="center"><Banner tone="error">{t("app.load_failed", { message: app.loadError })}</Banner></main>
{:else if !app.ready}
  <main class="center muted">{t("app.loading")}</main>
{:else if router.route === "setup"}
  <Setup />
{:else}
  <div class="shell">
    <nav aria-label={t("nav.main")}>
      <div class="brand">{t("app.name")}</div>
      {#each NAV as item (item.route)}
        <a href={`#/${item.route}`} aria-current={router.route === item.route ? "page" : undefined}>
          <Icon name={item.icon} />{t(navKey(item.route))}
        </a>
      {/each}
      <div class="version muted">v{app.info?.version}</div>
    </nav>
    <main>
      {#if app.notices.length || app.hotkey.error || app.hotkey.paused || resumeError || updateVersion || installing}
        <div class="notices">
          {#each app.notices as notice, i (i)}
            <Banner tone="warning" ondismiss={() => dismissNotice(i)}>
              {#if notice.kind === "config_reset"}
                {notice.backup ? t("notice.config_reset", { backup: notice.backup }) : t("notice.config_reset_kept")}
              {:else}
                {t("notice.user_rules_broken", { reason: notice.reason })}
              {/if}
            </Banner>
          {/each}
          {#if app.hotkey.error}
            <Banner tone="warning">{t("notice.hotkey_failed", { reason: app.hotkey.error })}</Banner>
          {:else if app.hotkey.paused}
            <Banner tone="info">
              {t("notice.hotkey_paused")}
              {#snippet action()}
                <Button busy={resumeBusy} onclick={resumeHotkey}>{t("notice.resume")}</Button>
              {/snippet}
            </Banner>
          {/if}
          {#if resumeError}<Banner tone="error">{resumeError}</Banner>{/if}
          {#if installing}
            <Banner tone="info">
              {t("update.status_installing", { version: installing.info.version })}
              <UpdateProgress downloaded={installing.downloaded} total={installing.total} />
            </Banner>
          {:else if updateVersion}
            <Banner tone="info" ondismiss={() => (app.updateDismissed = updateVersion)}>
              {t("update.available", { version: updateVersion })}
              {#snippet action()}<UpdateInstallButton />{/snippet}
            </Banner>
          {/if}
        </div>
      {/if}
      {#if router.route === "home"}<Home />
      {:else if router.route === "history"}<History />
      {:else if router.route === "rules"}<Rules />
      {:else if router.route === "profiles"}<Profiles />
      {:else if router.route === "settings"}<Settings />{/if}
    </main>
  </div>
{/if}
<Toasts />

<style>
  .shell { display: grid; grid-template-columns: var(--sidebar) minmax(0, 1fr); height: 100vh; }
  nav { display: flex; flex-direction: column; gap: var(--space-1); padding: var(--space-3); background: var(--surface); border-right: 1px solid var(--border); overflow-y: auto; }
  .brand { font-weight: 600; padding: var(--space-3) var(--space-2) var(--space-5); }
  a { display: flex; align-items: center; gap: var(--space-3); height: 36px; flex-shrink: 0; color: var(--text); text-decoration: none; padding: 0 var(--space-3); border-radius: var(--radius-sm); position: relative; }
  a:hover { background: var(--surface-2); }
  a[aria-current="page"] { font-weight: 600; background: var(--surface-2); }
  a[aria-current="page"]::before { content: ""; position: absolute; left: 0; width: 3px; height: 20px; background: var(--accent); border-radius: var(--radius-sm); }
  .version { margin-top: auto; padding: var(--space-4) var(--space-2) var(--space-1); font-size: var(--text-sm); }
  main { padding: var(--space-5); overflow-y: auto; min-width: 0; }
  main :global(> *) { max-width: var(--content); margin-left: auto; margin-right: auto; }
  .notices { display: flex; flex-direction: column; gap: var(--space-3); margin-bottom: var(--space-5); }
  .center { min-height: 100vh; display: flex; align-items: center; justify-content: center; }
</style>
