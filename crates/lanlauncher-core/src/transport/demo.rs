//! Simulated transport for development, tests and screenshots.
//!
//! `add_share` creates the share directory and writes a growing
//! `<id>.eti.!sync` file that is renamed to `<id>.eti` when "complete", plus a
//! `version.ini`. Optionally the simulated engine reports a stuck 99 % even
//! though the file is complete – the exact failure mode seen at the LAN.

use super::*;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct DemoShare {
    key: ShareKey,
    started: Instant,
    duration: Duration,
    total: u64,
    paused: bool,
    /// Report 99 % forever even when the file is complete.
    stuck_at_99: bool,
    /// Revision written to version.ini.
    revision: String,
    finished_written: bool,
    archive_source: Option<PathBuf>,
}

#[derive(Debug, Default)]
pub struct DemoTransport {
    shares: Mutex<HashMap<PathBuf, DemoShare>>,
    lan_only: Mutex<bool>,
    /// Speed of simulated downloads.
    pub duration: Duration,
    pub stuck_at_99: bool,
    pub archive_bytes: u64,
    pub revision: String,
    /// Copy this real archive as the completed `.eti` instead of fake bytes,
    /// so the full verify/extract pipeline can be exercised.
    pub archive_source: Option<PathBuf>,
}

impl DemoTransport {
    pub fn new() -> Self {
        Self {
            shares: Mutex::new(HashMap::new()),
            lan_only: Mutex::new(true),
            duration: Duration::from_secs(20),
            stuck_at_99: false,
            archive_bytes: 4096,
            revision: "20250308".into(),
            archive_source: None,
        }
    }

    /// Materialise the simulated files for a share according to elapsed time.
    fn tick(dir: &Path, share: &mut DemoShare) -> Result<ShareStatus> {
        let elapsed = if share.paused {
            Duration::ZERO
        } else {
            share.started.elapsed()
        };
        let frac = (elapsed.as_secs_f64() / share.duration.as_secs_f64()).clamp(0.0, 1.0);
        let done = (share.total as f64 * frac) as u64;
        std::fs::create_dir_all(dir).map_err(|e| crate::Error::io(dir, e))?;
        let final_path = dir.join(format!(
            "{}.eti",
            dir.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        ));
        let partial = partial_path(&final_path);
        if frac >= 1.0 {
            if !share.finished_written {
                match &share.archive_source {
                    Some(src) => {
                        std::fs::copy(src, &final_path)
                            .map_err(|e| crate::Error::io(&final_path, e))?;
                    }
                    None => std::fs::write(&final_path, demo_archive_bytes(share.total))
                        .map_err(|e| crate::Error::io(&final_path, e))?,
                }
                let _ = std::fs::remove_file(&partial);
                let v = dir.join(crate::paths::VERSION_FILE);
                std::fs::write(&v, &share.revision).map_err(|e| crate::Error::io(&v, e))?;
                share.finished_written = true;
            }
        } else {
            std::fs::write(&partial, vec![0u8; done as usize])
                .map_err(|e| crate::Error::io(&partial, e))?;
        }
        let reported_done = if share.stuck_at_99 {
            done.min(share.total * 99 / 100)
        } else {
            done
        };
        let state = if share.paused {
            ShareState::Paused
        } else if frac >= 1.0 && !share.stuck_at_99 {
            ShareState::Complete
        } else {
            ShareState::Downloading
        };
        Ok(ShareStatus {
            dir: dir.to_path_buf(),
            state,
            bytes_done: reported_done,
            bytes_total: share.total,
            files_total: 3,
            peers: if share.paused { 0 } else { 3 },
            download_bps: if state == ShareState::Downloading {
                (share.total as f64 / share.duration.as_secs_f64()) as u64
            } else {
                0
            },
            upload_bps: 0,
            error: None,
        })
    }
}

/// Deterministic bytes for a fake archive: a recognisable header followed by
/// zeros. Not a valid RAR; tests that need a real archive use the fixture.
pub fn demo_archive_bytes(total: u64) -> Vec<u8> {
    let mut v = b"NLL-DEMO-ARCHIVE".to_vec();
    v.resize(total as usize, 0);
    v
}

#[async_trait]
impl Transport for DemoTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Demo
    }
    async fn start(&self) -> Result<()> {
        Ok(())
    }
    async fn stop(&self) -> Result<()> {
        Ok(())
    }
    async fn health(&self) -> TransportHealth {
        TransportHealth {
            kind: TransportKind::Demo,
            running: true,
            api_reachable: true,
            version: Some("demo".into()),
            peers: 3,
            catalog_peers: 3,
            server_found: Some(true),
            lan_mode: *self.lan_only.lock().unwrap_or_else(|e| e.into_inner()),
            peer_details: false,
            detail: Some("simulated transport".into()),
        }
    }
    async fn add_share(&self, key: &ShareKey, dir: &Path, opts: &ShareOptions) -> Result<()> {
        let mut shares = self
            .shares
            .lock()
            .map_err(|_| crate::Error::Transport("lock".into()))?;
        shares.entry(dir.to_path_buf()).or_insert(DemoShare {
            key: key.clone(),
            started: Instant::now(),
            duration: self.duration,
            total: self.archive_bytes,
            paused: opts.paused,
            stuck_at_99: self.stuck_at_99,
            revision: self.revision.clone(),
            finished_written: false,
            archive_source: self.archive_source.clone(),
        });
        Ok(())
    }
    async fn remove_share(&self, dir: &Path) -> Result<()> {
        if let Ok(mut s) = self.shares.lock() {
            s.remove(dir);
        }
        Ok(())
    }
    async fn set_paused(&self, dir: &Path, paused: bool) -> Result<()> {
        if let Ok(mut s) = self.shares.lock() {
            if let Some(sh) = s.get_mut(dir) {
                if sh.paused && !paused {
                    sh.started = Instant::now();
                }
                sh.paused = paused;
            }
        }
        Ok(())
    }
    async fn share_status(&self, dir: &Path) -> Result<Option<ShareStatus>> {
        let mut shares = self
            .shares
            .lock()
            .map_err(|_| crate::Error::Transport("lock".into()))?;
        match shares.get_mut(dir) {
            Some(share) => Ok(Some(Self::tick(dir, share)?)),
            None => Ok(None),
        }
    }
    async fn list_shares(&self) -> Result<Vec<ShareStatus>> {
        let mut shares = self
            .shares
            .lock()
            .map_err(|_| crate::Error::Transport("lock".into()))?;
        let mut out = Vec::new();
        for (dir, share) in shares.iter_mut() {
            out.push(Self::tick(dir, share)?);
        }
        Ok(out)
    }
    async fn set_lan_mode(&self, lan_only: bool) -> Result<()> {
        if let Ok(mut l) = self.lan_only.lock() {
            *l = lan_only;
        }
        Ok(())
    }
}

impl DemoShare {
    pub fn key(&self) -> &ShareKey {
        &self.key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn demo_share_completes_and_writes_files() {
        let tmp = tempfile::tempdir().unwrap();
        let mut t = DemoTransport::new();
        t.duration = Duration::from_millis(50);
        t.stuck_at_99 = true;
        let key = ShareKey::parse("BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").unwrap();
        let dir = tmp.path().join("amongus");
        t.add_share(&key, &dir, &ShareOptions::default())
            .await
            .unwrap();
        let s = t.share_status(&dir).await.unwrap().unwrap();
        assert_eq!(s.state, ShareState::Downloading);
        tokio::time::sleep(Duration::from_millis(80)).await;
        let s = t.share_status(&dir).await.unwrap().unwrap();
        // engine still claims 99 %, but the file is complete on disk
        assert_eq!(s.state, ShareState::Downloading);
        assert!(s.progress() < 1.0);
        assert!(dir.join("amongus.eti").is_file());
        assert!(!dir.join("amongus.eti.!sync").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("version.ini")).unwrap(),
            "20250308"
        );
    }
}
