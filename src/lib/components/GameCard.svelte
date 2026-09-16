<script lang="ts">
  import type { GameView } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { coverSrc } from "$lib/api";
  import { formatPercent, formatSpeed, placeholderGradient, percentWidth } from "$lib/format";
  import { t } from "$lib/i18n";
  import { phaseBadge } from "$lib/phase";

  let { game, selected = false, onselect }: { game: GameView; selected?: boolean; onselect: (id: string) => void } = $props();

  const status = $derived(app.statusOf(game.id));
  const badge = $derived(phaseBadge(status));
  const busy = $derived(status ? ["queued", "syncing", "verifying", "extracting", "setup"].includes(status.phase) : false);
  // A tile is narrow, so the label carries what a bar alone cannot say: how
  // far along it is, and — the case that looks like a hung download — that
  // nothing is arriving because no one is offering the game yet. The full
  // figures are in the detail panel and under Downloads.
  // Which of the two "nothing is arriving" cases this is, is the backend's
  // call: it knows whether anyone offers the game. A tile is too narrow for
  // the sentence, so the short word carries the tooltip with it.
  const trouble = $derived(
    status?.problem?.code === "sync.no_peers"
      ? { label: "card.no_source", title: "problem.sync.no_peers.title" }
      : status?.stalled
        ? { label: "card.stalled", title: "problem.sync.stalled.title" }
        : null,
  );
  const label = $derived.by(() => {
    if (!status) return "";
    if (trouble) return t(trouble.label);
    const speed = status.phase === "syncing" ? formatSpeed(status.downloadBps) : "";
    return speed ? `${formatPercent(status.progress)} · ${speed}` : formatPercent(status.progress);
  });
</script>

<button class="tile" class:selected class:dimmed={game.disabledByEvent} onclick={() => onselect(game.id)}>
  <div class="cover" style:background={game.cover ? undefined : placeholderGradient(game.id)}>
    {#if game.cover}
      <img src={coverSrc(game.cover)} alt="" loading="lazy" />
    {:else}
      <span class="initials">{game.title.slice(0, 2)}</span>
    {/if}
    {#if badge}
      <span class="badge {badge.cls}">{t(badge.label)}</span>
    {/if}
    {#if status?.problem}
      <span class="warnmark" title={t(`problem.${status.problem.code}.title`, status.problem.params)}>!</span>
    {/if}
  </div>
  <div class="meta">
    <strong title={game.title}>{game.title}</strong>
    <small class="muted">{game.genre ?? ""}{game.maxPlayers ? ` · ${game.maxPlayers} ${t("detail.players")}` : ""}</small>
    {#if busy && status}
      <div class="progress" class:stalled={status.stalled} class:working={status.phase !== "syncing"}>
        <span style:width={percentWidth(status.progress)}></span>
      </div>
      <small class="muted label" class:warn-text={!!trouble} title={trouble ? t(trouble.title) : undefined}>{label}</small>
    {/if}
  </div>
</button>

<style>
  /* `.meta small` is more specific than `.label` would be on its own. */
  .meta .label.warn-text {
    color: var(--color-warning);
  }
  .meta .label {
    display: block;
    font-size: 0.75rem;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tile {
    display: flex;
    flex-direction: column;
    text-align: left;
    padding: 0;
    background: var(--color-surface);
    overflow: hidden;
    border-radius: var(--radius);
    transition: transform 0.12s, border-color 0.15s, box-shadow 0.15s;
  }
  .tile:hover {
    transform: translateY(-2px);
    box-shadow: var(--shadow);
  }
  .tile.selected {
    border-color: var(--color-primary);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-primary) 45%, transparent);
  }
  .tile.dimmed {
    opacity: 0.45;
  }
  .cover {
    position: relative;
    aspect-ratio: var(--cover-ratio);
    width: 100%;
    display: grid;
    place-items: center;
  }
  .cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .initials {
    font-size: 2.6rem;
    font-weight: 800;
    color: rgba(255, 255, 255, 0.75);
    letter-spacing: -0.03em;
  }
  .badge {
    position: absolute;
    left: 0.5rem;
    bottom: 0.5rem;
    backdrop-filter: blur(6px);
  }
  .warnmark {
    position: absolute;
    right: 0.5rem;
    top: 0.5rem;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--color-warning);
    color: var(--color-warning-text, #1a1400);
    font-weight: 800;
    display: grid;
    place-items: center;
    font-size: 0.85rem;
  }
  .meta {
    padding: 0.6rem 0.75rem 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    width: 100%;
  }
  .meta strong {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 0.95rem;
  }
  .meta small {
    font-size: 0.78rem;
  }
  .progress {
    margin-top: 0.35rem;
  }
</style>
