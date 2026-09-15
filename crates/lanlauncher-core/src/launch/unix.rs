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
                });
                break;
            }
        }
    }
    if let Some(p) = rp.proton.clone().filter(|p| p.is_file()) {
        out.push(DetectedRunner {
            runner: Runner::Proton,
            program: p,
            label: "Proton".into(),
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
        });
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
            });
        }
        return Ok(LaunchPlan {
            program: exe,
            args,
            cwd,
            env,
            runner: "native".into(),
            needs_elevation: false,
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
    .ok_or_else(|| Error::Launch("no Wine, CrossOver or Proton found".into()))?;

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
            })
        }
        Runner::Proton => {
            env.entry("STEAM_COMPAT_DATA_PATH".into())
                .or_insert(prefix_dir.to_string_lossy().to_string());
            env.entry("STEAM_COMPAT_CLIENT_INSTALL_PATH".into())
                .or_insert(
                    dirs_home()
                        .map(|h| h.join(".steam/steam").to_string_lossy().to_string())
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
