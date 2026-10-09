<script lang="ts">
  import type { GameView } from "$lib/types";
  import { app } from "$lib/stores/app.svelte";
  import { api, coverSrc } from "$lib/api";
  import { chat } from "$lib/stores/chat.svelte";
  import { formatSpeed, placeholderGradient } from "$lib/format";
  import { stepLabel } from "$lib/steps";
  import InstallProgress from "./InstallProgress.svelte";
  import { t, userText } from "$lib/i18n";
  import { isPlayable, phaseBadge } from "$lib/phase";

  let { game, selected = false, onselect }: { game: GameView; selected?: boolean; onselect: (id: string) => void } = $props();

  const status = $derived(app.statusOf(game.id));
  // "Ready" is the usual state of an installed game; on the cover it hid
  // the artwork on every tile. The check mark beside the player count says
  // the same, and the cover keeps its badge for the states worth a look.
  const badge = $derived.by(() => {
    const b = phaseBadge(status);
    return b?.cls === "ready" ? null : b;
  });
  const installed = $derived(isPlayable(status));
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
  /** The right-click menu, where it opened. */
  let menu = $state<{ x: number; y: number } | null>(null);
  let tile = $state<HTMLButtonElement | null>(null);

  function openMenu(e: MouseEvent) {
    // Linking needs the chat; without it the browser menu stays suppressed.
    if (!chat.enabled) return;
    e.preventDefault();
    const height = 48 + chat.conversations.length * 34;
    menu = { x: Math.min(e.clientX, window.innerWidth - 240), y: Math.max(8, Math.min(e.clientY, window.innerHeight - height)) };
  }

  /** Link the game in a conversation, and show it there. */
  async function share(conversation: string | null) {
    menu = null;
    try {
      await api.chat.shareGame(conversation, game.id);
      chat.show(conversation);
      chat.atBottom = true;
    } catch (e) {
      if (!chat.notePause(e)) app.toast("error", userText(e));
      chat.show(conversation);
    }
  }

  const label = $derived.by(() => {
    if (!status) return "";
    if (trouble) return t(trouble.label);
    const speed = status.phase === "syncing" ? formatSpeed(status.downloadBps) : "";
    const step = stepLabel(status, true);
    return speed ? `${step} · ${speed}` : step;
  });
</script>

<!-- One menu at a time: a right-click on another tile closes this one. -->
<svelte:window
  onclick={() => (menu = null)}
  oncontextmenu={(e) => { if (!tile?.contains(e.target as Node)) menu = null; }}
  onkeydown={(e) => e.key === "Escape" && (menu = null)}
  onblur={() => (menu = null)}
/>

<button class="tile" bind:this={tile} id={`game-${game.id}`} class:selected class:dimmed={game.disabledByEvent} onclick={() => onselect(game.id)} oncontextmenu={openMenu}>
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
    <div class="facts">
      <small class="muted">{game.genre ?? ""}{game.maxPlayers ? ` · ${game.maxPlayers} ${t("detail.players")}` : ""}</small>
      {#if installed}
        <span class="installed" title={t("phase.ready")} aria-label={t("phase.ready")} role="img">
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M4.2 8.3l2.5 2.5 5-5.3" /></svg>
        </span>
      {/if}
    </div>
    {#if busy && status}
      <div class="bar"><InstallProgress {status} /></div>
      <small class="muted label" class:warn-text={!!trouble} title={trouble ? t(trouble.title) : undefined}>{label}</small>
    {/if}
  </div>
</button>

{#if menu}
  <div class="ctx" role="menu" style:left={`${menu.x}px`} style:top={`${menu.y}px`}>
    <div class="ctx-head">🎮 {t("chat.game.share_in")}</div>
    {#each chat.conversations as c (c.id ?? "")}
      <button role="menuitem" onclick={() => share(c.id)}>
        {c.kind === "private" ? "✉" : "#"} {c.nick}
      </button>
    {/each}
  </div>
{/if}

<style>
  .ctx {
    position: fixed;
    z-index: 50;
    padding: 0.3rem;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }
  .ctx {
    display: flex;
    flex-direction: column;
    min-width: 200px;
    max-height: 70vh;
    overflow-y: auto;
  }
  .ctx-head {
    font-size: 0.75rem;
    color: var(--color-text-muted);
    padding: 0.35em 0.7em 0.25em;
  }
  .ctx button {
    background: transparent;
    border: none;
    text-align: left;
    padding: 0.45em 0.7em;
    border-radius: 6px;
    white-space: nowrap;
  }
  .ctx button:hover {
    background: var(--color-surface-alt);
  }
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
    /* The cover brings its own background, so the theme's figure shows on the
       strip below it — where a LANPage draws it too. */
    background-color: var(--color-surface);
    background-image: var(--surface-pattern, none);
    background-size: var(--surface-pattern-size, auto);
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
  .facts {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    min-height: 18px;
  }
  .facts small {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .installed {
    flex: none;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--color-success);
    display: grid;
    place-items: center;
  }
  .installed svg {
    width: 12px;
    height: 12px;
    fill: none;
    stroke: #fff;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .bar {
    margin-top: 0.35rem;
  }
</style>
