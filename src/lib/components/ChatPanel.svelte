<script lang="ts">
  import { api } from "$lib/api";
  import { isTopic, nickColor, rows } from "$lib/chat";
  import { currentLanguage, t, userText } from "$lib/i18n";
  import { app } from "$lib/stores/app.svelte";
  import { chat } from "$lib/stores/chat.svelte";
  import { tick } from "svelte";
  import ChatMessage from "./ChatMessage.svelte";
  import EmojiPicker from "./EmojiPicker.svelte";
  import GamePicker from "./GamePicker.svelte";
  import PollDialog from "./PollDialog.svelte";

  let text = $state("");
  let sending = $state(false);
  /** The name of a topic being opened, while the "+" field is shown. */
  let newTopic = $state<string | null>(null);
  let showEmoji = $state(false);
  let showPoll = $state(false);
  let showGame = $state(false);
  /** The "+" menu next to the input: emoji, poll, game. */
  let showAdd = $state(false);
  let list = $state<HTMLDivElement | null>(null);
  let input = $state<HTMLTextAreaElement | null>(null);

  const items = $derived(chat.list(chat.active));
  const dayFormat = $derived(new Intl.DateTimeFormat(currentLanguage(), { weekday: "long", day: "numeric", month: "long" }));
  const today = $derived(dayFormat.format(new Date()));
  const shown = $derived(rows(items, (ts) => dayFormat.format(new Date(ts))));
  const partner = $derived(chat.active && !isTopic(chat.active) ? chat.peers.find((p) => p.id === chat.active) : null);
  const activeName = $derived(chat.conversationName(chat.active));
  const unreadElsewhere = $derived(chat.conversations.filter((c) => c.id !== chat.active).reduce((n, c) => n + c.unread, 0));

  const unreadHere = $derived(chat.unread(chat.active));

  // Seconds left of a flood pause, counted down while it lasts.
  let now = $state(Date.now());
  $effect(() => {
    const until = chat.pausedUntil;
    now = Date.now();
    if (until <= now) return;
    const timer = setInterval(() => {
      now = Date.now();
      if (now >= until) clearInterval(timer);
    }, 250);
    return () => clearInterval(timer);
  });
  const pausedFor = $derived(Math.max(0, Math.ceil((chat.pausedUntil - now) / 1000)));

  // A conversation (or the panel) was opened: start at "new messages" when
  // there are any, like the big messengers, otherwise at the end.
  $effect(() => {
    void chat.active;
    void chat.open;
    const from = chat.unreadFrom;
    void tick().then(() => {
      const marker = from ? document.getElementById("chat-unread") : null;
      if (marker) {
        marker.scrollIntoView({ block: "start" });
        onScroll();
      } else {
        scrollToEnd();
      }
    });
  });

  // Stay at the end while new messages come in, unless the user scrolled up
  // to read; then the button below offers the way down.
  $effect(() => {
    void items.length;
    if (chat.atBottom) {
      void tick().then(() => {
        // Still at the end once the new rows are drawn: the marker or the
        // reader may have moved in between.
        if (chat.atBottom) scrollToEnd();
      });
    }
  });

  /** To the end; whether that is where the list now sits comes from the
   *  geometry, as for any scroll. */
  function scrollToEnd() {
    if (list) list.scrollTop = list.scrollHeight;
    onScroll();
  }

  function onScroll() {
    if (!list) return;
    chat.atBottom = list.scrollHeight - list.scrollTop - list.clientHeight < 40;
    if (chat.atBottom) chat.markRead();
  }

  /** The button at the bottom: to the first unread message while it is
   *  still below what is shown, otherwise to the end. */
  function down() {
    const id = chat.firstUnread;
    const el = id ? document.getElementById(`msg-${id}`) : null;
    if (el && list && el.getBoundingClientRect().top > list.getBoundingClientRect().bottom - 8) {
      el.scrollIntoView({ block: "start", behavior: "smooth" });
    } else {
      scrollToEnd();
    }
  }

  function jump(id: string) {
    const el = document.getElementById(`msg-${id}`);
    if (!el) return;
    el.scrollIntoView({ block: "center", behavior: "smooth" });
    el.classList.add("flash");
    setTimeout(() => el.classList.remove("flash"), 1300);
  }

  // Editing one's own message: its text goes into the input, and leaves it
  // again however the editing ends (switching conversations, replying).
  let editedId: string | null = null;
  $effect(() => {
    const item = chat.editing;
    if (!item) {
      if (editedId !== null) {
        editedId = null;
        text = "";
        void tick().then(autosize);
      }
      return;
    }
    editedId = item.id;
    text = item.text ?? "";
    chat.replyTo = null;
    void tick().then(() => {
      autosize();
      input?.focus();
    });
  });

  function stopEditing() {
    chat.editing = null;
    text = "";
    void tick().then(autosize);
  }

  async function send() {
    const body = text.trim();
    if (!body || sending) return;
    sending = true;
    try {
      if (chat.editing) {
        if (body !== chat.editing.text) await api.chat.edit(chat.editing.id, body);
        chat.editing = null;
      } else {
        await api.chat.send(chat.active, body, chat.replyTo?.id ?? null);
        chat.atBottom = true;
      }
      text = "";
      chat.replyTo = null;
      await tick();
      autosize();
    } catch (e) {
      if (!chat.notePause(e)) app.toast("error", userText(e));
    } finally {
      sending = false;
      input?.focus();
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      void send();
    } else if (e.key === "Escape" && chat.editing) {
      stopEditing();
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
    chat.show(id);
  }

  async function createTopic() {
    const name = newTopic?.trim();
    if (!name) {
      newTopic = null;
      return;
    }
    try {
      await chat.createTopic(name);
      newTopic = null;
    } catch (e) {
      if (!chat.notePause(e)) app.toast("error", userText(e));
    }
  }

  function focusOnMount(el: HTMLElement) {
    el.focus();
  }
</script>

<svelte:window onclick={() => (showAdd = false)} onkeydown={(e) => e.key === "Escape" && (showAdd = false)} />

{#if chat.open}
  <aside class="panel">
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
              {#if c.kind === "private"}
                <span class="dot" class:ok={c.online}></span>{c.nick}
              {:else}
                # {c.nick}
              {/if}
              {#if c.muted && chat.active !== c.id}<span class="muted-icon" title={t("chat.muted")}>🔕</span>{/if}
              {#if c.unread > 0}<span class="badge-count" class:quiet={c.muted}>{c.unread}</span>{/if}
            </button>
            <!-- The open conversation's bell sits in its tab. -->
            {#if chat.active === c.id}
              <button
                class="bell"
                class:off={c.muted}
                title={c.muted ? t("chat.unmute_conversation") : t("chat.mute_conversation")}
                onclick={() => chat.toggleMute(c.id)}
              >
                {c.muted ? "🔕" : "🔔"}
              </button>
            {/if}
            {#if c.id !== null}
              <button class="close" title={t("chat.hide")} onclick={() => c.id && chat.hide(c.id)}>✕</button>
            {/if}
          </div>
        {/each}
        {#if newTopic === null}
          <button class="tab-add" title={t("chat.topic.new")} onclick={() => (newTopic = "")}>＋</button>
        {:else}
          <form class="topic-form" onsubmit={(e) => { e.preventDefault(); void createTopic(); }}>
            <input
              use:focusOnMount
              bind:value={newTopic}
              maxlength="40"
              placeholder={t("chat.topic.placeholder")}
              onkeydown={(e) => e.key === "Escape" && (newTopic = null)}
              onblur={() => { if (!newTopic?.trim()) newTopic = null; }}
            />
          </form>
        {/if}
      </nav>

      <div class="messages" bind:this={list} onscroll={onScroll}>
        {#each shown as r (r.item.id)}
          {#if r.day}<div class="day"><span>{r.day === today ? t("chat.today") : r.day}</span></div>{/if}
          {#if r.item.id === chat.unreadFrom}<div class="unread-line" id="chat-unread"><span>{t("chat.unread_here")}</span></div>{/if}
          <ChatMessage item={r.item} head={r.head} onjump={jump} />
        {:else}
          <div class="empty">
            <p class="muted">{chat.active === null ? t("chat.empty.public") : isTopic(chat.active) ? t("chat.empty.topic") : t("chat.empty.private")}</p>
          </div>
        {/each}
      </div>
      {#if !chat.atBottom}
        <button class="to-end" class:has-unread={unreadHere > 0} title={t("chat.to_end")} onclick={down}>
          ↓{#if unreadHere > 0}<span>{t("chat.jump_unread", { count: unreadHere })}</span>{/if}
        </button>
      {/if}
      {#if unreadElsewhere > 0}
        <div class="elsewhere">{t("chat.unread_elsewhere", { count: unreadElsewhere })}</div>
      {/if}

      <footer>
        {#if partner && !partner.online}
          <div class="note">{t("chat.partner_offline", { nick: activeName })}</div>
        {/if}
        {#if pausedFor > 0}
          <div class="note paused">⏳ {t("chat.paused", { seconds: pausedFor })}</div>
        {/if}
        {#if chat.editing}
          <div class="replying">
            <div class="grow">
              <strong>✎ {t("chat.editing")}</strong>
              <span>{chat.editing.text ?? ""}</span>
            </div>
            <button class="icon" title={t("action.cancel")} onclick={stopEditing}>✕</button>
          </div>
        {:else if chat.replyTo}
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
          <div class="add-wrap">
            <button class="icon add" class:open={showAdd} title={t("chat.add")} aria-expanded={showAdd} onclick={(e) => { e.stopPropagation(); showAdd = !showAdd; }}>＋</button>
            {#if showAdd}
              <div class="add-menu" role="menu">
                <button role="menuitem" onclick={() => { showAdd = false; showEmoji = !showEmoji; }}>😊 {t("chat.emoji")}</button>
                <button role="menuitem" onclick={() => { showAdd = false; showPoll = true; }}>📊 {t("chat.poll.create")}</button>
                <button role="menuitem" onclick={() => { showAdd = false; showGame = true; }}>🎮 {t("chat.game.share")}</button>
              </div>
            {/if}
          </div>
          <textarea
            id="chat-input"
            bind:this={input}
            bind:value={text}
            rows="1"
            maxlength="2000"
            placeholder={chat.active === null ? t("chat.placeholder") : isTopic(chat.active) ? t("chat.placeholder.topic", { name: activeName }) : t("chat.placeholder.private", { nick: activeName })}
            oninput={autosize}
            onkeydown={onKey}
          ></textarea>
          <button class="send primary" title={chat.editing ? t("chat.save_edit") : t("chat.send")} disabled={!text.trim() || sending || pausedFor > 0} onclick={send}>{chat.editing ? "✓" : "➤"}</button>
        </div>
      </footer>
    {/if}
  </aside>
  {#if showPoll}
    <PollDialog onclose={() => (showPoll = false)} />
  {/if}
  {#if showGame}
    <GamePicker onclose={() => (showGame = false)} />
  {/if}
{/if}

<!-- Who is online: a pane of its own, as wide as the chat. -->
{#if chat.showPeople && chat.enabled}
  <aside class="pane people">
    <div class="people-head">{t("chat.online", { count: chat.online.length })}</div>
    <div class="people-list">
      <div class="person me">
        <span class="dot ok"></span>
        <span class="who">
          <span>{chat.nick} <small class="muted">({t("chat.you")})</small></span>
        </span>
      </div>
      {#each chat.people as p (p.id)}
        <div class="person" class:off={!p.online}>
          <span class="dot" class:ok={p.online}></span>
          <span class="who">
            <span class="name" style:color={nickColor(p.id)}>{p.nick}</span>
            {#if p.playing}<small class="playing" title={p.playing}>🎮 {p.playing}</small>{/if}
          </span>
          <button class="dm" title={t("chat.write_private", { nick: p.nick })} onclick={() => openPrivate(p.id)}>✉</button>
        </div>
      {:else}
        <p class="hint">{t("chat.nobody")}</p>
      {/each}
    </div>
  </aside>
{/if}

<!-- Always there, at the window's edge: the bubble opens and closes the chat,
     the people icon the list of who is online. -->
<aside class="rail">
  <button class="toggle" class:active={chat.open} title={chat.open ? t("chat.close") : t("chat.open")} onclick={() => chat.toggleOpen()}>
    💬
    {#if chat.totalUnread > 0}<span class="badge-count">{chat.totalUnread > 99 ? "99+" : chat.totalUnread}</span>{/if}
  </button>
  {#if chat.enabled}
    <button class="toggle people-toggle" class:active={chat.showPeople} title={t("chat.people")} onclick={() => chat.togglePeople()}>
      👥<small>{chat.online.length}</small>
    </button>
    {#if chat.relayOnline}<span class="relay" title={t("chat.relay")}>🗄</span>{/if}
    <span class="grow"></span>
    <!-- All conversations at once; each conversation has its own bell too. -->
    <button class="toggle sound" title={app.settings?.chatSound ? t("chat.sound.off") : t("chat.sound.on")} onclick={() => chat.setSound(!app.settings?.chatSound).catch((e) => app.toast("error", userText(e)))}>
      {app.settings?.chatSound ? "🔔" : "🔕"}
    </button>
  {/if}
</aside>

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
    padding: 0.7rem 0;
  }
  .rail .sound {
    font-size: 1rem;
  }
  .rail .toggle {
    position: relative;
    font-size: 1.3rem;
    padding: 0.35em 0.45em;
    background: transparent;
    border-color: transparent;
  }
  .rail .toggle.active {
    background: var(--color-surface-alt);
    border-color: var(--color-border);
  }
  .people-toggle {
    display: flex;
    flex-direction: column;
    align-items: center;
    line-height: 1.1;
  }
  .people-toggle small {
    font-size: 0.7rem;
    color: var(--color-text-muted);
  }
  .tab-add {
    flex-shrink: 0;
    border-radius: 999px;
    padding: 0.1em 0.6em;
    font-size: 0.85rem;
    background: transparent;
  }
  .topic-form input {
    width: 150px;
    padding: 0.2em 0.6em;
    font-size: 0.82rem;
    border-radius: 999px;
  }
  .rail .badge-count {
    position: absolute;
    top: -6px;
    right: -4px;
  }
  .panel,
  .pane {
    position: relative;
    width: 360px;
    display: flex;
    flex-direction: column;
  }
  /* A small window keeps the library usable next to the chat. */
  @media (max-width: 1100px) {
    .panel,
    .pane {
      width: 300px;
    }
  }
  .rail .relay {
    font-size: 0.9rem;
    opacity: 0.75;
    cursor: help;
  }
  .bell {
    background: transparent;
    border: none;
    padding: 0 0.35em 0 0;
    font-size: 0.72rem;
    opacity: 0.85;
  }
  .bell.off {
    opacity: 0.6;
  }
  .note {
    font-size: 0.75rem;
    color: var(--color-text-muted);
    margin-bottom: 0.4rem;
  }
  .note.paused {
    color: var(--color-warning);
    font-weight: 600;
  }
  .people-head {
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
    padding: 0.75rem 0.9rem 0.5rem;
    border-bottom: 1px solid var(--color-border);
  }
  .people-list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0.4rem;
  }
  .person {
    display: flex;
    align-items: center;
    gap: 0.55rem;
    padding: 0.35em 0.3em 0.35em 0.5em;
    border-radius: 8px;
  }
  .person:hover {
    background: var(--color-surface-alt);
  }
  .person.off {
    opacity: 0.55;
  }
  .who {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .who .name,
  .playing {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .playing {
    font-size: 0.72rem;
    color: var(--color-text-muted);
  }
  .person .dm {
    background: transparent;
    border-color: transparent;
    padding: 0.15em 0.5em;
    font-size: 0.95rem;
  }
  .person .dm:hover {
    border-color: var(--color-border);
  }
  .hint {
    font-size: 0.8rem;
    color: var(--color-text-muted);
    padding: 0.5rem;
  }
  .icon {
    background: transparent;
    border-color: transparent;
    padding: 0.25em 0.5em;
    font-size: 0.95rem;
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
  /* Wrapping rather than scrolling, so "+" and every tab stay in sight. */
  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem;
    padding: 0.4rem 0.6rem;
    max-height: 7.5rem;
    overflow-y: auto;
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
  .muted-icon {
    font-size: 0.7rem;
    opacity: 0.7;
  }
  .badge-count.quiet {
    background: var(--color-surface-alt);
    color: var(--color-text-muted);
  }
  .unread-line {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin: 0.7rem 0.6rem 0.2rem;
    color: var(--color-primary);
    font-size: 0.72rem;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .unread-line::before,
  .unread-line::after {
    content: "";
    flex: 1;
    border-top: 1px solid currentColor;
    opacity: 0.6;
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
    min-width: 2.2rem;
    height: 2.2rem;
    padding: 0 0.7rem;
    box-shadow: var(--shadow);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 0.35rem;
  }
  .to-end.has-unread {
    background: var(--color-primary);
    color: var(--color-primary-text);
    border-color: transparent;
    font-weight: 600;
    font-size: 0.8rem;
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
  .add-wrap {
    position: relative;
  }
  .icon.add {
    font-size: 1.1rem;
    font-weight: 700;
    border-radius: 999px;
    width: 2.3rem;
    height: 2.3rem;
    padding: 0;
    transition: transform 0.15s;
  }
  .icon.add.open {
    transform: rotate(45deg);
  }
  .add-menu {
    position: absolute;
    bottom: calc(100% + 6px);
    left: 0;
    z-index: 30;
    display: flex;
    flex-direction: column;
    min-width: 190px;
    padding: 0.3rem;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }
  .add-menu button {
    background: transparent;
    border: none;
    text-align: left;
    padding: 0.45em 0.7em;
    border-radius: 6px;
    white-space: nowrap;
  }
  .add-menu button:hover {
    background: var(--color-surface-alt);
  }
</style>
