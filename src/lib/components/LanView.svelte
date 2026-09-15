<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { api } from "$lib/api";
  import { t, userText } from "$lib/i18n";

  const cfg = $derived(app.event?.config ?? null);
  const host = $derived(app.settings?.lanpageHost ?? "launcher.lan");

  async function open(url: string) {
    try {
      await api.openUrl(url);
    } catch (e) {
      app.toast("error", userText(e));
    }
  }

  async function refresh() {
    try {
      app.event = await api.refreshEvent();
    } catch (e) {
      app.toast("error", userText(e));
    }
  }
</script>

<div class="page">
  <div class="row">
    <h1 class="grow">{cfg?.title ?? t("lan.title")}{cfg?.tag ? ` (${cfg.tag})` : ""}</h1>
    <button class="ghost" onclick={refresh}>⟳</button>
  </div>

  {#if !cfg}
    <div class="card">
      <h2>{t("lan.no_event.title")}</h2>
      <p class="muted">{t("lan.no_event.text", { host })}</p>
      <button onclick={() => (app.view = "settings")}>{t("nav.settings")}</button>
    </div>
  {:else}
    <div class="tiles">
      {#if cfg.website}
        <button class="card tile" onclick={() => open(cfg.website!)}><span class="icon">🌐</span><strong>{t("lan.website")}</strong><small>{cfg.website}</small></button>
      {/if}
      {#if cfg.ts3_server}
        <button class="card tile" onclick={() => open(cfg.ts3_server!)}><span class="icon">🎧</span><strong>{t("lan.ts3")}</strong><small>{cfg.ts3_server}</small></button>
      {/if}
      {#if cfg.discord_url}
        <button class="card tile" onclick={() => open(cfg.discord_url!)}><span class="icon">💬</span><strong>{t("lan.discord")}</strong><small>{cfg.discord_url}</small></button>
      {/if}
      {#if cfg.dc_hub}
        <button class="card tile" onclick={() => open(cfg.dc_hub!)}><span class="icon">📂</span><strong>{t("lan.dc_hub")}</strong><small>{cfg.dc_hub}</small></button>
      {/if}
    </div>

    {#if cfg.links.length}
      <h2>{t("lan.links")}</h2>
      <div class="tiles">
        {#each cfg.links as link (link.url)}
          <button class="card tile" onclick={() => open(link.url)}><span class="icon">🔗</span><strong>{link.label}</strong><small>{link.url}</small></button>
        {/each}
      </div>
    {/if}
  {/if}

  <h2>{t("lan.transport")}</h2>
  <div class="card">
    {#if app.health}
      <div class="row">
        <span class="dot {app.health.kind === 'demo' || (app.health.running && app.health.api_reachable && app.health.peers > 0) ? 'ok' : app.health.running ? 'warn' : 'error'}"></span>
        <strong>{t(`lan.transport.kind.${app.health.kind}`)}</strong>
      </div>
      <p class="muted">
        {t("lan.transport.peers", { count: app.health.peers })} · {app.health.lan_mode ? t("lan.transport.lan_mode") : t("lan.transport.internet")}
        {#if app.health.version} · {t("lan.transport.version", { version: app.health.version })}{/if}
      </p>
    {/if}
    {#if app.bootstrap?.transportError}<p class="warn">{userText(app.bootstrap.transportError)}</p>{/if}
  </div>

  <p class="hint soon">{t("lan.coming_soon")}</p>
</div>

<style>
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: 0.9rem;
    margin-bottom: 1.5rem;
  }
  .tile {
    text-align: left;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  .tile .icon {
    font-size: 1.5rem;
  }
  .tile small {
    color: var(--color-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .warn {
    color: var(--color-warning);
  }
  .soon {
    margin-top: 2rem;
  }
</style>
