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
    /// Installation was made by another launcher (the original ETI client) and
    /// adopted by comparing the archive listing with `local/`; no CRC test ran.
    /// "Repair" verifies and re-extracts it.
    #[serde(default)]
    pub adopted: bool,
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
    /// Bytes of every file directly in the share folder, `local/` excluded.
    ///
    /// The engine writes the package under names of its own (`…eti.!sync`
    /// while transferring, and whatever the share actually contains), so the
    /// only figure that always matches what is arriving is the folder itself.
    pub share_bytes: u64,
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
    /// How many bytes of this game have arrived.
    ///
    /// The engine's own figure when it has one; otherwise what the share
    /// folder holds. The folder is the honest source: the package may be
    /// transferred under a name of the engine's choosing, so looking only for
    /// `<id>.eti` and its `.!sync` twin showed nothing while gigabytes were
    /// landing next to them.
    pub fn bytes_on_disk(&self) -> u64 {
        // An installed game that is updating: the folder still holds the
        // version in use, and the engine counts it along with everything else,
        // so both would read as "almost done" before a byte of the new package
        // arrived. What lies in the folder beyond the installed package is the
        // new one — under whatever name it arrives — and once the engine
        // renames the finished file into place, an archive that is no longer
        // the installed one is the whole of it.
        if let Some(receipt) = &self.receipt {
            let beyond_installed = self.share_bytes.saturating_sub(receipt.archive_bytes);
            let swapped_in = self
                .archive_len
                .filter(|len| *len != receipt.archive_bytes)
                .unwrap_or(0);
            return beyond_installed
                .max(swapped_in)
                .max(self.partial_len.unwrap_or(0));
        }
        let engine = self.transport.as_ref().map(|t| t.bytes_done).unwrap_or(0);
        let on_disk = self
            .share_bytes
            .max(self.archive_len.unwrap_or(0))
            .max(self.partial_len.unwrap_or(0));
        // The larger of the two: the engine's figure is the better one when
        // it has it, and the web-UI path derives its number from a share size
        // that reads as a few hundred bytes while the engine is indexing.
        engine.max(on_disk)
    }

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
        // Everything that arrived, wherever in the share it sits: a package
        // is usually one `<id>.eti`, but some are a folder tree, and counting
        // only the top level showed "1.6 KB of 115.5 GB" for a download that
        // was running at full speed. `local/` (the extracted game, which would
        // dwarf the download) and the engine's own `.sync/` stay out. The
        // walk is bounded: this runs for every tracked game every couple of
        // seconds, and a package of more than 50,000 files would only be
        // counted more precisely, not more usefully.
        let receipt = Receipt::load(&paths.receipt);
        // An installed game that is not updating has nothing to count, and
        // this runs for every tracked game every couple of seconds. A pending
        // update is counted, whether it arrives as a `.!sync` file or as a
        // folder tree.
        // Nothing to count only when the installed package is also still
        // there: a game whose folder was emptied by hand is downloading
        // again, and for a folder-tree package there is no `.!sync` file to
        // notice that by.
        let settled = receipt
            .as_ref()
            .is_some_and(|r| r.revision == game.revision)
            && archive_len.is_some()
            && paths.local_dir.is_dir();
        let share_bytes = if settled && partial_len.is_none() {
            0
        } else {
            walkdir::WalkDir::new(&paths.share_dir)
                .max_depth(8)
                .into_iter()
                .filter_entry(|e| {
                    if !e.file_type().is_dir() || e.depth() == 0 {
                        return true;
                    }
                    let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                    // `local/` is the extracted game, `.sync/` the engine's
                    // bookkeeping, `.nll-staging`/`.nll-old*` belong to an
                    // extraction in progress — none of them is the download, and
                    // counting them showed more than the package's whole size.
                    !(e.depth() == 1
                        && (name == "local" || name == ".sync" || name.starts_with(".nll-")))
                })
                .flatten()
                .take(50_000)
                .filter(|e| e.file_type().is_file())
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        };
        let version_ini = std::fs::read_to_string(&paths.version_file)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s.len() <= 64);
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
            share_bytes,
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
    /// Compare an installation made by another launcher with its archive
    /// instead of re-extracting it.
    Adopt,
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
    /// When the engine started indexing this share; indexing postpones the
    /// stall warning, but not for ever.
    indexing_since: Option<Instant>,
    /// Last rate sample `(taken at, bytes)`; the transport's own rate is
    /// preferred, this covers engines that report none.
    rate_sample: Option<(Instant, u64)>,
    rate_bps: f64,
    verify_failed_at: Option<Instant>,
    verify_failed_len: Option<u64>,
    pub problem: Option<Problem>,
    pub work_progress: f64,
    pub work_started_at: Option<Instant>,
    /// Archive and `local/` were found on disk without a receipt (an ETI
    /// install): try to adopt before falling back to verify + extract.
    pub adopt_candidate: bool,
}

impl Tracker {
    pub fn new(game_id: &str) -> Self {
        Self {
            game_id: game_id.to_string(),
            phase: Phase::NotInstalled,
            wanted: false,
            adopt_candidate: false,
            last_archive_len: None,
            archive_stable_since: None,
            last_bytes: 0,
            last_progress_at: Instant::now(),
            indexing_since: None,
            rate_sample: None,
            rate_bps: 0.0,
            verify_failed_at: None,
            verify_failed_len: None,
            problem: None,
            work_progress: 0.0,
            work_started_at: None,
        }
    }

    pub fn request_install(&mut self) {
        self.wanted = true;
        self.adopt_candidate = false;
        if !self.phase.is_busy() && !self.phase.is_playable() {
            self.phase = Phase::Queued;
        }
        self.problem = None;
        self.last_progress_at = Instant::now();
        self.indexing_since = None;
    }

    /// Repair: forget verification failures and re-run the pipeline from
    /// whatever is on disk. Works from any phase, including Ready.
    pub fn request_repair(&mut self) {
        self.wanted = true;
        // Repair means "verify and re-extract", never the listing-only adoption.
        self.adopt_candidate = false;
        self.phase = Phase::Syncing;
        self.problem = None;
        self.verify_failed_at = None;
        self.verify_failed_len = None;
        self.archive_stable_since = None;
        // Force the stability timer to restart on the next observation.
        self.last_archive_len = None;
        self.last_progress_at = Instant::now();
        self.indexing_since = None;
        self.rate_sample = None;
        self.rate_bps = 0.0;
    }

    /// Measure the download rate from the growth of the archive on disk:
    /// Resilio's documented API reports no per-folder rate, and the file is
    /// the one number that is true on every transport. Smoothed over a few
    /// samples so a single slow tick does not make the display jump.
    fn sample_rate(&mut self, bytes_now: u64, now: Instant) {
        if !matches!(self.phase, Phase::Syncing | Phase::Queued) {
            self.rate_sample = None;
            self.rate_bps = 0.0;
            return;
        }
        let Some((at, bytes)) = self.rate_sample else {
            self.rate_sample = Some((now, bytes_now));
            return;
        };
        let seconds = now.duration_since(at).as_secs_f64();
        if seconds < 1.0 {
            return;
        }
        let measured = bytes_now.saturating_sub(bytes) as f64 / seconds;
        self.rate_bps = if self.rate_bps == 0.0 {
            measured
        } else {
            0.6 * self.rate_bps + 0.4 * measured
        };
        self.rate_sample = Some((now, bytes_now));
    }

    pub fn set_paused(&mut self, paused: bool) {
        if paused && self.phase == Phase::Syncing {
            self.phase = Phase::Paused;
        } else if !paused && self.phase == Phase::Paused {
            self.phase = Phase::Syncing;
            self.last_progress_at = Instant::now();
            self.indexing_since = None;
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
        self.adopt_candidate = false;
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
        let bytes_now = obs.bytes_on_disk();
        if bytes_now != self.last_bytes {
            self.last_bytes = bytes_now;
            self.last_progress_at = now;
            // Bytes arrived, so whatever indexing there was is behind us; the
            // grace period starts again only with the next indexing run.
            self.indexing_since = None;
        }
        self.sample_rate(bytes_now, now);
        // Archive stability.
        if obs.archive_len != self.last_archive_len {
            self.last_archive_len = obs.archive_len;
            self.archive_stable_since = obs.archive_len.map(|_| now);
        }

        match self.phase {
            Phase::Queued => {
                if self.adopt_candidate
                    && obs.archive_len.is_some()
                    && obs.partial_len.is_none()
                    && obs.local_present
                {
                    // Listing comparison only; the phase reads as "verifying"
                    // in the UI, which is what happens.
                    self.phase = Phase::Verifying;
                    self.work_progress = 0.0;
                    self.work_started_at = Some(now);
                    return Action::Adopt;
                }
                self.adopt_candidate = false;
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
                // The engine gave up on this share — usually a folder that
                // was removed under it, which it keeps reporting for ever
                // while the share looks finished. "Repair" re-adds the share,
                // and that is what clears it.
                // Not over a problem that already names the cause: a full
                // disk is reported by the engine as a folder error too, and
                // "repair" would do nothing about it.
                let has_specific_problem = self
                    .problem
                    .as_ref()
                    .is_some_and(|p| p.code.starts_with("install."));
                if let Some(t) = obs
                    .transport
                    .as_ref()
                    .filter(|_| !has_specific_problem)
                    .filter(|t| t.state == ShareState::Error)
                {
                    self.problem = Some(
                        Problem::new("sync.share_error", Severity::Error)
                            .param("detail", t.error.clone().unwrap_or_default())
                            .step("sync.share_error.step.1")
                            .with_fix(FixAction::RepairGame {
                                game_id: self.game_id.clone(),
                            }),
                    );
                    return Action::None;
                }
                // A package that arrives as one huge file gives the engine
                // nothing to count until it is done, so "no bytes" is not
                // "nothing is happening" while the engine reports a rate. A
                // trickle is not a transfer, though, and indexing only counts
                // for as long as indexing plausibly takes.
                const MOVING_BPS: u64 = 4096;
                const INDEXING_GRACE: Duration = Duration::from_secs(600);
                if let Some(t) = obs.transport.as_ref() {
                    if t.state == ShareState::Indexing {
                        let since = *self.indexing_since.get_or_insert(now);
                        if now.duration_since(since) < INDEXING_GRACE {
                            self.last_progress_at = now;
                        }
                    } else if t.download_bps >= MOVING_BPS {
                        // Data is moving: whatever indexing there was is over,
                        // and the next indexing run gets its own grace.
                        self.indexing_since = None;
                        self.last_progress_at = now;
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
        // While the engine is still indexing a share it reports a size of a
        // few hundred bytes for a package of many gigabytes; taking it at face
        // value turned every download into "0 B of 782 B". The catalog's own
        // figure is the floor.
        let total = obs
            .transport
            .as_ref()
            .map(|t| t.bytes_total)
            .unwrap_or(0)
            .max(obs.catalog_bytes)
            .max(1);
        let done = match self.phase {
            Phase::Ready | Phase::UpdateAvailable => total,
            _ => obs.bytes_on_disk(),
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
            // The engine's own figure when it has one, else what the archive
            // on disk actually grew by.
            download_bps: obs
                .transport
                .as_ref()
                .map(|t| t.download_bps)
                .filter(|b| *b > 0)
                .unwrap_or_else(|| self.rate_bps.max(0.0).round() as u64),
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

/// Files that must exist after extraction for the install to count as
/// complete. On Windows the package's own `game_start.cmd` decides what it
/// needs, and the manifest there is usually derived from that very script, so
/// a guessed executable must never produce a warning; only `local/` has to
/// exist. Every other platform launches through the manifest, where a missing
/// file is worth reporting.
pub fn required_files_for(manifest: Option<&Manifest>, platform: &str) -> Vec<String> {
    if platform == "windows" {
        return Vec::new();
    }
    manifest
        .map(|m| m.launch_for(platform).required_files)
        .unwrap_or_default()
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
    Adopted {
        game_id: String,
        generation: u64,
        /// `Ok(Some(files))` when `local/` matches the archive listing.
        result: Result<Option<u64>>,
        archive_len: u64,
        revision: String,
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
            }
            | WorkResult::Adopted {
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

    pub async fn catalog_len(&self) -> usize {
        self.catalog.read().await.games.len()
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
        // A download that never starts is otherwise invisible in the log: the
        // share either reached the engine or it did not, and that is the first
        // thing to know.
        match self
            .transport
            .add_share(
                &game.key,
                &paths.share_dir,
                &ShareOptions {
                    lan_only: self.lan_only,
                    paused: false,
                },
            )
            .await
        {
            Ok(()) => log::info!(
                "share for {game_id} registered at {}",
                paths.share_dir.display()
            ),
            Err(e) => {
                log::warn!(
                    "share for {game_id} not registered at {}: {e}",
                    paths.share_dir.display()
                );
                return Err(e);
            }
        }
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
        if let Err(e) = self
            .transport
            .add_share(
                &game.key,
                &paths.share_dir,
                &ShareOptions {
                    lan_only: self.lan_only,
                    paused: false,
                },
            )
            .await
        {
            // The repair itself (verify and re-extract) still runs on what is
            // on disk; only a fresh download would need the share.
            log::warn!(
                "share for {game_id} not registered at {}: {e}",
                paths.share_dir.display()
            );
        }
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
        self.remove_data(game_id, false).await.map(|_| ())
    }

    /// Stop a running download and delete what it has fetched so far.
    ///
    /// Unlike [`Self::uninstall`] an installation that is already on disk
    /// survives: when a receipt and `local/` exist, only the archive and its
    /// `.part` file are removed, so a cancelled *update* leaves the playable
    /// version and its savegames untouched. Without an installation the game
    /// is removed completely, the same as an uninstall.
    ///
    /// Returns `true` when an installation was kept.
    pub async fn cancel_download(&self, game_id: &str) -> Result<bool> {
        self.remove_data(game_id, true).await
    }

    /// Shared body of [`Self::uninstall`] and [`Self::cancel_download`].
    ///
    /// The tracker lock is held from the first step to the last. `tick` takes
    /// the same lock before it starts any job, so nothing can re-spawn a
    /// verify or extract run for this game while the files disappear under
    /// it; without that, a job started in between would recreate the
    /// directory or drive the tracker into `Failed`.
    ///
    /// Removing the share is what stops the download, so it is never re-added
    /// here: for a kept installation the archive is gone anyway (an update
    /// replaces it in place, so there is nothing left to seed), and re-adding
    /// the share would immediately start the very download that was just
    /// cancelled. The next launcher start registers it again via
    /// [`Self::adopt_existing`].
    async fn remove_data(&self, game_id: &str, keep_install: bool) -> Result<bool> {
        let game = self.game(game_id).await?;
        let paths = self.paths_for(&game)?;
        let mut trackers = self.trackers.lock().await;
        let keep = keep_install && paths.receipt.exists() && paths.local_dir.is_dir();
        if keep {
            // A fresh tracker drops the download state; the receipt then
            // decides whether the game reads as ready or as "update
            // available" again.
            trackers.insert(game_id.to_string(), Tracker::new(game_id));
        } else {
            trackers.remove(game_id);
        }
        self.cancel_work(game_id).await;
        let _ = self.transport.remove_share(&paths.share_dir).await;
        // The engine releases its handles on the folder only after it has
        // processed the removal; deleting right away fails with "access
        // denied" on Windows.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let archive = paths.archive.clone();
        let dir = paths.share_dir.clone();
        tokio::task::spawn_blocking(move || {
            if keep {
                let _ = std::fs::remove_file(partial_path(&archive));
                let _ = std::fs::remove_file(&archive);
                Ok(())
            } else {
                crate::extract::remove_dir_all_retrying(&dir)
            }
        })
        .await
        .map_err(|e| Error::Launch(e.to_string()))??;
        Ok(keep)
    }

    async fn cancel_work(&self, game_id: &str) {
        if let Some(w) = self.work.lock().await.remove(game_id) {
            w.cancel.store(true, Ordering::Relaxed);
            w.handle.abort();
        }
    }

    /// Stop every running job. Called before this manager is replaced so an
    /// orphaned extraction cannot race the successor's.
    pub async fn cancel_all(&self) {
        let mut work = self.work.lock().await;
        for (_, w) in work.drain() {
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
                    // Start in Queued so the first step can take the receipt
                    // shortcut (Ready) or the adoption path; only a download in
                    // progress goes straight to Syncing. Never re-extract an
                    // installation that is already on disk.
                    let installed_elsewhere = paths.archive.exists()
                        && paths.version_file.is_file()
                        && std::fs::read_dir(&paths.local_dir)
                            .map(|mut d| d.next().is_some())
                            .unwrap_or(false);
                    if paths.receipt.exists() || installed_elsewhere {
                        t.adopt_candidate = !paths.receipt.exists();
                        t.phase = Phase::Queued;
                    } else {
                        t.phase = Phase::Syncing;
                    }
                    if let Err(e) = self
                        .transport
                        .add_share(
                            &game.key,
                            &paths.share_dir,
                            &ShareOptions {
                                lan_only: self.lan_only,
                                paused: false,
                            },
                        )
                        .await
                    {
                        // Not fatal — the game keeps whatever is on disk — but
                        // it is why nothing arrives, so it has to be readable.
                        log::warn!(
                            "share for {} not registered at {}: {e}",
                            game.id,
                            paths.share_dir.display()
                        );
                    }
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
                                            adopted: false,
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
                WorkResult::Adopted {
                    game_id,
                    result,
                    archive_len,
                    revision,
                    ..
                } => {
                    self.work.lock().await.remove(&game_id);
                    if let Some(t) = trackers.get_mut(&game_id) {
                        t.adopt_candidate = false;
                        match result {
                            Ok(Some(files)) => {
                                let receipt = Receipt {
                                    version: 1,
                                    game_id: game_id.clone(),
                                    revision,
                                    installed_at: Utc::now(),
                                    archive_bytes: archive_len,
                                    files,
                                    // The other launcher ran its setup already.
                                    setup_done: true,
                                    exe_override: None,
                                    adopted: true,
                                };
                                let saved = catalog
                                    .game(&game_id)
                                    .and_then(|g| (self.resolve_paths)(g))
                                    .map(|p| receipt.save(&p.receipt));
                                match saved {
                                    Some(Ok(())) => {
                                        log::info!("adopt: {game_id} matches its archive ({files} files); marked ready");
                                        t.phase = Phase::Ready;
                                        t.problem = None;
                                    }
                                    Some(Err(e)) => t.fail(
                                        Problem::new("install.receipt_error", Severity::Error)
                                            .param("detail", e.to_string()),
                                    ),
                                    None => t.phase = Phase::Syncing,
                                }
                            }
                            Ok(None) => {
                                log::info!("adopt: {game_id} differs from its archive; verifying and extracting");
                                t.phase = Phase::Syncing;
                            }
                            Err(e) => {
                                log::warn!("adopt: {game_id} listing failed ({e}); verifying and extracting");
                                t.phase = Phase::Syncing;
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
            let required = required_files_for(manifest.as_ref(), Manifest::current_platform());
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
                    Action::Adopt => {
                        let revision = obs
                            .version_ini
                            .clone()
                            .unwrap_or_else(|| game.revision.clone());
                        self.spawn_adopt(&id, &paths, obs.archive_len.unwrap_or(0), revision)
                            .await
                    }
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

    async fn spawn_adopt(&self, id: &str, paths: &GamePaths, archive_len: u64, revision: String) {
        let cancel = Arc::new(AtomicBool::new(false));
        let archive = paths.archive.clone();
        let local = paths.local_dir.clone();
        let results = self.results.clone();
        let game_id = id.to_string();
        let generation = self.next_generation();
        let handle = tokio::task::spawn_blocking(move || {
            let result = extract::matches_extracted(&archive, &local);
            if let Ok(mut r) = results.lock() {
                r.push(WorkResult::Adopted {
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

    #[test]
    fn an_indexing_engine_does_not_shrink_the_download() {
        // Resilio answers with a few hundred bytes for a package of many
        // gigabytes while it is still indexing the share. Taken at face value
        // that reads as "0 B of 782 B" and the bar never moves, while the
        // folder on disk is filling at full speed.
        let mut o = obs(None, None, None, None);
        o.catalog_bytes = 60_000_000_000;
        o.share_bytes = 6_000_000_000;
        o.transport = Some(ShareStatus {
            dir: "/x".into(),
            state: ShareState::Downloading,
            bytes_done: 0,
            bytes_total: 782,
            files_total: 1,
            peers: 1,
            download_bps: 0,
            upload_bps: 0,
            error: None,
        });
        let mut t = Tracker::new("g");
        t.phase = Phase::Syncing;
        let s = t.status(&o, &Policy::default(), Instant::now(), false);
        assert_eq!(s.bytes_total, 60_000_000_000);
        assert_eq!(s.bytes_done, 6_000_000_000);
        assert!((s.progress - 0.1).abs() < 0.001, "{}", s.progress);
    }

    #[test]
    fn bytes_come_from_the_share_folder_when_the_package_has_another_name() {
        // Only `<id>.eti` and `<id>.eti.!sync` used to count, so a package the
        // engine transfers under a different name looked like no progress.
        let mut o = obs(None, None, None, None);
        o.share_bytes = 4096;
        assert_eq!(o.bytes_on_disk(), 4096);
        // The engine's own figure counts where it is the larger one …
        o.transport = Some(ShareStatus {
            dir: "/x".into(),
            state: ShareState::Downloading,
            bytes_done: 9000,
            bytes_total: 10_000,
            files_total: 1,
            peers: 1,
            download_bps: 0,
            upload_bps: 0,
            error: None,
        });
        assert_eq!(o.bytes_on_disk(), 9000);
        // … but on a first download a small figure from a share the engine is
        // still indexing does not outrank the gigabytes already on the disk.
        o.share_bytes = 6_000_000_000;
        o.transport.as_mut().unwrap().bytes_done = 234;
        assert_eq!(o.bytes_on_disk(), 6_000_000_000);

        // Updating an installed game: neither the old archive in the folder
        // nor the engine's figure (which counts it) says anything about the
        // new package — only the `.!sync` file of the arriving one does.
        o.share_bytes = 6_000_000_000;
        o.archive_len = Some(6_000_000_000);
        o.partial_len = None;
        o.receipt = Some(Receipt {
            version: 1,
            game_id: "g".into(),
            revision: "20250308".into(),
            installed_at: chrono::Utc::now(),
            archive_bytes: 6_000_000_000,
            files: 1,
            setup_done: true,
            exe_override: None,
            adopted: false,
        });
        assert_eq!(o.bytes_on_disk(), 0);
        // Once the new package starts arriving, its own file is the figure —
        // the engine reports the whole folder and would stay near "done".
        o.partial_len = Some(1_500_000_000);
        o.transport.as_mut().unwrap().bytes_done = 6_000_000_000;
        assert_eq!(o.bytes_on_disk(), 1_500_000_000);
        // Arriving under a name of the engine's choosing: what the folder
        // holds beyond the installed package counts, or the download would
        // look stalled for its whole length.
        o.partial_len = None;
        o.share_bytes = 6_000_000_000 + 1_500_000_000;
        assert_eq!(o.bytes_on_disk(), 1_500_000_000);
        // Renamed into place: the archive is no longer the installed one, so
        // it is the new package in full — no drop back to 0 % while the state
        // machine waits for a stable size.
        o.share_bytes = 7_000_000_000;
        o.archive_len = Some(7_000_000_000);
        assert_eq!(o.bytes_on_disk(), 7_000_000_000);
    }

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
            share_bytes: archive.or(partial).unwrap_or(0),
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
    fn a_package_that_arrives_as_a_folder_counts_too() {
        // The package of a big game is not always one `<id>.eti`: with only
        // the top level counted, a 115 GB download read as "1.6 KB".
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "bf4");
        std::fs::create_dir_all(paths.share_dir.join("Data")).unwrap();
        std::fs::create_dir_all(paths.share_dir.join(".nll-staging")).unwrap();
        std::fs::create_dir_all(&paths.local_dir).unwrap();
        // An extraction in progress is not part of the download either.
        std::fs::write(
            paths.share_dir.join(".nll-staging").join("x"),
            vec![0u8; 70_000],
        )
        .unwrap();
        std::fs::write(paths.share_dir.join("version.ini"), vec![0u8; 16]).unwrap();
        std::fs::write(
            paths.share_dir.join("Data").join("big.dat"),
            vec![0u8; 4096],
        )
        .unwrap();
        // The extracted game is not the download.
        std::fs::write(paths.local_dir.join("game.exe"), vec![0u8; 100_000]).unwrap();
        let game = Game {
            id: "bf4".into(),
            order: 0,
            title: "Battlefield 4".into(),
            key: crate::catalog::ShareKey::parse(crate::catalog::BUILTIN_CATALOG_KEY).unwrap(),
            revision: "20250308".into(),
            size_bytes: 1 << 40,
            release_year: None,
            publisher: None,
            max_players: None,
            needs_master_server: false,
            genre_id: None,
            readme: Default::default(),
        };
        let obs = Observation::from_disk(&paths, &game, &[], &crate::library::DiskTable::default());
        assert_eq!(obs.share_bytes, 16 + 4096);
    }

    #[test]
    fn a_running_transfer_is_never_called_stalled() {
        // Resilio counts a file only once it is complete, so a single huge
        // package shows no bytes for its whole transfer. The rate is what says
        // it is alive.
        let mut t = Tracker::new("bf4");
        t.request_install();
        let t0 = Instant::now();
        let mut o = obs(None, Some(1600), None, Some(0));
        if let Some(tr) = o.transport.as_mut() {
            tr.download_bps = 179_000_000;
            tr.peers = 1;
        }
        assert_eq!(t.step(&o, &policy(), t0), Action::None);
        assert_eq!(t.phase, Phase::Syncing);
        // Ten minutes later, still not a byte more on disk — and still no
        // "download stalled", because the engine is moving data.
        assert_eq!(
            t.step(&o, &policy(), t0 + Duration::from_secs(600)),
            Action::None
        );
        assert!(t.problem.is_none(), "{:?}", t.problem);
        // The moment the rate drops to zero the clock starts again.
        if let Some(tr) = o.transport.as_mut() {
            tr.download_bps = 0;
        }
        t.step(&o, &policy(), t0 + Duration::from_secs(600));
        t.step(&o, &policy(), t0 + Duration::from_secs(900));
        assert_eq!(
            t.problem.as_ref().map(|p| p.code.as_str()),
            Some("sync.stalled")
        );
    }

    #[test]
    fn a_share_the_engine_gave_up_on_asks_for_a_repair() {
        // "Folder not found" after the user deleted the folder: the share
        // reports a finished download that never moves, and only re-adding
        // it helps — which is what "Repair" does.
        let mut t = Tracker::new("ut99");
        t.request_install();
        let t0 = Instant::now();
        let mut o = obs(None, Some(10), None, Some(0));
        if let Some(tr) = o.transport.as_mut() {
            tr.state = ShareState::Error;
            tr.error = Some("folder not found".into());
        }
        assert_eq!(t.step(&o, &policy(), t0), Action::None);
        assert_eq!(t.phase, Phase::Syncing);
        assert_eq!(
            t.step(&o, &policy(), t0 + Duration::from_secs(2)),
            Action::None
        );
        let problem = t.problem.as_ref().expect("a problem");
        assert_eq!(problem.code, "sync.share_error");
        assert_eq!(
            problem.params.get("detail").map(String::as_str),
            Some("folder not found")
        );
        assert!(matches!(problem.fix, Some(FixAction::RepairGame { .. })));
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
    fn windows_ignores_manifest_required_files() {
        let mut m = Manifest::default();
        m.launch.exe = "game.exe".into();
        m.launch.required_files = vec!["game.exe".into()];
        // Windows starts the package's own script, and the manifest there is
        // usually derived from that script: a guessed exe must not warn.
        assert!(required_files_for(Some(&m), "windows").is_empty());
        assert_eq!(
            required_files_for(Some(&m), "linux"),
            vec!["game.exe".to_string()]
        );
        assert!(required_files_for(None, "linux").is_empty());
    }

    #[test]
    fn measures_the_download_rate_from_the_archive_on_disk() {
        // The documented Resilio API reports no rate, so the growth of the
        // file on disk is measured; two samples are needed before a rate
        // exists, and the engine's own figure wins when it has one.
        let policy = policy();
        let mut t = Tracker::new("g");
        t.request_install();
        let t0 = Instant::now();
        let mut o = obs(None, Some(0), None, None);
        t.step(&o, &policy, t0);
        assert_eq!(t.phase, Phase::Syncing);
        o.partial_len = Some(200);
        let t1 = t0 + Duration::from_secs(2);
        t.step(&o, &policy, t1);
        assert_eq!(
            t.status(&o, &policy, t1, false).download_bps,
            100,
            "200 bytes in 2 s"
        );
        o.partial_len = Some(600);
        let t2 = t0 + Duration::from_secs(4);
        t.step(&o, &policy, t2);
        assert_eq!(
            t.status(&o, &policy, t2, false).download_bps,
            140,
            "smoothed: 0.6 of the old rate plus 0.4 of the measured 200"
        );
        o.transport = Some(ShareStatus {
            dir: "/x".into(),
            state: ShareState::Downloading,
            bytes_done: 600,
            bytes_total: 1000,
            files_total: 1,
            peers: 2,
            download_bps: 9000,
            upload_bps: 0,
            error: None,
        });
        assert_eq!(t.status(&o, &policy, t2, false).download_bps, 9000);
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
            adopted: false,
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

    /// Setup hook that counts invocations, to prove adoption skips setup.
    struct CountingHook(Arc<std::sync::atomic::AtomicUsize>);

    #[async_trait::async_trait]
    impl SetupHook for CountingHook {
        async fn run_setup(&self, _: &GamePaths, _: Option<&Manifest>) -> Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn test_game(id: &str) -> Game {
        Game {
            id: id.into(),
            order: 1,
            title: id.into(),
            key: ShareKey::parse("BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").unwrap(),
            revision: "20250308".into(),
            size_bytes: 4096,
            release_year: None,
            publisher: None,
            max_players: None,
            needs_master_server: false,
            genre_id: None,
            readme: Default::default(),
        }
    }

    /// Manager over a passive folder transport with an on-disk ETI-style
    /// install of `sample_game.rar` (archive, version.ini, extracted local/).
    fn eti_install(root: &Path, id: &str) -> GamePaths {
        let paths = GamePaths::new(root, id);
        std::fs::create_dir_all(&paths.share_dir).unwrap();
        std::fs::copy(fixture("sample_game.rar"), &paths.archive).unwrap();
        std::fs::write(&paths.version_file, "20250308\n").unwrap();
        extract::extract_atomically(&paths.archive, &paths.local_dir, None, None).unwrap();
        paths
    }

    fn adopt_manager(
        root: &Path,
        id: &str,
        setups: Arc<std::sync::atomic::AtomicUsize>,
    ) -> InstallManager {
        let mut catalog = Catalog::default();
        catalog.games.push(test_game(id));
        let r2 = root.to_path_buf();
        let mut manager = InstallManager::new(
            Arc::new(crate::transport::folder::FolderTransport::new()),
            catalog,
            move |g| Some(GamePaths::new(&r2, &g.id)),
            |_, _| None,
            Arc::new(CountingHook(setups)),
        );
        manager.policy.stable_for = Duration::from_millis(200);
        manager
    }

    async fn tick_until_ready(manager: &InstallManager) -> Vec<Phase> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut seen = Vec::new();
        while Instant::now() < deadline {
            let statuses = manager.tick().await;
            if let Some(s) = statuses.first() {
                if seen.last() != Some(&s.phase) {
                    seen.push(s.phase);
                }
                if s.phase.is_playable() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        seen
    }

    #[tokio::test]
    async fn adopted_eti_install_becomes_ready_without_extracting_or_setup() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = eti_install(tmp.path(), "amongus");
        std::fs::write(paths.local_dir.join("savegame.dat"), "keep me").unwrap();
        let setups = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let manager = adopt_manager(tmp.path(), "amongus", setups.clone());
        manager.adopt_existing().await;
        assert_eq!(manager.tracker_phase("amongus").await, Some(Phase::Queued));

        let seen = tick_until_ready(&manager).await;
        assert_eq!(seen.last(), Some(&Phase::Ready), "phases: {seen:?}");
        assert!(!seen.contains(&Phase::Extracting), "phases: {seen:?}");
        assert_eq!(
            setups.load(Ordering::SeqCst),
            0,
            "setup must not run for adopted installs"
        );
        // Files the other launcher left in local/ survive.
        assert_eq!(
            std::fs::read_to_string(paths.local_dir.join("savegame.dat")).unwrap(),
            "keep me"
        );
        let receipt = Receipt::load(&paths.receipt).expect("receipt written");
        assert!(receipt.adopted);
        assert!(receipt.setup_done);
        assert_eq!(receipt.revision, "20250308");
        assert_eq!(receipt.files, 3);
    }

    #[tokio::test]
    async fn cancelling_an_update_keeps_the_installed_game() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = eti_install(tmp.path(), "amongus");
        std::fs::write(paths.local_dir.join("savegame.dat"), "keep me").unwrap();
        let setups = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let manager = adopt_manager(tmp.path(), "amongus", setups.clone());
        manager.adopt_existing().await;
        tick_until_ready(&manager).await;
        // A newer archive is being fetched over the installed version.
        std::fs::write(partial_path(&paths.archive), b"half a download").unwrap();

        assert!(manager.cancel_download("amongus").await.unwrap());
        assert!(!paths.archive.exists());
        assert!(!partial_path(&paths.archive).exists());
        assert!(paths.receipt.is_file());
        assert_eq!(
            std::fs::read_to_string(paths.local_dir.join("savegame.dat")).unwrap(),
            "keep me"
        );
        assert_eq!(
            manager.tick().await.first().map(|s| s.phase),
            Some(Phase::Ready)
        );
    }

    #[tokio::test]
    async fn cancelling_a_first_download_removes_the_game() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "amongus");
        std::fs::create_dir_all(&paths.share_dir).unwrap();
        std::fs::write(partial_path(&paths.archive), b"half a download").unwrap();
        let setups = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let manager = adopt_manager(tmp.path(), "amongus", setups.clone());

        assert!(!manager.cancel_download("amongus").await.unwrap());
        assert!(!paths.share_dir.exists());
    }

    #[tokio::test]
    async fn incomplete_eti_install_falls_back_to_verify_and_extract() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = eti_install(tmp.path(), "amongus");
        std::fs::remove_file(paths.local_dir.join("game.exe")).unwrap();
        let setups = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let manager = adopt_manager(tmp.path(), "amongus", setups.clone());
        manager.adopt_existing().await;

        let seen = tick_until_ready(&manager).await;
        assert_eq!(seen.last(), Some(&Phase::Ready), "phases: {seen:?}");
        assert!(seen.contains(&Phase::Extracting), "phases: {seen:?}");
        assert_eq!(setups.load(Ordering::SeqCst), 1);
        assert!(paths.local_dir.join("game.exe").is_file());
        assert!(!Receipt::load(&paths.receipt).unwrap().adopted);
    }

    #[tokio::test]
    async fn existing_receipt_is_ready_on_first_tick() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = eti_install(tmp.path(), "amongus");
        Receipt {
            version: 1,
            game_id: "amongus".into(),
            revision: "20250308".into(),
            installed_at: Utc::now(),
            archive_bytes: 0,
            files: 3,
            setup_done: true,
            exe_override: None,
            adopted: false,
        }
        .save(&paths.receipt)
        .unwrap();
        let setups = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let manager = adopt_manager(tmp.path(), "amongus", setups.clone());
        manager.adopt_existing().await;
        let first = manager.tick().await;
        assert_eq!(first[0].phase, Phase::Ready);
        assert!(
            manager.work.lock().await.is_empty(),
            "no job may be started"
        );
        assert_eq!(setups.load(Ordering::SeqCst), 0);
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
