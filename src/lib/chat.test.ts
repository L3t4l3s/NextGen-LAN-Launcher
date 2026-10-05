import { describe, expect, it } from "vitest";
import { isUnread, linkTarget, mentions, onlyEmoji, percent, rings, rows, segments } from "./chat";
import type { ChatItem } from "./types";

function item(id: string, from: string, ts: number, extra: Partial<ChatItem> = {}): ChatItem {
  return { id, from, nick: from, ts, received: ts, conversation: null, mine: false, deleted: false, text: "x", edited: false, topicName: null, game: null, reply: null, reactions: [], poll: null, ...extra };
}

describe("segments", () => {
  it("finds links without their trailing punctuation", () => {
    expect(segments("see http://myparty.lan/turnier.", [], "me")).toEqual([
      { kind: "text", text: "see " },
      { kind: "link", text: "http://myparty.lan/turnier" },
      { kind: "text", text: "." },
    ]);
  });

  it("marks mentions, the longest nickname first, and whether they name me", () => {
    expect(segments("@Max Power and @max, food!", ["Max", "Max Power"], "max")).toEqual([
      { kind: "mention", text: "@Max Power", me: false },
      { kind: "text", text: " and " },
      { kind: "mention", text: "@max", me: true },
      { kind: "text", text: ", food!" },
    ]);
    expect(mentions("hey @Bob", "bob")).toBe(true);
    expect(mentions("hey Bob", "bob")).toBe(false);
    expect(mentions("@Bobby gg", "Bob")).toBe(false);
    expect(mentions("gg @Bob!", "Bob")).toBe(true);
    expect(segments("a.b @x.y", ["x.y"], "")).toEqual([
      { kind: "text", text: "a.b " },
      { kind: "mention", text: "@x.y", me: false },
    ]);
  });

  it("opens links with the scheme in lower case", () => {
    const [link] = segments("HTTPS://LanPage.lan/X", [], "").filter((s) => s.kind === "link");
    expect(linkTarget(link.text)).toBe("https://LanPage.lan/X");
  });

  it("keeps markup as plain text", () => {
    expect(segments("<b>hi</b>", [], "me")).toEqual([{ kind: "text", text: "<b>hi</b>" }]);
  });
});

describe("rows", () => {
  const day = (ts: number) => (ts < 1000 ? "day1" : "day2");

  it("names the author again after the line that opened a topic", () => {
    const r = rows([item("1", "a", 1, { conversation: "#1", topicName: "T" }), item("2", "a", 2, { conversation: "#1" })], () => "d");
    expect(r[1].head).toBe(true);
  });

  it("groups runs of one person and starts a new group on a new day", () => {
    const r = rows([item("1", "a", 1), item("2", "a", 2), item("3", "b", 3), item("4", "b", 1001)], day);
    expect(r.map((x) => [x.head, x.day])).toEqual([
      [true, "day1"],
      [false, null],
      [true, null],
      [true, "day2"],
    ]);
  });
});

describe("unread and sounds", () => {
  const now = 10_000_000;

  it("go by arrival, so a slow clock elsewhere does not hide a message", () => {
    const slow = item("1", "a", now - 5 * 60_000, { received: now });
    expect(isUnread(slow, now - 1000, 0)).toBe(true);
    expect(rings(slow, now + 1000)).toBe(true);
  });

  it("keep history caught up on quiet and history from before the first start read", () => {
    const old = item("1", "a", now - 3 * 3600_000, { received: now });
    expect(rings(old, now)).toBe(false);
    expect(isUnread(old, 0, now - 60_000)).toBe(false);
    expect(isUnread(item("2", "a", now, { mine: true }), 0, 0)).toBe(false);
  });
});

describe("misc", () => {
  it("shows a few emoji large, nothing else", () => {
    expect(onlyEmoji("🔥🔥")).toBe(true);
    expect(onlyEmoji("👍🏽")).toBe(true);
    expect(onlyEmoji("gg 🔥")).toBe(false);
    expect(onlyEmoji("🔥🔥🔥🔥")).toBe(false);
    expect(onlyEmoji("123")).toBe(false);
  });

  it("computes poll shares", () => {
    expect(percent(1, 3)).toBe(33);
    expect(percent(0, 0)).toBe(0);
  });
});
