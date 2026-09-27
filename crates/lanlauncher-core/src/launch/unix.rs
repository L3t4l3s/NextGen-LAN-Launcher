//! macOS / Linux: run Windows game executables through a compatibility layer.

use super::{expand_args, resolve_exe, LaunchContext, LaunchPlan};
use crate::error::{Error, Result};
use crate::manifest::Runner;
use crate::settings::GameRunner;
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

fn programs_on_path(name: &str) -> Vec<PathBuf> {
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .filter(|p| p.is_file())
        .collect()
}

/// Treat symlinked Steam roots and lexical aliases as the same installation.
pub fn same_program(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left == right
}

/// Restore a saved runner even if a later global-path change means automatic
/// detection no longer reaches it.
pub fn persisted_runner(saved: &GameRunner) -> Option<DetectedRunner> {
    if !saved.program.is_file() {
        return None;
    }
    Some(DetectedRunner {
        runner: saved.runner,
        program: saved.program.clone(),
        label: saved.label.clone(),
        steam_root: saved.steam_root.clone(),
    })
}

/// Find available runners, best first.
pub fn detect_runners(settings: &crate::settings::Settings) -> Vec<DetectedRunner> {
    let mut out: Vec<DetectedRunner> = Vec::new();
    let rp = &settings.runner_paths;
    if cfg!(target_os = "macos") {
        let candidates = [
            rp.crossover_app.clone(),
            Some(PathBuf::from("/Applications/CrossOver.app")),
            dirs_home().map(|h| h.join("Applications/CrossOver.app")),
        ];
        for app in candidates.into_iter().flatten() {
            let wine = app.join("Contents/SharedSupport/CrossOver/bin/wine");
            if wine.is_file()
                && !out.iter().any(|runner| {
                    runner.runner == Runner::Crossover && same_program(&runner.program, &wine)
                })
            {
                out.push(DetectedRunner {
                    runner: Runner::Crossover,
                    program: wine,
                    label: format!(
                        "{} ({})",
                        app.file_stem()
                            .and_then(|name| name.to_str())
                            .unwrap_or("CrossOver"),
                        app.display()
                    ),
                    steam_root: None,
                });
            }
        }
    }
    // A path from the settings is a decision and comes before everything
    // that was merely found.
    if let Some(p) = rp.proton.clone().filter(|p| p.is_file()) {
        out.push(DetectedRunner {
            runner: Runner::Proton,
            program: p,
            label: "Proton".into(),
            steam_root: None,
        });
    }
    // Keep every distinct Wine available. A newly configured global Wine
    // must not make a PATH Wine selected by an existing game disappear.
    for wine in rp
        .wine
        .iter()
        .cloned()
        .chain(programs_on_path("wine"))
        .chain(programs_on_path("wine64"))
    {
        if wine.is_file()
            && !out
                .iter()
                .any(|runner| runner.runner == Runner::Wine && same_program(&runner.program, &wine))
        {
            out.push(DetectedRunner {
                runner: Runner::Wine,
                label: format!("Wine ({})", wine.display()),
                program: wine,
                steam_root: None,
            });
        }
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
    if let Some(home) = dirs_home() {
        for found in super::proton::find_protons(&home) {
            if let Some(existing) = out.iter_mut().find(|runner| {
                runner.runner == Runner::Proton && same_program(&runner.program, &found.proton)
            }) {
                // A configured path still wins the ordering, but discovery
                // supplies the owning Steam root and its useful version name.
                existing.label = found.label;
                existing.steam_root = Some(found.steam_root);
            } else {
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
    let selected = ctx.settings.game_runner(ctx.game_id);
    let selected_program = selected.map(|runner| runner.program.as_path());
    let chosen = if let Some(selected) = selected {
        runners
            .iter()
            .find(|d| d.runner == selected.runner && same_program(&d.program, &selected.program))
            .cloned()
            .or_else(|| persisted_runner(selected))
    } else {
        match wanted {
            Runner::Auto => runners.first().cloned(),
            r => runners
                .iter()
                .find(|d| d.runner == r)
                .cloned()
                .or_else(|| runners.first().cloned()),
        }
    }
    .ok_or_else(|| {
        if let Some(program) = selected_program {
            return Error::Launch(format!(
                "selected compatibility tool is no longer available: {}",
                program.display()
            ));
        }
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

    // Automatic mode keeps the original prefix for backwards compatibility.
    // An explicitly selected tool gets a stable, separate prefix: switching
    // Proton versions must neither migrate nor overwrite another version's
    // registry and save data.
    let prefix_dir = prefix_dir_for(&ctx.paths.share_dir, selected_program);
    match chosen.runner {
        Runner::Crossover => {
            let bottle = selected_program
                .map(|program| format!("nll-{}-{}", ctx.game_id, runner_key(program)))
                .unwrap_or_else(|| format!("nll-{}", ctx.game_id));
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
                runner: chosen.label,
                needs_elevation: false,
                raw_command_line: None,
            })
        }
        Runner::Proton => {
            let prefix = prefix_dir.to_string_lossy().to_string();
            if selected_program.is_some() {
                env.insert("STEAM_COMPAT_DATA_PATH".into(), prefix);
            } else {
                env.entry("STEAM_COMPAT_DATA_PATH".into()).or_insert(prefix);
            }
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
                runner: chosen.label,
                needs_elevation: false,
                raw_command_line: None,
            })
        }
        _ => {
            let prefix = prefix_dir.to_string_lossy().to_string();
            if selected_program.is_some() {
                env.insert("WINEPREFIX".into(), prefix);
            } else {
                env.entry("WINEPREFIX".into()).or_insert(prefix);
            }
            env.entry("WINEDEBUG".into()).or_insert("-all".into());
            let mut a = vec![exe.to_string_lossy().to_string()];
            a.extend(args);
            Ok(LaunchPlan {
                program: chosen.program,
                args: a,
                cwd,
                env,
                runner: chosen.label,
                needs_elevation: false,
                raw_command_line: None,
            })
        }
    }
}

pub fn prefix_dir(share_dir: &Path) -> PathBuf {
    share_dir.join(".nll-prefix")
}

fn prefix_dir_for(share_dir: &Path, program: Option<&Path>) -> PathBuf {
    program
        .map(|path| share_dir.join(format!(".nll-prefix-{}", runner_key(path))))
        .unwrap_or_else(|| prefix_dir(share_dir))
}

/// Stable FNV-1a key; unlike `DefaultHasher`, this does not change between
/// Rust releases and therefore keeps pointing at the same prefix.
fn runner_key(program: &Path) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in program.as_os_str().to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
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
        let automatic_prefix = {
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
            p.env["WINEPREFIX"].clone()
        };

        settings.set_game_runner(
            "g",
            Some(crate::settings::GameRunner {
                program: wine.clone(),
                runner: Runner::Wine,
                label: format!("Wine ({})", wine.display()),
                steam_root: None,
            }),
        );
        let mut selected_manifest = m.clone();
        selected_manifest
            .launch
            .env
            .insert("WINEPREFIX".into(), "/manifest/prefix".into());
        let selected_ctx = LaunchContext {
            paths: &paths,
            game_id: "g",
            settings: &settings,
            manifest: Some(&selected_manifest),
            receipt: None,
            alternative: None,
        };
        let selected = plan(&selected_ctx).unwrap();
        assert_eq!(selected.program, wine);
        assert!(selected.env["WINEPREFIX"].contains(".nll-prefix-"));
        assert_ne!(selected.env["WINEPREFIX"], automatic_prefix);

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
            ..selected_ctx
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

    #[test]
    fn manually_selected_runners_have_stable_separate_prefixes() {
        let share = Path::new("/games/example");
        let first = Path::new("/tools/Proton 9/proton");
        let second = Path::new("/tools/Proton 10/proton");

        assert_eq!(prefix_dir_for(share, None), share.join(".nll-prefix"));
        assert_eq!(
            prefix_dir_for(share, Some(first)),
            prefix_dir_for(share, Some(first))
        );
        assert_ne!(
            prefix_dir_for(share, Some(first)),
            prefix_dir_for(share, Some(second))
        );
    }

    #[test]
    fn lexical_aliases_are_one_runner_and_one_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let program = tmp.path().join("wine");
        std::fs::write(&program, "").unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        let alias = tmp.path().join("sub").join("..").join("wine");

        assert!(same_program(&program, &alias));
    }

    #[test]
    fn persisted_runner_survives_a_global_path_change() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "g");
        std::fs::create_dir_all(&paths.local_dir).unwrap();
        std::fs::write(paths.local_dir.join("game.exe"), "").unwrap();
        let wine = tmp.path().join("external-wine");
        std::fs::write(&wine, "").unwrap();
        let mut settings = Settings::default();
        settings.set_game_runner(
            "g",
            Some(crate::settings::GameRunner {
                program: wine.clone(),
                runner: Runner::Wine,
                label: format!("Wine ({})", wine.display()),
                steam_root: None,
            }),
        );
        let manifest = Manifest {
            id: "g".into(),
            launch: LaunchSpec {
                exe: "game.exe".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let ctx = LaunchContext {
            paths: &paths,
            game_id: "g",
            settings: &settings,
            manifest: Some(&manifest),
            receipt: None,
            alternative: None,
        };

        let planned = plan(&ctx).unwrap();
        assert_eq!(planned.program, wine);
        assert!(planned.runner.starts_with("Wine ("));
    }
}
