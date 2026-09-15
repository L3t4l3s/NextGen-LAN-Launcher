//! Well-known paths of the launcher and of the ETI library layout.
//!
//! ETI layout inside a library root (`X:\LAN` on Windows):
//!
//! ```text
//! <root>/eti_launcher/update/game.db      catalog
//! <root>/eti_launcher/update/assets.eti   cover art (tar)
//! <root>/<game_id>/<game_id>.eti          game package (RAR)
//! <root>/<game_id>/version.ini            package revision
//! <root>/<game_id>/game_start.cmd         Windows launch script
//! <root>/<game_id>/local/                 extracted game
//! ```

use std::path::{Path, PathBuf};

pub const LAUNCHER_SHARE_ID: &str = "eti_launcher";
pub const CATALOG_RELATIVE: &str = "eti_launcher/update/game.db";
pub const ASSETS_RELATIVE: &str = "eti_launcher/update/assets.eti";
pub const LOCAL_DIR: &str = "local";
pub const VERSION_FILE: &str = "version.ini";
pub const RECEIPT_FILE: &str = ".nll-install.json";

/// Application data directories (settings, logs, transport state, cover cache).
#[derive(Debug, Clone)]
pub struct AppDirs {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
    /// Where the log plugin writes `launcher.log`: the shell fills this from
    /// Tauri's app log dir (`%LOCALAPPDATA%\<id>\logs` on Windows,
    /// `~/Library/Logs/<id>` on macOS, `$XDG_DATA_HOME/<id>/logs` on Linux),
    /// so "open log folder" shows the real location.
    pub logs: PathBuf,
}

impl AppDirs {
    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    pub fn covers_dir(&self) -> PathBuf {
        self.cache.join("covers")
    }

    pub fn transport_dir(&self) -> PathBuf {
        self.data.join("transport")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.logs.clone()
    }
}

/// Paths of a single game inside a library root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamePaths {
    pub share_dir: PathBuf,
    pub archive: PathBuf,
    pub version_file: PathBuf,
    pub local_dir: PathBuf,
    pub receipt: PathBuf,
    pub start_script: PathBuf,
    pub setup_script: PathBuf,
}

impl GamePaths {
    pub fn new(library_root: &Path, game_id: &str) -> Self {
        let share_dir = library_root.join(game_id);
        Self {
            archive: share_dir.join(format!("{game_id}.eti")),
            version_file: share_dir.join(VERSION_FILE),
            local_dir: share_dir.join(LOCAL_DIR),
            receipt: share_dir.join(RECEIPT_FILE),
            start_script: share_dir.join("game_start.cmd"),
            setup_script: share_dir.join("game_setup.cmd"),
            share_dir,
        }
    }
}
