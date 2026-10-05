# Architecture

```
┌─────────────────────────────── Tauri window (WebView) ───────────────────────────────┐
│ Svelte 5 UI  ·  i18n de/en  ·  theme engine  ·  browser mock for development          │
└──────────────────────────────▲──────────────────────────────────────────────────────┘
                               │ invoke() commands / emitted events (install-status, transport-health, event-updated)
┌──────────────────────────────┴──────────────────────────────────────────────────────┐
│ src-tauri  (thin shell)                                                               │
│  commands.rs  fixes.rs (PowerShell/netsh)  lib.rs (service loops, sidecar, demo mode) │
└──────────────────────────────▲──────────────────────────────────────────────────────┘
                               │
┌──────────────────────────────┴──────────────────────────────────────────────────────┐
│ crates/lanlauncher-core (no GUI, fully unit-tested)                                   │
│  catalog ─ launcher_ini ─ theme ─ settings ─ library ─ manifest ─ script_probe        │
│  transport { resilio | folder | demo }  ─▶  install (state machine)  ─▶  extract      │
│  launch { windows scripts | unix runners }   lanpage (ini, stats)   diagnostics       │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

## Data flow of an installation

1. UI calls `install_game` → `InstallManager::install` adds the Resilio share for the game
   (`<root>/<game_id>`) via the transport and marks the tracker *wanted*.
2. Every 2 s `InstallManager::tick` observes each tracked game (`Observation::from_disk` +
   transport status) and runs `Tracker::step`, a pure function that decides the next action.
3. **Syncing → Verifying** happens when `<id>.eti` exists, no `<id>.eti.!sync` partial file
   exists, `version.ini` is present and the archive size has been stable for 15 s — *or* when
   the transport reports completion. The engine's percentage is never required.
4. **Verifying** runs a full UnRAR CRC test in a blocking task. Failure → back to Syncing with
   the problem `install.archive_incomplete` (retry on size change or after 2 min).
5. **Extracting** unpacks into `.nll-staging` and swaps it into `local/` atomically.
6. **Setup** applies manifest steps (copy/touch) and on Windows runs `game_setup.cmd`.
7. A receipt `.nll-install.json` (revision, size, files, setup_done, exe_override) is written.
   **Ready** ⇔ receipt exists, required files exist. `UpdateAvailable` ⇔ receipt revision ≠
   catalog revision (or a newer `version.ini` arrived).
8. **Repair** works from any phase: cancels running work, re-adds the share, resets the
   stability timer and re-runs 3–7.

## Transport

`Transport` is a trait. `ResilioTransport` spawns the official binary (the copy bundled with the
app ranks before any system install, see `locate_binary_detailed`; on Windows it runs in place
with `/noinstall`) with a generated `config.json` (API on 127.0.0.1 with a random password, storage in the app data dir,
`sync_max_time_diff` 48 h, LAN discovery mode 3, tracker/relay off in LAN mode), cleans up
orphaned instances (PID file + process list) and talks to the documented Sync API when an
`api_key` is configured, otherwise to the GUI endpoints. In managed mode the shell registers the
catalog share `<default root>/eti_launcher` itself (`register_catalog_share` in `src-tauri/src/lib.rs`,
key from `catalog::catalog_share_key`: settings override or `BUILTIN_CATALOG_KEY`). `TransportHealth`
carries `catalog_peers` and `server_found` (`Some(true)` when that share has a peer, `None` when the
transport cannot tell); the status bar shows it, and a changed `game.db` is reloaded automatically.
 `FolderTransport` only watches the
folders and hands out keys. `DemoTransport` simulates downloads including the "stuck at 99 %"
case and is used by `--demo` / `LANLAUNCHER_DEMO=1`.

## Launching

* Windows: `cmd.exe /C "game_start.cmd" "<game_path>" <id> <lang> "<player>"` as the invoking
  user (manifest `asInvoker`). Rights are requested on demand (`launch::elevate`, PowerShell
  `Start-Process -Verb RunAs` on a batch file in `<data>/run/`): the script's
  `netsh advfirewall … add rule` lines are registered once per game (marker in `<data>/firewall/`),
  at setup or the first start, and only scripts that write HKLM or import `.reg` files start
  with a prompt (`script_needs_admin`). `game_setup.cmd`, server scripts, the diagnostics
  repairs and programs whose own manifest demands administrator rights (error 740) prompt too.
  `game_setup.cmd "<game_path>" <id> <lang> "<player>"` runs once after extraction, through
  the same raw `cmd.exe /S /C` command line as `game_start.cmd`. Installations found on disk
  (made by the ETI launcher) are adopted by comparing the archive listing with `local/`
  (`extract::matches_extracted`) instead of being re-extracted.
* macOS/Linux: `launch::unix::plan` resolves exe/args from the manifest (user override →
  organiser overlay `nll-manifest.toml` in the share → bundled → derived from the script),
  picks CrossOver / Proton / Wine. Automatic selection keeps the original per-game prefix
  (`<share>/.nll-prefix`); an explicit per-game runner selection uses a stable, separate prefix
  (or CrossOver bottle) for each compatibility-tool path.

## Media

Covers are extracted from `eti_launcher/update/assets.eti` into the cache directory. Preview
videos are looked up at `eti_launcher/video/<id>.mp4` (also `videos/`, `update/video/`, `.webm`)
inside the default library root and played muted in the detail header. Library roots are added to
the Tauri asset-protocol scope at runtime so the WebView can load them.

## LAN chat

`crates/lanlauncher-core/src/chat/` is a chat between launchers without a server:

* **Presence:** each launcher broadcasts a UDP beacon (`{nll, id, nick, port, os}`) on port
  41950 every 3 s, to the broadcast address of every IPv4 card and to 255.255.255.255. A peer
  not heard from for 12 s is offline; a closing launcher sends `bye`. A new peer is answered
  directly, so one direction of broadcasts is enough.
* **Messages:** newline-delimited JSON over TCP (41950, or a random port when taken; the beacon
  names it), one writer per peer. Public events go to every online peer, private ones only to
  their recipient. Nothing is relayed. Connections carry frames both ways: a `Hello` with
  `duplex` is answered on the connection it came in on (the missing events plus a `Hello` with
  `answer`, which gets events back but no further `Hello`), and a peer that cannot be reached
  directly is written to over a connection it opened (`back_links`, for 30 s before the next
  direct try). So one of two launchers reaching the other is enough to catch up and talk; only
  a launcher that hears no one (no beacon arrives) stays alone. `Chat::heard_from_others` is that
  proof for the diagnostics: the firewall notices appear only while no beacon has arrived.
* **Presence extras:** the beacon carries `playing`, the title of the newest game in
  `AppState::running` (the same game the stats beacon reports to the LANPage). A 15 s loop
  (`chat::prune_running`) drops games from `running` once nothing runs from their folder
  (`launch::runs_from`: any process whose program or working folder is in it; the started pid is
  usually a starter that has ended, and Windows reuses its number), so both stop naming a game that has ended.
* **Flood limit:** `Flood` in the sender's launcher: more than 5 messages within 10 s and writing
  pauses for 30 s (`err.chat_too_fast|<s>`). Reactions, votes, closing and deleting do not
  count. A modified launcher can skip it.
* **Identity:** the peer id is an Ed25519 public key (`chat/crypto.rs`). Every event is signed;
  the body of a private event is sealed with a NaCl box (Curve25519 keys derived from the two
  ids), so only the two participants can read it, whoever else receives it. The envelope (who,
  to whom, when, nickname) stays readable. Events whose signature fails are dropped.
* **Conversations:** the public room, topics (`Body::Topic` opens one; public events carry its id
  in `Event::topic`, which the signature covers, so nobody can move a message into another topic)
  and private ones. The interface keys them `null`, `#<topic id>` and the peer id; `send_text`
  and friends take that key. Own text messages can be edited (`Body::Edit`, newest `seq` wins).
* **History:** everything is an immutable `Event` (`Text`, `React`, `Poll`, `PollOption`,
  `Vote`, `ClosePoll`, `Delete`) with id `<peer>:<seq>`. When two launchers meet, and every two
  minutes, each sends `Hello` with the ids it has and receives what it lacks — public events and
  their private conversation, minus what the asker no longer keeps (`since`). That is how a late arrival gets the backlog and how a private
  message to someone offline arrives later. For reactions and votes the highest `seq` of a person
  wins, so the order of arrival does not matter. `ChatState` folds events into `ItemView`s.
* **Relay:** `nll-chat-relay` (`crates/lanlauncher-core/src/bin/`) runs the same code with
  `ChatConfig::relay`. Its beacon says `relay`, launchers do not list it as a person; they send it
  private events only if `launcher.ini` names its id (`chat_relay`), since the beacon flag proves
  nothing; it keeps every signed event without folding it (`ChatState::insert_opaque`,
  private bodies stay sealed) and answers each `Hello` like any peer. No forwarding is needed on
  one flat network, where every launcher reaches every other directly.
* **Storage:** `<data>/chat/history.jsonl` (events this launcher got in the last five days,
  `KEEP_FOR`, at most the newest 5000, in their signed and sealed form). Older ones are dropped at
  start and every minute, so the chat of the last LAN is gone at the next. Arrival on the own
  clock decides, never the author's clock, which may be days off. Events travel with their age
  (`Frame::Events.ages`, a duration, so no clock has to be right), and a launcher that catches up
  late counts from when the event first reached the LAN, not from its own catch-up. Expired ids are kept in
  `expired.json` for another five days so a peer that got them later cannot hand them back. and `identity.json` (the secret key; the
  nickname is the player name).
* **Shell:** `src-tauri/src/chat.rs` starts and stops the chat with `settings.chatEnabled` and
  forwards changes as `chat-update`/`chat-reset`. The panel is `ChatPanel.svelte`; sounds are
  synthesised with WebAudio (`chat-sound.ts`).

## Diagnostics and fixes

`diagnostics.rs` produces `Problem { code, severity, params, steps, fix }`. The UI localises
by code (`problem.<code>.title/.cause`, `problem.<step>`). Command errors and fix results are
codes as well (`err.*`, `msg.*`, optional `|detail`) resolved by `userText()` in the frontend. `fixes.rs` executes `FixAction`s:
PowerShell `Set-NetConnectionProfile`, `netsh advfirewall` rules for all profiles, Defender
exclusions, transport restart, repair, open folder/URL.

## Directories

* Settings: `<config dir>/settings.json` (atomic writes).
* Data: `<data dir>/transport` (Resilio storage), `manifests/` (user overrides), `demo/`,
  `chat/` (history and chat identity).
* Cache: `<cache dir>/covers/<game_id>.jpg` from `assets.eti`.
* Library root(s): ETI layout `<root>/<game_id>/{<id>.eti, version.ini, game_start.cmd, local/}`.
