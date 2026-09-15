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

/// Command line for `game_start.cmd "<game_path>" <id> <lang> "<player>"`.
/// We run through `cmd.exe /C` so `exit` inside the script ends only the
/// script's shell.
pub fn plan(ctx: &LaunchContext<'_>) -> Result<LaunchPlan> {
    if !ctx.paths.start_script.is_file() {
        return Err(Error::Launch(
            "game_start.cmd is missing in the game folder".into(),
        ));
    }
    let comspec =
        std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into());
    Ok(LaunchPlan {
        program: PathBuf::from(comspec),
        args: script_args(
            &ctx.paths.start_script,
            &ctx.paths.share_dir,
            ctx.game_id,
            &ctx.settings.game_language,
            &ctx.settings.safe_player_name(),
        ),
        cwd: ctx.paths.share_dir.clone(),
        env: BTreeMap::new(),
        runner: "game_start.cmd".into(),
        needs_elevation: true,
    })
}

pub fn script_args(
    script: &Path,
    share_dir: &Path,
    game_id: &str,
    lang: &str,
    player: &str,
) -> Vec<String> {
    vec![
        "/C".into(),
        format!("\"{}\"", script.display()),
        format!("\"{}\"", share_dir.display()),
        game_id.to_string(),
        lang.to_string(),
        format!("\"{player}\""),
    ]
}

/// Plan for the one-time `game_setup.cmd "<game_path>" <id>`.
pub fn setup_plan(paths: &crate::paths::GamePaths, game_id: &str) -> Option<LaunchPlan> {
    if !paths.setup_script.is_file() {
        return None;
    }
    let comspec =
        std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into());
    Some(LaunchPlan {
        program: PathBuf::from(comspec),
        args: vec![
            "/C".into(),
            format!("\"{}\"", paths.setup_script.display()),
            format!("\"{}\"", paths.share_dir.display()),
            game_id.to_string(),
        ],
        cwd: paths.share_dir.clone(),
        env: BTreeMap::new(),
        runner: "game_setup.cmd".into(),
        needs_elevation: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_arguments_follow_eti_contract() {
        let args = script_args(
            Path::new(r"D:\LAN\quake3\game_start.cmd"),
            Path::new(r"D:\LAN\quake3"),
            "quake3",
            "de",
            "Player One",
        );
        assert_eq!(args[1], r#""D:\LAN\quake3\game_start.cmd""#);
        assert_eq!(args[2], r#""D:\LAN\quake3""#);
        assert_eq!(args[3], "quake3");
        assert_eq!(args[4], "de");
        assert_eq!(args[5], r#""Player One""#);
    }
}
