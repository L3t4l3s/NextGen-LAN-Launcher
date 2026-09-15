<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n";

  const level = $derived.by(() => {
    const h = app.health;
    if (!h) return "warn";
    if (h.kind === "demo") return "ok";
    if (h.kind === "folder") return "warn";
    if (!h.running || !h.api_reachable) return "error";
    if (h.peers === 0) return "warn";
    return "ok";
  });
  const label = $derived(
    app.health?.kind === "demo" ? t("status.sync.demo") : level === "ok" ? t("status.sync.ok") : level === "warn" ? t("status.sync.warn") : t("status.sync.error"),
  );
  const problems = $derived(Object.values(app.statuses).filter((s) => s.problem).length);
</script>

<footer>
  <span class="dot {level}"></span>
  <span>{label}</span>
  {#if app.health}
    <span class="sep">·</span>
    <span>{t("status.peers", { count: app.health.peers })}</span>
    {#if app.health.lan_mode}<span class="sep">·</span><span>{t("lan.transport.lan_mode")}</span>{/if}
  {/if}
  {#if app.bootstrap?.transportError}
    <span class="sep">·</span>
    <button class="link" onclick={() => (app.view = "diagnostics")}>{app.bootstrap.transportError}</button>
  {/if}
  <span class="grow"></span>
  {#if problems > 0}
    <button class="link warn" onclick={() => (app.view = "downloads")}>⚠ {t("status.problems", { count: problems })}</button>
    <span class="sep">·</span>
  {/if}
  {#if app.settings?.playerName}<span>{app.settings.playerName}</span><span class="sep">·</span>{/if}
  <span class="muted">v{app.bootstrap?.version}{app.bootstrap?.demo ? " · Demo" : ""}</span>
</footer>

<style>
  footer {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0 1.25rem;
    font-size: 0.82rem;
    background: var(--color-surface);
    border-top: 1px solid var(--color-border);
    color: var(--color-text-muted);
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
</style>
