//! Shared application state behind Tauri's `State<>`.

use lanlauncher_core::catalog::Catalog;
use lanlauncher_core::install::InstallManager;
use lanlauncher_core::lanpage::EventBundle;
use lanlauncher_core::manifest::ManifestStore;
use lanlauncher_core::paths::AppDirs;
use lanlauncher_core::settings::{Settings, TransportMode};
use lanlauncher_core::transport::Transport;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AppState {
    /// Library roots shared with the install manager's path resolver; kept in
    /// sync with `settings.library` by a background loop.
    pub library: Arc<std::sync::RwLock<lanlauncher_core::library::Library>>,
    pub dirs: AppDirs,
    pub demo: bool,
    pub settings: RwLock<Settings>,
    pub transport: RwLock<Option<Arc<dyn Transport>>>,
    pub manager: RwLock<Option<Arc<InstallManager>>>,
    pub event: RwLock<EventBundle>,
    pub manifests: ManifestStore,
    pub resource_dir: Option<PathBuf>,
    /// Games currently running (game_id → pid), for the stats beacon.
    pub running: RwLock<Vec<(String, u32)>>,
    pub transport_error: RwLock<Option<String>>,
}

impl AppState {
    pub fn settings_path(&self) -> PathBuf {
        self.dirs.settings_file()
    }

    pub async fn effective_transport_mode(&self) -> TransportMode {
        if self.demo {
            TransportMode::Demo
        } else {
            self.settings.read().await.transport
        }
    }

    pub async fn catalog(&self) -> Catalog {
        match self.manager.read().await.as_ref() {
            Some(m) => m.catalog().await,
            None => Catalog::default(),
        }
    }
}
