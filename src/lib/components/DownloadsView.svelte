<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { api } from "$lib/api";
  import { t, userText } from "$lib/i18n";
  import { formatBytes, formatPercent, formatSpeed } from "$lib/format";
  import ProblemCard from "./ProblemCard.svelte";

  const items = $derived(
    app.games
      .map((g) => ({ game: g, status: app.statusOf(g.id) }))
      .filter((x) => x.status && !["not_installed", "ready", "update_available"].includes(x.status.phase)),
  );

  async function act(fn: () => Promise<unknown>) {
    try {
      await fn();
    } catch (e) {
      app.toast("error", userText(e));
    }
  }
</script>

<div class="page">
  <h1>{t("downloads.title")}</h1>
  <p class="hint">{t("downloads.hint")}</p>

  {#if items.length === 0}
    <div class="card empty">
      <p class="muted">{t("downloads.empty")}</p>
      <button class="primary" onclick={() => (app.view = "library")}>{t("nav.library")}</button>
    </div>
  {:else}
    <div class="stack">
      {#each items as { game, status } (game.id)}
        {#if status}
          <div class="card item">
            <div class="row">
              <strong class="grow">{game.title}</strong>
              <span class="badge {status.phase === 'failed' ? 'error' : status.stalled || status.phase === 'paused' ? 'warn' : 'busy'}">{t(`phase.${status.phase}`)}</span>
            </div>
            <div class="progress" class:stalled={status.stalled} class:working={!["syncing", "paused"].includes(status.phase)}>
              <span style:width={formatPercent(status.progress)}></span>
            </div>
            <div class="row small muted">
              <span>{formatPercent(status.progress)}</span>
              <span>{t("detail.progress", { done: formatBytes(status.bytesDone), total: formatBytes(status.bytesTotal) })}</span>
              {#if status.downloadBps}<span>{formatSpeed(status.downloadBps)}</span>{/if}
              <span>{t("detail.peers", { count: status.peers })}</span>
              <span class="grow"></span>
              <button class="ghost" onclick={() => act(() => api.repair(game.id))}>🛠 {t("action.repair")}</button>
              {#if status.phase === "paused"}
                <button class="ghost" onclick={() => act(() => api.pause(game.id, false))}>{t("action.resume")}</button>
              {:else}
                <button class="ghost" onclick={() => act(() => api.pause(game.id, true))}>{t("action.pause")}</button>
              {/if}
              <button class="ghost" onclick={() => { app.selectedId = game.id; app.view = "library"; }}>{t("action.open")}</button>
            </div>
            {#if status.problem}
              <ProblemCard problem={status.problem} compact />
            {/if}
          </div>
        {/if}
      {/each}
    </div>
  {/if}
</div>

<style>
  .item {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  .small {
    font-size: 0.85rem;
  }
  .small button {
    font-size: 0.85rem;
    padding: 0.3em 0.7em;
  }
  .empty {
    text-align: center;
    padding: 2.5rem;
  }
</style>
