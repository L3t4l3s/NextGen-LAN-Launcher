//! Verification and extraction of ETI `.eti` packages (RAR archives).
//!
//! The archive is first *tested* (every entry's CRC is checked) so a game is
//! only ever marked playable when its data is provably complete. Extraction
//! goes to a staging directory next to `local/` and is moved into place
//! atomically, so an interrupted extraction never leaves a half-installed game
//! that looks complete.

use crate::error::{Error, Result};
use crate::manifest::is_safe_relative;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use unrar::Archive;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveSummary {
    pub files: u64,
    pub directories: u64,
    pub unpacked_bytes: u64,
    /// Entries that would escape the destination (rejected).
    pub unsafe_entries: Vec<String>,
}

/// Outcome of verifying an archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Verification {
    Ok(ArchiveSummary),
    /// The archive header is unreadable – usually still downloading.
    NotAnArchive {
        detail: String,
    },
    /// Header fine, but at least one entry failed its CRC or the file ends
    /// early – incomplete or corrupt download.
    Damaged {
        detail: String,
        entries_ok: u64,
    },
}

/// Progress callback: (bytes_done, bytes_total, current_file).
pub type Progress<'a> = &'a mut dyn FnMut(u64, u64, &str);

fn map_err(e: unrar::error::UnrarError) -> Error {
    Error::Archive(format!("{e}"))
}

/// List entries and compute the summary without extracting.
pub fn summarise(archive: &Path) -> Result<ArchiveSummary> {
    let list = Archive::new(archive).open_for_listing().map_err(map_err)?;
    let mut summary = ArchiveSummary {
        files: 0,
        directories: 0,
        unpacked_bytes: 0,
        unsafe_entries: Vec::new(),
    };
    for entry in list {
        let entry = entry.map_err(map_err)?;
        let name = entry.filename.to_string_lossy().to_string();
        if !is_safe_relative(&name) {
            summary.unsafe_entries.push(name);
            continue;
        }
        if entry.is_directory() {
            summary.directories += 1;
        } else {
            summary.files += 1;
            summary.unpacked_bytes += entry.unpacked_size;
        }
    }
    Ok(summary)
}

/// Files at least this large must match the archive's unpacked size when an
/// installation is adopted. Smaller files (configs, ini, cfg) are legitimately
/// rewritten by setup scripts and the games themselves, so for them presence
/// is enough.
pub const ADOPT_SIZE_CHECK_MIN_BYTES: u64 = 1024 * 1024;

/// Compare the archive listing with an already extracted `local/` directory
/// without reading archive data: every regular entry must exist, and entries
/// of at least [`ADOPT_SIZE_CHECK_MIN_BYTES`] must have their unpacked size.
/// `Ok(Some(files))` when everything matches, `Ok(None)` when a file is
/// missing, a large file differs in size, or the archive has no files; `Err`
/// when the archive cannot be listed. Used to adopt installations made by the
/// original ETI launcher without re-extracting them.
pub fn matches_extracted(archive: &Path, local_dir: &Path) -> Result<Option<u64>> {
    matches_extracted_with(archive, local_dir, ADOPT_SIZE_CHECK_MIN_BYTES)
}

/// [`matches_extracted`] with an explicit size-check threshold.
pub fn matches_extracted_with(
    archive: &Path,
    local_dir: &Path,
    size_check_min: u64,
) -> Result<Option<u64>> {
    let list = Archive::new(archive).open_for_listing().map_err(map_err)?;
    let mut files = 0u64;
    for entry in list {
        let entry = entry.map_err(map_err)?;
        let name = entry.filename.to_string_lossy().to_string();
        if !is_safe_relative(&name) {
            return Ok(None);
        }
        if entry.is_directory() {
            continue;
        }
        match std::fs::metadata(local_dir.join(&entry.filename)) {
            Ok(m)
                if m.is_file()
                    && (entry.unpacked_size < size_check_min || m.len() == entry.unpacked_size) =>
            {
                files += 1
            }
            _ => return Ok(None),
        }
    }
    Ok((files > 0).then_some(files))
}

/// Test every entry of the archive (CRC check). Never writes to disk.
pub fn verify(
    archive: &Path,
    cancel: Option<Arc<AtomicBool>>,
    mut progress: Option<Progress<'_>>,
) -> Result<Verification> {
    let summary = match summarise(archive) {
        Ok(s) => s,
        Err(e) => {
            return Ok(Verification::NotAnArchive {
                detail: e.to_string(),
            })
        }
    };
    let mut open = match Archive::new(archive).open_for_processing() {
        Ok(a) => a,
        Err(e) => {
            return Ok(Verification::NotAnArchive {
                detail: e.to_string(),
            })
        }
    };
    let mut done = 0u64;
    let mut entries_ok = 0u64;
    loop {
        if cancel
            .as_ref()
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(false)
        {
            return Err(Error::Archive("cancelled".into()));
        }
        let header = match open.read_header() {
            Ok(Some(h)) => h,
            Ok(None) => break,
            Err(e) => {
                return Ok(Verification::Damaged {
                    detail: e.to_string(),
                    entries_ok,
                })
            }
        };
        let name = header.entry().filename.to_string_lossy().to_string();
        let size = header.entry().unpacked_size;
        match header.test() {
            Ok(next) => {
                open = next;
                entries_ok += 1;
                done += size;
                if let Some(p) = progress.as_mut() {
                    p(done, summary.unpacked_bytes, &name);
                }
            }
            Err(e) => {
                return Ok(Verification::Damaged {
                    detail: format!("{name}: {e}"),
                    entries_ok,
                })
            }
        }
    }
    Ok(Verification::Ok(summary))
}

/// Extract the archive into `dest` (created if missing). Entries with unsafe
/// paths are skipped. Returns the number of files written.
pub fn extract(
    archive: &Path,
    dest: &Path,
    cancel: Option<Arc<AtomicBool>>,
    mut progress: Option<Progress<'_>>,
) -> Result<u64> {
    let summary = summarise(archive)?;
    std::fs::create_dir_all(dest).map_err(|e| Error::io(dest, e))?;
    let mut open = Archive::new(archive)
        .open_for_processing()
        .map_err(map_err)?;
    let mut done = 0u64;
    let mut files = 0u64;
    loop {
        if cancel
            .as_ref()
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(false)
        {
            return Err(Error::Archive("cancelled".into()));
        }
        let Some(header) = open.read_header().map_err(map_err)? else {
            break;
        };
        let name = header.entry().filename.to_string_lossy().replace('\\', "/");
        let size = header.entry().unpacked_size;
        if !is_safe_relative(&name) {
            open = header.skip().map_err(map_err)?;
            continue;
        }
        let target = dest.join(&name);
        if header.entry().is_directory() {
            std::fs::create_dir_all(&target).map_err(|e| Error::io(&target, e))?;
            open = header.skip().map_err(map_err)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        open = header
            .extract_to(&target)
            .map_err(|e| Error::Archive(format!("{name}: {e}")))?;
        files += 1;
        done += size;
        if let Some(p) = progress.as_mut() {
            p(done, summary.unpacked_bytes, &name);
        }
    }
    Ok(files)
}

/// Extract into a staging folder beside `final_dir`, then swap it into place.
/// An existing `final_dir` is moved aside and removed only after the swap
/// succeeded. Returns the number of files written.
pub fn extract_atomically(
    archive: &Path,
    final_dir: &Path,
    cancel: Option<Arc<AtomicBool>>,
    progress: Option<Progress<'_>>,
) -> Result<u64> {
    let parent = final_dir
        .parent()
        .ok_or_else(|| Error::Archive("destination has no parent".into()))?;
    let staging = parent.join(".nll-staging");
    let old = parent.join(".nll-old");
    let _ = std::fs::remove_dir_all(&staging);
    let _ = std::fs::remove_dir_all(&old);
    let files = match extract(archive, &staging, cancel, progress) {
        Ok(n) => n,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    if final_dir.exists() {
        std::fs::rename(final_dir, &old).map_err(|e| Error::io(final_dir, e))?;
    }
    if let Err(e) = std::fs::rename(&staging, final_dir) {
        // try to restore the previous installation
        if old.exists() {
            let _ = std::fs::rename(&old, final_dir);
        }
        return Err(Error::io(final_dir, e));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(files)
}

/// Convenience for tests and diagnostics: is this file a readable RAR?
pub fn is_rar(path: &Path) -> bool {
    Archive::new(path).open_for_listing().is_ok()
}

pub fn staging_dir(final_dir: &Path) -> PathBuf {
    final_dir.parent().unwrap_or(final_dir).join(".nll-staging")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn verifies_intact_archive() {
        let v = verify(&fixture("sample_game.rar"), None, None).unwrap();
        match v {
            Verification::Ok(s) => {
                assert_eq!(s.files, 3);
                assert_eq!(s.unpacked_bytes, 10 + 5 + 3000);
                assert!(s.unsafe_entries.is_empty());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn detects_truncated_archive() {
        let v = verify(&fixture("truncated_game.rar"), None, None).unwrap();
        assert!(
            matches!(v, Verification::Damaged { entries_ok: 2, .. }),
            "{v:?}"
        );
    }

    #[test]
    fn rejects_non_archives() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.eti");
        std::fs::write(&p, crate::transport::demo::demo_archive_bytes(4096)).unwrap();
        let v = verify(&p, None, None).unwrap();
        assert!(matches!(v, Verification::NotAnArchive { .. }));
        assert!(!is_rar(&p));
    }

    #[test]
    fn matches_extracted_detects_complete_and_incomplete_installs() {
        let tmp = tempfile::tempdir().unwrap();
        let local = tmp.path().join("local");
        let archive = fixture("sample_game.rar");
        assert_eq!(matches_extracted(&archive, &local).unwrap(), None);
        extract_atomically(&archive, &local, None, None).unwrap();
        assert_eq!(matches_extracted(&archive, &local).unwrap(), Some(3));
        // A rewritten small file (config edited by a setup script) is fine…
        std::fs::write(local.join("game.exe"), "x").unwrap();
        assert_eq!(matches_extracted(&archive, &local).unwrap(), Some(3));
        // …but a size mismatch on a file above the threshold is not.
        assert_eq!(matches_extracted_with(&archive, &local, 0).unwrap(), None);
        std::fs::remove_file(local.join("game.exe")).unwrap();
        assert_eq!(matches_extracted(&archive, &local).unwrap(), None);
        assert!(matches_extracted(
            &fixture("truncated_game.rar").with_extension("missing"),
            &local
        )
        .is_err());
    }

    #[test]
    fn extracts_atomically_and_replaces_old_install() {
        let tmp = tempfile::tempdir().unwrap();
        let local = tmp.path().join("local");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(local.join("stale.txt"), "old").unwrap();
        let mut seen = Vec::new();
        let n = extract_atomically(
            &fixture("sample_game.rar"),
            &local,
            None,
            Some(&mut |done, total, name| seen.push((done, total, name.to_string()))),
        )
        .unwrap();
        assert_eq!(n, 3);
        assert_eq!(
            std::fs::read_to_string(local.join("game.exe")).unwrap(),
            "hello lan\n"
        );
        assert!(local.join("sub").join("data.bin").is_file());
        assert!(!local.join("stale.txt").exists());
        assert!(!tmp.path().join(".nll-staging").exists());
        assert!(!tmp.path().join(".nll-old").exists());
        assert_eq!(seen.last().unwrap().0, 3015);
    }

    #[test]
    fn failed_extraction_keeps_old_install() {
        let tmp = tempfile::tempdir().unwrap();
        let local = tmp.path().join("local");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::write(local.join("keep.txt"), "keep").unwrap();
        let err = extract_atomically(&fixture("truncated_game.rar"), &local, None, None);
        assert!(err.is_err());
        assert!(local.join("keep.txt").is_file());
        assert!(!tmp.path().join(".nll-staging").exists());
    }

    #[test]
    fn cancellation_stops_extraction() {
        let tmp = tempfile::tempdir().unwrap();
        let flag = Arc::new(AtomicBool::new(true));
        let err = extract(&fixture("sample_game.rar"), tmp.path(), Some(flag), None).unwrap_err();
        assert!(err.to_string().contains("cancelled"));
    }
}
