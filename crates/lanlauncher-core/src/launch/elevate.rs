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

/// Whether this process has administrator rights. `None` off Windows, where
/// nothing here applies. Cached: it costs one `net session` call.
pub fn running_elevated() -> Option<bool> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    static CACHE: OnceLock<bool> = OnceLock::new();
    Some(*CACHE.get_or_init(|| {
        let mut cmd = std::process::Command::new("net");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console flash from a GUI process.
            cmd.creation_flags(0x0800_0000);
        }
        cmd.arg("session")
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

/// A batch file for one elevated run: the lines, then the exit code of the
/// last command. Written into the launcher's data dir with a plain name so
/// the cmd.exe command line needs nothing but the quoted path. Names are
/// fixed per purpose and overwritten, so nothing accumulates. The file is
/// user-writable and then read by an administrator cmd.exe; it is written
/// immediately before the run and its content is the user's own request, so
/// this is the same trust the user grants any script started from a UAC
/// prompt.
pub fn write_batch(dir: &Path, stem: &str, lines: &[String]) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let safe: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = dir.join(format!("{safe}.cmd"));
    let mut text = String::from("@echo off\r\n");
    for l in lines {
        text.push_str(l);
        text.push_str("\r\n");
    }
    text.push_str("exit /b %errorlevel%\r\n");
    std::fs::write(&path, text).map_err(|e| Error::io(&path, e))?;
    Ok(path)
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
/// exit code on. Single quotes are PowerShell's literal strings; a quote in
/// the path is doubled. cmd.exe gets `/S /C ""<batch>""` (Start-Process
/// passes the argument string verbatim, `/S` strips the outer quotes).
pub fn runas_script(batch: &Path, cwd: &Path) -> String {
    let q = |p: &Path| p.to_string_lossy().replace('\'', "''");
    format!(
        "$ErrorActionPreference = 'Stop'; $p = Start-Process -FilePath 'cmd.exe' -ArgumentList '/S /C \"\"{}\"\"' -WorkingDirectory '{}' -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
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
        let batch = write_batch(dir.path(), "q3 setup/1", &[batch_line(&plan)]).unwrap();
        assert!(batch.ends_with("q3_setup_1.cmd"));
        let text = std::fs::read_to_string(&batch).unwrap();
        assert!(text.starts_with("@echo off\r\n"));
        assert!(text.ends_with("exit /b %errorlevel%\r\n"));

        let script = runas_script(
            Path::new(r"C:\Users\O'Neil\run\x.cmd"),
            Path::new(r"D:\LAN"),
        );
        assert!(script.contains(r#"-ArgumentList '/S /C ""C:\Users\O''Neil\run\x.cmd""'"#));
        assert!(script.contains("-Verb RunAs -Wait -PassThru"));
        // "ab" in UTF-16LE is 61 00 62 00.
        assert_eq!(powershell_encoded("ab"), "YQBiAA==");
        assert_eq!(runas_plan(&batch, dir.path()).args[4], "-EncodedCommand");
    }
}
