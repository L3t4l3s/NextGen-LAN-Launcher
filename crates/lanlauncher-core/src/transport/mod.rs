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
    pub lan_mode: bool,
    pub detail: Option<String>,
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
