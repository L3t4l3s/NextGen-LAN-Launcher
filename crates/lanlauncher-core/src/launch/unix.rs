//! macOS / Linux: run Windows game executables through a compatibility layer.

use super::{expand_args, resolve_exe, LaunchContext, LaunchPlan};
use crate::error::{Error, Result};
use crate::manifest::Runner;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedRunner {
    pub runner: Runner,
    pub program: PathBuf,
    pub label: String,
    /// For Proton: the Steam installation it belongs to, which it needs as
    /// `STEAM_COMPAT_CLIENT_INSTALL_PATH`. `None` for a path from the
    /// settings and for everything that is not Proton.
    pub steam_root: Option<PathBuf>,
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// Find available runners, best first.
pub fn detect_runners(settings: &crate::settings::Settings) -> Vec<DetectedRunner> {
    let mut out = Vec::new();
    let rp = &settings.runner_paths;
    if cfg!(target_os = "macos") {
        let candidates = [
            rp.crossover_app.clone(),
            Some(PathBuf::from("/Applications/CrossOver.app")),
            dirs_home().map(|h| h.join("Applications/CrossOver.app")),
        ];
        for app in candidates.into_iter().flatten() {
            let wine = app.join("Contents/SharedSupport/CrossOver/bin/wine");
            if wine.is_file() {
                out.push(DetectedRunner {
                    runner: Runner::Crossover,
                    program: wine,
                    label: "CrossOver".into(),
                    steam_root: None,
                });
                break;
            }
        }
    }
    // A path from the settings is a decision and comes before everything
    // that was merely found.
    let configured_proton = rp.proton.clone().filter(|p| p.is_file());
    if let Some(p) = configured_proton.clone() {
        out.push(DetectedRunner {
            runner: Runner::Proton,
            program: p,
            label: "Proton".into(),
            steam_root: None,
        });
    }
    if let Some(w) = rp
        .wine
        .clone()
        .filter(|p| p.is_file())
        .or_else(|| which("wine"))
        .or_else(|| which("wine64"))
    {
        out.push(DetectedRunner {
            runner: Runner::Wine,
            program: w,
            label: "Wine".into(),
            steam_root: None,
        });
    }
    // Proton is not on `PATH` and lives inside a Steam library, so it has to
    // be searched for. Without this a Steam Deck with Proton installed
    // reported "no Wine, CrossOver or Proton found": only a path from the
    // settings was ever considered.
    //
    // Behind Wine on purpose. A desktop with Steam installed has a Proton
    // whether or not anybody meant to use it for this, and `Runner::Auto`
    // taking it over a Wine that is on `PATH` would change what every such
    // machine runs. A Steam Deck has no `wine`, so it still lands here.
    if configured_proton.is_none() {
        if let Some(home) = dirs_home() {
            for found in super::proton::find_protons(&home) {
                out.push(DetectedRunner {
                    runner: Runner::Proton,
                    program: found.proton,
                    label: found.label,
                    steam_root: Some(found.steam_root),
                });
            }
        }
    }
    out
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub fn plan(ctx: &LaunchContext<'_>) -> Result<LaunchPlan> {
    let (exe, args, cwd, wanted) = resolve_exe(ctx)?;
    if !exe.exists() {
        return Err(Error::Launch(format!(
            "executable not found: {}",
            exe.display()
        )));
    }
    let args = expand_args(&args, ctx);
    let mut env: BTreeMap<String, String> = ctx
        .manifest
        .map(|m| {
            m.launch_for(crate::manifest::Manifest::current_platform())
                .env
        })
        .unwrap_or_default();
    let is_windows_exe = exe
        .extension()
        .map(|e| e.eq_ignore_ascii_case("exe"))
        .unwrap_or(false);

    if wanted == Runner::Native || !is_windows_exe {
        if cfg!(target_os = "macos") && exe.extension().map(|e| e == "app").unwrap_or(false) {
            let mut a = vec!["-a".to_string(), exe.to_string_lossy().to_string()];
            if !args.is_empty() {
                a.push("--args".into());
                a.extend(args);
            }
            return Ok(LaunchPlan {
                program: PathBuf::from("/usr/bin/open"),
                args: a,
                cwd,
                env,
                runner: "native (.app)".into(),
                needs_elevation: false,
                raw_command_line: None,
            });
        }
        return Ok(LaunchPlan {
            program: exe,
            args,
            cwd,
            env,
            runner: "native".into(),
            needs_elevation: false,
            raw_command_line: None,
        });
    }

    let runners = detect_runners(ctx.settings);
    let chosen = match wanted {
        Runner::Auto => runners.first().cloned(),
        r => runners
            .iter()
            .find(|d| d.runner == r)
            .cloned()
            .or_else(|| runners.first().cloned()),
    }
    .ok_or_else(|| {
        // Naming the places searched turns an unanswerable report into one
        // line of evidence, as `resilio::locate_binary_detailed` does.
        let probed = dirs_home()
            .map(|h| super::proton::probed_paths(&h))
            .unwrap_or_default()
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        log::warn!("no runner found; looked for Proton in [{probed}] and for wine on PATH");
        Error::Launch(format!(
            "no Wine, CrossOver or Proton found (looked for Proton in {probed})"
        ))
    })?;

    // One prefix/bottle per game keeps registry tweaks isolated.
    let prefix_dir = ctx.paths.share_dir.join(".nll-prefix");
    match chosen.runner {
        Runner::Crossover => {
            let bottle = format!("nll-{}", ctx.game_id);
            let mut a = vec![
                "--bottle".to_string(),
                bottle,
                "--no-convert".to_string(),
                "--workdir".to_string(),
                cwd.to_string_lossy().to_string(),
                "--".to_string(),
                exe.to_string_lossy().to_string(),
            ];
            a.extend(args);
            Ok(LaunchPlan {
                program: chosen.program,
                args: a,
                cwd,
                env,
                runner: "CrossOver".into(),
                needs_elevation: false,
                raw_command_line: None,
            })
        }
        Runner::Proton => {
            env.entry("STEAM_COMPAT_DATA_PATH".into())
                .or_insert(prefix_dir.to_string_lossy().to_string());
            // The Steam that owns this Proton, not a guess: a Flatpak Steam
            // or a second installation lives nowhere near `~/.steam/steam`,
            // and Proton refuses to start when this points at the wrong one.
            env.entry("STEAM_COMPAT_CLIENT_INSTALL_PATH".into())
                .or_insert(
                    chosen
                        .steam_root
                        .clone()
                        .or_else(|| dirs_home().map(|h| h.join(".steam/steam")))
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default(),
                );
            let mut a = vec!["run".to_string(), exe.to_string_lossy().to_string()];
            a.extend(args);
            Ok(LaunchPlan {
                program: chosen.program,
                args: a,
                cwd,
                env,
                runner: "Proton".into(),
                needs_elevation: false,
                raw_command_line: None,
            })
        }
        _ => {
            env.entry("WINEPREFIX".into())
                .or_insert(prefix_dir.to_string_lossy().to_string());
            env.entry("WINEDEBUG".into()).or_insert("-all".into());
            let mut a = vec![exe.to_string_lossy().to_string()];
            a.extend(args);
            Ok(LaunchPlan {
                program: chosen.program,
                args: a,
                cwd,
                env,
                runner: "Wine".into(),
                needs_elevation: false,
                raw_command_line: None,
            })
        }
    }
}

pub fn prefix_dir(share_dir: &Path) -> PathBuf {
    share_dir.join(".nll-prefix")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{LaunchSpec, Manifest};
    use crate::paths::GamePaths;
    use crate::settings::Settings;

    /// The Steam Deck's report: Proton installed, nothing configured, and the
    /// launcher still said "no Wine, CrossOver or Proton found". The plan has
    /// to come out with the Proton that was found and with the two variables
    /// Proton refuses to start without — the client path taken from the Steam
    /// the Proton belongs to, not from a guess at `~/.steam/steam`.
    #[test]
    fn a_proton_nobody_configured_is_found_and_carries_its_steam_root() {
        let tmp = tempfile::tempdir().expect("tmp");
        let home = tmp.path().join("home");
        let steam = home.join(".local/share/Steam");
        let proton = steam.join("steamapps/common/Proton 9.0/proton");
        std::fs::create_dir_all(proton.parent().expect("parent")).expect("dirs");
        std::fs::write(&proton, "#!/bin/sh\n").expect("proton");

        let found = crate::launch::proton::find_protons(&home);
        assert_eq!(
            found.len(),
            1,
            "the search has to find it without being told"
        );
        assert_eq!(found[0].steam_root, steam);

        // What `plan` would then build out of it, without touching $HOME.
        let paths = GamePaths::new(tmp.path(), "g");
        let prefix = prefix_dir(&paths.share_dir);
        let mut env: BTreeMap<String, String> = BTreeMap::new();
        env.insert(
            "STEAM_COMPAT_DATA_PATH".into(),
            prefix.to_string_lossy().to_string(),
        );
        env.insert(
            "STEAM_COMPAT_CLIENT_INSTALL_PATH".into(),
            found[0].steam_root.to_string_lossy().to_string(),
        );
        assert!(env["STEAM_COMPAT_CLIENT_INSTALL_PATH"].ends_with(".local/share/Steam"));
        assert!(env["STEAM_COMPAT_DATA_PATH"].ends_with(".nll-prefix"));
    }

    #[test]
    fn native_and_wine_plans() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "g");
        std::fs::create_dir_all(paths.local_dir.join("bin")).unwrap();
        std::fs::write(paths.local_dir.join("bin/game.exe"), "").unwrap();
        std::fs::write(paths.local_dir.join("run.sh"), "").unwrap();
        let mut settings = Settings {
            player_name: "Neo".into(),
            ..Default::default()
        };
        // fake wine binary
        let wine = tmp.path().join("wine");
        std::fs::write(&wine, "").unwrap();
        settings.runner_paths.wine = Some(wine.clone());

        let m = Manifest {
            id: "g".into(),
            launch: LaunchSpec {
                exe: "bin/game.exe".into(),
                args: vec!["-name".into(), "%player%".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        let ctx = LaunchContext {
            paths: &paths,
            game_id: "g",
            settings: &settings,
            manifest: Some(&m),
            receipt: None,
            alternative: None,
        };
        let p = plan(&ctx).unwrap();
        assert_eq!(p.program, wine);
        assert_eq!(p.args[1..], ["-name", "Neo"]);
        assert_eq!(p.cwd, paths.local_dir.join("bin"));
        assert!(p.env["WINEPREFIX"].ends_with(".nll-prefix"));

        let native = Manifest {
            id: "g".into(),
            launch: LaunchSpec {
                exe: "run.sh".into(),
                runner: Runner::Native,
                ..Default::default()
            },
            ..Default::default()
        };
        let ctx = LaunchContext {
            manifest: Some(&native),
            ..ctx
        };
        let p = plan(&ctx).unwrap();
        assert_eq!(p.program, paths.local_dir.join("run.sh"));
        assert_eq!(p.runner, "native");
    }

    #[test]
    fn missing_exe_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "g");
        let settings = Settings::default();
        let ctx = LaunchContext {
            paths: &paths,
            game_id: "g",
            settings: &settings,
            manifest: None,
            receipt: None,
            alternative: None,
        };
        assert!(plan(&ctx).is_err());
    }
}
