<script lang="ts">
  import { app, type View } from "$lib/stores/app.svelte";
  import { api } from "$lib/api";
  import { t, userText } from "$lib/i18n";

  let refreshing = $state(false);

  const tabs: { id: View; label: string; count?: () => number }[] = [
    { id: "library", label: "nav.library" },
    { id: "downloads", label: "nav.downloads", count: () => app.activeGames.length },
    { id: "diagnostics", label: "nav.diagnostics" },
  ];

  async function refresh() {
    refreshing = true;
    try {
      const n = await api.refreshCatalog();
      await app.reloadGames();
      app.toast("success", t("nav.refresh.done", { count: n }));
    } catch (e) {
      app.toast("error", userText(e));
    } finally {
      refreshing = false;
    }
  }
</script>

<header>
  <div class="brand" onclick={() => (app.view = "library")} role="button" tabindex="0" onkeydown={(e) => e.key === "Enter" && (app.view = "library")}>
    {#if app.event?.theme?.logo}
      <img src={app.event.theme.logo} alt="" />
    {:else}
      <span class="logo"></span>
    {/if}
    <div class="titles">
      <strong>{app.eventTitle || t("app.title")}</strong>
      {#if app.eventTitle}<small>{t("app.title")}</small>{/if}
    </div>
  </div>

  <nav>
    {#each tabs as tab (tab.id)}
      <button class:active={app.view === tab.id} class="tab" onclick={() => (app.view = tab.id)}>
        {t(tab.label)}
        {#if tab.count && tab.count() > 0}<span class="count">{tab.count()}</span>{/if}
      </button>
    {/each}
  </nav>

  <div class="right">
    <button class="ghost" onclick={refresh} disabled={refreshing} title={t("nav.refresh")}>
      <span class:spin={refreshing}>⟳</span> {t("nav.refresh")}
    </button>
    <button class:active={app.view === "settings"} class="tab" onclick={() => (app.view = "settings")}>⚙ {t("nav.settings")}</button>
  </div>
</header>

<style>
  header {
    display: flex;
    align-items: center;
    gap: 1.5rem;
    padding: 0 1.25rem;
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 0.7rem;
    cursor: pointer;
    min-width: 220px;
  }
  .brand img {
    height: 34px;
    max-width: 140px;
    object-fit: contain;
  }
  .logo {
    width: 34px;
    height: 34px;
    border-radius: 10px;
    background: linear-gradient(135deg, var(--color-primary), var(--color-accent));
  }
  .titles {
    display: flex;
    flex-direction: column;
    line-height: 1.15;
  }
  .titles small {
    color: var(--color-text-muted);
    font-size: 0.75rem;
  }
  nav {
    display: flex;
    gap: 0.25rem;
    flex: 1;
  }
  .tab {
    background: transparent;
    border-color: transparent;
    padding: 0.5em 1.1em;
    font-weight: 600;
    color: var(--color-text-muted);
    display: inline-flex;
    gap: 0.5em;
    align-items: center;
  }
  .tab:hover:not(:disabled) {
    color: var(--color-text);
    background: var(--color-surface-alt);
    border-color: transparent;
  }
  .tab.active {
    color: var(--color-text);
    background: var(--color-surface-alt);
    box-shadow: inset 0 -3px 0 var(--color-primary);
  }
  .count {
    background: var(--color-primary);
    color: var(--color-primary-text);
    border-radius: 999px;
    font-size: 0.72rem;
    padding: 0 0.5em;
  }
  .right {
    display: flex;
    gap: 0.4rem;
  }
  .spin {
    display: inline-block;
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
