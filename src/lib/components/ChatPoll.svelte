<script lang="ts">
  import { api, coverSrc } from "$lib/api";
  import { percent } from "$lib/chat";
  import { t, userText } from "$lib/i18n";
  import { app } from "$lib/stores/app.svelte";
  import type { ChatItem, ChatPoll } from "$lib/types";

  let { item, poll }: { item: ChatItem; poll: ChatPoll } = $props();
  let adding = $state(false);
  let newOption = $state("");
  let busy = $state(false);

  const most = $derived(Math.max(0, ...poll.options.map((o) => o.voters.length)));
  const voted = $derived(poll.options.some((o) => o.mine));

  async function run(action: () => Promise<void>) {
    busy = true;
    try {
      await action();
    } catch (e) {
      app.toast("error", userText(e));
    } finally {
      busy = false;
    }
  }

  function choose(id: string) {
    if (poll.closed) return;
    const mine = poll.options.filter((o) => o.mine).map((o) => o.id);
    let next: string[];
    if (poll.kind === "single") next = mine.includes(id) ? [] : [id];
    else next = mine.includes(id) ? mine.filter((m) => m !== id) : [...mine, id];
    void run(() => api.chat.vote(item.id, next));
  }

  function addOption() {
    const text = newOption.trim();
    if (!text) return;
    void run(async () => {
      await api.chat.addPollOption(item.id, text, null);
      newOption = "";
      adding = false;
    });
  }

  const cover = (game: string | null) => (game ? coverSrc(app.games.find((g) => g.id === game)?.cover ?? null) : null);
</script>

<div class="poll">
  <div class="head">
    <span class="icon">📊</span>
    <strong class="question">{poll.question}</strong>
  </div>
  <div class="kind">
    {poll.kind === "single" ? t("chat.poll.kind.single") : t("chat.poll.kind.multiple")}{poll.open ? ` · ${t("chat.poll.open_hint")}` : ""}
    {#if poll.closed}<span class="badge">{t("chat.poll.closed")}</span>{/if}
  </div>
  <ul>
    {#each poll.options as o (o.id)}
      {@const share = percent(o.voters.length, poll.participants)}
      {@const src = cover(o.game)}
      <li>
        <button
          class="option"
          class:mine={o.mine}
          class:winner={poll.closed && o.voters.length === most && most > 0}
          disabled={poll.closed || busy}
          title={o.voters.length ? o.voters.join(", ") : t("chat.poll.no_votes")}
          onclick={() => choose(o.id)}
        >
          <span class="bar" style:width={`${voted || poll.closed ? share : 0}%`}></span>
          <span class="check">{poll.kind === "single" ? (o.mine ? "◉" : "○") : o.mine ? "☑" : "☐"}</span>
          {#if src}<img src={src} alt="" />{:else if o.game}<span class="game">🎮</span>{/if}
          <span class="text">{o.text}{#if o.addedBy}<small> · {o.addedBy}</small>{/if}</span>
          {#if voted || poll.closed}<span class="count">{o.voters.length} · {share}%</span>{/if}
        </button>
      </li>
    {/each}
  </ul>
  <div class="foot">
    <span class="participants">{t("chat.poll.participants", { count: poll.participants })}</span>
    {#if !poll.closed && (poll.open || item.mine)}
      <button class="link" onclick={() => (adding = !adding)}>+ {t("chat.poll.add_option")}</button>
    {/if}
    {#if !poll.closed && item.mine}
      <button class="link" disabled={busy} onclick={() => run(() => api.chat.closePoll(item.id))}>{t("chat.poll.close")}</button>
    {/if}
  </div>
  {#if adding}
    <form class="add" onsubmit={(e) => { e.preventDefault(); addOption(); }}>
      <input bind:value={newOption} maxlength="120" placeholder={t("chat.poll.option_placeholder")} />
      <button type="submit" disabled={!newOption.trim() || busy}>{t("action.save")}</button>
    </form>
  {/if}
</div>

<style>
  .poll {
    min-width: 220px;
  }
  .head {
    display: flex;
    gap: 0.4rem;
    align-items: flex-start;
  }
  .question {
    overflow-wrap: anywhere;
  }
  .kind {
    font-size: 0.75rem;
    opacity: 0.75;
    margin: 0.15rem 0 0.4rem;
    display: flex;
    gap: 0.4rem;
    align-items: center;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
  }
  .option {
    position: relative;
    overflow: hidden;
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.45rem;
    text-align: left;
    padding: 0.4em 0.6em;
    background: color-mix(in srgb, var(--color-bg) 55%, transparent);
    border-color: var(--color-border);
  }
  .option:disabled {
    opacity: 1;
  }
  .option.mine {
    border-color: var(--color-primary);
  }
  .option.winner {
    border-color: var(--color-success);
  }
  .bar {
    position: absolute;
    inset: 0 auto 0 0;
    background: color-mix(in srgb, var(--color-primary) 28%, transparent);
    transition: width 0.4s ease;
    z-index: 0;
  }
  .winner .bar {
    background: color-mix(in srgb, var(--color-success) 30%, transparent);
  }
  .option > :not(.bar) {
    position: relative;
    z-index: 1;
  }
  .check {
    opacity: 0.8;
  }
  img {
    width: 42px;
    height: 30px;
    object-fit: cover;
    border-radius: 4px;
  }
  .text {
    flex: 1;
    overflow-wrap: anywhere;
  }
  .text small {
    opacity: 0.65;
  }
  .count {
    font-size: 0.75rem;
    opacity: 0.85;
    white-space: nowrap;
  }
  .foot {
    display: flex;
    flex-wrap: wrap;
    gap: 0.2rem 0.8rem;
    align-items: center;
    margin-top: 0.4rem;
    font-size: 0.78rem;
  }
  .participants {
    flex: 1 0 100%;
    opacity: 0.75;
  }
  .link {
    white-space: nowrap;
    background: none;
    border: none;
    padding: 0;
    color: inherit;
    text-decoration: underline dotted;
    font-size: inherit;
  }
  .add {
    display: flex;
    gap: 0.3rem;
    margin-top: 0.4rem;
  }
  .add input {
    flex: 1;
    min-width: 0;
    padding: 0.3em 0.5em;
  }
  .add button {
    padding: 0.3em 0.7em;
  }
</style>
