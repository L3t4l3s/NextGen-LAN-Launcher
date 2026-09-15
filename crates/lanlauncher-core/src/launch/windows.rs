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
    Ok(LaunchPlan {
        program: PathBuf::from(comspec()),
        args: vec!["/S".into(), "/C".into(), raw.clone()],
        cwd: ctx.paths.share_dir.clone(),
        env: BTreeMap::new(),
        runner: "game_start.cmd".into(),
        needs_elevation: true,
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

/// Plan for the one-time `game_setup.cmd "<game_path>" <id>`.
pub fn setup_plan(paths: &crate::paths::GamePaths, game_id: &str) -> Option<LaunchPlan> {
    if !paths.setup_script.is_file() {
        return None;
    }
    let raw = format!(
        "\"{} {} {}\"",
        quote(&paths.setup_script.to_string_lossy()),
        quote(&paths.share_dir.to_string_lossy()),
        game_id
    );
    Some(LaunchPlan {
        program: PathBuf::from(comspec()),
        args: vec!["/S".into(), "/C".into(), raw.clone()],
        cwd: paths.share_dir.clone(),
        env: BTreeMap::new(),
        runner: "game_setup.cmd".into(),
        needs_elevation: true,
        raw_command_line: Some(format!("/S /C {raw}")),
    })
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
}
