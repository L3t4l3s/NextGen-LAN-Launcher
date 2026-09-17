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

/// What an ETI script reaches for and is not there.
///
/// Two shapes fail in practice. The helpers of the original launcher
/// (`%programfiles%\eti\lan launcher\unrar.exe`, `fnr.exe`) on a machine that
/// never had it, and a program inside the game's own folder that the package
/// did not bring — `cd /d "%~dp0"`, `cd local`, `"OpenAL\oalinst.exe"` is the
/// usual shape of a setup script. Both end in cmd's "The system cannot find
/// the path specified", which names nothing it looked for, so a failed setup
/// told the user neither what nor why.
///
/// The script is followed the way cmd would: `set` fills variables, `cd` and
/// `pushd` move the working directory (`%~dp0` is `script_dir`), and a
/// program in command position is resolved against wherever the script
/// currently stands. `del "C:\tmp\old.exe"` and `if exist "…"` are about a
/// file rather than a program to run and are left alone, or a helpful message
/// would turn into a wrong one.
///
/// `env` resolves `%VAR%` (on Windows `std::env::var` is the right one: it
/// matches case-insensitively, as cmd does), `exists` answers for the file
/// system. Where a variable cannot be resolved the trail stops: an unknown
/// path is not a missing one, and a working directory nobody knows makes
/// every relative path below it unknown too.
pub fn missing_paths(
    script: &str,
    script_dir: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<String> {
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    let mut found: Vec<String> = Vec::new();
    let script_dir = script_dir.to_string_lossy().replace('/', "\\");
    let mut cwd = Some(script_dir.trim_end_matches('\\').to_string());
    let mut stack: Vec<Option<String>> = Vec::new();
    // Paths the script creates on its way: `md work` and then `cd work` is
    // not a missing directory, it is a directory about to exist.
    let mut made: Vec<String> = Vec::new();
    let note = |path: String, made: &[String], found: &mut Vec<String>| {
        // Windows does not care about case, and `md Work` / `cd work` is the
        // same folder.
        let lower = path.to_ascii_lowercase();
        if made.iter().any(|m| {
            let m = m.to_ascii_lowercase();
            lower == m || lower.starts_with(&format!("{m}\\"))
        }) {
            return;
        }
        if !exists(Path::new(&path)) && !found.iter().any(|f| f.to_ascii_lowercase() == lower) {
            found.push(path);
        }
    };
    for line in script.lines() {
        let line = line.trim_start().trim_start_matches('@').trim_start();
        let lower_line = line.to_ascii_lowercase();
        if lower_line.starts_with("rem ") || lower_line.starts_with("::") || lower_line == "rem" {
            continue;
        }
        // `cd local && "OpenAL\oalinst.exe"` is two commands, and the second
        // one is the interesting half.
        for statement in statements(line) {
            let trimmed = until_the_next_command(statement).trim();
            let lower = trimmed.to_ascii_lowercase();
            let resolve = |token: &str, vars: &BTreeMap<String, String>| {
                expand(token, &|n| lookup(vars, env, n), &script_dir)
            };
            if lower.starts_with("set ") {
                // Both spellings: `set "var=value"` and `set var="value"`.
                let rest = trimmed[4..].trim();
                let rest = rest
                    .strip_prefix('"')
                    .and_then(|r| r.strip_suffix('"'))
                    .unwrap_or(rest);
                if let Some((name, value)) = rest.split_once('=') {
                    if let Some(v) = resolve(value.trim().trim_matches('"'), &vars) {
                        vars.insert(name.trim().trim_matches('"').to_ascii_lowercase(), v);
                    }
                }
                continue;
            }
            let words = tokens(trimmed);
            let Some(first) = words.first().map(|w| w.to_ascii_lowercase()) else {
                continue;
            };
            // At an `exit` the reading stops. Whether this one runs cannot be
            // told from the text, and the two in ETI's own scripts are the
            // last line of their file — naming a path from a branch that
            // never ran would send the user to repair a package that is fine.
            if first == "exit" {
                return found;
            }
            // A script that branches is no longer a list of steps: a `cd`
            // inside an `if` or behind a `goto` may never run, and carrying
            // it on would name paths that were never used. The working
            // directory is unknown from here, so only what brings its own —
            // an absolute path — is still named. The line itself is read on:
            // `if exist x call "C:\…\unrar.exe"` says something.
            if matches!(first.as_str(), "if" | "for" | "goto")
                || first.starts_with(':')
                || trimmed.starts_with('(')
            {
                cwd = None;
            }
            if matches!(first.as_str(), "md" | "mkdir") {
                let target = trimmed[first.len()..].trim().trim_matches('"');
                if let Some(path) = resolve(target, &vars).and_then(|t| join(cwd.as_deref(), &t)) {
                    made.push(path);
                }
                continue;
            }
            // `D:` on its own changes the drive, nothing else.
            if first.len() == 2 && first.ends_with(':') && drive_of(&first).is_some() {
                cwd = Some(first.to_ascii_uppercase());
                continue;
            }
            if first == "popd" {
                cwd = stack.pop().unwrap_or(cwd);
                continue;
            }
            // `cd..` and `cd\` need no space, and cmd takes them — but only
            // as a command word. `"cd\setup.exe"` in quotes is a program in a
            // folder that happens to be called `cd`.
            let glued = first
                .strip_prefix("cd")
                .filter(|_| !trimmed.starts_with('"'))
                .filter(|rest| rest.starts_with('.') || rest.starts_with('\\'));
            if glued.is_some() || matches!(first.as_str(), "cd" | "chdir" | "pushd") {
                if first == "pushd" {
                    stack.push(cwd.clone());
                }
                // The rest of the line is the directory, not the first word of
                // it: `cd /d %programfiles%\eti\lan launcher` has two spaces in
                // its path and no quotes around it. The switches are cmd's.
                let mut rest = match glued {
                    Some(_) => &trimmed[2..],
                    None => trimmed[first.len()..].trim(),
                };
                while let Some(after) = rest.strip_prefix('/') {
                    // `cd /d"C:\x"` needs no space after the switch.
                    rest = match after.find(|c: char| c.is_whitespace() || c == '"') {
                        Some(at) => after[at..].trim_start(),
                        None => "",
                    };
                }
                let target = rest.trim().trim_matches('"');
                if target.is_empty() {
                    continue;
                }
                cwd = resolve(target, &vars).and_then(|target| join(cwd.as_deref(), &target));
                // A directory that is not there is exactly the answer cmd gives
                // ("the path specified"), so it belongs in the list as much as a
                // program does.
                if let Some(dir) = &cwd {
                    note(dir.clone(), &made, &mut found);
                }
                continue;
            }
            // `start /d "local" game.exe` runs the program in another
            // directory, and the switch that says which is one this reader
            // does not follow. Naming a path from the wrong directory is
            // worse than naming none.
            if first == "start"
                && words
                    .iter()
                    .any(|w| w.to_ascii_lowercase().starts_with("/d"))
            {
                continue;
            }
            // How many of the next words may name a program. One, except
            // after `start`, whose first argument may be a window title.
            let mut candidates = 1;
            let mut named: Vec<String> = Vec::new();
            for token in &words {
                // `/wait`, `/b`, `/min`: switches, not the program.
                if candidates > 0 && !token.starts_with('/') {
                    candidates -= 1;
                    if let Some(name) = resolve(token, &vars).filter(|p| looks_like_a_program(p)) {
                        named.push(name);
                    }
                }
                if let Some(n) = precedes_a_command(token) {
                    candidates = n;
                }
            }
            // `start "launcher.exe" "local\real.exe"` runs the second one;
            // the first is what the window is called. Only a *quoted* first
            // argument is a title, though — `start setup.exe patch.exe` runs
            // setup.exe and hands it an argument.
            if first == "start" && named.len() > 1 {
                if after_start_is_quoted(trimmed) {
                    named.remove(0);
                }
                // What follows the program is its arguments, whatever they
                // look like.
                named.truncate(1);
            }
            for name in named {
                // A bare name is what cmd looks up on `PATH` after the current
                // directory: `reg.exe` lives in the system folder and is not
                // missing because a game folder has no copy.
                let bare = !name.contains('\\') && !name.contains('/');
                if let Some(path) = join(cwd.as_deref(), &name) {
                    if !(bare && on_the_path(&name, &|n| lookup(&vars, env, n), exists)) {
                        note(path, &made, &mut found);
                    }
                }
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

/// Does the first argument of a `start` stand in quotes? Then it is the
/// window's title, not the program.
/// An empty title (`start "" "prog.exe"`) does not count: `tokens` drops it,
/// so there is no word to skip.
fn after_start_is_quoted(statement: &str) -> bool {
    let mut rest = statement.trim_start()[5..].trim_start();
    while let Some(after) = rest.strip_prefix('/') {
        rest = after
            .split_once(char::is_whitespace)
            .map(|(_, r)| r.trim_start())
            .unwrap_or("");
    }
    rest.starts_with('"') && !rest.starts_with("\"\"")
}

/// The commands of one line: cmd chains them with `&`, `&&`, `|` and `||`,
/// and a script that writes `cd local && "OpenAL\oalinst.exe"` means both
/// halves. Quoted text is passed over, because a path may contain anything.
fn statements(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '&' | '|' if !quoted => {
                if i > start {
                    out.push(&line[start..i]);
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&line[start..]);
    out
}

/// One command up to its redirections: `cd /d "%~dp0" >nul 2>&1` is one
/// directory and two redirections, not a directory with a strange name.
fn until_the_next_command(line: &str) -> &str {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '>' | '<' if !quoted => return &line[..i],
            _ => {}
        }
    }
    line
}

/// A path as cmd would read it where the script stands: an absolute one as
/// it is, one that begins with a backslash at the root of the current drive,
/// and anything else below `cwd`. `.` and `..` are followed, repeated and
/// trailing separators collapse (`%~dp0` ends in one, and scripts write
/// `"%~dp0\local"` all the same). `None` where nobody knows what the working
/// directory is.
fn join(cwd: Option<&str>, path: &str) -> Option<String> {
    let path = path.replace('/', "\\");
    let (mut base, rest) = if let Some(unc) = path.strip_prefix("\\\\") {
        let mut parts = unc.splitn(3, '\\');
        let server = parts.next().unwrap_or_default();
        let share = parts.next().unwrap_or_default();
        (
            format!("\\\\{server}\\{share}"),
            parts.next().unwrap_or_default().to_string(),
        )
    } else if drive_of(&path).is_some() {
        (path[..2].to_string(), path[2..].to_string())
    } else if path.starts_with('\\') {
        (drive_of(cwd?)?.to_string(), path.clone())
    } else {
        (cwd?.to_string(), path.clone())
    };
    for part in rest.split('\\') {
        match part {
            "" | "." => {}
            ".." => {
                if let Some(cut) = base.rfind('\\') {
                    base.truncate(cut);
                }
            }
            part => {
                base.push('\\');
                base.push_str(part);
            }
        }
    }
    Some(base)
}

/// `C:` of a path that names a drive.
fn drive_of(path: &str) -> Option<&str> {
    let bytes = path.as_bytes();
    (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':').then(|| &path[..2])
}

/// `%VAR%` replaced by its value, and `%~dp0` by the script's own folder —
/// every ETI setup script begins with `cd /d "%~dp0"`. `None` when a variable
/// is unknown or the token uses one of cmd's other expansions (`%1`, `%~n0`),
/// which say nothing about a path on this machine.
fn expand(token: &str, env: &dyn Fn(&str) -> Option<String>, script_dir: &str) -> Option<String> {
    // `!var!` is read when the line runs, not when it is parsed: every ETI
    // script turns delayed expansion on, and what it holds is unknowable here.
    if token.matches('!').count() >= 2 {
        return None;
    }
    // Case-insensitively — cmd does not care, and `%~DP0` is in the wild —
    // and in one pass over the original, so a folder whose own name contains
    // the pattern cannot make this go round for ever.
    let here = format!("{}\\", script_dir.trim_end_matches('\\'));
    let lower = token.to_ascii_lowercase();
    let mut expanded = String::with_capacity(token.len());
    let mut rest = 0;
    while let Some(at) = lower[rest..].find("%~dp0") {
        expanded.push_str(&token[rest..rest + at]);
        expanded.push_str(&here);
        rest += at + 5;
    }
    expanded.push_str(&token[rest..]);
    let token = expanded;
    let mut out = String::new();
    let mut rest = token.as_str();
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

/// Is a program of that name on `PATH`? Only for a bare name: cmd looks in
/// the current directory first and then along `PATH`, so a script calling
/// `reg.exe` means the one in the system folder.
fn on_the_path(
    name: &str,
    env: &dyn Fn(&str) -> Option<String>,
    exists: &dyn Fn(&Path) -> bool,
) -> bool {
    let Some(path) = env("PATH") else {
        return false;
    };
    path.split(';')
        .map(|dir| dir.trim().trim_end_matches('\\'))
        .filter(|dir| !dir.is_empty())
        .any(|dir| exists(Path::new(&format!("{dir}\\{name}"))))
}

/// Something the script means to run, by its extension — the only tokens
/// whose absence can be stated without guessing what the line intended.
fn looks_like_a_program(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".exe")
        || lower.ends_with(".com")
        || lower.ends_with(".bat")
        || lower.ends_with(".cmd")
        || lower.ends_with(".msi")
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
    fn a_script_names_what_it_reaches_for_and_does_not_find() {
        // The two shapes that fail in practice, in one script: a helper of
        // the original launcher through a variable, and a program inside the
        // game's folder that the package did not bring — the latter is what
        // `opencnc` does, and what "The system cannot find the path
        // specified" meant on the LAN.
        let script = concat!(
            "@echo off\r\n",
            "rem \"%programfiles%\\eti\\lan launcher\\notchecked.exe\"\r\n",
            "set \"unrar=%programfiles%\\eti\\lan launcher\\unrar.exe\"\r\n",
            "cd /d \"%~dp0\"\r\n",
            "cd local\r\n",
            "\"OpenAL\\oalinst.exe\"\r\n",
            "\"Configuration.exe\"\r\n",
            "call \"%unrar%\" x -y data.rar\r\n",
            "del \"C:\\Temp\\leftover.exe\"\r\n",
            "call \"%~dp0other.cmd\"\r\n",
            "start /wait \"Setup\" \"%programfiles%\\eti\\lan launcher\\quiet.exe\" /s\r\n",
            "echo done\r\n",
        );
        let env = |name: &str| match name.to_ascii_lowercase().as_str() {
            "programfiles" => Some(r"C:\Program Files".to_string()),
            _ => None,
        };
        // Everything is there except the OpenAL installer and the helpers.
        let here = |p: &Path| {
            let p = p.to_string_lossy().to_string();
            !p.contains("eti\\lan launcher") && !p.contains("OpenAL")
        };
        let missing = missing_paths(script, Path::new(r"E:\LAN\opencnc"), &env, &here);
        assert_eq!(
            missing,
            vec![
                r"E:\LAN\opencnc\local\OpenAL\oalinst.exe".to_string(),
                r"C:\Program Files\eti\lan launcher\unrar.exe".to_string(),
                r"C:\Program Files\eti\lan launcher\quiet.exe".to_string(),
            ],
            "the program under the game's own folder and the helpers of the \
             old launcher; a comment, a file being deleted, `%~dp0other.cmd` \
             (which is there) and what exists are not what the script could \
             not find"
        );
        // `start setup.exe patch.exe` runs the first one: without quotes
        // there is no window title to skip, and `patch.exe` is an argument
        // to it, not the program.
        assert_eq!(
            missing_paths(
                "cd /d \"%~dp0\"\r\nstart setup.exe patch.exe\r\n",
                Path::new(r"E:\LAN\g"),
                &env,
                &|p: &Path| p == Path::new(r"E:\LAN\g"),
            ),
            vec![r"E:\LAN\g\setup.exe".to_string()]
        );
        // A folder the script makes itself is not one that is missing.
        assert_eq!(
            missing_paths(
                "cd /d \"%~dp0\"\r\nmd work\r\ncd work\r\n\"tool.exe\"\r\n",
                Path::new(r"E:\LAN\g"),
                &env,
                &|p: &Path| p == Path::new(r"E:\LAN\g"),
            ),
            Vec::<String>::new()
        );
        // A working directory that is not there is the same answer cmd gives.
        let nothing = |_: &Path| false;
        // The reading stops at an `exit`, wherever it stands: whether this
        // one runs cannot be told from the text — the two in ETI's scripts
        // are the last line of their file — and a path from a branch that
        // never ran would send the user to repair a package that is fine.
        assert_eq!(
            missing_paths(
                concat!(
                    "cd /d \"%~dp0\"\r\n",
                    "if exist save.dat echo keep\r\n",
                    "exit /b 0\r\n",
                    "\"%programfiles%\\eti\\lan launcher\\never.exe\"\r\n",
                ),
                Path::new(r"E:\LAN\g"),
                &env,
                &nothing,
            ),
            vec![r"E:\LAN\g".to_string()]
        );
        // A drive of its own, a value only known while the line runs, and
        // everything after an `exit`.
        assert_eq!(
            missing_paths(
                concat!(
                    "cd /d \"%~dp0\"\r\n",
                    "D:\r\n",
                    "cd \\games\r\n",
                    "\"setup.exe\"\r\n",
                    "cd !elsewhere!\r\n",
                    "\"other.exe\"\r\n",
                    "exit /b 0\r\n",
                    "\"never.exe\"\r\n",
                ),
                Path::new(r"E:\LAN\g"),
                &env,
                &nothing,
            ),
            vec![
                r"E:\LAN\g".to_string(),
                r"D:\games".to_string(),
                r"D:\games\setup.exe".to_string(),
            ],
            "the other drive is followed, the delayed value stops the trail, \
             and nothing after `exit` is read"
        );
        // `%~dp0` ends in a backslash and scripts write one after it anyway;
        // `\tools` is the root of the current drive, not of the game folder.
        assert_eq!(
            missing_paths(
                concat!(
                    "cd /d \"%~dp0\"\r\n",
                    "\"%~dp0\\local\\game.exe\"\r\n",
                    "cd \\tools\r\n",
                    "\"patch.exe\"\r\n",
                ),
                Path::new(r"E:\LAN\g"),
                &env,
                &nothing,
            ),
            vec![
                r"E:\LAN\g".to_string(),
                r"E:\LAN\g\local\game.exe".to_string(),
                r"E:\tools".to_string(),
                r"E:\tools\patch.exe".to_string(),
            ]
        );
        // A branch makes the working directory unknown — what is behind an
        // `if` may never run — but an absolute path still says something, and
        // `start`'s window title is not the program.
        assert_eq!(
            missing_paths(
                concat!(
                    "cd /d \"%~dp0\"\r\n",
                    "if exist save.dat cd backup\r\n",
                    "\"tool.exe\"\r\n",
                    "\"%programfiles%\\eti\\lan launcher\\fnr.exe\" --cl\r\n",
                    "start \"launcher.exe\" \"real.exe\"\r\n",
                ),
                Path::new(r"E:\LAN\g"),
                &env,
                &nothing,
            ),
            vec![
                r"E:\LAN\g".to_string(),
                r"C:\Program Files\eti\lan launcher\fnr.exe".to_string(),
            ],
            "no path is invented for a directory nobody knows"
        );
        // A switch glued to its path, and `cd..`/`cd\` without a space.
        assert_eq!(
            missing_paths(
                "cd /d\"%~dp0\"\r\ncd local\r\ncd..\r\n\"game.exe\"\r\n",
                Path::new(r"E:\LAN\g"),
                &env,
                &|p: &Path| p != Path::new(r"E:\LAN\g\game.exe"),
            ),
            vec![r"E:\LAN\g\game.exe".to_string()],
            "back where it started: beside the script, not below `local`"
        );
        // Three shapes of `cd` that used to end in a wrong path or none.
        let quirks = concat!(
            "cd /d %programfiles%\\eti\\lan launcher\r\n",
            "\"unrar.exe\"\r\n",
            "cd /d \"C:\\\"\r\n",
            "\"boot.exe\"\r\n",
            "cd /d \"%~DP0\"\r\n",
            "\"start.exe\"\r\n",
        );
        assert_eq!(
            missing_paths(quirks, Path::new(r"E:\LAN\g"), &env, &nothing),
            vec![
                r"C:\Program Files\eti\lan launcher".to_string(),
                r"C:\Program Files\eti\lan launcher\unrar.exe".to_string(),
                // Without the drive's trailing backslash, which is how it
                // goes on to carry the paths below it.
                "C:".to_string(),
                r"C:\boot.exe".to_string(),
                r"E:\LAN\g".to_string(),
                r"E:\LAN\g\start.exe".to_string(),
            ],
            "a path with spaces, a drive root, and %~dp0 written in capitals"
        );
        // A redirection is not part of the directory, and what follows `&&`
        // is a command of its own.
        assert_eq!(
            missing_paths(
                "cd /d \"%~dp0\" >nul 2>&1\r\ncd local && \"OpenAL\\oalinst.exe\"\r\n",
                Path::new(r"E:\LAN\g"),
                &env,
                &nothing,
            ),
            vec![
                r"E:\LAN\g".to_string(),
                r"E:\LAN\g\local".to_string(),
                r"E:\LAN\g\local\OpenAL\oalinst.exe".to_string(),
            ]
        );
        let missing = missing_paths(
            "cd /d \"%~dp0\"\r\ncd local\r\n\"x.exe\"\r\n",
            Path::new(r"E:\LAN\g"),
            &env,
            &nothing,
        );
        assert_eq!(
            missing,
            vec![
                r"E:\LAN\g".to_string(),
                r"E:\LAN\g\local".to_string(),
                r"E:\LAN\g\local\x.exe".to_string(),
            ]
        );
        // Nothing to say about a script whose paths are all there …
        assert!(missing_paths(script, Path::new(r"E:\LAN\opencnc"), &env, &|_| true).is_empty());
        // … and a variable nobody can resolve makes the trail stop rather
        // than a wrong path be named.
        assert_eq!(
            missing_paths(
                "cd /d \"%confdir%\"\r\n\"tool.exe\"\r\n",
                Path::new(r"E:\LAN\g"),
                &|_| None,
                &nothing,
            ),
            Vec::<String>::new()
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
