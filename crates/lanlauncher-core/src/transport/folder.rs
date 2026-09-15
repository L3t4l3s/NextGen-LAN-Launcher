//! Passive transport: the user runs their own sync client.
//!
//! We cannot add shares ourselves; instead the UI shows the key to paste into
//! Resilio (like macETI-LAN does). Status is derived from what is on disk.

use super::*;
use std::sync::Mutex;

#[derive(Debug, Default)]
pub struct FolderTransport {
    /// Shares the user asked for; remembered so the UI can list pending keys.
    requested: Mutex<Vec<(PathBuf, ShareKey)>>,
}

impl FolderTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keys the user still has to add manually (dir does not exist yet).
    pub fn pending_keys(&self) -> Vec<(PathBuf, ShareKey)> {
        self.requested
            .lock()
            .map(|r| r.iter().filter(|(d, _)| !d.is_dir()).cloned().collect())
            .unwrap_or_default()
    }

    /// Estimate status of a share directory from the files inside it.
    pub fn status_from_disk(dir: &Path) -> Option<ShareStatus> {
        if !dir.is_dir() {
            return None;
        }
        let mut done = 0u64;
        let mut partial = 0u64;
        let mut files = 0u64;
        for entry in walkdir::WalkDir::new(dir)
            .max_depth(1)
            .into_iter()
            .flatten()
        {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            files += 1;
            let name = entry.file_name().to_string_lossy();
            if name.ends_with(PARTIAL_SUFFIX) {
                partial += meta.len();
            } else {
                done += meta.len();
            }
        }
        let state = if partial > 0 {
            ShareState::Downloading
        } else if files == 0 {
            ShareState::Pending
        } else {
            ShareState::Complete
        };
        Some(ShareStatus {
            dir: dir.to_path_buf(),
            state,
            bytes_done: done + partial,
            // unknown total: report what we have; the installer uses the
            // catalog size for the progress bar.
            bytes_total: 0,
            files_total: files,
            peers: 0,
            download_bps: 0,
            upload_bps: 0,
            error: None,
        })
    }
}

#[async_trait]
impl Transport for FolderTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Folder
    }
    async fn start(&self) -> Result<()> {
        Ok(())
    }
    async fn stop(&self) -> Result<()> {
        Ok(())
    }
    async fn health(&self) -> TransportHealth {
        TransportHealth {
            kind: TransportKind::Folder,
            running: true,
            api_reachable: false,
            version: None,
            peers: 0,
            catalog_peers: 0,
            server_found: None,
            lan_mode: false,
            detail: Some("folder mode: sync client is managed by the user".into()),
        }
    }
    async fn add_share(&self, key: &ShareKey, dir: &Path, _opts: &ShareOptions) -> Result<()> {
        let mut r = self
            .requested
            .lock()
            .map_err(|_| crate::Error::Transport("lock".into()))?;
        if !r.iter().any(|(d, _)| d == dir) {
            r.push((dir.to_path_buf(), key.clone()));
        }
        Ok(())
    }
    async fn remove_share(&self, dir: &Path) -> Result<()> {
        if let Ok(mut r) = self.requested.lock() {
            r.retain(|(d, _)| d != dir);
        }
        Ok(())
    }
    async fn set_paused(&self, _dir: &Path, _paused: bool) -> Result<()> {
        Ok(())
    }
    async fn share_status(&self, dir: &Path) -> Result<Option<ShareStatus>> {
        Ok(Self::status_from_disk(dir))
    }
    async fn list_shares(&self) -> Result<Vec<ShareStatus>> {
        let dirs: Vec<PathBuf> = self
            .requested
            .lock()
            .map(|r| r.iter().map(|(d, _)| d.clone()).collect())
            .unwrap_or_default();
        Ok(dirs
            .iter()
            .filter_map(|d| Self::status_from_disk(d))
            .collect())
    }
    async fn set_lan_mode(&self, _lan_only: bool) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_status_detects_partial_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("quake3");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            FolderTransport::status_from_disk(&dir).unwrap().state,
            ShareState::Pending
        );
        std::fs::write(dir.join("quake3.eti.!sync"), vec![0u8; 100]).unwrap();
        let s = FolderTransport::status_from_disk(&dir).unwrap();
        assert_eq!(s.state, ShareState::Downloading);
        assert_eq!(s.bytes_done, 100);
        std::fs::rename(dir.join("quake3.eti.!sync"), dir.join("quake3.eti")).unwrap();
        std::fs::write(dir.join("version.ini"), "20160922").unwrap();
        let s = FolderTransport::status_from_disk(&dir).unwrap();
        assert_eq!(s.state, ShareState::Complete);
        assert_eq!(s.files_total, 2);
    }
}
