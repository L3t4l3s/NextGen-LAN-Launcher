//! macOS / Linux: run Windows game executables through a compatibility layer.

use super::setup_script::Script;
use super::{expand_args, resolve_exe, LaunchContext, LaunchPlan};
use crate::error::{Error, Result};
use crate::manifest::{ProtonBuild, Runner};
use crate::paths::GamePaths;
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

/// The detected runner of `kind` at `program`, or the stored pin when that is
/// the same tool and detection no longer reaches it. The one rule for "is
/// this the selected tool", shared by the plan and the runner choice.
pub fn find_runner(
    runners: &[DetectedRunner],
    kind: Runner,
    program: &Path,
    stored: Option<&GameRunner>,
) -> Option<DetectedRunner> {
    runners
        .iter()
        .find(|d| d.runner == kind && same_program(&d.program, program))
        .cloned()
        .or_else(|| {
            stored
                .filter(|stored| stored.runner == kind && same_program(&stored.program, program))
                .and_then(persisted_runner)
        })
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

/// The Steam a Proton runs for (`STEAM_COMPAT_CLIENT_INSTALL_PATH`): the one
/// it was found under, else `~/.steam/steam` for a Proton set by hand.
pub fn steam_client_root(steam_root: Option<&Path>) -> Option<PathBuf> {
    steam_root
        .map(Path::to_path_buf)
        .or_else(|| dirs_home().map(|h| h.join(".steam/steam")))
}

pub fn plan(ctx: &LaunchContext<'_>) -> Result<LaunchPlan> {
    if starts_through_script(ctx) {
        // The script decides what starts; a script run through a native
        // profile is no case, `starts_through_script` asks for Wine.
        if let Some(mut plan) = script_plan(ctx, Script::Start)? {
            plan.wrapper.splice(0..0, trusted_wrapper(ctx.manifest));
            return Ok(plan);
        }
    }
    let mut plan = plan_without_wrapper(ctx)?;
    // A `.app` starts through `open`, which hands it to launchd and exits at
    // once: a wrapper would wrap `open`, never the game. The profile's
    // wrapper goes outside the Steam Linux Runtime the plan may already
    // carry ([`steam_runtime`]): `gamescope -- <runtime> -- proton …`.
    if plan.program != Path::new("/usr/bin/open") {
        plan.wrapper.splice(0..0, trusted_wrapper(ctx.manifest));
    }
    Ok(plan)
}

/// The profile's wrapper, from a profile allowed to start host programs:
/// the user's own and the bundled ones. One from a game share comes from
/// whoever fills the share, one guessed from `game_start.cmd` from a script
/// that was never written for this; neither may put a program of their
/// choosing in front of the start.
fn trusted_wrapper(manifest: Option<&crate::manifest::Manifest>) -> Vec<String> {
    use crate::manifest::Manifest;
    manifest
        .filter(|m| m.wrapper_is_trusted())
        .map(|m| m.launch_for(Manifest::current_platform()).wrapper)
        .unwrap_or_default()
        .into_iter()
        .filter(|word| !word.is_empty())
        .collect()
}

fn plan_without_wrapper(ctx: &LaunchContext<'_>) -> Result<LaunchPlan> {
    let (exe, args, cwd, wanted) = resolve_exe(ctx)?;
    // In whatever case it is on disk: Wine does not care, the file system
    // does, and the names come from scripts written for Windows.
    let on_disk = |path: &Path| {
        path.strip_prefix(&ctx.paths.local_dir)
            .ok()
            .and_then(|rel| {
                crate::paths::find_ignoring_case(&ctx.paths.local_dir, &rel.to_string_lossy())
            })
    };
    let exe = match exe.exists() {
        true => exe,
        false => on_disk(&exe)
            .ok_or_else(|| Error::Launch(format!("executable not found: {}", exe.display())))?,
    };
    let cwd = match cwd.exists() {
        true => cwd,
        false => on_disk(&cwd)
            .ok_or_else(|| Error::Launch(format!("working folder not found: {}", cwd.display())))?,
    };
    let args = expand_args(&args, ctx);
    let env: BTreeMap<String, String> = ctx
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
                wrapper: Vec::new(),
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
            wrapper: Vec::new(),
        });
    }

    wine_plan(ctx, exe, args, cwd, wanted, env)
}

/// What runs a Windows program of this game: the selected or automatic
/// runner, in the game's prefix (or bottle), with the profile's variables.
/// The game's own start and its setup script ([`setup_script_plan`]) share
/// it, so the setup writes into exactly the prefix the game then reads.
fn wine_plan(
    ctx: &LaunchContext<'_>,
    exe: PathBuf,
    args: Vec<String>,
    cwd: PathBuf,
    wanted: Runner,
    mut env: BTreeMap<String, String>,
) -> Result<LaunchPlan> {
    let runners = detect_runners(ctx.settings);
    let selected = ctx.settings.game_runner(ctx.game_id);
    let selected_program = selected.map(|runner| runner.program.as_path());
    let chosen = if let Some(selected) = selected {
        find_runner(&runners, selected.runner, &selected.program, Some(selected))
    } else {
        // A Proton path in the settings is a decision, like a pin.
        let build = ctx
            .manifest
            .and_then(|m| m.launch.proton)
            .filter(|_| ctx.settings.runner_paths.proton.is_none());
        automatic_runner(&runners, wanted, build)
    }
    .ok_or_else(|| {
        if let Some(program) = selected_program {
            log::warn!(
                "selected compatibility tool is no longer available: {}",
                program.display()
            );
            return Error::Code("err.runner_missing".into());
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
        Error::Code(format!("err.no_runner|{probed}"))
    })?;

    // Automatic mode keeps the original prefix for backwards compatibility.
    // An explicitly selected tool gets a stable, separate prefix: switching
    // Proton versions must neither migrate nor overwrite another version's
    // registry and save data. The exception is a tool pinned while it was
    // already the one Automatic ran — that prefix is its own, and moving it to
    // an empty one would look to the player like lost savegames.
    let separate_program = selected
        .filter(|s| !s.shares_default_prefix)
        .map(|s| s.program.as_path());
    let prefix_dir = prefix_dir_for(&ctx.paths.share_dir, separate_program);
    match chosen.runner {
        Runner::Crossover => {
            let bottle = separate_program
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
                wrapper: Vec::new(),
            })
        }
        Runner::Proton => {
            set_prefix(
                &mut env,
                "STEAM_COMPAT_DATA_PATH",
                &prefix_dir,
                separate_program.is_some(),
            );
            // The Steam that owns this Proton, not a guess: a Flatpak Steam
            // or a second installation lives nowhere near `~/.steam/steam`,
            // and Proton refuses to start when this points at the wrong one.
            env.entry("STEAM_COMPAT_CLIENT_INSTALL_PATH".into())
                .or_insert(
                    steam_client_root(chosen.steam_root.as_deref())
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default(),
                );
            if crate::player_settings::brings_own_steam_client(&ctx.paths.local_dir) {
                // A package with its own steamclient (Goldberg's
                // ColdClientLoader, RevEmu's loader) starts the game with it;
                // under Proton the game's steam_api loads Proton's bridge to
                // the real Steam (lsteamclient) as well, and with no Steam
                // running it ends with "Steam must be running" (Borderlands
                // 2, 32 bit) or crashes inside the emulator's DLL
                // (Counter-Strike: Global Offensive). The emulator has no
                // use for the bridge.
                add_dll_override(&mut env, "lsteamclient", "d");
            }
            let wrapper = steam_runtime(&chosen, &ctx.paths.share_dir, &mut env);
            // Inside the runtime Proton gets the verb the entry point got, as
            // Steam and umu pass it (`waitforexitandrun`: wait until the
            // prefix's Wine from a setup just before has gone, then run);
            // on the host it stays `run`, as it always was. Untested against
            // a real runtime, check at the LAN.
            let verb = if wrapper.is_empty() {
                "run"
            } else {
                "waitforexitandrun"
            };
            let mut a = vec![verb.to_string(), exe.to_string_lossy().to_string()];
            a.extend(args);
            Ok(LaunchPlan {
                program: chosen.program,
                args: a,
                cwd,
                env,
                runner: chosen.label,
                needs_elevation: false,
                raw_command_line: None,
                wrapper,
            })
        }
        _ => {
            set_prefix(
                &mut env,
                "WINEPREFIX",
                &prefix_dir,
                separate_program.is_some(),
            );
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
                wrapper: Vec::new(),
            })
        }
    }
}

/// The Steam Linux Runtime in front of a Proton that asks for one
/// (`proton::required_runtime`), as Steam runs it: `<runtime>/_v2-entry-point
/// --verb=waitforexitandrun --`, then Proton. Empty when the Proton names no
/// runtime, or when it is not installed — then Proton runs on the host as
/// before, and the diagnostics page says what is missing.
///
/// The runtime's container sees the home folder, not a library on another
/// drive (a Steam Deck's SD card under `/run/media`): the Proton goes in as
/// `STEAM_COMPAT_TOOL_PATHS`, the game's folder as `STEAM_COMPAT_INSTALL_PATH`
/// (unless a profile set them) and always in `STEAM_COMPAT_MOUNTS`, which the
/// container shares as well. The `fnr` helper below the launcher's data
/// folder is only seen while that is in the home folder.
fn steam_runtime(
    chosen: &DetectedRunner,
    share_dir: &Path,
    env: &mut BTreeMap<String, String>,
) -> Vec<String> {
    // No Steam to look in: nothing, rather than `steamapps/` below
    // whatever the current folder is.
    let root = match env.get("STEAM_COMPAT_CLIENT_INSTALL_PATH") {
        Some(root) if !root.is_empty() => PathBuf::from(root),
        _ => return Vec::new(),
    };
    let Some(required) = super::proton::required_runtime(&chosen.program, &root) else {
        return Vec::new();
    };
    let Some(entry) = required.entry_point else {
        if first_time(format!(
            "missing {} {}",
            required.appid,
            chosen.program.display()
        )) {
            log::warn!(
                "{} asks for the Steam Linux Runtime (app {}), which is not installed; running it without",
                chosen.label,
                required.appid
            );
        }
        return Vec::new();
    };
    if first_time(format!("using {}", entry.display())) {
        log::info!(
            "{} runs inside the Steam Linux Runtime {}",
            chosen.label,
            entry.display()
        );
    }
    if let Some(dir) = chosen.program.parent() {
        env.entry("STEAM_COMPAT_TOOL_PATHS".into())
            .or_insert_with(|| dir.to_string_lossy().to_string());
    }
    let share = share_dir.to_string_lossy().to_string();
    env.entry("STEAM_COMPAT_INSTALL_PATH".into())
        .or_insert_with(|| share.clone());
    let mounts = env.entry("STEAM_COMPAT_MOUNTS".into()).or_default();
    if !mounts.split(':').any(|m| m == share) {
        if !mounts.is_empty() {
            mounts.push(':');
        }
        mounts.push_str(&share);
    }
    vec![
        entry.to_string_lossy().to_string(),
        "--verb=waitforexitandrun".into(),
        "--".into(),
    ]
}

/// Whether `what` is new in this run: plans are built for every start and
/// setup, the log line about the runtime is worth once.
fn first_time(what: String) -> bool {
    static SEEN: std::sync::Mutex<std::collections::BTreeSet<String>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());
    SEEN.lock().unwrap_or_else(|e| e.into_inner()).insert(what)
}

/// Whether a start runs the game's whole `game_start.cmd` in its prefix
/// (`Script::Start`), as Windows does, rather than an executable: when the
/// executable would only be read off that script — no profile names one,
/// or the profile names none at all — and nobody chose one. A profile that
/// names its executable, a user's own configuration, an executable picked
/// by hand or an alternative entry point start that instead, after the
/// script's preparation ([`preparation_plan`]).
pub fn starts_through_script(ctx: &LaunchContext<'_>) -> bool {
    starts_through_script_for(
        ctx.manifest,
        ctx.receipt,
        &ctx.paths.start_script,
        ctx.alternative,
    )
}

/// [`starts_through_script`] from its parts: the one rule, also for
/// whether the interface has to ask for an executable.
pub fn starts_through_script_for(
    manifest: Option<&crate::manifest::Manifest>,
    receipt: Option<&crate::install::Receipt>,
    start_script: &Path,
    alternative: Option<usize>,
) -> bool {
    let chosen = alternative.is_some() || receipt.is_some_and(|r| r.exe_override.is_some());
    let spec = manifest.map(|m| m.launch_for(crate::manifest::Manifest::current_platform()));
    let from_script = manifest
        .zip(spec.as_ref())
        .is_none_or(|(m, spec)| !m.user_config && (m.exe_from_script || spec.exe.is_empty()));
    let native = spec.is_some_and(|s| s.runner == Runner::Native);
    !chosen && from_script && !native && start_script.is_file()
}

/// What the start script prepares before the game, for a start that runs
/// an executable ([`starts_through_script`] says no): `cmd.exe /c
/// .nll-prep-run.cmd`, waited for before the game starts. `None` without a
/// start script, for a native game, and for a start through the script.
/// Whether a part before the game can run on its own is the files' business
/// (`setup_script::prepare` writes none then).
pub fn preparation_plan(ctx: &LaunchContext<'_>) -> Result<Option<LaunchPlan>> {
    if starts_through_script(ctx) || !ctx.paths.start_script.is_file() {
        return Ok(None);
    }
    script_plan(ctx, Script::Preparation)
}

/// `cmd.exe /c .nll-setup-run.cmd` with the game's runner, in the game's
/// prefix and from the game's folder: how a setup script runs on macOS and
/// Linux (see [`super::setup_script`]). `None` for a game that does not run
/// through Wine at all — a native build in its profile — whose Windows setup
/// script has nothing to set up.
///
/// The game's executable is not needed, only what would run it: a package
/// whose executable the user still has to choose gets its setup all the same.
pub fn setup_script_plan(ctx: &LaunchContext<'_>) -> Result<Option<LaunchPlan>> {
    script_plan(ctx, Script::Setup)
}

/// `script` through Wine's `cmd.exe` with the game's runner, in its prefix,
/// from the game's folder, with the profile's variables. `None` for a native
/// game. Proton gives `cmd.exe` a console window of its own, as Windows does,
/// and a menu (`set /p`) is answered there — also when the launcher's own
/// input is `/dev/null` (tried with Proton 11: the choice typed into the
/// window reached the script).
pub fn script_plan(ctx: &LaunchContext<'_>, script: Script) -> Result<Option<LaunchPlan>> {
    let platform = crate::manifest::Manifest::current_platform();
    let spec = ctx.manifest.map(|m| m.launch_for(platform));
    let wanted = spec.as_ref().map(|s| s.runner).unwrap_or(Runner::Auto);
    let native = match resolve_exe(ctx) {
        Ok((exe, ..)) => !exe
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("exe")),
        Err(_) => false,
    };
    if wanted == Runner::Native || native {
        return Ok(None);
    }
    let mut env = spec.map(|s| s.env).unwrap_or_default();
    env.extend(super::setup_script::env(
        script,
        &ctx.paths.share_dir,
        &ctx.settings.safe_player_name(),
    ));
    // By its name from the game's folder, not by its path: cmd strips the
    // quotes around a `/c` path holding `(` or `&` ("Games (LAN)") and then
    // finds nothing — tried with Proton 11.
    wine_plan(
        ctx,
        PathBuf::from("cmd.exe"),
        vec!["/c".to_string(), script.wrapper_name().to_string()],
        ctx.paths.share_dir.clone(),
        wanted,
        env,
    )
    .map(Some)
}

/// The profile's dedicated server (`[server]`): `cmd.exe /c <exe> …` with
/// the game's runner, in its prefix, from the server's folder (by its name:
/// cmd strips the quotes of a `/c` path holding `(` or `&`). `None` when the
/// profile names none; an error when the file is not there, or for a game
/// that does not run through Wine.
pub fn server_plan(ctx: &LaunchContext<'_>) -> Result<Option<LaunchPlan>> {
    let Some(server) = ctx.manifest.map(|m| &m.server).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let exe = crate::paths::find_ignoring_case(&ctx.paths.local_dir, &server.exe)
        .filter(|p| p.is_file())
        .ok_or_else(|| Error::Code(format!("err.extra_missing|{}", server.exe)))?;
    let platform = crate::manifest::Manifest::current_platform();
    let spec = ctx.manifest.map(|m| m.launch_for(platform));
    let wanted = spec.as_ref().map(|s| s.runner).unwrap_or(Runner::Auto);
    if wanted == Runner::Native {
        return Err(Error::Code("err.windows_only".into()));
    }
    let name = exe
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let cwd = exe
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.paths.local_dir.clone());
    let mut args = vec!["/c".to_string(), name];
    args.extend(server.args.iter().cloned());
    wine_plan(
        ctx,
        PathBuf::from("cmd.exe"),
        args,
        cwd,
        wanted,
        spec.map(|s| s.env).unwrap_or_default(),
    )
    .map(Some)
}

/// `WINEDLLOVERRIDES` with `dll=mode` added, unless the plan's (or else the
/// launcher's own) value names the DLL already: a choice made there stands.
fn add_dll_override(env: &mut BTreeMap<String, String>, dll: &str, mode: &str) {
    let current = env
        .get("WINEDLLOVERRIDES")
        .cloned()
        .or_else(|| std::env::var("WINEDLLOVERRIDES").ok())
        .unwrap_or_default();
    let named = current.split(';').any(|entry| {
        entry
            .split_once('=')
            .map_or(entry, |(names, _)| names)
            .split(',')
            .any(|name| name.trim().eq_ignore_ascii_case(dll))
    });
    if named {
        return;
    }
    let value = if current.trim().is_empty() {
        format!("{dll}={mode}")
    } else {
        format!("{};{dll}={mode}", current.trim_end_matches(';'))
    };
    env.insert("WINEDLLOVERRIDES".into(), value);
}

/// A program of the game a link was registered for
/// ([`super::url_protocol`]), run directly with the game's runner in its
/// prefix, from its own folder: no `cmd.exe` in between, which would take
/// a `&` in the link for the start of another command.
pub fn url_plan(ctx: &LaunchContext<'_>, program: &Path, args: Vec<String>) -> Result<LaunchPlan> {
    let platform = crate::manifest::Manifest::current_platform();
    let spec = ctx.manifest.map(|m| m.launch_for(platform));
    let wanted = spec.as_ref().map(|s| s.runner).unwrap_or(Runner::Auto);
    if wanted == Runner::Native {
        return Err(Error::Code("err.windows_only".into()));
    }
    let cwd = program
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| ctx.paths.local_dir.clone());
    wine_plan(
        ctx,
        program.to_path_buf(),
        args,
        cwd,
        wanted,
        spec.map(|s| s.env).unwrap_or_default(),
    )
}

/// Whether `plan` runs the game's start script ([`starts_through_script`]).
pub fn is_script_start(plan: &LaunchPlan) -> bool {
    plan.args
        .last()
        .is_some_and(|a| a == Script::Start.wrapper_name())
}

/// Whether `plan` runs the package's `server_start.cmd` ([`Script::Server`]),
/// whose files have to be written first.
pub fn is_server_script(plan: &LaunchPlan) -> bool {
    plan.args
        .last()
        .is_some_and(|a| a == Script::Server.wrapper_name())
}

/// The prefix a plan runs in, as one string to compare: the CrossOver
/// bottle by name, else `STEAM_COMPAT_DATA_PATH` or `WINEPREFIX` resolved
/// (relative to the plan's folder, through symlinks — `/home` is
/// `/var/home` on Fedora Atomic). `None` for a native start.
pub fn prefix_id(plan: &LaunchPlan) -> Option<String> {
    if plan.args.first().is_some_and(|a| a == "--bottle") {
        return plan.args.get(1).map(|bottle| format!("bottle:{bottle}"));
    }
    if plan.runner.starts_with("native") {
        return None;
    }
    ["STEAM_COMPAT_DATA_PATH", "WINEPREFIX"]
        .iter()
        .find_map(|name| plan.env.get(*name))
        .map(|dir| {
            let dir = Path::new(dir);
            let dir = if dir.is_absolute() {
                dir.to_path_buf()
            } else {
                plan.cwd.join(dir)
            };
            crate::transport::normalise_dir(&dir)
        })
}

/// What Automatic runs for a game: the newest of the Proton build its
/// manifest asks for when one is installed and the manifest wants no other
/// kind of runner, else the first runner that fits what the manifest asks
/// for, else the best one there is.
fn automatic_runner(
    runners: &[DetectedRunner],
    wanted: Runner,
    build: Option<ProtonBuild>,
) -> Option<DetectedRunner> {
    if matches!(wanted, Runner::Auto | Runner::Proton) {
        let asked = runners
            .iter()
            .filter(|d| d.runner == Runner::Proton)
            .filter_map(|d| match build? {
                ProtonBuild::Ge => super::proton::ge_build(&d.program, &d.label).map(|v| (v, d)),
            })
            // The first of equal builds: the order of `detect_runners`.
            .rev()
            .max_by(|a, b| a.0.cmp(&b.0));
        if let Some((_, asked)) = asked {
            return Some(asked.clone());
        }
    }
    match wanted {
        Runner::Auto => runners.first().cloned(),
        r => runners
            .iter()
            .find(|d| d.runner == r)
            .cloned()
            .or_else(|| runners.first().cloned()),
    }
}

/// A tool that ran in a game's own prefix, as the launcher recorded it when
/// it started one there.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefixUser {
    pub runner: Runner,
    pub program: PathBuf,
}

/// What ran in a game's own prefix (`.nll-prefix`, bottle `nll-<id>`), kept
/// beside the install receipt.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PrefixRecord {
    /// Every tool that ran there, not only the last: a newer Proton that
    /// Automatic tried once and that crashed has touched the prefix, but the
    /// saves are still the older one's, and pinning that one must find them.
    pub users: Vec<PrefixUser>,
    /// The prefix existed before its record did. Which versions made it is
    /// unknown then — every one Automatic picked over the months — so any
    /// tool of the prefix's kind counts as having run there.
    pub older_than_record: bool,
}

const PREFIX_RECORD_FILE: &str = ".nll-default-prefix.json";

/// Which kind of tool `plan` runs in the game's own prefix, or `None` for a
/// plan that runs anywhere else: natively, in a separate version's prefix, or
/// in one the manifest names. Read off the plan's environment rather than
/// from how `plan` decided, because this is where the program really runs:
/// a manifest value naming the same folder is the same prefix.
fn own_prefix_kind(plan: &LaunchPlan, paths: &GamePaths, game_id: &str) -> Option<Runner> {
    let bottle = format!("nll-{game_id}");
    if plan
        .args
        .windows(2)
        .any(|pair| pair[0] == "--bottle" && pair[1] == bottle)
    {
        return Some(Runner::Crossover);
    }
    let own = prefix_dir(&paths.share_dir);
    let names_own = |name: &str| {
        plan.env.get(name).is_some_and(|value| {
            let path = Path::new(value);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                plan.cwd.join(path)
            };
            path.components().eq(own.components()) || same_program(&path, &own)
        })
    };
    if names_own("STEAM_COMPAT_DATA_PATH") {
        Some(Runner::Proton)
    } else if names_own("WINEPREFIX") {
        Some(Runner::Wine)
    } else {
        None
    }
}

/// Whether `plan` runs in the game's own prefix and that already holds a
/// prefix of any kind — also one another kind of tool made, whose saves are
/// in it just the same. Asked before the start (which creates one), for
/// [`remember_default_prefix_user`].
pub fn own_prefix_exists_before_start(plan: &LaunchPlan, paths: &GamePaths, game_id: &str) -> bool {
    own_prefix_kind(plan, paths, game_id).is_some()
        && [Runner::Proton, Runner::Wine, Runner::Crossover]
            .into_iter()
            .any(|kind| own_prefix_has_layout(paths, game_id, kind, &plan.env))
}

/// Whether the game's own prefix is there, laid out the way `kind` makes
/// one: Proton puts the Wine prefix into `pfx/`, Wine uses the folder
/// itself, CrossOver keeps a bottle of its own.
fn own_prefix_has_layout(
    paths: &GamePaths,
    game_id: &str,
    kind: Runner,
    env: &BTreeMap<String, String>,
) -> bool {
    match kind {
        Runner::Crossover => super::crossover_bottle_exists(&format!("nll-{game_id}"), env),
        kind => has_layout(&prefix_dir(&paths.share_dir), kind),
    }
}

/// A Wine or Proton prefix really made in `dir`, not just the folder: the
/// launcher creates Proton's outer folder before Proton runs, so a start
/// that failed at once leaves one behind with nothing in it.
fn has_layout(dir: &Path, kind: Runner) -> bool {
    match kind {
        Runner::Proton => dir.join("pfx").is_dir(),
        Runner::Wine => dir.join("drive_c").is_dir(),
        _ => false,
    }
}

/// Add the tool a started plan ran in the game's own prefix to the record.
/// Called once the start succeeded, never for a plan that was only shown: a
/// plan built for the details page has run nowhere. `existed_before` is
/// [`own_prefix_exists_before_start`], asked before that start.
pub fn remember_default_prefix_user(
    plan: &LaunchPlan,
    paths: &GamePaths,
    game_id: &str,
    existed_before: bool,
) {
    let Some(runner) = own_prefix_kind(plan, paths, game_id) else {
        return;
    };
    let user = PrefixUser {
        runner,
        program: std::fs::canonicalize(&plan.program).unwrap_or_else(|_| plan.program.clone()),
    };
    let path = paths.share_dir.join(PREFIX_RECORD_FILE);
    let mut record = match default_prefix_record(paths) {
        Some(record) => record,
        None => PrefixRecord {
            users: Vec::new(),
            older_than_record: existed_before,
        },
    };
    if record.users.contains(&user) {
        return;
    }
    record.users.push(user);
    // Through a temporary file: a record cut short by a crash would read as
    // none at all, and the next start would write it anew without the tools
    // that ran before.
    let staging = paths.share_dir.join(format!(
        "{PREFIX_RECORD_FILE}.{}.tmp",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|t| t.as_nanos())
            .unwrap_or_default()
    ));
    let written = serde_json::to_vec_pretty(&record)
        .map_err(|e| e.to_string())
        .and_then(|json| std::fs::write(&staging, json).map_err(|e| e.to_string()))
        .and_then(|()| std::fs::rename(&staging, &path).map_err(|e| e.to_string()));
    if let Err(e) = written {
        log::warn!("cannot record the prefix user in {}: {e}", path.display());
    }
}

/// The record of what ran in the game's own prefix, if there is one.
pub fn default_prefix_record(paths: &GamePaths) -> Option<PrefixRecord> {
    let text = std::fs::read(paths.share_dir.join(PREFIX_RECORD_FILE)).ok()?;
    serde_json::from_slice(&text)
        .inspect_err(|e| {
            log::warn!(
                "unreadable prefix record in {}: {e}",
                paths.share_dir.display()
            )
        })
        .ok()
}

/// Whether pinning `chosen` keeps the game on its own prefix instead of
/// giving it one of its own. Picking a version whose saves are in the game's
/// prefix is not switching versions, and must not look to the player like
/// lost savegames.
///
/// - A tool that already has a separate prefix or bottle for this game keeps
///   it: its saves are there, whatever ran in the game's own prefix since.
/// - A tool the record says ran in the game's prefix shares it. The record,
///   not today's Automatic pick: Steam installs a newer Proton in the
///   background, Automatic moves on, and the version the saves were made
///   with is still the one to match.
/// - A prefix older than its record — or with no record yet, made before the
///   launcher kept one — was shared by every version Automatic picked, so
///   any tool of its kind shares it: the usual reason to pin an older Proton
///   is that the newer one broke the game, and its saves are there.
/// - Anything else gets a prefix of its own.
pub fn pinning_keeps_default_prefix(
    paths: &GamePaths,
    game_id: &str,
    chosen: &DetectedRunner,
) -> bool {
    let env = BTreeMap::new();
    // The spelling `set_game_runner` stores and `plan` derives the key from.
    let program = std::fs::canonicalize(&chosen.program).unwrap_or_else(|_| chosen.program.clone());
    let has_its_own = match chosen.runner {
        Runner::Crossover => {
            super::crossover_bottle_exists(&format!("nll-{game_id}-{}", runner_key(&program)), &env)
        }
        kind => has_layout(&prefix_dir_for(&paths.share_dir, Some(&program)), kind),
    };
    if has_its_own {
        return false;
    }
    let record = default_prefix_record(paths);
    let recorded = record.as_ref().is_some_and(|record| {
        record
            .users
            .iter()
            .any(|user| user.runner == chosen.runner && same_program(&user.program, &program))
    });
    recorded
        || (record
            .as_ref()
            .is_none_or(|record| record.older_than_record)
            && own_prefix_has_layout(paths, game_id, chosen.runner, &env))
}

/// Put the prefix into the environment.
///
/// The game's own prefix defers to a value the manifest set, like every other
/// variable in `plan`: an organiser who ships a prepared prefix for a game
/// means it. A *separate* version's prefix does not defer, and that is the
/// point of it — letting the manifest's value through would put every pinned
/// Proton back into one shared prefix, the corruption per-version prefixes
/// exist to prevent. The start's log line names the prefix actually used.
fn set_prefix(env: &mut BTreeMap<String, String>, name: &str, dir: &Path, separate: bool) {
    let value = dir.to_string_lossy().to_string();
    if !separate {
        env.entry(name.into()).or_insert(value);
        return;
    }
    // Not logged here: plans are built each time the details are shown.
    // The prefix a start really used is in its `starting …` log line.
    env.insert(name.into(), value);
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

        let found = crate::launch::proton::find_protons_in(&home, &[]);
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

    /// A profile asking for GE-Proton gets it from Automatic, ahead of a
    /// Wine and of Valve's Proton, recognised by its `version` file whatever
    /// the folder is called; without one installed the usual order holds.
    #[test]
    fn a_profile_asking_for_ge_proton_gets_it_when_installed() {
        let tmp = tempfile::tempdir().expect("tmp");
        let tool = |dir: &str, version: Option<&str>| {
            let proton = tmp.path().join(dir).join("proton");
            std::fs::create_dir_all(proton.parent().expect("parent")).expect("dirs");
            std::fs::write(&proton, "#!/bin/sh\n").expect("proton");
            if let Some(version) = version {
                std::fs::write(tmp.path().join(dir).join("version"), version).expect("version");
            }
            DetectedRunner {
                runner: Runner::Proton,
                program: proton,
                label: dir.into(),
                steam_root: None,
            }
        };
        let wine = DetectedRunner {
            runner: Runner::Wine,
            program: "/usr/bin/wine".into(),
            label: "Wine".into(),
            steam_root: None,
        };
        let valve = tool("Proton 11.0", Some("1789520217 proton-11.0-3\n"));
        let latest = tool("Proton-GE Latest", Some("1789520217 GE-Proton11-7\n"));
        let runners = vec![wine.clone(), valve.clone(), latest.clone()];
        let pick = |runners: &[DetectedRunner], build| {
            automatic_runner(runners, Runner::Auto, build).map(|d| d.label)
        };
        assert_eq!(
            pick(&runners, Some(ProtonBuild::Ge)),
            Some(latest.label.clone())
        );
        assert_eq!(pick(&runners, None), Some(wine.label.clone()));
        // A build without a `version` file is known by its name.
        let named = tool("GE-Proton10-25", None);
        assert_eq!(
            pick(&[valve.clone(), named.clone()], Some(ProtonBuild::Ge)),
            Some(named.label.clone())
        );
        // The newest GE, whatever its folder is called; an older naming too.
        let old = tool("Proton-6.21-GE-2", Some("1650000000 Proton-6.21-GE-2\n"));
        assert_eq!(
            pick(
                &[old.clone(), named.clone(), latest.clone()],
                Some(ProtonBuild::Ge)
            ),
            Some(latest.label.clone())
        );
        assert_eq!(
            pick(&[valve.clone(), old.clone()], Some(ProtonBuild::Ge)),
            Some(old.label)
        );
        // A profile that wants Wine gets Wine.
        assert_eq!(
            automatic_runner(&runners, Runner::Wine, Some(ProtonBuild::Ge)).map(|d| d.label),
            Some(wine.label.clone())
        );
        let without = vec![wine.clone(), valve];
        assert_eq!(pick(&without, Some(ProtonBuild::Ge)), Some(wine.label));
    }

    /// The setup script runs in the very prefix the game then starts in,
    /// with the same runner, through Wine's `cmd.exe` and without the
    /// profile's wrapper; a native game gets no Windows setup at all. Unix
    /// only, like the setup: a Windows path has no Wine drive to become.
    #[cfg(unix)]
    #[test]
    fn the_setup_script_runs_in_the_games_own_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "g");
        std::fs::create_dir_all(&paths.local_dir).unwrap();
        std::fs::write(paths.local_dir.join("game.exe"), "").unwrap();
        let proton = tmp.path().join("Proton 11.0/proton");
        std::fs::create_dir_all(proton.parent().unwrap()).unwrap();
        std::fs::write(&proton, "").unwrap();
        let wine = tmp.path().join("wine");
        std::fs::write(&wine, "").unwrap();
        let mut settings = Settings::default();
        settings.runner_paths.proton = Some(proton.clone());
        settings.runner_paths.wine = Some(wine.clone());
        let manifest = |runner, exe: &str| Manifest {
            id: "g".into(),
            launch: LaunchSpec {
                exe: exe.into(),
                runner,
                wrapper: vec!["gamescope".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        let both = |m: &Manifest| {
            let ctx = LaunchContext {
                paths: &paths,
                game_id: "g",
                settings: &settings,
                manifest: Some(m),
                receipt: None,
                alternative: None,
            };
            (plan(&ctx).unwrap(), setup_script_plan(&ctx).unwrap())
        };

        let (game, setup) = both(&manifest(Runner::Proton, "game.exe"));
        let setup = setup.expect("a Windows game gets its setup");
        assert_eq!(setup.program, proton);
        assert_eq!(
            setup.args,
            vec!["run", "cmd.exe", "/c", ".nll-setup-run.cmd"]
        );
        assert_eq!(
            setup.env.get("STEAM_COMPAT_DATA_PATH"),
            game.env.get("STEAM_COMPAT_DATA_PATH")
        );
        assert_eq!(setup.cwd, paths.share_dir);
        assert!(setup.wrapper.is_empty());
        assert!(setup.env["NLL_GAME_PATH"].starts_with("Z:\\"));

        let (game, setup) = both(&manifest(Runner::Wine, "game.exe"));
        let setup = setup.unwrap();
        assert_eq!(setup.program, wine);
        assert_eq!(setup.args, vec!["cmd.exe", "/c", ".nll-setup-run.cmd"]);
        assert_eq!(setup.env.get("WINEPREFIX"), game.env.get("WINEPREFIX"));

        std::fs::write(paths.local_dir.join("game.x86_64"), "").unwrap();
        let native = LaunchContext {
            paths: &paths,
            game_id: "g",
            settings: &settings,
            manifest: Some(&manifest(Runner::Auto, "game.x86_64")),
            receipt: None,
            alternative: None,
        };
        assert_eq!(setup_script_plan(&native).unwrap(), None);
    }

    /// An executable read off the start script starts through the script,
    /// as on Windows; an
    /// executable a profile names, or one picked by hand, starts as it is,
    /// after the script's preparation.
    #[cfg(unix)]
    #[test]
    fn a_game_without_a_profile_starts_through_its_script() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "g");
        std::fs::create_dir_all(&paths.local_dir).unwrap();
        std::fs::write(paths.local_dir.join("game.exe"), "").unwrap();
        std::fs::write(&paths.start_script, "\"game.exe\"\r\n").unwrap();
        let wine = tmp.path().join("wine");
        std::fs::write(&wine, "").unwrap();
        let mut settings = Settings::default();
        settings.runner_paths.wine = Some(wine.clone());
        let manifest = |from_script: bool| Manifest {
            id: "g".into(),
            exe_from_script: from_script,
            launch: LaunchSpec {
                exe: "game.exe".into(),
                runner: Runner::Wine,
                ..Default::default()
            },
            ..Default::default()
        };
        let derived = manifest(true);
        let curated = manifest(false);
        fn ctx<'a>(
            paths: &'a GamePaths,
            settings: &'a Settings,
            manifest: Option<&'a Manifest>,
            receipt: Option<&'a crate::install::Receipt>,
        ) -> LaunchContext<'a> {
            LaunchContext {
                paths,
                game_id: "g",
                settings,
                manifest,
                receipt,
                alternative: None,
            }
        }

        let through_script = plan(&ctx(&paths, &settings, Some(&derived), None)).unwrap();
        assert!(is_script_start(&through_script));
        assert_eq!(through_script.args, ["cmd.exe", "/c", ".nll-start-run.cmd"]);
        assert_eq!(through_script.cwd, paths.share_dir);
        assert!(through_script.env["NLL_SCRIPT"].ends_with(".nll-start.cmd"));
        assert!(plan(&ctx(&paths, &settings, None, None)).is_ok_and(|p| is_script_start(&p)));
        assert_eq!(
            preparation_plan(&ctx(&paths, &settings, Some(&derived), None)).unwrap(),
            None
        );

        let profiled = plan(&ctx(&paths, &settings, Some(&curated), None)).unwrap();
        assert!(!is_script_start(&profiled));
        let prep = preparation_plan(&ctx(&paths, &settings, Some(&curated), None))
            .unwrap()
            .unwrap();
        assert_eq!(prep.args, ["cmd.exe", "/c", ".nll-prep-run.cmd"]);
        assert_eq!(prep.env.get("WINEPREFIX"), profiled.env.get("WINEPREFIX"));

        let picked = crate::install::Receipt {
            version: 1,
            game_id: "g".into(),
            revision: "1".into(),
            installed_at: chrono::Utc::now(),
            archive_bytes: 0,
            files: 0,
            setup_done: true,
            exe_override: Some("game.exe".into()),
            adopted: false,
            script_setup_prefixes: Vec::new(),
        };
        assert!(!is_script_start(
            &plan(&ctx(&paths, &settings, Some(&derived), Some(&picked))).unwrap()
        ));

        // No script, no start through it.
        std::fs::remove_file(&paths.start_script).unwrap();
        assert!(!is_script_start(
            &plan(&ctx(&paths, &settings, Some(&derived), None)).unwrap()
        ));
        assert_eq!(
            preparation_plan(&ctx(&paths, &settings, Some(&curated), None)).unwrap(),
            None
        );
    }

    /// A pinned version gets a prefix of its own, and the setup's bookkeeping
    /// has to tell the two apart; the same prefix reached through a symlink
    /// is one.
    #[cfg(unix)]
    #[test]
    fn a_prefix_is_named_by_where_it_is_and_a_bottle_by_its_name() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("var-home");
        std::fs::create_dir_all(real.join("g/.nll-prefix")).unwrap();
        std::fs::create_dir_all(real.join("g/local")).unwrap();
        let link = tmp.path().join("home");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let plan = |runner: &str, args: &[&str], env: &[(&str, String)]| LaunchPlan {
            program: PathBuf::from("/x"),
            args: args.iter().map(|a| a.to_string()).collect(),
            cwd: link.join("g/local"),
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
            runner: runner.into(),
            needs_elevation: false,
            raw_command_line: None,
            wrapper: Vec::new(),
        };
        let compat = |dir: PathBuf| ("STEAM_COMPAT_DATA_PATH", dir.to_string_lossy().to_string());
        let through_link = prefix_id(&plan(
            "Proton",
            &["run"],
            &[compat(link.join("g/.nll-prefix"))],
        ));
        let direct = prefix_id(&plan(
            "Proton",
            &["run"],
            &[compat(real.join("g/.nll-prefix"))],
        ));
        assert!(through_link.is_some());
        assert_eq!(through_link, direct);
        let pinned = prefix_id(&plan(
            "Proton",
            &["run"],
            &[compat(real.join("g/.nll-prefix-0123456789abcdef"))],
        ));
        assert_ne!(pinned, direct);
        assert_eq!(
            prefix_id(&plan(
                "Wine",
                &[],
                &[("WINEPREFIX", "../.nll-prefix".into())]
            )),
            direct
        );
        assert_eq!(
            prefix_id(&plan("CrossOver", &["--bottle", "nll-g"], &[])).as_deref(),
            Some("bottle:nll-g")
        );
        assert_eq!(prefix_id(&plan("native", &[], &[])), None);
    }

    /// A wrapper goes in front of the start from the user's own or a bundled
    /// profile, never from a game share's; the tool the plan runs stays
    /// `program`, so the prefix record and `cxbottle` still see Wine.
    #[test]
    fn a_wrapper_comes_only_from_a_trusted_profile_and_leaves_the_tool_alone() {
        use crate::manifest::{ManifestOrigin, PlatformOverride};
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "g");
        std::fs::create_dir_all(paths.local_dir.join("bin")).unwrap();
        std::fs::write(paths.local_dir.join("bin/game.exe"), "").unwrap();
        let wine = tmp.path().join("wine");
        std::fs::write(&wine, "").unwrap();
        let mut settings = Settings::default();
        settings.runner_paths.wine = Some(wine.clone());
        let mut m = Manifest {
            id: "g".into(),
            launch: LaunchSpec {
                exe: "bin/game.exe".into(),
                runner: Runner::Wine,
                wrapper: vec!["gamemoderun".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        m.platform.insert(
            Manifest::current_platform().into(),
            PlatformOverride {
                wrapper: Some(vec!["gamescope".into(), "-f".into(), "--".into()]),
                workdir: Some("bin".into()),
                ..Default::default()
            },
        );
        let plan_from = |origin| {
            let mut m = m.clone();
            m.origin = origin;
            plan(&LaunchContext {
                paths: &paths,
                game_id: "g",
                settings: &settings,
                manifest: Some(&m),
                receipt: None,
                alternative: None,
            })
            .unwrap()
        };

        let own = plan_from(ManifestOrigin::UserOverride);
        assert_eq!(
            own.wrapper,
            ["gamescope", "-f", "--"],
            "the platform's wins"
        );
        assert!(same_program(&own.program, &wine));
        assert_eq!(own.cwd, paths.local_dir.join("bin"));
        assert_eq!(plan_from(ManifestOrigin::Bundled).wrapper.len(), 3);
        assert!(plan_from(ManifestOrigin::ShareOverlay).wrapper.is_empty());
        assert!(plan_from(ManifestOrigin::DerivedFromScript)
            .wrapper
            .is_empty());
    }

    /// Proton 11 asks for Steam Linux Runtime 4: installed, the game, its
    /// setup and its start script run through its entry point, the profile's
    /// wrapper outside, the tool itself still the plan's program.
    #[cfg(unix)]
    #[test]
    fn a_proton_runs_inside_the_runtime_it_asks_for() {
        use crate::manifest::ManifestOrigin;
        use crate::settings::GameRunner;
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "g");
        std::fs::create_dir_all(&paths.local_dir).unwrap();
        std::fs::write(paths.local_dir.join("game.exe"), "").unwrap();
        let steam = tmp.path().join("Steam");
        let proton = steam.join("steamapps/common/Proton 11.0/proton");
        std::fs::create_dir_all(proton.parent().unwrap()).unwrap();
        std::fs::write(&proton, "").unwrap();
        std::fs::write(
            proton.parent().unwrap().join("toolmanifest.vdf"),
            "\"manifest\"\n{\n  \"require_tool_appid\" \"4183110\"\n}\n",
        )
        .unwrap();
        let mut settings = Settings::default();
        settings.set_game_runner(
            "g",
            Some(GameRunner {
                program: proton.clone(),
                runner: Runner::Proton,
                label: "Proton 11.0".into(),
                steam_root: Some(steam.clone()),
                shares_default_prefix: true,
            }),
        );
        let m = Manifest {
            id: "g".into(),
            origin: ManifestOrigin::Bundled,
            launch: LaunchSpec {
                exe: "game.exe".into(),
                runner: Runner::Proton,
                wrapper: vec!["gamemoderun".into()],
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

        // Not installed: on the host, as before.
        let game = plan(&ctx).unwrap();
        assert_eq!(game.wrapper, ["gamemoderun"]);
        assert!(!game.env.contains_key("STEAM_COMPAT_TOOL_PATHS"));

        std::fs::write(
            steam.join("steamapps/appmanifest_4183110.acf"),
            "\"AppState\"\n{\n \"installdir\" \"SteamLinuxRuntime_4\"\n}\n",
        )
        .unwrap();
        let entry = steam.join("steamapps/common/SteamLinuxRuntime_4/_v2-entry-point");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, "").unwrap();
        let runtime = [
            entry.to_string_lossy().to_string(),
            "--verb=waitforexitandrun".into(),
            "--".into(),
        ];
        let game = plan(&ctx).unwrap();
        assert_eq!(game.program, proton);
        assert_eq!(game.wrapper[0], "gamemoderun");
        assert_eq!(game.wrapper[1..], runtime);
        assert_eq!(game.args[0], "waitforexitandrun");
        assert_eq!(
            game.env["STEAM_COMPAT_MOUNTS"],
            paths.share_dir.to_string_lossy()
        );
        assert_eq!(
            game.env["STEAM_COMPAT_TOOL_PATHS"],
            proton.parent().unwrap().to_string_lossy()
        );
        assert_eq!(
            game.env["STEAM_COMPAT_INSTALL_PATH"],
            paths.share_dir.to_string_lossy()
        );
        let setup = setup_script_plan(&ctx).unwrap().unwrap();
        assert_eq!(setup.wrapper, runtime, "a setup gets no profile wrapper");
        assert_eq!(setup.program, proton);
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
                shares_default_prefix: false,
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

    /// The review's finding: pinning by name the very tool Automatic already
    /// ran moved the game to `.nll-prefix-<key>`, an empty prefix, while the
    /// registry and savegames stayed behind in `.nll-prefix`. A pin marked as
    /// sharing the default prefix stays where the game has been, and defers
    /// to a manifest's prefix exactly as Automatic does.
    #[test]
    fn a_tool_pinned_while_automatic_ran_it_keeps_the_games_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "g");
        std::fs::create_dir_all(paths.local_dir.join("bin")).unwrap();
        std::fs::write(paths.local_dir.join("bin/game.exe"), "").unwrap();
        let wine = tmp.path().join("wine");
        std::fs::write(&wine, "").unwrap();
        let mut settings = Settings::default();
        settings.runner_paths.wine = Some(wine.clone());
        let m = Manifest {
            id: "g".into(),
            launch: LaunchSpec {
                exe: "bin/game.exe".into(),
                // Named, not Auto: on a Mac with CrossOver installed that
                // comes first, and Automatic would not be this Wine.
                runner: Runner::Wine,
                ..Default::default()
            },
            ..Default::default()
        };
        let plan_with = |settings: &Settings, manifest: &Manifest| {
            plan(&LaunchContext {
                paths: &paths,
                game_id: "g",
                settings,
                manifest: Some(manifest),
                receipt: None,
                alternative: None,
            })
            .unwrap()
        };
        let automatic = plan_with(&settings, &m).env["WINEPREFIX"].clone();

        settings.set_game_runner(
            "g",
            Some(crate::settings::GameRunner {
                program: wine.clone(),
                runner: Runner::Wine,
                label: "Wine".into(),
                steam_root: None,
                shares_default_prefix: true,
            }),
        );
        assert_eq!(
            plan_with(&settings, &m).env["WINEPREFIX"],
            automatic,
            "the savegames live in the prefix the game has always used"
        );

        let mut with_prefix = m.clone();
        with_prefix
            .launch
            .env
            .insert("WINEPREFIX".into(), "/manifest/prefix".into());
        assert_eq!(
            plan_with(&settings, &with_prefix).env["WINEPREFIX"],
            "/manifest/prefix",
            "on the game's own prefix an organiser's value counts, as it does for Automatic"
        );
    }

    /// The decision behind that flag, made when the tool is picked.
    #[test]
    fn pinning_keeps_the_prefix_for_what_ran_there_and_for_nothing_else() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "g");
        std::fs::create_dir_all(&paths.share_dir).unwrap();
        let tool = |name: &str| {
            let program = tmp.path().join(name);
            std::fs::write(&program, "").unwrap();
            program
        };
        let (older, newer, never) = (tool("wine-9"), tool("wine-10"), tool("wine-7"));
        let wine = |program: &Path| DetectedRunner {
            runner: Runner::Wine,
            program: program.to_path_buf(),
            label: String::new(),
            steam_root: None,
        };
        let keeps = |program: &Path| pinning_keeps_default_prefix(&paths, "g", &wine(program));
        let ran = |program: &Path, existed_before: bool| {
            let plan = LaunchPlan {
                program: program.to_path_buf(),
                args: Vec::new(),
                cwd: tmp.path().to_path_buf(),
                env: BTreeMap::from([(
                    "WINEPREFIX".into(),
                    prefix_dir(&paths.share_dir).to_string_lossy().to_string(),
                )]),
                runner: "Wine".into(),
                needs_elevation: false,
                raw_command_line: None,
                wrapper: Vec::new(),
            };
            remember_default_prefix_user(&plan, &paths, "g", existed_before);
            std::fs::create_dir_all(prefix_dir(&paths.share_dir).join("drive_c")).unwrap();
        };

        assert!(!keeps(&older), "no prefix yet: nothing to keep");

        // A prefix recorded from its first start: only what ran there.
        ran(&older, false);
        ran(&newer, false);
        assert!(keeps(&older), "the newer one does not push out the older");
        assert!(keeps(&newer));
        assert!(
            !keeps(&never),
            "a version that never ran there gets its own"
        );
        assert!(
            !pinning_keeps_default_prefix(
                &paths,
                "g",
                &DetectedRunner {
                    runner: Runner::Proton,
                    ..wine(&older)
                }
            ),
            "the same file as another kind of runner is not what ran there"
        );

        // A tool with a prefix of its own for this game keeps that one.
        let program = std::fs::canonicalize(&older).unwrap();
        let own = prefix_dir_for(&paths.share_dir, Some(&program));
        std::fs::create_dir_all(&own).unwrap();
        assert!(
            keeps(&older),
            "an empty folder a failed start left is no prefix"
        );
        std::fs::create_dir_all(own.join("drive_c")).unwrap();
        assert!(!keeps(&older), "its saves are in its own prefix");
    }

    /// A prefix made before the launcher kept a record was shared by every
    /// version Automatic picked; any tool of its kind may be what saved there.
    #[test]
    fn a_prefix_older_than_its_record_is_shared_by_its_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "g");
        std::fs::create_dir_all(prefix_dir(&paths.share_dir).join("pfx")).unwrap();
        let proton = |name: &str| {
            let program = tmp.path().join(name);
            std::fs::write(&program, "").unwrap();
            DetectedRunner {
                runner: Runner::Proton,
                program,
                label: String::new(),
                steam_root: None,
            }
        };
        let (older, newer) = (proton("proton-9"), proton("proton-10"));

        assert!(
            pinning_keeps_default_prefix(&paths, "g", &older),
            "no record yet, a Proton prefix is there"
        );
        assert!(
            !pinning_keeps_default_prefix(
                &paths,
                "g",
                &DetectedRunner {
                    runner: Runner::Wine,
                    ..older.clone()
                }
            ),
            "Wine does not use a Proton prefix's layout"
        );

        // Steam brought Proton 10, Automatic ran it once — and it crashed.
        // Pinning 9 again must still find the saves.
        let plan = LaunchPlan {
            program: newer.program.clone(),
            args: Vec::new(),
            cwd: tmp.path().to_path_buf(),
            env: BTreeMap::from([(
                "STEAM_COMPAT_DATA_PATH".into(),
                format!("{}/", prefix_dir(&paths.share_dir).display()),
            )]),
            runner: "Proton".into(),
            needs_elevation: false,
            raw_command_line: None,
            wrapper: Vec::new(),
        };
        let existed = own_prefix_exists_before_start(&plan, &paths, "g");
        assert!(existed, "a trailing slash names the same folder");
        remember_default_prefix_user(&plan, &paths, "g", existed);
        let record = default_prefix_record(&paths).unwrap();
        assert!(record.older_than_record);
        assert_eq!(record.users.len(), 1);
        assert!(pinning_keeps_default_prefix(&paths, "g", &older));
    }

    /// Wine made the prefix; the first start the record sees is Proton's.
    /// The prefix still predates the record, and the Wine keeps it.
    #[test]
    fn a_prefix_another_kind_made_still_counts_as_older() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "g");
        std::fs::create_dir_all(prefix_dir(&paths.share_dir).join("drive_c")).unwrap();
        let proton = tmp.path().join("proton");
        let wine = tmp.path().join("wine");
        std::fs::write(&proton, "").unwrap();
        std::fs::write(&wine, "").unwrap();
        let plan = LaunchPlan {
            program: proton,
            args: Vec::new(),
            cwd: tmp.path().to_path_buf(),
            env: BTreeMap::from([(
                "STEAM_COMPAT_DATA_PATH".into(),
                prefix_dir(&paths.share_dir).to_string_lossy().to_string(),
            )]),
            runner: "Proton".into(),
            needs_elevation: false,
            raw_command_line: None,
            wrapper: Vec::new(),
        };
        let existed = own_prefix_exists_before_start(&plan, &paths, "g");
        assert!(existed);
        remember_default_prefix_user(&plan, &paths, "g", existed);
        assert!(pinning_keeps_default_prefix(
            &paths,
            "g",
            &DetectedRunner {
                runner: Runner::Wine,
                program: wine,
                label: String::new(),
                steam_root: None,
            }
        ));
    }

    /// Only a plan that runs in the game's own prefix is recorded, and it
    /// records the tool that ran there — checked on what `plan` builds, so a
    /// change to how it names the prefix cannot quietly stop the record.
    #[test]
    fn the_tool_that_ran_in_the_games_own_prefix_is_recorded() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "g");
        std::fs::create_dir_all(paths.local_dir.join("bin")).unwrap();
        std::fs::write(paths.local_dir.join("bin/game.exe"), "").unwrap();
        let wine = tmp.path().join("wine");
        std::fs::write(&wine, "").unwrap();
        let crossover = tmp.path().join("CrossOver/bin/wine");
        std::fs::create_dir_all(crossover.parent().unwrap()).unwrap();
        std::fs::write(&crossover, "").unwrap();
        let m = Manifest {
            id: "g".into(),
            launch: LaunchSpec {
                exe: "bin/game.exe".into(),
                runner: Runner::Wine,
                ..Default::default()
            },
            ..Default::default()
        };
        let pin = |program: &Path, runner: Runner, shares: bool| {
            let mut settings = Settings::default();
            settings.runner_paths.wine = Some(wine.clone());
            if runner != Runner::Auto {
                settings.set_game_runner(
                    "g",
                    Some(crate::settings::GameRunner {
                        program: program.to_path_buf(),
                        runner,
                        label: String::new(),
                        steam_root: None,
                        shares_default_prefix: shares,
                    }),
                );
            }
            settings
        };
        let run = |settings: &Settings| {
            let plan = plan(&LaunchContext {
                paths: &paths,
                game_id: "g",
                settings,
                manifest: Some(&m),
                receipt: None,
                alternative: None,
            })
            .unwrap();
            remember_default_prefix_user(&plan, &paths, "g", false);
            default_prefix_record(&paths)
                .map(|record| record.users)
                .unwrap_or_default()
        };

        assert_eq!(
            run(&pin(&wine, Runner::Wine, false)),
            vec![],
            "a separate prefix"
        );
        assert_eq!(
            run(&pin(&crossover, Runner::Crossover, false)),
            vec![],
            "a separate bottle"
        );
        let users = run(&pin(&wine, Runner::Auto, false));
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].runner, Runner::Wine);
        assert!(same_program(&users[0].program, &wine));
        let users = run(&pin(&crossover, Runner::Crossover, true));
        assert_eq!(users.len(), 2, "the game's own bottle");
        assert_eq!(users[1].runner, Runner::Crossover);
        assert_eq!(
            run(&pin(&wine, Runner::Auto, false)).len(),
            2,
            "recorded once"
        );
    }

    /// The same for CrossOver, where the prefix is a named bottle: the pinned
    /// tool that Automatic already used keeps the game's `nll-<id>` bottle.
    #[test]
    fn a_crossover_pin_sharing_the_default_keeps_the_games_bottle() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = GamePaths::new(tmp.path(), "g");
        std::fs::create_dir_all(paths.local_dir.join("bin")).unwrap();
        std::fs::write(paths.local_dir.join("bin/game.exe"), "").unwrap();
        let wine = tmp.path().join("CrossOver/bin/wine");
        std::fs::create_dir_all(wine.parent().unwrap()).unwrap();
        std::fs::write(&wine, "").unwrap();
        let m = Manifest {
            id: "g".into(),
            launch: LaunchSpec {
                exe: "bin/game.exe".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let bottle_for = |shares: bool| {
            let mut settings = Settings::default();
            settings.set_game_runner(
                "g",
                Some(crate::settings::GameRunner {
                    program: wine.clone(),
                    runner: Runner::Crossover,
                    label: "CrossOver".into(),
                    steam_root: None,
                    shares_default_prefix: shares,
                }),
            );
            let p = plan(&LaunchContext {
                paths: &paths,
                game_id: "g",
                settings: &settings,
                manifest: Some(&m),
                receipt: None,
                alternative: None,
            })
            .unwrap();
            let at = p.args.iter().position(|a| a == "--bottle").unwrap();
            p.args[at + 1].clone()
        };
        assert_eq!(bottle_for(true), "nll-g");
        assert!(bottle_for(false).starts_with("nll-g-"));
    }

    #[test]
    fn a_choice_saved_before_the_flag_existed_keeps_its_separate_prefix() {
        // Settings written by the branch that introduced pinning know nothing
        // of the flag. Reading them must not move anybody's game.
        let saved =
            r#"{"program":"/usr/bin/wine","runner":"wine","label":"Wine","steamRoot":null}"#;
        let runner: crate::settings::GameRunner = serde_json::from_str(saved).unwrap();
        assert!(!runner.shares_default_prefix);
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
                shares_default_prefix: false,
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

    #[test]
    fn a_dll_override_keeps_what_was_chosen() {
        let mut env = BTreeMap::new();
        env.insert("WINEDLLOVERRIDES".to_string(), "d3d9=n,b;".to_string());
        add_dll_override(&mut env, "lsteamclient", "d");
        assert_eq!(env["WINEDLLOVERRIDES"], "d3d9=n,b;lsteamclient=d");
        // Named already, in a list of its own: left as the profile has it.
        env.insert(
            "WINEDLLOVERRIDES".to_string(),
            "steam_api,lsteamclient=n".to_string(),
        );
        add_dll_override(&mut env, "lsteamclient", "d");
        assert_eq!(env["WINEDLLOVERRIDES"], "steam_api,lsteamclient=n");
    }
}
