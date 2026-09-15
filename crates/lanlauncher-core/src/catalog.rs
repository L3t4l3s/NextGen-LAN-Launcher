//! Reader for the ETI `game.db` catalog and the `assets.eti` cover archive.
//!
//! The catalog is a plain SQLite file distributed through the `eti_launcher`
//! Resilio share. We open it read-only, validate every row defensively (the
//! file comes from the network) and never execute anything from it.
//!
//! Schema (verified against the catalog shipped in `sync_server.tar`):
//!
//! ```sql
//! CREATE TABLE games (game_id TEXT, db_id INTEGER PRIMARY KEY, game_title TEXT,
//!   game_key TEXT, game_release TEXT, game_publisher TEXT, game_size NUMERIC,
//!   game_readme_de TEXT, game_readme_en TEXT, game_readme_fr TEXT,
//!   game_maxplayers INTEGER, game_master_req INTEGER, genre_id INTEGER, game_version TEXT);
//! CREATE TABLE genre (genre_id INTEGER PRIMARY KEY, genre_de TEXT, genre_en TEXT, genre_fr TEXT);
//! CREATE TABLE discarded (del_id INTEGER PRIMARY KEY, game_id TEXT, game_key INTEGER);
//! CREATE TABLE tools (tool_id TEXT, db_id INTEGER PRIMARY KEY, tool_name TEXT, tool_key TEXT,
//!   tool_maintainer TEXT, tool_size TEXT, tool_readme_de TEXT, tool_readme_en TEXT,
//!   tool_readme_fr TEXT, tool_disabled INTEGER);
//! ```

use crate::error::{Error, Result};
use regex::Regex;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Resilio read-only secret: `B` + 32 Base32 characters.
pub static READ_ONLY_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^B[A-Z2-7]{32}$").expect("valid regex"));
/// Game id used as folder name. Lowercase, no path separators.
pub static GAME_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9_-]{0,63}$").expect("valid regex"));

pub const MAX_ROWS: usize = 5_000;
pub const MAX_CATALOG_BYTES: u64 = 50 * 1024 * 1024;

/// A Resilio read-only key. Never printed in `Debug` output or serialised by
/// accident: it grants download access to the share.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ShareKey(String);

impl ShareKey {
    pub fn parse(value: &str) -> Option<Self> {
        let v = value.trim();
        READ_ONLY_KEY_RE.is_match(v).then(|| Self(v.to_string()))
    }

    /// The secret itself, for handing to the transport.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

/// Read-only key of the ETI catalog share (`eti_launcher`), built into the
/// launcher like the original ETI client does. Source: the public
/// `sync_server.tar` from eti-lan.xyz (`/root/eti-config.conf`, variable
/// `eti_call`, used by `/etc/init.d/eti` to add `/lan/eti_launcher`). The key
/// is identical for every ETI client and distributed openly; a LAN with its
/// own catalog overrides it via `settings.catalogKey`.
pub const BUILTIN_CATALOG_KEY: &str = "BICDWADB4KCVNR6FCAGYTHEKZBYVUGTZX";

/// Effective catalog key: a valid settings override wins, otherwise the
/// built-in key. `None` when neither parses. An invalid override is logged and
/// ignored so a hand-edited settings file cannot silently disable the catalog.
pub fn catalog_share_key(override_key: Option<&str>) -> Option<ShareKey> {
    if let Some(k) = override_key.map(str::trim).filter(|k| !k.is_empty()) {
        match ShareKey::parse(k) {
            Some(key) => return Some(key),
            None => log::warn!("ignoring invalid catalogKey override in settings"),
        }
    }
    ShareKey::parse(BUILTIN_CATALOG_KEY)
}

impl std::fmt::Debug for ShareKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ShareKey(B…{})", &self.0[self.0.len() - 4..])
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Genre {
    pub id: i64,
    pub de: String,
    pub en: String,
    pub fr: String,
}

impl Genre {
    pub fn name(&self, lang: &str) -> &str {
        match lang {
            "de" => &self.de,
            "fr" if !self.fr.is_empty() => &self.fr,
            _ => &self.en,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub order: i64,
    pub title: String,
    pub key: ShareKey,
    /// Package revision (`YYYYMMDD`), *not* the game's own version.
    pub revision: String,
    /// Approximate download size in bytes (catalog stores GB as decimal).
    pub size_bytes: u64,
    pub release_year: Option<String>,
    pub publisher: Option<String>,
    /// Free text like `16`, `2+`, `2 (+ 6 CPU)`.
    pub max_players: Option<String>,
    /// Needs a dedicated/master server on the LAN.
    pub needs_master_server: bool,
    pub genre_id: Option<i64>,
    pub readme: BTreeMap<String, String>,
}

impl Game {
    pub fn genre<'a>(&self, catalog: &'a Catalog) -> Option<&'a Genre> {
        self.genre_id.and_then(|id| catalog.genres.get(&id))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    pub id: String,
    pub order: i64,
    pub name: String,
    pub key: Option<ShareKey>,
    pub maintainer: Option<String>,
    pub size: Option<String>,
    pub disabled: bool,
    pub readme: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub games: Vec<Game>,
    pub genres: BTreeMap<i64, Genre>,
    pub tools: Vec<Tool>,
    /// Game ids listed in the `discarded` table; shares that should be removed.
    pub discarded: Vec<String>,
    /// Rows that failed validation; reported to the user as a warning.
    pub skipped_rows: Vec<String>,
}

impl Catalog {
    pub fn game(&self, id: &str) -> Option<&Game> {
        self.games.iter().find(|g| g.id == id)
    }

    /// Open and read a catalog file. The file is opened read-only and copied
    /// into memory so the sync engine can replace it at any time.
    pub fn load(path: &Path) -> Result<Self> {
        let meta = std::fs::metadata(path).map_err(|e| Error::io(path, e))?;
        if meta.len() > MAX_CATALOG_BYTES {
            return Err(Error::Catalog(format!(
                "catalog is {} bytes, limit is {}",
                meta.len(),
                MAX_CATALOG_BYTES
            )));
        }
        let mut header = [0u8; 16];
        std::fs::File::open(path)
            .and_then(|mut f| f.read_exact(&mut header))
            .map_err(|e| Error::io(path, e))?;
        if &header != b"SQLite format 3\0" {
            return Err(Error::Catalog("file is not a SQLite database".into()));
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")?;
        let check: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(Error::Catalog(format!("quick_check failed: {check}")));
        }
        Self::from_connection(&conn)
    }

    pub fn from_connection(conn: &Connection) -> Result<Self> {
        let games_is_table: Option<String> = conn
            .query_row(
                "SELECT type FROM sqlite_master WHERE name='games'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if games_is_table.as_deref() != Some("table") {
            return Err(Error::Catalog("table `games` is missing".into()));
        }

        let columns = table_columns(conn, "games")?;
        let mut catalog = Catalog::default();

        if table_columns(conn, "genre").is_ok() {
            let mut stmt = conn
                .prepare("SELECT genre_id, genre_de, genre_en, genre_fr FROM genre LIMIT 1000")?;
            let rows = stmt.query_map([], |r| {
                Ok(Genre {
                    id: r.get(0)?,
                    de: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    en: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    fr: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                })
            })?;
            for g in rows.flatten() {
                catalog.genres.insert(g.id, g);
            }
        }

        let col = |name: &str| -> String {
            if columns.iter().any(|c| c == name) {
                name.to_string()
            } else {
                format!("NULL AS {name}")
            }
        };
        let sql = format!(
            "SELECT game_id, db_id, game_title, game_key, game_version, game_size, {}, {}, {}, {}, {}, {}, {}, {} FROM games ORDER BY db_id LIMIT {}",
            col("game_release"),
            col("game_publisher"),
            col("game_maxplayers"),
            col("game_master_req"),
            col("genre_id"),
            col("game_readme_de"),
            col("game_readme_en"),
            col("game_readme_fr"),
            MAX_ROWS + 1
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        let mut seen = std::collections::HashSet::new();
        while let Some(row) = rows.next()? {
            if catalog.games.len() >= MAX_ROWS {
                return Err(Error::Catalog(format!("more than {MAX_ROWS} games")));
            }
            let raw_id: Option<String> = row.get(0)?;
            let raw_id = raw_id.unwrap_or_default();
            match parse_game(row, &raw_id) {
                Ok(game) => {
                    if !seen.insert(game.id.clone()) {
                        return Err(Error::Catalog(format!("duplicate game id `{}`", game.id)));
                    }
                    catalog.games.push(game);
                }
                Err(reason) => catalog.skipped_rows.push(format!("{raw_id}: {reason}")),
            }
        }

        if table_columns(conn, "tools").is_ok() {
            let mut stmt = conn.prepare(
                "SELECT tool_id, db_id, tool_name, tool_key, tool_maintainer, tool_size, tool_disabled, tool_readme_de, tool_readme_en, tool_readme_fr FROM tools ORDER BY db_id LIMIT 1000",
            )?;
            let rows = stmt.query_map([], |r| {
                let mut readme = BTreeMap::new();
                for (i, lang) in [(7, "de"), (8, "en"), (9, "fr")] {
                    if let Some(t) = r.get::<_, Option<String>>(i)? {
                        if !t.trim().is_empty() {
                            readme.insert(lang.to_string(), t);
                        }
                    }
                }
                Ok(Tool {
                    id: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    order: r.get::<_, Option<i64>>(1)?.unwrap_or_default(),
                    name: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    key: r
                        .get::<_, Option<String>>(3)?
                        .and_then(|k| ShareKey::parse(&k)),
                    maintainer: r.get(4)?,
                    size: r
                        .get::<_, Option<rusqlite::types::Value>>(5)?
                        .map(value_to_string),
                    disabled: r.get::<_, Option<i64>>(6)?.unwrap_or(0) != 0,
                    readme,
                })
            })?;
            for t in rows.flatten() {
                if GAME_ID_RE.is_match(&t.id) {
                    catalog.tools.push(t);
                }
            }
        }

        if table_columns(conn, "discarded").is_ok() {
            let mut stmt =
                conn.prepare("SELECT game_id FROM discarded ORDER BY del_id LIMIT 1000")?;
            let rows = stmt.query_map([], |r| r.get::<_, Option<String>>(0))?;
            for id in rows.flatten().flatten() {
                if GAME_ID_RE.is_match(&id) {
                    catalog.discarded.push(id);
                }
            }
        }

        Ok(catalog)
    }
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .flatten()
        .collect();
    if cols.is_empty() {
        return Err(Error::Catalog(format!("table `{table}` is missing")));
    }
    Ok(cols)
}

fn value_to_string(v: rusqlite::types::Value) -> String {
    use rusqlite::types::Value::*;
    match v {
        Null => String::new(),
        Integer(i) => i.to_string(),
        Real(f) => f.to_string(),
        Text(t) => t,
        Blob(_) => String::new(),
    }
}

fn parse_game(row: &rusqlite::Row<'_>, raw_id: &str) -> std::result::Result<Game, String> {
    let get_text = |i: usize| -> std::result::Result<Option<String>, String> {
        row.get::<_, Option<rusqlite::types::Value>>(i)
            .map(|v| v.map(value_to_string).filter(|s| !s.trim().is_empty()))
            .map_err(|e| e.to_string())
    };
    let id = raw_id.trim().to_string();
    if !GAME_ID_RE.is_match(&id) {
        return Err("invalid game_id".into());
    }
    if id == crate::paths::LAUNCHER_SHARE_ID {
        return Err("reserved id".into());
    }
    let order: i64 = row
        .get::<_, Option<i64>>(1)
        .map_err(|e| e.to_string())?
        .unwrap_or(0);
    let title = get_text(2)?.ok_or("missing title")?;
    if title.chars().count() > 300 || title.chars().any(char::is_control) {
        return Err("invalid title".into());
    }
    let key = get_text(3)?
        .and_then(|k| ShareKey::parse(&k))
        .ok_or("missing or non read-only key")?;
    let revision = get_text(4)?.ok_or("missing game_version")?;
    if revision.len() > 64 || revision.chars().any(char::is_control) {
        return Err("invalid game_version".into());
    }
    let size_gb: f64 = match row
        .get::<_, Option<rusqlite::types::Value>>(5)
        .map_err(|e| e.to_string())?
    {
        Some(rusqlite::types::Value::Integer(i)) => i as f64,
        Some(rusqlite::types::Value::Real(f)) => f,
        Some(rusqlite::types::Value::Text(t)) => t.trim().replace(',', ".").parse().unwrap_or(0.0),
        _ => 0.0,
    };
    if !(0.0..100_000.0).contains(&size_gb) {
        return Err("invalid game_size".into());
    }
    let mut readme = BTreeMap::new();
    for (i, lang) in [(11, "de"), (12, "en"), (13, "fr")] {
        if let Some(t) = get_text(i)? {
            readme.insert(lang.to_string(), t);
        }
    }
    Ok(Game {
        id,
        order,
        title,
        key,
        revision,
        size_bytes: (size_gb * 1_000_000_000.0).round() as u64,
        release_year: get_text(6)?,
        publisher: get_text(7)?,
        max_players: get_text(8)?,
        needs_master_server: row
            .get::<_, Option<i64>>(9)
            .map_err(|e| e.to_string())?
            .unwrap_or(0)
            != 0,
        genre_id: row.get::<_, Option<i64>>(10).map_err(|e| e.to_string())?,
        readme,
    })
}

/// A cover image inside `assets.eti`: where it goes and whether it sits
/// directly in `assets/` (the official layout, which wins over screenshots
/// or duplicates in other folders).
#[derive(Debug, Clone, PartialEq, Eq)]
struct CoverMember {
    target: PathBuf,
    direct: bool,
}

/// Archive member → cover, or `None` for anything that is not a cover image.
/// Accepts `assets/<id>.jpg`, a flat `<id>.png`, nested folders, `./`
/// prefixes and Windows separators; the id is lowercased so it matches the
/// catalog's lowercase game ids.
fn cover_member(dest_dir: &Path, member: &str) -> Option<CoverMember> {
    static MEMBER_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^(?:\./)?(.*?)([a-z0-9][a-z0-9_-]{0,63})\.(jpe?g|png)$")
            .expect("valid regex")
    });
    let normalised = member.replace('\\', "/");
    let caps = MEMBER_RE.captures(&normalised)?;
    let folder = &caps[1];
    if !folder.is_empty() && !folder.ends_with('/') {
        return None;
    }
    Some(CoverMember {
        target: dest_dir.join(format!(
            "{}.{}",
            caps[2].to_ascii_lowercase(),
            caps[3].to_ascii_lowercase()
        )),
        direct: folder.eq_ignore_ascii_case("assets/") || folder.is_empty(),
    })
}

/// Covers larger than this are skipped (the real ones are a few hundred KB).
const MAX_COVER_BYTES: u64 = 20 * 1024 * 1024;

/// Bookkeeping for one extraction run: members directly under `assets/`
/// always win, anything nested only fills gaps.
#[derive(Default)]
struct CoverRun {
    written: usize,
    /// Targets written in this run by a member directly under `assets/`.
    direct_targets: std::collections::HashSet<PathBuf>,
    /// Every target written in this run (direct or nested).
    targets: std::collections::HashSet<PathBuf>,
    /// First member names, for the log when nothing matched.
    seen: Vec<String>,
}

impl CoverRun {
    fn note(&mut self, member: &str) {
        if self.seen.len() < 5 {
            self.seen.push(member.to_string());
        }
    }
    /// Decide whether this member should be written now. A direct member
    /// always wins; a nested one only fills a gap this run has not written
    /// yet. Files from earlier runs are replaced, so an updated `assets.eti`
    /// takes effect and a switched catalog does not inherit stale covers.
    fn accept(&mut self, m: &CoverMember) -> bool {
        if m.direct {
            self.direct_targets.insert(m.target.clone());
            true
        } else {
            !self.direct_targets.contains(&m.target) && !self.targets.contains(&m.target)
        }
    }
    fn wrote(&mut self, m: &CoverMember) {
        self.written += 1;
        self.targets.insert(m.target.clone());
    }
}

/// Covers are written to `<target>.part` first and renamed on success, so a
/// member that fails half-way never destroys the cover of an earlier run.
fn part_path(target: &Path) -> PathBuf {
    let mut os = target.as_os_str().to_owned();
    os.push(".part");
    PathBuf::from(os)
}

/// Move a completely written `.part` file onto its target.
fn commit_part(part: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::rename(part, target).inspect_err(|_| {
        let _ = std::fs::remove_file(part);
    })
}

/// Extract cover images from `assets.eti` into `dest_dir/<game_id>.<ext>`.
/// ETI ships covers as `assets/<game_id>.jpg|png` inside an archive that is
/// RAR like every other `.eti` (a plain or gzip-compressed tar is accepted as
/// well; the format is sniffed from the first bytes). Returns the number of
/// covers written; members that are no cover image are ignored, a member
/// that cannot be written is logged and skipped.
pub fn extract_covers(assets: &Path, dest_dir: &Path) -> Result<usize> {
    std::fs::create_dir_all(dest_dir).map_err(|e| Error::io(dest_dir, e))?;
    let mut file = std::fs::File::open(assets).map_err(|e| Error::io(assets, e))?;
    let mut magic = Vec::with_capacity(8);
    std::io::Read::read_to_end(&mut std::io::Read::take(&mut file, 8), &mut magic)
        .map_err(|e| Error::io(assets, e))?;
    let mut run = CoverRun::default();
    if magic.starts_with(b"Rar!\x1a\x07") {
        extract_covers_rar(assets, dest_dir, &mut run)?;
    } else {
        std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(0))
            .map_err(|e| Error::io(assets, e))?;
        let reader: Box<dyn std::io::Read> = if magic.starts_with(&[0x1f, 0x8b]) {
            Box::new(flate2::read::GzDecoder::new(file))
        } else {
            Box::new(file)
        };
        extract_covers_tar(reader, dest_dir, &mut run)?;
    }
    if run.written == 0 {
        log::warn!(
            "covers: no cover images in {} (first members: {})",
            assets.display(),
            run.seen.join(", ")
        );
    }
    Ok(run.written)
}

fn extract_covers_tar(
    reader: Box<dyn std::io::Read>,
    dest_dir: &Path,
    run: &mut CoverRun,
) -> Result<()> {
    let mut archive = tar::Archive::new(reader);
    for entry in archive
        .entries()
        .map_err(|e| Error::Archive(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| Error::Archive(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| Error::Archive(e.to_string()))?
            .to_string_lossy()
            .to_string();
        run.note(&path);
        let Some(m) = cover_member(dest_dir, &path) else {
            continue;
        };
        if entry.size() > MAX_COVER_BYTES || !run.accept(&m) {
            continue;
        }
        let part = part_path(&m.target);
        let write = std::fs::File::create(&part)
            .and_then(|mut out| std::io::copy(&mut entry, &mut out))
            .and_then(|_| commit_part(&part, &m.target));
        match write {
            Ok(()) => run.wrote(&m),
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                log::warn!("covers: cannot write {}: {e}", m.target.display());
            }
        }
    }
    Ok(())
}

fn extract_covers_rar(assets: &Path, dest_dir: &Path, run: &mut CoverRun) -> Result<()> {
    use crate::extract::map_err;
    // unrar consumes the archive handle when a member fails to extract. To
    // keep the covers after a bad member (CRC error, reserved Windows name),
    // the archive is reopened and the members up to the failed one skipped.
    let mut handled = 0usize;
    loop {
        let mut open = unrar::Archive::new(assets)
            .open_for_processing()
            .map_err(map_err)?;
        let mut index = 0usize;
        loop {
            let Some(header) = open.read_header().map_err(map_err)? else {
                return Ok(());
            };
            index += 1;
            if index <= handled {
                open = header.skip().map_err(map_err)?;
                continue;
            }
            let name = header.entry().filename.to_string_lossy().to_string();
            run.note(&name);
            let member = if header.entry().is_directory()
                || header.entry().unpacked_size > MAX_COVER_BYTES
            {
                None
            } else {
                cover_member(dest_dir, &name).filter(|m| run.accept(m))
            };
            open = match &member {
                Some(m) => {
                    let part = part_path(&m.target);
                    match header.extract_to(&part) {
                        Ok(next) => {
                            match commit_part(&part, &m.target) {
                                Ok(()) => run.wrote(m),
                                Err(e) => {
                                    log::warn!("covers: cannot write {}: {e}", m.target.display())
                                }
                            }
                            next
                        }
                        Err(e) => {
                            log::warn!("covers: cannot extract {name}: {e}");
                            let _ = std::fs::remove_file(&part);
                            handled = index;
                            break;
                        }
                    }
                }
                None => header.skip().map_err(map_err)?,
            };
        }
    }
}

/// Locate a preview video shipped in the launcher share
/// (`eti_launcher/video/<id>.mp4`, as mirrored in the eti-lan/LAN-Launcher repo).
pub fn find_video(library_root: &Path, game_id: &str) -> Option<std::path::PathBuf> {
    for dir in video_dirs(library_root) {
        for ext in ["mp4", "webm"] {
            let p = dir.join(format!("{game_id}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Folders inside a library root that may hold preview videos.
pub fn video_dirs(library_root: &Path) -> Vec<std::path::PathBuf> {
    let base = library_root.join(crate::paths::LAUNCHER_SHARE_ID);
    ["video", "videos", "update/video"]
        .iter()
        .map(|d| base.join(d))
        .collect()
}

/// Locate a cached cover for a game id.
pub fn find_cover(covers_dir: &Path, game_id: &str) -> Option<std::path::PathBuf> {
    ["jpg", "png", "jpeg"]
        .iter()
        .map(|ext| covers_dir.join(format!("{game_id}.{ext}")))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../tests/fixtures/game_db_fixture.sql"))
            .unwrap();
        conn
    }

    #[test]
    fn reads_games_genres_tools_and_discarded() {
        let cat = Catalog::from_connection(&fixture_conn()).unwrap();
        assert_eq!(cat.games.len(), 4, "skipped: {:?}", cat.skipped_rows);
        let q3 = cat.game("quake3").unwrap();
        assert_eq!(q3.title, "Quake 3 Arena");
        assert_eq!(q3.revision, "20160922");
        assert_eq!(q3.size_bytes, 910_000_000);
        assert_eq!(q3.max_players.as_deref(), Some("16"));
        assert_eq!(q3.genre(&cat).unwrap().en, "First-person Shooter");
        assert_eq!(q3.genre(&cat).unwrap().name("de"), "Ego-Shooter");
        assert!(q3.readme.contains_key("de"));
        let bf = cat.game("bfbc2").unwrap();
        assert!(bf.needs_master_server);
        assert_eq!(bf.size_bytes, 16_000_000_000);
        assert_eq!(
            cat.game("weird").unwrap().max_players.as_deref(),
            Some("2 (+ 6 CPU)")
        );
        assert_eq!(cat.tools.len(), 2);
        assert!(cat.tools[1].disabled);
        assert_eq!(cat.discarded, vec!["sc2".to_string()]);
    }

    #[test]
    fn rejects_write_keys_and_bad_ids() {
        let cat = Catalog::from_connection(&fixture_conn()).unwrap();
        assert_eq!(cat.skipped_rows.len(), 2);
        assert!(cat
            .skipped_rows
            .iter()
            .any(|r| r.contains("non read-only key")));
        assert!(cat
            .skipped_rows
            .iter()
            .any(|r| r.contains("invalid game_id")));
    }

    #[test]
    fn catalog_share_key_prefers_valid_override() {
        let valid = "BABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        assert_eq!(
            catalog_share_key(Some(valid)).map(|k| k.expose().to_string()),
            Some(valid.to_string())
        );
        assert_eq!(
            catalog_share_key(Some(&format!("  {valid}  "))).map(|k| k.expose().to_string()),
            Some(valid.to_string())
        );
        let builtin = ShareKey::parse(BUILTIN_CATALOG_KEY);
        assert!(
            builtin.is_some(),
            "built-in catalog key must be a valid read-only key"
        );
        assert_eq!(catalog_share_key(None), builtin);
        assert_eq!(catalog_share_key(Some("   ")), builtin);
        assert_eq!(catalog_share_key(Some("garbage")), builtin);
    }

    #[test]
    fn share_key_redacts_debug() {
        let k = ShareKey::parse("BIS3FAIKG3OSTTK5GIZQXQPF5TALHQBMR").unwrap();
        assert_eq!(format!("{k:?}"), "ShareKey(B…QBMR)");
        assert!(ShareKey::parse("AIS3FAIKG3OSTTK5GIZQXQPF5TALHQBMR").is_none());
        assert!(ShareKey::parse("short").is_none());
    }

    #[test]
    fn load_rejects_non_sqlite_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("game.db");
        std::fs::write(&p, b"not a database at all, definitely").unwrap();
        assert!(matches!(Catalog::load(&p), Err(Error::Catalog(_))));
    }

    #[test]
    fn load_reads_file_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("game.db");
        let conn = Connection::open(&p).unwrap();
        conn.execute_batch(include_str!("../tests/fixtures/game_db_fixture.sql"))
            .unwrap();
        drop(conn);
        let cat = Catalog::load(&p).unwrap();
        assert_eq!(cat.games.len(), 4);
    }

    #[test]
    fn extracts_covers_from_tar() {
        let dir = tempfile::tempdir().unwrap();
        let tar_path = dir.path().join("assets.eti");
        {
            let f = std::fs::File::create(&tar_path).unwrap();
            let mut b = tar::Builder::new(f);
            let data = b"\xFF\xD8\xFF fake jpeg";
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, "assets/quake3.jpg", &data[..])
                .unwrap();
            let mut h2 = tar::Header::new_gnu();
            h2.set_size(3);
            h2.set_mode(0o644);
            h2.set_cksum();
            // A member outside assets/ still lands flat in the cover dir.
            b.append_data(&mut h2, "other/evil.jpg", &b"abc"[..])
                .unwrap();
            b.finish().unwrap();
        }
        let out = dir.path().join("covers");
        // Stale covers from an earlier run are replaced, direct or nested.
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("quake3.jpg"), "old direct").unwrap();
        std::fs::write(out.join("evil.jpg"), "old nested").unwrap();
        assert_eq!(extract_covers(&tar_path, &out).unwrap(), 2);
        assert!(find_cover(&out, "quake3").is_some());
        assert!(out.join("evil.jpg").is_file(), "kept inside the cover dir");
        assert_eq!(std::fs::read(out.join("evil.jpg")).unwrap(), b"abc");
        assert!(std::fs::read(out.join("quake3.jpg"))
            .unwrap()
            .starts_with(b"\xFF\xD8"));
        assert!(!dir.path().join("other").exists());
        assert!(
            std::fs::read_dir(&out).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".part")),
            "no .part files left behind"
        );

        // The same archive gzip-compressed yields the same covers.
        let gz_path = dir.path().join("assets.gz.eti");
        {
            let mut enc = flate2::write::GzEncoder::new(
                std::fs::File::create(&gz_path).unwrap(),
                flate2::Compression::fast(),
            );
            std::io::copy(&mut std::fs::File::open(&tar_path).unwrap(), &mut enc).unwrap();
            enc.finish().unwrap();
        }
        let out2 = dir.path().join("covers2");
        assert_eq!(extract_covers(&gz_path, &out2).unwrap(), 2);
        assert!(find_cover(&out2, "quake3").is_some());

        // Not an archive at all → error, not silent success.
        let bogus = dir.path().join("bogus.eti");
        std::fs::write(&bogus, "definitely not an archive of any kind").unwrap();
        assert!(extract_covers(&bogus, &out2).is_err());
        // A RAR signature without content is tolerated by unrar: no covers, no crash.
        let bogus_rar = dir.path().join("bogus_rar.eti");
        std::fs::write(&bogus_rar, "Rar!\x1a\x07\x01\x00 truncated").unwrap();
        assert!(matches!(extract_covers(&bogus_rar, &out2), Ok(0) | Err(_)));
    }

    #[test]
    fn extracts_covers_from_rar_like_eti_ships_them() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("covers");
        let rar = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/assets_covers.rar");
        // assets/quake3.jpg, assets/nested/amongus.png, assets/readme.txt
        assert_eq!(extract_covers(&rar, &out).unwrap(), 2);
        assert!(find_cover(&out, "quake3").is_some());
        assert!(find_cover(&out, "amongus").is_some());
        assert!(!out.join("readme.txt").exists());
    }

    #[test]
    fn cover_member_accepts_eti_layouts_only() {
        let d = Path::new("/c");
        let m = |s: &str| cover_member(d, s);
        let quake = m("assets/quake3.jpg").unwrap();
        assert_eq!(quake.target, PathBuf::from("/c/quake3.jpg"));
        assert!(quake.direct);
        assert_eq!(
            m("./assets/Quake3.JPG").unwrap().target,
            PathBuf::from("/c/quake3.jpg")
        );
        let nested = m("assets\\sub\\cod4.png").unwrap();
        assert_eq!(nested.target, PathBuf::from("/c/cod4.png"));
        assert!(!nested.direct);
        assert!(m("wc3.jpeg").unwrap().direct);
        assert_eq!(m("assets/readme.txt"), None);
        assert_eq!(m("assets/.hidden.jpg"), None);
        assert_eq!(m("assets/"), None);
        assert_eq!(m("assets/bad name.jpg"), None);
    }
}
