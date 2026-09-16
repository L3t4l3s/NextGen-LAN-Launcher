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
    /// Covers shipped with the app (`assets/covers` in the repository,
    /// `covers/` in the bundle); consulted after the cover cache.
    pub bundled_covers: Option<PathBuf>,
    /// Games currently running (game_id → pid), for the stats beacon.
    pub running: RwLock<Vec<(String, u32)>>,
    /// The last thing the launcher tried to start, for the diagnostics page.
    /// "A cmd window opens and nothing happens" is only answerable when the
    /// command line and the exit code are visible somewhere.
    pub last_launch: RwLock<Option<LaunchAttempt>>,
    pub transport_error: RwLock<Option<String>>,
    /// Size/mtime of `game.db` and `assets.eti` at the last successful catalog
    /// load, so the file watcher does not redo a load another path just did.
    pub catalog_sig: std::sync::Mutex<crate::CatalogSig>,
    /// Serialises catalog reloads: two at once would extract `assets.eti`
    /// into the same cover cache concurrently.
    pub catalog_reload: tokio::sync::Mutex<()>,
    /// Catalog loaded at start before the install manager exists (the sync
    /// engine may take its whole API timeout to come up); `catalog()` falls
    /// back to it so the library shows up right away.
    pub startup_catalog: RwLock<Option<Catalog>>,
}

/// What the launcher started (or failed to start) last, as shown on the
/// diagnostics page.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchAttempt {
    pub game_id: String,
    /// Catalog title when known, otherwise the id.
    pub title: String,
    /// What was started: the game itself or an extra (`keygen.exe`, server).
    pub what: String,
    /// Milliseconds since the epoch, formatted by the frontend.
    pub at: u64,
    /// `game_start.cmd`, `Wine 9.0`, …
    pub runner: String,
    pub program: String,
    /// The command line as it was passed on (raw string on Windows, otherwise
    /// the arguments joined for reading).
    pub command_line: String,
    pub cwd: String,
    /// The plan asked for administrator rights.
    pub elevated: bool,
    pub pid: Option<u32>,
    /// Error code (`err.…`) when the start itself failed.
    pub error: Option<String>,
    /// Exit code once the process ended; `None` while it still runs or when
    /// the system did not report one.
    pub exit_code: Option<i32>,
    /// The process has ended (with or without a code).
    pub ended: bool,
}

impl LaunchAttempt {
    /// Record the intent before spawning: everything but the outcome.
    pub fn new(
        game_id: &str,
        title: &str,
        what: &str,
        plan: &lanlauncher_core::launch::LaunchPlan,
    ) -> Self {
        let command_line = plan
            .raw_command_line
            .clone()
            .unwrap_or_else(|| plan.args.join(" "));
        Self {
            game_id: game_id.to_string(),
            title: title.to_string(),
            what: what.to_string(),
            at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            runner: plan.runner.clone(),
            program: plan.program.display().to_string(),
            command_line,
            cwd: plan.cwd.display().to_string(),
            elevated: plan.needs_elevation,
            pid: None,
            error: None,
            exit_code: None,
            ended: false,
        }
    }
}

impl AppState {
    /// Remember a launch attempt and, once the process ends, its exit code.
    /// The watcher only writes back while the record is still the current one.
    pub async fn record_launch(
        self: &std::sync::Arc<Self>,
        mut attempt: LaunchAttempt,
        outcome: lanlauncher_core::error::Result<(u32, lanlauncher_core::launch::ExitWatch)>,
    ) -> lanlauncher_core::error::Result<u32> {
        let stamp = attempt.at;
        match outcome {
            Ok((pid, watch)) => {
                attempt.pid = Some(pid);
                *self.last_launch.write().await = Some(attempt);
                let st = self.clone();
                tauri::async_runtime::spawn(async move {
                    let code = watch.await.ok().flatten();
                    let mut slot = st.last_launch.write().await;
                    if let Some(rec) = slot.as_mut().filter(|r| r.at == stamp) {
                        rec.exit_code = code;
                        rec.ended = true;
                    }
                });
                Ok(pid)
            }
            Err(e) => {
                attempt.error = Some(e.to_string());
                attempt.ended = true;
                *self.last_launch.write().await = Some(attempt);
                Err(e)
            }
        }
    }

    pub fn settings_path(&self) -> PathBuf {
        self.dirs.settings_file()
    }

    /// Batch files for elevated runs (see `launch::elevate`).
    pub fn run_dir(&self) -> PathBuf {
        self.dirs.data.join("run")
    }

    /// Cover directories in lookup order: cache first, bundled set second.
    pub fn cover_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.dirs.covers_dir()];
        dirs.extend(self.bundled_covers.clone());
        dirs
    }

    pub async fn effective_transport_mode(&self) -> TransportMode {
        if self.demo {
            TransportMode::Demo
        } else {
            self.settings.read().await.transport
        }
    }

    /// Default library root as configured in settings (the source of truth;
    /// `library` lags behind it by up to 2 s).
    pub async fn default_root_path(&self) -> Option<PathBuf> {
        self.settings
            .read()
            .await
            .library
            .default_root()
            .map(|r| r.path.clone())
    }

    pub async fn catalog(&self) -> Catalog {
        match self.manager.read().await.as_ref() {
            Some(m) => m.catalog().await,
            None => self
                .startup_catalog
                .read()
                .await
                .clone()
                .unwrap_or_default(),
        }
    }
}
