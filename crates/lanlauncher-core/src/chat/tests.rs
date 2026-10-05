//! Several chats on 127.0.0.1, each beaconing straight at the others.

use super::*;

fn free_udp_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Lan {
    ports: Vec<u16>,
    dirs: Vec<tempfile::TempDir>,
}

impl Lan {
    fn new(n: usize) -> Self {
        Self {
            ports: (0..n).map(|_| free_udp_port()).collect(),
            dirs: (0..n).map(|_| tempfile::tempdir().unwrap()).collect(),
        }
    }

    async fn start(&self, i: usize, nick: &str) -> Chat {
        self.start_as(i, nick, false).await
    }

    async fn start_as(&self, i: usize, nick: &str, relay: bool) -> Chat {
        self.start_with(i, nick, relay, None).await
    }

    /// One whose firewall lets nobody connect: it names a port where
    /// nothing listens.
    async fn start_unreachable(&self, i: usize, nick: &str) -> Chat {
        let closed = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        self.start_with(i, nick, false, Some(closed)).await
    }

    async fn start_with(&self, i: usize, nick: &str, relay: bool, advertise: Option<u16>) -> Chat {
        let mut config = ChatConfig::new(self.dirs[i].path().to_path_buf(), nick);
        config.relay = relay;
        config.advertise_port = advertise;
        config.port = self.ports[i];
        config.bind = IpAddr::V4(Ipv4Addr::LOCALHOST);
        config.beacon_every = Duration::from_millis(100);
        config.beacon_targets = Some(
            self.ports
                .iter()
                .map(|p| SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), *p))
                .collect(),
        );
        Chat::start(config).await.unwrap()
    }
}

async fn until(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..200 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

fn texts(chat: &Chat) -> Vec<String> {
    chat.snapshot()
        .items
        .into_iter()
        .filter_map(|i| i.text)
        .collect()
}

fn online(chat: &Chat) -> Vec<String> {
    chat.snapshot()
        .peers
        .into_iter()
        .filter(|p| p.online)
        .map(|p| p.nick)
        .collect()
}

#[tokio::test]
async fn launchers_find_each_other_and_talk_in_public() {
    let lan = Lan::new(2);
    let a = lan.start(0, "Alice").await;
    let b = lan.start(1, "Bob").await;
    until("discovery", || {
        online(&a) == ["Bob"] && online(&b) == ["Alice"]
    })
    .await;
    let mut updates = b.subscribe();
    a.send_text(None, "gg", None).unwrap();
    until("delivery", || texts(&b) == ["gg"]).await;
    let first = b.snapshot().items.remove(0);
    assert_eq!(first.nick, "Alice");
    assert!(!first.mine && first.conversation.is_none());
    assert!(matches!(
        updates.try_recv(),
        Ok(ChatUpdate::Items { .. }) | Ok(ChatUpdate::Peers { .. })
    ));
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn private_messages_reach_only_their_recipient() {
    let lan = Lan::new(3);
    let a = lan.start(0, "Alice").await;
    let b = lan.start(1, "Bob").await;
    let c = lan.start(2, "Carol").await;
    until("discovery", || {
        online(&a).len() == 2 && online(&c).len() == 2
    })
    .await;
    a.send_text(Some(b.me()), "psst", None).unwrap();
    a.send_text(None, "hello all", None).unwrap();
    until("public", || texts(&c) == ["hello all"]).await;
    until("private", || texts(&b).len() == 2).await;
    let private = b
        .snapshot()
        .items
        .into_iter()
        .find(|i| i.text.as_deref() == Some("psst"))
        .unwrap();
    assert_eq!(private.conversation, Some(a.me()));
    // Carol must not even have it on disk.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(texts(&c), ["hello all"]);
    for chat in [a, b, c] {
        chat.stop().await;
    }
}

#[tokio::test]
async fn a_late_arrival_reads_the_history_and_gets_what_was_sent_to_it() {
    let lan = Lan::new(2);
    let a = lan.start(0, "Alice").await;
    let b = lan.start(1, "Bob").await;
    until("discovery", || online(&a) == ["Bob"]).await;
    let bob = b.me();
    b.stop().await;
    until("bye", || online(&a).is_empty()).await;
    a.send_text(None, "who wants pizza?", None).unwrap();
    a.send_text(Some(bob), "you owe me one", None).unwrap();
    // Bob comes back with the same identity and history directory.
    let b = lan.start(1, "Bob").await;
    until("catch-up", || texts(&b).len() == 2).await;
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn one_launcher_reaching_the_other_is_enough() {
    let lan = Lan::new(2);
    let a = lan.start(0, "Alice").await;
    a.send_text(None, "before you came", None).unwrap();
    // Bob's firewall lets nobody in; his beacons still reach Alice.
    let b = lan.start_unreachable(1, "Bob").await;
    until("catch-up over Bob's own connection", || {
        texts(&b) == ["before you came"]
    })
    .await;
    b.send_text(None, "hi", None).unwrap();
    until("Bob to Alice", || texts(&a).len() == 2).await;
    a.send_text(Some(b.me()), "psst", None).unwrap();
    until("Alice to Bob, back the way Bob came", || {
        texts(&b).len() == 3
    })
    .await;
    assert!(a.heard_from_others() && b.heard_from_others());
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn reactions_votes_and_new_names_travel() {
    let lan = Lan::new(2);
    let a = lan.start(0, "Alice").await;
    let b = lan.start(1, "Bob").await;
    until("discovery", || online(&b) == ["Alice"]).await;
    let poll = a
        .create_poll(
            None,
            "Next?",
            vec![
                PollChoice {
                    text: "Quake".into(),
                    game: None,
                },
                PollChoice {
                    text: "UT".into(),
                    game: None,
                },
            ],
            PollKind::Single,
            true,
        )
        .unwrap();
    until("poll", || b.snapshot().items.len() == 1).await;
    b.vote(&poll.id, vec!["1".into()]).unwrap();
    b.react(&poll.id, "🔥").unwrap();
    assert_eq!(
        b.vote(&poll.id, vec!["0".into(), "1".into()]),
        Err(ERR_INVALID)
    );
    assert_eq!(b.close_poll(&poll.id), Err(ERR_NOT_ALLOWED));
    until("vote and reaction", || {
        let item = a.snapshot().items.remove(0);
        let poll = item.poll.unwrap();
        poll.options[1].voters == ["Bob"] && item.reactions.len() == 1
    })
    .await;
    a.close_poll(&poll.id).unwrap();
    until("closed", || {
        b.snapshot().items[0]
            .poll
            .as_ref()
            .is_some_and(|p| p.closed)
    })
    .await;
    assert_eq!(b.vote(&poll.id, vec!["0".into()]), Err(ERR_POLL_CLOSED));
    a.set_nick("Alice the Great").await;
    until("rename", || online(&b) == ["Alice the Great"]).await;
    a.stop().await;
    assert_eq!(a.send_text(None, "x", None).map(|_| ()), Err(ERR_DISABLED));
    b.stop().await;
}

#[test]
fn large_histories_are_split_into_frames_of_bounded_size() {
    let events: Vec<Stored> = (1..=300)
        .map(|seq| Event {
            id: format!("a:{seq}"),
            from: "a".into(),
            nick: "A".into(),
            seq,
            ts: 0,
            to: None,
            body: Body::Text {
                text: "x".repeat(model::MAX_TEXT),
                reply_to: None,
            },
            topic: None,
            sig: String::new(),
        })
        .map(|event| Stored {
            seen: 0,
            event,
            born: None,
        })
        .collect();
    let frames = batches(&events);
    assert!(frames.len() > 1);
    assert!(frames.iter().all(|f| f.len() <= MAX_FRAME));
    let total: usize = frames
        .iter()
        .map(|f| match serde_json::from_str::<Frame>(f).unwrap() {
            Frame::Events { events, ages } => {
                assert_eq!(ages.len(), events.len());
                events.len()
            }
            Frame::Hello { .. } => 0,
        })
        .sum();
    assert_eq!(total, 300);
}

async fn send_raw(port: u16, frame: serde_json::Value) {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    stream
        .write_all(format!("{frame}\n").as_bytes())
        .await
        .unwrap();
    stream.flush().await.unwrap();
}

/// The TCP port a chat listens on, as its peers learned it.
fn tcp_port_of(observer: &Chat, peer: &str) -> u16 {
    lock(&observer.inner.peers)[peer].addr.port()
}

#[tokio::test]
async fn unsigned_or_forged_events_are_not_taken_in() {
    let lan = Lan::new(2);
    let a = lan.start(0, "Alice").await;
    let b = lan.start(1, "Bob").await;
    until("discovery", || online(&b) == ["Alice"]).await;
    let port = tcp_port_of(&b, &a.me());
    // Mallory writes as Bob: right id, no valid signature.
    let forged = serde_json::json!({
        "id": format!("{}:1", b.me()), "from": b.me(), "nick": "Bob", "seq": 1, "ts": 1,
        "body": {"type": "text", "text": "I quit"}, "sig": "00",
    });
    send_raw(port, serde_json::json!({"t": "events", "events": [forged]})).await;
    a.send_text(None, "after", None).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(texts(&a), ["after"]);
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn a_hello_in_someone_elses_name_gets_nothing() {
    let lan = Lan::new(2);
    let a = lan.start(0, "Alice").await;
    let b = lan.start(1, "Bob").await;
    until("discovery", || online(&a) == ["Bob"]).await;
    a.send_text(Some(b.me()), "only for Bob", None).unwrap();
    until("private", || texts(&b).len() == 1).await;
    let bob_port = tcp_port_of(&a, &b.me());
    // Mallory claims Bob's id and asks for everything, at her own address.
    let trap = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let hello = serde_json::json!({
        "t": "hello", "from": b.me(), "nick": "Bob", "port": trap.local_addr().unwrap().port(),
        "os": "linux", "have": [],
    });
    send_raw(tcp_port_of(&b, &a.me()), hello).await;
    let caught = tokio::time::timeout(Duration::from_millis(500), trap.accept()).await;
    assert!(caught.is_err(), "Alice answered Mallory");
    assert_eq!(tcp_port_of(&a, &b.me()), bob_port, "Bob's address stays");
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn a_relay_hands_on_what_was_said_while_nobody_else_was_there() {
    let lan = Lan::new(3);
    let relay = lan.start_as(0, "Archiv", true).await;
    let a = lan.start(1, "Alice").await;
    until("relay seen", || {
        a.snapshot().peers.iter().any(|p| p.online && p.relay)
    })
    .await;
    // Bob's id is known from an earlier meeting; he is not here now.
    let bob_dir = lan.dirs[2].path().to_path_buf();
    let bob_id = store::load_keys(&bob_dir).unwrap().id().to_string();
    a.send_text(None, "Turnier um 20 Uhr", None).unwrap();
    a.send_text(Some(bob_id.clone()), "du schuldest mir ein Bier", None)
        .unwrap();
    // A relay the LANPage does not name gets public messages only: anyone
    // can call themselves a relay.
    until("public stored", || lock(&relay.inner.state).len() == 1).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(lock(&relay.inner.state).len(), 1);
    // Named, it is caught up on the private one right away.
    a.set_trusted_relays(vec![relay.me()]);
    until("relay stored", || lock(&relay.inner.state).len() == 2).await;
    a.stop().await;
    // The relay cannot read the private message, on disk or in memory.
    let history = std::fs::read_to_string(lan.dirs[0].path().join("history.jsonl")).unwrap();
    assert!(history.contains("Turnier"));
    assert!(!history.contains("Bier"));
    assert!(relay.snapshot().items.is_empty());
    // Bob starts after Alice has gone and gets both.
    let b = lan.start(2, "Bob").await;
    assert_eq!(b.me(), bob_id);
    until("catch-up from the relay", || texts(&b).len() == 2).await;
    assert!(online(&b).contains(&"Archiv".to_string()));
    relay.stop().await;
    b.stop().await;
}

#[test]
fn relay_ids_come_from_launcher_ini() {
    let id = "AB".repeat(32);
    let extra =
        std::collections::BTreeMap::from([("chat_relay".to_string(), format!("{id}, not-an-id"))]);
    assert_eq!(relays_from_launcher_ini(&extra), vec![id.to_lowercase()]);
    assert!(relays_from_launcher_ini(&Default::default()).is_empty());
}

#[test]
fn the_block_the_relay_prints_parses_as_launcher_ini() {
    let id = "cd".repeat(32);
    let ini = format!("chat_relay ### NextGen chat relay {{\n{id}\n}}\n");
    let config = crate::launcher_ini::LanConfig::parse(&ini).unwrap();
    assert_eq!(relays_from_launcher_ini(&config.extra), vec![id]);
}

#[tokio::test]
async fn messages_from_the_last_lan_are_gone_at_start() {
    let lan = Lan::new(1);
    let dir = lan.dirs[0].path();
    let keys = store::load_keys(dir).unwrap();
    let day = 24 * 3600 * 1000;
    let stored: Vec<Stored> = [
        (1, now_ms() - 6 * day, "last LAN"),
        (2, now_ms() - day, "yesterday"),
    ]
    .into_iter()
    .map(|(seq, ts, text)| {
        let plain = Event {
            id: format!("{}:{seq}", keys.id()),
            from: keys.id().to_string(),
            nick: "Me".into(),
            seq,
            ts,
            to: None,
            body: Body::Text {
                text: text.into(),
                reply_to: None,
            },
            topic: None,
            sig: String::new(),
        };
        Stored {
            seen: ts,
            event: keys.seal(&plain).unwrap(),
            born: None,
        }
    })
    .collect();
    let (history, _, _) = store::History::open(dir);
    history.append(&stored);
    let chat = lan.start(0, "Me").await;
    assert_eq!(texts(&chat), ["yesterday"]);
    // Remembered, so a peer that still has it cannot hand it back after a
    // restart either.
    assert!(store::load_gone(dir).contains_key(&format!("{}:1", keys.id())));
    let on_disk = std::fs::read_to_string(dir.join("history.jsonl")).unwrap();
    assert!(
        !on_disk.contains(&format!("{}:1\"", keys.id())),
        "rewritten without it"
    );
    chat.stop().await;
}

#[tokio::test]
async fn caught_up_history_keeps_its_age() {
    let lan = Lan::new(2);
    let a = lan.start(0, "Alice").await;
    a.send_text(None, "from yesterday", None).unwrap();
    // Alice has had it for a day.
    {
        let mut state = lock(&a.inner.state);
        let stored = state.stored();
        let mut fresh = ChatState::new(&a.me());
        for s in stored {
            let plain = a.inner.keys.open(&s.event).unwrap();
            fresh.insert_opened(plain, s.event, s.seen - 24 * 3600 * 1000);
        }
        *state = fresh;
    }
    let b = lan.start(1, "Bob").await;
    until("catch-up", || texts(&b).len() == 1).await;
    let a_day = 24 * 3600 * 1000;
    // For "unread", it arrived just now ...
    let received = b.snapshot().items[0].received;
    assert!(now_ms() - received < 60_000, "new to Bob");
    // ... but its five days count from when Alice got it, on disk too.
    let born = lock(&b.inner.state).stored()[0].born();
    assert!(
        (now_ms() - born - a_day).abs() < 60_000,
        "expires with Alice's copy"
    );
    let on_disk = store::History::open(lan.dirs[1].path()).1;
    assert_eq!(on_disk[0].born(), born);
    a.stop().await;
    b.stop().await;
}

#[tokio::test]
async fn topics_and_edits_travel() {
    let lan = Lan::new(2);
    let a = lan.start(0, "Alice").await;
    let b = lan.start(1, "Bob").await;
    until("discovery", || online(&b) == ["Alice"]).await;
    let topic = a.create_topic("  CS   Turnier ").unwrap();
    assert_eq!(topic.topic_name.as_deref(), Some("CS Turnier"));
    let conversation = topic.conversation.clone().unwrap();
    let msg = a
        .send_text(Some(conversation.clone()), "20 Uhr", None)
        .unwrap();
    assert_eq!(msg.conversation.as_deref(), Some(conversation.as_str()));
    a.edit(&msg.id, "21 Uhr").unwrap();
    until("topic message, edited", || {
        b.snapshot().items.iter().any(|i| {
            i.conversation.as_deref() == Some(conversation.as_str())
                && i.text.as_deref() == Some("21 Uhr")
                && i.edited
        })
    })
    .await;
    assert_eq!(b.edit(&msg.id, "nope"), Err(ERR_NOT_ALLOWED));
    // A topic that does not exist cannot be written to.
    assert_eq!(
        a.send_text(Some("#nobody:1".into()), "x", None).map(|_| ()),
        Err(ERR_UNKNOWN_TARGET)
    );
    a.stop().await;
    b.stop().await;
}

#[test]
fn a_flood_pauses_writing_for_a_while() {
    let mut flood = Flood::default();
    let start = Instant::now();
    for i in 0..FLOOD_COUNT {
        assert!(flood.check(start + Duration::from_secs(i as u64)).is_ok());
    }
    let at = start + Duration::from_secs(FLOOD_COUNT as u64);
    assert_eq!(flood.check(at), Err(FLOOD_PAUSE), "one too many");
    assert!(flood.check(at + FLOOD_PAUSE / 2).is_err(), "still paused");
    assert_eq!(flood.left(at + FLOOD_PAUSE / 2), Some(FLOOD_PAUSE / 2));
    assert!(flood.check(at + FLOOD_PAUSE).is_ok(), "over");
    // Writing at a normal pace never pauses.
    let mut calm = Flood::default();
    for i in 0..50u64 {
        assert!(calm.check(start + FLOOD_WINDOW / 4 * i as u32).is_ok());
    }
}

#[tokio::test]
async fn a_flood_is_refused_but_reactions_still_go() {
    let lan = Lan::new(2);
    let a = lan.start(0, "Alice").await;
    for i in 0..FLOOD_COUNT {
        a.send_text(None, &format!("{i}"), None).unwrap();
    }
    assert_eq!(a.send_text(None, "spam", None).err(), Some(ERR_TOO_FAST));
    // A refused message is no flood.
    let b = lan.start(1, "Bob").await;
    for _ in 0..FLOOD_COUNT * 2 {
        assert_eq!(b.send_text(None, "   ", None).err(), Some(ERR_INVALID));
    }
    assert!(b.send_text(None, "hi", None).is_ok());
    b.stop().await;
    assert!(a.paused_for().is_some());
    let first = a.snapshot().items[0].id.clone();
    assert!(a.react(&first, "👍").is_ok());
    a.stop().await;
}

#[tokio::test]
async fn others_see_which_game_is_running() {
    let lan = Lan::new(2);
    let a = lan.start(0, "Alice").await;
    let b = lan.start(1, "Bob").await;
    until("discovery", || online(&b) == ["Alice"]).await;
    a.set_playing(Some("  Quake III Arena\n")).await;
    let playing = |c: &Chat| c.snapshot().peers.first().and_then(|p| p.playing.clone());
    until("playing", || {
        playing(&b).as_deref() == Some("Quake III Arena")
    })
    .await;
    a.set_playing(None).await;
    until("stopped", || playing(&b).is_none()).await;
    a.stop().await;
    b.stop().await;
}
