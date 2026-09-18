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
    /// Startup, settings changes and manual restarts must never spawn engines
    /// concurrently against the same Resilio storage directory.
    pub transport_lifecycle: tokio::sync::Mutex<()>,
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
    /// Which entry point was started, for a repeat: `None` is the primary.
    pub alternative: Option<usize>,
    pub pid: Option<u32>,
    /// Error code (`err.…`) when the start itself failed.
    pub error: Option<String>,
    /// Exit code once the process ended; `None` while it still runs or when
    /// the system did not report one.
    pub exit_code: Option<i32>,
    /// The process has ended (with or without a code).
    pub ended: bool,
    /// What the program printed, as far as it was captured (the tail of the
    /// log file). Empty when it printed nothing.
    pub output: String,
    /// Output was written to a file at all. A normal start keeps its own
    /// console, so "nothing to show" and "nothing printed" are different
    /// answers and the page says which one it is.
    pub captured: bool,
    /// The file that output was captured into; `None` when nothing was.
    #[serde(skip)]
    pub log: Option<PathBuf>,
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
            alternative: None,
            pid: None,
            error: None,
            exit_code: None,
            ended: false,
            output: String::new(),
            captured: false,
            log: None,
        }
    }
}

impl AppState {
    /// Remember a launch attempt and, once the process ends, its exit code.
    /// The watcher only writes back while the record is still the current one.
    pub async fn record_launch(
        self: &std::sync::Arc<Self>,
        mut attempt: LaunchAttempt,
        log: Option<PathBuf>,
        outcome: lanlauncher_core::error::Result<(u32, lanlauncher_core::launch::ExitWatch)>,
    ) -> lanlauncher_core::error::Result<u32> {
        let stamp = attempt.at;
        // Whether a transcript exists, not whether one was asked for: the
        // file is created when the start begins to capture, so this is the
        // honest answer for both the ordinary and the elevated path.
        attempt.captured = log.as_ref().is_some_and(|p| p.is_file());
        attempt.log = log.clone();
        match outcome {
            Ok((pid, watch)) => {
                attempt.pid = Some(pid);
                *self.last_launch.write().await = Some(attempt);
                let st = self.clone();
                tauri::async_runtime::spawn(async move {
                    let code = watch.await.ok().flatten();
                    let output = log
                        .as_ref()
                        .map(|p| st.launch_output(p))
                        .unwrap_or_default();
                    let mut slot = st.last_launch.write().await;
                    if let Some(rec) = slot.as_mut().filter(|r| r.at == stamp) {
                        rec.exit_code = code;
                        rec.ended = true;
                        rec.output = output;
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

    /// Where a started program's output is captured. One file per game and
    /// kind: a game started while its server script is still running would
    /// otherwise truncate the file under the server's open handle.
    pub fn launch_log(&self, game_id: &str, what: &str) -> PathBuf {
        let safe = |s: &str| -> String {
            s.chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .take(48)
                .collect()
        };
        self.run_dir()
            .join(format!("start-{}-{}.log", safe(what), safe(game_id)))
    }

    /// The tail of one of those files, for the diagnostics page.
    /// The transcript only when it belongs to this run: a batch that never
    /// started (no rights, no file written) would otherwise be explained with
    /// the output of the run before it.
    pub fn launch_output_since(&self, path: &std::path::Path, started_ms: u64) -> String {
        let fresh = std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .is_some_and(|d| d.as_millis() as u64 + 2_000 >= started_ms);
        if fresh {
            self.launch_output(path)
        } else {
            String::new()
        }
    }

    pub fn launch_output(&self, path: &std::path::Path) -> String {
        use std::io::{Read, Seek, SeekFrom};
        const TAIL: usize = 8 * 1024;
        // Only the end of the file: a dedicated server writes for hours, and
        // this is read again every couple of seconds while it runs.
        let Ok(mut file) = std::fs::File::open(path) else {
            return String::new();
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if file
            .seek(SeekFrom::Start(len.saturating_sub(TAIL as u64)))
            .is_err()
        {
            return String::new();
        }
        let mut text = Vec::with_capacity(TAIL);
        if file.take(TAIL as u64).read_to_end(&mut text).is_err() {
            return String::new();
        }
        // A tail starts in the middle of a line, and possibly in the middle
        // of a character; the first line goes.
        let start = if len > TAIL as u64 {
            text.iter()
                .position(|b| *b == b'\n')
                .map(|i| i + 1)
                .unwrap_or(0)
        } else {
            0
        };
        // Windows scripts write in the console code page; whatever does not
        // decode is shown as the replacement character rather than dropping
        // the line it sits in.
        String::from_utf8_lossy(&text[start..]).trim().to_string()
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
