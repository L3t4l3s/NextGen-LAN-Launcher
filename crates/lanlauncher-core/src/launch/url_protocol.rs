//! Links a game registers for itself, opened on macOS/Linux in its prefix.
//!
//! Battlefield 3's `game_setup.cmd` registers `bf3lan://` under
//! `HKLM\SOFTWARE\Classes` for `local\BFUlauncher.exe`, and the master
//! server's Battlelog page joins a game through such a link. Under Wine that
//! registration lands in the game's prefix, where the host's browser never
//! looks. The launcher reads it back from the prefix's `system.reg`
//! ([`handlers_of`]), offers itself to the desktop as the handler of those
//! schemes ([`register`], `x-scheme-handler/<scheme>` in `mimeapps.list`),
//! and when a link arrives (`--open-url`) runs the registered program in the
//! game's prefix with the link in place of `%1`.
//!
//! Only programs of the extracted game (`local/`) count: the prefix belongs to
//! the game, but a registration naming anything else (a browser, `cmd.exe`)
//! is not something a link from a web page should start.

use std::path::{Path, PathBuf};

/// The desktop entry that offers the launcher for the games' schemes.
pub const DESKTOP_FILE: &str = "xyz.nextgen-lan.launcher.url-handler.desktop";

/// What a game registered for one scheme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handler {
    /// Lower case.
    pub scheme: String,
    /// The program, on the host, inside the game's folder.
    pub program: PathBuf,
    /// Its arguments as registered, `%1` standing for the link.
    pub args: Vec<String>,
}

impl Handler {
    /// The arguments with the link in place of `%1` (and `%L`, which
    /// Windows fills alike). A registration without either still gets the
    /// link: one that names only the program expects it all the same.
    pub fn args_for(&self, url: &str) -> Vec<String> {
        let mut used = false;
        // One pass: a `%L` inside the link is the link's, not a placeholder.
        let mut args: Vec<String> = self
            .args
            .iter()
            .map(|a| {
                let mut out = String::with_capacity(a.len() + url.len());
                let mut rest = a.as_str();
                while let Some(at) = rest.find('%') {
                    out.push_str(&rest[..at]);
                    let after = &rest[at + 1..];
                    match after.chars().next() {
                        Some('1' | 'l' | 'L') => {
                            out.push_str(url);
                            used = true;
                            rest = &after[1..];
                        }
                        _ => {
                            out.push('%');
                            rest = after;
                        }
                    }
                }
                out.push_str(rest);
                out
            })
            .collect();
        if !used {
            args.push(url.to_string());
        }
        args
    }
}

/// The scheme of `url`, lower case, when `url` is one link and nothing else:
/// it travels as one line to the running launcher and becomes one argument
/// of a Windows program.
pub fn scheme(url: &str) -> Option<String> {
    if url.len() > 4096 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let (scheme, rest) = url.split_once(':')?;
    let mut chars = scheme.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        && scheme.len() <= 64;
    // Never one the desktop has its own handler for.
    let scheme = scheme.to_ascii_lowercase();
    let reserved = [
        "http",
        "https",
        "file",
        "ftp",
        "mailto",
        "steam",
        "javascript",
        "data",
    ];
    (valid && !rest.is_empty() && !reserved.contains(&scheme.as_str())).then_some(scheme)
}

/// The URL protocols a Wine registry file registers under
/// `Software\Classes`: `(scheme, command)` for each key that carries a
/// `"URL Protocol"` value and a `shell\open\command`.
pub fn in_registry(text: &str) -> Vec<(String, String)> {
    const CLASSES: &str = "software\\classes\\";
    let mut protocols = Vec::new();
    let mut commands = std::collections::BTreeMap::new();
    let mut section: Option<String> = None;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(head) = line.strip_prefix('[') {
            // `[Software\\Classes\\bf3lan\\shell\\open\\command] 1792077914`
            section = head
                .rsplit_once(']')
                .map(|(key, _)| unescape(key).to_ascii_lowercase())
                .and_then(|key| key.strip_prefix(CLASSES).map(str::to_string));
            continue;
        }
        let Some(key) = section.as_deref() else {
            continue;
        };
        if let Some(scheme) = key.strip_suffix("\\shell\\open\\command") {
            if let Some(value) = line.strip_prefix("@=") {
                if let Some(value) = string_value(value) {
                    commands.insert(scheme.to_string(), value);
                }
            }
        } else if !key.contains('\\') && line.to_ascii_lowercase().starts_with("\"url protocol\"=")
        {
            protocols.push(key.to_string());
        }
    }
    protocols
        .into_iter()
        .filter(|s| scheme(&format!("{s}:x")).as_deref() == Some(s.as_str()))
        .filter_map(|s| commands.remove(&s).map(|c| (s, c)))
        .collect()
}

/// A `"…"` value of a registry file, unescaped; `None` for any other type.
fn string_value(value: &str) -> Option<String> {
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    Some(unescape(inner))
}

/// A string as a Wine registry file writes it: `\\` and `\"`, `\n` and
/// its kind, and everything beyond ASCII as `\x` with up to four hex digits
/// of UTF-16 (a library under `/home/jürgen` reads `j\x00fcrgen`).
fn unescape(text: &str) -> String {
    let mut units: Vec<u16> = Vec::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let push = |units: &mut Vec<u16>, c: char| {
        let mut buf = [0u16; 2];
        units.extend_from_slice(c.encode_utf16(&mut buf));
    };
    while let Some(c) = chars.next() {
        if c != '\\' {
            push(&mut units, c);
            continue;
        }
        match chars.next() {
            Some('x') => {
                let mut value = 0u32;
                let mut digits = 0;
                while digits < 4 {
                    match chars.peek().and_then(|d| d.to_digit(16)) {
                        Some(d) => {
                            value = value * 16 + d;
                            digits += 1;
                            chars.next();
                        }
                        None => break,
                    }
                }
                if digits == 0 {
                    push(&mut units, 'x');
                } else {
                    units.push(value as u16);
                }
            }
            Some('n') => push(&mut units, '\n'),
            Some('r') => push(&mut units, '\r'),
            Some('t') => push(&mut units, '\t'),
            Some('0') => units.push(0),
            Some(next) => push(&mut units, next),
            None => push(&mut units, '\\'),
        }
    }
    String::from_utf16_lossy(&units)
}

/// A Windows command line split as programs read theirs: words apart at
/// spaces, a quoted part kept together without its quotes.
pub fn split_command(command: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in command.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    words.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            c => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        words.push(word);
    }
    words
}

/// Where a Windows path of the prefix lies on the host: `Z:` is the root,
/// `C:` the prefix's `drive_c`. Doubled separators count once (ETI's script
/// writes `bf3\\local\\BFUlauncher.exe`), and the case of the names does not
/// matter, as on Windows.
pub fn host_path(windows: &str, drive_c: &Path) -> Option<PathBuf> {
    let (drive, rest) = windows.split_once(':')?;
    let root = match drive.to_ascii_lowercase().as_str() {
        "z" => PathBuf::from("/"),
        "c" => drive_c.to_path_buf(),
        _ => return None,
    };
    let parts: Vec<&str> = rest
        .split(['\\', '/'])
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    if parts.contains(&"..") {
        return None;
    }
    if parts.is_empty() {
        return Some(root);
    }
    crate::paths::find_ignoring_case(&root, &parts.join("/"))
}

/// The registry files of a game's prefixes: its own (`.nll-prefix`) and
/// those of runners pinned for it (`.nll-prefix-<key>`), Wine's layout and
/// Proton's (`pfx/`).
fn registries(share_dir: &Path) -> Vec<(PathBuf, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(share_dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(".nll-prefix")
        {
            continue;
        }
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        for prefix in [dir.join("pfx"), dir] {
            let reg = prefix.join("system.reg");
            if reg.is_file() {
                found.push((reg, prefix.join("drive_c")));
                break;
            }
        }
    }
    found.sort();
    found
}

/// The newest change to any of the game's registry files, to tell when
/// [`handlers_of`] has to read them again.
pub fn registries_changed(share_dir: &Path) -> Option<std::time::SystemTime> {
    registries(share_dir)
        .iter()
        .filter_map(|(reg, _)| std::fs::metadata(reg).and_then(|m| m.modified()).ok())
        .max()
}

/// What the game's prefixes register for links: one handler per scheme,
/// only for programs of the extracted game (`local/`).
pub fn handlers_of(share_dir: &Path) -> Vec<Handler> {
    // The extracted game: the prefix lives in the same folder, and its
    // `drive_c` is not the game's.
    let Ok(game_dir) = share_dir.join("local").canonicalize() else {
        return Vec::new();
    };
    let mut handlers: Vec<Handler> = Vec::new();
    for (reg, drive_c) in registries(share_dir) {
        // A registry file of a few megabytes; only the protocols matter.
        let Ok(bytes) = std::fs::read(&reg) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        if !text.contains("\"URL Protocol\"") {
            continue;
        }
        for (scheme, command) in in_registry(&text) {
            if handlers.iter().any(|h| h.scheme == scheme) {
                continue;
            }
            let mut words = split_command(&command).into_iter();
            let Some(program) = words.next() else {
                continue;
            };
            let inside = host_path(&program, &drive_c)
                .and_then(|p| p.canonicalize().ok())
                .filter(|p| p.is_file() && p.starts_with(&game_dir));
            match inside {
                Some(program) => handlers.push(Handler {
                    scheme,
                    program,
                    args: words.collect(),
                }),
                None => log::info!(
                    "{}: {scheme}:// names {program}, which is not a program in the game's folder; not offered",
                    share_dir.display()
                ),
            }
        }
    }
    handlers
}

/// Offer `exec --open-url %u` to the desktop for `schemes`
/// (`$XDG_DATA_HOME/applications/<DESKTOP_FILE>`, defaults in
/// `$XDG_CONFIG_HOME/mimeapps.list`); with none, take the offer back.
/// A scheme another program is the default for keeps it. True when a file
/// changed.
pub fn register(
    schemes: &[String],
    exec: &Path,
    data_home: &Path,
    config_home: &Path,
) -> std::io::Result<bool> {
    let desktop = data_home.join("applications").join(DESKTOP_FILE);
    let mut changed = false;
    if schemes.is_empty() {
        if desktop.exists() {
            std::fs::remove_file(&desktop)?;
            changed = true;
        }
    } else {
        let mime: String = schemes
            .iter()
            .map(|s| format!("x-scheme-handler/{s};"))
            .collect();
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=NextGen LAN Launcher\nComment=Opens game links (Battlelog) in the game's prefix\nExec={} --open-url %u\nTerminal=false\nNoDisplay=true\nMimeType={mime}\n",
            desktop_quote(&exec.to_string_lossy())
        );
        if std::fs::read_to_string(&desktop).ok().as_deref() != Some(entry.as_str()) {
            std::fs::create_dir_all(desktop.parent().unwrap_or(data_home))?;
            replace_file(&desktop, entry.as_bytes())?;
            changed = true;
        }
    }
    let list = config_home.join("mimeapps.list");
    let text = std::fs::read_to_string(&list).unwrap_or_default();
    let updated = with_defaults(&text, schemes);
    if updated != text {
        std::fs::create_dir_all(config_home)?;
        replace_file(&list, updated.as_bytes())?;
        changed = true;
    }
    Ok(changed)
}

/// Write `bytes` to `path` through a file beside it, so a desktop reading
/// the list meanwhile never sees half of it; a symlink (a dotfiles
/// repository) is written through instead of replaced.
fn replace_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if path.is_symlink() {
        return std::fs::write(path, bytes);
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".nll-tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// `mimeapps.list` with this launcher as the default for `schemes` where
/// nobody else is, and without its entries for any other scheme. Everything
/// else stays as it was.
fn with_defaults(text: &str, schemes: &[String]) -> String {
    const SECTION: &str = "[Default Applications]";
    // The scheme of an entry that names this launcher, and the entry
    // without it (`None` when nothing else is left).
    let ours = |line: &str| -> Option<(String, Option<String>)> {
        let (key, value) = line.split_once('=')?;
        let scheme = key.trim().strip_prefix("x-scheme-handler/")?;
        let names: Vec<&str> = value
            .split(';')
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .collect();
        if !names.contains(&DESKTOP_FILE) {
            return None;
        }
        let others: Vec<&str> = names.into_iter().filter(|n| *n != DESKTOP_FILE).collect();
        let rest = (!others.is_empty()).then(|| format!("{}={};", key.trim(), others.join(";")));
        Some((scheme.to_ascii_lowercase(), rest))
    };
    let taken = |line: &str| -> Option<String> {
        let (key, _) = line.split_once('=')?;
        key.trim()
            .strip_prefix("x-scheme-handler/")
            .map(str::to_ascii_lowercase)
    };
    let mut out: Vec<String> = Vec::new();
    let mut in_defaults = false;
    let mut seen_defaults = false;
    let mut present: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_defaults {
                add_missing(&mut out, schemes, &present);
            }
            in_defaults = trimmed == SECTION;
            seen_defaults |= in_defaults;
        } else if in_defaults {
            if let Some((scheme, rest)) = ours(trimmed) {
                if !schemes.contains(&scheme) {
                    if let Some(rest) = rest {
                        out.push(rest);
                    }
                    continue;
                }
            }
            if let Some(scheme) = taken(trimmed) {
                present.push(scheme);
            }
        }
        out.push(line.to_string());
    }
    if in_defaults {
        add_missing(&mut out, schemes, &present);
    } else if !seen_defaults && !schemes.is_empty() {
        if out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push(SECTION.to_string());
        add_missing(&mut out, schemes, &present);
    }
    if out.is_empty() {
        return text.to_string();
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    if joined.trim_end_matches('\n') == text.trim_end_matches('\n') {
        return text.to_string();
    }
    joined
}

/// Default entries for the schemes nobody has one for, placed at the end
/// of the section (before its blank lines).
fn add_missing(out: &mut Vec<String>, schemes: &[String], present: &[String]) {
    let blank = out.iter().rev().take_while(|l| l.trim().is_empty()).count();
    let at = out.len() - blank;
    let lines: Vec<String> = schemes
        .iter()
        .filter(|s| !present.contains(s))
        .map(|s| format!("x-scheme-handler/{s}={DESKTOP_FILE}"))
        .collect();
    out.splice(at..at, lines);
}

/// A program path for `Exec=`: quoted, with what the desktop entry
/// specification wants escaped inside quotes, and `%` doubled.
fn desktop_quote(path: &str) -> String {
    let mut out = String::from("\"");
    for c in path.chars() {
        match c {
            // The value is unescaped once as a string, then as a quoted
            // word: a backslash takes four, the others a doubled one.
            '\\' => out.push_str("\\\\\\\\"),
            '"' | '`' | '$' => {
                out.push_str("\\\\");
                out.push(c);
            }
            '%' => out.push_str("%%"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const REG: &str = r#"WINE REGISTRY Version 2
;; All keys relative to \\Machine

[Software\\Classes\\bf3lan] 1792077914
#time=1dd5cb9607796e2
@="URL:bf3lan Protocol"
"URL Protocol"=""

[Software\\Classes\\bf3lan\\shell\\open\\command] 1792077914
#time=1dd5cb960779b74
@="Z:\\home\\kevin\\LAN\\bf3\\\\local\\\\BFUlauncher.exe %1"

[Software\\Classes\\htmlfile\\shell\\open\\command] 1792077914
@="\"C:\\Program Files\\Internet Explorer\\iexplore.exe\" %1"

[Software\\Classes\\other\\shell\\open\\command] 1792077914
@="C:\\x.exe %1"
"#;

    #[test]
    fn the_registry_names_the_protocols_and_their_programs() {
        // What Battlefield 3's game_setup.cmd left in a Proton 11 prefix,
        // doubled backslashes included; a class without "URL Protocol" is
        // no link.
        assert_eq!(
            in_registry(REG),
            vec![(
                "bf3lan".to_string(),
                r"Z:\home\kevin\LAN\bf3\\local\\BFUlauncher.exe %1".to_string()
            )]
        );
        assert_eq!(
            split_command(r#""C:\Program Files\a.exe" "%1" -x"#),
            vec![r"C:\Program Files\a.exe", "%1", "-x"]
        );
    }

    // `Z:` is the root of a Unix file system; the links are opened on Linux
    // only.
    #[cfg(unix)]
    #[test]
    fn a_handler_runs_only_a_program_of_its_game() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("bf3");
        std::fs::create_dir_all(game.join("local")).unwrap();
        std::fs::write(game.join("local/BFUlauncher.exe"), b"MZ").unwrap();
        let pfx = game.join(".nll-prefix/pfx");
        std::fs::create_dir_all(pfx.join("drive_c/windows")).unwrap();
        std::fs::write(pfx.join("drive_c/windows/notepad.exe"), b"MZ").unwrap();
        let z = |p: &Path| format!("Z:{}", p.display()).replace('/', "\\\\\\\\");
        let reg = format!(
            "[Software\\\\Classes\\\\bf3lan]\n\"URL Protocol\"=\"\"\n\
             [Software\\\\Classes\\\\bf3lan\\\\shell\\\\open\\\\command]\n@=\"{} %1\"\n\
             [Software\\\\Classes\\\\evil]\n\"URL Protocol\"=\"\"\n\
             [Software\\\\Classes\\\\evil\\\\shell\\\\open\\\\command]\n@=\"C:\\\\windows\\\\notepad.exe %1\"\n",
            z(&game.join("LOCAL/bfulauncher.exe"))
        );
        std::fs::write(pfx.join("system.reg"), reg).unwrap();
        let handlers = handlers_of(&game);
        assert_eq!(handlers.len(), 1, "{handlers:?}");
        assert_eq!(handlers[0].scheme, "bf3lan");
        assert_eq!(
            handlers[0].program,
            game.join("local/BFUlauncher.exe").canonicalize().unwrap()
        );
        assert_eq!(
            handlers[0].args_for("bf3lan://join/1"),
            vec!["bf3lan://join/1"]
        );
        assert!(registries_changed(&game).is_some());
    }

    #[test]
    fn only_a_single_link_of_a_game_scheme_is_opened() {
        assert_eq!(scheme("bf3lan://join/1.2.3.4").as_deref(), Some("bf3lan"));
        assert_eq!(scheme("BF3LAN://x").as_deref(), Some("bf3lan"));
        assert_eq!(scheme("bf3lan://a b"), None);
        assert_eq!(scheme("bf3lan://a\nstatus"), None);
        assert_eq!(scheme("https://example.org"), None);
        assert_eq!(scheme("3x://y"), None);
        assert_eq!(scheme("nocolon"), None);
    }

    #[test]
    fn the_desktop_gets_the_launcher_for_the_schemes_nobody_else_has() {
        let tmp = tempfile::tempdir().unwrap();
        let (data, config) = (tmp.path().join("data"), tmp.path().join("config"));
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("mimeapps.list"),
            "[Added Associations]\ntext/plain=kate.desktop;\n\n[Default Applications]\nx-scheme-handler/bf4lan=other.desktop\ntext/html=firefox.desktop\n\n[Removed Associations]\n",
        )
        .unwrap();
        let schemes = vec!["bf3lan".to_string(), "bf4lan".to_string()];
        let exec = Path::new("/home/a b/NextGen 100%.AppImage");
        assert!(register(&schemes, exec, &data, &config).unwrap());
        let entry = std::fs::read_to_string(data.join("applications").join(DESKTOP_FILE)).unwrap();
        assert!(entry.contains("Exec=\"/home/a b/NextGen 100%%.AppImage\" --open-url %u\n"));
        assert!(entry.contains("MimeType=x-scheme-handler/bf3lan;x-scheme-handler/bf4lan;\n"));
        let list = std::fs::read_to_string(config.join("mimeapps.list")).unwrap();
        assert_eq!(
            list,
            format!(
                "[Added Associations]\ntext/plain=kate.desktop;\n\n[Default Applications]\nx-scheme-handler/bf4lan=other.desktop\ntext/html=firefox.desktop\nx-scheme-handler/bf3lan={DESKTOP_FILE}\n\n[Removed Associations]\n"
            )
        );
        // Again: nothing to write.
        assert!(!register(&schemes, exec, &data, &config).unwrap());
        // The game is gone: the offer goes, the other program's entry stays.
        assert!(register(&[], exec, &data, &config).unwrap());
        assert!(!data.join("applications").join(DESKTOP_FILE).exists());
        let list = std::fs::read_to_string(config.join("mimeapps.list")).unwrap();
        assert!(!list.contains(DESKTOP_FILE), "{list}");
        assert!(list.contains("x-scheme-handler/bf4lan=other.desktop"));
    }

    #[test]
    fn registry_strings_keep_their_umlauts_and_links_their_percent_signs() {
        assert_eq!(
            unescape(r"Z:\\home\\j\x00fcrgen\\a.exe"),
            r"Z:\home\jürgen\a.exe"
        );
        assert_eq!(unescape(r#"a\x00e4b\"c\""#), "aäb\"c\"");
        let h = Handler {
            scheme: "x".into(),
            program: PathBuf::from("/a"),
            args: vec!["-u".into(), "\"%1\"".into()],
        };
        assert_eq!(h.args_for("x://a%Lb%1"), vec!["-u", "\"x://a%Lb%1\""]);
    }

    #[test]
    fn an_entry_shared_with_another_program_keeps_the_other() {
        let text = format!(
            "[Default Applications]\nx-scheme-handler/bf3lan={DESKTOP_FILE};firefox.desktop;\n"
        );
        assert_eq!(
            with_defaults(&text, &[]),
            "[Default Applications]\nx-scheme-handler/bf3lan=firefox.desktop;\n"
        );
        assert_eq!(with_defaults(&text, &["bf3lan".to_string()]), text);
    }

    #[test]
    fn a_missing_list_gets_its_section() {
        assert_eq!(
            with_defaults("", &["bf3lan".to_string()]),
            format!("[Default Applications]\nx-scheme-handler/bf3lan={DESKTOP_FILE}\n")
        );
        assert_eq!(with_defaults("", &[]), "");
        let kept = "[Default Applications]\ntext/html=firefox.desktop";
        assert_eq!(with_defaults(kept, &[]), kept);
    }
}
