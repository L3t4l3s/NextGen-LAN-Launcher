//! Library roots: the folders games are synced into.
//!
//! ETI used exactly one root (`X:\LAN`). Players with several SSDs had to move
//! folders around by hand, so we support a list of roots from the start. Each
//! game lives in exactly one root; new installs go to the root with the most
//! free space unless the user picked one explicitly.

use crate::paths::{strip_verbatim, GamePaths};
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

/// Does this folder hold anything of the game? The engine's own `.sync`
/// bookkeeping is not a game: a share the user removed leaves it behind, and
/// it would pin the game to that disk for ever. `None` when the folder does
/// not exist or cannot be read.
fn dir_is_empty(dir: &Path) -> Option<bool> {
    let mut entries = std::fs::read_dir(dir).ok()?;
    Some(!entries.any(|e| {
        e.map(|e| e.file_name().to_string_lossy().to_ascii_lowercase() != ".sync")
            .unwrap_or(false)
    }))
}

/// Free/total bytes of the volume a path lives on. Returns `None` if the path
/// does not exist yet or the platform does not report disk information.
/// Prefer [`DiskTable`] when querying many paths in one go.
pub fn disk_space(path: &Path) -> Option<(u64, u64)> {
    DiskTable::refresh().space_for(path)
}

/// Snapshot of mounted volumes, refreshed once per polling round so that
/// many games do not each re-enumerate every mount.
#[derive(Debug, Default)]
pub struct DiskTable {
    mounts: Vec<(PathBuf, u64, u64)>,
}

impl DiskTable {
    pub fn refresh() -> Self {
        let disks = sysinfo::Disks::new_with_refreshed_list();
        Self {
            mounts: disks
                .iter()
                .map(|d| {
                    (
                        strip_verbatim(d.mount_point().to_path_buf()),
                        d.available_space(),
                        d.total_space(),
                    )
                })
                .collect(),
        }
    }

    /// (free, total) bytes of the volume holding `path` or its nearest
    /// existing parent.
    pub fn space_for(&self, path: &Path) -> Option<(u64, u64)> {
        let mut p = Some(path);
        let canonical = loop {
            let cur = p?;
            if let Ok(c) = cur.canonicalize() {
                break strip_verbatim(c);
            }
            p = cur.parent();
        };
        self.mounts
            .iter()
            .filter(|(m, _, _)| canonical.starts_with(m))
            .max_by_key(|(m, _, _)| m.as_os_str().len())
            .map(|(_, free, total)| (*free, *total))
    }

    pub fn free_for(&self, path: &Path) -> Option<u64> {
        self.space_for(path).map(|(free, _)| free)
    }
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

    /// Like [`Library::locate_game`], but an empty folder does not count.
    /// A cancelled or failed install leaves one behind, and it would pin the
    /// game to that disk for good — including to one without room for it.
    fn locate_game_with_content(&self, game_id: &str) -> Option<&LibraryRoot> {
        self.roots
            .iter()
            .find(|r| !dir_is_empty(&r.path.join(game_id)).unwrap_or(true))
    }

    /// Root to use for a new install: where the game already is, else the
    /// default root when `needed_bytes` fit there, else the root with the
    /// most free space that fits, else the default root anyway.
    ///
    /// Enumerates the volumes, so this belongs on the install path, not in a
    /// loop: everything that only wants to know where a game *is* takes
    /// [`Library::game_paths`].
    pub fn choose_root_for(&self, game_id: &str, needed_bytes: u64) -> Option<&LibraryRoot> {
        // A game that is already somewhere needs no volume enumeration at
        // all; only a new one pays for the snapshot, and then once for every
        // root rather than once per root.
        if let Some(r) = self.locate_game_with_content(game_id) {
            return Some(r);
        }
        let disks = DiskTable::refresh();
        self.choose_root_with(game_id, needed_bytes, |p| disks.free_for(p))
    }

    /// [`Library::choose_root_for`] with the free space injected, so the rule
    /// can be tested without two real volumes.
    pub fn choose_root_with(
        &self,
        game_id: &str,
        needed_bytes: u64,
        free_for: impl Fn(&Path) -> Option<u64>,
    ) -> Option<&LibraryRoot> {
        if let Some(r) = self.locate_game_with_content(game_id) {
            return Some(r);
        }
        // The root the user marked as the default is a choice, not a
        // suggestion: free space only decides when that one has no room.
        if let Some(r) = self
            .default_root()
            .filter(|r| free_for(&r.path).is_some_and(|free| free >= needed_bytes))
        {
            return Some(r);
        }
        let mut best: Option<(&LibraryRoot, u64)> = None;
        for r in &self.roots {
            if let Some(free) = free_for(&r.path) {
                if free >= needed_bytes && best.map(|(_, f)| free > f).unwrap_or(true) {
                    best = Some((r, free));
                }
            }
        }
        if best.is_none() {
            // Nothing fits anywhere. The game goes to the default root and the
            // state machine says so with the numbers; silently picking the
            // biggest of several too-small disks would only hide it.
            log::warn!(
                "{game_id} needs {needed_bytes} bytes and no library root has room; using the default"
            );
        }
        best.map(|(r, _)| r).or_else(|| self.default_root())
    }

    /// Where a game is, or would go by default. For a *new* install prefer
    /// [`Library::game_paths_for`]: it is what puts a game on the disk with
    /// room for it.
    pub fn game_paths(&self, game_id: &str) -> Option<GamePaths> {
        // A root that actually holds something wins over one with an empty
        // leftover folder: otherwise a download running on the second disk is
        // watched on the first, which reports no progress for ever.
        self.locate_game_with_content(game_id)
            .or_else(|| self.locate_game(game_id))
            .or_else(|| self.default_root())
            .map(|r| GamePaths::new(&r.path, game_id))
    }

    /// Empty `<root>/<game_id>` folders in every root but `keep`. A cancelled
    /// install leaves one behind, and it decides where later lookups go.
    pub fn empty_leftovers(&self, game_id: &str, keep: &Path) -> Vec<PathBuf> {
        self.roots
            .iter()
            .map(|r| r.path.join(game_id))
            .filter(|dir| dir != keep)
            .filter(|dir| dir_is_empty(dir).unwrap_or(false))
            .collect()
    }

    /// Paths for a game of known size, choosing the root as
    /// [`Library::choose_root_for`] describes.
    pub fn game_paths_for(&self, game_id: &str, needed_bytes: u64) -> Option<GamePaths> {
        self.choose_root_for(game_id, needed_bytes)
            .map(|r| GamePaths::new(&r.path, game_id))
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_new_game_goes_where_it_fits() {
        // Two roots, the first one too small: the settings promise the game
        // lands where there is room, and until now it always took the first.
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        std::fs::create_dir_all(&small).unwrap();
        std::fs::create_dir_all(&big).unwrap();
        let mut lib = Library::default();
        lib.add_root(LibraryRoot::new(&small));
        lib.add_root(LibraryRoot::new(&big));

        // An install that already exists stays where it is, whatever the
        // free space says.
        std::fs::create_dir_all(small.join("quake3")).unwrap();
        std::fs::write(small.join("quake3").join("quake3.eti"), b"x").unwrap();
        let paths = lib.game_paths_for("quake3", u64::MAX).unwrap();
        assert_eq!(paths.share_dir, small.join("quake3"));

        // A game nobody has yet goes to the root with room for it, not to
        // the first one in the list.
        let free = |p: &Path| Some(if p == small { 10 } else { 900 });
        let chosen = lib.choose_root_with("unknown", 500, free).unwrap();
        assert_eq!(chosen.path, big);

        // An existing install is never moved, however little room is left.
        let chosen = lib.choose_root_with("quake3", 500, free).unwrap();
        assert_eq!(chosen.path, small);

        // The default root has room: it stays the choice, even though the
        // other one is larger. Picking a folder is the user's decision.
        let chosen = lib.choose_root_with("unknown", 5, free).unwrap();
        assert_eq!(chosen.path, small);

        // Nothing fits anywhere: the default root is still where it goes, and
        // the disk check says the rest.
        let chosen = lib.choose_root_with("unknown", u64::MAX, free).unwrap();
        assert_eq!(chosen.path, small);
    }
    use super::*;

    #[test]
    fn verbatim_prefixes_are_stripped() {
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\D:\LAN")),
            PathBuf::from(r"D:\LAN")
        );
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\UNC\nas\share")),
            PathBuf::from(r"\\nas\share")
        );
        assert_eq!(strip_verbatim(PathBuf::from("/lan")), PathBuf::from("/lan"));
    }

    #[test]
    fn disk_table_reports_space_for_existing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let table = DiskTable::refresh();
        let nested = tmp.path().join("not-yet-created");
        assert_eq!(
            table.free_for(tmp.path()).is_some(),
            table.free_for(&nested).is_some()
        );
    }

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
    fn an_empty_folder_does_not_pin_a_game_to_a_full_disk() {
        // A cancelled install leaves `D:\LAN\bf4` behind. It must not decide
        // where the next attempt goes — that is how a 115 GB game ended up on
        // the disk without room while another one had 500 GB free.
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small");
        let big = dir.path().join("big");
        std::fs::create_dir_all(small.join("bf4")).unwrap();
        std::fs::create_dir_all(&big).unwrap();
        let mut lib = Library::default();
        lib.add_root(LibraryRoot::new(&small));
        lib.add_root(LibraryRoot::new(&big));
        let free = |p: &Path| Some(if p == small { 10 } else { 900 });
        assert_eq!(lib.choose_root_with("bf4", 500, free).unwrap().path, big);
        // …and the leftover is named so the install can clear it away.
        assert_eq!(
            lib.empty_leftovers("bf4", &big.join("bf4")),
            vec![small.join("bf4")]
        );
        // The engine's own bookkeeping folder is not an install either.
        std::fs::create_dir_all(small.join("bf4").join(".sync")).unwrap();
        assert_eq!(lib.choose_root_with("bf4", 500, free).unwrap().path, big);
        assert_eq!(
            lib.empty_leftovers("bf4", &big.join("bf4")),
            vec![small.join("bf4")]
        );
        // Once something is in it, it is an install again and stays put.
        std::fs::write(small.join("bf4").join("bf4.eti.!sync"), b"x").unwrap();
        assert_eq!(lib.choose_root_with("bf4", 500, free).unwrap().path, small);
        assert!(lib.empty_leftovers("bf4", &big.join("bf4")).is_empty());
    }

    #[test]
    fn locate_prefers_existing_install() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::create_dir_all(b.join("quake3")).unwrap();
        std::fs::write(b.join("quake3").join("quake3.eti"), b"x").unwrap();
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
