//! macOS/Linux: waiting in a script for a host program it started.
//!
//! The scripts' `fnr.exe` and `unrar.exe` become host scripts
//! (`launch::fnr`, `launch::unrar`) that Wine starts by their `Z:` path —
//! and does not wait for: the next line runs a few milliseconds later, and
//! when the script ends first the host program is cut off (measured with
//! Proton 11: Supraball's lobby client started while its own files were
//! still being unpacked, and quit). `start /wait /unix` does not wait
//! either.
//!
//! So every rewritten call carries a mark (`--nll-mark <id>`), and
//! `setup_script` chains `call "%NLL_WAIT%" <id>` behind it. The host script
//! writes `<id>.run` when it starts and `<id>.done` when it ends, in the
//! `marks` folder beside the wait script; the wait script polls for
//! `<id>.done`. When neither file is there after a second, the host program
//! did not start at all (no Perl, no launcher binary) and the script goes on
//! as it would have before.

use std::path::{Path, PathBuf};

/// The variable the chained calls name: the wait script's `Z:` path.
pub const VAR: &str = "NLL_WAIT";

/// The folder of marks, beside the wait script and one level above the host
/// scripts (`tools/marks`, `tools/fnr/…`, `tools/unrar/…`).
pub const MARKS: &str = "marks";

/// Waits up to five seconds for the host program to show up (`<id>.run` or
/// `<id>.done`), then until it is done, polling once a second (Wine's
/// `timeout`, which sleeps without a console key), for at most 30 minutes.
const WAIT_CMD: &str = "@echo off\r\n\
rem Written by NextGen LAN Launcher: waits for a host program a script started.\r\n\
setlocal\r\n\
set \"nll_mark=%~dp0marks\\%~1\"\r\n\
set /a nll_start=5\r\n\
set /a nll_left=1800\r\n\
:nll_start\r\n\
timeout /t 1 /nobreak >nul\r\n\
if exist \"%nll_mark%.run\" goto nll_wait\r\n\
if exist \"%nll_mark%.done\" goto nll_wait\r\n\
set /a nll_start-=1\r\n\
if %nll_start% gtr 0 goto nll_start\r\n\
exit /b 0\r\n\
:nll_wait\r\n\
if exist \"%nll_mark%.done\" (\r\n\
del \"%nll_mark%.done\" >nul 2>&1\r\n\
exit /b 0\r\n\
)\r\n\
set /a nll_left-=1\r\n\
if %nll_left% leq 0 exit /b 0\r\n\
timeout /t 1 /nobreak >nul\r\n\
goto nll_wait\r\n";

/// The lines a host script starts with: take the mark off the arguments and
/// say the program runs.
pub const SH_BEGIN: &str = "nll_mark=\n\
if [ \"$1\" = --nll-mark ]; then\n\
\x20 nll_mark=\"$(dirname \"$0\")/../marks/$2\"\n\
\x20 shift 2\n\
\x20 rm -f \"$nll_mark.done\"\n\
\x20 : > \"$nll_mark.run\"\n\
fi\n";

/// The lines it ends with, after the program, its exit code in `$nll_rc`.
pub const SH_END: &str = "if [ -n \"$nll_mark\" ]; then\n\
\x20 : > \"$nll_mark.done\"\n\
\x20 rm -f \"$nll_mark.run\"\n\
fi\n\
exit $nll_rc\n";

/// Writes `nll-wait.cmd` and the marks folder into `tools` (only when the
/// text changed: a running script may be reading it) and returns its path.
pub fn install(tools: &Path) -> std::io::Result<PathBuf> {
    let marks = tools.join(MARKS);
    std::fs::create_dir_all(&marks)?;
    // Marks of host programs that were cut off, or of waits that gave up:
    // older than the longest wait, they belong to no running script.
    let stale = std::time::Duration::from_secs(2 * 3600);
    for entry in std::fs::read_dir(&marks).into_iter().flatten().flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > stale);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let script = tools.join("nll-wait.cmd");
    super::setup_script::write_if_changed(&script, WAIT_CMD.as_bytes())?;
    Ok(script)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A host script built from the two halves, as fnr's and unrar's are.
    fn host_script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("tool").join("x.sh");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            format!("#!/bin/sh\n{SH_BEGIN}{body}\nnll_rc=$?\n{SH_END}"),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn a_marked_run_leaves_its_done_mark_and_keeps_the_exit_code() {
        let tmp = tempfile::tempdir().unwrap();
        install(tmp.path()).unwrap();
        let script = host_script(
            tmp.path(),
            "echo \"$@\" > \"$(dirname \"$0\")/args\"; false",
        );
        let status = std::process::Command::new(&script)
            .args(["--nll-mark", "ab12", "x", "-y"])
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(1));
        let marks = tmp.path().join(MARKS);
        assert!(marks.join("ab12.done").exists());
        assert!(!marks.join("ab12.run").exists());
        // The mark is not passed on.
        let args = std::fs::read_to_string(script.parent().unwrap().join("args")).unwrap();
        assert_eq!(args.trim(), "x -y");
        // Unmarked, as before.
        let status = std::process::Command::new(&script)
            .arg("x")
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(1));
        assert!(tmp.path().join("nll-wait.cmd").is_file());
    }
}
