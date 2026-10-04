// State of the chat panel (Svelte 5 runes). The backend owns the messages;
// this keeps the copy the panel shows, what was read, and plays the sounds.

import { api, listen } from "$lib/api";
import { byTime, convKey, isUnread, mentions, rings } from "$lib/chat";
import { playSound, unlockAudio } from "$lib/chat-sound";
import { userText } from "$lib/i18n";
import { app } from "$lib/stores/app.svelte";
import type { ChatItem, ChatPeer, ChatSnapshot, ChatUpdate } from "$lib/types";

const OPEN_KEY = "nll.chat.open";
const READ_KEY = "nll.chat.read";
const SINCE_KEY = "nll.chat.since";

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
  /** null: the public room. */
  id: string | null;
  nick: string;
  online: boolean;
  unread: number;
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
  /** Private conversations opened in this session, even before a word was said. */
  private opened = $state<string[]>([]);
  /** Conversations closed by hand; a new message brings them back. */
  private hidden = $state<string[]>([]);

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
    // A first start has read nothing and missed nothing: the history that
    // arrives is the room's past, not a pile of unread messages.
    store(SINCE_KEY, this.since);
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
      if (this.isOnScreen(item.conversation)) continue;
      const direct = item.conversation !== null || mentions(item.text, this.nick);
      sound = direct ? "direct" : (sound ?? "message");
    }
    this.items = next;
    this.markRead();
    if (sound && app.settings?.chatSound) playSound(sound);
  }

  /** The conversation is visible right now, so its new messages are read. */
  private isOnScreen(conversation: string | null): boolean {
    return this.open && this.active === conversation && typeof document !== "undefined" && document.hasFocus();
  }

  markRead() {
    if (!this.isOnScreen(this.active)) return;
    const key = convKey(this.active);
    const newest = Math.max(0, ...this.list(this.active).map((i) => i.received));
    if (newest > (this.readUpTo[key] ?? 0)) {
      this.readUpTo = { ...this.readUpTo, [key]: newest };
      store(READ_KEY, this.readUpTo);
    }
  }

  setOpen(open: boolean) {
    this.open = open;
    store(OPEN_KEY, open);
    this.markRead();
  }

  show(conversation: string | null) {
    if (conversation && !this.opened.includes(conversation)) this.opened = [...this.opened, conversation];
    if (conversation) this.hidden = this.hidden.filter((c) => c !== conversation);
    if (this.active !== conversation) this.replyTo = null;
    this.active = conversation;
    this.setOpen(true);
  }

  /** Close a private conversation tab (its messages stay). */
  hide(conversation: string) {
    this.opened = this.opened.filter((c) => c !== conversation);
    const key = convKey(conversation);
    const newest = Math.max(0, ...this.list(conversation).map((i) => i.received));
    this.readUpTo = { ...this.readUpTo, [key]: Math.max(newest, this.readUpTo[key] ?? 0) };
    store(READ_KEY, this.readUpTo);
    this.hidden = [...this.hidden.filter((c) => c !== conversation), conversation];
    if (this.active === conversation) this.active = null;
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

  get online(): ChatPeer[] {
    return this.peers.filter((p) => p.online);
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
    return [...new Set([this.nick, ...this.peers.map((p) => p.nick)])];
  }

  /** The public room, then private conversations: opened ones, and those
   *  with messages unless closed and nothing new arrived since. */
  get conversations(): Conversation[] {
    const ids = new Set<string>(this.opened);
    for (const i of Object.values(this.items)) {
      if (i.conversation === null) continue;
      const hiddenAndRead = this.hidden.includes(i.conversation) && i.received <= (this.readUpTo[i.conversation] ?? 0);
      if (!hiddenAndRead) ids.add(i.conversation);
    }
    const privates = [...ids].map((id) => ({
      id,
      nick: this.nickOf(id),
      online: this.peers.some((p) => p.id === id && p.online),
      unread: this.unread(id),
    }));
    privates.sort((a, b) => a.nick.localeCompare(b.nick));
    return [{ id: null, nick: "", online: true, unread: this.unread(null) }, ...privates];
  }
}

export const chat = new ChatStore();
