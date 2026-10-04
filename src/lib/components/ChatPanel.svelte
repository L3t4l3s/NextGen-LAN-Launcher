<script lang="ts">
  import { api } from "$lib/api";
  import { nickColor, rows } from "$lib/chat";
  import { currentLanguage, t, userText } from "$lib/i18n";
  import { app } from "$lib/stores/app.svelte";
  import { chat } from "$lib/stores/chat.svelte";
  import { tick } from "svelte";
  import ChatMessage from "./ChatMessage.svelte";
  import EmojiPicker from "./EmojiPicker.svelte";
  import PollDialog from "./PollDialog.svelte";

  let text = $state("");
  let sending = $state(false);
  let showPeople = $state(false);
  let showEmoji = $state(false);
  let showPoll = $state(false);
  let list = $state<HTMLDivElement | null>(null);
  let input = $state<HTMLTextAreaElement | null>(null);
  /** The list sits at its end, so new messages scroll into view. */
  let atBottom = $state(true);

  const osIcon: Record<string, string> = { windows: "🪟", linux: "🐧", macos: "🍎" };

  const items = $derived(chat.list(chat.active));
  const dayFormat = $derived(new Intl.DateTimeFormat(currentLanguage(), { weekday: "long", day: "numeric", month: "long" }));
  const today = $derived(dayFormat.format(new Date()));
  const shown = $derived(rows(items, (ts) => dayFormat.format(new Date(ts))));
  const partner = $derived(chat.active ? chat.peers.find((p) => p.id === chat.active) : null);
  const unreadElsewhere = $derived(chat.conversations.filter((c) => c.id !== chat.active).reduce((n, c) => n + c.unread, 0));

  // Stay at the bottom while new messages come in, unless the user scrolled
  // up to read; then the button below offers the way back.
  $effect(() => {
    void items.length;
    void chat.active;
    if (atBottom) void tick().then(() => scrollToEnd());
  });

  function scrollToEnd() {
    if (list) list.scrollTop = list.scrollHeight;
    chat.markRead();
  }

  function onScroll() {
    if (!list) return;
    atBottom = list.scrollHeight - list.scrollTop - list.clientHeight < 40;
    if (atBottom) chat.markRead();
  }

  function jump(id: string) {
    const el = document.getElementById(`msg-${id}`);
    if (!el) return;
    el.scrollIntoView({ block: "center", behavior: "smooth" });
    el.classList.add("flash");
    setTimeout(() => el.classList.remove("flash"), 1300);
  }

  async function send() {
    const body = text.trim();
    if (!body || sending) return;
    sending = true;
    try {
      await api.chat.send(chat.active, body, chat.replyTo?.id ?? null);
      text = "";
      chat.replyTo = null;
      atBottom = true;
      await tick();
      autosize();
    } catch (e) {
      app.toast("error", userText(e));
    } finally {
      sending = false;
      input?.focus();
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      void send();
    } else if (e.key === "Escape" && chat.replyTo) {
      chat.replyTo = null;
    }
  }

  function autosize() {
    if (!input) return;
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, 140)}px`;
  }

  function insertEmoji(emoji: string) {
    const el = input;
    if (!el) {
      text += emoji;
      return;
    }
    const start = el.selectionStart ?? text.length;
    const end = el.selectionEnd ?? text.length;
    text = text.slice(0, start) + emoji + text.slice(end);
    void tick().then(() => {
      el.focus();
      el.setSelectionRange(start + emoji.length, start + emoji.length);
    });
  }

  function openPrivate(id: string) {
    showPeople = false;
    chat.show(id);
  }
</script>

{#if !chat.open}
  <aside class="rail">
    <button class="toggle" title={t("chat.open")} onclick={() => chat.setOpen(true)}>
      💬
      {#if chat.totalUnread > 0}<span class="badge-count">{chat.totalUnread > 99 ? "99+" : chat.totalUnread}</span>{/if}
    </button>
    {#if chat.enabled}<span class="online-count" title={t("chat.online", { count: chat.online.length })}><span>👥</span>{chat.online.length}</span>{/if}
  </aside>
{:else}
  <aside class="panel">
    <header>
      <strong class="grow">💬 {t("chat.title")}</strong>
      {#if chat.enabled}
        <button class="icon" class:active={showPeople} title={t("chat.people")} onclick={() => (showPeople = !showPeople)}>👥 {chat.online.length}</button>
        <button class="icon" title={app.settings?.chatSound ? t("chat.sound.off") : t("chat.sound.on")} onclick={() => chat.setSound(!app.settings?.chatSound).catch((e) => app.toast("error", userText(e)))}>
          {app.settings?.chatSound ? "🔔" : "🔕"}
        </button>
      {/if}
      <button class="icon" title={t("chat.close")} onclick={() => chat.setOpen(false)}>›</button>
    </header>

    {#if !chat.loaded}
      <div class="empty"><span class="muted">…</span></div>
    {:else if !chat.enabled}
      <div class="empty">
        <p class="muted">{chat.problem ? `⚠ ${chat.problem}` : t("chat.disabled")}</p>
        <button onclick={() => (app.view = "settings")}>{t("chat.disabled.settings")}</button>
      </div>
    {:else}
      {#if chat.problem}<div class="problem">⚠ {userText(chat.problem)}</div>{/if}

      <nav class="tabs">
        {#each chat.conversations as c (c.id ?? "")}
          <div class="tab-wrap" class:active={chat.active === c.id}>
            <button class="tab" onclick={() => chat.show(c.id)}>
              {#if c.id === null}
                # {t("chat.public")}
              {:else}
                <span class="dot" class:ok={c.online}></span>{c.nick}
              {/if}
              {#if c.unread > 0}<span class="badge-count">{c.unread}</span>{/if}
            </button>
            {#if c.id !== null}
              <button class="close" title={t("chat.hide")} onclick={() => c.id && chat.hide(c.id)}>✕</button>
            {/if}
          </div>
        {/each}
      </nav>

      {#if showPeople}
        <div class="people">
          <div class="people-head">{t("chat.online", { count: chat.online.length })}</div>
          <div class="person me">
            <span class="dot ok"></span>
            <span class="grow">{chat.nick} <small class="muted">({t("chat.you")})</small></span>
          </div>
          {#each chat.peers as p (p.id)}
            <button class="person" class:off={!p.online} title={`${p.address} · ${p.os}`} onclick={() => openPrivate(p.id)}>
              <span class="dot" class:ok={p.online}></span>
              <span class="grow" style:color={nickColor(p.id)}>{p.nick}</span>
              <span class="os">{osIcon[p.os] ?? "💻"}</span>
              <span class="dm">✉</span>
            </button>
          {:else}
            <p class="hint">{t("chat.nobody")}</p>
          {/each}
        </div>
      {/if}

      {#if chat.active}
        <div class="partner">
          {t("chat.private_with", { nick: chat.nickOf(chat.active) })}
          {#if !partner?.online}<span class="muted"> · {t("chat.offline_hint", { nick: chat.nickOf(chat.active) })}</span>{/if}
        </div>
      {/if}

      <div class="messages" bind:this={list} onscroll={onScroll}>
        {#each shown as r (r.item.id)}
          {#if r.day}<div class="day"><span>{r.day === today ? t("chat.today") : r.day}</span></div>{/if}
          <ChatMessage item={r.item} head={r.head} onjump={jump} />
        {:else}
          <div class="empty">
            <p class="muted">{chat.active ? t("chat.empty.private") : t("chat.empty.public")}</p>
          </div>
        {/each}
      </div>
      {#if !atBottom}
        <button class="to-end" onclick={() => { atBottom = true; scrollToEnd(); }}>↓</button>
      {/if}
      {#if unreadElsewhere > 0 && !showPeople}
        <div class="elsewhere">{t("chat.unread_elsewhere", { count: unreadElsewhere })}</div>
      {/if}

      <footer>
        {#if chat.replyTo}
          <div class="replying">
            <div class="grow">
              <strong>{t("chat.replying_to", { nick: chat.replyTo.nick })}</strong>
              <span>{chat.replyTo.text ?? chat.replyTo.poll?.question ?? ""}</span>
            </div>
            <button class="icon" title={t("action.cancel")} onclick={() => (chat.replyTo = null)}>✕</button>
          </div>
        {/if}
        {#if showEmoji}
          <div class="emoji-anchor">
            <EmojiPicker onpick={insertEmoji} onclose={() => (showEmoji = false)} />
          </div>
        {/if}
        <div class="compose">
          <button class="icon" title={t("chat.emoji")} onclick={() => (showEmoji = !showEmoji)}>😊</button>
          <button class="icon" title={t("chat.poll.create")} onclick={() => (showPoll = true)}>📊</button>
          <textarea
            id="chat-input"
            bind:this={input}
            bind:value={text}
            rows="1"
            maxlength="2000"
            placeholder={chat.active ? t("chat.placeholder.private", { nick: chat.nickOf(chat.active) }) : t("chat.placeholder")}
            oninput={autosize}
            onkeydown={onKey}
          ></textarea>
          <button class="send primary" title={t("chat.send")} disabled={!text.trim() || sending} onclick={send}>➤</button>
        </div>
      </footer>
    {/if}
  </aside>
  {#if showPoll}
    <PollDialog onclose={() => (showPoll = false)} />
  {/if}
{/if}

<style>
  aside {
    border-left: 1px solid var(--color-border);
    background: var(--color-surface);
    min-height: 0;
  }
  .rail {
    width: 52px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.6rem;
    padding-top: 0.7rem;
  }
  .rail .toggle {
    position: relative;
    font-size: 1.3rem;
    padding: 0.35em 0.45em;
    background: transparent;
  }
  .rail .badge-count {
    position: absolute;
    top: -6px;
    right: -4px;
  }
  .online-count {
    display: flex;
    flex-direction: column;
    align-items: center;
    font-size: 0.75rem;
    color: var(--color-text-muted);
  }
  .panel {
    position: relative;
    width: 360px;
    display: flex;
    flex-direction: column;
  }
  /* A small window keeps the library usable next to the chat. */
  @media (max-width: 1100px) {
    .panel {
      width: 300px;
    }
  }
  header {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.5rem 0.6rem 0.4rem 0.9rem;
    border-bottom: 1px solid var(--color-border);
  }
  .icon {
    background: transparent;
    border-color: transparent;
    padding: 0.25em 0.5em;
    font-size: 0.95rem;
  }
  .icon.active {
    background: var(--color-surface-alt);
  }
  .badge-count {
    background: var(--color-primary);
    color: var(--color-primary-text);
    border-radius: 999px;
    font-size: 0.68rem;
    font-weight: 700;
    padding: 0 0.45em;
    line-height: 1.5;
  }
  .problem {
    font-size: 0.8rem;
    color: var(--color-warning);
    padding: 0.4rem 0.9rem;
    border-bottom: 1px solid var(--color-border);
  }
  .tabs {
    display: flex;
    gap: 0.25rem;
    padding: 0.4rem 0.6rem;
    overflow-x: auto;
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }
  .tab-wrap {
    display: flex;
    align-items: center;
    border-radius: 999px;
    border: 1px solid var(--color-border);
    flex-shrink: 0;
  }
  .tab-wrap.active {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 16%, transparent);
  }
  .tab {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    background: transparent;
    border: none;
    padding: 0.2em 0.7em;
    font-size: 0.82rem;
    white-space: nowrap;
  }
  .close {
    background: transparent;
    border: none;
    padding: 0 0.5em 0 0;
    font-size: 0.7rem;
    opacity: 0.55;
  }
  .close:hover {
    opacity: 1;
  }
  .dot {
    width: 8px;
    height: 8px;
  }
  .dot.ok {
    box-shadow: none;
  }
  .people {
    position: absolute;
    top: 88px;
    left: 0.6rem;
    right: 0.6rem;
    z-index: 20;
    max-height: 60%;
    overflow-y: auto;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    padding: 0.4rem;
  }
  .people-head {
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
    padding: 0.2rem 0.5rem 0.4rem;
  }
  .person {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    text-align: left;
    background: transparent;
    border: none;
    padding: 0.35em 0.5em;
    border-radius: 8px;
  }
  button.person:hover {
    background: var(--color-surface-alt);
  }
  .person.off {
    opacity: 0.55;
  }
  .person .dm {
    opacity: 0;
  }
  .person:hover .dm {
    opacity: 0.8;
  }
  .partner {
    font-size: 0.78rem;
    padding: 0.35rem 0.9rem;
    border-bottom: 1px solid var(--color-border);
  }
  .messages {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0.3rem 0 0.8rem;
  }
  .day {
    text-align: center;
    margin: 0.8rem 0 0.2rem;
  }
  .day span {
    font-size: 0.72rem;
    color: var(--color-text-muted);
    background: var(--color-surface-alt);
    border-radius: 999px;
    padding: 0.15em 0.7em;
  }
  .empty {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.6rem;
    text-align: center;
    padding: 1.5rem;
  }
  .to-end {
    position: absolute;
    right: 1rem;
    bottom: 5.2rem;
    border-radius: 999px;
    width: 2.2rem;
    height: 2.2rem;
    padding: 0;
    box-shadow: var(--shadow);
  }
  .elsewhere {
    font-size: 0.75rem;
    text-align: center;
    color: var(--color-primary);
    padding: 0.2rem;
  }
  footer {
    position: relative;
    border-top: 1px solid var(--color-border);
    padding: 0.5rem 0.6rem;
  }
  .replying {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    border-left: 3px solid var(--color-primary);
    padding: 0.1rem 0 0.1rem 0.5rem;
    margin-bottom: 0.4rem;
    font-size: 0.8rem;
    min-width: 0;
  }
  .replying div {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .replying span {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: var(--color-text-muted);
  }
  .emoji-anchor {
    position: absolute;
    bottom: calc(100% + 4px);
    left: 0.6rem;
  }
  .compose {
    display: flex;
    align-items: flex-end;
    gap: 0.2rem;
  }
  textarea {
    flex: 1;
    min-width: 0;
    resize: none;
    max-height: 140px;
    padding: 0.45em 0.7em;
    border-radius: 18px;
    line-height: 1.35;
  }
  .send {
    border-radius: 999px;
    width: 2.3rem;
    height: 2.3rem;
    padding: 0;
    flex-shrink: 0;
  }
</style>
