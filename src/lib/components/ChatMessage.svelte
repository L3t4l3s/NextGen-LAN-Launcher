<script lang="ts">
  import { api, confirmDialog } from "$lib/api";
  import { linkTarget, nickColor, onlyEmoji, segments } from "$lib/chat";
  import { quickReactions } from "$lib/emoji";
  import { t, userText } from "$lib/i18n";
  import { app } from "$lib/stores/app.svelte";
  import { chat } from "$lib/stores/chat.svelte";
  import type { ChatItem } from "$lib/types";
  import ChatPoll from "./ChatPoll.svelte";
  import EmojiPicker from "./EmojiPicker.svelte";

  let { item, head, onjump }: { item: ChatItem; head: boolean; onjump: (id: string) => void } = $props();
  let reacting = $state(false);
  let picker = $state(false);

  const time = $derived(new Date(item.ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }));
  const big = $derived(!item.reply && onlyEmoji(item.text));
  const parts = $derived(item.text ? segments(item.text, chat.nicks, chat.nick) : []);
  const myReaction = $derived(item.reactions.find((r) => r.mine)?.emoji ?? "");

  async function react(emoji: string) {
    reacting = false;
    picker = false;
    try {
      // The same emoji again takes it back; another one replaces it.
      await api.chat.react(item.id, emoji === myReaction ? "" : emoji);
    } catch (e) {
      app.toast("error", userText(e));
    }
  }

  async function remove() {
    if (!(await confirmDialog(t("chat.delete.confirm")))) return;
    try {
      await api.chat.remove(item.id);
    } catch (e) {
      app.toast("error", userText(e));
    }
  }

  function reply() {
    chat.replyTo = item;
    document.getElementById("chat-input")?.focus();
  }
</script>

<div class="msg" class:mine={item.mine} class:head id={`msg-${item.id}`} role="group" onmouseleave={() => { reacting = false; }}>
  {#if head && !item.mine}
    <div class="nick" style:color={nickColor(item.from)}>{item.nick}</div>
  {/if}
  <div class="line">
    <div class="bubble" class:big class:deleted={item.deleted} class:poll={!!item.poll}>
      {#if item.reply}
        <button class="quote" onclick={() => item.reply && onjump(item.reply.id)}>
          <strong>{item.reply.nick ?? "…"}</strong>
          <span>{item.reply.deleted ? t("chat.deleted") : (item.reply.text ?? "…")}</span>
        </button>
      {/if}
      {#if item.deleted}
        <em>{t("chat.deleted")}</em>
      {:else if item.poll}
        <ChatPoll {item} poll={item.poll} />
      {:else}
        <span class="text">{#each parts as p, i (i)}{#if p.kind === "link"}<a href={p.text} onclick={(e) => { e.preventDefault(); void api.openUrl(linkTarget(p.text)).catch((err) => app.toast("error", userText(err))); }}>{p.text}</a>{:else if p.kind === "mention"}<span class="mention" class:me={p.me}>{p.text}</span>{:else}{p.text}{/if}{/each}</span>
      {/if}
      <span class="time">{time}</span>
    </div>
    {#if !item.deleted}
      <div class="actions">
        <button title={t("chat.reply")} onclick={reply}>↩</button>
        <button title={t("chat.react")} onclick={() => (reacting = !reacting)}>☺</button>
        {#if item.mine}<button title={t("chat.delete")} onclick={remove}>🗑</button>{/if}
      </div>
    {/if}
  </div>
  {#if reacting}
    <div class="quick">
      {#each quickReactions as e (e)}
        <button class:chosen={e === myReaction} onclick={() => react(e)}>{e}</button>
      {/each}
      <button class="more" title={t("chat.emoji")} onclick={() => { picker = true; reacting = false; }}>＋</button>
    </div>
  {/if}
  {#if picker}
    <div class="picker-anchor">
      <EmojiPicker onpick={react} onclose={() => (picker = false)} />
    </div>
  {/if}
  {#if item.reactions.length}
    <div class="reactions">
      {#each item.reactions as r (r.emoji)}
        <button class="chip" class:mine={r.mine} title={r.nicks.join(", ")} onclick={() => react(r.emoji)}>
          {r.emoji}{#if r.nicks.length > 1}<span>{r.nicks.length}</span>{/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .msg {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    padding: 0 0.6rem;
    margin-top: 2px;
  }
  .msg.head {
    margin-top: 0.55rem;
  }
  .msg.mine {
    align-items: flex-end;
  }
  .nick {
    font-size: 0.75rem;
    font-weight: 700;
    margin: 0 0 0.1rem 0.5rem;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    max-width: 100%;
  }
  .mine .line {
    flex-direction: row-reverse;
  }
  .bubble {
    position: relative;
    max-width: 260px;
    padding: 0.4em 0.65em 0.35em;
    border-radius: 14px;
    background: var(--color-surface-alt);
    color: var(--color-text);
    font-size: 0.9rem;
    line-height: 1.35;
    user-select: text;
    overflow-wrap: anywhere;
  }
  .bubble.poll {
    width: 260px;
  }
  .mine .bubble {
    background: color-mix(in srgb, var(--color-primary) 80%, var(--color-surface));
    color: var(--color-primary-text);
  }
  .head:not(.mine) .bubble {
    border-top-left-radius: 4px;
  }
  .head.mine .bubble {
    border-top-right-radius: 4px;
  }
  .bubble.big {
    background: transparent;
    font-size: 2rem;
    line-height: 1.15;
    padding: 0;
  }
  .bubble.big .time {
    display: block;
    color: var(--color-text-muted);
  }
  .bubble.deleted {
    opacity: 0.6;
  }
  .text {
    white-space: pre-wrap;
  }
  .time {
    float: right;
    font-size: 0.66rem;
    opacity: 0.6;
    margin: 0.45em 0 0 0.6em;
    line-height: 1;
  }
  .quote {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    width: 100%;
    text-align: left;
    border: none;
    border-left: 3px solid currentColor;
    border-radius: 6px;
    background: color-mix(in srgb, currentColor 10%, transparent);
    padding: 0.2em 0.5em;
    margin-bottom: 0.3em;
    font-size: 0.8rem;
    color: inherit;
    opacity: 0.85;
  }
  .quote span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 220px;
  }
  a {
    color: inherit;
    text-decoration: underline;
  }
  .mention {
    font-weight: 700;
  }
  .mention.me {
    background: color-mix(in srgb, var(--color-warning) 35%, transparent);
    border-radius: 4px;
    padding: 0 0.15em;
  }
  .actions {
    display: flex;
    gap: 0.1rem;
    opacity: 0;
    transition: opacity 0.1s;
  }
  .msg:hover .actions,
  .actions:focus-within {
    opacity: 1;
  }
  .actions button,
  .quick button {
    background: transparent;
    border: none;
    padding: 0.15em 0.3em;
    font-size: 0.95rem;
    border-radius: 6px;
    color: var(--color-text-muted);
  }
  .actions button:hover,
  .quick button:hover {
    background: var(--color-surface-alt);
    color: var(--color-text);
  }
  .quick {
    display: flex;
    gap: 0.1rem;
    margin-top: 0.2rem;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 999px;
    padding: 0.1rem 0.3rem;
    box-shadow: var(--shadow);
    z-index: 5;
  }
  .quick button {
    font-size: 1.15rem;
  }
  .quick button.chosen {
    background: color-mix(in srgb, var(--color-primary) 30%, transparent);
  }
  .picker-anchor {
    position: relative;
    margin-top: 0.2rem;
  }
  .reactions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.2rem;
    margin-top: 0.2rem;
    max-width: 260px;
  }
  .mine .reactions {
    justify-content: flex-end;
  }
  .chip {
    display: inline-flex;
    gap: 0.25em;
    align-items: center;
    padding: 0.05em 0.45em;
    border-radius: 999px;
    font-size: 0.85rem;
    background: var(--color-surface-alt);
  }
  .chip span {
    font-size: 0.72rem;
    font-weight: 700;
  }
  .chip.mine {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 22%, var(--color-surface-alt));
  }
  :global(.msg.flash) .bubble {
    animation: flash 1.2s ease;
  }
  @keyframes flash {
    0%,
    40% {
      box-shadow: 0 0 0 3px var(--color-accent);
    }
    100% {
      box-shadow: 0 0 0 0 transparent;
    }
  }
</style>
