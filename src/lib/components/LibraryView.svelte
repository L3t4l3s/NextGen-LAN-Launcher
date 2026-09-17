<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { t } from "$lib/i18n";
  import GameCard from "./GameCard.svelte";
  import GameDetail from "./GameDetail.svelte";
  import { isBusy, isPlayable } from "$lib/phase";

  // In the store, not here: this component is destroyed on every tab switch,
  // and with it went the sort order the user had chosen.
  const view = $derived(app.libraryView);

  /** "8" / "16-32" / "bis 64" → the largest number in the text, 0 when none. */
  function playerCount(value: string | null): number {
    const numbers = (value ?? "").match(/\d+/g);
    return numbers ? Math.max(...numbers.map(Number)) : 0;
  }

  /** "2004" / "2004 (Remake)" / "" → the first four-digit year, 0 when none. */
  function releaseYear(value: string | null): number {
    const year = (value ?? "").match(/\d{4}/);
    return year ? Number(year[0]) : 0;
  }

  const genres = $derived([...new Set(app.games.map((g) => g.genre).filter((g): g is string => !!g))].sort());

  const visible = $derived.by(() => {
    const q = view.query.trim().toLowerCase();
    // filter() already returns a fresh array, so sorting it in place is safe.
    return app.games
      .filter((g) => !q || g.title.toLowerCase().includes(q) || g.id.includes(q) || (g.publisher ?? "").toLowerCase().includes(q))
      .filter((g) => !view.genre || g.genre === view.genre)
      .filter((g) => {
        if (view.filter === "all") return true;
        const s = app.statusOf(g.id);
        return view.filter === "installed" ? isPlayable(s) : isBusy(s) || s?.phase === "failed" || s?.phase === "paused";
      })
      .sort((a, b) => {
        switch (view.sort) {
          // Descending for the numbers: the biggest, the most players and the
          // newest are what people look for.
          case "players":
            return playerCount(b.maxPlayers) - playerCount(a.maxPlayers);
          case "size":
            return b.sizeBytes - a.sizeBytes;
          case "year":
            return releaseYear(b.releaseYear) - releaseYear(a.releaseYear);
          default:
            return a.title.localeCompare(b.title, app.settings?.language ?? "de");
        }
      });
  });
</script>

<div class="library" class:with-detail={!!app.selected}>
  <section class="grid-area">
    <div class="toolbar">
      <input type="search" placeholder={t("library.search")} bind:value={view.query} />
      <div class="segments">
        {#each ["all", "installed", "active"] as f (f)}
          <button class:active={view.filter === f} onclick={() => (view.filter = f as typeof view.filter)}>{t(`library.filter.${f}`)}</button>
        {/each}
      </div>
      <select bind:value={view.genre} aria-label={t("library.filter.genre")}>
        <option value="">{t("library.filter.genre.all")}</option>
        {#each genres as g (g)}
          <option value={g}>{g}</option>
        {/each}
      </select>
      <select bind:value={view.sort} aria-label={t("library.sort")}>
        {#each ["title", "players", "size", "year"] as option (option)}
          <option value={option}>{t(`library.sort.${option}`)}</option>
        {/each}
      </select>
      <span class="muted count">{t("library.count", { count: visible.length })}</span>
    </div>

    {#if app.games.length === 0}
      <div class="empty card">
        <h2>{t("library.empty.title")}</h2>
        <p class="muted">{t("library.empty.text")}</p>
        <button class="primary" onclick={() => (app.view = "diagnostics")}>{t("nav.diagnostics")}</button>
      </div>
    {:else if visible.length === 0}
      <p class="muted empty">{t("library.nothing_found")}</p>
    {:else}
      <div class="grid">
        {#each visible as game (game.id)}
          <GameCard {game} selected={app.selectedId === game.id} onselect={(id) => (app.selectedId = app.selectedId === id ? null : id)} />
        {/each}
      </div>
    {/if}
    {#if app.bootstrap?.demo}<p class="hint demo">{t("library.empty.demo")}</p>{/if}
  </section>

  {#if app.selected}
    <aside>
      <GameDetail game={app.selected} onclose={() => (app.selectedId = null)} />
    </aside>
  {/if}
</div>

<style>
  .library {
    display: grid;
    grid-template-columns: 1fr;
    height: 100%;
    min-height: 0;
  }
  .library.with-detail {
    grid-template-columns: 1fr minmax(360px, 430px);
  }
  .grid-area {
    overflow: auto;
    padding: 1.25rem 1.5rem 2rem;
    min-height: 0;
  }
  .toolbar {
    display: flex;
    gap: 0.75rem;
    align-items: center;
    margin-bottom: 1rem;
    flex-wrap: wrap;
  }
  .toolbar input {
    flex: 1;
    min-width: 200px;
  }
  .segments {
    display: inline-flex;
    border: 1px solid var(--color-border);
    border-radius: calc(var(--radius) * 0.7);
    overflow: hidden;
  }
  .segments button {
    border: none;
    border-radius: 0;
    background: transparent;
    color: var(--color-text-muted);
  }
  .segments button.active {
    background: var(--color-surface-alt);
    color: var(--color-text);
  }
  .count {
    font-size: 0.85rem;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(168px, 1fr));
    gap: 1rem;
  }
  aside {
    border-left: 1px solid var(--color-border);
    background-color: var(--color-surface);
    background-image: var(--surface-pattern, none);
    background-size: var(--surface-pattern-size, auto);
    overflow: auto;
    min-height: 0;
  }
  .empty {
    margin: 3rem auto;
    max-width: 520px;
    text-align: center;
  }
  .demo {
    margin-top: 1.5rem;
    text-align: center;
  }
</style>
