// Pure helpers for the chat panel; the state lives in stores/chat.svelte.ts.

import type { ChatItem } from "./types";

/** Key of a conversation for maps: "" is the public room. */
export const convKey = (conversation: string | null): string => conversation ?? "";

/** Topics are public rooms of their own, keyed "#<id of the event that opened it>". */
export const isTopic = (conversation: string | null): boolean => !!conversation?.startsWith("#");

export type Segment = { kind: "text"; text: string } | { kind: "link"; text: string } | { kind: "mention"; text: string; me: boolean };

const LINK = /\bhttps?:\/\/[^\s<>"]+[^\s<>".,;:!?)\]}'»“]/gi;
const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * Split a message into plain text, links and @mentions of known nicknames.
 * Rendering the parts as text nodes keeps whatever someone types from ever
 * becoming markup.
 */
export function segments(text: string, nicks: string[], myNick: string): Segment[] {
  // Longest nicknames first, so "@Max Power" is not taken for "@Max"; a
  // mention ends where the name does ("@Bobby" does not name "Bob").
  const names = [...new Set(nicks.filter(Boolean))].sort((a, b) => b.length - a.length);
  const mention = names.length ? new RegExp(`@(${names.map(escape).join("|")})(?![\\p{L}\\p{N}_])`, "giu") : null;
  const out: Segment[] = [];
  const pushText = (part: string) => {
    if (!part) return;
    if (!mention) {
      out.push({ kind: "text", text: part });
      return;
    }
    let last = 0;
    for (const m of part.matchAll(mention)) {
      if (m.index > last) out.push({ kind: "text", text: part.slice(last, m.index) });
      out.push({ kind: "mention", text: m[0], me: m[1].toLowerCase() === myNick.toLowerCase() });
      last = m.index + m[0].length;
    }
    if (last < part.length) out.push({ kind: "text", text: part.slice(last) });
  };
  let last = 0;
  for (const m of text.matchAll(LINK)) {
    pushText(text.slice(last, m.index));
    out.push({ kind: "link", text: m[0] });
    last = (m.index ?? 0) + m[0].length;
  }
  pushText(text.slice(last));
  return out;
}

/** The address to open for a link: the backend takes the scheme in lower case only. */
export function linkTarget(link: string): string {
  return link.replace(/^https?/i, (scheme) => scheme.toLowerCase());
}

/** Whether a message names `myNick` with an @. */
export function mentions(text: string | null, myNick: string): boolean {
  if (!text || !myNick) return false;
  return segments(text, [myNick], myNick).some((s) => s.kind === "mention" && s.me);
}

/** How far the clocks of two launchers may disagree and a message still
 *  counts as "just written". */
const SKEW_MS = 10 * 60_000;

/** Unread: arrived after the last read, and not history from before this
 *  launcher's first chat. Arrival is this launcher's own clock; the author's
 *  may be off. */
export function isUnread(item: ChatItem, readUpTo: number, since: number): boolean {
  return !item.mine && !item.deleted && item.received > readUpTo && item.ts >= since - SKEW_MS;
}

/** A new message rings when it just arrived and was just written, so a
 *  backlog caught up on stays quiet. */
export function rings(item: ChatItem, now: number): boolean {
  return now - item.received < 60_000 && item.received - item.ts < SKEW_MS;
}

/** A stable colour per person, readable on dark and light themes. */
export function nickColor(id: string): string {
  let hash = 0;
  for (const c of id) hash = (hash * 31 + c.charCodeAt(0)) >>> 0;
  return `hsl(${hash % 360} 65% 60%)`;
}

/** A message that only holds one to three emoji is shown large. */
export function onlyEmoji(text: string | null): boolean {
  if (!text) return false;
  const trimmed = text.trim();
  if (!trimmed || trimmed.length > 24) return false;
  const chars = [...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(trimmed)].map((s) => s.segment).filter((s) => s.trim());
  return chars.length <= 3 && chars.every((c) => /\p{Extended_Pictographic}/u.test(c));
}

export interface Row {
  item: ChatItem;
  /** Show name (and the gap above): first of a run by the same person. */
  head: boolean;
  /** Day label above this message, when a new day starts. */
  day: string | null;
}

const RUN_MS = 3 * 60_000;

/** Group consecutive messages of one person and mark day changes. */
export function rows(items: ChatItem[], dayLabel: (ts: number) => string): Row[] {
  const out: Row[] = [];
  let prev: ChatItem | null = null;
  for (const item of items) {
    const day = dayLabel(item.ts);
    const newDay = !prev || dayLabel(prev.ts) !== day;
    // A poll, and the line that opens a topic, each stand on their own.
    const opener = (i: ChatItem) => i.conversation === `#${i.id}`;
    const head = newDay || !prev || prev.from !== item.from || item.ts - prev.ts > RUN_MS || !!item.poll || !!prev.poll || opener(prev);
    out.push({ item, head, day: newDay ? day : null });
    prev = item;
  }
  return out;
}

/** Order messages as the backend does: time, then id. */
export function byTime(a: ChatItem, b: ChatItem): number {
  return a.ts - b.ts || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
}

/** Share of the votes for a poll option, 0–100. */
export function percent(votes: number, participants: number): number {
  return participants > 0 ? Math.round((votes / participants) * 100) : 0;
}
