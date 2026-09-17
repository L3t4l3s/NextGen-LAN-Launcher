//! On-demand elevation on Windows.
//!
//! The launcher itself runs as the invoking user (manifest `asInvoker`).
//! Only what really needs administrator rights is run through the Windows
//! "run as administrator" path (a UAC prompt): the one-time `game_setup.cmd`,
//! the firewall rules the start scripts would add, the few start scripts
//! that write to HKLM, and the repair actions of the diagnostics page.
//!
//! Elevated runs go through a small batch file the launcher writes, started
//! with PowerShell `Start-Process -Verb RunAs`. The PowerShell script is
//! passed `-EncodedCommand`, so no quoting layer between Rust, PowerShell and
//! cmd.exe can break a path with spaces.

use super::LaunchPlan;
use crate::error::{Error, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// `CREATE_NO_WINDOW`: helper processes must not flash a console window from
/// the GUI process. Games and their scripts are started *without* it, they
/// need their console.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Hide the console of a helper process (no-op off Windows).
pub fn hide_window(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    // `tokio::process::Command` has its own `creation_flags` on Windows, so
    // the extension trait of the standard library must not be imported here.
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Hide the console of a synchronous helper process (no-op off Windows).
pub fn hide_window_std(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Whether this process has administrator rights. `None` off Windows, where
/// nothing here applies. Cached: it costs one `net session` call.
pub fn running_elevated() -> Option<bool> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    static CACHE: OnceLock<bool> = OnceLock::new();
    Some(*CACHE.get_or_init(|| {
        let mut cmd = std::process::Command::new("net");
        hide_window_std(&mut cmd)
            .arg("session")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }))
}

/// Does a script need administrator rights beyond the firewall rules the
/// launcher registers itself? Registry writes below HKLM, `.reg` imports,
/// service and boot configuration, ownership changes. `netsh advfirewall`
/// alone does not count: those rules are added once at setup time.
pub fn script_needs_admin(text: &str) -> bool {
    text.lines()
        .map(|l| l.trim_start().to_ascii_lowercase())
        .filter(|l| !l.starts_with("rem ") && !l.starts_with("::"))
        .map(|l| l.trim_start_matches('@').to_string())
        .any(|l| {
            let copies_into_system = (l.contains("%programfiles")
                || l.contains("%windir%")
                || l.contains("%systemroot%"))
                && ["copy", "xcopy", "robocopy", "del", "mkdir", "md "]
                    .iter()
                    .any(|c| l.starts_with(c));
            l.contains("hklm") && l.contains("reg ")
                || l.contains("hkey_local_machine")
                || l.contains("regedit")
                || l.contains("reg import")
                || l.contains("regsvr32")
                || l.starts_with("sc ")
                || l.starts_with("bcdedit")
                || l.starts_with("takeown")
                || l.starts_with("icacls")
                || l.contains("netsh advfirewall set")
                || l.contains("netsh interface")
                || l.contains("netsh int ")
                || l.contains("netsh winsock")
                || copies_into_system
        })
}

/// Marker that the firewall rules of a game's start script were registered
/// (in the launcher's data dir, independent of the receipt the install
/// state machine writes).
pub fn firewall_marker(data_dir: &Path, game_id: &str) -> PathBuf {
    data_dir.join("firewall").join(format!("{game_id}.done"))
}

/// Replace the variables ETI scripts set from their four arguments
/// (`%game_path%`, `%game_id%`, `%game_lang%`, `%player%`) and `%~dp0`,
/// the script's own folder with a trailing backslash.
pub fn expand_script_vars(
    line: &str,
    share_dir: &Path,
    game_id: &str,
    lang: &str,
    player: &str,
) -> String {
    let mut out = line.to_string();
    let dp0 = format!(
        "{}\\",
        share_dir.to_string_lossy().trim_end_matches(['\\', '/'])
    );
    for (var, value) in [
        ("%~dp0", dp0.as_str()),
        ("%game_path%", share_dir.to_string_lossy().as_ref()),
        ("%game_id%", game_id),
        ("%game_lang%", lang),
        ("%player%", player),
    ] {
        // Batch variables are case-insensitive.
        let lower = out.to_ascii_lowercase();
        let mut result = String::with_capacity(out.len());
        let mut rest = 0;
        for (idx, _) in lower.match_indices(var) {
            result.push_str(&out[rest..idx]);
            result.push_str(value);
            rest = idx + var.len();
        }
        result.push_str(&out[rest..]);
        out = result;
    }
    out
}

/// The `netsh advfirewall firewall add rule …` commands of a start script,
/// variables expanded and output redirects removed, so the launcher can add
/// the rules once (elevated) and the script may run as a normal user.
pub fn firewall_add_rules(
    script: &str,
    share_dir: &Path,
    game_id: &str,
    lang: &str,
    player: &str,
) -> Vec<String> {
    script
        .lines()
        .map(|l| l.trim().trim_start_matches('@'))
        .filter(|l| {
            let lower = l.to_ascii_lowercase();
            lower.starts_with("netsh advfirewall firewall add rule")
        })
        .map(|l| {
            let cut = l.find(" >").or_else(|| l.find(" 2>")).unwrap_or(l.len());
            expand_script_vars(l[..cut].trim_end(), share_dir, game_id, lang, player)
        })
        .collect()
}

/// One line of an elevated batch: whether its failure counts, and whether its
/// output goes into the log or stays on the elevated console.
///
/// Both matter in the same batch. `netsh advfirewall firewall delete rule`
/// exits 1 when no rule matched, which is the normal case on a clean machine,
/// so a run must not fail over it. A game's `game_setup.cmd` on the other
/// hand prints instructions and may wait for a key press, so its output has
/// to stay where the user can see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchLine {
    pub command: String,
    /// `false`: run it, log it, but let it fail without failing the run.
    pub required: bool,
    /// `false`: leave the output on the elevated console instead of the log.
    pub capture: bool,
}

impl BatchLine {
    /// A command that may fail; its failure is not the run's.
    pub fn optional(command: impl Into<String>) -> Self {
        Self {
            capture: true,
            required: false,
            command: command.into(),
        }
    }

    /// A command the user has to watch: its output stays on the console.
    pub fn console(command: impl Into<String>) -> Self {
        Self {
            capture: false,
            required: true,
            command: command.into(),
        }
    }
}

impl<T: Into<String>> From<T> for BatchLine {
    fn from(command: T) -> Self {
        Self {
            command: command.into(),
            required: true,
            capture: true,
        }
    }
}

/// A batch file for one elevated run: the lines, then the exit code of the
/// last command. Written into the launcher's data dir with a plain name so
/// the cmd.exe command line needs nothing but the quoted path. Names are
/// fixed per purpose and overwritten, so nothing accumulates. The file is
/// user-writable and then read by an administrator cmd.exe; it is written
/// immediately before the run and its content is the user's own request, so
/// this is the same trust the user grants any script started from a UAC
/// prompt.
/// Where [`write_batch_logged`] writes the transcript of `stem`. The name is
/// derived, so a caller that wants to read it afterwards asks here rather
/// than building the same string a second time.
pub fn batch_log_path(dir: &Path, stem: &str) -> PathBuf {
    dir.join(format!("{}.log", safe_stem(stem)))
}

pub fn write_batch(dir: &Path, stem: &str, lines: &[BatchLine]) -> Result<PathBuf> {
    write_batch_inner(dir, stem, lines, None).map(|(batch, _)| batch)
}

/// As [`write_batch`], but every line also appends its output to a log file
/// next to the batch, and the returned path points at that log.
///
/// The elevated process gets its own console, so whatever `netsh` or
/// PowerShell prints there is lost to the launcher: a failure arrives as a
/// bare exit code with nothing to act on. Reading the log afterwards is what
/// turns that into a message a player, or a domain admin, can work with.
pub fn write_batch_logged(
    dir: &Path,
    stem: &str,
    lines: &[BatchLine],
) -> Result<(PathBuf, PathBuf)> {
    write_batch_logged_to(dir, stem, lines, None)
}

/// As [`write_batch_logged`], with the transcript file named by the caller —
/// the launcher shows one file per game and start kind on the diagnostics
/// page and wants an elevated run in the same place as an ordinary one.
pub fn write_batch_logged_to(
    dir: &Path,
    stem: &str,
    lines: &[BatchLine],
    log: Option<&Path>,
) -> Result<(PathBuf, PathBuf)> {
    let log = log
        .map(PathBuf::from)
        .unwrap_or_else(|| batch_log_path(dir, stem));
    // Left over from the previous run; the batch only appends.
    let _ = std::fs::remove_file(&log);
    write_batch_inner(dir, stem, lines, Some(&log))
}

fn safe_stem(stem: &str) -> String {
    stem.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn write_batch_inner(
    dir: &Path,
    stem: &str,
    lines: &[BatchLine],
    log: Option<&Path>,
) -> Result<(PathBuf, PathBuf)> {
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let path = dir.join(format!("{}.cmd", safe_stem(stem)));
    let mut text = String::from("@echo off\r\n");
    // `>>"<log>" 2>&1` per line rather than one block around all of them:
    // cmd's parenthesised blocks trip over the parentheses in rule names
    // such as "NextGen LAN Launcher Sync (in)".
    let redirect = log
        .map(|l| format!(" >>\"{}\" 2>&1", l.display()))
        .unwrap_or_default();
    if let Some(l) = log {
        // The batch truncates the log itself: deleting it beforehand is best
        // effort, and a leftover from the previous run would put that run's
        // failure into this run's error message.
        text.push_str(&format!("type nul >\"{}\"\r\n", l.display()));
    }
    // Every line runs — the game's setup must not be skipped because a
    // firewall rule before it failed — but the batch reports the *first*
    // failure instead of only the last line's exit code. Four netsh rules
    // where the first one fails used to come back as success.
    text.push_str("set \"NLL_RC=0\"\r\n");
    for (i, l) in lines.iter().enumerate() {
        // Our own markers always go to the log — only the command's output
        // stays on the console for a console line. Without its marker the
        // reader would quote the last *captured* command, which succeeded.
        if log.is_some() {
            // Numbered so the log says which line produced which message.
            text.push_str(&format!("echo --- {}{redirect}\r\n", i + 1));
        }
        text.push_str(&l.command);
        if l.capture {
            text.push_str(&redirect);
        }
        text.push_str("\r\n");
        if !l.required {
            continue;
        }
        text.push_str("set \"NLL_LAST=%errorlevel%\"\r\n");
        if log.is_some() {
            // Marks this command's own section as the failing one.
            text.push_str(&format!(
                "if not \"%NLL_LAST%\"==\"0\" echo {FAILURE_MARKER} %NLL_LAST%{redirect}\r\n"
            ));
        }
        text.push_str(
            "if not \"%NLL_LAST%\"==\"0\" if \"%NLL_RC%\"==\"0\" set \"NLL_RC=%NLL_LAST%\"\r\n",
        );
    }
    text.push_str("exit /b %NLL_RC%\r\n");
    std::fs::write(&path, text).map_err(|e| Error::io(&path, e))?;
    Ok((path, log.map(Path::to_path_buf).unwrap_or_default()))
}

/// What the batch echoes into its log for a command that failed.
const FAILURE_MARKER: &str = "!! exit";

/// Which line of the batch failed, counted from one, where the transcript
/// says so.
///
/// Every line writes its number before it runs and a failure marker after it,
/// so the first marked section names the command whose exit code the batch
/// returned. A caller that knows what it put in which line can tell whose
/// failure it is looking at.
pub fn failed_line(transcript: &str) -> Option<usize> {
    transcript
        .split("--- ")
        .skip(1)
        .find(|s| s.contains(FAILURE_MARKER))
        .and_then(|s| s.lines().next())
        .and_then(|l| l.trim().parse().ok())
}

/// The output of the command that failed in a transcript
/// [`write_batch_logged`] produced, joined into one line.
///
/// The batch reports the first failing command's exit code, so that command's
/// output is the one that explains it; a later command that succeeded would
/// name the wrong culprit. A command whose output stayed on the console has
/// an empty section, which correctly yields no quote at all. Without any
/// failure marker (a truncated log, or a run that failed before the first
/// command) the last section is the best guess left.
pub fn last_section(transcript: &str) -> String {
    let sections: Vec<&str> = transcript.split("--- ").skip(1).collect();
    let picked = sections
        .iter()
        .find(|s| s.contains(FAILURE_MARKER))
        .or_else(|| sections.last())
        .copied();
    // A section starts with the number the batch echoed; a transcript
    // without any marker (truncated, or cut off before the first command)
    // has no such line to drop.
    let (body, skip_number) = match picked {
        Some(s) => (s, 1),
        None => (transcript, 0),
    };
    body.lines()
        .map(str::trim)
        .skip(skip_number)
        .filter(|l| !l.is_empty() && !l.starts_with(FAILURE_MARKER))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The batch line that runs a plan: for cmd.exe script plans the command
/// line inside `/S /C "…"` behind `call`, for programs the quoted path plus
/// its arguments.
pub fn batch_line(plan: &LaunchPlan) -> String {
    match &plan.raw_command_line {
        Some(raw) => {
            let inner = raw.trim_start().trim_start_matches("/S /C").trim();
            let inner = inner
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(inner);
            format!("call {inner}")
        }
        None => {
            let mut line = format!("\"{}\"", plan.program.display());
            for a in &plan.args {
                line.push(' ');
                if a.contains(' ') {
                    line.push_str(&format!("\"{a}\""));
                } else {
                    line.push_str(a);
                }
            }
            line
        }
    }
}

/// PowerShell that runs a batch file as administrator, waits and passes its
/// exit code on. The batch file is the `FilePath` itself: handing `cmd.exe` a
/// quoted command line through `-ArgumentList` adds a quoting layer that
/// PowerShell rewrites, which can make the elevated run fail before the user
/// has even confirmed the prompt. Single quotes are PowerShell's literal
/// strings, so a quote inside a path is doubled.
pub fn runas_script(batch: &Path, cwd: &Path) -> String {
    let q = |p: &Path| p.to_string_lossy().replace('\'', "''");
    format!(
        "$ErrorActionPreference = 'Stop'; $p = Start-Process -FilePath '{}' -WorkingDirectory '{}' -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
        q(batch),
        q(cwd)
    )
}

/// `-EncodedCommand` form: base64 of the UTF-16LE script.
pub fn powershell_encoded(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Plan that runs `batch` as administrator through PowerShell (UAC prompt).
pub fn runas_plan(batch: &Path, cwd: &Path) -> LaunchPlan {
    LaunchPlan {
        program: PathBuf::from("powershell.exe"),
        args: vec![
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-EncodedCommand".into(),
            powershell_encoded(&runas_script(batch, cwd)),
        ],
        cwd: cwd.to_path_buf(),
        env: BTreeMap::new(),
        runner: "runas".into(),
        needs_elevation: false,
        raw_command_line: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marked_command_is_the_one_quoted() {
        // The first rule failed, a later one succeeded: quoting the last
        // section would report "Ok." for a run that failed.
        let transcript =
            "--- 1\r\nDer Parameter ist ung\u{fc}ltig.\r\n!! exit 1\r\n--- 2\r\nOk.\r\n";
        assert_eq!(last_section(transcript), "Der Parameter ist ung\u{fc}ltig.");
        // Without a marker the last section is the best guess left.
        assert_eq!(
            last_section("--- 1\r\nOk.\r\n\r\n--- 2\r\nWeg.\r\n"),
            "Weg."
        );
        // A run whose command said nothing leaves no reason to quote.
        assert_eq!(last_section("--- 1\r\nOk.\r\n\r\n--- 2\r\n"), "");
        assert_eq!(last_section(""), "");
        // A truncated transcript (the tail of a long log) has no marker left.
        assert_eq!(
            last_section("Zugriff verweigert.\r\n"),
            "Zugriff verweigert."
        );

        // A console line that failed has an empty section: no quote is
        // better than quoting the command before it, which worked.
        assert_eq!(last_section("--- 1\r\nOk.\r\n--- 2\r\n!! exit 2\r\n"), "");
    }

    #[test]
    fn the_failing_line_is_known_by_number() {
        // Which line failed decides whether a caller may explain the failure
        // with what it put in that line.
        assert_eq!(
            failed_line("--- 1\r\nOk.\r\n--- 2\r\nWeg.\r\n!! exit 1\r\n"),
            Some(2)
        );
        // The first failure is the one the batch returns.
        assert_eq!(
            failed_line("--- 1\r\n!! exit 1\r\n--- 2\r\n!! exit 5\r\n"),
            Some(1)
        );
        // Nothing failed, or nothing survived of the transcript: no claim.
        assert_eq!(failed_line("--- 1\r\nOk.\r\n"), None);
        assert_eq!(failed_line("Zugriff verweigert.\r\n"), None);
        assert_eq!(failed_line(""), None);
    }

    #[test]
    fn detects_admin_needs_but_not_firewall_rules() {
        let firewall_only = "netsh advfirewall firewall add rule name=\"%game_id%\" dir=in action=allow program=\"%game_path%\\local\\x.exe\" profile=any enable=yes >nul\r\n\"x.exe\"\r\nnetsh advfirewall firewall delete rule name=\"%game_id%\" >nul";
        assert!(!script_needs_admin(firewall_only));
        assert!(script_needs_admin(
            "reg add \"HKLM\\SOFTWARE\\X\" /v a /d 1 /f"
        ));
        assert!(script_needs_admin(
            "REG ADD HKEY_LOCAL_MACHINE\\SOFTWARE\\X /f"
        ));
        assert!(script_needs_admin("regedit /s \"install.reg\""));
        assert!(!script_needs_admin(
            "rem reg add HKLM\\x\r\nreg add HKCU\\Software\\X /f"
        ));
        assert!(script_needs_admin("@reg import \"%~dp0install.reg\""));
        assert!(script_needs_admin("regsvr32 /s x.dll"));
        assert!(script_needs_admin(
            "copy /y x.dll \"%programfiles%\\Game\\\""
        ));
        assert!(!script_needs_admin(
            "copy /y x.cfg \"%game_path%\\local\\\""
        ));
    }

    #[test]
    fn extracts_and_expands_firewall_rules() {
        let script = "set game_path=%1\r\nnetsh advfirewall firewall add rule name=\"%game_id%\" dir=in action=allow program=\"%game_path%\\local\\hl2.exe\" profile=any enable=yes >nul\r\n@netsh advfirewall firewall add rule name=\"%game_id%\" dir=in action=allow program=\"%~dp0local\\srcds.exe\" profile=any enable=yes 2>nul\r\n\"hl2.exe\"\r\nnetsh advfirewall firewall delete rule name=\"%game_id%\" >nul\r\n";
        let rules = firewall_add_rules(
            script,
            Path::new(r"D:\LAN Party\goldsrc"),
            "goldsrc",
            "de",
            "P",
        );
        assert_eq!(
            rules,
            vec![
                r#"netsh advfirewall firewall add rule name="goldsrc" dir=in action=allow program="D:\LAN Party\goldsrc\local\hl2.exe" profile=any enable=yes"#,
                r#"netsh advfirewall firewall add rule name="goldsrc" dir=in action=allow program="D:\LAN Party\goldsrc\local\srcds.exe" profile=any enable=yes"#,
            ]
        );
        assert_eq!(
            firewall_marker(Path::new("/d"), "goldsrc"),
            PathBuf::from("/d").join("firewall").join("goldsrc.done")
        );
        assert_eq!(
            expand_script_vars(
                "%GAME_PATH%|%player%",
                Path::new("C:\\g"),
                "id",
                "de",
                "Max"
            ),
            "C:\\g|Max"
        );
    }

    #[test]
    fn batch_and_runas_are_quoted_for_cmd_and_powershell() {
        let plan = LaunchPlan {
            program: PathBuf::from(r"C:\Windows\System32\cmd.exe"),
            args: vec![],
            cwd: PathBuf::from(r"D:\LAN\q3"),
            env: BTreeMap::new(),
            runner: "game_setup.cmd".into(),
            needs_elevation: true,
            raw_command_line: Some(
                r#"/S /C ""D:\LAN\q3\game_setup.cmd" "D:\LAN\q3" q3 de "Player One"""#.into(),
            ),
        };
        assert_eq!(
            batch_line(&plan),
            r#"call "D:\LAN\q3\game_setup.cmd" "D:\LAN\q3" q3 de "Player One""#
        );
        let exe = LaunchPlan {
            program: PathBuf::from(r"D:\LAN\q3\keygen.exe"),
            args: vec!["a b".into(), "c".into()],
            raw_command_line: None,
            ..plan.clone()
        };
        assert_eq!(batch_line(&exe), r#""D:\LAN\q3\keygen.exe" "a b" c"#);

        let dir = tempfile::tempdir().unwrap();
        let batch = write_batch(dir.path(), "q3 setup/1", &[batch_line(&plan).into()]).unwrap();
        assert!(batch.ends_with("q3_setup_1.cmd"));
        let text = std::fs::read_to_string(&batch).unwrap();
        assert!(text.starts_with("@echo off\r\n"));
        assert!(text.ends_with("exit /b %NLL_RC%\r\n"));

        // With a log, every line appends to it and the stale log of the
        // previous run is gone before the batch runs.
        let stale = dir.path().join("firewall-rules.log");
        std::fs::write(&stale, "old run").unwrap();
        let (batch, log) = write_batch_logged(
            dir.path(),
            "firewall-rules",
            &[
                BatchLine::optional(r#"netsh advfirewall firewall delete rule name=all dir=in"#),
                r#"netsh advfirewall firewall add rule name="Sync (in)" dir=in"#.into(),
                BatchLine::console(r#"call "D:\LAN\q3\game_setup.cmd""#),
            ],
        )
        .unwrap();
        assert_eq!(log, stale);
        assert!(!log.exists());
        let text = std::fs::read_to_string(&batch).unwrap();
        assert!(text.contains(&format!("echo --- 1 >>\"{}\" 2>&1", log.display())));
        // The stale-rule delete runs and is logged, but netsh's "no rule
        // matched" (exit 1) must not fail the repair.
        assert_eq!(text.matches("set \"NLL_LAST=%errorlevel%\"").count(), 2);
        // The game's script keeps its console: no redirect, no log section,
        // but its exit code still counts.
        assert!(text.contains("call \"D:\\LAN\\q3\\game_setup.cmd\"\r\n"));
        // Its section header and failure marker still go to the log, so a
        // failing setup script is not reported with the previous command's
        // "Ok." — it simply has nothing to quote.
        assert!(text.contains(&format!("echo --- 3 >>\"{}\" 2>&1", log.display())));
        assert!(!text.contains(&format!("game_setup.cmd\" >>\"{}\"", log.display())));
        // The batch truncates the log itself; a leftover from the previous
        // run must not be read as this run's failure.
        assert!(text.contains(&format!("type nul >\"{}\"", log.display())));
        assert!(text.contains(&format!("dir=in >>\"{}\" 2>&1", log.display())));
        // Every line runs, and the exit code is the *first* failure: four
        // netsh rules whose first one fails must not report success.
        assert!(text.contains("set \"NLL_RC=0\""));
        assert!(
            text.contains(r#"if not "%NLL_LAST%"=="0" if "%NLL_RC%"=="0" set "NLL_RC=%NLL_LAST%""#)
        );
        assert!(text.contains(&format!(
            "echo !! exit %NLL_LAST% >>\"{}\" 2>&1",
            log.display()
        )));
        assert!(text.ends_with("exit /b %NLL_RC%\r\n"));

        let script = runas_script(
            Path::new(r"C:\Users\O'Neil\run\x.cmd"),
            Path::new(r"D:\LAN"),
        );
        // The batch runs directly: no cmd.exe, no nested quoting.
        assert!(script.contains(r"-FilePath 'C:\Users\O''Neil\run\x.cmd'"));
        assert!(!script.contains("cmd.exe"));
        assert!(script.contains("-Verb RunAs -Wait -PassThru"));
        // "ab" in UTF-16LE is 61 00 62 00.
        assert_eq!(powershell_encoded("ab"), "YQBiAA==");
        assert_eq!(runas_plan(&batch, dir.path()).args[4], "-EncodedCommand");
    }
}
