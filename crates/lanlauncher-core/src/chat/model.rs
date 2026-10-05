//! What the chat is made of, independent of how it travels.
//!
//! Everything said in the chat is an [`Event`]: a message, a reaction, a poll,
//! a vote. Events never change once written; a reaction taken back is a new
//! event, and for each person the newest one counts. That makes the history
//! a set that any two launchers can merge by exchanging the ids they are
//! missing, in any order, as often as they like — which is all the network
//! layer does.
//!
//! [`ChatState`] folds the events into [`ItemView`]s, one per message or poll,
//! as the interface shows them.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Longest message, in characters.
pub const MAX_TEXT: usize = 2000;
/// Longest nickname, in characters.
pub const MAX_NICK: usize = 32;
pub const MAX_QUESTION: usize = 300;
/// Longest topic name, in characters.
pub const MAX_TOPIC: usize = 40;
pub const MAX_OPTION_TEXT: usize = 120;
/// Options a poll may start with; open polls may grow to [`MAX_POLL_OPTIONS_TOTAL`].
pub const MAX_POLL_OPTIONS: usize = 12;
pub const MAX_POLL_OPTIONS_TOTAL: usize = 30;
/// A reaction is one emoji; the longest family/flag sequences stay below this.
pub const MAX_EMOJI_BYTES: usize = 32;
/// Longest sealed body (hex) a relay keeps without being able to read it:
/// the largest readable body, encrypted, with room to spare.
const MAX_SEALED_HEX: usize = 64 * 1024;
const MAX_ID: usize = 64;

/// Error codes the chat hands to the interface (`err.chat_*`, see i18n).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatError(pub &'static str);

impl std::fmt::Display for ChatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ChatError {}

pub const ERR_INVALID: ChatError = ChatError("err.chat_invalid");
pub const ERR_UNKNOWN_TARGET: ChatError = ChatError("err.chat_unknown_target");
pub const ERR_NOT_ALLOWED: ChatError = ChatError("err.chat_not_allowed");
pub const ERR_POLL_CLOSED: ChatError = ChatError("err.chat_poll_closed");
pub const ERR_DISABLED: ChatError = ChatError("err.chat_disabled");
/// Too many messages in a short time; see `Flood` in the chat module.
pub const ERR_TOO_FAST: ChatError = ChatError("err.chat_too_fast");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    /// `<from>:<seq>`, unique across the LAN.
    pub id: String,
    /// Peer id of the author (random, kept in the data directory).
    pub from: String,
    /// The author's nickname when the event was written.
    pub nick: String,
    /// Per-author counter; for reactions and votes the highest one counts.
    pub seq: u64,
    /// Author's clock, milliseconds since the epoch.
    pub ts: i64,
    /// Private message to this peer; `None` is the public room.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Public event in a topic: the id of the [`Body::Topic`] event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    pub body: Body,
    /// The author's Ed25519 signature (hex) over everything else
    /// ([`super::crypto`]); empty only before signing.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sig: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PollKind {
    /// One answer per person.
    #[default]
    Single,
    /// Any number of answers per person.
    Multiple,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollChoice {
    pub text: String,
    /// Catalog id when the option is a game, so the interface can show its cover.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Body {
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
        /// A game of the catalog this message links to (its id); `text` is
        /// its title. Signed with the body: a launcher from before links
        /// drops such a message (no release had them; none needs to cope).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        game: Option<String>,
    },
    /// One reaction per person and message; an empty emoji takes it back.
    React {
        target: String,
        emoji: String,
    },
    Poll {
        question: String,
        options: Vec<PollChoice>,
        kind: PollKind,
        /// Everybody may add answers, not only the author.
        #[serde(default)]
        open: bool,
    },
    PollOption {
        poll: String,
        #[serde(flatten)]
        choice: PollChoice,
    },
    /// The voter's complete answer; an empty list withdraws the vote.
    Vote {
        poll: String,
        choices: Vec<String>,
    },
    ClosePoll {
        poll: String,
    },
    /// Only the author may delete; the message stays as "deleted".
    Delete {
        target: String,
    },
    /// Only the author may edit a text message; the newest edit counts.
    Edit {
        target: String,
        text: String,
    },
    /// Opens a topic: a public room of its own, for everyone.
    Topic {
        name: String,
    },
    /// The body of a private event as it travels: one of the others,
    /// encrypted for the two participants (hex). Never folded into the state.
    Sealed {
        nonce: String,
        data: String,
    },
}

impl Body {
    /// The message or poll this event refers to, for everything that is not
    /// one itself.
    pub fn target(&self) -> Option<&str> {
        match self {
            Body::Text { .. } | Body::Poll { .. } | Body::Topic { .. } | Body::Sealed { .. } => {
                None
            }
            Body::React { target, .. } | Body::Delete { target } | Body::Edit { target, .. } => {
                Some(target)
            }
            Body::PollOption { poll, .. } | Body::Vote { poll, .. } | Body::ClosePoll { poll } => {
                Some(poll)
            }
        }
    }
}

pub fn valid_peer_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn valid_event_id(id: &str) -> bool {
    id.split_once(':')
        .is_some_and(|(peer, seq)| valid_peer_id(peer) && seq.parse::<u64>().is_ok())
}

fn within(text: &str, max_chars: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= max_chars
}

/// A catalog id: short, one line.
fn valid_game_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ID && !id.chars().any(|c| c.is_control())
}

fn valid_choice(c: &PollChoice) -> bool {
    within(&c.text, MAX_OPTION_TEXT) && c.game.as_deref().is_none_or(|g| g.len() <= MAX_ID)
}

/// Trim a nickname to what the chat accepts; empty when nothing is left.
pub fn clean_nick(nick: &str) -> String {
    nick.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NICK)
        .collect()
}

impl Event {
    /// Whether the outside of an event is well-formed, for a relay that
    /// keeps private events it cannot open: ids, nickname, and a sealed body
    /// of sane size exactly where the event is private.
    pub fn envelope_is_valid(&self) -> bool {
        match (&self.to, &self.body) {
            (Some(_), Body::Sealed { nonce, data }) => {
                let mut outside = self.clone();
                outside.body = Body::Delete {
                    target: format!("{}:0", self.from),
                };
                outside.is_valid()
                    && nonce.len() == 48
                    && data.len() <= MAX_SEALED_HEX
                    && [nonce, data]
                        .iter()
                        .all(|h| h.chars().all(|c| c.is_ascii_hexdigit()))
            }
            (None, _) => self.is_valid(),
            (Some(_), _) => false,
        }
    }

    /// Whether the event is well-formed. Anyone on the LAN can send anything,
    /// so this runs on every event that arrives, before it is stored.
    pub fn is_valid(&self) -> bool {
        if !valid_peer_id(&self.from)
            || self.id != format!("{}:{}", self.from, self.seq)
            || !within(&self.nick, MAX_NICK)
            || self
                .to
                .as_deref()
                .is_some_and(|t| !valid_peer_id(t) || t == self.from)
            // Topics are public, and a topic is not inside another one.
            || self.topic.as_deref().is_some_and(|t| {
                !valid_event_id(t) || self.to.is_some() || matches!(self.body, Body::Topic { .. })
            })
        {
            return false;
        }
        match &self.body {
            Body::Text {
                text,
                reply_to,
                game,
            } => {
                within(text, MAX_TEXT)
                    && reply_to.as_deref().is_none_or(valid_event_id)
                    && game.as_deref().is_none_or(valid_game_id)
            }
            Body::React { target, emoji } => {
                valid_event_id(target)
                    && emoji.len() <= MAX_EMOJI_BYTES
                    && !emoji.chars().any(|c| c.is_control() || c.is_whitespace())
            }
            Body::Poll {
                question, options, ..
            } => {
                within(question, MAX_QUESTION)
                    && (2..=MAX_POLL_OPTIONS).contains(&options.len())
                    && options.iter().all(valid_choice)
            }
            Body::PollOption { poll, choice } => valid_event_id(poll) && valid_choice(choice),
            Body::Vote { poll, choices } => {
                valid_event_id(poll)
                    && choices.len() <= MAX_POLL_OPTIONS_TOTAL
                    && choices.iter().all(|c| c.len() <= MAX_ID * 2)
            }
            Body::ClosePoll { poll } => valid_event_id(poll),
            Body::Delete { target } => valid_event_id(target),
            Body::Edit { target, text } => valid_event_id(target) && within(text, MAX_TEXT),
            Body::Topic { name } => self.to.is_none() && within(name, MAX_TOPIC),
            Body::Sealed { .. } => false,
        }
    }

    /// The conversation as seen from `me`: `None` for the public room, the
    /// other person's id for a private one, `#<topic id>` for a topic (the
    /// event opening a topic belongs to it).
    pub fn conversation(&self, me: &str) -> Option<String> {
        if let Body::Topic { .. } = self.body {
            return Some(format!("#{}", self.id));
        }
        if let Some(topic) = &self.topic {
            return Some(format!("#{topic}"));
        }
        let to = self.to.as_ref()?;
        Some(if self.from == me {
            to.clone()
        } else {
            self.from.clone()
        })
    }

    /// Whether `peer` takes part in this event's conversation and may get it.
    pub fn visible_to(&self, peer: &str) -> bool {
        match &self.to {
            None => true,
            Some(to) => to == peer || self.from == peer,
        }
    }
}

/// An event as kept on disk and sent on: the signed (and, if private,
/// sealed) form, with the time this launcher first saw it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    /// When it reached this launcher, by its own clock: what "unread" is
    /// measured with.
    pub seen: i64,
    pub event: Event,
    /// When it first reached the LAN, worked out from the age the sender
    /// reported; what expiry counts from. `None`: the same as `seen`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub born: Option<i64>,
}

impl Stored {
    pub fn born(&self) -> i64 {
        self.born.unwrap_or(self.seen)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyView {
    pub id: String,
    /// `None` until the message replied to has arrived.
    pub nick: Option<String>,
    pub text: Option<String>,
    pub deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReactionView {
    pub emoji: String,
    pub nicks: Vec<String>,
    pub mine: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionView {
    pub id: String,
    pub text: String,
    pub game: Option<String>,
    /// Nicknames of everyone who chose it.
    pub voters: Vec<String>,
    pub mine: bool,
    /// Added by a participant (open poll) rather than the author.
    pub added_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PollView {
    pub question: String,
    pub kind: PollKind,
    pub open: bool,
    pub closed: bool,
    pub options: Vec<OptionView>,
    /// People who voted at all.
    pub participants: usize,
}

/// A message or poll as the interface shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub id: String,
    pub from: String,
    pub nick: String,
    /// For ordering: the author's time, unless that lies after the moment
    /// this launcher first saw the message (a clock running ahead).
    pub ts: i64,
    /// When this launcher first saw it, by its own clock: what "unread" and
    /// "new" are measured with, whatever the author's clock says.
    pub received: i64,
    /// `None` is the public room, `#<id>` a topic, otherwise the other
    /// person's id.
    pub conversation: Option<String>,
    pub mine: bool,
    pub deleted: bool,
    pub text: Option<String>,
    /// The text was changed after it was sent.
    pub edited: bool,
    /// Set on the event that opened a topic: its name.
    pub topic_name: Option<String>,
    /// A linked game's catalog id; `text` is its title.
    pub game: Option<String>,
    pub reply: Option<ReplyView>,
    pub reactions: Vec<ReactionView>,
    pub poll: Option<PollView>,
}

/// (seq, emoji, nick) of one person's reaction; an empty emoji was taken back.
type Reaction = (u64, String, String);
/// (seq, option ids, nick) of one person's vote.
type Vote = (u64, Vec<String>, String);

#[derive(Debug)]
struct Item {
    event: Event,
    seen: i64,
    /// Peer → (seq, emoji, nick); empty emoji = taken back.
    reactions: HashMap<String, Reaction>,
    deleted: bool,
    poll: Option<PollState>,
    /// The newest edit: (seq, text).
    edit: Option<(u64, String)>,
}

impl Item {
    /// The text as it reads now, edits included.
    fn text(&self) -> Option<&str> {
        match (&self.edit, &self.event.body) {
            (Some((_, text)), _) | (None, Body::Text { text, .. }) => Some(text),
            _ => None,
        }
    }
}

/// An option added to a poll: (event id, choice, nick, author time).
type Added = (String, PollChoice, String, i64);

#[derive(Debug, Default)]
struct PollState {
    /// Every option someone was allowed to add, whenever it arrived; which
    /// of them count is decided by [`PollState::counted`].
    added: Vec<Added>,
    /// Peer → (seq, choices, nick).
    votes: HashMap<String, Vote>,
    /// Author time of the author's earliest close.
    closed: Option<i64>,
}

impl PollState {
    /// The added options that count, the same on every launcher whatever
    /// order the events came in: those added before the close, earliest
    /// first, up to [`MAX_POLL_OPTIONS_TOTAL`] in all.
    fn counted(&self, base: usize) -> Vec<&Added> {
        let mut out: Vec<&Added> = self
            .added
            .iter()
            .filter(|(_, _, _, ts)| self.closed.is_none_or(|closed| *ts <= closed))
            .collect();
        out.sort_by(|a, b| a.3.cmp(&b.3).then_with(|| a.0.cmp(&b.0)));
        out.truncate(MAX_POLL_OPTIONS_TOTAL.saturating_sub(base));
        out
    }
}

#[derive(Debug)]
struct Entry {
    seen: i64,
    /// See [`Stored::born`].
    born: i64,
    /// As received: signed, private bodies sealed. This is what is saved and
    /// passed on.
    wire: Event,
    /// As read: what the state is folded from. For an event kept only to
    /// be passed on (a relay), the same as `wire`.
    plain: Event,
    /// Folded into the state; `false` for events kept only to pass on.
    opened: bool,
}

impl Entry {
    fn stored(&self) -> Stored {
        Stored {
            seen: self.seen,
            event: self.wire.clone(),
            born: (self.born != self.seen).then_some(self.born),
        }
    }
}

/// All events this launcher knows, folded into messages and polls.
#[derive(Debug)]
pub struct ChatState {
    me: String,
    /// Every accepted event in the order it arrived, for syncing and saving.
    events: Vec<Entry>,
    /// Events older than this (author time) were trimmed away and are not
    /// taken in again; 0 while nothing was trimmed.
    floor: i64,
    ids: HashSet<String>,
    items: HashMap<String, Item>,
    /// Events whose message has not arrived yet, by that message's id.
    pending: HashMap<String, Vec<Event>>,
    /// Message id → ids of the messages replying to it.
    replies: HashMap<String, Vec<String>>,
    /// Ids that expired here, with when (this launcher's clock): a peer that
    /// got them later still has them and would hand them back.
    gone: HashMap<String, i64>,
}

impl ChatState {
    pub fn new(me: &str) -> Self {
        Self {
            me: me.to_string(),
            events: Vec::new(),
            floor: 0,
            ids: HashSet::new(),
            items: HashMap::new(),
            pending: HashMap::new(),
            gone: HashMap::new(),
            replies: HashMap::new(),
        }
    }

    pub fn me(&self) -> &str {
        &self.me
    }

    pub fn contains(&self, id: &str) -> bool {
        self.ids.contains(id)
    }

    /// Everything kept, in the form saved and sent on.
    pub fn stored(&self) -> Vec<Stored> {
        self.events.iter().map(Entry::stored).collect()
    }

    /// Back-date an event just taken in to when it first reached the LAN.
    pub fn set_born(&mut self, id: &str, born: i64) {
        if let Some(e) = self.events.iter_mut().rev().find(|e| e.plain.id == id) {
            e.born = born.min(e.seen);
        }
    }

    pub fn floor(&self) -> i64 {
        self.floor
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Highest sequence number among this launcher's own events.
    pub fn last_own_seq(&self) -> u64 {
        self.events
            .iter()
            .filter(|e| e.plain.from == self.me)
            .map(|e| e.plain.seq)
            .max()
            .unwrap_or(0)
    }

    /// Whether `event` is visible here at all: public, or a private message
    /// to or from this launcher. Anything else was misrouted.
    fn concerns_me(&self, event: &Event) -> bool {
        event.visible_to(&self.me)
    }

    /// Take in an event that travels as it reads (tests, nothing private).
    pub fn insert(&mut self, event: Event, seen: i64) -> Option<Vec<String>> {
        self.insert_opened(event.clone(), event, seen)
    }

    /// Take an event in: `plain` is what it says, `wire` the form it came in
    /// (checked by the caller). Returns the ids of the items whose view
    /// changed, or `None` when it was a duplicate, invalid, not meant for us
    /// or older than what is kept.
    pub fn insert_opened(&mut self, plain: Event, wire: Event, seen: i64) -> Option<Vec<String>> {
        if self.ids.contains(&plain.id)
            || self.gone.contains_key(&plain.id)
            || wire.id != plain.id
            || !plain.is_valid()
            || !self.concerns_me(&plain)
            // A topic's opener may be older than what is kept; the topic
            // still needs it (see `keep_topic_openers`).
            || (plain.ts < self.floor && !matches!(plain.body, Body::Topic { .. }))
        {
            return None;
        }
        self.ids.insert(plain.id.clone());
        self.events.push(Entry {
            seen,
            born: seen,
            wire,
            plain: plain.clone(),
            opened: true,
        });
        let mut changed = Vec::new();
        self.apply(plain, seen, &mut changed);
        changed.sort();
        changed.dedup();
        Some(changed)
    }

    /// Keep an event only to pass it on, without reading it: what a relay
    /// does with everything, private events of others included. The caller
    /// has checked the signature. Returns whether it was new.
    pub fn insert_opaque(&mut self, wire: Event, seen: i64) -> bool {
        if self.ids.contains(&wire.id)
            || self.gone.contains_key(&wire.id)
            || !wire.envelope_is_valid()
            || (wire.ts < self.floor && !matches!(wire.body, Body::Topic { .. }))
        {
            return false;
        }
        self.ids.insert(wire.id.clone());
        self.events.push(Entry {
            seen,
            born: seen,
            plain: wire.clone(),
            wire,
            opened: false,
        });
        true
    }

    /// Keep the newest `limit` events (by author time) and fold the state
    /// again from them. Returns whether anything was dropped.
    pub fn trim(&mut self, limit: usize) -> bool {
        if self.events.len() <= limit {
            return false;
        }
        let mut entries = std::mem::take(&mut self.events);
        entries.sort_by(|a, b| {
            a.plain
                .ts
                .cmp(&b.plain.ts)
                .then_with(|| a.plain.id.cmp(&b.plain.id))
        });
        let dropped: Vec<Entry> = entries.drain(..entries.len() - limit).collect();
        self.floor = entries
            .first()
            .map_or(self.floor, |e| e.plain.ts.max(self.floor));
        let (entries, _) = keep_topic_openers(entries, dropped);
        self.refold(entries);
        true
    }

    /// Forget what this launcher got more than `keep` ago and refuse it from
    /// now on: the chat of the last LAN is not the chat of this one.
    ///
    /// Measured by arrival on this launcher's own clock, never by the
    /// author's: an author whose clock is days off (a flat CMOS battery)
    /// would otherwise have their messages refused everywhere. What it does
    /// not cover is this launcher's own clock jumping while it runs: forward,
    /// and what it holds expires early on this machine; backward, and arrival
    /// times lie in the future — those are pulled back to now, so they still
    /// expire, just later. Expired ids are remembered for another `keep`, by
    /// which time every launcher has let them go too. Returns whether
    /// anything went.
    pub fn expire(&mut self, now: i64, keep: i64) -> bool {
        for e in self.events.iter_mut() {
            e.born = e.born.min(now);
        }
        for at in self.gone.values_mut() {
            *at = (*at).min(now);
        }
        self.gone.retain(|_, at| now - *at < keep);
        // A topic's opener stays while anything in the topic does (see
        // `keep_topic_openers`); keeping it is not a change.
        let used: HashSet<String> = self
            .events
            .iter()
            .filter(|e| now - e.born < keep)
            .filter_map(|e| e.plain.topic.clone())
            .collect();
        let goes = |e: &Entry| {
            now - e.born >= keep
                && !(matches!(e.plain.body, Body::Topic { .. }) && used.contains(&e.plain.id))
        };
        if !self.events.iter().any(goes) {
            return false;
        }
        let (dropped, kept): (Vec<Entry>, Vec<Entry>) =
            std::mem::take(&mut self.events).into_iter().partition(goes);
        for e in &dropped {
            self.gone.insert(e.plain.id.clone(), now);
        }
        self.refold(kept);
        true
    }

    /// Expired ids with when they expired, to save; see [`ChatState::expire`].
    pub fn gone(&self) -> &HashMap<String, i64> {
        &self.gone
    }

    pub fn set_gone(&mut self, gone: HashMap<String, i64>) {
        self.gone = gone;
    }

    /// Fold the state again from `entries`, as if they had just arrived.
    fn refold(&mut self, entries: Vec<Entry>) {
        self.ids.clear();
        self.items.clear();
        self.pending.clear();
        self.replies.clear();
        for e in entries {
            let (id, born) = (e.plain.id.clone(), e.born);
            if e.opened {
                self.insert_opened(e.plain, e.wire, e.seen);
            } else {
                self.insert_opaque(e.wire, e.seen);
            }
            self.set_born(&id, born);
        }
    }

    fn apply(&mut self, event: Event, seen: i64, changed: &mut Vec<String>) {
        let Some(target) = event.body.target().map(str::to_string) else {
            // A message or poll of its own.
            let id = event.id.clone();
            if let Body::Text {
                reply_to: Some(r), ..
            } = &event.body
            {
                self.replies.entry(r.clone()).or_default().push(id.clone());
            }
            let poll = matches!(event.body, Body::Poll { .. }).then(PollState::default);
            self.items.insert(
                id.clone(),
                Item {
                    event,
                    seen,
                    reactions: HashMap::new(),
                    deleted: false,
                    poll,
                    edit: None,
                },
            );
            changed.push(id.clone());
            changed.extend(self.replies.get(&id).cloned().unwrap_or_default());
            for later in self.pending.remove(&id).unwrap_or_default() {
                self.apply(later, seen, changed);
            }
            return;
        };
        let me = self.me.clone();
        let Some(item) = self.items.get_mut(&target) else {
            self.pending.entry(target).or_default().push(event);
            return;
        };
        // A reaction to a private message must stay in that conversation;
        // one that claims otherwise is ignored rather than leaked.
        if event.conversation(&me) != item.event.conversation(&me) {
            return;
        }
        let author = item.event.from == event.from;
        match event.body {
            Body::React { emoji, .. } => {
                let newer = item
                    .reactions
                    .get(&event.from)
                    .is_none_or(|(seq, _, _)| *seq < event.seq);
                if newer {
                    item.reactions
                        .insert(event.from.clone(), (event.seq, emoji, event.nick.clone()));
                }
            }
            Body::Delete { .. } => {
                if !author {
                    return;
                }
                item.deleted = true;
                changed.extend(self.replies.get(&target).cloned().unwrap_or_default());
            }
            Body::Edit { text, .. } => {
                // A link's text is the game's title: not to be edited.
                let is_text = matches!(item.event.body, Body::Text { game: None, .. });
                let newer = item.edit.as_ref().is_none_or(|(seq, _)| *seq < event.seq);
                if !author || !is_text || !newer {
                    return;
                }
                item.edit = Some((event.seq, text));
                // Replies quote it.
                changed.extend(self.replies.get(&target).cloned().unwrap_or_default());
            }
            Body::PollOption { choice, .. } => {
                let open = matches!(item.event.body, Body::Poll { open: true, .. });
                let Some(poll) = item.poll.as_mut() else {
                    return;
                };
                if !open && !author {
                    return;
                }
                poll.added
                    .push((event.id.clone(), choice, event.nick.clone(), event.ts));
            }
            Body::Vote { choices, .. } => {
                let Some(poll) = item.poll.as_mut() else {
                    return;
                };
                let newer = poll
                    .votes
                    .get(&event.from)
                    .is_none_or(|(seq, _, _)| *seq < event.seq);
                if newer {
                    poll.votes
                        .insert(event.from.clone(), (event.seq, choices, event.nick.clone()));
                }
            }
            Body::ClosePoll { .. } => {
                let Some(poll) = item.poll.as_mut() else {
                    return;
                };
                if !author {
                    return;
                }
                poll.closed = Some(poll.closed.map_or(event.ts, |t| t.min(event.ts)));
            }
            Body::Text { .. } | Body::Poll { .. } | Body::Topic { .. } | Body::Sealed { .. } => {}
        }
        changed.push(target);
    }

    /// The conversation of an item, if it exists (for routing what refers to it).
    pub fn item_conversation(&self, id: &str) -> Option<Option<String>> {
        self.items.get(id).map(|i| i.event.conversation(&self.me))
    }

    pub fn item_author(&self, id: &str) -> Option<&str> {
        self.items.get(id).map(|i| i.event.from.as_str())
    }

    /// `id` opened a topic that is still there.
    pub fn is_topic(&self, id: &str) -> bool {
        self.items
            .get(id)
            .is_some_and(|i| matches!(i.event.body, Body::Topic { .. }) && !i.deleted)
    }

    /// `id` is a text message (one that can be edited).
    pub fn is_text(&self, id: &str) -> bool {
        self.items
            .get(id)
            .is_some_and(|i| matches!(i.event.body, Body::Text { game: None, .. }) && !i.deleted)
    }

    pub fn is_deleted(&self, id: &str) -> bool {
        self.items.get(id).is_some_and(|i| i.deleted)
    }

    /// For a poll: (closed, open for additions, kind, option ids). `None`
    /// when `id` is no poll.
    pub fn poll_info(&self, id: &str) -> Option<(bool, bool, PollKind, Vec<String>)> {
        let item = self.items.get(id)?;
        let poll = item.poll.as_ref()?;
        let Body::Poll { open, kind, .. } = &item.event.body else {
            return None;
        };
        Some((poll.closed.is_some(), *open, *kind, option_ids(item, poll)))
    }

    pub fn view(&self, id: &str) -> Option<ItemView> {
        let item = self.items.get(id)?;
        let e = &item.event;
        let reply = match &e.body {
            Body::Text {
                reply_to: Some(r), ..
            } => Some(match self.items.get(r) {
                Some(target) => ReplyView {
                    id: r.clone(),
                    nick: Some(target.event.nick.clone()),
                    text: (!target.deleted).then(|| snippet(target)),
                    deleted: target.deleted,
                },
                None => ReplyView {
                    id: r.clone(),
                    nick: None,
                    text: None,
                    deleted: false,
                },
            }),
            _ => None,
        };
        let text = if item.deleted {
            None
        } else {
            item.text().map(str::to_string)
        };
        let game = match &e.body {
            Body::Text { game, .. } if !item.deleted => game.clone(),
            _ => None,
        };
        let topic_name = match &e.body {
            Body::Topic { name } if !item.deleted => Some(name.clone()),
            _ => None,
        };
        let poll = match (&e.body, &item.poll) {
            (
                Body::Poll {
                    question,
                    kind,
                    open,
                    ..
                },
                Some(state),
            ) if !item.deleted => Some(poll_view(&self.me, item, state, question, *kind, *open)),
            _ => None,
        };
        let reactions = if item.deleted {
            Vec::new()
        } else {
            reaction_views(&self.me, &item.reactions)
        };
        Some(ItemView {
            id: e.id.clone(),
            from: e.from.clone(),
            nick: e.nick.clone(),
            ts: e.ts.min(item.seen.max(0)).max(0),
            received: item.seen,
            conversation: e.conversation(&self.me),
            mine: e.from == self.me,
            deleted: item.deleted,
            text,
            edited: item.edit.is_some() && !item.deleted,
            topic_name,
            game,
            reply,
            reactions,
            poll,
        })
    }

    pub fn views(&self) -> Vec<ItemView> {
        let mut out: Vec<ItemView> = self.items.keys().filter_map(|id| self.view(id)).collect();
        out.sort_by(|a, b| a.ts.cmp(&b.ts).then_with(|| a.id.cmp(&b.id)));
        out
    }

    /// Ids of the events `peer` may see: everything public and its private
    /// conversation with this launcher — or, for a relay (`all`), everything,
    /// since it keeps private events sealed. What it lacks is sent to it.
    pub fn ids_for(&self, peer: &str, all: bool) -> Vec<String> {
        self.events
            .iter()
            .filter(|e| all || e.plain.visible_to(peer))
            .map(|e| e.plain.id.clone())
            .collect()
    }

    /// Events visible to `peer` that are not in `have` and not older than
    /// `since` (what the peer no longer keeps), in arrival order, as sent,
    /// with when they arrived here.
    pub fn missing_for(
        &self,
        peer: &str,
        have: &HashSet<String>,
        since: i64,
        all: bool,
    ) -> Vec<Stored> {
        let sending: Vec<Stored> = self
            .events
            .iter()
            .filter(|e| {
                (all || e.plain.visible_to(peer))
                    && e.plain.ts >= since
                    && !have.contains(&e.plain.id)
            })
            .map(Entry::stored)
            .collect();
        self.with_topic_openers(sending, have)
    }

    /// Put in front the openers of topics these events are in, should the
    /// peer lack them: they may be older than what it asked for.
    fn with_topic_openers(&self, mut sending: Vec<Stored>, have: &HashSet<String>) -> Vec<Stored> {
        let ids: HashSet<&str> = sending.iter().map(|s| s.event.id.as_str()).collect();
        let topics: HashSet<&str> = sending
            .iter()
            .filter_map(|s| s.event.topic.as_deref())
            .collect();
        let mut out: Vec<Stored> = self
            .events
            .iter()
            .filter(|e| {
                topics.contains(e.plain.id.as_str())
                    && !have.contains(&e.plain.id)
                    && !ids.contains(e.plain.id.as_str())
            })
            .map(Entry::stored)
            .collect();
        out.append(&mut sending);
        out
    }
}

/// Keep the event that opened a topic as long as anything in that topic is
/// kept: without it the topic has no name and cannot be written to.
/// Returns (kept, really dropped).
fn keep_topic_openers(mut kept: Vec<Entry>, dropped: Vec<Entry>) -> (Vec<Entry>, Vec<Entry>) {
    let used: HashSet<String> = kept.iter().filter_map(|e| e.plain.topic.clone()).collect();
    let mut gone = Vec::new();
    for e in dropped {
        if matches!(e.plain.body, Body::Topic { .. }) && used.contains(&e.plain.id) {
            kept.push(e);
        } else {
            gone.push(e);
        }
    }
    (kept, gone)
}

fn option_ids(item: &Item, poll: &PollState) -> Vec<String> {
    let base = match &item.event.body {
        Body::Poll { options, .. } => options.len(),
        _ => 0,
    };
    (0..base)
        .map(|i| i.to_string())
        .chain(poll.counted(base).into_iter().map(|(id, ..)| id.clone()))
        .collect()
}

fn poll_view(
    me: &str,
    item: &Item,
    state: &PollState,
    question: &str,
    kind: PollKind,
    open: bool,
) -> PollView {
    let base: Vec<(String, PollChoice, Option<String>)> = match &item.event.body {
        Body::Poll { options, .. } => options
            .iter()
            .enumerate()
            .map(|(i, c)| (i.to_string(), c.clone(), None))
            .collect(),
        _ => Vec::new(),
    };
    let counted = state.counted(base.len());
    let all: Vec<(String, PollChoice, Option<String>)> = base
        .into_iter()
        .chain(
            counted
                .into_iter()
                .map(|(id, c, nick, _)| (id.clone(), c.clone(), Some(nick.clone()))),
        )
        .collect();
    let known: HashSet<&str> = all.iter().map(|(id, _, _)| id.as_str()).collect();
    // Votes count only for options that exist; a single-choice poll takes
    // the first, so a malformed vote cannot count twice.
    let mut by_option: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
    let mut participants = 0;
    let mut voters: Vec<(&String, &Vote)> = state.votes.iter().collect();
    voters.sort_by_key(|(_, (_, _, nick))| nick.to_lowercase());
    for (peer, (_, choices, nick)) in voters {
        let mut seen = HashSet::new();
        let mut counted = false;
        for c in choices {
            if !known.contains(c.as_str()) || !seen.insert(c.as_str()) {
                continue;
            }
            by_option
                .entry(c.as_str())
                .or_default()
                .push((peer.as_str(), nick.as_str()));
            counted = true;
            if kind == PollKind::Single {
                break;
            }
        }
        if counted {
            participants += 1;
        }
    }
    let options = all
        .into_iter()
        .map(|(id, choice, added_by)| {
            let votes = by_option.get(id.as_str()).cloned().unwrap_or_default();
            OptionView {
                mine: votes.iter().any(|(p, _)| *p == me),
                voters: votes.iter().map(|(_, n)| n.to_string()).collect(),
                id,
                text: choice.text,
                game: choice.game,
                added_by,
            }
        })
        .collect();
    PollView {
        question: question.to_string(),
        kind,
        open,
        closed: state.closed.is_some(),
        options,
        participants,
    }
}

fn reaction_views(me: &str, reactions: &HashMap<String, Reaction>) -> Vec<ReactionView> {
    // Emoji in the order they were first given, so chips do not jump around.
    let mut groups: BTreeMap<String, (u64, Vec<String>, bool)> = BTreeMap::new();
    let mut entries: Vec<(&String, &Reaction)> = reactions
        .iter()
        .filter(|(_, (_, e, _))| !e.is_empty())
        .collect();
    entries.sort_by_key(|(_, (seq, _, _))| *seq);
    for (peer, (seq, emoji, nick)) in entries {
        let g = groups
            .entry(emoji.clone())
            .or_insert((*seq, Vec::new(), false));
        g.1.push(nick.clone());
        g.2 |= peer == me;
    }
    let mut out: Vec<(u64, ReactionView)> = groups
        .into_iter()
        .map(|(emoji, (first, nicks, mine))| (first, ReactionView { emoji, nicks, mine }))
        .collect();
    out.sort_by_key(|(first, _)| *first);
    out.into_iter().map(|(_, v)| v).collect()
}

/// A short quote of a message for a reply, edits included.
fn snippet(item: &Item) -> String {
    let text = match (item.text(), &item.event.body) {
        (Some(text), _) => text,
        (None, Body::Poll { question, .. }) => question.as_str(),
        _ => "",
    };
    let one_line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > 120 {
        let cut: String = one_line.chars().take(119).collect();
        format!("{cut}…")
    } else {
        one_line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(from: &str, seq: u64, to: Option<&str>, body: Body) -> Event {
        Event {
            id: format!("{from}:{seq}"),
            from: from.into(),
            nick: from.to_uppercase(),
            seq,
            ts: seq as i64,
            to: to.map(String::from),
            body,
            topic: None,
            sig: String::new(),
        }
    }

    fn text(t: &str) -> Body {
        Body::Text {
            text: t.into(),
            reply_to: None,
            game: None,
        }
    }

    fn poll(kind: PollKind, open: bool) -> Body {
        Body::Poll {
            question: "Next game?".into(),
            options: vec![
                PollChoice {
                    text: "Quake".into(),
                    game: Some("q3".into()),
                },
                PollChoice {
                    text: "UT".into(),
                    game: None,
                },
            ],
            kind,
            open,
        }
    }

    #[test]
    fn duplicates_and_forgeries_are_rejected() {
        let mut s = ChatState::new("me");
        assert!(s.insert(ev("a", 1, None, text("hi")), 1).is_some());
        assert!(s.insert(ev("a", 1, None, text("hi")), 1).is_none());
        let mut forged = ev("a", 2, None, text("x"));
        forged.id = "b:2".into();
        assert!(s.insert(forged, 2).is_none());
        assert!(s.insert(ev("a", 3, None, text("   ")), 3).is_none());
        let long = "x".repeat(MAX_TEXT + 1);
        assert!(s.insert(ev("a", 4, None, text(&long)), 4).is_none());
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn private_messages_of_others_are_not_taken_in() {
        let mut s = ChatState::new("me");
        assert!(s.insert(ev("a", 1, Some("b"), text("psst")), 1).is_none());
        assert!(s.insert(ev("a", 2, Some("me"), text("psst")), 2).is_some());
        let v = s.view("a:2").unwrap();
        assert_eq!(v.conversation.as_deref(), Some("a"));
        assert!(s.ids_for("c", false).is_empty());
        assert_eq!(s.ids_for("a", false), vec!["a:2".to_string()]);
    }

    #[test]
    fn one_reaction_per_person_and_the_newest_wins() {
        let mut s = ChatState::new("me");
        s.insert(ev("a", 1, None, text("gg")), 1);
        let react = |from: &str, seq, emoji: &str| {
            ev(
                from,
                seq,
                None,
                Body::React {
                    target: "a:1".into(),
                    emoji: emoji.into(),
                },
            )
        };
        assert_eq!(s.insert(react("me", 5, "👍"), 5), Some(vec!["a:1".into()]));
        s.insert(react("b", 6, "👍"), 6);
        s.insert(react("me", 7, "🔥"), 7);
        // An older event arriving late does not undo a newer one.
        s.insert(react("me", 4, "😢"), 8);
        let v = s.view("a:1").unwrap();
        assert_eq!(v.reactions.len(), 2);
        assert_eq!(v.reactions[0].emoji, "👍");
        assert_eq!(v.reactions[0].nicks, vec!["B"]);
        assert!(!v.reactions[0].mine);
        assert_eq!(v.reactions[1].emoji, "🔥");
        assert!(v.reactions[1].mine);
        s.insert(react("me", 9, ""), 9);
        assert_eq!(s.view("a:1").unwrap().reactions.len(), 1);
    }

    #[test]
    fn reactions_arriving_before_their_message_are_kept() {
        let mut s = ChatState::new("me");
        let react = ev(
            "b",
            2,
            None,
            Body::React {
                target: "a:1".into(),
                emoji: "❤️".into(),
            },
        );
        assert_eq!(s.insert(react, 2), Some(vec![]));
        s.insert(ev("a", 1, None, text("late")), 3);
        assert_eq!(s.view("a:1").unwrap().reactions[0].emoji, "❤️");
    }

    #[test]
    fn a_reaction_cannot_leave_its_private_conversation() {
        let mut s = ChatState::new("me");
        s.insert(ev("a", 1, Some("me"), text("secret")), 1);
        let public = ev(
            "a",
            2,
            None,
            Body::React {
                target: "a:1".into(),
                emoji: "👍".into(),
            },
        );
        s.insert(public, 2);
        assert!(s.view("a:1").unwrap().reactions.is_empty());
    }

    #[test]
    fn replies_quote_their_target_and_follow_its_deletion() {
        let mut s = ChatState::new("me");
        let reply = ev(
            "b",
            2,
            None,
            Body::Text {
                text: "yes".into(),
                reply_to: Some("a:1".into()),
                game: None,
            },
        );
        s.insert(reply, 2);
        assert_eq!(s.view("b:2").unwrap().reply.unwrap().nick, None);
        let changed = s.insert(ev("a", 1, None, text("Pizza?")), 3).unwrap();
        assert_eq!(changed, vec!["a:1".to_string(), "b:2".to_string()]);
        let quote = s.view("b:2").unwrap().reply.unwrap();
        assert_eq!(quote.nick.as_deref(), Some("A"));
        assert_eq!(quote.text.as_deref(), Some("Pizza?"));
        // Only the author can delete.
        let del = |from: &str, seq| {
            ev(
                from,
                seq,
                None,
                Body::Delete {
                    target: "a:1".into(),
                },
            )
        };
        s.insert(del("b", 4), 4);
        assert!(!s.view("a:1").unwrap().deleted);
        let changed = s.insert(del("a", 5), 5).unwrap();
        assert_eq!(changed, vec!["a:1".to_string(), "b:2".to_string()]);
        let v = s.view("a:1").unwrap();
        assert!(v.deleted && v.text.is_none());
        assert!(s.view("b:2").unwrap().reply.unwrap().deleted);
    }

    #[test]
    fn single_choice_polls_count_one_answer_and_the_latest_vote() {
        let mut s = ChatState::new("me");
        s.insert(ev("a", 1, None, poll(PollKind::Single, false)), 1);
        let vote = |from: &str, seq, choices: &[&str]| {
            ev(
                from,
                seq,
                None,
                Body::Vote {
                    poll: "a:1".into(),
                    choices: choices.iter().map(|c| c.to_string()).collect(),
                },
            )
        };
        s.insert(vote("me", 2, &["0", "1"]), 2);
        s.insert(vote("b", 3, &["1"]), 3);
        s.insert(vote("b", 4, &["0"]), 4);
        s.insert(vote("c", 5, &["nonsense"]), 5);
        let p = s.view("a:1").unwrap().poll.unwrap();
        assert_eq!(p.options[0].voters, vec!["B", "ME"]);
        assert!(p.options[0].mine);
        assert!(p.options[1].voters.is_empty());
        assert_eq!(p.participants, 2);
        assert_eq!(p.options[0].game.as_deref(), Some("q3"));
    }

    #[test]
    fn multiple_choice_and_open_polls() {
        let mut s = ChatState::new("me");
        s.insert(ev("a", 1, None, poll(PollKind::Multiple, true)), 1);
        let add = ev(
            "b",
            2,
            None,
            Body::PollOption {
                poll: "a:1".into(),
                choice: PollChoice {
                    text: "CS 1.6".into(),
                    game: None,
                },
            },
        );
        s.insert(add, 2);
        s.insert(
            ev(
                "me",
                3,
                None,
                Body::Vote {
                    poll: "a:1".into(),
                    choices: vec!["0".into(), "b:2".into()],
                },
            ),
            3,
        );
        let p = s.view("a:1").unwrap().poll.unwrap();
        assert_eq!(p.options.len(), 3);
        assert_eq!(p.options[2].added_by.as_deref(), Some("B"));
        assert!(p.options[0].mine && p.options[2].mine && !p.options[1].mine);
        assert_eq!(p.participants, 1);
    }

    #[test]
    fn closed_polls_take_no_new_options_and_only_the_author_closes() {
        let mut s = ChatState::new("me");
        s.insert(ev("a", 1, None, poll(PollKind::Single, false)), 1);
        let add = |from: &str, seq| {
            ev(
                from,
                seq,
                None,
                Body::PollOption {
                    poll: "a:1".into(),
                    choice: PollChoice {
                        text: "more".into(),
                        game: None,
                    },
                },
            )
        };
        // Not open: only the author adds.
        s.insert(add("b", 2), 2);
        assert_eq!(s.view("a:1").unwrap().poll.unwrap().options.len(), 2);
        s.insert(add("a", 3), 3);
        assert_eq!(s.view("a:1").unwrap().poll.unwrap().options.len(), 3);
        let close = |from: &str, seq| ev(from, seq, None, Body::ClosePoll { poll: "a:1".into() });
        s.insert(close("b", 4), 4);
        assert!(!s.view("a:1").unwrap().poll.unwrap().closed);
        s.insert(close("a", 5), 5);
        assert!(s.view("a:1").unwrap().poll.unwrap().closed);
        s.insert(add("a", 6), 6);
        assert_eq!(s.view("a:1").unwrap().poll.unwrap().options.len(), 3);
        let (closed, open, kind, ids) = s.poll_info("a:1").unwrap();
        assert!(closed && !open && kind == PollKind::Single);
        assert_eq!(ids, vec!["0", "1", "a:3"]);
    }

    #[test]
    fn added_options_do_not_depend_on_the_order_of_arrival() {
        let option = |from: &str, seq| {
            ev(
                from,
                seq,
                None,
                Body::PollOption {
                    poll: "a:1".into(),
                    choice: PollChoice {
                        text: format!("opt {seq}"),
                        game: None,
                    },
                },
            )
        };
        let events = [
            ev("a", 1, None, poll(PollKind::Single, true)),
            option("b", 2),
            ev("a", 3, None, Body::ClosePoll { poll: "a:1".into() }),
            option("c", 4),
        ];
        let ids = |order: &[usize]| {
            let mut s = ChatState::new("me");
            for (i, n) in order.iter().enumerate() {
                s.insert(events[*n].clone(), i as i64);
            }
            s.poll_info("a:1").unwrap().3
        };
        let expected = vec!["0".to_string(), "1".into(), "b:2".into()];
        assert_eq!(ids(&[0, 1, 2, 3]), expected);
        assert_eq!(ids(&[0, 2, 3, 1]), expected, "close before the option");
        assert_eq!(ids(&[3, 2, 1, 0]), expected, "everything before the poll");
    }

    #[test]
    fn a_clock_running_ahead_does_not_put_a_message_at_the_bottom_forever() {
        let mut s = ChatState::new("me");
        let mut e = ev("a", 1, None, text("from the future"));
        e.ts = 10_000;
        s.insert(e, 500);
        assert_eq!(s.view("a:1").unwrap().ts, 500);
    }

    #[test]
    fn sync_sends_only_what_the_peer_may_see_and_lacks() {
        let mut s = ChatState::new("me");
        s.insert(ev("a", 1, None, text("public")), 1);
        s.insert(ev("me", 2, Some("a"), text("to a")), 2);
        s.insert(ev("me", 3, Some("b"), text("to b")), 3);
        let have: HashSet<String> = ["a:1".to_string()].into();
        let missing: Vec<String> = s
            .missing_for("a", &have, 0, false)
            .into_iter()
            .map(|e| e.event.id)
            .collect();
        assert_eq!(missing, vec!["me:2"]);
        assert_eq!(s.last_own_seq(), 3);
    }

    #[test]
    fn events_survive_a_round_trip_through_json() {
        let e = ev("a", 1, None, poll(PollKind::Multiple, true));
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), e);
        let opt = ev(
            "a",
            2,
            Some("b"),
            Body::PollOption {
                poll: "a:1".into(),
                choice: PollChoice {
                    text: "x".into(),
                    game: Some("g".into()),
                },
            },
        );
        let json = serde_json::to_string(&opt).unwrap();
        assert!(json.contains(r#""type":"poll_option""#), "{json}");
        assert_eq!(serde_json::from_str::<Event>(&json).unwrap(), opt);
    }

    #[test]
    fn trimming_keeps_the_newest_and_refuses_what_fell_out() {
        let mut s = ChatState::new("me");
        s.insert(ev("a", 1, None, text("old")), 1);
        s.insert(ev("a", 2, None, text("mid")), 2);
        let react = Body::React {
            target: "a:2".into(),
            emoji: "👍".into(),
        };
        s.insert(ev("b", 3, None, react), 3);
        assert!(s.trim(2));
        assert_eq!(s.floor(), 2);
        assert!(s.view("a:1").is_none());
        assert_eq!(s.view("a:2").unwrap().reactions.len(), 1);
        // A peer sending the trimmed message again does not bring it back.
        assert!(s.insert(ev("a", 1, None, text("old")), 4).is_none());
        assert_eq!(s.missing_for("x", &HashSet::new(), 3, false).len(), 1);
    }

    #[test]
    fn a_relay_keeps_sealed_private_events_it_cannot_read() {
        let mut relay = ChatState::new("relay");
        let mut sealed = ev("a", 1, Some("b"), text("x"));
        sealed.body = Body::Sealed {
            nonce: "0".repeat(48),
            data: "ab".repeat(40),
        };
        assert!(relay.insert_opaque(sealed.clone(), 1));
        assert!(!relay.insert_opaque(sealed.clone(), 2), "duplicate");
        // Readable bodies of private events, or sealed public ones, are not
        // what a launcher sends.
        assert!(!relay.insert_opaque(ev("a", 2, Some("b"), text("x")), 3));
        let mut open_sealed = sealed.clone();
        open_sealed.to = None;
        open_sealed.id = "a:3".into();
        open_sealed.seq = 3;
        assert!(!relay.insert_opaque(open_sealed, 4));
        assert!(relay.insert_opaque(ev("c", 4, None, text("hi")), 5));
        assert!(relay.views().is_empty(), "nothing folded");
        // Bob gets both; Carol only the public one; another relay all.
        let none = HashSet::new();
        assert_eq!(relay.missing_for("b", &none, 0, false).len(), 2);
        assert_eq!(relay.missing_for("c", &none, 0, false).len(), 1);
        assert_eq!(relay.ids_for("r2", true).len(), 2);
        assert!(relay.trim(1));
        assert_eq!(relay.len(), 1);
    }

    #[test]
    fn expired_messages_go_and_do_not_come_back() {
        let mut s = ChatState::new("me");
        // Arrival decides, not the author's clock: "a:1" claims to be new
        // but arrived long ago; "b:2" comes from a PC whose clock is years
        // behind but arrived just now.
        s.insert(ev("a", 1_000, None, text("last LAN")), 1);
        s.insert(ev("b", 2, None, text("wrong clock")), 100);
        assert!(!s.expire(50, 60), "nothing older than the window");
        assert!(s.expire(100, 60));
        assert!(s.view("a:1000").is_none());
        assert!(s.view("b:2").is_some());
        // Someone who got it later hands it back: refused.
        assert!(s
            .insert(ev("a", 1_000, None, text("last LAN")), 101)
            .is_none());
        // A clock that went back does not keep a message forever.
        let mut back = ChatState::new("me");
        back.insert(ev("c", 1, None, text("x")), 1_000);
        assert!(
            !back.expire(500, 60),
            "arrival in the future: pulled to now"
        );
        assert!(back.expire(560, 60));
        // Once everybody has let it go, the id is forgotten too.
        s.expire(200, 60);
        assert!(!s.gone().contains_key("a:1000"));
        assert!(s.gone().contains_key("b:2"), "expired in this round");
    }

    #[test]
    fn only_the_author_edits_and_the_newest_edit_counts() {
        let mut s = ChatState::new("me");
        s.insert(ev("a", 1, None, text("Pizza um 8")), 1);
        let reply = Body::Text {
            text: "ok".into(),
            reply_to: Some("a:1".into()),
            game: None,
        };
        s.insert(ev("b", 2, None, reply), 2);
        let edit = |from: &str, seq, t: &str| {
            ev(
                from,
                seq,
                None,
                Body::Edit {
                    target: "a:1".into(),
                    text: t.into(),
                },
            )
        };
        assert_eq!(
            s.insert(edit("b", 3, "hacked"), 3),
            Some(vec![]),
            "taken in, but it changes nothing"
        );
        assert_eq!(s.view("a:1").unwrap().text.as_deref(), Some("Pizza um 8"));
        let changed = s.insert(edit("a", 5, "Pizza um 9"), 5).unwrap();
        assert_eq!(
            changed,
            vec!["a:1".to_string(), "b:2".to_string()],
            "the reply quotes it"
        );
        s.insert(edit("a", 4, "older edit"), 6);
        let v = s.view("a:1").unwrap();
        assert_eq!(v.text.as_deref(), Some("Pizza um 9"));
        assert!(v.edited);
        assert_eq!(
            s.view("b:2").unwrap().reply.unwrap().text.as_deref(),
            Some("Pizza um 9")
        );
    }

    #[test]
    fn topics_are_public_rooms_of_their_own() {
        let mut s = ChatState::new("me");
        s.insert(
            ev(
                "a",
                1,
                None,
                Body::Topic {
                    name: "CS-Turnier".into(),
                },
            ),
            1,
        );
        let topic = s.view("a:1").unwrap();
        assert_eq!(topic.conversation.as_deref(), Some("#a:1"));
        assert_eq!(topic.topic_name.as_deref(), Some("CS-Turnier"));
        assert!(s.is_topic("a:1"));
        let mut msg = ev("b", 2, None, text("wer spielt mit?"));
        msg.topic = Some("a:1".into());
        s.insert(msg, 2);
        assert_eq!(s.view("b:2").unwrap().conversation.as_deref(), Some("#a:1"));
        // A reaction claiming the public room does not reach it.
        s.insert(
            ev(
                "c",
                3,
                None,
                Body::React {
                    target: "b:2".into(),
                    emoji: "👍".into(),
                },
            ),
            3,
        );
        assert!(s.view("b:2").unwrap().reactions.is_empty());
        // Topics are never private, and never nested.
        let mut private = ev("b", 4, Some("me"), text("x"));
        private.topic = Some("a:1".into());
        assert!(!private.is_valid());
        let mut nested = ev("b", 5, None, Body::Topic { name: "x".into() });
        nested.topic = Some("a:1".into());
        assert!(!nested.is_valid());
        assert!(!ev(
            "b",
            6,
            None,
            Body::Topic {
                name: "x".repeat(MAX_TOPIC + 1)
            }
        )
        .is_valid());
    }

    #[test]
    fn a_topic_keeps_its_opener_while_anything_in_it_is_kept() {
        let opener = || ev("a", 1, None, Body::Topic { name: "LAN".into() });
        let in_topic = |from: &str, seq| {
            let mut e = ev(from, seq, None, text("x"));
            e.topic = Some("a:1".into());
            e
        };
        let mut s = ChatState::new("me");
        s.insert(opener(), 1);
        s.insert(ev("b", 2, None, text("public")), 2);
        s.insert(in_topic("c", 3), 3);
        assert!(s.trim(1));
        assert!(s.is_topic("a:1"), "trimmed with the old public message");
        assert!(s.view("b:2").is_none());
        // A late joiner asking only for the newest gets the opener first.
        let sent: Vec<String> = s
            .missing_for("x", &HashSet::new(), 3, false)
            .into_iter()
            .map(|e| e.event.id)
            .collect();
        assert_eq!(sent, vec!["a:1", "c:3"]);
        // ... and takes it in although it is older than its own floor.
        let mut late = ChatState::new("x");
        late.insert(ev("d", 9, None, text("y")), 9);
        late.insert(in_topic("e", 10), 10);
        assert!(late.trim(1));
        assert!(late.insert(opener(), 11).is_some());
        assert!(late.is_topic("a:1"));
        // Expiry keeps it as long as the topic has messages, then lets it go.
        let mut e = ChatState::new("me");
        e.insert(opener(), 1);
        e.insert(in_topic("c", 3), 100);
        assert!(!e.expire(100, 60), "keeping the opener is no change");
        assert!(e.is_topic("a:1"));
        assert!(!e.gone().contains_key("a:1"));
        assert!(e.expire(200, 60));
        assert!(!e.is_topic("a:1"));
        assert!(e.gone().contains_key("a:1"));
    }

    #[test]
    fn a_linked_game_shows_its_id_and_keeps_its_title() {
        let mut s = ChatState::new("me");
        let link = |seq, game: &str| {
            ev(
                "a",
                seq,
                None,
                Body::Text {
                    text: "Quake III Arena".into(),
                    reply_to: None,
                    game: Some(game.into()),
                },
            )
        };
        s.insert(link(1, "quake3"), 1);
        let v = s.view("a:1").unwrap();
        assert_eq!(v.game.as_deref(), Some("quake3"));
        assert_eq!(v.text.as_deref(), Some("Quake III Arena"));
        assert!(!s.is_text("a:1"), "a link is not edited");
        let edit = Body::Edit {
            target: "a:1".into(),
            text: "something else".into(),
        };
        s.insert(ev("a", 2, None, edit), 2);
        assert_eq!(
            s.view("a:1").unwrap().text.as_deref(),
            Some("Quake III Arena")
        );
        assert!(!link(3, "").is_valid());
        assert!(!link(3, "a\nb").is_valid());
        // A plain message travels as before: no `game` on the wire.
        let json = serde_json::to_string(&ev("a", 4, None, text("hi"))).unwrap();
        assert!(!json.contains("game"), "{json}");
    }

    #[test]
    fn nicknames_are_trimmed() {
        assert_eq!(clean_nick("  Bob\n "), "Bob");
        assert_eq!(clean_nick(&"x".repeat(50)).chars().count(), MAX_NICK);
    }
}
