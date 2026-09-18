//! Sync transport abstraction.
//!
//! ETI distributes games through Resilio Sync shares. The launcher never
//! trusts the transport's notion of "done" for gameplay decisions (see
//! [`crate::install`]), but it needs the transport to add/remove shares and to
//! display progress and peer information. Implementations:
//!
//! * [`resilio::ResilioTransport`] – runs and controls a Resilio Sync process.
//! * [`folder::FolderTransport`] – passive: the user runs a sync client; we only
//!   watch the folders and hand out keys.
//! * [`demo::DemoTransport`] – simulated transfers for UI development.

pub mod demo;
pub mod folder;
pub mod resilio;

use crate::catalog::ShareKey;
use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    Resilio,
    Folder,
    Demo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareState {
    /// Share known to the transport, no data yet or waiting for peers.
    Pending,
    Indexing,
    Downloading,
    /// Transport reports everything received.
    Complete,
    Paused,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareStatus {
    pub dir: PathBuf,
    pub state: ShareState,
    /// What the engine counts as finished — for Resilio the bytes of the
    /// files it has completed, so a share transferring one big package stays
    /// at the size of the little `version.ini` beside it until the end. That
    /// makes it a poor progress bar and a good answer to "may this be
    /// verified yet".
    pub bytes_done: u64,
    pub bytes_total: u64,
    /// What has arrived, finished or not: the engine's own figure, or the
    /// bytes its peers report having sent where those are further along.
    pub bytes_received: u64,
    /// Is `bytes_received` a figure the engine actually gave? The web UI
    /// answers for some shares with a state and nothing else, and a
    /// fabricated zero must not be read as "nothing has arrived".
    pub bytes_known: bool,
    /// Does `bytes_done` really mean "finished"? Only then does it answer
    /// whether an archive may be verified. The web UI's percentage covers
    /// the file in flight and cannot say, and where nobody can say, the disk
    /// decides.
    pub finished_known: bool,
    pub files_total: u64,
    pub peers: u32,
    pub download_bps: u64,
    pub upload_bps: u64,
    /// Transport-specific error text, if any.
    pub error: Option<String>,
}

impl ShareStatus {
    pub fn progress(&self) -> f64 {
        if self.bytes_total == 0 {
            if self.state == ShareState::Complete {
                1.0
            } else {
                0.0
            }
        } else {
            (self.bytes_received as f64 / self.bytes_total as f64).clamp(0.0, 1.0)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportActivity {
    Discovering,
    Indexing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportHealth {
    /// Normal preparation, not a fault. None after discovery or on failure.
    #[serde(default)]
    pub activity: Option<TransportActivity>,
    pub kind: TransportKind,
    pub running: bool,
    pub api_reachable: bool,
    pub version: Option<String>,
    /// Total connected peers across shares (best effort).
    pub peers: u32,
    /// Peers connected on the catalog share (`eti_launcher`) only.
    pub catalog_peers: u32,
    /// `Some(true)`: a sync server serves the catalog share; `Some(false)`: the
    /// share is registered but nobody offers it; `None`: cannot tell (folder
    /// mode, API failure, catalog share not registered).
    pub server_found: Option<bool>,
    pub lan_mode: bool,
    /// The transport can name the peers of a share (Resilio with an API key).
    #[serde(default)]
    pub peer_details: bool,
    pub detail: Option<String>,
    /// Current rates over all shares, for the status bar. Shown even at zero:
    /// "nothing is moving" is an answer too.
    #[serde(default)]
    pub download_bps: u64,
    #[serde(default)]
    pub upload_bps: u64,
    /// Address of the engine's web interface, credentials included, for
    /// "open in browser". The password is the random one from the engine
    /// config: it goes to the browser, never into the log.
    #[serde(default)]
    pub web_ui: Option<String>,
}

/// What the status bar shows every second: the current rates and how many
/// other PCs are around. Cheap enough to ask the engine for once a second,
/// unlike the full health (which also asks for the version and builds its
/// detail line).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TransportRates {
    pub download_bps: u64,
    pub upload_bps: u64,
}

impl From<PeerSummary> for TransportRates {
    fn from(s: PeerSummary) -> Self {
        Self {
            download_bps: s.download_bps,
            upload_bps: s.upload_bps,
        }
    }
}

/// Peer counts split into "everything" and "the catalog share".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PeerSummary {
    /// The most peers any one share reports: the same PC appears on every
    /// share it holds, so adding them up counts it many times over.
    pub total: u32,
    /// `None` when no catalog share is registered at all.
    pub catalog: Option<u32>,
    /// Rates over all shares. These *do* add up: each share transfers its own
    /// bytes.
    pub download_bps: u64,
    pub upload_bps: u64,
}

/// Is `dir` the catalog share (`<root>/eti_launcher`)? Trailing separators are
/// tolerated; Windows compares case-insensitively like [`normalise_dir`].
pub fn is_catalog_share(dir: &Path) -> bool {
    let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if cfg!(target_os = "windows") {
        name.eq_ignore_ascii_case(crate::paths::LAUNCHER_SHARE_ID)
    } else {
        name == crate::paths::LAUNCHER_SHARE_ID
    }
}

/// How many other PCs are around, plus the count on the catalog share.
///
/// The engine counts peers per share, and the same PC sits on many of them —
/// summing turned one sync server into "28 participants". Without peer ids
/// per share the most any single share sees is the honest figure.
pub fn peer_summary<'a>(shares: impl IntoIterator<Item = &'a ShareStatus>) -> PeerSummary {
    let mut out = PeerSummary::default();
    for s in shares {
        out.total = out.total.max(s.peers);
        if is_catalog_share(&s.dir) {
            out.catalog = Some(out.catalog.unwrap_or(0).max(s.peers));
        }
        out.download_bps += s.download_bps;
        out.upload_bps += s.upload_bps;
    }
    out
}

/// One peer of a share, for the "where does this come from" panel.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharePeer {
    /// Device name the peer announces.
    pub name: String,
    /// `direct`, `relay`, … when the engine reports it.
    pub connection: Option<String>,
    /// The peer has the whole share.
    pub synced: bool,
    /// Current rates, measured from the engine's counters between two polls;
    /// 0 until a second sample exists.
    pub download_bps: u64,
    pub upload_bps: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShareOptions {
    /// Restrict traffic to the LAN (no tracker/relay/DHT).
    pub lan_only: bool,
    /// Do not download until explicitly resumed.
    pub paused: bool,
}

#[async_trait]
pub trait Transport: Send + Sync {
    fn kind(&self) -> TransportKind;
    /// PID of the sync engine process this transport started, if any.
    fn process_id(&self) -> Option<u32> {
        None
    }
    /// Bring the transport up (start the process, wait for the API).
    async fn start(&self) -> Result<()>;
    /// Shut the transport down cleanly.
    async fn stop(&self) -> Result<()>;
    async fn health(&self) -> TransportHealth;
    async fn add_share(&self, key: &ShareKey, dir: &Path, opts: &ShareOptions) -> Result<()>;
    async fn remove_share(&self, dir: &Path) -> Result<()>;
    async fn set_paused(&self, dir: &Path, paused: bool) -> Result<()>;
    async fn share_status(&self, dir: &Path) -> Result<Option<ShareStatus>>;
    /// Current rates over all shares, asked for once a second. The default
    /// takes them from [`Transport::list_shares`]; a transport that can
    /// answer more cheaply than with its full share list overrides this.
    async fn rates(&self) -> Result<TransportRates> {
        Ok(peer_summary(self.list_shares().await?.iter()).into())
    }
    /// Forget whatever was cached about the shares. Called before an action
    /// that has to see the engine as it is right now (a repair the user just
    /// asked for), not as it was a moment ago.
    fn invalidate(&self) {}
    /// Peers of one share with their current rates. Empty when the transport
    /// cannot tell; only queried while the user looks at the detail panel.
    async fn share_peers(&self, _dir: &Path) -> Result<Vec<SharePeer>> {
        Ok(Vec::new())
    }
    async fn list_shares(&self) -> Result<Vec<ShareStatus>>;
    /// Switch between LAN-only and internet mode.
    async fn set_lan_mode(&self, lan_only: bool) -> Result<()>;
}

/// Resilio writes incomplete files as `<name>.!sync` and renames them when the
/// last block arrived. The presence of the final file therefore is a strong,
/// transport-independent completion signal.
pub const PARTIAL_SUFFIX: &str = ".!sync";

pub fn partial_path(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(PARTIAL_SUFFIX);
    PathBuf::from(s)
}

/// Directory key for matching engine-reported paths against our own: the
/// engine may differ in separators, trailing slashes, the Windows long-path
/// prefix (Resilio answers `\\?\C:\LAN\…` for a folder we registered as
/// `C:\LAN\…`) and in case.
pub fn normalise_dir(path: &Path) -> String {
    let path = crate::paths::strip_verbatim(path.to_path_buf());
    let s = path.to_string_lossy().replace('\\', "/");
    let s = s.trim_end_matches('/');
    if cfg!(target_os = "windows") {
        s.to_ascii_lowercase()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_windows_long_path_prefix_does_not_break_matching() {
        // Resilio reports the folder back with the prefix. Without this the
        // install state machine finds no share for the game and shows no
        // progress and no peers at all.
        assert_eq!(
            normalise_dir(Path::new(r"\\?\C:\LAN\eti_launcher")),
            normalise_dir(Path::new(r"C:\LAN\eti_launcher"))
        );
        assert_eq!(
            normalise_dir(Path::new(r"\\?\UNC\srv\lan\x")),
            normalise_dir(Path::new(r"\\srv\lan\x"))
        );
    }
    use super::*;

    fn share(dir: &str, peers: u32) -> ShareStatus {
        rated_share(dir, peers, 0, 0)
    }

    fn rated_share(dir: &str, peers: u32, down: u64, up: u64) -> ShareStatus {
        ShareStatus {
            dir: PathBuf::from(dir),
            state: ShareState::Downloading,
            bytes_done: 0,
            bytes_total: 0,
            bytes_received: 0,
            bytes_known: true,
            finished_known: true,
            files_total: 0,
            peers,
            download_bps: down,
            upload_bps: up,
            error: None,
        }
    }

    #[test]
    fn is_catalog_share_matches_last_component() {
        assert!(is_catalog_share(Path::new("/lan/eti_launcher")));
        assert!(is_catalog_share(Path::new("/lan/eti_launcher/")));
        assert!(!is_catalog_share(Path::new("/lan/eti_launcher_old")));
        assert!(!is_catalog_share(Path::new("/lan/quake3")));
        assert!(!is_catalog_share(Path::new("/eti_launcher/update")));
    }

    #[test]
    fn peer_summary_separates_catalog_peers() {
        let shares = [share("/lan/quake3", 2), share("/lan/eti_launcher", 1)];
        let s = peer_summary(shares.iter());
        assert_eq!(s.total, 2);
        assert_eq!(s.catalog, Some(1));

        let s = peer_summary([share("/lan/quake3", 2)].iter());
        assert_eq!(s.total, 2);
        assert_eq!(s.catalog, None);

        let s = peer_summary([share("/lan/eti_launcher", 0)].iter());
        assert_eq!(s.total, 0);
        assert_eq!(s.catalog, Some(0));
    }

    #[test]
    fn peer_summary_does_not_add_the_same_pc_up_once_per_share() {
        // One sync server, registered on 28 shares, is one participant — not
        // 28. Without peer ids the largest share count is the honest answer.
        let shares: Vec<_> = (0..28)
            .map(|i| share(&format!("/lan/game{i}"), 1))
            .collect();
        assert_eq!(peer_summary(shares.iter()).total, 1);
    }

    #[test]
    fn peer_summary_adds_the_rates_up() {
        // Rates are per share and do not overlap: each share moves its own
        // bytes, so the status bar wants the sum.
        let shares = [
            rated_share("/lan/quake3", 1, 1_000, 10),
            rated_share("/lan/eti_launcher", 1, 500, 0),
        ];
        let s = peer_summary(shares.iter());
        assert_eq!(s.download_bps, 1_500);
        assert_eq!(s.upload_bps, 10);
    }
}
