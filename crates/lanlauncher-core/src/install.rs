//! Install state machine.
//!
//! The one rule that fixes the "stuck at 99 %" problem of the old launcher:
//! **a game is playable when its data is verified on disk, never because the
//! sync engine says so.** Transport progress is displayed, but the transition
//! to "Ready" is driven by:
//!
//! 1. `<id>.eti` exists (Resilio renames `<id>.eti.!sync` when complete),
//! 2. its size has been stable for a few seconds,
//! 3. `version.ini` is present,
//! 4. the archive passes a full CRC test,
//! 5. extraction into `local/` succeeded and required files exist,
//! 6. a receipt with the installed revision is written.
//!
//! [`Tracker::step`] is a pure function over an [`Observation`] so every
//! scenario (engine stuck at 99 %, incomplete archive, update arrival, repair)
//! is unit-tested without a sync engine. [`InstallManager`] drives trackers
//! against a real [`Transport`] and runs the blocking archive work.

use crate::catalog::{Catalog, Game};
use crate::error::{Error, Result};
use crate::extract::{self, Verification};
use crate::manifest::Manifest;
use crate::paths::GamePaths;
use crate::problem::{FixAction, Problem, Severity};
use crate::transport::{partial_path, ShareOptions, ShareState, ShareStatus, Transport};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Mutex, RwLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    NotInstalled,
    /// Install requested, share being added.
    Queued,
    Syncing,
    /// Archive complete on disk, CRC test running.
    Verifying,
    Extracting,
    /// Post-extraction steps (setup script / manifest steps).
    Setup,
    Ready,
    /// Installed and playable, but the catalog has a newer package revision.
    UpdateAvailable,
    Paused,
    Failed,
}

impl Phase {
    pub fn is_playable(self) -> bool {
        matches!(self, Phase::Ready | Phase::UpdateAvailable)
    }
    pub fn is_busy(self) -> bool {
        matches!(
            self,
            Phase::Queued | Phase::Syncing | Phase::Verifying | Phase::Extracting | Phase::Setup
        )
    }
}

/// Written to `<share>/.nll-install.json` after a successful install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub version: u32,
    pub game_id: String,
    /// Package revision that was extracted (content of version.ini).
    pub revision: String,
    pub installed_at: DateTime<Utc>,
    pub archive_bytes: u64,
    pub files: u64,
    /// Windows `game_setup.cmd` / manifest setup steps finished.
    pub setup_done: bool,
    /// User-chosen executable when the manifest could not decide.
    #[serde(default)]
    pub exe_override: Option<String>,
}

impl Receipt {
    pub fn load(path: &Path) -> Option<Receipt> {
        let text = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&text).ok()
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)
            .map_err(|e| Error::io(&tmp, e))?;
        std::fs::rename(&tmp, path).map_err(|e| Error::io(path, e))
    }
}

/// Everything the state machine looks at for one game at one instant.
#[derive(Debug, Clone, Default)]
pub struct Observation {
    pub catalog_revision: String,
    pub catalog_bytes: u64,
    pub archive_len: Option<u64>,
    pub partial_len: Option<u64>,
    pub version_ini: Option<String>,
    pub receipt: Option<Receipt>,
    /// `local/` exists (extraction happened and was not deleted by hand).
    pub local_present: bool,
    /// All manifest `required_files` exist (advisory: a mismatch after a
    /// successful extraction yields a warning, never a re-download).
    pub required_files_ok: bool,
    pub transport: Option<ShareStatus>,
    pub disk_free: Option<u64>,
}

impl Observation {
    /// Gather the on-disk part of an observation.
    pub fn from_disk(
        paths: &GamePaths,
        game: &Game,
        required_files: &[String],
        disks: &crate::library::DiskTable,
    ) -> Self {
        let archive_len = std::fs::metadata(&paths.archive)
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len());
        let partial_len = std::fs::metadata(partial_path(&paths.archive))
            .ok()
            .map(|m| m.len());
        let version_ini = std::fs::read_to_string(&paths.version_file)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s.len() <= 64);
        let receipt = Receipt::load(&paths.receipt);
        let local_present = paths.local_dir.is_dir();
        let required_files_ok = local_present
            && if required_files.is_empty() {
                std::fs::read_dir(&paths.local_dir)
                    .map(|mut d| d.next().is_some())
                    .unwrap_or(false)
            } else {
                required_files
                    .iter()
                    .all(|f| paths.local_dir.join(f.replace('\\', "/")).exists())
            };
        Self {
            catalog_revision: game.revision.clone(),
            catalog_bytes: game.size_bytes,
            archive_len,
            partial_len,
            version_ini,
            receipt,
            local_present,
            required_files_ok,
            transport: None,
            disk_free: disks.free_for(&paths.share_dir),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Policy {
    /// Archive size must not change for this long before we verify.
    pub stable_for: Duration,
    /// No byte progress for this long → report a stall problem.
    pub stall_after: Duration,
    /// After a failed verification, retry no sooner than this (or when the
    /// archive size changes).
    pub verify_retry_after: Duration,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            stable_for: Duration::from_secs(15),
            stall_after: Duration::from_secs(120),
            verify_retry_after: Duration::from_secs(120),
        }
    }
}

/// What the manager should do after a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Verify,
    Extract,
    Setup,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameStatus {
    pub game_id: String,
    pub phase: Phase,
    /// 0.0–1.0 for the current phase (sync or extraction).
    pub progress: f64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub peers: u32,
    pub download_bps: u64,
    pub installed_revision: Option<String>,
    pub catalog_revision: String,
    /// Bytes have not changed for longer than the stall policy.
    pub stalled: bool,
    pub problem: Option<Problem>,
    /// Present when the user has to pick an executable (macOS/Linux).
    pub needs_exe_choice: bool,
    pub updated_at: DateTime<Utc>,
}

/// Per-game bookkeeping between observations.
#[derive(Debug, Clone)]
pub struct Tracker {
    pub game_id: String,
    pub phase: Phase,
    /// The user asked for this game (persisted by the manager as a queued share).
    pub wanted: bool,
    last_archive_len: Option<u64>,
    archive_stable_since: Option<Instant>,
    last_bytes: u64,
    last_progress_at: Instant,
    verify_failed_at: Option<Instant>,
    verify_failed_len: Option<u64>,
    pub problem: Option<Problem>,
    pub work_progress: f64,
    pub work_started_at: Option<Instant>,
}

impl Tracker {
    pub fn new(game_id: &str) -> Self {
        Self {
            game_id: game_id.to_string(),
            phase: Phase::NotInstalled,
            wanted: false,
            last_archive_len: None,
            archive_stable_since: None,
            last_bytes: 0,
            last_progress_at: Instant::now(),
            verify_failed_at: None,
            verify_failed_len: None,
            problem: None,
            work_progress: 0.0,
            work_started_at: None,
        }
    }

    pub fn request_install(&mut self) {
        self.wanted = true;
        if !self.phase.is_busy() && !self.phase.is_playable() {
            self.phase = Phase::Queued;
        }
        self.problem = None;
        self.last_progress_at = Instant::now();
    }

    /// Repair: forget verification failures and re-run the pipeline from
    /// whatever is on disk. Works from any phase, including Ready.
    pub fn request_repair(&mut self) {
        self.wanted = true;
        self.phase = Phase::Syncing;
        self.problem = None;
        self.verify_failed_at = None;
        self.verify_failed_len = None;
        self.archive_stable_since = None;
        // Force the stability timer to restart on the next observation.
        self.last_archive_len = None;
        self.last_progress_at = Instant::now();
    }

    pub fn set_paused(&mut self, paused: bool) {
        if paused && self.phase == Phase::Syncing {
            self.phase = Phase::Paused;
        } else if !paused && self.phase == Phase::Paused {
            self.phase = Phase::Syncing;
            self.last_progress_at = Instant::now();
        }
    }

    /// Missing manifest files after a successful extraction are reported,
    /// never "fixed" by re-downloading: the manifest may simply not match this
    /// package revision.
    fn check_required_files(&mut self, obs: &Observation) {
        const CODE: &str = "install.required_files_missing";
        if obs.required_files_ok {
            if self
                .problem
                .as_ref()
                .map(|p| p.code == CODE)
                .unwrap_or(false)
            {
                self.problem = None;
            }
        } else if self.problem.is_none() {
            self.problem = Some(
                Problem::new(CODE, Severity::Warning)
                    .step("install.required_files_missing.step.choose_exe")
                    .step("install.required_files_missing.step.repair")
                    .with_fix(FixAction::RepairGame {
                        game_id: self.game_id.clone(),
                    }),
            );
        }
    }

    pub fn fail(&mut self, problem: Problem) {
        self.phase = Phase::Failed;
        self.problem = Some(problem);
    }

    pub fn verification_failed(&mut self, detail: String, archive_len: Option<u64>) {
        self.phase = Phase::Syncing;
        self.verify_failed_at = Some(Instant::now());
        self.verify_failed_len = archive_len;
        self.archive_stable_since = None;
        self.problem = Some(
            Problem::new("install.archive_incomplete", Severity::Warning)
                .param("detail", detail)
                .step("install.archive_incomplete.step.wait")
                .step("install.archive_incomplete.step.repair")
                .with_fix(FixAction::RepairGame {
                    game_id: self.game_id.clone(),
                }),
        );
    }

    /// Pure decision step. `now` is injectable for tests.
    pub fn step(&mut self, obs: &Observation, policy: &Policy, now: Instant) -> Action {
        // 1. Installed state from the receipt wins for playability.
        if !self.phase.is_busy() || self.phase == Phase::Queued {
            if let Some(r) = &obs.receipt {
                if obs.local_present && !matches!(self.phase, Phase::Failed | Phase::Paused) {
                    let newer_in_catalog = r.revision != obs.catalog_revision;
                    let newer_on_disk = obs
                        .version_ini
                        .as_deref()
                        .map(|v| v != r.revision)
                        .unwrap_or(false);
                    self.phase = if newer_in_catalog || newer_on_disk {
                        Phase::UpdateAvailable
                    } else {
                        Phase::Ready
                    };
                    self.check_required_files(obs);
                    return Action::None;
                }
            }
            if self.phase == Phase::NotInstalled && !self.wanted {
                return Action::None;
            }
        }

        // Track byte progress for stall detection and display.
        let bytes_now = obs
            .transport
            .as_ref()
            .filter(|t| t.bytes_done > 0)
            .map(|t| t.bytes_done)
            .unwrap_or(obs.archive_len.or(obs.partial_len).unwrap_or(0));
        if bytes_now != self.last_bytes {
            self.last_bytes = bytes_now;
            self.last_progress_at = now;
        }
        // Archive stability.
        if obs.archive_len != self.last_archive_len {
            self.last_archive_len = obs.archive_len;
            self.archive_stable_since = obs.archive_len.map(|_| now);
        }

        match self.phase {
            Phase::Queued => {
                self.phase = Phase::Syncing;
                Action::None
            }
            Phase::Syncing => {
                if let Some(free) = obs.disk_free {
                    let needed = obs
                        .catalog_bytes
                        .saturating_mul(2)
                        .saturating_sub(bytes_now);
                    if obs.archive_len.is_none() && free < needed && needed > 0 {
                        self.problem = Some(
                            Problem::new("install.disk_full", Severity::Error)
                                .param("free_bytes", free)
                                .param("needed_bytes", needed)
                                .step("install.disk_full.step.free")
                                .step("install.disk_full.step.other_root"),
                        );
                    }
                }
                let complete_on_disk = obs.archive_len.is_some()
                    && obs.partial_len.is_none()
                    && obs.version_ini.is_some()
                    && self
                        .archive_stable_since
                        .map(|t| now.duration_since(t) >= policy.stable_for)
                        .unwrap_or(false);
                let transport_complete = obs.archive_len.is_some()
                    && obs.partial_len.is_none()
                    && obs
                        .transport
                        .as_ref()
                        .map(|t| t.state == ShareState::Complete)
                        .unwrap_or(false);
                if complete_on_disk || transport_complete {
                    let retry_ok = match (self.verify_failed_at, self.verify_failed_len) {
                        (Some(t), len) => {
                            len != obs.archive_len
                                || now.duration_since(t) >= policy.verify_retry_after
                        }
                        (None, _) => true,
                    };
                    if retry_ok {
                        self.phase = Phase::Verifying;
                        self.work_progress = 0.0;
                        self.work_started_at = Some(now);
                        return Action::Verify;
                    }
                }
                let stalled = now.duration_since(self.last_progress_at) >= policy.stall_after;
                if stalled
                    && self
                        .problem
                        .as_ref()
                        .map(|p| !p.code.starts_with("install.archive_incomplete"))
                        .unwrap_or(true)
                {
                    let peers = obs.transport.as_ref().map(|t| t.peers).unwrap_or(0);
                    let code = if obs.transport.is_some() && peers == 0 {
                        "sync.no_peers"
                    } else {
                        "sync.stalled"
                    };
                    self.problem = Some(
                        Problem::new(code, Severity::Warning)
                            .param(
                                "minutes",
                                (now.duration_since(self.last_progress_at).as_secs() / 60).max(1),
                            )
                            .step(format!("{code}.step.1"))
                            .step(format!("{code}.step.2"))
                            .with_fix(FixAction::RepairGame {
                                game_id: self.game_id.clone(),
                            }),
                    );
                } else if !stalled
                    && self
                        .problem
                        .as_ref()
                        .map(|p| p.code.starts_with("sync."))
                        .unwrap_or(false)
                {
                    self.problem = None;
                }
                Action::None
            }
            Phase::Verifying => Action::Verify,
            Phase::Extracting => Action::Extract,
            Phase::Setup => Action::Setup,
            Phase::Paused | Phase::Failed | Phase::NotInstalled => Action::None,
            Phase::Ready | Phase::UpdateAvailable => {
                // Receipt vanished or `local/` deleted by hand → back to syncing.
                if obs.receipt.is_none() || !obs.local_present {
                    self.phase = Phase::Syncing;
                    self.archive_stable_since = None;
                    self.last_archive_len = None;
                } else {
                    self.check_required_files(obs);
                }
                Action::None
            }
        }
    }

    pub fn status(
        &self,
        obs: &Observation,
        policy: &Policy,
        now: Instant,
        needs_exe_choice: bool,
    ) -> GameStatus {
        let total = obs
            .transport
            .as_ref()
            .map(|t| t.bytes_total)
            .filter(|t| *t > 0)
            .unwrap_or(obs.catalog_bytes.max(1));
        let done = match self.phase {
            Phase::Ready | Phase::UpdateAvailable => total,
            _ => obs
                .transport
                .as_ref()
                .filter(|t| t.bytes_done > 0)
                .map(|t| t.bytes_done)
                .unwrap_or(obs.archive_len.or(obs.partial_len).unwrap_or(0)),
        };
        let progress = match self.phase {
            Phase::Verifying | Phase::Extracting | Phase::Setup => self.work_progress,
            Phase::Ready | Phase::UpdateAvailable => 1.0,
            Phase::NotInstalled => 0.0,
            _ => (done as f64 / total as f64).clamp(0.0, 0.999),
        };
        GameStatus {
            game_id: self.game_id.clone(),
            phase: self.phase,
            progress,
            bytes_done: done.min(total),
            bytes_total: total,
            peers: obs.transport.as_ref().map(|t| t.peers).unwrap_or(0),
            download_bps: obs.transport.as_ref().map(|t| t.download_bps).unwrap_or(0),
            installed_revision: obs.receipt.as_ref().map(|r| r.revision.clone()),
            catalog_revision: obs.catalog_revision.clone(),
            stalled: self.phase == Phase::Syncing
                && now.duration_since(self.last_progress_at) >= policy.stall_after,
            problem: self.problem.clone(),
            needs_exe_choice,
            updated_at: Utc::now(),
        }
    }
}

/// Hooks the manager calls for platform-specific post-extraction work.
#[async_trait::async_trait]
pub trait SetupHook: Send + Sync {
    /// Run after extraction; return an error to fail the install.
    async fn run_setup(&self, paths: &GamePaths, manifest: Option<&Manifest>) -> Result<()>;
}

/// Default hook: applies manifest copy/touch steps only.
pub struct ManifestSetupHook;

#[async_trait::async_trait]
impl SetupHook for ManifestSetupHook {
    async fn run_setup(&self, paths: &GamePaths, manifest: Option<&Manifest>) -> Result<()> {
        apply_manifest_setup(paths, manifest)
    }
}

pub fn apply_manifest_setup(paths: &GamePaths, manifest: Option<&Manifest>) -> Result<()> {
    let Some(m) = manifest else { return Ok(()) };
    for c in &m.setup.copy {
        let from = paths.local_dir.join(c.from.replace('\\', "/"));
        let to = paths.local_dir.join(c.to.replace('\\', "/"));
        if from.is_file() {
            if let Some(p) = to.parent() {
                std::fs::create_dir_all(p).map_err(|e| Error::io(p, e))?;
            }
            std::fs::copy(&from, &to).map_err(|e| Error::io(&to, e))?;
        }
    }
    for t in &m.setup.touch {
        let p = paths.local_dir.join(t.replace('\\', "/"));
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        if !p.exists() {
            std::fs::write(&p, b"").map_err(|e| Error::io(&p, e))?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InstallEvent {
    Status(GameStatus),
}

type PathResolver = Box<dyn Fn(&Game) -> Option<GamePaths> + Send + Sync>;
type ManifestResolver = Box<dyn Fn(&Game, &GamePaths) -> Option<Manifest> + Send + Sync>;

struct ActiveWork {
    cancel: Arc<AtomicBool>,
    handle: tokio::task::JoinHandle<()>,
    /// Ties results to the job that produced them; a stale result from a
    /// cancelled job must never touch the current job's bookkeeping.
    generation: u64,
}

/// Drives trackers against a transport.
pub struct InstallManager {
    transport: Arc<dyn Transport>,
    catalog: RwLock<Catalog>,
    trackers: Mutex<HashMap<String, Tracker>>,
    work: Mutex<HashMap<String, ActiveWork>>,
    generation: std::sync::atomic::AtomicU64,
    progress: Arc<std::sync::Mutex<HashMap<String, f64>>>,
    results: Arc<std::sync::Mutex<Vec<WorkResult>>>,
    pub policy: Policy,
    pub lan_only: bool,
    resolve_paths: PathResolver,
    resolve_manifest: ManifestResolver,
    setup_hook: Arc<dyn SetupHook>,
    events: broadcast::Sender<InstallEvent>,
}

enum WorkResult {
    Verified {
        game_id: String,
        generation: u64,
        result: Result<Verification>,
        archive_len: Option<u64>,
    },
    Extracted {
        game_id: String,
        generation: u64,
        result: Result<u64>,
        archive_len: u64,
        revision: String,
    },
    SetupDone {
        game_id: String,
        generation: u64,
        result: Result<()>,
    },
}

impl WorkResult {
    fn key(&self) -> (&str, u64) {
        match self {
            WorkResult::Verified {
                game_id,
                generation,
                ..
            }
            | WorkResult::Extracted {
                game_id,
                generation,
                ..
            }
            | WorkResult::SetupDone {
                game_id,
                generation,
                ..
            } => (game_id, *generation),
        }
    }
}

impl InstallManager {
    pub fn new(
        transport: Arc<dyn Transport>,
        catalog: Catalog,
        resolve_paths: impl Fn(&Game) -> Option<GamePaths> + Send + Sync + 'static,
        resolve_manifest: impl Fn(&Game, &GamePaths) -> Option<Manifest> + Send + Sync + 'static,
        setup_hook: Arc<dyn SetupHook>,
    ) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            transport,
            catalog: RwLock::new(catalog),
            trackers: Mutex::new(HashMap::new()),
            work: Mutex::new(HashMap::new()),
            generation: std::sync::atomic::AtomicU64::new(0),
            progress: Arc::new(std::sync::Mutex::new(HashMap::new())),
            results: Arc::new(std::sync::Mutex::new(Vec::new())),
            policy: Policy::default(),
            lan_only: true,
            resolve_paths: Box::new(resolve_paths),
            resolve_manifest: Box::new(resolve_manifest),
            setup_hook,
            events,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<InstallEvent> {
        self.events.subscribe()
    }

    pub async fn set_catalog(&self, catalog: Catalog) {
        *self.catalog.write().await = catalog;
    }

    pub async fn catalog(&self) -> Catalog {
        self.catalog.read().await.clone()
    }

    async fn game(&self, id: &str) -> Result<Game> {
        self.catalog
            .read()
            .await
            .game(id)
            .cloned()
            .ok_or_else(|| Error::Catalog(format!("unknown game `{id}`")))
    }

    fn paths_for(&self, game: &Game) -> Result<GamePaths> {
        (self.resolve_paths)(game)
            .ok_or_else(|| Error::Settings("no library root configured".into()))
    }

    /// Start (or resume) installing a game.
    pub async fn install(&self, game_id: &str) -> Result<()> {
        let game = self.game(game_id).await?;
        let paths = self.paths_for(&game)?;
        std::fs::create_dir_all(&paths.share_dir).map_err(|e| Error::io(&paths.share_dir, e))?;
        self.transport
            .add_share(
                &game.key,
                &paths.share_dir,
                &ShareOptions {
                    lan_only: self.lan_only,
                    paused: false,
                },
            )
            .await?;
        let mut trackers = self.trackers.lock().await;
        trackers
            .entry(game_id.to_string())
            .or_insert_with(|| Tracker::new(game_id))
            .request_install();
        Ok(())
    }

    /// Repair from any phase: stop running work, re-add the share, re-verify.
    pub async fn repair(&self, game_id: &str) -> Result<()> {
        self.cancel_work(game_id).await;
        let game = self.game(game_id).await?;
        let paths = self.paths_for(&game)?;
        let _ = self.transport.set_paused(&paths.share_dir, false).await;
        let _ = std::fs::remove_dir_all(extract::staging_dir(&paths.local_dir));
        let _ = self
            .transport
            .add_share(
                &game.key,
                &paths.share_dir,
                &ShareOptions {
                    lan_only: self.lan_only,
                    paused: false,
                },
            )
            .await;
        let mut trackers = self.trackers.lock().await;
        trackers
            .entry(game_id.to_string())
            .or_insert_with(|| Tracker::new(game_id))
            .request_repair();
        Ok(())
    }

    pub async fn set_paused(&self, game_id: &str, paused: bool) -> Result<()> {
        let game = self.game(game_id).await?;
        let paths = self.paths_for(&game)?;
        self.transport.set_paused(&paths.share_dir, paused).await?;
        if let Some(t) = self.trackers.lock().await.get_mut(game_id) {
            t.set_paused(paused);
        }
        Ok(())
    }

    /// Remove the share from the transport and delete all local data.
    pub async fn uninstall(&self, game_id: &str) -> Result<()> {
        self.cancel_work(game_id).await;
        let game = self.game(game_id).await?;
        let paths = self.paths_for(&game)?;
        let _ = self.transport.remove_share(&paths.share_dir).await;
        if paths.share_dir.is_dir() {
            std::fs::remove_dir_all(&paths.share_dir)
                .map_err(|e| Error::io(&paths.share_dir, e))?;
        }
        self.trackers.lock().await.remove(game_id);
        Ok(())
    }

    async fn cancel_work(&self, game_id: &str) {
        if let Some(w) = self.work.lock().await.remove(game_id) {
            w.cancel.store(true, Ordering::Relaxed);
            w.handle.abort();
        }
    }

    /// Discover games that already have files on disk (previous runs).
    pub async fn adopt_existing(&self) {
        let catalog = self.catalog.read().await.clone();
        let mut trackers = self.trackers.lock().await;
        for game in &catalog.games {
            let Some(paths) = (self.resolve_paths)(game) else {
                continue;
            };
            if paths.share_dir.is_dir() && !trackers.contains_key(&game.id) {
                let mut t = Tracker::new(&game.id);
                let has_data = paths.archive.exists()
                    || partial_path(&paths.archive).exists()
                    || paths.receipt.exists();
                if has_data {
                    t.wanted = true;
                    t.phase = Phase::Syncing;
                    let _ = self
                        .transport
                        .add_share(
                            &game.key,
                            &paths.share_dir,
                            &ShareOptions {
                                lan_only: self.lan_only,
                                paused: false,
                            },
                        )
                        .await;
                }
                trackers.insert(game.id.clone(), t);
            }
        }
    }

    /// One polling round: observe every tracked game, step the state machine,
    /// start blocking work when needed, publish statuses.
    pub async fn tick(&self) -> Vec<GameStatus> {
        let now = Instant::now();
        let catalog = self.catalog.read().await.clone();
        let share_statuses: HashMap<_, _> = self
            .transport
            .list_shares()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|s| (crate::transport::normalise_dir(&s.dir), s))
            .collect();
        let results: Vec<WorkResult> = self
            .results
            .lock()
            .map(|mut r| std::mem::take(&mut *r))
            .unwrap_or_default();
        let progress_snapshot: HashMap<String, f64> =
            self.progress.lock().map(|p| p.clone()).unwrap_or_default();

        let disks = crate::library::DiskTable::refresh();
        let mut out = Vec::new();
        let mut trackers = self.trackers.lock().await;

        for r in results {
            let (rid, rgen) = r.key();
            let current = self.work.lock().await.get(rid).map(|w| w.generation);
            if current != Some(rgen) {
                log::info!("tick: dropping stale work result for {rid} (gen {rgen})");
                continue;
            }
            match r {
                WorkResult::Verified {
                    game_id,
                    result,
                    archive_len,
                    ..
                } => {
                    self.work.lock().await.remove(&game_id);
                    if let Some(t) = trackers.get_mut(&game_id) {
                        match result {
                            Ok(Verification::Ok(summary)) => {
                                if !summary.unsafe_entries.is_empty() {
                                    t.fail(
                                        Problem::new("install.unsafe_archive", Severity::Error)
                                            .param("entries", summary.unsafe_entries.join(", ")),
                                    );
                                } else {
                                    t.phase = Phase::Extracting;
                                    t.work_progress = 0.0;
                                }
                            }
                            Ok(Verification::Damaged { detail, .. }) => {
                                t.verification_failed(detail, archive_len)
                            }
                            Ok(Verification::NotAnArchive { detail }) => {
                                t.verification_failed(detail, archive_len)
                            }
                            Err(e) if e.to_string().contains("cancelled") => {}
                            Err(e) => t.fail(
                                Problem::new("install.verify_error", Severity::Error)
                                    .param("detail", e.to_string()),
                            ),
                        }
                    }
                }
                WorkResult::Extracted {
                    game_id,
                    result,
                    archive_len,
                    revision,
                    ..
                } => {
                    self.work.lock().await.remove(&game_id);
                    if let Some(t) = trackers.get_mut(&game_id) {
                        match result {
                            Ok(files) => {
                                if let Some(game) = catalog.game(&game_id) {
                                    if let Some(paths) = (self.resolve_paths)(game) {
                                        let receipt = Receipt {
                                            version: 1,
                                            game_id: game_id.clone(),
                                            revision,
                                            installed_at: Utc::now(),
                                            archive_bytes: archive_len,
                                            files,
                                            setup_done: false,
                                            exe_override: None,
                                        };
                                        if let Err(e) = receipt.save(&paths.receipt) {
                                            t.fail(
                                                Problem::new(
                                                    "install.receipt_error",
                                                    Severity::Error,
                                                )
                                                .param("detail", e.to_string()),
                                            );
                                            continue;
                                        }
                                    }
                                }
                                t.phase = Phase::Setup;
                                t.work_progress = 0.0;
                            }
                            Err(e) if e.to_string().contains("cancelled") => {}
                            Err(e) => {
                                let code = if e.to_string().to_lowercase().contains("space") {
                                    "install.disk_full"
                                } else {
                                    "install.extract_error"
                                };
                                t.fail(
                                    Problem::new(code, Severity::Error)
                                        .param("detail", e.to_string())
                                        .with_fix(FixAction::RepairGame {
                                            game_id: game_id.clone(),
                                        }),
                                );
                            }
                        }
                    }
                }
                WorkResult::SetupDone {
                    game_id, result, ..
                } => {
                    self.work.lock().await.remove(&game_id);
                    if let Some(t) = trackers.get_mut(&game_id) {
                        match result {
                            Ok(()) => {
                                if let Some(game) = catalog.game(&game_id) {
                                    if let Some(paths) = (self.resolve_paths)(game) {
                                        if let Some(mut r) = Receipt::load(&paths.receipt) {
                                            r.setup_done = true;
                                            let _ = r.save(&paths.receipt);
                                        }
                                    }
                                }
                                t.phase = Phase::Ready;
                                t.problem = None;
                            }
                            Err(e) => {
                                // Setup problems must not hide a playable game: mark ready with a warning.
                                t.phase = Phase::Ready;
                                t.problem = Some(
                                    Problem::new("install.setup_failed", Severity::Warning)
                                        .param("detail", e.to_string()),
                                );
                            }
                        }
                    }
                }
            }
        }

        let ids: Vec<String> = trackers.keys().cloned().collect();
        for id in ids {
            let Some(game) = catalog.game(&id).cloned() else {
                continue;
            };
            let Some(paths) = (self.resolve_paths)(&game) else {
                continue;
            };
            let manifest = (self.resolve_manifest)(&game, &paths);
            let required: Vec<String> = manifest
                .as_ref()
                .map(|m| m.launch_for(Manifest::current_platform()).required_files)
                .unwrap_or_default();
            let mut obs = Observation::from_disk(&paths, &game, &required, &disks);
            obs.transport = share_statuses
                .get(&crate::transport::normalise_dir(&paths.share_dir))
                .cloned();
            let tracker = trackers.get_mut(&id).expect("tracker exists");
            if let Some(p) = progress_snapshot.get(&id) {
                tracker.work_progress = *p;
            }
            let action = tracker.step(&obs, &self.policy, now);
            let busy = self.work.lock().await.contains_key(&id);
            if action != Action::None {
                log::info!(
                    "tick: {} phase={:?} action={:?} busy={}",
                    id,
                    tracker.phase,
                    action,
                    busy
                );
            }
            if !busy {
                match action {
                    Action::Verify => self.spawn_verify(&id, &paths, obs.archive_len).await,
                    Action::Extract => {
                        let revision = obs
                            .version_ini
                            .clone()
                            .unwrap_or_else(|| game.revision.clone());
                        self.spawn_extract(&id, &paths, obs.archive_len.unwrap_or(0), revision)
                            .await
                    }
                    Action::Setup => self.spawn_setup(&id, &paths, manifest.clone()).await,
                    Action::Ready | Action::None => {}
                }
            }
            let needs_exe_choice = tracker.phase.is_playable()
                && !cfg!(target_os = "windows")
                && manifest
                    .as_ref()
                    .map(|m| m.launch_for(Manifest::current_platform()).exe.is_empty())
                    .unwrap_or(true)
                && obs
                    .receipt
                    .as_ref()
                    .and_then(|r| r.exe_override.as_ref())
                    .is_none();
            let status = tracker.status(&obs, &self.policy, now, needs_exe_choice);
            let _ = self.events.send(InstallEvent::Status(status.clone()));
            out.push(status);
        }
        out.sort_by(|a, b| a.game_id.cmp(&b.game_id));
        out
    }

    async fn spawn_verify(&self, id: &str, paths: &GamePaths, archive_len: Option<u64>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let archive = paths.archive.clone();
        let results = self.results.clone();
        let progress = self.progress.clone();
        let game_id = id.to_string();
        let c2 = cancel.clone();
        let generation = self.next_generation();
        let handle = tokio::task::spawn_blocking(move || {
            let gid = game_id.clone();
            let mut cb = |done: u64, total: u64, _: &str| {
                if let Ok(mut p) = progress.lock() {
                    p.insert(
                        gid.clone(),
                        if total > 0 {
                            done as f64 / total as f64
                        } else {
                            0.0
                        },
                    );
                }
            };
            let result = extract::verify(&archive, Some(c2), Some(&mut cb));
            if let Ok(mut r) = results.lock() {
                r.push(WorkResult::Verified {
                    game_id,
                    generation,
                    result,
                    archive_len,
                });
            }
        });
        self.work.lock().await.insert(
            id.to_string(),
            ActiveWork {
                cancel,
                handle,
                generation,
            },
        );
    }

    async fn spawn_extract(&self, id: &str, paths: &GamePaths, archive_len: u64, revision: String) {
        let cancel = Arc::new(AtomicBool::new(false));
        let archive = paths.archive.clone();
        let local = paths.local_dir.clone();
        let results = self.results.clone();
        let progress = self.progress.clone();
        let game_id = id.to_string();
        let c2 = cancel.clone();
        let generation = self.next_generation();
        let handle = tokio::task::spawn_blocking(move || {
            let gid = game_id.clone();
            let mut cb = |done: u64, total: u64, _: &str| {
                if let Ok(mut p) = progress.lock() {
                    p.insert(
                        gid.clone(),
                        if total > 0 {
                            done as f64 / total as f64
                        } else {
                            0.0
                        },
                    );
                }
            };
            let result = extract::extract_atomically(&archive, &local, Some(c2), Some(&mut cb));
            log::info!("extract: done {} ok={}", game_id, result.is_ok());
            if let Ok(mut r) = results.lock() {
                r.push(WorkResult::Extracted {
                    game_id,
                    generation,
                    result,
                    archive_len,
                    revision,
                });
            }
        });
        self.work.lock().await.insert(
            id.to_string(),
            ActiveWork {
                cancel,
                handle,
                generation,
            },
        );
    }

    async fn spawn_setup(&self, id: &str, paths: &GamePaths, manifest: Option<Manifest>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let results = self.results.clone();
        let hook = self.setup_hook.clone();
        let paths = paths.clone();
        let game_id = id.to_string();
        let generation = self.next_generation();
        let handle = tokio::spawn(async move {
            let result = hook.run_setup(&paths, manifest.as_ref()).await;
            if let Ok(mut r) = results.lock() {
                r.push(WorkResult::SetupDone {
                    game_id,
                    generation,
                    result,
                });
            }
        });
        self.work.lock().await.insert(
            id.to_string(),
            ActiveWork {
                cancel,
                handle,
                generation,
            },
        );
    }

    fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub async fn tracker_phase(&self, game_id: &str) -> Option<Phase> {
        self.trackers.lock().await.get(game_id).map(|t| t.phase)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ShareKey;

    fn obs(
        archive: Option<u64>,
        partial: Option<u64>,
        version: Option<&str>,
        transport_pct: Option<u64>,
    ) -> Observation {
        Observation {
            catalog_revision: "20250308".into(),
            catalog_bytes: 1000,
            archive_len: archive,
            partial_len: partial,
            version_ini: version.map(str::to_string),
            receipt: None,
            local_present: false,
            required_files_ok: false,
            transport: transport_pct.map(|p| ShareStatus {
                dir: "/x".into(),
                state: if p >= 100 {
                    ShareState::Complete
                } else {
                    ShareState::Downloading
                },
                bytes_done: p * 10,
                bytes_total: 1000,
                files_total: 3,
                peers: 2,
                download_bps: 0,
                upload_bps: 0,
                error: None,
            }),
            disk_free: Some(1 << 40),
        }
    }

    fn policy() -> Policy {
        Policy {
            stable_for: Duration::from_secs(15),
            stall_after: Duration::from_secs(120),
            verify_retry_after: Duration::from_secs(120),
        }
    }

    #[test]
    fn engine_stuck_at_99_percent_still_verifies_complete_archive() {
        let mut t = Tracker::new("amongus");
        t.request_install();
        let t0 = Instant::now();
        assert_eq!(
            t.step(&obs(None, Some(500), None, Some(50)), &policy(), t0),
            Action::None
        );
        assert_eq!(t.phase, Phase::Syncing);
        // file renamed to final name, engine still says 99 %
        let done = obs(Some(1000), None, Some("20250308"), Some(99));
        assert_eq!(
            t.step(&done, &policy(), t0 + Duration::from_secs(10)),
            Action::None
        );
        assert_eq!(
            t.step(&done, &policy(), t0 + Duration::from_secs(20)),
            Action::None,
            "not stable yet"
        );
        assert_eq!(
            t.step(&done, &policy(), t0 + Duration::from_secs(26)),
            Action::Verify
        );
        assert_eq!(t.phase, Phase::Verifying);
    }

    #[test]
    fn incomplete_archive_stays_syncing_and_retries_on_change() {
        let mut t = Tracker::new("g");
        t.request_install();
        let t0 = Instant::now();
        let done = obs(Some(800), None, Some("20250308"), Some(80));
        t.step(&done, &policy(), t0);
        assert_eq!(
            t.step(&done, &policy(), t0 + Duration::from_secs(16)),
            Action::Verify
        );
        t.verification_failed("checksum error".into(), Some(800));
        assert_eq!(t.phase, Phase::Syncing);
        assert_eq!(
            t.problem.as_ref().unwrap().code,
            "install.archive_incomplete"
        );
        // same size, too early → no retry
        assert_eq!(
            t.step(&done, &policy(), t0 + Duration::from_secs(40)),
            Action::None
        );
        // size changed → stable again after 15 s → retry
        let grown = obs(Some(1000), None, Some("20250308"), Some(100));
        t.step(&grown, &policy(), t0 + Duration::from_secs(50));
        assert_eq!(
            t.step(&grown, &policy(), t0 + Duration::from_secs(66)),
            Action::Verify
        );
    }

    #[test]
    fn stall_without_peers_is_reported_and_clears_on_progress() {
        let mut t = Tracker::new("g");
        t.request_install();
        let t0 = Instant::now();
        let mut o = obs(None, Some(100), None, Some(10));
        o.transport.as_mut().unwrap().peers = 0;
        t.step(&o, &policy(), t0);
        t.step(&o, &policy(), t0 + Duration::from_secs(130));
        assert_eq!(t.problem.as_ref().unwrap().code, "sync.no_peers");
        let s = t.status(&o, &policy(), t0 + Duration::from_secs(130), false);
        assert!(s.stalled);
        assert!(s.progress > 0.0 && s.progress < 1.0);
        let o2 = obs(None, Some(300), None, Some(30));
        t.step(&o2, &policy(), t0 + Duration::from_secs(131));
        assert!(t.problem.is_none());
    }

    #[test]
    fn receipt_makes_game_ready_and_update_is_detected() {
        let mut t = Tracker::new("g");
        let mut o = obs(Some(1000), None, Some("20250308"), None);
        o.receipt = Some(Receipt {
            version: 1,
            game_id: "g".into(),
            revision: "20250308".into(),
            installed_at: Utc::now(),
            archive_bytes: 1000,
            files: 3,
            setup_done: true,
            exe_override: None,
        });
        o.local_present = true;
        o.required_files_ok = true;
        assert_eq!(t.step(&o, &policy(), Instant::now()), Action::None);
        assert_eq!(t.phase, Phase::Ready);
        o.catalog_revision = "20260101".into();
        t.step(&o, &policy(), Instant::now());
        assert_eq!(t.phase, Phase::UpdateAvailable);
        assert!(t.phase.is_playable());
        // manifest files missing but local/ present → stays playable with a warning
        o.required_files_ok = false;
        o.catalog_revision = "20250308".into();
        t.step(&o, &policy(), Instant::now());
        assert_eq!(t.phase, Phase::Ready);
        assert_eq!(
            t.problem.as_ref().unwrap().code,
            "install.required_files_missing"
        );
        o.required_files_ok = true;
        t.step(&o, &policy(), Instant::now());
        assert!(t.problem.is_none());
        // local/ deleted by hand → back to syncing
        o.local_present = false;
        t.step(&o, &policy(), Instant::now());
        assert_eq!(t.phase, Phase::Syncing);
    }

    #[test]
    fn repair_works_from_failed_and_ready() {
        let mut t = Tracker::new("g");
        t.fail(Problem::new("x", Severity::Error));
        assert_eq!(t.phase, Phase::Failed);
        t.request_repair();
        assert_eq!(t.phase, Phase::Syncing);
        assert!(t.problem.is_none());
        let t0 = Instant::now();
        let done = obs(Some(1000), None, Some("20250308"), Some(100));
        t.step(&done, &policy(), t0);
        // transport complete + archive present → verify without waiting for stability
        assert_eq!(
            t.step(&done, &policy(), t0 + Duration::from_secs(1)),
            Action::Verify
        );
    }

    #[test]
    fn disk_full_is_reported_before_download() {
        let mut t = Tracker::new("g");
        t.request_install();
        let mut o = obs(None, None, None, Some(0));
        o.disk_free = Some(10);
        t.step(&o, &policy(), Instant::now());
        t.step(&o, &policy(), Instant::now());
        assert_eq!(t.problem.as_ref().unwrap().code, "install.disk_full");
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[tokio::test]
    async fn full_pipeline_with_demo_transport_stuck_at_99() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let mut demo = crate::transport::demo::DemoTransport::new();
        demo.duration = Duration::from_millis(100);
        demo.stuck_at_99 = true;
        demo.archive_source = Some(fixture("sample_game.rar"));
        let transport: Arc<dyn Transport> = Arc::new(demo);
        let mut catalog = Catalog::default();
        catalog.games.push(Game {
            id: "amongus".into(),
            order: 1,
            title: "Among Us".into(),
            key: ShareKey::parse("BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").unwrap(),
            revision: "20250308".into(),
            size_bytes: 4096,
            release_year: None,
            publisher: None,
            max_players: None,
            needs_master_server: false,
            genre_id: None,
            readme: Default::default(),
        });
        let r2 = root.clone();
        let mut manager = InstallManager::new(
            transport,
            catalog,
            move |g| Some(GamePaths::new(&r2, &g.id)),
            |_, _| {
                Some(Manifest {
                    id: "amongus".into(),
                    launch: crate::manifest::LaunchSpec {
                        exe: "game.exe".into(),
                        required_files: vec!["game.exe".into(), "sub/data.bin".into()],
                        ..Default::default()
                    },
                    setup: crate::manifest::SetupSpec {
                        touch: vec!["bin/steam_settings/disable_overlay.txt".into()],
                        ..Default::default()
                    },
                    ..Default::default()
                })
            },
            Arc::new(ManifestSetupHook),
        );
        manager.policy.stable_for = Duration::from_millis(200);
        manager.install("amongus").await.unwrap();

        let deadline = Instant::now() + Duration::from_secs(15);
        let mut last = Vec::new();
        while Instant::now() < deadline {
            last = manager.tick().await;
            if last.iter().any(|s| s.phase == Phase::Ready) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let s = &last[0];
        assert_eq!(s.phase, Phase::Ready, "status: {s:?}");
        assert_eq!(s.installed_revision.as_deref(), Some("20250308"));
        let paths = GamePaths::new(&root, "amongus");
        assert!(paths.local_dir.join("game.exe").is_file());
        assert!(paths
            .local_dir
            .join("bin/steam_settings/disable_overlay.txt")
            .is_file());
        let receipt = Receipt::load(&paths.receipt).unwrap();
        assert!(receipt.setup_done);
        assert_eq!(receipt.files, 3);

        // Repair re-runs verification and ends Ready again.
        manager.repair("amongus").await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut phase = Phase::Syncing;
        while Instant::now() < deadline {
            let st = manager.tick().await;
            phase = st[0].phase;
            if phase == Phase::Ready {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(phase, Phase::Ready);

        manager.uninstall("amongus").await.unwrap();
        assert!(!paths.share_dir.exists());
        assert!(manager.tick().await.is_empty());
    }
}
