/// Where Steam keeps Proton, and how to find it without being told.
///
/// Proton is not on `PATH` and has no fixed location: it lives inside a Steam
/// library, and a Steam Deck has at least two of those (the internal drive and
/// the SD card, the latter somewhere under `/run/media`). Until now the
/// launcher only used a path from the settings, so a Deck with Proton
/// installed still reported "no Wine, CrossOver or Proton found" — the search
/// simply did not exist.
///
/// This module is the search, and it is pure: every entry point takes the home
/// directory and reads the filesystem, so a whole Steam installation can be
/// built in a temporary folder and asserted against.
use std::path::{Path, PathBuf};

/// A Proton installation, with the Steam it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtonInstall {
    /// The `proton` script that is run.
    pub proton: PathBuf,
    /// The Steam root it was found under — Proton needs it as
    /// `STEAM_COMPAT_CLIENT_INSTALL_PATH`, and hard-coding `~/.steam/steam`
    /// is wrong for a Flatpak Steam or a second installation.
    pub steam_root: PathBuf,
    /// What to show the user, e.g. `Proton 9.0 (Beta)` or `GE-Proton9-20`.
    pub label: String,
}

/// Steam installations to look at, in the order they are preferred.
///
/// `~/.steam/steam` and `~/.steam/root` are symlinks the client maintains;
/// they usually point at one of the others, and the duplicates are removed
/// by `canonicalize` in [`protons_in`].
pub fn steam_roots(home: &Path) -> Vec<PathBuf> {
    [
        ".steam/steam",
        ".steam/root",
        ".local/share/Steam",
        // Debian and Ubuntu package it here.
        ".steam/debian-installation",
        // Flatpak Steam, which is how a lot of distributions ship it.
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
    ]
    .iter()
    .map(|r| home.join(r))
    .filter(|p| p.is_dir())
    .collect()
}

/// Every Steam library belonging to a root: its own `steamapps` plus each
/// path in `libraryfolders.vdf`. The second part is what finds Proton on a
/// Steam Deck's SD card.
pub fn steam_libraries(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    let vdf = root.join("steamapps/libraryfolders.vdf");
    if let Ok(text) = std::fs::read_to_string(&vdf) {
        out.extend(library_paths_in_vdf(&text));
    }
    out.retain(|p| p.join("steamapps/common").is_dir());
    // A real `libraryfolders.vdf` lists the main library as well, and the
    // root may be reached through a symlink, so the same directory arrives
    // under two spellings. Comparing the resolved form keeps it once.
    let mut seen = Vec::new();
    out.retain(|p| {
        let real = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
        let fresh = !seen.contains(&real);
        seen.push(real);
        fresh
    });
    out
}

/// The `"path"` values of a `libraryfolders.vdf`.
///
/// Hand-parsed on purpose: the file is a handful of quoted pairs, a VDF crate
/// would be a dependency for one key, and the format has changed shape twice
/// (a flat map of index → path, then a block per library) while the `"path"`
/// line stayed the same.
pub fn library_paths_in_vdf(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('"').filter(|s| !s.trim().is_empty());
            let key = parts.next()?;
            let value = parts.next()?.trim();
            if value.is_empty() {
                return None;
            }
            // Today's shape is a block per library with a `"path"` key. The
            // older one was a flat map of index to path — `"1" "/mnt/games"`
            // — and a Steam that was never migrated still has it.
            let is_path_key = key == "path";
            let is_index_key = !key.is_empty() && key.chars().all(|c| c.is_ascii_digit());
            (is_path_key || is_index_key).then(|| PathBuf::from(value))
        })
        .collect()
}

/// How a Proton is ranked against the others. Lower sorts first.
///
/// Numbered Steam releases come first, newest first, then Valve's
/// `Experimental`/`Hotfix`, and finally tools discovered in
/// `compatibilitytools.d`. Merely installing a compatibility tool for Steam,
/// Lutris or another launcher is not a decision to use it for every LAN game;
/// an explicit path in the launcher settings remains the way to make that
/// decision.
fn rank(label: &str, from_compat_tools: bool) -> (u8, std::cmp::Reverse<Vec<u32>>) {
    let tier = if from_compat_tools {
        2
    } else if label.contains("Experimental") || label.contains("Hotfix") {
        1
    } else {
        0
    };
    (tier, std::cmp::Reverse(version_of(label)))
}

/// The numbers in a name, so `Proton 10.0` sorts above `Proton 9.0`.
fn version_of(label: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let mut digits = String::new();
    for c in label.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else if !digits.is_empty() {
            out.push(digits.parse().unwrap_or(0));
            digits.clear();
        }
    }
    if !digits.is_empty() {
        out.push(digits.parse().unwrap_or(0));
    }
    out
}

/// Every Proton under this home directory, best first.
pub fn find_protons(home: &Path) -> Vec<ProtonInstall> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for root in steam_roots(home) {
        // The symlinked roots resolve onto the real one; without this a
        // machine reports the same Proton three times.
        let real = std::fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
        if seen.contains(&real) {
            continue;
        }
        seen.push(real);
        out.extend(protons_in(&root));
    }
    out.sort_by(|a, b| {
        rank(
            &a.label,
            a.proton.to_string_lossy().contains("compatibilitytools.d"),
        )
        .cmp(&rank(
            &b.label,
            b.proton.to_string_lossy().contains("compatibilitytools.d"),
        ))
    });
    // Two spellings of one path are one Proton; `dedup_by` alone would only
    // catch them where they happen to land next to each other.
    let mut kept = Vec::new();
    let mut seen_proton = Vec::new();
    for install in out {
        let real =
            std::fs::canonicalize(&install.proton).unwrap_or_else(|_| install.proton.clone());
        if seen_proton.contains(&real) {
            continue;
        }
        seen_proton.push(real);
        kept.push(install);
    }
    kept
}

/// Every Proton belonging to one Steam root.
fn protons_in(root: &Path) -> Vec<ProtonInstall> {
    let mut out = Vec::new();
    // Tools the user dropped in themselves.
    if let Ok(entries) = std::fs::read_dir(root.join("compatibilitytools.d")) {
        for entry in entries.flatten() {
            let proton = entry.path().join("proton");
            if proton.is_file() {
                out.push(ProtonInstall {
                    proton,
                    steam_root: root.to_path_buf(),
                    label: entry.file_name().to_string_lossy().to_string(),
                });
            }
        }
    }
    // Proton as a Steam download, in every library this root knows.
    for library in steam_libraries(root) {
        let Ok(entries) = std::fs::read_dir(library.join("steamapps/common")) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("Proton") {
                continue;
            }
            let proton = entry.path().join("proton");
            if proton.is_file() {
                out.push(ProtonInstall {
                    proton,
                    steam_root: root.to_path_buf(),
                    label: name,
                });
            }
        }
    }
    out
}

/// Every place a Proton was looked for, so a failed search can be explained
/// in the log rather than guessed at — the same courtesy
/// `resilio::locate_binary_detailed` does for the sync engine.
pub fn probed_paths(home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in steam_roots(home) {
        out.push(root.join("compatibilitytools.d"));
        for library in steam_libraries(&root) {
            out.push(library.join("steamapps/common"));
        }
    }
    if out.is_empty() {
        // Worth saying which homes were considered when none of them exist.
        out.extend(
            [
                ".steam/steam",
                ".local/share/Steam",
                ".var/app/com.valvesoftware.Steam/.local/share/Steam",
            ]
            .iter()
            .map(|r| home.join(r)),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, "#!/bin/sh\n").expect("write");
    }

    /// A Steam Deck as it really looks: Steam in `~/.local/share/Steam`, one
    /// Proton on the internal drive and one on the SD card, which is a second
    /// library listed in `libraryfolders.vdf`. The `~/.steam/steam` symlink
    /// belongs to the one test that is about it — `symlink` does not exist on
    /// Windows, and this module is compiled there too.
    fn steam_deck(home: &Path, card: &Path) {
        let steam = home.join(".local/share/Steam");
        touch(&steam.join("steamapps/common/Proton 9.0 (Beta)/proton"));
        touch(&card.join("steamapps/common/Proton 8.0/proton"));
        std::fs::write(
            steam.join("steamapps/libraryfolders.vdf"),
            format!(
                r#"
"libraryfolders"
{{
	"0"
	{{
		"path"		"{}"
		"label"		""
	}}
	"1"
	{{
		"path"		"{}"
		"label"		"SD-Karte"
	}}
}}
"#,
                steam.display(),
                card.display()
            ),
        )
        .expect("vdf");
    }

    #[test]
    fn a_proton_on_the_sd_card_is_found_too() {
        let home = tempfile::tempdir().expect("home");
        let card = tempfile::tempdir().expect("card");
        steam_deck(home.path(), card.path());

        let found = find_protons(home.path());
        let labels: Vec<_> = found.iter().map(|p| p.label.as_str()).collect();
        assert!(
            labels.contains(&"Proton 9.0 (Beta)"),
            "internal drive missing: {labels:?}"
        );
        assert!(
            labels.contains(&"Proton 8.0"),
            "the SD card is a library of its own and has to be searched: {labels:?}"
        );
        // Newest first among the numbered releases.
        assert_eq!(labels.first(), Some(&"Proton 9.0 (Beta)"));
    }

    #[test]
    fn the_steam_root_travels_with_the_find() {
        let home = tempfile::tempdir().expect("home");
        let card = tempfile::tempdir().expect("card");
        steam_deck(home.path(), card.path());
        // Even the Proton that sits on the card belongs to the Steam that
        // listed it; that is what STEAM_COMPAT_CLIENT_INSTALL_PATH wants.
        for install in find_protons(home.path()) {
            assert!(
                install.steam_root.join("steamapps").is_dir(),
                "{:?} is not a Steam root",
                install.steam_root
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_root_does_not_report_the_same_proton_twice() {
        let home = tempfile::tempdir().expect("home");
        let card = tempfile::tempdir().expect("card");
        steam_deck(home.path(), card.path());
        // `~/.steam/steam` is a symlink onto `~/.local/share/Steam`, so both
        // roots are the same directory and the first draft counted the Proton
        // under each spelling of the path.
        std::fs::create_dir_all(home.path().join(".steam")).expect("dirs");
        std::os::unix::fs::symlink(
            home.path().join(".local/share/Steam"),
            home.path().join(".steam/steam"),
        )
        .expect("symlink");
        let nine = find_protons(home.path())
            .iter()
            .filter(|p| p.label == "Proton 9.0 (Beta)")
            .count();
        assert_eq!(nine, 1, "the symlinked root was counted again");
    }

    #[test]
    fn a_discovered_custom_tool_does_not_override_official_proton() {
        let home = tempfile::tempdir().expect("home");
        let card = tempfile::tempdir().expect("card");
        steam_deck(home.path(), card.path());
        touch(
            &home
                .path()
                .join(".local/share/Steam/compatibilitytools.d/GE-Proton9-20/proton"),
        );
        let found = find_protons(home.path());
        assert_eq!(
            found.first().map(|p| p.label.as_str()),
            Some("Proton 9.0 (Beta)"),
            "a tool installed for another launcher is not a global preference"
        );
    }

    #[test]
    fn experimental_beats_a_legacy_custom_tool_when_no_release_is_installed() {
        let home = tempfile::tempdir().expect("home");
        let steam = home.path().join(".local/share/Steam");
        touch(&steam.join("steamapps/common/Proton - Experimental/proton"));
        touch(&steam.join("compatibilitytools.d/ULWGL-Proton-8.0-5-3/proton"));

        let found = find_protons(home.path());
        assert_eq!(
            found.first().map(|p| p.label.as_str()),
            Some("Proton - Experimental"),
            "a current Valve build should beat an unrelated legacy compatibility tool"
        );
    }

    #[test]
    fn experimental_is_the_fallback_not_the_default() {
        let home = tempfile::tempdir().expect("home");
        let card = tempfile::tempdir().expect("card");
        steam_deck(home.path(), card.path());
        touch(
            &home
                .path()
                .join(".local/share/Steam/steamapps/common/Proton - Experimental/proton"),
        );
        let labels: Vec<_> = find_protons(home.path())
            .iter()
            .map(|p| p.label.clone())
            .collect();
        let experimental = labels.iter().position(|l| l.contains("Experimental"));
        let numbered = labels.iter().position(|l| l == "Proton 9.0 (Beta)");
        assert!(
            numbered < experimental,
            "a numbered release should be preferred: {labels:?}"
        );
    }

    #[test]
    fn a_flatpak_steam_is_searched_as_well() {
        let home = tempfile::tempdir().expect("home");
        touch(
            &home.path().join(
                ".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/common/Proton 9.0/proton",
            ),
        );
        let found = find_protons(home.path());
        assert_eq!(
            found.len(),
            1,
            "Flatpak Steam is how many distributions ship it"
        );
        assert!(found[0].proton.is_file());
    }

    #[test]
    fn nothing_installed_finds_nothing_and_says_where_it_looked() {
        let home = tempfile::tempdir().expect("home");
        assert!(find_protons(home.path()).is_empty());
        assert!(
            !probed_paths(home.path()).is_empty(),
            "a failed search must still be able to name the places it tried"
        );
    }

    #[test]
    fn both_shapes_of_libraryfolders_vdf_are_read() {
        // The modern shape: a block per library.
        let modern = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/deck/.local/share/Steam"
		"contentid"		"123"
	}
	"1"
	{
		"path"		"/run/media/deck/SN128"
	}
}
"#;
        assert_eq!(
            library_paths_in_vdf(modern),
            vec![
                PathBuf::from("/home/deck/.local/share/Steam"),
                PathBuf::from("/run/media/deck/SN128")
            ]
        );
        // The older shape: a flat map of index to path.
        let flat = r#"
"LibraryFolders"
{
	"TimeNextStatsReport"		"1234"
	"ContentStatsID"		"5678"
	"1"		"/mnt/games"
	"2"		"/run/media/deck/SN128"
}
"#;
        assert_eq!(
            library_paths_in_vdf(flat),
            vec![
                PathBuf::from("/mnt/games"),
                PathBuf::from("/run/media/deck/SN128")
            ],
            "a Steam that was never migrated still writes this"
        );
        // Nothing to find is not an error.
        assert!(library_paths_in_vdf("").is_empty());
        assert!(library_paths_in_vdf("\"libraryfolders\"\n{\n}\n").is_empty());
    }

    #[test]
    fn versions_sort_by_number_and_not_by_text() {
        // Plain string order would put "Proton 9.0" above "Proton 10.0".
        assert!(version_of("Proton 10.0") > version_of("Proton 9.0"));
        assert!(version_of("GE-Proton9-20") > version_of("GE-Proton9-2"));
        assert_eq!(version_of("Proton - Experimental"), Vec::<u32>::new());
    }
}
