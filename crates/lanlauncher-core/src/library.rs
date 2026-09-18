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
    /// Search every configured root, preferring the effective default. A
    /// missing, locked or partially synced database must not hide a usable
    /// catalog on another drive.
    pub fn load_catalog(&self) -> Option<(&LibraryRoot, crate::catalog::Catalog)> {
        let default = self.default_root();
        default
            .into_iter()
            .chain(self.roots.iter().filter(|r| Some(*r) != default))
            .find_map(|root| {
                crate::catalog::Catalog::load(&root.path.join(crate::paths::CATALOG_RELATIVE))
                    .ok()
                    .map(|catalog| (root, catalog))
            })
    }

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
    /// default root when `needed_bytes` fit there, else the root with the most
    /// free space that fits — and when it fits nowhere, the roomiest root
    /// there is (the default one on a tie).
    ///
    /// Enumerates the volumes, so this belongs on the install path, not in a
    /// loop: everything that only wants to know where a game *is* takes
    /// [`Library::game_paths`].
    pub fn choose_root_for(&self, game_id: &str, needed_bytes: u64) -> Option<&LibraryRoot> {
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
            let paths = GamePaths::new(&r.path, game_id);
            // Installed games stay put. An unfinished download only stays
            // when the remaining reservation fits; otherwise resume it on
            // a configured volume with room for the entire installation.
            let remaining = needed_bytes.saturating_sub(download_bytes(&paths.share_dir));
            if paths.local_dir.exists()
                || paths.receipt.exists()
                || free_for(&r.path).is_none_or(|free| free >= remaining)
                || !self.roots.iter().any(|other| {
                    other.path != r.path
                        && free_for(&other.path).is_some_and(|free| free >= needed_bytes)
                })
            {
                return Some(r);
            }
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
            // Nothing fits anywhere: then the roomiest disk is the best of the
            // bad options — the default root may be the smallest of them, and
            // the state machine still says what is missing.
            // On equal free space the default root wins: two roots on the
            // same disk would otherwise send the game to whichever was added
            // last.
            let roomiest = self
                .roots
                .iter()
                .filter_map(|r| free_for(&r.path).map(|free| (r, free)))
                .max_by_key(|(r, free)| (*free, r.is_default));
            log::warn!(
                "{game_id} needs {needed_bytes} bytes and no library root has room; using {}",
                roomiest
                    .map(|(r, free)| format!("{} ({free} bytes free)", r.path.display()))
                    .unwrap_or_else(|| "the default".into())
            );
            return roomiest.map(|(r, _)| r).or_else(|| self.default_root());
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

/// Logical bytes already reserved for a download; extracted data and sync
/// bookkeeping are not part of the archive reservation.
pub fn download_bytes(dir: &Path) -> u64 {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0
                || !e.file_type().is_dir()
                || !matches!(e.file_name().to_str(), Some("local" | ".sync"))
                    && !e.file_name().to_string_lossy().starts_with(".nll-")
        })
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .fold(0u64, |sum, m| sum.saturating_add(m.len()))
}

/// Move an unfinished download after the transport has released it. Cross-
/// volume copies are published only once complete; the original is retained
/// on any copy failure. Installed games and symlinks are never relocated.
pub fn relocate_download(source: &GamePaths, destination: &GamePaths) -> std::io::Result<()> {
    relocate_download_inner(source, destination, true)
}

fn relocate_download_inner(
    source: &GamePaths,
    destination: &GamePaths,
    rename_first: bool,
) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    if source.local_dir.exists() || source.receipt.exists() {
        return Err(Error::other("cannot relocate an installed game"));
    }
    for entry in walkdir::WalkDir::new(&source.share_dir) {
        if entry.map_err(Error::other)?.file_type().is_symlink() {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "download contains a symlink",
            ));
        }
    }
    if destination.share_dir.exists() {
        // Only an actually empty folder may be replaced.
        std::fs::remove_dir(&destination.share_dir)?;
    }
    let parent = destination
        .share_dir
        .parent()
        .ok_or_else(|| Error::other("missing root"))?;
    std::fs::create_dir_all(parent)?;
    if rename_first && std::fs::rename(&source.share_dir, &destination.share_dir).is_ok() {
        return Ok(());
    }
    let id = source
        .share_dir
        .file_name()
        .ok_or_else(|| Error::other("missing game id"))?
        .to_string_lossy();
    let staging = parent.join(format!(".nll-moving-{id}"));
    // create_dir, not create_dir_all: never reuse another attempt's data.
    std::fs::create_dir(&staging)?;
    let copied = (|| {
        for entry in walkdir::WalkDir::new(&source.share_dir).min_depth(1) {
            let entry = entry.map_err(Error::other)?;
            let target = staging.join(
                entry
                    .path()
                    .strip_prefix(&source.share_dir)
                    .map_err(Error::other)?,
            );
            if entry.file_type().is_symlink() {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "download contains a symlink",
                ));
            } else if entry.file_type().is_dir() {
                std::fs::create_dir(&target)?;
            } else if entry.file_type().is_file() {
                std::fs::copy(entry.path(), &target)?;
            } else {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "download contains a special file",
                ));
            }
        }
        Ok(())
    })();
    if let Err(e) = copied {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    let backup = source.share_dir.with_file_name(format!(".nll-moved-{id}"));
    if backup.exists() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(Error::new(
            ErrorKind::AlreadyExists,
            "previous relocation backup exists",
        ));
    }
    if let Err(e) = std::fs::rename(&source.share_dir, &backup) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&staging, &destination.share_dir) {
        let _ = std::fs::rename(&backup, &source.share_dir);
        return Err(e);
    }
    if let Err(e) = std::fs::remove_dir_all(&backup) {
        log::warn!(
            "download relocated; old copy remains at {}: {e}",
            backup.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn catalog_search_checks_secondary_roots_and_prefers_a_readable_default() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        let mut library = Library::default();
        library.add_root(LibraryRoot::new(&first));
        library.add_root(LibraryRoot::new(&second));
        assert!(library.load_catalog().is_none());
        let write_catalog = |root: &Path| {
            let db = root.join(crate::paths::CATALOG_RELATIVE);
            std::fs::create_dir_all(db.parent().unwrap()).unwrap();
            rusqlite::Connection::open(db)
                .unwrap()
                .execute_batch(include_str!("../tests/fixtures/game_db_fixture.sql"))
                .unwrap();
        };
        write_catalog(&second);
        assert_eq!(library.load_catalog().unwrap().0.path, second);
        // A partial database in the default root must not mask the second.
        std::fs::create_dir_all(first.join(crate::paths::CATALOG_RELATIVE).parent().unwrap())
            .unwrap();
        std::fs::write(first.join(crate::paths::CATALOG_RELATIVE), b"incomplete").unwrap();
        assert_eq!(library.load_catalog().unwrap().0.path, second);
        std::fs::remove_file(first.join(crate::paths::CATALOG_RELATIVE)).unwrap();
        write_catalog(&first);
        assert_eq!(library.load_catalog().unwrap().0.path, first);
        library.roots[0].is_default = false;
        library.roots[1].is_default = true;
        assert_eq!(library.load_catalog().unwrap().0.path, second);
    }

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
        std::fs::create_dir(small.join("quake3").join("local")).unwrap();
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

        // Nothing fits anywhere: the roomiest disk is the least bad place,
        // and the disk check says the rest.
        let chosen = lib.choose_root_with("unknown", u64::MAX, free).unwrap();
        assert_eq!(chosen.path, big);
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
        // A partial download can move when its current volume has no room.
        std::fs::write(small.join("bf4").join("bf4.eti.!sync"), b"x").unwrap();
        assert_eq!(lib.choose_root_with("bf4", 500, free).unwrap().path, big);
        assert!(lib.empty_leftovers("bf4", &big.join("bf4")).is_empty());
        // Already reserved archive space is subtracted before deciding.
        std::fs::write(small.join("bf4").join("bf4.eti.!sync"), [0; 490]).unwrap();
        assert_eq!(lib.choose_root_with("bf4", 500, free).unwrap().path, small);
        // A selected destination beats a locked empty old `.sync` folder.
        std::fs::remove_file(small.join("bf4").join("bf4.eti.!sync")).unwrap();
        std::fs::create_dir(big.join("bf4")).unwrap();
        std::fs::write(big.join("bf4").join(".nll-download"), []).unwrap();
        assert_eq!(lib.game_paths("bf4").unwrap().share_dir, big.join("bf4"));
    }

    #[test]
    fn relocation_preserves_partial_download_and_protects_installed_games() {
        let dir = tempfile::tempdir().unwrap();
        let source = GamePaths::new(&dir.path().join("small"), "g");
        let destination = GamePaths::new(&dir.path().join("big"), "g");
        std::fs::create_dir_all(source.share_dir.join(".sync")).unwrap();
        std::fs::write(source.share_dir.join("g.eti.!sync"), b"partial").unwrap();
        std::fs::write(source.share_dir.join(".sync/ID"), b"metadata").unwrap();
        relocate_download(&source, &destination).unwrap();
        assert!(!source.share_dir.exists());
        assert_eq!(
            std::fs::read(destination.share_dir.join("g.eti.!sync")).unwrap(),
            b"partial"
        );
        assert_eq!(
            std::fs::read(destination.share_dir.join(".sync/ID")).unwrap(),
            b"metadata"
        );
        std::fs::create_dir(&destination.local_dir).unwrap();
        assert!(relocate_download(&destination, &source).is_err());
        assert!(destination.local_dir.is_dir());
    }

    #[test]
    fn cross_volume_copy_keeps_metadata_and_refuses_to_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let source = GamePaths::new(&dir.path().join("a"), "g");
        let target = GamePaths::new(&dir.path().join("b"), "g");
        std::fs::create_dir_all(source.share_dir.join(".sync")).unwrap();
        std::fs::write(source.share_dir.join("g.eti.!sync"), b"partial").unwrap();
        std::fs::write(source.share_dir.join(".sync/ID"), b"metadata").unwrap();
        relocate_download_inner(&source, &target, false).unwrap();
        assert!(!source.share_dir.exists());
        assert_eq!(
            std::fs::read(target.share_dir.join("g.eti.!sync")).unwrap(),
            b"partial"
        );
        assert_eq!(
            std::fs::read(target.share_dir.join(".sync/ID")).unwrap(),
            b"metadata"
        );
        std::fs::create_dir_all(&source.share_dir).unwrap();
        std::fs::write(source.share_dir.join("keep.txt"), b"keep").unwrap();
        assert!(relocate_download_inner(&target, &source, false).is_err());
        assert_eq!(
            std::fs::read(source.share_dir.join("keep.txt")).unwrap(),
            b"keep"
        );
        assert!(target.share_dir.exists());
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
