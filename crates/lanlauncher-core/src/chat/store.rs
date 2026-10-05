//! The chat history and this launcher's chat identity on disk.
//!
//! The history is one JSON line per event (`history.jsonl`) in the form it
//! travels — signed, private bodies sealed — appended as events arrive. The
//! chat keeps the newest [`HISTORY_LIMIT`] events and rewrites the file when
//! it trims ([`super::model::ChatState::trim`]).

use super::crypto::Keys;
use super::model::Stored;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Events kept across restarts. A long LAN weekend stays well below it, and
/// a `Hello` listing all of their ids stays well below the frame limit.
pub const HISTORY_LIMIT: usize = 5000;

pub struct History {
    path: PathBuf,
}

impl History {
    /// Read what is there; unreadable lines are skipped. The flag says
    /// whether any were, so the caller rewrites the file without them.
    pub fn open(dir: &Path) -> (Self, Vec<Stored>, bool) {
        let path = dir.join("history.jsonl");
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let stored: Vec<Stored> = text
            .lines()
            .filter_map(|l| serde_json::from_str::<Stored>(l).ok())
            .collect();
        let damaged = text.lines().count() != stored.len();
        (Self { path }, stored, damaged)
    }

    pub fn append(&self, entries: &[Stored]) {
        if entries.is_empty() {
            return;
        }
        let mut text = String::new();
        for e in entries {
            if let Ok(line) = serde_json::to_string(e) {
                text.push_str(&line);
                text.push('\n');
            }
        }
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| f.write_all(text.as_bytes()));
        if let Err(e) = written {
            log::warn!("chat history {}: {e}", self.path.display());
        }
    }

    /// Replace the file with exactly `entries` (temp file + rename).
    pub fn rewrite(&self, entries: &[Stored]) {
        let mut text = String::new();
        for e in entries {
            if let Ok(line) = serde_json::to_string(e) {
                text.push_str(&line);
                text.push('\n');
            }
        }
        let tmp = self.path.with_extension("jsonl.tmp");
        if let Err(e) = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &self.path)) {
            log::warn!("chat history {}: {e}", self.path.display());
        }
    }
}

/// Ids that expired here ([`super::model::ChatState::expire`]), so peers
/// cannot hand them back after a restart. Unreadable means none.
pub fn load_gone(dir: &Path) -> std::collections::HashMap<String, i64> {
    std::fs::read_to_string(dir.join("expired.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_gone(dir: &Path, gone: &std::collections::HashMap<String, i64>) {
    let path = dir.join("expired.json");
    let tmp = path.with_extension("json.tmp");
    let written = serde_json::to_string(gone)
        .map_err(std::io::Error::other)
        .and_then(|json| std::fs::write(&tmp, json))
        .and_then(|_| std::fs::rename(&tmp, &path));
    if let Err(e) = written {
        log::warn!("chat {}: {e}", path.display());
    }
}

/// This launcher's key pair, made once and kept in `identity.json`. Its
/// public half is the peer id, which keeps a private conversation together
/// when the nickname changes. The file holds a secret: it stays in the
/// launcher's own data directory.
///
/// Only a missing file means "new launcher". One that cannot be read right
/// now (locked by a virus scanner, say) is an error: the chat does not start
/// rather than replace an identity whose private messages would then be
/// unreadable. One that is unreadable for good is kept aside, not deleted.
pub fn load_keys(dir: &Path) -> std::io::Result<Keys> {
    let path = dir.join("identity.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let keys = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| Keys::from_secret_hex(v.get("secret")?.as_str()?));
            if let Some(keys) = keys {
                return Ok(keys);
            }
            let aside = dir.join("identity.json.broken");
            log::warn!(
                "chat identity {} unreadable; kept as {} and a new one made",
                path.display(),
                aside.display()
            );
            std::fs::rename(&path, &aside)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let keys = Keys::generate();
    let json = serde_json::json!({ "secret": keys.secret_hex() }).to_string();
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &path))?;
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::model::{Body, Event};

    fn stored(seq: u64) -> Stored {
        Stored {
            seen: seq as i64,
            event: Event {
                id: format!("a:{seq}"),
                from: "a".into(),
                nick: "A".into(),
                seq,
                ts: seq as i64,
                to: None,
                body: Body::Text {
                    text: "hi".into(),
                    reply_to: None,
                },
                topic: None,
                sig: "00".into(),
            },
            born: None,
        }
    }

    #[test]
    fn history_survives_a_restart_and_reports_broken_lines() {
        let dir = tempfile::tempdir().unwrap();
        let (h, loaded, damaged) = History::open(dir.path());
        assert!(loaded.is_empty() && !damaged);
        let (a, b) = (stored(1), stored(2));
        h.append(std::slice::from_ref(&a));
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join("history.jsonl"))
            .unwrap();
        writeln!(f, "{{not json").unwrap();
        h.append(std::slice::from_ref(&b));
        let (h, loaded, damaged) = History::open(dir.path());
        assert_eq!(loaded, vec![a.clone(), b.clone()]);
        assert!(damaged);
        h.rewrite(&loaded);
        let (_, again, damaged) = History::open(dir.path());
        assert_eq!(again, vec![a, b]);
        assert!(!damaged);
    }

    #[test]
    fn identity_is_made_once() {
        let dir = tempfile::tempdir().unwrap();
        let id = load_keys(dir.path()).unwrap().id().to_string();
        assert_eq!(id.len(), 64);
        assert_eq!(load_keys(dir.path()).unwrap().id(), id);
    }

    #[test]
    fn a_damaged_identity_is_kept_aside_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("identity.json"), "{half").unwrap();
        let keys = load_keys(dir.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("identity.json.broken")).unwrap(),
            "{half"
        );
        assert_eq!(load_keys(dir.path()).unwrap().id(), keys.id());
    }
}
