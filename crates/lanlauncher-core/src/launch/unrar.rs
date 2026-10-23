//! macOS/Linux: the old ETI launcher's `unrar.exe`, as the launcher itself.
//!
//! Setup scripts unpack extra archives with the copy that came with ETI's
//! launcher (`"%programfiles%\eti\lan launcher\unrar.exe" x -o -y
//! "coh_maps_8p.eti" %cohpath%`: Company of Heroes' maps and its player
//! profile into `Documents\My Games`). No prefix has that program, and
//! SteamOS has no unrar of its own. `setup_script::filter` turns those calls
//! into `"%NLL_UNRAR%" …`; Wine starts the host script named by its `Z:`
//! path and waits for it, as with `launch::fnr`, and the script starts the
//! launcher's own binary with [`FLAG`], which unpacks with the UnRAR library
//! it carries anyway and exits before anything else of the launcher starts.
//!
//! What it understands is what ETI's scripts use: `x`, the switches `-o`,
//! `-o+` and `-y` (overwrite, ask nothing — what it does anyway) and a few
//! that change nothing here, the archive and a destination folder.
//! Anything else (`e`, `-o-`, a password, an exclusion) ends with unrar's
//! code 7 instead of doing something the script did not ask for.

use std::path::{Path, PathBuf};

/// The variable the rewritten calls name (`"%NLL_UNRAR%" x …`): the helper
/// script's `Z:` path, set by the launcher on every script it runs.
pub const VAR: &str = "NLL_UNRAR";

/// The argument that makes the launcher's binary unpack and exit.
pub const FLAG: &str = "--nll-unrar";

/// The helper Wine starts: the launcher's binary, without the libraries
/// Proton put in front (`LD_LIBRARY_PATH`), which are no business of it.
fn shell(launcher: &Path) -> String {
    let quoted = launcher.to_string_lossy().replace('\'', r"'\''");
    format!(
        "#!/bin/sh\n\
         # Written by NextGen LAN Launcher: unrar.exe of ETI's scripts for Wine.\n\
         exec env -u LD_LIBRARY_PATH -u LD_PRELOAD '{quoted}' {FLAG} \"$@\"\n"
    )
}

/// Writes `nll-unrar.sh` into `dir` (only when its text changed: a running
/// script may be reading it) and returns its path. `launcher` is the binary
/// to call back — under an AppImage the image itself (`$APPIMAGE`), not the
/// program inside its mount, which is gone once the launcher ends.
pub fn install(dir: &Path, launcher: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let script = dir.join("nll-unrar.sh");
    super::setup_script::write_if_changed(&script, shell(launcher).as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(script)
}

/// What one call asks for.
#[derive(Debug, PartialEq, Eq)]
struct Call {
    archive: String,
    dest: String,
}

/// Switches that ask for what this does anyway: overwrite (`-o`, `-o+`),
/// answer yes (`-y`), say less (`-inul`, `-id…`), no comments (`-c-`), run
/// in the background (`-ibck`).
fn harmless(switch: &str) -> bool {
    let lower = switch.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "-o" | "-o+" | "-y" | "-inul" | "-c-" | "-ibck"
    ) || lower.starts_with("-id")
}

/// unrar's command line as ETI's scripts write it. A destination that cmd
/// gave as `"C:\…\Company of Heroes\"` reaches a host program with its
/// closing quote inside (`\"` is a literal quote to Windows' rules), so
/// quotes around either path are dropped.
fn parse(args: &[String]) -> Result<Call, String> {
    let mut words = args.iter().map(|a| a.trim_matches('"').to_string());
    let command = words.next().ok_or("no command")?.to_ascii_lowercase();
    if command != "x" {
        return Err(format!("command {command:?} is not supported"));
    }
    let mut paths = Vec::new();
    for word in words.filter(|w| !w.is_empty()) {
        if word.starts_with('-') {
            if !harmless(&word) {
                return Err(format!("switch {word:?} is not supported"));
            }
        } else {
            paths.push(word);
        }
    }
    let mut paths = paths.into_iter();
    let archive = paths.next().ok_or("no archive")?;
    let dest = paths.next().unwrap_or_else(|| ".".to_string());
    if let Some(more) = paths.next() {
        return Err(format!("file lists are not supported ({more:?})"));
    }
    Ok(Call { archive, dest })
}

/// A path as cmd handed it over, on the host: `Z:\home\x` is `/home/x`,
/// another drive is the prefix's (`dosdevices`), a relative one stays
/// relative to the folder Wine started the helper in. `None` for a drive
/// path when no prefix is known: it must not land at the host's root.
fn host_path(path: &str, prefix: Option<&Path>) -> Option<PathBuf> {
    let bytes = path.as_bytes();
    if bytes.len() < 2 || bytes[1] != b':' || !bytes[0].is_ascii_alphabetic() {
        return Some(PathBuf::from(path.replace('\\', "/")));
    }
    let letter = bytes[0].to_ascii_lowercase();
    let rest = path[2..].replace('\\', "/");
    if letter == b'z' {
        return Some(PathBuf::from(if rest.is_empty() {
            "/".into()
        } else {
            rest
        }));
    }
    let drive = prefix?
        .join("dosdevices")
        .join(format!("{}:", letter as char));
    Some(as_wine_finds_it(&drive, &rest))
}

/// `rest` below `root` the way Wine finds it, regardless of case: each part
/// that exists in another spelling (`Users`, `my games`) takes that one, so
/// nothing lands in a second folder beside the one the game reads.
fn as_wine_finds_it(root: &Path, rest: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for part in rest.split('/').filter(|p| !p.is_empty()) {
        let found = std::fs::read_dir(&path).ok().and_then(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name())
                .find(|name| name.to_string_lossy().eq_ignore_ascii_case(part))
        });
        match found {
            Some(name) => path.push(name),
            None => path.push(part),
        }
    }
    path
}

/// The prefix the calling Wine runs in, as Wine and Proton name it.
fn prefix_from_env() -> Option<PathBuf> {
    std::env::var_os("WINEPREFIX")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("STEAM_COMPAT_DATA_PATH").map(|p| PathBuf::from(p).join("pfx"))
        })
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".wine")))
}

/// Unpacks as asked and returns unrar's exit code: 0 done, 6 the archive
/// cannot be opened, 7 a command it does not know, 2 anything else.
pub fn run(args: &[String]) -> i32 {
    run_in(args, prefix_from_env().as_deref())
}

fn run_in(args: &[String], prefix: Option<&Path>) -> i32 {
    let call = match parse(args) {
        Ok(call) => call,
        Err(e) => {
            eprintln!("unrar: {e}");
            return 7;
        }
    };
    let (Some(archive), Some(dest)) = (
        host_path(&call.archive, prefix),
        host_path(&call.dest, prefix),
    ) else {
        eprintln!("unrar: no Wine prefix known for a drive path");
        return 2;
    };
    if !archive.is_file() {
        eprintln!("unrar: cannot open {}", archive.display());
        return 6;
    }
    match crate::extract::extract(&archive, &dest, None, None) {
        Ok(files) => {
            println!(
                "unrar: {files} files from {} into {}",
                archive.display(),
                dest.display()
            );
            0
        }
        Err(e) => {
            eprintln!("unrar: {e}");
            2
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn words(line: &[&str]) -> Vec<String> {
        line.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn the_calls_etis_scripts_make_are_understood() {
        // Company of Heroes: the destination ends in `\"`, which reaches a
        // host program with the quote inside.
        let call = parse(&words(&[
            "x",
            "-o",
            "-y",
            "coh_maps_8p.eti",
            r#"C:\users\steamuser\Documents\My Games\Company of Heroes""#,
        ]))
        .unwrap();
        assert_eq!(
            call,
            Call {
                archive: "coh_maps_8p.eti".into(),
                dest: r"C:\users\steamuser\Documents\My Games\Company of Heroes".into(),
            }
        );
        let call = parse(&words(&["X", "-o+", "-inul", "a.eti"])).unwrap();
        assert_eq!(call.dest, ".");
        // What it would not do as asked: refused, not done otherwise.
        assert!(parse(&words(&["e", "a.eti"])).is_err());
        assert!(parse(&words(&["x", "-o-", "a.eti"])).is_err());
        assert!(parse(&words(&["x", "-psecret", "a.eti"])).is_err());
        assert!(parse(&words(&["x", "a.eti", "dest", "file.txt"])).is_err());
    }

    #[test]
    fn windows_paths_land_where_wine_has_them() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path();
        assert_eq!(
            host_path(r"Z:\home\kevin\LAN\coh\local", Some(prefix)),
            Some(PathBuf::from("/home/kevin/LAN/coh/local"))
        );
        assert_eq!(
            host_path(r"maps\a.eti", Some(prefix)),
            Some(PathBuf::from("maps/a.eti"))
        );
        // Another spelling of a folder that is there is that folder.
        let docs = prefix.join("dosdevices/c:/users/steamuser/Documents");
        std::fs::create_dir_all(&docs).unwrap();
        assert_eq!(
            host_path(r"C:\Users\steamuser\documents\My Games", Some(prefix)),
            Some(docs.join("My Games"))
        );
        // No prefix: a drive path has nowhere to go.
        assert_eq!(host_path(r"C:\users\x", None), None);
        assert_eq!(run_in(&words(&["x", r"C:\a.eti"]), None), 2);
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn unpacks_into_a_drive_of_the_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("pfx");
        let drive = tmp.path().join("drive_c");
        std::fs::create_dir_all(prefix.join("dosdevices")).unwrap();
        std::fs::create_dir_all(&drive).unwrap();
        std::os::unix::fs::symlink(&drive, prefix.join("dosdevices/c:")).unwrap();
        let archive = fixture("sample_game.rar");
        let args = words(&[
            "x",
            "-o",
            "-y",
            &format!("Z:{}", archive.display()).replace('/', "\\"),
            r#"C:\users\steamuser\Documents\My Games\Demo""#,
        ]);
        assert_eq!(run_in(&args, Some(&prefix)), 0);
        let target = drive.join("users/steamuser/Documents/My Games/Demo");
        assert!(std::fs::read_dir(&target).unwrap().next().is_some());
        // Again over what is there: overwritten, not refused.
        assert_eq!(run_in(&args, Some(&prefix)), 0);
        // A missing archive is unrar's open error, a switch it would not
        // honour its usage error.
        let elsewhere = tmp.path().join("elsewhere");
        let args = words(&["x", "missing.eti", elsewhere.to_str().unwrap()]);
        assert_eq!(run_in(&args, Some(&prefix)), 6);
        let args = words(&[
            "x",
            "-o-",
            archive.to_str().unwrap(),
            elsewhere.to_str().unwrap(),
        ]);
        assert_eq!(run_in(&args, Some(&prefix)), 7);
        assert!(!elsewhere.exists());
    }

    #[test]
    fn the_helper_calls_the_launcher_back() {
        let tmp = tempfile::tempdir().unwrap();
        let script = install(tmp.path(), Path::new("/opt/it's here/launcher")).unwrap();
        let text = std::fs::read_to_string(&script).unwrap();
        assert!(text.contains(r"'/opt/it'\''s here/launcher' --nll-unrar"));
        assert!(text.contains("-u LD_LIBRARY_PATH"));
    }
}
