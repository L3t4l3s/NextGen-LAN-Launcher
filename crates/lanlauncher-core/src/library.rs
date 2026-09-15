//! Library roots: the folders games are synced into.
//!
//! ETI used exactly one root (`X:\LAN`). Players with several SSDs had to move
//! folders around by hand, so we support a list of roots from the start. Each
//! game lives in exactly one root; new installs go to the root with the most
//! free space unless the user picked one explicitly.

use crate::paths::GamePaths;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRoot {
    pub path: PathBuf,
    /// Friendly label shown in the UI (`SSD D:`); defaults to the path.
    #[serde(default)]
    pub label: String,
    /// The root that receives the catalog share and new installs by default.
    #[serde(default)]
    pub is_default: bool,
}

impl LibraryRoot {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            label: path.to_string_lossy().to_string(),
            path,
            is_default: false,
        }
    }
}

/// Free/total bytes of the volume a path lives on. Returns `None` if the path
/// does not exist yet or the platform does not report disk information.
pub fn disk_space(path: &Path) -> Option<(u64, u64)> {
    let canonical = path.canonicalize().ok()?;
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|d| canonical.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| (d.available_space(), d.total_space()))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    pub roots: Vec<LibraryRoot>,
}

impl Library {
    pub fn default_root(&self) -> Option<&LibraryRoot> {
        self.roots
            .iter()
            .find(|r| r.is_default)
            .or_else(|| self.roots.first())
    }

    pub fn add_root(&mut self, mut root: LibraryRoot) {
        if self.roots.is_empty() {
            root.is_default = true;
        }
        if root.is_default {
            for r in &mut self.roots {
                r.is_default = false;
            }
        }
        if let Some(existing) = self.roots.iter_mut().find(|r| r.path == root.path) {
            *existing = root;
        } else {
            self.roots.push(root);
        }
    }

    pub fn remove_root(&mut self, path: &Path) {
        let was_default = self.roots.iter().any(|r| r.path == path && r.is_default);
        self.roots.retain(|r| r.path != path);
        if was_default {
            if let Some(first) = self.roots.first_mut() {
                first.is_default = true;
            }
        }
    }

    /// Find the root that already contains this game (share dir exists).
    pub fn locate_game(&self, game_id: &str) -> Option<&LibraryRoot> {
        self.roots.iter().find(|r| r.path.join(game_id).is_dir())
    }

    /// Root to use for a new install: an existing location wins, then the
    /// root with the most free space that fits `needed_bytes`, then default.
    pub fn choose_root_for(&self, game_id: &str, needed_bytes: u64) -> Option<&LibraryRoot> {
        if let Some(r) = self.locate_game(game_id) {
            return Some(r);
        }
        let mut best: Option<(&LibraryRoot, u64)> = None;
        for r in &self.roots {
            if let Some((free, _)) = disk_space(&r.path) {
                if free >= needed_bytes && best.map(|(_, f)| free > f).unwrap_or(true) {
                    best = Some((r, free));
                }
            }
        }
        best.map(|(r, _)| r).or_else(|| self.default_root())
    }

    pub fn game_paths(&self, game_id: &str) -> Option<GamePaths> {
        self.locate_game(game_id)
            .or_else(|| self.default_root())
            .map(|r| GamePaths::new(&r.path, game_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_root_becomes_default_and_default_is_exclusive() {
        let mut lib = Library::default();
        lib.add_root(LibraryRoot::new("/a"));
        assert!(lib.roots[0].is_default);
        let mut b = LibraryRoot::new("/b");
        b.is_default = true;
        lib.add_root(b);
        assert!(!lib.roots[0].is_default);
        assert!(lib.roots[1].is_default);
        lib.remove_root(Path::new("/b"));
        assert!(lib.roots[0].is_default);
    }

    #[test]
    fn locate_prefers_existing_install() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::create_dir_all(b.join("quake3")).unwrap();
        std::fs::create_dir_all(&a).unwrap();
        let mut lib = Library::default();
        lib.add_root(LibraryRoot::new(&a));
        lib.add_root(LibraryRoot::new(&b));
        assert_eq!(lib.choose_root_for("quake3", 0).unwrap().path, b);
        assert_eq!(
            lib.game_paths("quake3").unwrap().local_dir,
            b.join("quake3").join("local")
        );
        // unknown game goes to a root with space (both on same disk here → first with most space)
        assert!(lib.choose_root_for("newgame", 0).is_some());
    }
}
