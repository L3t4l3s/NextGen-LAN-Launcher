// Browser-only stand-in for the LAN chat: a few simulated players, some
// history, a running game poll, and now and then a new message so sounds and
// unread counters can be seen without a LAN.

import type { ChatItem, ChatPeer, ChatPoll, ChatSnapshot, ChatUpdate, PollKind } from "./types";

type Emit = (event: string, payload: unknown) => void;

const ME = "me0000000000demo";
const peers: ChatPeer[] = [
  { id: "a1b2c3d4e5f60001", nick: "Gandalf", os: "linux", online: true, address: "192.168.1.21", relay: false, playing: "Quake III Arena", info: { host: "MITTELERDE", system: "Arch Linux (rolling)", cpu: "AMD Ryzen 7 5800X3D", gpu: "AMD Radeon RX 7800 XT", version: "0.2.0 (demo)" } },
  { id: "a1b2c3d4e5f60002", nick: "Tinkerbell", os: "windows", online: true, address: "192.168.1.34", relay: false, playing: null, info: { host: "TINK-PC", system: "Windows 11 (26100)", cpu: "Intel Core i5-12400F", gpu: "NVIDIA GeForce RTX 3060", version: "0.2.0 (demo)" } },
  { id: "a1b2c3d4e5f60003", nick: "xX_Sniper_Xx", os: "windows", online: true, address: "192.168.1.57", relay: false, playing: "Counter-Strike 1.6 (GoldSrc)", info: { host: "DESKTOP-7Q2K", system: "Windows 10 (19045)", cpu: "Intel Core i7-9700K", gpu: "NVIDIA GeForce GTX 1080", version: "0.2.0 (demo)" } },
  { id: "a1b2c3d4e5f60004", nick: "MacGyver", os: "macos", online: false, address: "192.168.1.80", relay: false, playing: null, info: null },
  { id: "a1b2c3d4e5f600ff", nick: "Chat-Archiv", os: "linux", online: true, address: "192.168.1.10", relay: true, playing: null, info: null },
];
const [gandalf, tink, sniper] = peers;

const chatter = [
  "Wer hat Lust auf eine Runde CS?",
  "Pizza ist da! 🍕",
  "gg wp 🔥",
  "Kann jemand den Server für Quake hosten?",
  "Ich brauch noch 5 Minuten, Update lädt",
  "@DemoPlayer bist du dabei?",
  "Turnier startet um 20 Uhr, Anmeldung auf der LANPage",
];

export function createChatMock(emit: Emit, nick: () => string, gameTitle: (id: string) => string | null = () => null) {
  let enabled = true;
  let seq = 0;
  const items = new Map<string, ChatItem>();
  /** Votes per poll: voter id → (nick, option ids). */
  const votes = new Map<string, Map<string, { nick: string; choices: string[] }>>();
  /** Reactions per message: person id → (nick, emoji). */
  const reactions = new Map<string, Map<string, { nick: string; emoji: string }>>();

  const ago = (min: number) => Date.now() - min * 60_000;
  const add = (from: ChatPeer | null, text: string | null, ts: number, extra: Partial<ChatItem> = {}): ChatItem => {
    const id = `${from?.id ?? ME}:${++seq}`;
    const item: ChatItem = {
      id,
      from: from?.id ?? ME,
      nick: from?.nick ?? nick(),
      ts,
      received: ts,
      conversation: null,
      mine: !from,
      deleted: false,
      text,
      edited: false,
      topicName: null,
      game: null,
      reply: null,
      reactions: [],
      poll: null,
      ...extra,
    };
    items.set(id, item);
    return item;
  };

  function refresh(id: string): ChatItem | null {
    const item = items.get(id);
    if (!item) return null;
    const r = reactions.get(id);
    const groups = new Map<string, { nicks: string[]; mine: boolean }>();
    for (const [who, { nick: n, emoji }] of r ?? []) {
      const g = groups.get(emoji) ?? { nicks: [], mine: false };
      g.nicks.push(n);
      g.mine ||= who === ME;
      groups.set(emoji, g);
    }
    item.reactions = [...groups].map(([emoji, g]) => ({ emoji, ...g }));
    if (item.poll) {
      const v = votes.get(id) ?? new Map();
      const counted = item.poll.kind === "single" ? [...v].map(([who, x]) => [who, { ...x, choices: x.choices.slice(0, 1) }] as const) : [...v];
      item.poll.options = item.poll.options.map((o) => ({
        ...o,
        voters: counted.filter(([, x]) => x.choices.includes(o.id)).map(([, x]) => x.nick),
        mine: counted.some(([who, x]) => who === ME && x.choices.includes(o.id)),
      }));
      item.poll.participants = counted.filter(([, x]) => x.choices.length > 0).length;
    }
    return item;
  }

  const push = (...changed: (ChatItem | null)[]) => {
    const list = changed.filter((x): x is ChatItem => !!x).map((x) => structuredClone(x));
    if (list.length) emit("chat-update", { kind: "items", items: list } satisfies ChatUpdate);
  };

  // History.
  const welcome = add(gandalf, "Willkommen zur Beispiel-LAN! Der Chat läuft direkt zwischen den Launchern, ganz ohne Server.", ago(95));
  const q = add(tink, "Hat jemand ein Netzwerkkabel übrig? 🙏", ago(40));
  add(sniper, "Liegt bei mir, Tisch 7", ago(38), { reply: { id: q.id, nick: tink.nick, text: q.text, deleted: false } });
  add(null, "Ich bin auch da 👋", ago(20));
  add(sniper, "Counter-Strike 1.6 (GoldSrc)", ago(15), { game: "goldsrc" });
  const poll: ChatPoll = {
    question: "Was zocken wir als Nächstes?",
    kind: "single",
    open: false,
    closed: false,
    participants: 0,
    options: [
      { id: "0", text: "Quake 3 Arena", game: "quake3", voters: [], mine: false, addedBy: null },
      { id: "1", text: "Counter-Strike 1.6 (GoldSrc)", game: "goldsrc", voters: [], mine: false, addedBy: null },
      { id: "2", text: "Rocket League", game: "rocket", voters: [], mine: false, addedBy: null },
    ],
  };
  const pollItem = add(gandalf, null, ago(12), { poll });
  // A topic with a little in it.
  const topic = add(sniper, null, ago(30), { topicName: "CS-Turnier" });
  topic.conversation = `#${topic.id}`;
  add(sniper, "Anmeldung bis 19:30, 5v5, Maps: dust2, inferno, nuke", ago(29), { conversation: topic.conversation });
  add(gandalf, "Team Zauberer ist dabei 🧙", ago(25), { conversation: topic.conversation, edited: true });
  votes.set(pollItem.id, new Map([
    [gandalf.id, { nick: gandalf.nick, choices: ["1"] }],
    [tink.id, { nick: tink.nick, choices: ["2"] }],
    [sniper.id, { nick: sniper.nick, choices: ["1"] }],
  ]));
  reactions.set(welcome.id, new Map([
    [tink.id, { nick: tink.nick, emoji: "❤️" }],
    [sniper.id, { nick: sniper.nick, emoji: "🔥" }],
    [ME, { nick: "DemoPlayer", emoji: "❤️" }],
  ]));
  add(tink, "Kommst du nachher mit zum Bäcker?", ago(6), { conversation: tink.id });
  add(sniper, "1v1 nach dem Turnier? 😎", ago(1), { conversation: sniper.id });
  for (const id of items.keys()) refresh(id);

  // Someone says something now and then.
  setInterval(() => {
    if (!enabled) return;
    const who = peers[Math.floor(Math.random() * 3)];
    const text = chatter[Math.floor(Math.random() * chatter.length)].replace("@DemoPlayer", `@${nick()}`);
    push(add(who, text, Date.now()));
  }, 45_000);

  // The flood limit of the real chat: past five messages in ten seconds,
  // thirty seconds of pause.
  let sent: number[] = [];
  let pausedUntil = 0;
  const flood = () => {
    const now = Date.now();
    if (now < pausedUntil) throw new Error(`err.chat_too_fast|${Math.ceil((pausedUntil - now) / 1000)}`);
    sent = sent.filter((t) => now - t < 10_000);
    if (sent.length >= 5) {
      sent = [];
      pausedUntil = now + 30_000;
      throw new Error("err.chat_too_fast|30");
    }
    sent.push(now);
  };

  const invoke = (cmd: string, args: Record<string, unknown>): unknown => {
    if (cmd === "set_chat_sound") return;
    if (cmd === "chat_snapshot") {
      if (!enabled) return null;
      return { me: ME, nick: nick(), items: [...items.values()].map((i) => structuredClone(i)), peers: structuredClone(peers), problem: null } satisfies ChatSnapshot;
    }
    if (!enabled) throw new Error("err.chat_disabled");
    switch (cmd) {
      case "chat_send": {
        flood();
        const replyTo = args.replyTo as string | null;
        const target = replyTo ? items.get(replyTo) : null;
        const item = add(null, args.text as string, Date.now(), {
          conversation: (args.conversation as string | null) ?? null,
          reply: target ? { id: target.id, nick: target.nick, text: target.text ?? target.poll?.question ?? null, deleted: target.deleted } : null,
        });
        push(item);
        // A friendly thumbs-up from somebody a moment later.
        setTimeout(() => {
          const who = item.conversation ? peers.find((p) => p.id === item.conversation) : tink;
          if (!who?.online) return;
          const r = reactions.get(item.id) ?? new Map();
          r.set(who.id, { nick: who.nick, emoji: "👍" });
          reactions.set(item.id, r);
          push(refresh(item.id));
        }, 2500);
        return structuredClone(item);
      }
      case "chat_react": {
        const target = args.target as string;
        const r = reactions.get(target) ?? new Map();
        if (args.emoji) r.set(ME, { nick: nick(), emoji: args.emoji as string });
        else r.delete(ME);
        reactions.set(target, r);
        push(refresh(target));
        return;
      }
      case "chat_create_poll": {
        const options = (args.options as { text: string; game: string | null }[]).map((o, i) => ({ id: String(i), text: o.text, game: o.game, voters: [], mine: false, addedBy: null }));
        const item = add(null, null, Date.now(), {
          conversation: (args.conversation as string | null) ?? null,
          poll: { question: args.question as string, kind: args.kind as PollKind, open: args.open as boolean, closed: false, options, participants: 0 },
        });
        push(item);
        return structuredClone(item);
      }
      case "chat_vote": {
        const id = args.poll as string;
        if (items.get(id)?.poll?.closed) throw new Error("err.chat_poll_closed");
        const v = votes.get(id) ?? new Map();
        v.set(ME, { nick: nick(), choices: args.choices as string[] });
        votes.set(id, v);
        push(refresh(id));
        return;
      }
      case "chat_add_poll_option": {
        const id = args.poll as string;
        const p = items.get(id)?.poll;
        if (p) p.options.push({ id: `${ME}:${++seq}`, text: args.text as string, game: null, voters: [], mine: false, addedBy: nick() });
        push(refresh(id));
        return;
      }
      case "chat_close_poll": {
        const p = items.get(args.poll as string)?.poll;
        if (p) p.closed = true;
        push(refresh(args.poll as string));
        return;
      }
      case "chat_edit": {
        const item = items.get(args.target as string);
        if (!item?.mine) throw new Error("err.chat_not_allowed");
        item.text = args.text as string;
        item.edited = true;
        const replies = [...items.values()].filter((i) => i.reply?.id === item.id);
        for (const r of replies) r.reply = { ...r.reply!, text: item.text };
        push(item, ...replies);
        return;
      }
      case "chat_share_game": {
        flood();
        const game = gameTitle(args.game as string);
        if (!game) throw new Error("err.unknown_game");
        const item = add(null, game, Date.now(), { conversation: (args.conversation as string | null) ?? null, game: args.game as string });
        push(item);
        return structuredClone(item);
      }
      case "chat_create_topic": {
        const item = add(null, null, Date.now(), { topicName: (args.name as string).trim() });
        item.conversation = `#${item.id}`;
        push(item);
        return structuredClone(item);
      }
      case "chat_delete": {
        const item = items.get(args.target as string);
        if (!item?.mine) throw new Error("err.chat_not_allowed");
        Object.assign(item, { deleted: true, text: null, poll: null, reactions: [] });
        const replies = [...items.values()].filter((i) => i.reply?.id === item.id);
        for (const r of replies) r.reply = { ...r.reply!, deleted: true, text: null };
        push(item, ...replies);
        return;
      }
    }
    throw new Error(`mock: unknown command ${cmd}`);
  };

  return {
    invoke,
    setEnabled(on: boolean) {
      if (on === enabled) return;
      enabled = on;
      emit("chat-reset", null);
    },
  };
}
