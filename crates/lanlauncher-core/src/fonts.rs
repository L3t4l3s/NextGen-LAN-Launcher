//! Colour fonts the AppImage's web engine cannot draw.
//!
//! The AppImage carries WebKitGTK (and its Skia) from the build machine but
//! takes FreeType and the fonts from the machine it runs on. Fedora 44 ships
//! its colour emoji as `Noto-COLRv1.ttf`, and drawing one of its gradient
//! emoji (😀, the chat's emoji menu) trips an assertion in Skia's COLRv1 code
//! (`colrv1_configure_skpaint`): the web process dies and the window goes
//! blank until the launcher is restarted. Leaving the same machine's COLRv1
//! fonts out through fontconfig was checked to cure it there: the emoji
//! then come from the next colour font (Twemoji, COLRv0) and nothing dies.
//!
//! Only COLR version 1 is left out. Version 0 (flat layers) and bitmap colour
//! fonts (CBDT, sbix) do not reach that code.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The COLR table version of a font file, `None` when it has no COLR table
/// or is not a font. A collection counts with its highest version.
pub fn colr_version(path: &Path) -> Option<u16> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut tag = [0u8; 4];
    file.read_exact(&mut tag).ok()?;
    if &tag == b"ttcf" {
        let mut header = [0u8; 8];
        file.read_exact(&mut header).ok()?;
        let count = u32::from_be_bytes(header[4..8].try_into().ok()?).min(64);
        let mut offsets = Vec::new();
        for _ in 0..count {
            offsets.push(read_u32(&mut file)?);
        }
        return offsets
            .into_iter()
            .filter_map(|at| colr_in_face(&mut file, u64::from(at)))
            .max();
    }
    colr_in_face(&mut file, 0)
}

/// The COLR version of the face whose table directory starts at `at`.
fn colr_in_face<F: Read + Seek>(file: &mut F, at: u64) -> Option<u16> {
    file.seek(SeekFrom::Start(at + 4)).ok()?;
    let mut count = [0u8; 2];
    file.read_exact(&mut count).ok()?;
    let tables = usize::from(u16::from_be_bytes(count).min(512));
    file.seek(SeekFrom::Start(at + 12)).ok()?;
    // The whole table directory in one read.
    let mut directory = vec![0u8; tables * 16];
    file.read_exact(&mut directory).ok()?;
    for record in directory.as_chunks::<16>().0 {
        if &record[0..4] == b"COLR" {
            let offset = u32::from_be_bytes(record[8..12].try_into().ok()?);
            file.seek(SeekFrom::Start(u64::from(offset))).ok()?;
            let mut version = [0u8; 2];
            file.read_exact(&mut version).ok()?;
            return Some(u16::from_be_bytes(version));
        }
    }
    None
}

fn read_u32<F: Read>(file: &mut F) -> Option<u32> {
    let mut b = [0u8; 4];
    file.read_exact(&mut b).ok()?;
    Some(u32::from_be_bytes(b))
}

/// The colour font files the session's fontconfig knows, from `fc-list`
/// run against `config` (the session's own file, not one this launcher
/// wrote). Without it (or when it fails — a fontconfig too old for the
/// `color` property), every font in the usual folders.
pub fn colour_font_candidates(config: &Path) -> Vec<PathBuf> {
    // The machine's fc-list with the machine's libraries, not the image's.
    let mut fc_list = crate::launch::host_command("fc-list").into_std();
    let listed = fc_list
        .args([":color=true", "--format", "%{file}\\n"])
        .env("FONTCONFIG_FILE", config)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success());
    // An answer, even an empty one, is the answer: no colour fonts. Only a
    // missing fc-list or one that does not know `color` costs the walk.
    if let Some(listed) = listed {
        return String::from_utf8_lossy(&listed.stdout)
            .lines()
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .collect();
    }
    let mut dirs = vec![
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("/usr/local/share/fonts"),
    ];
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match std::env::var_os("XDG_DATA_HOME") {
        Some(data) => dirs.push(PathBuf::from(data).join("fonts")),
        None => dirs.extend(home.iter().map(|h| h.join(".local/share/fonts"))),
    }
    dirs.extend(home.iter().map(|h| h.join(".fonts")));
    dirs.iter()
        .flat_map(|d| walkdir::WalkDir::new(d).follow_links(true).into_iter())
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| {
            p.extension().is_some_and(|x| {
                let x = x.to_string_lossy().to_ascii_lowercase();
                matches!(x.as_str(), "ttf" | "otf" | "ttc" | "otc")
            })
        })
        .collect()
}

/// The configuration fontconfig reads when nothing overrides it:
/// `FONTCONFIG_FILE`, else `fonts.conf` in the first `FONTCONFIG_PATH`
/// folder that has one, else `/etc/fonts/fonts.conf` — and only if it exists.
/// A file this launcher would include without existing would leave its
/// window with no fonts at all.
pub fn session_config(file: Option<&str>, path: Option<&str>) -> Option<PathBuf> {
    let candidate = match (file, path) {
        (Some(f), _) if !f.is_empty() => Some(PathBuf::from(f)),
        (_, Some(p)) if !p.is_empty() => p
            .split(':')
            .map(|d| Path::new(d).join("fonts.conf"))
            .find(|f| f.is_file()),
        _ => Some(PathBuf::from("/etc/fonts/fonts.conf")),
    }?;
    (candidate.is_absolute() && candidate.is_file()).then_some(candidate)
}

/// Those of `files` that carry COLR version 1 or later.
///
/// Each file is read once, however many faces `fc-list` names in it. A path
/// that is not UTF-8 is left in: the fontconfig file cannot name it, and
/// reporting it as left out would be wrong.
pub fn colrv1_fonts(files: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = files.into_iter().filter(|f| f.to_str().is_some()).collect();
    files.sort();
    files.dedup();
    files.retain(|f| colr_version(f).is_some_and(|v| v >= 1));
    files
}

/// A fontconfig file that reads `base` (the configuration the session
/// would have used) and leaves `rejected` out.
pub fn rejecting_config(base: &Path, rejected: &[PathBuf]) -> String {
    let mut xml = String::from(
        "<?xml version=\"1.0\"?>\n\
         <!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n\
         <!-- Written by NextGen LAN Launcher for its own window: these colour\n     \
         fonts take its web engine down (COLR version 1). -->\n\
         <fontconfig>\n",
    );
    xml.push_str(&format!(
        "  <include ignore_missing=\"yes\">{}</include>\n  <selectfont>\n    <rejectfont>\n",
        escape(&base.to_string_lossy())
    ));
    // A pattern on the file name rather than a glob: fontconfig's globs
    // cannot escape `*` or `?`, a pattern compares the string as it is.
    for file in rejected {
        xml.push_str(&format!(
            "      <pattern><patelt name=\"file\"><string>{}</string></patelt></pattern>\n",
            escape(&file.to_string_lossy())
        ));
    }
    xml.push_str("    </rejectfont>\n  </selectfont>\n</fontconfig>\n");
    xml
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal font file: an sfnt header with one table, `table`, whose
    /// data starts with `version`.
    fn font(table: &[u8; 4], version: u16) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        f.extend_from_slice(&1u16.to_be_bytes());
        f.extend_from_slice(&[0; 6]);
        f.extend_from_slice(table);
        f.extend_from_slice(&[0; 4]);
        f.extend_from_slice(&28u32.to_be_bytes());
        f.extend_from_slice(&4u32.to_be_bytes());
        f.extend_from_slice(&version.to_be_bytes());
        f.extend_from_slice(&[0; 2]);
        f
    }

    #[test]
    fn only_colr_version_one_is_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let v1 = dir.path().join("Noto-COLRv1.ttf");
        let v0 = dir.path().join("Twemoji.ttf");
        let plain = dir.path().join("Plain.ttf");
        let junk = dir.path().join("broken.ttf");
        std::fs::write(&v1, font(b"COLR", 1)).unwrap();
        std::fs::write(&v0, font(b"COLR", 0)).unwrap();
        std::fs::write(&plain, font(b"glyf", 1)).unwrap();
        std::fs::write(&junk, b"no").unwrap();
        assert_eq!(colr_version(&v1), Some(1));
        assert_eq!(colr_version(&v0), Some(0));
        assert_eq!(colr_version(&plain), None);
        assert_eq!(colr_version(&junk), None);
        let found = colrv1_fonts([v0, plain, junk, v1.clone(), v1.clone()]);
        assert_eq!(found, vec![v1]);
    }

    #[test]
    fn a_collection_counts_with_any_of_its_faces() {
        let face = font(b"COLR", 1);
        let mut ttc = Vec::new();
        ttc.extend_from_slice(b"ttcf");
        ttc.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        ttc.extend_from_slice(&1u32.to_be_bytes());
        ttc.extend_from_slice(&16u32.to_be_bytes());
        // The table offset in the face is absolute: shift it past the header.
        let mut face = face;
        face[20..24].copy_from_slice(&(28u32 + 16).to_be_bytes());
        ttc.extend_from_slice(&face);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("emoji.ttc");
        std::fs::write(&path, ttc).unwrap();
        assert_eq!(colr_version(&path), Some(1));
    }

    /// `FONTCONFIG_PATH` is split on `:`, which a Windows path has itself;
    /// fontconfig and this check only matter on Linux.
    #[cfg(unix)]
    #[test]
    fn the_session_config_is_the_one_fontconfig_would_read_and_must_exist() {
        let dir = tempfile::tempdir().unwrap();
        let own = dir.path().join("own.conf");
        std::fs::write(&own, "<fontconfig/>").unwrap();
        let own_s = own.to_str().unwrap();
        assert_eq!(session_config(Some(own_s), None), Some(own.clone()));
        // A file that is not there: leave the environment alone.
        let gone = dir.path().join("gone.conf");
        assert_eq!(session_config(Some(gone.to_str().unwrap()), None), None);
        assert_eq!(session_config(Some("relative.conf"), None), None);
        // `FONTCONFIG_PATH`: the first folder that has a fonts.conf.
        let with = dir.path().join("with");
        std::fs::create_dir(&with).unwrap();
        std::fs::write(with.join("fonts.conf"), "<fontconfig/>").unwrap();
        let path = format!("{}:{}", dir.path().join("empty").display(), with.display());
        assert_eq!(
            session_config(None, Some(&path)),
            Some(with.join("fonts.conf"))
        );
        assert_eq!(session_config(None, Some("/nonexistent")), None);
    }

    #[test]
    fn the_config_keeps_the_sessions_and_names_each_file_exactly() {
        let xml = rejecting_config(
            Path::new("/etc/fonts/fonts.conf"),
            &[PathBuf::from("/usr/share/fonts/a&b/Emoji*.ttf")],
        );
        assert!(xml.contains("<include ignore_missing=\"yes\">/etc/fonts/fonts.conf</include>"));
        assert!(xml.contains(
            "<patelt name=\"file\"><string>/usr/share/fonts/a&amp;b/Emoji*.ttf</string></patelt>"
        ));
    }
}
