// State of the chat panel (Svelte 5 runes). The backend owns the messages;
// this keeps the copy the panel shows, what was read, and plays the sounds.

import { api, listen } from "$lib/api";
import { byTime, convKey, isTopic, isUnread, mentions, rings } from "$lib/chat";
import { playSound, unlockAudio } from "$lib/chat-sound";
import { t, userText } from "$lib/i18n";
import { app } from "$lib/stores/app.svelte";
import type { ChatItem, ChatPeer, ChatSnapshot, ChatUpdate, LanPagePlayer } from "$lib/types";

const OPEN_KEY = "nll.chat.open";
const READ_KEY = "nll.chat.read";
const SINCE_KEY = "nll.chat.since";
const MUTED_KEY = "nll.chat.muted";

function load<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw === null ? fallback : (JSON.parse(raw) as T);
  } catch {
    return fallback;
  }
}

function store(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Private mode or blocked storage: the panel just forgets.
  }
}

export interface Conversation {
  /** null: the public room; "#<id>" a topic; otherwise a peer id. */
  id: string | null;
  kind: "public" | "topic" | "private";
  /** Shown on the tab: a topic's name, a person's nickname. */
  nick: string;
  online: boolean;
  unread: number;
  muted: boolean;
}

class ChatStore {
  /** The backend runs a chat (the setting is on and it started). */
  enabled = $state(false);
  loaded = $state(false);
  me = $state("");
  nick = $state("");
  problem = $state<string | null>(null);
  items = $state<Record<string, ChatItem>>({});
  peers = $state<ChatPeer[]>([]);
  open = $state(load(OPEN_KEY, true));
  /** Conversation shown: null is the public room. */
  active = $state<string | null>(null);
  /** Arrival time of the newest message read, per conversation ("" = public). */
  readUpTo = $state<Record<string, number>>(load(READ_KEY, {}));
  /** First start of the chat here: history from before it is not "unread". */
  private since = load<number | null>(SINCE_KEY, null) ?? Date.now();
  replyTo = $state<ChatItem | null>(null);
  /** One's own message being edited in the input. */
  editing = $state<ChatItem | null>(null);
  /** The list of people, opened from the bar at the right. */
  showPeople = $state(false);
  /** Players the LANPage shows as online who are not in the chat. */
  lanPlayers = $state<LanPagePlayer[]>([]);
  /** Writing is paused until then (ms) after too many messages; 0: not. */
  pausedUntil = $state(0);
  /** Private conversations opened in this session, even before a word was said. */
  private opened = $state<string[]>([]);
  /** Conversations closed by hand; a new message brings them back. */
  private hidden = $state<string[]>([]);
  /** Conversations that make no sound ("" = public). The bell in the
   *  header (the `chatSound` setting) still silences all of them. */
  muted = $state<string[]>(load(MUTED_KEY, []));
  /** The message list is scrolled to its end; only then is what arrives read. */
  atBottom = $state(true);
  /** First unread message when the conversation was opened: the panel draws
   *  "new messages" above it and starts there instead of at the end. */
  unreadFrom = $state<string | null>(null);
  private placed = false;

  /** The bell: one setting, saved on its own. */
  async setSound(on: boolean) {
    await api.chat.setSound(on);
    if (app.settings) app.settings = { ...app.settings, chatSound: on };
  }

  async init() {
    unlockAudio();
    await listen("chat-update", (payload) => this.apply(payload as ChatUpdate));
    await listen("chat-reset", () => void this.reload());
    await this.reload();
    // The LANPage hears from every launcher every few minutes; once a minute
    // is plenty to follow it.
    void this.loadLanPlayers();
    setInterval(() => void this.loadLanPlayers(), 60_000);
    // Whatever is on screen counts as read once the window has focus again.
    window.addEventListener("focus", () => this.markRead());
  }

  async reload() {
    let snap: ChatSnapshot | null = null;
    let failure: string | null = null;
    try {
      snap = await api.chat.snapshot();
    } catch (e) {
      failure = userText(e);
    }
    this.enabled = !!snap;
    this.loaded = true;
    if (!snap) {
      this.items = {};
      this.peers = [];
      this.problem = failure;
      return;
    }
    this.me = snap.me;
    this.nick = snap.nick;
    this.problem = snap.problem;
    this.peers = snap.peers;
    const next: Record<string, ChatItem> = {};
    for (const i of snap.items) next[i.id] = i;
    this.items = next;
    this.leaveDeletedTopic();
    // A first start has read nothing and missed nothing: the history that
    // arrives is the room's past, not a pile of unread messages.
    store(SINCE_KEY, this.since);
    // Only the first load places the marker; a later one (the chat restarted,
    // old messages expired) must not pull the reader away from where they are.
    if (!this.placed) {
      this.placed = true;
      this.captureUnread();
    }
    this.markRead();
  }

  private apply(update: ChatUpdate) {
    if (update.kind === "peers") {
      this.peers = update.peers;
      return;
    }
    if (update.kind === "reset") {
      void this.reload();
      return;
    }
    const next = { ...this.items };
    let sound: "message" | "direct" | null = null;
    for (const item of update.items) {
      const fresh = !next[item.id];
      next[item.id] = item;
      if (!fresh || item.mine || item.deleted) continue;
      // Only what is new for everyone rings, not history caught up on.
      if (!rings(item, Date.now())) continue;
      // Rings also when the conversation is open: at a LAN the window is
      // often in view while the player looks elsewhere.
      if (this.isMuted(item.conversation)) continue;
      const direct = (item.conversation !== null && !isTopic(item.conversation)) || mentions(item.text, this.nick);
      sound = direct ? "direct" : (sound ?? "message");
    }
    this.items = next;
    this.leaveDeletedTopic();
    this.markRead();
    if (sound && app.settings?.chatSound) playSound(sound);
  }

  /** A topic deleted while open leaves nothing to show: back to the room. */
  private leaveDeletedTopic() {
    const active = this.active;
    if (active !== null && isTopic(active) && this.items[active.slice(1)]?.deleted) {
      this.opened = this.opened.filter((c) => c !== active);
      this.show(null);
    }
  }

  /** The conversation is visible right now, so its new messages are read. */
  private isOnScreen(conversation: string | null): boolean {
    return this.open && this.active === conversation && typeof document !== "undefined" && document.hasFocus();
  }

  isMuted(conversation: string | null): boolean {
    return this.muted.includes(convKey(conversation));
  }

  toggleMute(conversation: string | null) {
    const key = convKey(conversation);
    this.muted = this.muted.includes(key) ? this.muted.filter((k) => k !== key) : [...this.muted, key];
    store(MUTED_KEY, this.muted);
  }

  /** Remember where the unread part of the shown conversation begins. Runs
   *  when a conversation is opened, before anything is marked read. */
  private captureUnread() {
    const read = this.readUpTo[convKey(this.active)] ?? 0;
    this.unreadFrom = this.list(this.active).find((i) => isUnread(i, read, this.since))?.id ?? null;
    // The panel starts at the marker; until it reports being at the end,
    // nothing below the marker counts as read.
    if (this.unreadFrom) this.atBottom = false;
  }

  /** The first unread message of the shown conversation now, for the jump button. */
  get firstUnread(): string | null {
    const read = this.readUpTo[convKey(this.active)] ?? 0;
    return this.list(this.active).find((i) => isUnread(i, read, this.since))?.id ?? null;
  }

  markRead() {
    if (!this.isOnScreen(this.active) || !this.atBottom) return;
    const key = convKey(this.active);
    const newest = Math.max(0, ...this.list(this.active).map((i) => i.received));
    if (newest > (this.readUpTo[key] ?? 0)) {
      this.readUpTo = { ...this.readUpTo, [key]: newest };
      store(READ_KEY, this.readUpTo);
    }
  }

  setOpen(open: boolean) {
    const opening = open && !this.open;
    this.open = open;
    store(OPEN_KEY, open);
    if (opening) this.captureUnread();
    this.markRead();
  }

  /** The speech bubble in the bar: open or close the chat. */
  toggleOpen() {
    this.setOpen(!this.open);
  }

  /** The people icon in the bar: show or hide the list, opening the chat
   *  for it when needed. */
  /** The list of people is a pane of its own, next to the chat or alone. */
  togglePeople() {
    this.showPeople = !this.showPeople;
  }

  /** Take note of a flood pause (`err.chat_too_fast|<seconds>`); true when
   *  the error was one, so the caller need not show it as well. */
  notePause(e: unknown): boolean {
    const m = /^err\.chat_too_fast\|(\d+)/.exec(e instanceof Error ? e.message : String(e));
    if (!m) return false;
    this.pausedUntil = Date.now() + Number(m[1]) * 1000;
    return true;
  }

  async createTopic(name: string) {
    const item = await api.chat.createTopic(name);
    this.items = { ...this.items, [item.id]: item };
    this.show(item.conversation);
  }

  /** What a conversation is called in tabs, headers and placeholders. */
  conversationName(conversation: string | null): string {
    if (conversation === null) return t("chat.public");
    if (isTopic(conversation)) return this.items[conversation.slice(1)]?.topicName ?? "…";
    return this.nickOf(conversation);
  }

  show(conversation: string | null) {
    if (conversation && !this.opened.includes(conversation)) this.opened = [...this.opened, conversation];
    if (conversation) this.hidden = this.hidden.filter((c) => c !== conversation);
    const switching = this.active !== conversation || !this.open;
    if (this.active !== conversation) {
      this.replyTo = null;
      this.editing = null;
    }
    this.active = conversation;
    if (switching) {
      this.atBottom = true;
      this.captureUnread();
    }
    this.open = true;
    store(OPEN_KEY, true);
    this.markRead();
  }

  /** Close a private conversation tab (its messages stay). */
  hide(conversation: string) {
    this.opened = this.opened.filter((c) => c !== conversation);
    const key = convKey(conversation);
    const newest = Math.max(0, ...this.list(conversation).map((i) => i.received));
    this.readUpTo = { ...this.readUpTo, [key]: Math.max(newest, this.readUpTo[key] ?? 0) };
    store(READ_KEY, this.readUpTo);
    this.hidden = [...this.hidden.filter((c) => c !== conversation), conversation];
    if (this.active === conversation) this.show(null);
  }

  list(conversation: string | null): ChatItem[] {
    return Object.values(this.items)
      .filter((i) => i.conversation === conversation)
      .sort(byTime);
  }

  unread(conversation: string | null): number {
    const read = this.readUpTo[convKey(conversation)] ?? 0;
    return Object.values(this.items).filter((i) => i.conversation === conversation && isUnread(i, read, this.since)).length;
  }

  get totalUnread(): number {
    return this.conversations.reduce((n, c) => n + c.unread, 0);
  }

  /** People, without relays. */
  get people(): ChatPeer[] {
    return this.peers.filter((p) => !p.relay);
  }

  get online(): ChatPeer[] {
    return this.people.filter((p) => p.online);
  }

  /** The LANPage's players without anyone the chat knows by now: the list
   *  is fetched once a minute, the chat changes in between. */
  get lanOthers(): LanPagePlayer[] {
    const known = new Set(this.peers.map((p) => p.address));
    return this.lanPlayers.filter((p) => !known.has(p.address));
  }

  /** Who was in the chat and has gone. */
  get gone(): ChatPeer[] {
    return this.people.filter((p) => !p.online);
  }

  /** Everyone online: in the chat, and by the LANPage without it. */
  get onlineCount(): number {
    return this.online.length + this.lanOthers.length;
  }

  private async loadLanPlayers() {
    if (!this.enabled) {
      this.lanPlayers = [];
      return;
    }
    try {
      this.lanPlayers = await api.chat.lanpagePlayers();
    } catch {
      // Keep the last list: one failed request is no reason to empty it.
    }
  }

  /** A relay keeps what is said for those who start later. */
  get relayOnline(): boolean {
    return this.peers.some((p) => p.relay && p.online);
  }

  nickOf(peer: string): string {
    return (
      this.peers.find((p) => p.id === peer)?.nick ??
      Object.values(this.items).find((i) => i.from === peer)?.nick ??
      peer.slice(0, 8)
    );
  }

  /** Every nickname known, for @mentions. */
  get nicks(): string[] {
    return [...new Set([this.nick, ...this.people.map((p) => p.nick)])];
  }

  /** The public room, then topics, then private conversations: opened
   *  ones, and those with messages unless closed and nothing new arrived
   *  since. A deleted topic goes. */
  get conversations(): Conversation[] {
    const ids = new Set<string>(this.opened);
    const deletedTopics = new Set(
      Object.values(this.items)
        .filter((i) => i.deleted && i.conversation === `#${i.id}`)
        .map((i) => i.conversation),
    );
    for (const i of Object.values(this.items)) {
      if (i.conversation === null || deletedTopics.has(i.conversation)) continue;
      const hiddenAndRead = this.hidden.includes(i.conversation) && i.received <= (this.readUpTo[i.conversation] ?? 0);
      if (!hiddenAndRead) ids.add(i.conversation);
    }
    const others: Conversation[] = [...ids]
      .filter((id) => !deletedTopics.has(id))
      .map((id) => ({
        id,
        kind: isTopic(id) ? "topic" : "private",
        nick: this.conversationName(id),
        online: isTopic(id) || this.peers.some((p) => p.id === id && p.online),
        unread: this.unread(id),
        muted: this.isMuted(id),
      }));
    // Topics in the order they were opened, people by name.
    const opened = (c: Conversation) => (c.id ? (this.items[c.id.slice(1)]?.ts ?? 0) : 0);
    const topics = others.filter((c) => c.kind === "topic").sort((a, b) => opened(a) - opened(b));
    const privates = others.filter((c) => c.kind === "private").sort((a, b) => a.nick.localeCompare(b.nick));
    return [
      { id: null, kind: "public", nick: t("chat.public"), online: true, unread: this.unread(null), muted: this.isMuted(null) },
      ...topics,
      ...privates,
    ];
  }
}

export const chat = new ChatStore();
