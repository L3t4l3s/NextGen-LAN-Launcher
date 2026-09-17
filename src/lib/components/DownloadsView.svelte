<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { api, confirmDialog } from "$lib/api";
  import { t, userText } from "$lib/i18n";
  import { formatBytes, formatPercent, formatSpeed, percentWidth } from "$lib/format";
  import type { GameStatus, SharePeer } from "$lib/types";
  import ProblemCard from "./ProblemCard.svelte";
  import Sparkline from "./Sparkline.svelte";

  const items = $derived(
    app.games
      .map((g) => ({ game: g, status: app.statusOf(g.id) }))
      .filter((x) => x.status && !["not_installed", "ready", "update_available"].includes(x.status.phase)),
  );

  // One open panel at a time; its sources are polled only while it is open
  // and never twice at once (a slow engine would otherwise pile up calls).
  /** Bytes are moving over the network: only then do rate and sources mean anything. */
  const transferring = (status: GameStatus) => ["queued", "syncing", "paused"].includes(status.phase);

  let expanded = $state<string | null>(null);

  // The panel disappears when a game moves on to verifying or leaves the list
  // altogether; without this its peer poll would keep running every two
  // seconds with nothing to show.
  $effect(() => {
    const open = items.find((x) => x.game.id === expanded);
    if (expanded && (!open?.status || !transferring(open.status))) expanded = null;
  });
  let peers = $state<SharePeer[]>([]);
  let peersLoaded = $state(false);

  $effect(() => {
    const id = expanded;
    // Also on a switch between two games: the previous game's sources must
    // not stay on screen until the first poll for the new one answers.
    peers = [];
    peersLoaded = false;
    if (!id) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      const list = await api.sharePeers(id).catch(() => [] as SharePeer[]);
      if (!active) return;
      peers = list;
      peersLoaded = true;
      timer = setTimeout(() => void poll(), 2000);
    };
    void poll();
    return () => {
      active = false;
      clearTimeout(timer);
    };
  });

  async function act(fn: () => Promise<unknown>) {
    try {
      await fn();
    } catch (e) {
      app.toast("error", userText(e));
    }
  }

  // Cancelling drops the share and the partial archive. An installed version
  // underneath (a cancelled update) keeps its files and savegames, which the
  // backend reports back so the message matches what happened.
  async function cancel(game: { id: string; title: string }) {
    if (!(await confirmDialog(t("downloads.cancel.confirm", { title: game.title })))) return;
    await act(async () => {
      const kept = await api.cancelDownload(game.id);
      app.toast("info", t(kept ? "downloads.cancel.kept" : "downloads.cancel.done", { title: game.title }));
      await app.reloadGames();
    });
  }
</script>

<!-- No heading: the tab above already says where you are. -->
<div class="page downloads">
  {#if items.length === 0 && app.hintGames.length === 0}
    <div class="card empty">
      <p class="muted">{t("downloads.empty")}</p>
      <button class="primary" onclick={() => (app.view = "library")}>{t("nav.library")}</button>
    </div>
  {:else if items.length === 0}
    <p class="muted">{t("downloads.empty")}</p>
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
              <span style:width={percentWidth(status.progress)}></span>
            </div>
            <div class="row small muted">
              <span>{formatPercent(status.progress)}</span>
              <!-- Bytes, rate and sources describe a transfer. While the
                   launcher checks or unpacks an archive none of them moves,
                   and showing them there reads as a stuck download. -->
              {#if transferring(status)}
                <span>{t("detail.progress", { done: formatBytes(status.bytesDone), total: formatBytes(status.bytesTotal) })}</span>
                {#if status.downloadBps}<span>{formatSpeed(status.downloadBps)}</span>{/if}
                <span>{t("detail.peers", { count: status.peers })}</span>
                <!-- Which disk it is going to: with several library folders
                     that is the first question when something looks wrong. -->
                {#if game.shareDir}<span class="path" title={game.shareDir}>{game.shareDir}</span>{/if}
              {/if}
              <span class="grow"></span>
              <button class="ghost" onclick={() => act(() => api.repair(game.id))}>🛠 {t("action.repair")}</button>
              {#if status.phase === "paused"}
                <button class="ghost" onclick={() => act(() => api.pause(game.id, false))}>{t("action.resume")}</button>
              {:else}
                <button class="ghost" onclick={() => act(() => api.pause(game.id, true))}>{t("action.pause")}</button>
              {/if}
              <button class="ghost danger" onclick={() => cancel(game)}>{t("downloads.cancel")}</button>
              <button class="ghost" onclick={() => { app.selectedId = game.id; app.view = "library"; }}>{t("action.open")}</button>
              {#if transferring(status)}
                <button class="ghost" aria-expanded={expanded === game.id} onclick={() => (expanded = expanded === game.id ? null : game.id)}>
                  {expanded === game.id ? "▾" : "▸"} {t("downloads.details")}
                </button>
              {/if}
            </div>
            {#if expanded === game.id && transferring(status)}
              <div class="details">
                <Sparkline values={app.speedHistory[game.id] ?? []} format={formatSpeed} label={t("downloads.speed_history")} />
                <div class="sources">
                  <strong class="small">{t("downloads.sources.title")}</strong>
                  {#if peers.length}
                    <ul>
                      {#each peers as peer, index (index)}
                        <li>
                          <span class="grow">{peer.name}</span>
                          {#if peer.connection}<span class="muted small">{peer.connection}</span>{/if}
                          <span class="muted small">{peer.synced ? t("downloads.sources.synced") : t("downloads.sources.partial")}</span>
                          <span class="rate">{peer.downloadBps ? `↓ ${formatSpeed(peer.downloadBps)}` : ""}</span>
                        </li>
                      {/each}
                    </ul>
                  {:else if peersLoaded}
                    <p class="muted small">{app.health?.peer_details ? t("downloads.sources.empty") : t("downloads.sources.unavailable")}</p>
                  {/if}
                </div>
              </div>
            {/if}
            {#if status.problem}
              <ProblemCard problem={status.problem} compact />
            {/if}
          </div>
        {/if}
      {/each}
    </div>
  {/if}

  {#if app.hintGames.length > 0}
    <h2>{t("downloads.hints.title")}</h2>
    <p class="hint">{t("downloads.hints.text")}</p>
    <div class="stack">
      {#each app.hintGames as game (game.id)}
        {@const status = app.statusOf(game.id)}
        {#if status?.problem}
          <div class="card item">
            <div class="row">
              <strong class="grow">{game.title}</strong>
              <span class="badge ready">{t(`phase.${status.phase}`)}</span>
              <button class="ghost" onclick={() => { app.selectedId = game.id; app.view = "library"; }}>{t("action.open")}</button>
            </div>
            <ProblemCard problem={status.problem} />
          </div>
        {/if}
      {/each}
    </div>
  {/if}
</div>

<style>
  /* A column in the middle of the window rather than a wide band across it:
     a download row is a line of text, not a table. */
  .downloads {
    max-width: 900px;
  }
  .path {
    max-width: 22em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .details {
    display: grid;
    gap: 0.9rem;
    grid-template-columns: minmax(220px, 1fr) minmax(220px, 1.2fr);
    padding: 0.7rem 0 0.2rem;
  }
  @media (max-width: 720px) {
    .details {
      grid-template-columns: 1fr;
    }
  }
  .sources ul {
    list-style: none;
    margin: 0.3rem 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .sources li {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    font-size: 0.85rem;
  }
  .sources .rate {
    font-variant-numeric: tabular-nums;
  }
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
