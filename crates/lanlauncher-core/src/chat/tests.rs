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
        let mut config = ChatConfig::new(self.dirs[i].path().to_path_buf(), nick);
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
    let events: Vec<Event> = (1..=300)
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
            sig: String::new(),
        })
        .collect();
    let frames = batches(&events);
    assert!(frames.len() > 1);
    assert!(frames.iter().all(|f| f.len() <= MAX_FRAME));
    let total: usize = frames
        .iter()
        .map(|f| match serde_json::from_str::<Frame>(f).unwrap() {
            Frame::Events { events } => events.len(),
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
