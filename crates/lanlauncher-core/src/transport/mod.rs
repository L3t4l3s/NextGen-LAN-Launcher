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
    pub bytes_done: u64,
    pub bytes_total: u64,
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
            (self.bytes_done as f64 / self.bytes_total as f64).clamp(0.0, 1.0)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportHealth {
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
    pub detail: Option<String>,
}

/// Peer counts split into "everything" and "the catalog share".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PeerSummary {
    pub total: u32,
    /// `None` when no catalog share is registered at all.
    pub catalog: Option<u32>,
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

/// Sum peers over all shares and, separately, over the catalog share.
pub fn peer_summary<'a>(shares: impl IntoIterator<Item = &'a ShareStatus>) -> PeerSummary {
    let mut out = PeerSummary::default();
    for s in shares {
        out.total += s.peers;
        if is_catalog_share(&s.dir) {
            out.catalog = Some(out.catalog.unwrap_or(0) + s.peers);
        }
    }
    out
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
/// engine may differ in separators, trailing slashes and (on Windows) case.
pub fn normalise_dir(path: &Path) -> String {
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
    use super::*;

    fn share(dir: &str, peers: u32) -> ShareStatus {
        ShareStatus {
            dir: PathBuf::from(dir),
            state: ShareState::Downloading,
            bytes_done: 0,
            bytes_total: 0,
            files_total: 0,
            peers,
            download_bps: 0,
            upload_bps: 0,
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
        assert_eq!(
            s,
            PeerSummary {
                total: 3,
                catalog: Some(1)
            }
        );

        let s = peer_summary([share("/lan/quake3", 2)].iter());
        assert_eq!(
            s,
            PeerSummary {
                total: 2,
                catalog: None
            }
        );

        let s = peer_summary([share("/lan/eti_launcher", 0)].iter());
        assert_eq!(
            s,
            PeerSummary {
                total: 0,
                catalog: Some(0)
            }
        );
    }
}
