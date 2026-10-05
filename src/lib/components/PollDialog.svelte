<script lang="ts">
  import { api, coverSrc } from "$lib/api";
  import { t, userText } from "$lib/i18n";
  import { app } from "$lib/stores/app.svelte";
  import { isTopic } from "$lib/chat";
  import { chat } from "$lib/stores/chat.svelte";
  import type { PollKind } from "$lib/types";

  let { onclose }: { onclose: () => void } = $props();

  type Preset = "single" | "multiple" | "games" | "yesno";
  const presets: { id: Preset; icon: string }[] = [
    { id: "single", icon: "🔘" },
    { id: "multiple", icon: "☑️" },
    { id: "games", icon: "🎮" },
    { id: "yesno", icon: "👍" },
  ];

  const MAX = 12;
  let preset = $state<Preset>("single");
  let question = $state("");
  let options = $state<string[]>(["", ""]);
  let games = $state<string[]>([]);
  /** Games that are not in the library, typed in by hand. */
  let extraGames = $state<string[]>([]);
  let extraGame = $state("");
  let gameFilter = $state("");
  let multiple = $state(false);
  let open = $state(false);
  let busy = $state(false);

  const shownGames = $derived(
    app.games
      .filter((g) => !gameFilter.trim() || g.title.toLowerCase().includes(gameFilter.trim().toLowerCase()))
      .sort((a, b) => a.title.localeCompare(b.title)),
  );

  function pick(p: Preset) {
    preset = p;
    multiple = p === "multiple";
    if (p === "yesno") {
      options = [t("chat.poll.yes"), t("chat.poll.no"), t("chat.poll.maybe")];
    } else if (options.every((o) => [t("chat.poll.yes"), t("chat.poll.no"), t("chat.poll.maybe")].includes(o))) {
      options = ["", ""];
    }
    if (p === "games" && !question.trim()) question = t("chat.poll.games.question");
  }

  function toggleGame(id: string) {
    games = games.includes(id) ? games.filter((g) => g !== id) : games.length + extraGames.length < MAX ? [...games, id] : games;
  }

  function addExtraGame() {
    const name = extraGame.trim();
    if (!name || games.length + extraGames.length >= MAX) return;
    if (!extraGames.some((g) => g.toLowerCase() === name.toLowerCase())) extraGames = [...extraGames, name];
    extraGame = "";
  }

  const choices = $derived(
    preset === "games"
      ? [
          ...games.map((id): { text: string; game: string | null } => ({ text: app.games.find((g) => g.id === id)?.title ?? id, game: id })),
          ...extraGames.map((text) => ({ text, game: null })),
        ]
      : options.map((o) => o.trim()).filter(Boolean).map((text) => ({ text, game: null })),
  );
  const valid = $derived(question.trim().length > 0 && choices.length >= 2 && new Set(choices.map((c) => c.text.toLowerCase())).size === choices.length);

  async function create() {
    if (!valid) return;
    busy = true;
    const kind: PollKind = multiple ? "multiple" : "single";
    try {
      await api.chat.createPoll(chat.active, question.trim(), choices, kind, open);
      onclose();
    } catch (e) {
      app.toast("error", userText(e));
    } finally {
      busy = false;
    }
  }
</script>

<div class="modal-backdrop" role="presentation" onclick={onclose} onkeydown={(e) => e.key === "Escape" && onclose()}>
  <div class="modal card" role="dialog" tabindex="-1" aria-labelledby="poll-title" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
    <h2 id="poll-title">{t("chat.poll.create")}</h2>
    <p class="hint">
      {chat.active === null
        ? t("chat.poll.public")
        : isTopic(chat.active)
          ? t("chat.poll.topic", { name: chat.conversationName(chat.active) })
          : t("chat.poll.private_to", { nick: chat.conversationName(chat.active) })}
    </p>

    <div class="presets">
      {#each presets as p (p.id)}
        <button class="preset" class:active={preset === p.id} onclick={() => pick(p.id)}>
          <span>{p.icon}</span>
          <strong>{t(`chat.poll.preset.${p.id}`)}</strong>
          <small>{t(`chat.poll.preset.${p.id}.hint`)}</small>
        </button>
      {/each}
    </div>

    <label for="poll-question">{t("chat.poll.question")}</label>
    <input id="poll-question" bind:value={question} maxlength="300" placeholder={t("chat.poll.question_placeholder")} />

    {#if preset === "games"}
      <div class="row games-head">
        <label for="poll-games" class="grow">{t("chat.poll.games", { count: games.length + extraGames.length, max: MAX })}</label>
        <input id="poll-games" class="filter" bind:value={gameFilter} placeholder={t("library.search")} />
      </div>
      <ul class="games">
        {#each shownGames as g (g.id)}
          {@const src = coverSrc(g.cover)}
          <li>
            <button class:chosen={games.includes(g.id)} onclick={() => toggleGame(g.id)}>
              <span class="check">{games.includes(g.id) ? "☑" : "☐"}</span>
              {#if src}<img src={src} alt="" />{/if}
              <span class="grow">{g.title}</span>
            </button>
          </li>
        {:else}
          <li class="muted">{t("library.nothing_found")}</li>
        {/each}
      </ul>
      <span class="label">{t("chat.poll.extra_games")}</span>
      {#if extraGames.length}
        <div class="extras">
          {#each extraGames as g (g)}
            <span class="extra">🎮 {g}<button class="ghost" title={t("chat.poll.remove_option")} onclick={() => (extraGames = extraGames.filter((x) => x !== g))}>✕</button></span>
          {/each}
        </div>
      {/if}
      <form class="row extra-form" onsubmit={(e) => { e.preventDefault(); addExtraGame(); }}>
        <input class="grow" bind:value={extraGame} maxlength="120" placeholder={t("chat.poll.extra_placeholder")} aria-label={t("chat.poll.extra_games")} />
        <button type="submit" class="ghost" disabled={!extraGame.trim() || games.length + extraGames.length >= MAX}>+ {t("chat.poll.add_option")}</button>
      </form>
    {:else}
      <span class="label">{t("chat.poll.options")}</span>
      <div class="options">
        {#each options as _, i (i)}
          <div class="row">
            <input class="grow" bind:value={options[i]} maxlength="120" placeholder={t("chat.poll.option_n", { n: i + 1 })} aria-label={t("chat.poll.option_n", { n: i + 1 })} />
            {#if options.length > 2}<button class="ghost" title={t("chat.poll.remove_option")} onclick={() => (options = options.filter((_, j) => j !== i))}>✕</button>{/if}
          </div>
        {/each}
        {#if options.length < MAX}
          <button class="ghost add" onclick={() => (options = [...options, ""])}>+ {t("chat.poll.add_option")}</button>
        {/if}
      </div>
    {/if}

    <label class="check-line"><input type="checkbox" bind:checked={multiple} /> {t("chat.poll.allow_multiple")}</label>
    {#if preset !== "games"}
      <label class="check-line"><input type="checkbox" bind:checked={open} /> {t("chat.poll.allow_add")}</label>
    {/if}

    <div class="row buttons">
      <span class="grow"></span>
      <button class="ghost" onclick={onclose}>{t("action.cancel")}</button>
      <button class="primary" disabled={!valid || busy} onclick={create}>{t("chat.poll.submit")}</button>
    </div>
  </div>
</div>

<style>
  .modal {
    width: min(520px, 94vw);
    max-height: 90vh;
    overflow-y: auto;
  }
  .presets {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 0.4rem;
    margin: 0.4rem 0 0.9rem;
  }
  .preset {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.15rem;
    padding: 0.5em 0.3em;
    text-align: center;
  }
  .preset span {
    font-size: 1.3rem;
  }
  .preset small {
    font-size: 0.7rem;
    color: var(--color-text-muted);
    line-height: 1.2;
  }
  .preset.active {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 18%, var(--color-surface-alt));
  }
  input {
    width: 100%;
  }
  .label {
    display: block;
    font-weight: 600;
    margin: 0.8rem 0 0.3rem;
  }
  .options {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }
  .options .row {
    flex-wrap: nowrap;
  }
  .add {
    align-self: flex-start;
  }
  .games-head {
    margin-top: 0.8rem;
  }
  .games-head label {
    margin: 0;
  }
  .filter {
    width: 160px;
  }
  .games {
    list-style: none;
    margin: 0.3rem 0 0;
    padding: 0;
    max-height: 230px;
    overflow-y: auto;
    border: 1px solid var(--color-border);
    border-radius: calc(var(--radius) * 0.6);
  }
  .games button {
    width: 100%;
    display: flex;
    gap: 0.5rem;
    align-items: center;
    text-align: left;
    border: none;
    border-radius: 0;
    background: transparent;
    padding: 0.35em 0.6em;
  }
  .games button.chosen {
    background: color-mix(in srgb, var(--color-primary) 18%, transparent);
  }
  .games img {
    width: 42px;
    height: 30px;
    object-fit: cover;
    border-radius: 4px;
  }
  .games li.muted {
    padding: 0.5em 0.7em;
  }
  .extras {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
    margin-bottom: 0.4rem;
  }
  .extra {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    background: var(--color-surface-alt);
    border-radius: 999px;
    padding: 0.1em 0.2em 0.1em 0.7em;
    font-size: 0.85rem;
  }
  .extra button {
    padding: 0.1em 0.4em;
    font-size: 0.7rem;
  }
  .extra-form {
    flex-wrap: nowrap;
  }
  .check-line {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    font-weight: 400;
    margin-top: 0.7rem;
  }
  .check-line input {
    width: auto;
  }
  .buttons {
    margin-top: 1rem;
  }
</style>
