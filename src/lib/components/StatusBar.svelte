<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { formatRate } from "$lib/format";
  import { t, userText } from "$lib/i18n";
  import { transportLevel } from "$lib/transport-status";

  const level = $derived(transportLevel(app.health));
  // Live: a saved override is always valid (the backend rejects bad keys).
  const keyConfigured = $derived(!!app.bootstrap?.builtinCatalogKey || !!app.settings?.catalogKey);
  // "found" / "missing" come from the catalog share's peers; folder mode and a
  // missing key cannot tell and say so instead of claiming "no server".
  const server = $derived.by((): { key: string; link?: "settings" } | null => {
    const h = app.health;
    if (!h || h.kind === "demo") return null;
    if (h.server_found === true) return { key: "status.server.found" };
    if (level === "preparing") return null;
    if (h.server_found === false) return { key: "status.server.missing" };
    if (h.kind === "resilio" && !keyConfigured) return { key: "status.server.no_key", link: "settings" };
    return { key: "status.server.unknown" };
  });
  const label = $derived(
    level === "preparing" ? t(`status.sync.${app.health?.activity ?? "starting"}`) : app.health?.kind === "demo" ? t("status.sync.demo") : level === "ok" ? t("status.sync.ok") : level === "warn" ? t("status.sync.warn") : t("status.sync.error"),
  );
  const problems = $derived(Object.values(app.statuses).filter((s) => s.problem).length);
</script>

<footer>
  <span class="dot {level}"></span>
  <span>{label}</span>
  {#if server}
    <span class="sep">·</span>
    {#if server.link}
      <button class="link" onclick={() => (app.view = "settings")}>{t(server.key)}</button>
    {:else}
      <span>{t(server.key)}</span>
    {/if}
  {/if}
  {#if app.health}
    <span class="sep">·</span>
    <span>{t("status.peers", { count: app.health.peers })}</span>
    {#if app.health.kind !== "folder"}
      <!-- Folder mode: somebody else's sync client moves the bytes, so a
           reassuring "0 B/s" would be a lie rather than an answer. -->
      <span class="sep">·</span>
      <span title={t("status.rates")}>↓ {formatRate(app.health.download_bps ?? 0)} ↑ {formatRate(app.health.upload_bps ?? 0)}</span>
    {/if}
    {#if app.health.lan_mode}<span class="sep">·</span><span>{t("lan.transport.lan_mode")}</span>{/if}
  {/if}
  {#if app.bootstrap?.transportError}
    <span class="sep">·</span>
    <button class="link" onclick={() => (app.view = "diagnostics")}>{userText(app.bootstrap.transportError)}</button>
  {/if}
  <span class="grow"></span>
  {#if problems > 0}
    <button class="link warn" onclick={() => (app.view = "downloads")}>⚠ {t("status.problems", { count: problems })}</button>
    <span class="sep">·</span>
  {/if}
  {#if app.settings?.playerName}<span>{app.settings.playerName}</span><span class="sep">·</span>{/if}
  <span class="dim">v{app.bootstrap?.version}{app.bootstrap?.demo ? " · Demo" : ""}</span>
</footer>

<style>
  .dot.preparing { background: var(--color-primary); }
  footer {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0 1.25rem;
    font-size: 0.82rem;
    background: var(--color-footer);
    border-top: 1px solid var(--color-border);
    color: var(--color-footer-text);
  }
  .sep {
    opacity: 0.5;
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    color: inherit;
    font-size: inherit;
    text-decoration: underline dotted;
  }
  .link.warn {
    color: var(--color-warning);
  }
  /* Not the global `.muted`: that one paints the body's muted colour, which
     need not read on a status bar the theme coloured. */
  .dim {
    opacity: 0.75;
  }
</style>
