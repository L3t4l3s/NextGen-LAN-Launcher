//! LAN chat between launchers, without a server.
//!
//! * **Presence:** every launcher broadcasts a small UDP beacon (nickname,
//!   TCP port) on [`CHAT_PORT`] every few seconds, to the broadcast address
//!   of each network card. Whoever was not heard from for a while is offline.
//! * **Messages:** go over TCP, one JSON frame per line, straight to the
//!   people concerned — everybody for the public room, one person for a
//!   private message. Nothing passes through a third machine.
//! * **History:** when two launchers meet (or meet again), each sends the
//!   other the ids of what it has and gets back what it lacks: public events,
//!   plus their private conversation. A late arrival thus reads what was said
//!   before, and a private message to someone offline arrives when they are
//!   back. [`model`] explains why merging that way is safe.
//! * **Connections** carry frames both ways: the answer to a `Hello` comes
//!   back on the connection it came in on, and a launcher that cannot reach
//!   a peer writes to it over a connection the peer opened. A firewall that
//!   lets only one of the two in thus still lets them catch up and talk.
//!
//! Nothing here is authenticated: anyone on the LAN can claim any nickname.
//! That is the trust model of a LAN party, and the interface does not claim
//! more.

pub mod crypto;
pub mod model;
pub mod store;

use model::{
    clean_nick, valid_peer_id, Body, ChatError, ChatState, Event, ItemView, PollChoice, PollKind,
    Stored, ERR_DISABLED, ERR_INVALID, ERR_NOT_ALLOWED, ERR_POLL_CLOSED, ERR_TOO_FAST,
    ERR_UNKNOWN_TARGET,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{broadcast, mpsc};
use tokio::task::AbortHandle;

/// UDP port of the beacons, and the TCP port tried first for messages.
pub const CHAT_PORT: u16 = 41950;
/// Version of the wire format in beacons; launchers ignore other versions.
const PROTOCOL: u8 = 1;
/// Longest frame accepted from the network.
const MAX_FRAME: usize = 1024 * 1024;
/// Events per frame stay below this many bytes when catching a peer up.
const BATCH_BYTES: usize = 256 * 1024;
/// How long the chat keeps what was said, counted from when this launcher got
/// it: a LAN weekend with room to spare, so the chat of the last LAN is gone
/// at the next one.
pub const KEEP_FOR: Duration = Duration::from_secs(5 * 24 * 3600);
/// How often old messages are looked for while the chat runs.
const EXPIRE_EVERY: Duration = Duration::from_secs(60);
/// Every so often the history is compared again, in case a message was lost
/// to a connection that broke while it was on its way.
const RESYNC_EVERY: Duration = Duration::from_secs(120);
/// A peer that could not be reached is tried directly again after this;
/// until then frames go over the connection it opened, if there is one.
const RETRY_DIRECT_AFTER: Duration = Duration::from_secs(30);
/// Flood limit: more than [`FLOOD_COUNT`] messages within [`FLOOD_WINDOW`]
/// and the sender has to pause for [`FLOOD_PAUSE`].
const FLOOD_COUNT: usize = 5;
const FLOOD_WINDOW: Duration = Duration::from_secs(10);
const FLOOD_PAUSE: Duration = Duration::from_secs(30);
/// Longest value of a [`PeerInfo`] field.
const MAX_INFO: usize = 100;
/// Longest game title a beacon carries.
const MAX_PLAYING: usize = 60;

#[derive(Debug, Clone)]
pub struct ChatConfig {
    /// UDP port of the beacons; TCP is tried on the same number first.
    pub port: u16,
    /// Address to listen on; `0.0.0.0` except in tests.
    pub bind: IpAddr,
    /// Where beacons go. `None`: the broadcast address of every network card.
    pub beacon_targets: Option<Vec<SocketAddr>>,
    pub beacon_every: Duration,
    /// Where history and identity are kept.
    pub data_dir: PathBuf,
    pub nick: String,
    /// Run as a relay (`nll-chat-relay`): no person, only a memory. It keeps
    /// every event — private ones sealed, as it cannot open them — and hands
    /// them to whoever starts later.
    pub relay: bool,
    /// Tests: name this TCP port to others instead of the real one, to play
    /// a launcher whose firewall lets nobody connect.
    #[doc(hidden)]
    pub advertise_port: Option<u16>,
    /// About this computer, from the first `Hello` on ([`Chat::set_info`]).
    pub info: Option<PeerInfo>,
}

impl ChatConfig {
    pub fn new(data_dir: PathBuf, nick: &str) -> Self {
        Self {
            port: CHAT_PORT,
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            beacon_targets: None,
            beacon_every: Duration::from_secs(3),
            data_dir,
            nick: nick.to_string(),
            relay: false,
            advertise_port: None,
            info: None,
        }
    }
}

/// Relay ids from the LANPage's `launcher.ini`, a `chat_relay { … }` block
/// with one or more ids (by line or comma): the relays trusted with private
/// messages. Anything that is not a peer id is ignored.
pub fn relays_from_launcher_ini(extra: &std::collections::BTreeMap<String, String>) -> Vec<String> {
    extra
        .get("chat_relay")
        .map(|v| {
            v.split(|c: char| c == ',' || c.is_whitespace())
                .map(str::trim)
                .filter(|id| id.len() == 64 && id.chars().all(|c| c.is_ascii_hexdigit()))
                .map(str::to_ascii_lowercase)
                .collect()
        })
        .unwrap_or_default()
}

/// A game title as a beacon carries it: one line, not too long.
fn clean_playing(title: &str) -> String {
    title
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_PLAYING)
        .collect()
}

/// The computer's name, for a launcher whose player has not given a name.
pub fn host_nick() -> String {
    let host = clean_nick(&gethostname::gethostname().to_string_lossy());
    if host.is_empty() {
        "Player".to_string()
    } else {
        host
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Beacon {
    nll: u8,
    id: String,
    nick: String,
    port: u16,
    #[serde(default)]
    os: String,
    /// Sent when the chat shuts down, so the others need not wait for the timeout.
    #[serde(default)]
    bye: bool,
    /// A relay, not a person ([`ChatConfig::relay`]).
    #[serde(default)]
    relay: bool,
    /// The game the player has running, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    playing: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Frame {
    /// "This is what I have": the receiver answers with what is missing.
    Hello {
        from: String,
        nick: String,
        port: u16,
        os: String,
        have: Vec<String>,
        /// The sender keeps nothing older than this (author time); 0 when
        /// it never trimmed. Older events are not sent to it.
        #[serde(default)]
        since: i64,
        #[serde(default)]
        relay: bool,
        /// The sender reads what comes back on connections it opened, so
        /// the answer may take this one. Launchers before this only wrote.
        #[serde(default)]
        duplex: bool,
        /// This `Hello` answers one: it gets the missing events back, not
        /// another `Hello`.
        #[serde(default)]
        answer: bool,
        /// About the sender's computer, for the list of people.
        #[serde(default)]
        info: Option<PeerInfo>,
    },
    /// Parsed one by one: an event of a newer version must not take the
    /// others in the frame down with it.
    Events {
        events: Vec<serde_json::Value>,
        /// How long the sender has had each event, in milliseconds. A
        /// duration, so neither clock needs to be right: the receiver counts
        /// the five days from when the event first reached the LAN, not
        /// from when it caught up on it. Missing means just written.
        #[serde(default)]
        ages: Vec<i64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerView {
    pub id: String,
    pub nick: String,
    /// `windows`, `linux`, `macos` as the other launcher reports it.
    pub os: String,
    pub online: bool,
    pub address: String,
    /// A relay keeping the history, not a person to write to.
    pub relay: bool,
    /// The game the player has running, as their launcher reports it.
    pub playing: Option<String>,
    /// About their computer, once a `Hello` brought it.
    pub info: Option<PeerInfo>,
}

/// About a launcher's computer: what the stats beacon reports to the
/// LANPage too (`lanpage::StatsReport`), plus the launcher's version. Shown
/// when hovering a name in the list of people. Unsigned, like everything in
/// a `Hello`: a description, not a proof.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    #[serde(default)]
    pub host: String,
    /// Operating system with version, e.g. "Windows 11 (26100)".
    #[serde(default)]
    pub system: String,
    #[serde(default)]
    pub cpu: String,
    #[serde(default)]
    pub version: String,
}

impl PeerInfo {
    /// Each field one line of sane length.
    fn cleaned(self) -> Self {
        let clean = |s: String| -> String {
            s.trim()
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_INFO)
                .collect()
        };
        Self {
            host: clean(self.host),
            system: clean(self.system),
            cpu: clean(self.cpu),
            version: clean(self.version),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSnapshot {
    pub me: String,
    pub nick: String,
    pub items: Vec<ItemView>,
    pub peers: Vec<PeerView>,
    /// `err.chat_*` code when the chat cannot reach the LAN as it should.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatUpdate {
    /// New or changed messages and polls.
    Items { items: Vec<ItemView> },
    /// The complete list of known people.
    Peers { peers: Vec<PeerView> },
    /// The history was trimmed: fetch the snapshot again.
    Reset,
}

struct Peer {
    nick: String,
    os: String,
    /// TCP address for messages.
    addr: SocketAddr,
    /// Where its beacons come from, for answering one directly.
    beacon_from: Option<SocketAddr>,
    /// Last beacon from `addr`'s IP. A PC on cable and Wi-Fi beacons from
    /// both; the address only moves when the current one has gone quiet.
    addr_seen: Instant,
    last_seen: Instant,
    online: bool,
    last_hello: Instant,
    relay: bool,
    playing: Option<String>,
    info: Option<PeerInfo>,
}

/// Counts what this launcher writes, so nobody floods the chat: past
/// [`FLOOD_COUNT`] messages in [`FLOOD_WINDOW`], writing pauses for
/// [`FLOOD_PAUSE`]. Kept by the sender: a modified launcher could skip it,
/// which at a private LAN is not the problem this solves.
#[derive(Default)]
struct Flood {
    sent: std::collections::VecDeque<Instant>,
    until: Option<Instant>,
}

impl Flood {
    /// Note one message at `now`; `Err` with the time left while paused.
    fn check(&mut self, now: Instant) -> Result<(), Duration> {
        if let Some(until) = self.until {
            if now < until {
                return Err(until - now);
            }
            self.until = None;
        }
        while self
            .sent
            .front()
            .is_some_and(|t| now.duration_since(*t) >= FLOOD_WINDOW)
        {
            self.sent.pop_front();
        }
        if self.sent.len() >= FLOOD_COUNT {
            self.sent.clear();
            self.until = Some(now + FLOOD_PAUSE);
            return Err(FLOOD_PAUSE);
        }
        self.sent.push_back(now);
        Ok(())
    }

    fn left(&self, now: Instant) -> Option<Duration> {
        self.until.filter(|u| now < *u).map(|u| u - now)
    }
}

struct Inner {
    relay: bool,
    data_dir: PathBuf,
    /// Relays the LANPage names: only these get private events (sealed).
    /// Any machine can call itself a relay, and even sealed, a private
    /// message tells who wrote to whom and when.
    trusted_relays: Mutex<HashSet<String>>,
    keys: crypto::Keys,
    me: String,
    os: String,
    nick: Mutex<String>,
    playing: Mutex<Option<String>>,
    info: Mutex<Option<PeerInfo>>,
    flood: Mutex<Flood>,
    state: Mutex<ChatState>,
    history: Mutex<store::History>,
    peers: Mutex<HashMap<String, Peer>>,
    links: Mutex<HashMap<String, mpsc::UnboundedSender<Arc<str>>>>,
    /// Connections peers opened to us, by peer: the way to them when they
    /// cannot be reached directly. Weak, so a closed one is gone.
    back_links: Mutex<HashMap<String, mpsc::WeakUnboundedSender<Arc<str>>>>,
    /// A beacon of another launcher arrived: the firewall lets the chat in.
    heard_beacon: AtomicBool,
    /// This, for the tasks a connection starts.
    this: Weak<Inner>,
    updates: broadcast::Sender<ChatUpdate>,
    seq: Mutex<u64>,
    tcp_port: u16,
    udp: Option<Arc<UdpSocket>>,
    udp_port: u16,
    targets: Option<Vec<SocketAddr>>,
    offline_after: Duration,
    problem: Option<String>,
    tasks: Mutex<Vec<AbortHandle>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A running chat. Cheap to clone; [`Chat::stop`] ends it.
#[derive(Clone)]
pub struct Chat {
    inner: Arc<Inner>,
}

fn bind_udp(bind: IpAddr, port: u16) -> std::io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    // A second launcher on the same machine (a test, a dev build next to the
    // installed one) shares the port instead of losing the chat.
    socket.set_reuse_address(true)?;
    #[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos"))))]
    socket.set_reuse_port(true)?;
    socket.set_broadcast(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddr::new(bind, port).into())?;
    UdpSocket::from_std(socket.into())
}

/// The broadcast address of every IPv4 network card, plus the general one.
/// Sending only to 255.255.255.255 leaves through one card on most systems,
/// which on a PC with a VPN or a virtual switch is the wrong one.
fn broadcast_targets(port: u16) -> Vec<SocketAddr> {
    let mut out: Vec<SocketAddr> = Vec::new();
    for iface in if_addrs::get_if_addrs().unwrap_or_default() {
        if iface.is_loopback() {
            continue;
        }
        if let if_addrs::IfAddr::V4(v4) = &iface.addr {
            let broadcast = v4
                .broadcast
                .unwrap_or_else(|| Ipv4Addr::from(u32::from(v4.ip) | !u32::from(v4.netmask)));
            out.push(SocketAddr::new(IpAddr::V4(broadcast), port));
        }
    }
    out.push(SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), port));
    out.sort();
    out.dedup();
    out
}

impl Chat {
    pub async fn start(config: ChatConfig) -> std::io::Result<Chat> {
        std::fs::create_dir_all(&config.data_dir)?;
        let keys = store::load_keys(&config.data_dir)?;
        let me = keys.id().to_string();
        let (history, stored, damaged) = store::History::open(&config.data_dir);
        let mut state = ChatState::new(&me);
        let mut dropped = damaged;
        for s in stored {
            let (id, born) = (s.event.id.clone(), s.born());
            // A history from before signing, or one edited by hand.
            let kept = if config.relay {
                crypto::verify(&s.event) && state.insert_opaque(s.event, s.seen)
            } else {
                match keys.open(&s.event) {
                    Some(plain) => state.insert_opened(plain, s.event, s.seen).is_some(),
                    None => false,
                }
            };
            if kept {
                state.set_born(&id, born);
            }
            dropped |= !kept;
        }
        state.set_gone(store::load_gone(&config.data_dir));
        let expired = state.expire(now_ms(), KEEP_FOR.as_millis() as i64);
        if expired {
            store::save_gone(&config.data_dir, state.gone());
        }
        if state.trim(store::HISTORY_LIMIT) || dropped || expired {
            history.rewrite(&state.stored());
        }
        let seq = state.last_own_seq();
        let listener = match TcpListener::bind(SocketAddr::new(config.bind, config.port)).await {
            Ok(l) => l,
            Err(_) => TcpListener::bind(SocketAddr::new(config.bind, 0)).await?,
        };
        let tcp_port = listener.local_addr()?.port();
        let (udp, problem) = match bind_udp(config.bind, config.port) {
            Ok(s) => (Some(Arc::new(s)), None),
            Err(e) => {
                log::warn!("chat: UDP port {} not usable: {e}", config.port);
                (None, Some(format!("err.chat_port_busy|{}", config.port)))
            }
        };
        let nick = match clean_nick(&config.nick) {
            n if n.is_empty() => host_nick(),
            n => n,
        };
        let (updates, _) = broadcast::channel(256);
        let inner = Arc::new_cyclic(|this| Inner {
            relay: config.relay,
            data_dir: config.data_dir.clone(),
            trusted_relays: Mutex::new(HashSet::new()),
            keys,
            me,
            os: std::env::consts::OS.to_string(),
            nick: Mutex::new(nick),
            playing: Mutex::new(None),
            info: Mutex::new(config.info.clone().map(PeerInfo::cleaned)),
            flood: Mutex::new(Flood::default()),
            state: Mutex::new(state),
            history: Mutex::new(history),
            peers: Mutex::new(HashMap::new()),
            links: Mutex::new(HashMap::new()),
            back_links: Mutex::new(HashMap::new()),
            heard_beacon: AtomicBool::new(false),
            this: this.clone(),
            updates,
            seq: Mutex::new(seq),
            tcp_port: config.advertise_port.unwrap_or(tcp_port),
            udp,
            udp_port: config.port,
            targets: config.beacon_targets.clone(),
            offline_after: config.beacon_every * 4,
            problem,
            tasks: Mutex::new(Vec::new()),
        });
        log::info!(
            "chat: peer {} listening on TCP {tcp_port}, beacons on UDP {}",
            inner.me,
            config.port
        );
        let i = inner.clone();
        spawn(&inner, async move { i.accept_loop(listener).await });
        if inner.udp.is_some() {
            let i = inner.clone();
            spawn(&inner, async move { i.beacon_receive_loop().await });
        }
        let i = inner.clone();
        let every = config.beacon_every;
        spawn(&inner, async move { i.beacon_loop(every).await });
        Ok(Chat { inner })
    }

    /// Say goodbye and end every task. Further calls are refused.
    pub async fn stop(&self) {
        self.inner.send_beacon(true).await;
        let tasks = std::mem::take(&mut *lock(&self.inner.tasks));
        for t in tasks {
            t.abort();
        }
        lock(&self.inner.links).clear();
        lock(&self.inner.back_links).clear();
    }

    /// Whether a beacon of another launcher has arrived since the start:
    /// proof that the firewall lets the chat in, whatever its rules say.
    pub fn heard_from_others(&self) -> bool {
        self.inner.heard_beacon.load(Ordering::Relaxed)
    }

    /// The relays the LANPage names ([`relays_from_launcher_ini`]). One that
    /// is new to the list is caught up on the private messages it lacks.
    pub fn set_trusted_relays(&self, ids: Vec<String>) {
        let ids: HashSet<String> = ids.into_iter().collect();
        let added: Vec<String> = {
            let mut trusted = lock(&self.inner.trusted_relays);
            let added = ids.difference(&trusted).cloned().collect();
            *trusted = ids;
            added
        };
        // Comparing histories is a pull: the relay would only ask in a few
        // minutes. It already had everything public; the private events
        // (sealed) go to it now.
        for id in added {
            if !self.inner.is_relay(&id)
                || !lock(&self.inner.peers).get(&id).is_some_and(|p| p.online)
            {
                continue;
            }
            let private: Vec<Stored> = lock(&self.inner.state)
                .missing_for(&id, &HashSet::new(), 0, true)
                .into_iter()
                .filter(|s| s.event.to.is_some())
                .collect();
            for line in batches(&private) {
                self.inner.send_to(&id, line);
            }
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ChatUpdate> {
        self.inner.updates.subscribe()
    }

    pub fn me(&self) -> String {
        self.inner.me.clone()
    }

    /// Everyone the chat knows, without the messages.
    pub fn peers(&self) -> Vec<PeerView> {
        self.inner.peer_views()
    }

    pub fn snapshot(&self) -> ChatSnapshot {
        ChatSnapshot {
            me: self.inner.me.clone(),
            nick: lock(&self.inner.nick).clone(),
            items: lock(&self.inner.state).views(),
            peers: self.inner.peer_views(),
            problem: self.inner.problem.clone(),
        }
    }

    /// A new nickname is announced right away, not with the next beacon.
    pub async fn set_nick(&self, nick: &str) {
        let nick = match clean_nick(nick) {
            n if n.is_empty() => host_nick(),
            n => n,
        };
        if *lock(&self.inner.nick) == nick {
            return;
        }
        *lock(&self.inner.nick) = nick;
        self.inner.send_beacon(false).await;
    }

    /// The game this player has running (its title), or `None`; the others
    /// see it next to the name.
    pub async fn set_playing(&self, game: Option<&str>) {
        let game = game.map(clean_playing).filter(|g| !g.is_empty());
        if *lock(&self.inner.playing) == game {
            return;
        }
        *lock(&self.inner.playing) = game;
        self.inner.send_beacon(false).await;
    }

    /// About this computer, for the others' list of people; it travels
    /// with the next `Hello` (on meeting, and every few minutes).
    pub fn set_info(&self, info: PeerInfo) {
        *lock(&self.inner.info) = Some(info.cleaned());
    }

    /// How long writing is paused after too many messages, if it is.
    pub fn paused_for(&self) -> Option<Duration> {
        lock(&self.inner.flood).left(Instant::now())
    }

    fn running(&self) -> Result<(), ChatError> {
        if lock(&self.inner.tasks).is_empty() {
            Err(ERR_DISABLED)
        } else {
            Ok(())
        }
    }

    /// The conversation an existing message or poll belongs to.
    fn conversation_of(&self, target: &str) -> Result<Option<String>, ChatError> {
        lock(&self.inner.state)
            .item_conversation(target)
            .ok_or(ERR_UNKNOWN_TARGET)
    }

    /// Write into `conversation`: `None` is the public room, `#<id>` a
    /// topic, anything else a person (private).
    pub fn send_text(
        &self,
        conversation: Option<String>,
        text: &str,
        reply_to: Option<String>,
    ) -> Result<ItemView, ChatError> {
        self.running()?;
        if let Some(r) = &reply_to {
            if self.conversation_of(r)? != conversation {
                return Err(ERR_NOT_ALLOWED);
            }
        }
        let id = self.inner.publish(
            conversation,
            Body::Text {
                text: text.trim_end().to_string(),
                reply_to,
                game: None,
            },
        )?;
        self.view(&id)
    }

    /// Link a game of the catalog: the others see its cover and title and
    /// can open it in their library. `title` is the message's text.
    pub fn share_game(
        &self,
        conversation: Option<String>,
        game: &str,
        title: &str,
    ) -> Result<ItemView, ChatError> {
        self.running()?;
        let id = self.inner.publish(
            conversation,
            Body::Text {
                text: title.trim().to_string(),
                reply_to: None,
                game: Some(game.to_string()),
            },
        )?;
        self.view(&id)
    }

    /// React with `emoji`; an empty one takes the reaction back.
    pub fn react(&self, target: &str, emoji: &str) -> Result<(), ChatError> {
        self.running()?;
        let conversation = self.conversation_of(target)?;
        self.inner.publish(
            conversation,
            Body::React {
                target: target.to_string(),
                emoji: emoji.to_string(),
            },
        )?;
        Ok(())
    }

    pub fn create_poll(
        &self,
        conversation: Option<String>,
        question: &str,
        options: Vec<PollChoice>,
        kind: PollKind,
        open: bool,
    ) -> Result<ItemView, ChatError> {
        self.running()?;
        let options: Vec<PollChoice> = options
            .into_iter()
            .map(|c| PollChoice {
                text: c.text.trim().to_string(),
                game: c.game,
            })
            .filter(|c| !c.text.is_empty())
            .collect();
        let id = self.inner.publish(
            conversation,
            Body::Poll {
                question: question.trim().to_string(),
                options,
                kind,
                open,
            },
        )?;
        self.view(&id)
    }

    /// Replace this launcher's vote; an empty list withdraws it.
    pub fn vote(&self, poll: &str, choices: Vec<String>) -> Result<(), ChatError> {
        self.running()?;
        let (closed, _, kind, ids) = lock(&self.inner.state)
            .poll_info(poll)
            .ok_or(ERR_UNKNOWN_TARGET)?;
        if closed {
            return Err(ERR_POLL_CLOSED);
        }
        let mut unique: Vec<String> = Vec::new();
        for c in choices {
            if !ids.contains(&c) {
                return Err(ERR_INVALID);
            }
            if !unique.contains(&c) {
                unique.push(c);
            }
        }
        if kind == PollKind::Single && unique.len() > 1 {
            return Err(ERR_INVALID);
        }
        let conversation = self.conversation_of(poll)?;
        self.inner.publish(
            conversation,
            Body::Vote {
                poll: poll.to_string(),
                choices: unique,
            },
        )?;
        Ok(())
    }

    pub fn add_poll_option(&self, poll: &str, choice: PollChoice) -> Result<(), ChatError> {
        self.running()?;
        let (closed, open, _, ids) = lock(&self.inner.state)
            .poll_info(poll)
            .ok_or(ERR_UNKNOWN_TARGET)?;
        let author = lock(&self.inner.state).item_author(poll) == Some(self.inner.me.as_str());
        if closed {
            return Err(ERR_POLL_CLOSED);
        }
        if (!open && !author) || ids.len() >= model::MAX_POLL_OPTIONS_TOTAL {
            return Err(ERR_NOT_ALLOWED);
        }
        let conversation = self.conversation_of(poll)?;
        self.inner.publish(
            conversation,
            Body::PollOption {
                poll: poll.to_string(),
                choice: PollChoice {
                    text: choice.text.trim().to_string(),
                    game: choice.game,
                },
            },
        )?;
        Ok(())
    }

    pub fn close_poll(&self, poll: &str) -> Result<(), ChatError> {
        self.running()?;
        self.own(poll)?;
        lock(&self.inner.state)
            .poll_info(poll)
            .ok_or(ERR_UNKNOWN_TARGET)?;
        let conversation = self.conversation_of(poll)?;
        self.inner.publish(
            conversation,
            Body::ClosePoll {
                poll: poll.to_string(),
            },
        )?;
        Ok(())
    }

    /// Change the text of one's own message; the newest edit counts.
    pub fn edit(&self, target: &str, text: &str) -> Result<(), ChatError> {
        self.running()?;
        self.own(target)?;
        if !lock(&self.inner.state).is_text(target) {
            return Err(ERR_NOT_ALLOWED);
        }
        let conversation = self.conversation_of(target)?;
        self.inner.publish(
            conversation,
            Body::Edit {
                target: target.to_string(),
                text: text.trim_end().to_string(),
            },
        )?;
        Ok(())
    }

    /// Open a topic: a public room of its own, for everyone.
    pub fn create_topic(&self, name: &str) -> Result<ItemView, ChatError> {
        self.running()?;
        let name: String = name.split_whitespace().collect::<Vec<_>>().join(" ");
        let id = self.inner.publish(None, Body::Topic { name })?;
        self.view(&id)
    }

    pub fn delete(&self, target: &str) -> Result<(), ChatError> {
        self.running()?;
        self.own(target)?;
        if lock(&self.inner.state).is_deleted(target) {
            return Ok(());
        }
        let conversation = self.conversation_of(target)?;
        self.inner.publish(
            conversation,
            Body::Delete {
                target: target.to_string(),
            },
        )?;
        Ok(())
    }

    fn own(&self, target: &str) -> Result<(), ChatError> {
        match lock(&self.inner.state).item_author(target) {
            None => Err(ERR_UNKNOWN_TARGET),
            Some(a) if a == self.inner.me => Ok(()),
            Some(_) => Err(ERR_NOT_ALLOWED),
        }
    }

    fn view(&self, id: &str) -> Result<ItemView, ChatError> {
        lock(&self.inner.state).view(id).ok_or(ERR_INVALID)
    }
}

/// Run a task that [`Chat::stop`] ends.
fn spawn<F>(inner: &Inner, fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let handle = tokio::spawn(fut).abort_handle();
    let mut tasks = lock(&inner.tasks);
    tasks.retain(|t| !t.is_finished());
    tasks.push(handle);
}

impl Inner {
    fn next_seq(&self) -> u64 {
        // Milliseconds where the clock allows, so the newest vote of a
        // launcher whose history was deleted still beats its older ones.
        let mut seq = lock(&self.seq);
        *seq = (*seq + 1).max(now_ms().max(0) as u64);
        *seq
    }

    /// Write an event of our own into `conversation` (see
    /// [`Chat::send_text`]), show it and send it to whom it concerns.
    fn publish(&self, conversation: Option<String>, body: Body) -> Result<String, ChatError> {
        let (to, topic) = match conversation {
            None => (None, None),
            Some(c) => match c.strip_prefix('#') {
                Some(topic) => (None, Some(topic.to_string())),
                None => (Some(c), None),
            },
        };
        if let Some(t) = &to {
            if !valid_peer_id(t) || *t == self.me {
                return Err(ERR_INVALID);
            }
        }
        if let Some(t) = &topic {
            if !lock(&self.state).is_topic(t) {
                return Err(ERR_UNKNOWN_TARGET);
            }
        }
        // What others read counts against the flood limit; reactions,
        // votes, closing a poll and deleting do not.
        let counts = matches!(
            body,
            Body::Text { .. }
                | Body::Poll { .. }
                | Body::PollOption { .. }
                | Body::Topic { .. }
                | Body::Edit { .. }
        );
        let seq = self.next_seq();
        let event = Event {
            id: format!("{}:{seq}", self.me),
            from: self.me.clone(),
            nick: lock(&self.nick).clone(),
            seq,
            ts: now_ms(),
            to,
            body,
            topic,
            sig: String::new(),
        };
        if !event.is_valid() {
            return Err(ERR_INVALID);
        }
        // Counted once it would go out: a refused message is no flood.
        if counts && lock(&self.flood).check(Instant::now()).is_err() {
            return Err(ERR_TOO_FAST);
        }
        let wire = self.keys.seal(&event).ok_or(ERR_INVALID)?;
        let id = event.id.clone();
        self.take_in_opened(vec![(Some(event.clone()), wire.clone(), now_ms())]);
        let frame = Frame::Events {
            events: vec![serde_json::to_value(&wire).map_err(|_| ERR_INVALID)?],
            ages: vec![0],
        };
        // Trusted relays get private events too: sealed, they keep them for
        // a recipient who is not here yet.
        let trusted = lock(&self.trusted_relays).clone();
        let recipients: Vec<String> = lock(&self.peers)
            .iter()
            .filter(|(id, p)| {
                p.online
                    && (event.to.as_ref().is_none_or(|to| to == *id)
                        || (p.relay && trusted.contains(*id)))
            })
            .map(|(id, _)| id.clone())
            .collect();
        if let Some(line) = frame_line(&frame) {
            for peer in recipients {
                // Someone offline gets it when they are back (`Hello`).
                if lock(&self.peers).get(&peer).is_some_and(|p| p.online) {
                    self.send_to(&peer, line.clone());
                }
            }
        }
        Ok(id)
    }

    /// Events from the network: only those whose signature holds and, if
    /// private, that open for us — or, on a relay, every signed one.
    /// `ages` says how long the sender has had each one.
    fn take_in(&self, events: Vec<(Event, i64)>) {
        let now = now_ms();
        let keep = KEEP_FOR.as_millis() as i64;
        let checked = events
            .into_iter()
            .filter_map(|(wire, age)| {
                let born = now - age.clamp(0, keep);
                if self.relay {
                    crypto::verify(&wire).then_some((None, wire, born))
                } else {
                    Some((Some(self.keys.open(&wire)?), wire, born))
                }
            })
            .collect();
        self.take_in_opened(checked);
    }

    /// Store new (plain, wire, born) events — plain `None` for one kept only
    /// to pass on, born when it first reached the LAN (expiry counts from
    /// there; "unread" from now) — save them and tell the interface what
    /// changed.
    fn take_in_opened(&self, events: Vec<(Option<Event>, Event, i64)>) {
        let seen = now_ms();
        let mut changed: Vec<String> = Vec::new();
        let mut accepted: Vec<Stored> = Vec::new();
        // Trimming lets the history grow by a tenth before it rewrites the
        // file, not once per message. The file is written under the state
        // lock (order: state, then history): a rewrite from a snapshot taken
        // outside it could overwrite an event appended in between.
        let trimmed = {
            let mut state = lock(&self.state);
            for (plain, wire, born) in events {
                let id = wire.id.clone();
                let new = match plain {
                    Some(plain) => state.insert_opened(plain, wire.clone(), seen).map(|ids| {
                        changed.extend(ids);
                    }),
                    None => state.insert_opaque(wire.clone(), seen).then_some(()),
                };
                if new.is_some() {
                    state.set_born(&id, born);
                    accepted.push(Stored {
                        seen,
                        event: wire,
                        born: (born < seen).then_some(born),
                    });
                }
            }
            let trimmed = state.len() > store::HISTORY_LIMIT + store::HISTORY_LIMIT / 10
                && state.trim(store::HISTORY_LIMIT);
            let history = lock(&self.history);
            if trimmed {
                history.rewrite(&state.stored());
            } else {
                history.append(&accepted);
            }
            trimmed
        };
        if trimmed {
            let _ = self.updates.send(ChatUpdate::Reset);
            return;
        }
        if changed.is_empty() {
            return;
        }
        changed.sort();
        changed.dedup();
        let items: Vec<ItemView> = {
            let state = lock(&self.state);
            changed.iter().filter_map(|id| state.view(id)).collect()
        };
        let _ = self.updates.send(ChatUpdate::Items { items });
    }

    fn peer_views(&self) -> Vec<PeerView> {
        let mut out: Vec<PeerView> = lock(&self.peers)
            .iter()
            .map(|(id, p)| PeerView {
                id: id.clone(),
                nick: p.nick.clone(),
                os: p.os.clone(),
                online: p.online,
                address: p.addr.ip().to_string(),
                relay: p.relay,
                playing: p.playing.clone().filter(|_| p.online),
                info: p.info.clone(),
            })
            .collect();
        out.sort_by(|a, b| {
            b.online
                .cmp(&a.online)
                .then_with(|| a.nick.to_lowercase().cmp(&b.nick.to_lowercase()))
                .then_with(|| a.id.cmp(&b.id))
        });
        out
    }

    fn emit_peers(&self) {
        let _ = self.updates.send(ChatUpdate::Peers {
            peers: self.peer_views(),
        });
    }

    fn beacon(&self, bye: bool) -> Vec<u8> {
        serde_json::to_vec(&Beacon {
            nll: PROTOCOL,
            id: self.me.clone(),
            nick: lock(&self.nick).clone(),
            port: self.tcp_port,
            os: self.os.clone(),
            bye,
            relay: self.relay,
            playing: lock(&self.playing).clone(),
        })
        .unwrap_or_default()
    }

    async fn send_beacon(&self, bye: bool) {
        let Some(udp) = &self.udp else {
            return;
        };
        let data = self.beacon(bye);
        let targets = self
            .targets
            .clone()
            .unwrap_or_else(|| broadcast_targets(self.udp_port));
        for t in targets {
            if let Err(e) = udp.send_to(&data, t).await {
                log::debug!("chat: beacon to {t}: {e}");
            }
        }
        // Peers that only answered directly (their broadcasts do not reach
        // us) get the beacon directly too.
        let direct: Vec<SocketAddr> = lock(&self.peers)
            .values()
            .filter(|p| p.online)
            .filter_map(|p| p.beacon_from)
            .collect();
        for t in direct {
            let _ = udp.send_to(&data, t).await;
        }
    }

    /// Drop what has grown older than [`KEEP_FOR`], on disk too.
    fn expire_old(&self) {
        let expired = {
            let mut state = lock(&self.state);
            let expired = state.expire(now_ms(), KEEP_FOR.as_millis() as i64);
            if expired {
                lock(&self.history).rewrite(&state.stored());
                store::save_gone(&self.data_dir, state.gone());
            }
            expired
        };
        if expired {
            let _ = self.updates.send(ChatUpdate::Reset);
        }
    }

    async fn beacon_loop(self: Arc<Self>, every: Duration) {
        let mut tick = tokio::time::interval(every);
        let mut last_expiry = Instant::now();
        loop {
            tick.tick().await;
            self.send_beacon(false).await;
            if last_expiry.elapsed() >= EXPIRE_EVERY {
                last_expiry = Instant::now();
                self.expire_old();
            }
            // Who has gone quiet, and who is due for a history comparison.
            let mut gone = false;
            let mut resync = Vec::new();
            {
                let mut peers = lock(&self.peers);
                for (id, p) in peers.iter_mut() {
                    if p.online && p.last_seen.elapsed() > self.offline_after {
                        p.online = false;
                        gone = true;
                    }
                    if p.online && p.last_hello.elapsed() > RESYNC_EVERY {
                        p.last_hello = Instant::now();
                        resync.push(id.clone());
                    }
                }
            }
            if gone {
                self.emit_peers();
            }
            for id in resync {
                self.send_hello(&id);
            }
        }
    }

    async fn beacon_receive_loop(self: Arc<Self>) {
        let Some(udp) = self.udp.clone() else {
            return;
        };
        let mut buf = vec![0u8; 2048];
        loop {
            match udp.recv_from(&mut buf).await {
                Ok((n, from)) => {
                    if let Ok(beacon) = serde_json::from_slice::<Beacon>(&buf[..n]) {
                        self.on_beacon(beacon, from).await;
                    }
                }
                // Windows reports an ICMP "port unreachable" for an earlier
                // send as an error on the next receive; that is no reason to
                // stop listening.
                Err(e) => {
                    log::debug!("chat: receive: {e}");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        }
    }

    async fn on_beacon(&self, b: Beacon, from: SocketAddr) {
        if b.nll != PROTOCOL || b.id == self.me || !valid_peer_id(&b.id) {
            return;
        }
        if b.bye {
            let changed = lock(&self.peers)
                .get_mut(&b.id)
                .map(|p| std::mem::replace(&mut p.online, false))
                .unwrap_or(false);
            if changed {
                self.emit_peers();
            }
            return;
        }
        self.heard_beacon.store(true, Ordering::Relaxed);
        let addr = SocketAddr::new(from.ip(), b.port);
        let (came_online, changed) =
            self.saw_peer(&b.id, &b.nick, &b.os, b.relay, addr, Some(from));
        let playing = b
            .playing
            .as_deref()
            .map(clean_playing)
            .filter(|g| !g.is_empty());
        // Always taken over, also from the beacon that brings the peer in.
        let game_changed = lock(&self.peers)
            .get_mut(&b.id)
            .is_some_and(|p| std::mem::replace(&mut p.playing, playing.clone()) != playing);
        let changed = changed || game_changed;
        if came_online {
            // Answer directly, so the other side need not wait for our next
            // broadcast — or ever receive one, where broadcasts do not pass.
            if let Some(udp) = &self.udp {
                let _ = udp.send_to(&self.beacon(false), from).await;
            }
            self.send_hello(&b.id);
        }
        if changed {
            self.emit_peers();
        }
    }

    /// Note a sign of life. Returns (it was offline or unknown, the list changed).
    fn saw_peer(
        &self,
        id: &str,
        nick: &str,
        os: &str,
        relay: bool,
        addr: SocketAddr,
        beacon_from: Option<SocketAddr>,
    ) -> (bool, bool) {
        let nick = match clean_nick(nick) {
            n if n.is_empty() => id.chars().take(8).collect(),
            n => n,
        };
        let os: String = os
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(16)
            .collect();
        let mut peers = lock(&self.peers);
        let now = Instant::now();
        let seen = match peers.get_mut(id) {
            // A `Hello` is unsigned and only a TCP connection: it says
            // nothing about a peer whose beacons are arriving anyway.
            Some(p) if p.online && beacon_from.is_none() => (false, false, false),
            Some(p) => {
                let came_online = !p.online;
                if p.addr == addr {
                    p.addr_seen = now;
                }
                let moved = p.addr != addr
                    && (came_online || p.addr_seen.elapsed() > self.offline_after / 2);
                if moved {
                    p.addr = addr;
                    p.addr_seen = now;
                }
                let changed =
                    came_online || moved || p.nick != nick || p.os != os || p.relay != relay;
                p.nick = nick;
                p.os = os;
                p.relay = relay;
                if beacon_from.is_some() {
                    p.beacon_from = beacon_from;
                }
                p.last_seen = now;
                p.online = true;
                if came_online {
                    p.last_hello = now;
                }
                (came_online, changed, moved)
            }
            None => {
                peers.insert(
                    id.to_string(),
                    Peer {
                        nick,
                        os,
                        addr,
                        beacon_from,
                        addr_seen: now,
                        last_seen: now,
                        online: true,
                        last_hello: now,
                        relay,
                        playing: None,
                        info: None,
                    },
                );
                (true, true, false)
            }
        };
        drop(peers);
        let (came_online, changed, moved) = seen;
        if came_online {
            log::info!("chat: {id} online{}", if relay { " (relay)" } else { "" });
        }
        // The writer holds on to the old address, or to a connection the
        // other side dropped when it restarted (a first write into it would
        // still "succeed"); the next frame starts a fresh one.
        if moved || came_online {
            lock(&self.links).remove(id);
        }
        (came_online, changed)
    }

    /// Whether `peer` is a relay the LANPage names, which may get every
    /// event (private ones sealed).
    fn is_relay(&self, peer: &str) -> bool {
        lock(&self.trusted_relays).contains(peer)
            && lock(&self.peers).get(peer).is_some_and(|p| p.relay)
    }

    fn send_hello(&self, peer: &str) {
        if let Some(line) = self.hello(peer, false) {
            self.send_to(peer, line);
        }
    }

    /// Our `Hello` for `peer`: what we have of what it may see.
    fn hello(&self, peer: &str, answer: bool) -> Option<Arc<str>> {
        let all = self.is_relay(peer);
        let (have, since) = {
            let state = lock(&self.state);
            (state.ids_for(peer, all), state.floor())
        };
        let frame = Frame::Hello {
            from: self.me.clone(),
            nick: lock(&self.nick).clone(),
            port: self.tcp_port,
            os: self.os.clone(),
            have,
            since,
            relay: self.relay,
            duplex: true,
            answer,
            info: lock(&self.info).clone(),
        };
        let line = frame_line(&frame);
        if line.is_none() {
            log::warn!("chat: history comparison with {peer} too large to send");
        }
        line
    }

    /// Queue a frame for a peer; one connection and one writer per peer keep
    /// the frames in order. Lock order: `links`, then `peers`.
    fn send_to(&self, peer: &str, line: Arc<str>) {
        let mut links = lock(&self.links);
        if let Some(tx) = links.get(peer) {
            if tx.send(line.clone()).is_ok() {
                return;
            }
        }
        let Some(addr) = lock(&self.peers).get(peer).map(|p| p.addr) else {
            return;
        };
        let Some(this) = self.this.upgrade() else {
            return;
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let _ = tx.send(line);
        let own = tx.downgrade();
        links.insert(peer.to_string(), tx);
        drop(links);
        spawn(self, this.link_writer(peer.to_string(), addr, rx, own));
    }

    /// Writes queued frames to one peer, connecting (again) when needed, and
    /// reads what comes back on that connection. A frame that cannot be
    /// delivered after one reconnect goes over a connection the peer opened
    /// to us, if there is one, or else is dropped: the next history
    /// comparison brings it across.
    async fn link_writer(
        self: Arc<Self>,
        peer: String,
        addr: SocketAddr,
        mut rx: mpsc::UnboundedReceiver<Arc<str>>,
        own: mpsc::WeakUnboundedSender<Arc<str>>,
    ) {
        let mut conn: Option<(OwnedWriteHalf, Arc<AtomicBool>)> = None;
        // When the peer could not be reached; while there is a way back,
        // frames take it for a while rather than wait for a timeout each.
        let mut unreachable_since: Option<Instant> = None;
        while let Some(line) = rx.recv().await {
            let back = || lock(&self.back_links).get(&peer).and_then(|b| b.upgrade());
            if unreachable_since.is_some_and(|t| t.elapsed() < RETRY_DIRECT_AFTER) {
                if let Some(back) = back() {
                    let _ = back.send(line);
                    continue;
                }
            }
            // The reader saw the other side close: a write would still
            // "succeed" once and vanish.
            if conn
                .as_ref()
                .is_some_and(|(_, closed)| closed.load(Ordering::Acquire))
            {
                conn = None;
            }
            let mut sent = false;
            for _ in 0..2 {
                if conn.is_none() {
                    let stream =
                        tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(addr))
                            .await
                            .ok()
                            .and_then(Result::ok);
                    let Some(stream) = stream else {
                        break;
                    };
                    unreachable_since = None;
                    let (read, write) = stream.into_split();
                    let closed = Arc::new(AtomicBool::new(false));
                    let (this, done, back) = (self.clone(), closed.clone(), own.clone());
                    spawn(&self, async move {
                        this.read_frames(read, addr, Back::Ours(back)).await;
                        done.store(true, Ordering::Release);
                    });
                    conn = Some((write, closed));
                }
                let Some((write, _)) = conn.as_mut() else {
                    break;
                };
                let written =
                    tokio::time::timeout(Duration::from_secs(5), write.write_all(line.as_bytes()))
                        .await;
                if matches!(written, Ok(Ok(()))) {
                    sent = true;
                    break;
                }
                conn = None;
            }
            if sent {
                continue;
            }
            let back = back();
            if unreachable_since.is_none() {
                log::info!(
                    "chat: {peer} not reachable at {addr}{}",
                    if back.is_some() {
                        "; writing over the connection it opened"
                    } else {
                        ""
                    }
                );
            }
            unreachable_since = Some(Instant::now());
            if let Some(back) = back {
                let _ = back.send(line);
            }
        }
    }

    /// Whether `from` is where we know `peer` to be.
    fn is_known_at(&self, peer: &str, from: SocketAddr) -> bool {
        lock(&self.peers)
            .get(peer)
            .is_some_and(|p| p.addr.ip() == from.ip())
    }

    /// Remember a connection `peer` opened to us as the way back to it
    /// (checked with [`Inner::is_known_at`] by the caller).
    fn remember_back_link(&self, peer: &str, tx: &mpsc::UnboundedSender<Arc<str>>) {
        let mut back = lock(&self.back_links);
        back.retain(|_, b| b.upgrade().is_some());
        back.insert(peer.to_string(), tx.downgrade());
    }

    async fn accept_loop(self: Arc<Self>, listener: TcpListener) {
        loop {
            match listener.accept().await {
                Ok((stream, from)) => {
                    let (read, write) = stream.into_split();
                    let (tx, rx) = mpsc::unbounded_channel();
                    spawn(&self, back_writer(write, rx));
                    let i = self.clone();
                    spawn(&self, async move {
                        i.read_frames(read, from, Back::Theirs(tx)).await
                    });
                }
                Err(e) => {
                    log::debug!("chat: accept: {e}");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
    }

    async fn read_frames(self: Arc<Self>, read: OwnedReadHalf, from: SocketAddr, back: Back) {
        let mut reader = BufReader::new(read);
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = (&mut reader)
                .take(MAX_FRAME as u64 + 1)
                .read_until(b'\n', &mut line)
                .await;
            match read {
                Ok(0) | Err(_) => return,
                Ok(_) if line.len() > MAX_FRAME => {
                    log::warn!("chat: frame from {from} too large, closing");
                    return;
                }
                Ok(_) => {}
            }
            let Ok(frame) = serde_json::from_slice::<Frame>(&line) else {
                continue;
            };
            match &back {
                Back::Theirs(tx) => self.on_frame(frame, from, Some(tx), true),
                Back::Ours(own) => self.on_frame(frame, from, own.upgrade().as_ref(), false),
            }
        }
    }

    /// `back` writes on the connection the frame came in on; `theirs` says
    /// the peer opened it.
    fn on_frame(
        &self,
        frame: Frame,
        from: SocketAddr,
        back: Option<&mpsc::UnboundedSender<Arc<str>>>,
        theirs: bool,
    ) {
        match frame {
            Frame::Hello {
                from: peer,
                nick,
                port,
                os,
                have,
                since,
                relay,
                duplex,
                answer,
                info,
            } => {
                if peer == self.me || !valid_peer_id(&peer) {
                    return;
                }
                // On a connection we opened, the address we dialled is the
                // peer's; `from` would be its outgoing port.
                let addr = if theirs {
                    SocketAddr::new(from.ip(), port)
                } else {
                    from
                };
                let (came_online, changed) = self.saw_peer(&peer, &nick, &os, relay, addr, None);
                // Only from where the peer is known to be: a `Hello` is
                // unsigned.
                let info_changed = match info.map(PeerInfo::cleaned) {
                    Some(info) if !theirs || self.is_known_at(&peer, from) => lock(&self.peers)
                        .get_mut(&peer)
                        .is_some_and(|p| p.info.replace(info.clone()).as_ref() != Some(&info)),
                    _ => false,
                };
                if changed || info_changed {
                    self.emit_peers();
                }
                let have: HashSet<String> = have.into_iter().collect();
                let all = self.is_relay(&peer);
                let missing = lock(&self.state).missing_for(&peer, &have, since, all);
                if !missing.is_empty() {
                    log::info!("chat: {peer} lacks {} events; sending", missing.len());
                }
                // A `Hello` is unsigned: on a connection the sender opened,
                // answer there only if it comes from where the peer is
                // known to be (`saw_peer` places a new or returning one
                // there), else anyone could collect a peer's events by
                // claiming its id.
                let trusted_way = !theirs || self.is_known_at(&peer, from);
                match back.filter(|_| duplex && trusted_way) {
                    // Back the way it came: one of the two reaching the
                    // other is enough.
                    Some(back) => {
                        if theirs {
                            self.remember_back_link(&peer, back);
                        }
                        for line in batches(&missing) {
                            let _ = back.send(line);
                        }
                        if !answer {
                            if let Some(line) = self.hello(&peer, true) {
                                let _ = back.send(line);
                            }
                        }
                    }
                    // A launcher that only writes: answer on our own.
                    None => {
                        if came_online {
                            // Its beacons may not reach us; compare the
                            // other way too.
                            self.send_hello(&peer);
                        }
                        for line in batches(&missing) {
                            self.send_to(&peer, line);
                        }
                    }
                }
            }
            Frame::Events { events, ages } => {
                let events: Vec<(Event, i64)> = events
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, v)| {
                        Some((
                            serde_json::from_value(v).ok()?,
                            ages.get(i).copied().unwrap_or(0),
                        ))
                    })
                    .collect();
                self.take_in(events);
            }
        }
    }
}

fn frame_line(frame: &Frame) -> Option<Arc<str>> {
    let mut text = serde_json::to_string(frame).ok()?;
    text.push('\n');
    (text.len() <= MAX_FRAME).then(|| Arc::from(text))
}

/// Frames of at most [`BATCH_BYTES`] carrying `events`, each with how long
/// this launcher has had it.
fn batches(events: &[Stored]) -> Vec<Arc<str>> {
    let now = now_ms();
    let mut out = Vec::new();
    let mut current: Vec<serde_json::Value> = Vec::new();
    let mut ages: Vec<i64> = Vec::new();
    let mut size = 0;
    for s in events {
        let Ok(value) = serde_json::to_value(&s.event) else {
            continue;
        };
        let len = value.to_string().len();
        if !current.is_empty() && size + len > BATCH_BYTES {
            out.extend(frame_line(&Frame::Events {
                events: std::mem::take(&mut current),
                ages: std::mem::take(&mut ages),
            }));
            size = 0;
        }
        size += len;
        current.push(value);
        ages.push((now - s.born()).max(0));
    }
    if !current.is_empty() {
        out.extend(frame_line(&Frame::Events {
            events: current,
            ages,
        }));
    }
    out
}

/// Where the answer to a frame goes.
enum Back {
    /// The peer opened the connection; this writes on it.
    Theirs(mpsc::UnboundedSender<Arc<str>>),
    /// We opened it: our link to the peer, while it is the current one.
    Ours(mpsc::WeakUnboundedSender<Arc<str>>),
}

/// Writes answers on a connection a peer opened, until it or the reader
/// side is done.
async fn back_writer(mut write: OwnedWriteHalf, mut rx: mpsc::UnboundedReceiver<Arc<str>>) {
    while let Some(line) = rx.recv().await {
        let written =
            tokio::time::timeout(Duration::from_secs(5), write.write_all(line.as_bytes())).await;
        if !matches!(written, Ok(Ok(()))) {
            return;
        }
    }
}

#[cfg(test)]
mod tests;
