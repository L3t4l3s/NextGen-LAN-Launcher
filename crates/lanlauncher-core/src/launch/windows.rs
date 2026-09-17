//! Windows: run the original ETI batch scripts.

use super::{LaunchContext, LaunchPlan};
use crate::error::{Error, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Folder the ETI scripts expect helper tools in
/// (`%programfiles%\eti\lan launcher\unrar.exe`, `fnr.exe`).
pub fn legacy_tools_dir() -> PathBuf {
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    Path::new(&pf).join("eti").join("lan launcher")
}

/// `cmd.exe /S /C "<script> <args…>"` for `game_start.cmd "<game_path>" <id> <lang> "<player>"`.
///
/// `/S` makes cmd strip exactly the outer pair of quotes, so paths with
/// spaces and the quoted player name survive intact. The whole string is
/// passed verbatim (`LaunchPlan::raw_command_line`).
pub fn plan(ctx: &LaunchContext<'_>) -> Result<LaunchPlan> {
    if !ctx.paths.start_script.is_file() {
        return Err(Error::Code("err.start_script_missing".into()));
    }
    let raw = script_command_line(
        &ctx.paths.start_script,
        &ctx.paths.share_dir,
        ctx.game_id,
        &ctx.settings.game_language,
        &ctx.settings.safe_player_name(),
    );
    // Firewall rules are registered at setup time, so only scripts that
    // write HKLM (16 of 158 official ones) need the UAC prompt at play time.
    let needs_elevation = std::fs::read_to_string(&ctx.paths.start_script)
        .map(|t| super::elevate::script_needs_admin(&t))
        .unwrap_or(false);
    Ok(LaunchPlan {
        program: PathBuf::from(comspec()),
        args: vec!["/S".into(), "/C".into(), raw.clone()],
        cwd: ctx.paths.share_dir.clone(),
        env: BTreeMap::new(),
        runner: "game_start.cmd".into(),
        needs_elevation,
        raw_command_line: Some(format!("/S /C {raw}")),
    })
}

fn comspec() -> String {
    std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into())
}

fn quote(s: &str) -> String {
    // cmd has no escape for a double quote inside a quoted string; the
    // player name is already sanitised (no quotes) by Settings::safe_player_name.
    format!("\"{}\"", s.replace('"', ""))
}

/// The ETI contract: `game_start.cmd "%game_path%" %game_id% %game_lang% "%player%"`.
pub fn script_command_line(
    script: &Path,
    share_dir: &Path,
    game_id: &str,
    lang: &str,
    player: &str,
) -> String {
    format!(
        "\"{} {} {} {} {}\"",
        quote(&script.to_string_lossy()),
        quote(&share_dir.to_string_lossy()),
        game_id,
        lang,
        quote(player)
    )
}

/// Plan for the one-time `game_setup.cmd`. ETI calls it with the same four
/// arguments as `game_start.cmd` (`"<game_path>" <id> <lang> "<player>"`);
/// a quarter of the official setup scripts read `%3`/`%4`.
pub fn setup_plan(
    paths: &crate::paths::GamePaths,
    game_id: &str,
    lang: &str,
    player: &str,
) -> Option<LaunchPlan> {
    script_plan(&paths.setup_script, paths, game_id, lang, player)
}

/// Plan for the optional `server_start.cmd` (dedicated server). The official
/// scripts only rely on `%~dp0`, but they get the same four arguments as
/// `game_start.cmd` so a script that reads them keeps working.
pub fn server_plan(
    paths: &crate::paths::GamePaths,
    game_id: &str,
    lang: &str,
    player: &str,
) -> Option<LaunchPlan> {
    script_plan(&paths.server_script(), paths, game_id, lang, player)
}

/// `cmd.exe /S /C "<script> "<game_path>" <id> <lang> "<player>""` for any
/// ETI batch script in the game folder; `None` when the script is absent.
fn script_plan(
    script: &Path,
    paths: &crate::paths::GamePaths,
    game_id: &str,
    lang: &str,
    player: &str,
) -> Option<LaunchPlan> {
    if !script.is_file() {
        return None;
    }
    let raw = script_command_line(script, &paths.share_dir, game_id, lang, player);
    let runner = script
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "script".into());
    // Setup scripts install redistributables and write HKLM; server scripts
    // open ports themselves. Both keep the prompt.
    Some(LaunchPlan {
        program: PathBuf::from(comspec()),
        args: vec!["/S".into(), "/C".into(), raw.clone()],
        cwd: paths.share_dir.clone(),
        env: BTreeMap::new(),
        runner,
        needs_elevation: true,
        raw_command_line: Some(format!("/S /C {raw}")),
    })
}

/// Programs an ETI script calls that are not on the disk.
///
/// The scripts run helpers from the ETI installation
/// (`%programfiles%\eti\lan launcher\unrar.exe`, `fnr.exe`). Where those are
/// missing — every machine that never had the original launcher — cmd answers
/// "The system cannot find the path specified" and names nothing it looked
/// for, so a failed setup told the user neither what nor why.
///
/// Only absolute paths in command position count: `del "C:\tmp\old.exe"` and
/// `if exist "…"` are about a file, not a program to run, and naming those
/// would turn a helpful message into a wrong one. `set` lines are followed,
/// because keeping the helper in a variable and calling `%unrar%` later is
/// the common shape.
///
/// `env` resolves `%VAR%` (on Windows `std::env::var` is the right one: it
/// matches case-insensitively, as cmd does), `exists` answers for the file
/// system. A token whose variables cannot be resolved is left alone: an
/// unknown path is not a missing one.
pub fn missing_tools(
    script: &str,
    env: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    let mut found: Vec<PathBuf> = Vec::new();
    for line in script.lines() {
        let trimmed = line.trim_start().trim_start_matches('@').trim_start();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("rem ") || lower.starts_with("::") || lower == "rem" {
            continue;
        }
        if lower.starts_with("set ") {
            // Both spellings: `set "var=value"` and `set var="value"`.
            let rest = trimmed[4..].trim();
            let rest = rest
                .strip_prefix('"')
                .and_then(|r| r.strip_suffix('"'))
                .unwrap_or(rest);
            if let Some((name, value)) = rest.split_once('=') {
                let resolved = expand(value.trim().trim_matches('"'), &|n| lookup(&vars, env, n));
                if let Some(v) = resolved {
                    vars.insert(name.trim().trim_matches('"').to_ascii_lowercase(), v);
                }
            }
            continue;
        }
        // How many of the next words may name a program. One, except after
        // `start`, whose first argument is a window title.
        let mut candidates = 1;
        for token in tokens(trimmed) {
            // `/wait`, `/b`, `/min`: switches, not the program.
            if candidates > 0 && !token.starts_with('/') {
                candidates -= 1;
                if let Some(path) = expand(&token, &|n| lookup(&vars, env, n)) {
                    if is_absolute_program(&path) {
                        let path = PathBuf::from(path);
                        if !exists(&path) && !found.contains(&path) {
                            found.push(path);
                        }
                    }
                }
            }
            if let Some(n) = precedes_a_command(&token) {
                candidates = n;
            }
        }
    }
    found
}

/// A variable of the script itself before the machine's environment: the
/// script's own `set` is what decides where it looks.
fn lookup(
    vars: &BTreeMap<String, String>,
    env: &dyn Fn(&str) -> Option<String>,
    name: &str,
) -> Option<String> {
    vars.get(&name.to_ascii_lowercase())
        .cloned()
        .or_else(|| env(name))
}

/// Tokens after which cmd expects a program to run, and how many of the
/// following words may be that program: `start "Title" "prog.exe"` puts a
/// window title in front of it, everything else names the program next.
fn precedes_a_command(token: &str) -> Option<usize> {
    match token.to_ascii_lowercase().as_str() {
        "start" => Some(2),
        "call" | "do" | "then" | "else" | "&" | "&&" | "|" | "||" | "(" => Some(1),
        _ => None,
    }
}

/// Words of a batch line: a quoted string is one word, everything else splits
/// on whitespace and on the separators cmd itself treats as such.
fn tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => {
                out.push(std::mem::take(&mut current));
                quoted = !quoted;
            }
            c if !quoted && (c.is_whitespace() || c == '=' || c == ',' || c == ';') => {
                out.push(std::mem::take(&mut current));
            }
            c => current.push(c),
        }
    }
    out.push(current);
    out.retain(|t| !t.is_empty());
    out
}

/// `%VAR%` replaced by its value; `None` when a variable is unknown or the
/// token uses cmd's own expansions (`%~dp0`, `%1`), which say nothing about a
/// path on this machine.
fn expand(token: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let mut out = String::new();
    let mut rest = token;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('%')?;
        let name = &after[..end];
        if name.is_empty() || name.starts_with('~') || name.contains(char::is_whitespace) {
            return None;
        }
        out.push_str(&env(name)?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// An absolute Windows path to something executable — the only kind of
/// reference whose absence can be stated without guessing.
fn is_absolute_program(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if !(lower.ends_with(".exe")
        || lower.ends_with(".com")
        || lower.ends_with(".bat")
        || lower.ends_with(".cmd"))
    {
        return false;
    }
    let bytes = path.as_bytes();
    let drive = bytes.len() > 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    drive || path.starts_with(r"\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_follows_eti_contract_and_cmd_quoting() {
        let line = script_command_line(
            Path::new(r"D:\LAN Party\quake3\game_start.cmd"),
            Path::new(r"D:\LAN Party\quake3"),
            "quake3",
            "de",
            "Player One",
        );
        assert_eq!(
            line,
            r#"""D:\LAN Party\quake3\game_start.cmd" "D:\LAN Party\quake3" quake3 de "Player One"""#
        );
        assert_eq!(quote("Ev\"il"), "\"Evil\"");
    }

    #[test]
    fn a_script_names_the_helper_programs_it_cannot_find() {
        // The line that fails in practice: ETI's setup scripts patch config
        // files with fnr.exe from the original launcher's folder. Without it
        // cmd says only "The system cannot find the path specified".
        let script = concat!(
            "@echo off\r\n",
            "rem \"%programfiles%\\eti\\lan launcher\\notchecked.exe\"\r\n",
            "set \"unrar=%programfiles%\\eti\\lan launcher\\unrar.exe\"\r\n",
            "set patch=\"%programfiles%\\eti\\lan launcher\\patch.exe\"\r\n",
            "\"%programfiles%\\eti\\lan launcher\\fnr.exe\" --cl --dir \"%1\" --fileMask \"*.ini\"\r\n",
            "if exist data.rar call \"%unrar%\" x -y data.rar\r\n",
            "start /wait \"Setup\" \"%programfiles%\\eti\\lan launcher\\quiet.exe\" /s\r\n",
            "%patch% -q config.ini\r\n",
            "del \"C:\\Temp\\leftover.exe\"\r\n",
            "call \"%~dp0other.cmd\"\r\n",
            "start \"\" \"C:\\Windows\\System32\\reg.exe\" add HKLM\\Software\\Game\r\n",
            "echo done\r\n",
        );
        let env = |name: &str| match name.to_ascii_lowercase().as_str() {
            "programfiles" => Some(r"C:\Program Files".to_string()),
            _ => None,
        };
        let here = |p: &Path| p == Path::new(r"C:\Windows\System32\reg.exe");
        let missing = missing_tools(script, &env, &here);
        assert_eq!(
            missing,
            vec![
                PathBuf::from(r"C:\Program Files\eti\lan launcher\fnr.exe"),
                PathBuf::from(r"C:\Program Files\eti\lan launcher\unrar.exe"),
                PathBuf::from(r"C:\Program Files\eti\lan launcher\quiet.exe"),
                PathBuf::from(r"C:\Program Files\eti\lan launcher\patch.exe"),
            ],
            "the four helpers — two through variables the script set, in both \
             spellings, one behind `start`'s switches and window title; a \
             comment, a file being deleted, %~dp0 and what exists are not \
             programs this script could not find"
        );
        // Nothing to say about a script that calls only what is there.
        assert!(missing_tools(script, &env, &|_| true).is_empty());
        // An unknown variable is not a missing file: nobody knows where it
        // would have pointed, so only the plain path is left.
        assert_eq!(
            missing_tools(script, &|_| None, &|_| false),
            vec![PathBuf::from(r"C:\Windows\System32\reg.exe")]
        );
    }

    #[test]
    fn setup_plan_uses_the_full_contract_and_raw_command_line() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = crate::paths::GamePaths::new(tmp.path(), "cnctw");
        assert!(setup_plan(&paths, "cnctw", "de", "Player").is_none());
        std::fs::create_dir_all(&paths.share_dir).unwrap();
        std::fs::write(&paths.setup_script, "echo setup").unwrap();
        let plan = setup_plan(&paths, "cnctw", "de", "Player One").unwrap();
        let expected = format!(
            "/S /C \"\"{}\" \"{}\" cnctw de \"Player One\"\"",
            paths.setup_script.display(),
            paths.share_dir.display()
        );
        assert_eq!(plan.raw_command_line.as_deref(), Some(expected.as_str()));
        assert_eq!(plan.cwd, paths.share_dir);
        assert!(plan.needs_elevation);
    }
}
