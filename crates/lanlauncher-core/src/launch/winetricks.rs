//! Windows components in a game's prefix, installed with winetricks.
//!
//! Many LAN games need something Windows ships separately — DirectPlay, a
//! Visual C++ runtime, d3dx9. A profile names them as winetricks verbs, and
//! the game details offer a button that installs them into the prefix the
//! game starts in. Nothing runs on its own: installing takes minutes and
//! usually needs the internet once (winetricks downloads from Microsoft and
//! archive.org), which a LAN often does not have.
//!
//! The prefix comes from the game's launch plan, so the components land
//! exactly where the game runs: `WINEPREFIX` for Wine, `<compat>/pfx` for
//! Proton (with Proton's own Wine, as protontricks does it). CrossOver keeps
//! its bottles to itself and has its own installer for such components.

use super::LaunchPlan;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The winetricks script and the folders its helpers (cabextract) are in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    pub script: PathBuf,
    /// Put in front of the host's `PATH` for the run.
    pub path_dirs: Vec<PathBuf>,
    /// `wine`, `wine64` and `wineserver` stand-ins that start Proton's own
    /// with Proton's libraries ([`SHIM`]); `None` when they could not be
    /// written.
    pub shims: Option<PathBuf>,
}

/// The names [`SHIM`] is written under.
const SHIM_NAMES: [&str; 3] = ["wine", "wine64", "wineserver"];

/// One script for `wine`, `wine64` and `wineserver`: Proton's binary of the
/// same name with Proton's libraries in front. Only Wine gets them — the
/// downloads, checksums and cabextract winetricks runs are the host's and
/// must load the host's libraries.
const SHIM: &str = "#!/bin/sh\n\
# Written by NextGen LAN Launcher: Proton's Wine with Proton's libraries.\n\
LD_LIBRARY_PATH=\"$NLL_PROTON_LIBS\" exec \"$NLL_PROTON_BIN/${0##*/}\" \"$@\"\n";

/// Where components go, and the Wine that installs them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Wine {
        prefix: PathBuf,
        wine: PathBuf,
        /// What the game's start sets of [`CARRIED`]: the same prefix
        /// architecture and the same Wine build.
        carry: BTreeMap<String, String>,
    },
    Proton {
        /// `<STEAM_COMPAT_DATA_PATH>/pfx`.
        prefix: PathBuf,
        wine: PathBuf,
        wineserver: PathBuf,
        /// Proton's own Wine libraries, as `WINEDLLPATH`.
        dll_paths: Vec<PathBuf>,
        /// Proton's own shared libraries, in front of `LD_LIBRARY_PATH` for
        /// its Wine: it is built against them, not against the host's.
        lib_paths: Vec<PathBuf>,
    },
}

impl Target {
    pub fn prefix(&self) -> &Path {
        match self {
            Target::Wine { prefix, .. } | Target::Proton { prefix, .. } => prefix,
        }
    }
}

/// The variables of a Wine start that winetricks has to see as well: a
/// prefix made as `win32` stays one, and a Wine build of its own needs its
/// own loader, server and libraries.
const CARRIED: [&str; 4] = ["WINEARCH", "WINEDLLPATH", "WINELOADER", "WINESERVER"];

/// Why components cannot be installed for a plan, as an `err.<code>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// CrossOver's bottles: its own "Install Software" does this.
    Crossover,
    /// The game does not run through Wine at all.
    NotWindows,
    /// Proton makes its prefix on the first start; there is nothing to
    /// install into before that.
    StartFirst,
    /// A Proton without the Wine it should carry.
    ProtonLayout(PathBuf),
}

impl Refusal {
    pub fn code(&self) -> String {
        match self {
            Refusal::Crossover => "err.components_crossover".into(),
            Refusal::NotWindows => "err.components_native".into(),
            Refusal::StartFirst => "err.components_start_first".into(),
            Refusal::ProtonLayout(dir) => {
                format!("err.components_proton_layout|{}", dir.display())
            }
        }
    }
}

/// Where the components for `plan` go.
pub fn target(plan: &LaunchPlan) -> Result<Target, Refusal> {
    // A native start keeps a manifest's `WINEPREFIX` in its environment, but
    // its program is the game, not a Wine.
    if plan.runner.starts_with("native") {
        return Err(Refusal::NotWindows);
    }
    if plan.args.first().is_some_and(|a| a == "--bottle") {
        return Err(Refusal::Crossover);
    }
    // By the program, not by `STEAM_COMPAT_DATA_PATH`: a profile's variables
    // reach a Wine start too, and its prefix is still `WINEPREFIX`.
    if plan
        .program
        .file_name()
        .is_some_and(|name| name == "proton")
    {
        let Some(compat) = plan.env.get("STEAM_COMPAT_DATA_PATH") else {
            return Err(Refusal::StartFirst);
        };
        let compat = absolute(Path::new(compat), &plan.cwd);
        let prefix = compat.join("pfx");
        if !prefix.join("drive_c").is_dir() {
            return Err(Refusal::StartFirst);
        }
        let proton_dir = plan.program.parent().unwrap_or(Path::new("/"));
        // `files/` since Proton 5.13, `dist/` before.
        let Some(dist) = ["files", "dist"]
            .iter()
            .map(|d| proton_dir.join(d))
            .find(|d| d.join("bin/wine").is_file())
        else {
            return Err(Refusal::ProtonLayout(proton_dir.to_path_buf()));
        };
        return Ok(Target::Proton {
            prefix,
            wine: dist.join("bin/wine"),
            wineserver: dist.join("bin/wineserver"),
            // Proton 8 and later: `lib/wine`, `lib/vkd3d` and the
            // multiarch folders; before: `lib64` and `lib`.
            dll_paths: ["lib64/wine", "lib/wine", "lib/vkd3d"]
                .iter()
                .map(|d| dist.join(d))
                .filter(|d| d.is_dir())
                .collect(),
            lib_paths: ["lib/x86_64-linux-gnu", "lib/i386-linux-gnu", "lib64", "lib"]
                .iter()
                .map(|d| dist.join(d))
                .filter(|d| d.is_dir())
                .collect(),
        });
    }
    if let Some(prefix) = plan.env.get("WINEPREFIX") {
        let mut carry: BTreeMap<String, String> = CARRIED
            .iter()
            .filter_map(|name| Some((name.to_string(), plan.env.get(*name)?.clone())))
            .collect();
        // Wine finds the server next to itself; winetricks looks for
        // `${WINE}server` and then on `PATH`, which for a `wine64` or a
        // build of its own is another Wine's — "version mismatch".
        if let Some(server) = plan
            .program
            .parent()
            .map(|dir| dir.join("wineserver"))
            .filter(|server| server.is_file())
        {
            carry
                .entry("WINESERVER".into())
                .or_insert_with(|| path_text(&server));
        }
        return Ok(Target::Wine {
            prefix: absolute(Path::new(prefix), &plan.cwd),
            wine: plan.program.clone(),
            carry,
        });
    }
    Err(Refusal::NotWindows)
}

fn absolute(path: &Path, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// What to run: winetricks for the verbs a profile names, once per verb
/// ([`run`] appends each to `args`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub prefix: PathBuf,
    pub verbs: Vec<String>,
    /// The Wine of the prefix, for `wineboot -k` when a run has to end.
    pub wine: PathBuf,
}

/// The host's own values for the variables a job sets, the AppImage's
/// folders taken out: `PATH`, `LD_LIBRARY_PATH`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostEnv {
    pub path: String,
    pub ld_library_path: String,
}

impl HostEnv {
    /// What a child of the machine gets: the values [`super::host_command`]
    /// hands it, so there is one recipe for both.
    pub fn current() -> Self {
        Self {
            path: host_var("PATH"),
            ld_library_path: host_var("LD_LIBRARY_PATH"),
        }
    }
}

/// The run that installs `verbs` into `target`. Each goes to winetricks,
/// which skips what its log says is installed and applies settings verbs
/// (`winxp`, `sound=alsa`) again — those are states a later verb may have
/// changed. `force` reinstalls even what is recorded, for a prefix a Proton
/// upgrade has overwritten.
pub fn job(target: &Target, verbs: &[String], force: bool, tools: &Tools, host: &HostEnv) -> Job {
    let mut env = BTreeMap::new();
    env.insert("WINEPREFIX".into(), path_text(target.prefix()));
    // No version check against the internet, no prompt about it.
    env.insert("WINETRICKS_LATEST_VERSION_CHECK".into(), "disabled".into());
    env.insert("WINEDEBUG".into(), "-all".into());
    let wine = match target {
        Target::Wine { wine, carry, .. } => {
            env.extend(carry.clone());
            env.insert("WINE".into(), path_text(wine));
            wine.clone()
        }
        Target::Proton {
            wine,
            wineserver,
            dll_paths,
            lib_paths,
            ..
        } => {
            // As protontricks runs it: Proton's Wine on Proton's prefix.
            if !dll_paths.is_empty() {
                env.insert("WINEDLLPATH".into(), join(dll_paths.iter(), ""));
            }
            let bin = wine.parent().unwrap_or(Path::new("/"));
            // A stand-in of the same name: Wine starts itself again by it.
            let stand_ins = tools
                .shims
                .as_ref()
                .filter(|_| !lib_paths.is_empty())
                .and_then(|shims| {
                    let stand_in = |real: &Path| {
                        let name = real.file_name()?.to_str()?;
                        SHIM_NAMES.contains(&name).then(|| shims.join(name))
                    };
                    Some((stand_in(wine)?, stand_in(wineserver)?))
                });
            match stand_ins {
                Some((shim_wine, shim_server)) => {
                    env.insert("WINE".into(), path_text(&shim_wine));
                    env.insert("WINESERVER".into(), path_text(&shim_server));
                    // The real one: Wine finds its preloader next to it,
                    // and the processes it starts inherit the libraries
                    // from the stand-in that started it.
                    env.insert("WINELOADER".into(), path_text(wine));
                    // winetricks reads the architecture from the real
                    // binaries when `WINE` is a wrapper.
                    env.insert("WINE_BIN".into(), path_text(wine));
                    env.insert("WINESERVER_BIN".into(), path_text(wineserver));
                    env.insert("NLL_PROTON_BIN".into(), path_text(bin));
                    env.insert(
                        "NLL_PROTON_LIBS".into(),
                        join(lib_paths.iter(), &host.ld_library_path),
                    );
                    shim_wine
                }
                // Without the stand-ins Proton's Wine runs on the host's
                // libraries: worse for Wine than for everything else
                // winetricks starts, which is why they are not put in front
                // for the whole run.
                _ => {
                    env.insert("WINE".into(), path_text(wine));
                    env.insert("WINELOADER".into(), path_text(wine));
                    env.insert("WINESERVER".into(), path_text(wineserver));
                    wine.clone()
                }
            }
        }
    };
    env.insert("PATH".into(), join(tools.path_dirs.iter(), &host.path));
    let mut args = vec![path_text(&tools.script), "--unattended".to_string()];
    if force {
        args.push("--force".into());
    }
    Job {
        // Through `sh`: a script copied out of a read-only image or a
        // resource folder may have lost its exec bit.
        program: PathBuf::from("sh"),
        args,
        env,
        prefix: target.prefix().to_path_buf(),
        verbs: verbs.to_vec(),
        wine,
    }
}

/// `dirs` in front of `rest` (an existing `PATH`-style value), joined by `:`.
fn join<'a>(dirs: impl Iterator<Item = &'a PathBuf>, rest: &str) -> String {
    dirs.map(|d| path_text(d))
        .chain(std::iter::once(rest.to_string()))
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(":")
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

/// The winetricks to run: the bundled one, copied to `data_dir` first — a
/// resource folder or an AppImage mount may be read-only, and a copied
/// resource may have lost its exec bit — or else one on the host's `PATH`.
/// `None` when there is neither.
///
/// `proton_shims` writes the stand-ins [`Tools::shims`] names; only a
/// Proton target uses them.
pub fn tools(
    resource_dir: Option<&Path>,
    data_dir: &Path,
    host_path: &str,
    proton_shims: bool,
) -> Option<Tools> {
    let bundled = resource_dir
        .map(|r| r.join("winetricks"))
        .filter(|d| d.join("winetricks").is_file());
    let shim_dir = data_dir.join("tools").join("proton-shims");
    let shims = match proton_shims.then(|| write_shims(&shim_dir)) {
        Some(Ok(())) => Some(shim_dir),
        Some(Err(e)) => {
            log::warn!("Proton stand-ins could not be written: {e}");
            None
        }
        None => None,
    };
    if let Some(bundled) = bundled {
        let copy = data_dir.join("tools").join("winetricks");
        match copy_tree(&bundled, &copy) {
            Ok(()) => {
                let bin = copy.join("bin");
                return Some(Tools {
                    script: copy.join("winetricks"),
                    path_dirs: if bin.is_dir() { vec![bin] } else { Vec::new() },
                    shims,
                });
            }
            Err(e) => log::warn!(
                "bundled winetricks could not be copied to {}: {e}",
                copy.display()
            ),
        }
    }
    std::env::split_paths(host_path)
        .map(|dir| dir.join("winetricks"))
        .find(|p| p.is_file())
        .map(|script| Tools {
            script,
            path_dirs: Vec::new(),
            shims,
        })
}

fn write_shims(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for name in SHIM_NAMES {
        let path = dir.join(name);
        if std::fs::read(&path).ok().as_deref() != Some(SHIM.as_bytes()) {
            std::fs::write(&path, SHIM)?;
        }
        make_runnable(&path)?;
    }
    Ok(())
}

fn make_runnable(_path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(_path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// Copy `from` into `to`, files made readable and executable for the user,
/// and what `from` no longer has taken out of `to`: a helper an older
/// release bundled must not stay in front of `PATH`.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    let shipped: Vec<std::ffi::OsString> = std::fs::read_dir(from)?
        .map(|e| e.map(|e| e.file_name()))
        .collect::<Result<_, _>>()?;
    for entry in std::fs::read_dir(to)? {
        let entry = entry?;
        if !shipped.contains(&entry.file_name()) {
            if entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(entry.path())?;
            } else {
                std::fs::remove_file(entry.path())?;
            }
        }
    }
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            // Unchanged files stay: a run in progress may be executing them.
            // The size first, so a changed file costs no second read.
            let same = std::fs::metadata(&target)
                .is_ok_and(|old| old.len() == entry.metadata().map_or(u64::MAX, |new| new.len()))
                && std::fs::read(&target)
                    .is_ok_and(|old| std::fs::read(entry.path()).is_ok_and(|new| new == old));
            if !same {
                std::fs::copy(entry.path(), &target)?;
            }
            make_runnable(&target)?;
        }
    }
    Ok(())
}

/// How a run went, read from the prefix and from what winetricks said.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// Verbs the prefix has now, of those asked for.
    pub installed: Vec<String>,
    /// Verbs asked for and still missing.
    pub failed: Vec<String>,
    /// A download failed: no internet, or a source that is gone. winetricks
    /// says the same for both.
    pub offline: bool,
    /// A helper program winetricks needs and did not find (`cabextract`).
    pub missing_tool: Option<String>,
    /// The run was ended after its time limit (an installer waiting for a
    /// click nobody saw).
    pub timed_out: bool,
}

/// Read the outcome of a run: the verbs whose own winetricks call ended
/// well are in, the rest is not — the exit status, not the prefix's
/// `winetricks.log`, which records a verb before winetricks checks it went
/// in, and an alias or a DLL override under another name. The output says
/// why something is missing.
pub fn outcome(output: &str, installed: Vec<String>, failed: Vec<String>) -> Outcome {
    let offline = output
        .lines()
        .any(|l| l.contains("Downloading ") && l.trim_end().ends_with("failed"));
    let missing_tool = output.lines().find_map(|l| {
        let rest = l.split("Cannot find ").nth(1)?;
        let tool = rest.split(['.', ' ']).next()?.trim();
        // Only what winetricks gives up on; for 7z, unzip and unrar it
        // falls back to a Windows 7-Zip on its own.
        (l.contains("Please install") && !tool.is_empty()).then(|| tool.to_string())
    });
    Outcome {
        installed,
        failed,
        offline,
        missing_tool,
        timed_out: false,
    }
}

fn host_var(name: &str) -> String {
    super::host_value(name).unwrap_or_default()
}

/// Wine's own processes, which outlive a game or a run by a few seconds
/// (`wineserver` waits before it goes). A running game always has a process
/// of its own besides them.
/// Linux keeps 15 characters of a name: `winemenubuilder.exe` shows as
/// `winemenubuilder`.
const WINE_SYSTEM: [&str; 14] = [
    "wineserver",
    "wineserver64",
    "services.exe",
    "winedevice.exe",
    "plugplay.exe",
    "svchost.exe",
    "rpcss.exe",
    "explorer.exe",
    "conhost.exe",
    "winemenubuilder.exe",
    "wineboot.exe",
    "start.exe",
    "rundll32.exe",
    "tabtip.exe",
];

fn is_wine_system(name: &str) -> bool {
    WINE_SYSTEM.iter().any(|system| {
        name.eq_ignore_ascii_case(system)
            || (name.len() == 15
                && system
                    .get(..15)
                    .is_some_and(|s| s.eq_ignore_ascii_case(name)))
    })
}

/// Whether something runs in the target's prefix: a process with it as its
/// `WINEPREFIX` — a game, the launcher it left running — or, for Proton, a
/// `proton` still setting it up with `STEAM_COMPAT_DATA_PATH` (it sets
/// `WINEPREFIX` only for what it starts). Asked of the prefix and not of
/// the PID a start returned, because many games' first program starts the
/// real one and ends, and a game started before the launcher was is in no
/// list at all. Where another process' environment cannot be read (a
/// different user, macOS), that process does not count.
pub fn prefix_in_use(target: &Target) -> bool {
    let prefix = target.prefix();
    let mut marks = vec![("WINEPREFIX=", prefix.to_path_buf())];
    if let (Target::Proton { .. }, Some(compat)) = (target, prefix.parent()) {
        marks.push(("STEAM_COMPAT_DATA_PATH=", compat.to_path_buf()));
    }
    let marks: Vec<(&str, PathBuf, PathBuf)> = marks
        .into_iter()
        .map(|(name, path)| {
            let real = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            (name, path, real)
        })
        .collect();
    let sys = crate::transport::resilio::scan_processes_with(
        sysinfo::ProcessRefreshKind::nothing()
            .with_environ(sysinfo::UpdateKind::Always)
            .with_cwd(sysinfo::UpdateKind::Always),
    );
    let in_use = crate::transport::resilio::real_processes(&sys)
        .filter(|(_, process)| !is_wine_system(&process.name().to_string_lossy()))
        .any(|(_, process)| {
            process
                .environ()
                .iter()
                .filter_map(|e| e.to_str())
                .any(|entry| {
                    marks.iter().any(|(name, path, real)| {
                        entry.strip_prefix(name).is_some_and(|value| {
                            let value = Path::new(value.trim_end_matches('/'));
                            // A relative value is the process's own: Wine
                            // resolves it against where it runs.
                            let value = match process.cwd() {
                                Some(cwd) if value.is_relative() => cwd.join(value),
                                _ => value.to_path_buf(),
                            };
                            value == *path
                                || std::fs::canonicalize(&value).is_ok_and(|v| v == *real)
                        })
                    })
                })
        });
    in_use
}

/// The process group of a run, killed once: at the time limit, or when the
/// run is dropped before it ended. Its id is the PID of the group's first
/// process and may be handed out again once that is reaped, so it is
/// forgotten as soon as the run ended or the group was killed.
struct GroupGuard(Option<u32>);

impl GroupGuard {
    fn kill(&mut self) {
        #[cfg(unix)]
        if let Some(group) = self.0.take().and_then(|g| libc::pid_t::try_from(g).ok()) {
            // The group leader's PID is never 0 or 1, so this is never
            // "every process".
            if group > 1 {
                // SAFETY: plain syscall; a group that is gone is ESRCH.
                unsafe { libc::kill(-group, libc::SIGKILL) };
            }
        }
        self.0 = None;
    }
}

impl Drop for GroupGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Run `job`, with its output in `log`, and read the outcome. Gives up
/// after `limit` — an installer waiting for a click nobody sees must not
/// hold the button for ever.
pub async fn run(job: &Job, log: &Path, limit: std::time::Duration) -> std::io::Result<Outcome> {
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::File::create(log)?;
    log::info!(
        "installing {} into {} (log: {})",
        job.verbs.join(" "),
        job.prefix.display(),
        log.display()
    );
    let deadline = tokio::time::Instant::now() + limit;
    let (mut installed, mut failed) = (Vec::new(), Vec::new());
    let mut timed_out = false;
    // One call per verb: its exit status says whether that verb went in.
    // All of them, also after a failure — a setting needs no download, and
    // one missing installer is no reason to leave out the rest.
    for verb in &job.verbs {
        if timed_out {
            failed.push(verb.clone());
            continue;
        }
        let out = std::fs::OpenOptions::new().append(true).open(log)?;
        let err = out.try_clone()?;
        let mut cmd = super::host_command(&job.program);
        cmd.args(&job.args)
            .arg(verb)
            .envs(&job.env)
            .stdin(std::process::Stdio::null())
            .stdout(out)
            .stderr(err)
            .kill_on_drop(true);
        // Its own process group: at the time limit the installer and every
        // Wine process under winetricks go with it, not only `sh`.
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd.spawn()?;
        // Should this future be dropped (the command abandoned), the group
        // goes with it; `kill_on_drop` alone would end only `sh`.
        let mut group = GroupGuard(child.id());
        match tokio::time::timeout_at(deadline, child.wait()).await {
            Ok(status) => {
                let status = status?;
                group.0 = None;
                log::info!(
                    "winetricks {verb} for {} ended: {status}",
                    job.prefix.display()
                );
                if status.success() {
                    installed.push(verb.clone());
                } else {
                    failed.push(verb.clone());
                }
            }
            Err(_) => {
                timed_out = true;
                failed.push(verb.clone());
                log::warn!("winetricks {verb} for {} timed out", job.prefix.display());
                group.kill();
                let _ = child.kill().await;
                // And whatever Wine started that left the group: everything
                // still running in this prefix.
                let mut stop = super::host_command(&job.wine);
                stop.args(["wineboot", "-k"])
                    .envs(&job.env)
                    .stdin(std::process::Stdio::null())
                    .kill_on_drop(true);
                let _ =
                    tokio::time::timeout(std::time::Duration::from_secs(30), stop.status()).await;
            }
        }
    }
    let output = std::fs::read_to_string(log).unwrap_or_default();
    Ok(Outcome {
        timed_out,
        ..outcome(&output, installed, failed)
    })
}

// Components are installed on Linux and macOS only (`sh`, `:`-joined
// lists, process groups), and so are their tests: on Windows a path joins
// with a backslash and none of it runs.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// What winetricks recorded in `prefix`: for checking a real run, not
    /// for judging one (see [`outcome`]).
    fn installed(prefix: &Path) -> Vec<String> {
        std::fs::read_to_string(prefix.join("winetricks.log"))
            .map(|text| {
                text.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn plan(program: &Path, env: &[(&str, &str)], args: &[&str]) -> LaunchPlan {
        LaunchPlan {
            program: program.to_path_buf(),
            args: args.iter().map(|a| a.to_string()).collect(),
            cwd: PathBuf::from("/games/q3/local"),
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            runner: String::new(),
            needs_elevation: false,
            raw_command_line: None,
            wrapper: Vec::new(),
        }
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn the_target_is_the_prefix_the_game_starts_in() {
        let tmp = tempfile::tempdir().unwrap();
        let wine = tmp.path().join("wine");
        let mut wine_plan = plan(
            &wine,
            &[
                ("WINEPREFIX", "/games/q3/.nll-prefix"),
                ("WINEARCH", "win32"),
                ("DXVK_HUD", "1"),
            ],
            &[],
        );
        wine_plan.runner = "native".into();
        assert_eq!(
            target(&wine_plan),
            Err(Refusal::NotWindows),
            "a native start keeps the variable"
        );
        wine_plan.runner = "Wine".into();
        assert_eq!(
            target(&wine_plan),
            Ok(Target::Wine {
                prefix: PathBuf::from("/games/q3/.nll-prefix"),
                wine: wine.clone(),
                carry: [("WINEARCH".to_string(), "win32".to_string())].into(),
            }),
            "a win32 prefix stays one; the game's other variables are not winetricks' business"
        );

        // A Wine build of its own: its own server, not the one on `PATH`.
        touch(&tmp.path().join("wineserver"));
        let Ok(Target::Wine { carry, .. }) = target(&wine_plan) else {
            panic!("{:?}", target(&wine_plan));
        };
        assert_eq!(
            carry.get("WINESERVER"),
            Some(&tmp.path().join("wineserver").display().to_string())
        );
        std::fs::remove_file(tmp.path().join("wineserver")).unwrap();

        // Proton: its prefix exists only after the first start.
        let proton_dir = tmp.path().join("Proton 9.0");
        let compat = tmp.path().join("compat");
        let proton = plan(
            &proton_dir.join("proton"),
            &[("STEAM_COMPAT_DATA_PATH", compat.to_str().unwrap())],
            &["run", "game.exe"],
        );
        assert_eq!(target(&proton), Err(Refusal::StartFirst));
        std::fs::create_dir_all(compat.join("pfx/drive_c")).unwrap();
        assert_eq!(
            target(&proton),
            Err(Refusal::ProtonLayout(proton_dir.clone()))
        );
        touch(&proton_dir.join("files/bin/wine"));
        // Proton 8 and later.
        for dir in [
            "lib/wine",
            "lib/vkd3d",
            "lib/x86_64-linux-gnu",
            "lib/i386-linux-gnu",
        ] {
            std::fs::create_dir_all(proton_dir.join("files").join(dir)).unwrap();
        }
        let Ok(Target::Proton {
            prefix,
            wine,
            wineserver,
            dll_paths,
            lib_paths,
        }) = target(&proton)
        else {
            panic!("{:?}", target(&proton));
        };
        assert_eq!(prefix, compat.join("pfx"));
        assert_eq!(wine, proton_dir.join("files/bin/wine"));
        assert_eq!(wineserver, proton_dir.join("files/bin/wineserver"));
        let files = proton_dir.join("files");
        assert_eq!(dll_paths, [files.join("lib/wine"), files.join("lib/vkd3d")]);
        assert_eq!(
            lib_paths,
            [
                files.join("lib/x86_64-linux-gnu"),
                files.join("lib/i386-linux-gnu"),
                files.join("lib"),
            ]
        );

        // A profile's Proton variable on a Wine start: still `WINEPREFIX`.
        let mut wine_with_compat = plan(
            &wine,
            &[
                ("WINEPREFIX", "/games/q3/.nll-prefix"),
                ("STEAM_COMPAT_DATA_PATH", compat.to_str().unwrap()),
            ],
            &[],
        );
        wine_with_compat.runner = "Wine".into();
        assert!(matches!(target(&wine_with_compat), Ok(Target::Wine { .. })));

        let crossover = plan(&wine, &[], &["--bottle", "nll-q3"]);
        assert_eq!(target(&crossover), Err(Refusal::Crossover));
        assert_eq!(target(&plan(&wine, &[], &[])), Err(Refusal::NotWindows));
        assert_eq!(
            Refusal::ProtonLayout(PathBuf::from("/p")).code(),
            "err.components_proton_layout|/p"
        );
    }

    #[test]
    fn a_job_hands_winetricks_every_verb_and_the_right_wine() {
        let prefix = PathBuf::from("/games/q3/.nll-prefix");
        let target = Target::Wine {
            prefix: prefix.clone(),
            wine: PathBuf::from("/usr/bin/wine"),
            carry: BTreeMap::new(),
        };
        let tools = Tools {
            script: PathBuf::from("/data/tools/winetricks/winetricks"),
            path_dirs: vec![PathBuf::from("/data/tools/winetricks/bin")],
            shims: Some(PathBuf::from("/data/tools/proton-shims")),
        };
        let host = HostEnv {
            path: "/usr/bin".into(),
            ld_library_path: "/opt/lib".into(),
        };
        let verbs = vec!["winxp".to_string(), "directplay".to_string()];

        let job = job(&target, &verbs, false, &tools, &host);
        assert_eq!(job.program, PathBuf::from("sh"));
        assert_eq!(
            job.args,
            ["/data/tools/winetricks/winetricks", "--unattended"],
            "the verb is added per call"
        );
        assert_eq!(job.verbs, verbs);
        assert_eq!(job.env["WINEPREFIX"], "/games/q3/.nll-prefix");
        assert_eq!(job.env["WINE"], "/usr/bin/wine");
        let win32 = Target::Wine {
            prefix: prefix.clone(),
            wine: PathBuf::from("/usr/bin/wine"),
            carry: [("WINEARCH".to_string(), "win32".to_string())].into(),
        };
        assert_eq!(
            super::job(&win32, &verbs, false, &tools, &host).env["WINEARCH"],
            "win32"
        );
        assert_eq!(job.env["PATH"], "/data/tools/winetricks/bin:/usr/bin");
        assert_eq!(job.env["WINETRICKS_LATEST_VERSION_CHECK"], "disabled");
        assert!(
            !job.env.contains_key("LD_LIBRARY_PATH"),
            "the host's own Wine"
        );
        let forced = super::job(&target, &verbs, true, &tools, &host);
        assert_eq!(forced.args[2], "--force");

        let proton = Target::Proton {
            prefix,
            wine: PathBuf::from("/p/files/bin/wine"),
            wineserver: PathBuf::from("/p/files/bin/wineserver"),
            dll_paths: vec![
                PathBuf::from("/p/files/lib64/wine"),
                PathBuf::from("/p/files/lib/wine"),
            ],
            lib_paths: vec![PathBuf::from("/p/files/lib64")],
        };
        let job = super::job(&proton, &verbs, false, &tools, &host);
        assert_eq!(job.env["WINE"], "/data/tools/proton-shims/wine");
        assert_eq!(job.env["WINELOADER"], "/p/files/bin/wine");
        assert_eq!(job.env["WINESERVER"], "/data/tools/proton-shims/wineserver");
        assert_eq!(job.env["WINE_BIN"], "/p/files/bin/wine");
        assert_eq!(job.env["WINESERVER_BIN"], "/p/files/bin/wineserver");
        assert_eq!(job.env["NLL_PROTON_BIN"], "/p/files/bin");
        assert_eq!(job.env["NLL_PROTON_LIBS"], "/p/files/lib64:/opt/lib");
        assert_eq!(
            job.env["WINEDLLPATH"],
            "/p/files/lib64/wine:/p/files/lib/wine"
        );
        assert!(
            !job.env.contains_key("LD_LIBRARY_PATH"),
            "curl and cabextract keep the host's libraries"
        );
        assert_eq!(job.wine, PathBuf::from("/data/tools/proton-shims/wine"));

        // No stand-ins: Proton's Wine itself, on the host's libraries.
        let bare = Tools {
            shims: None,
            ..tools.clone()
        };
        let job = super::job(&proton, &verbs, false, &bare, &host);
        assert_eq!(job.env["WINE"], "/p/files/bin/wine");
        assert_eq!(job.env["WINESERVER"], "/p/files/bin/wineserver");
        assert!(!job.env.contains_key("LD_LIBRARY_PATH"));
        assert_eq!(job.wine, PathBuf::from("/p/files/bin/wine"));
    }

    #[test]
    fn the_bundled_winetricks_is_copied_out_runnable_and_the_path_is_the_fallback() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let resources = tmp.path().join("res");
        touch(&resources.join("winetricks/winetricks"));
        touch(&resources.join("winetricks/bin/cabextract"));
        touch(&resources.join("winetricks/bin/lib/libmspack.so.0"));
        let data = tmp.path().join("data");
        let found = tools(Some(&resources), &data, "", true).unwrap();
        let copy = data.join("tools/winetricks");
        assert_eq!(found.script, copy.join("winetricks"));
        assert_eq!(found.path_dirs, [copy.join("bin")]);
        assert_eq!(found.shims, Some(data.join("tools/proton-shims")));
        let mode = std::fs::metadata(copy.join("bin/cabextract"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o111, 0o111, "runnable after the copy");
        assert!(copy.join("bin/lib/libmspack.so.0").is_file());

        // A helper a later release no longer ships goes from the copy.
        std::fs::remove_file(resources.join("winetricks/bin/cabextract")).unwrap();
        tools(Some(&resources), &data, "", true).unwrap();
        assert!(!copy.join("bin/cabextract").exists());
        assert!(copy.join("bin/lib/libmspack.so.0").is_file());

        // Nothing bundled: the host's own, else none.
        let host = tmp.path().join("usr/bin");
        assert_eq!(tools(None, &data, host.to_str().unwrap(), false), None);
        touch(&host.join("winetricks"));
        let found = tools(None, &data, host.to_str().unwrap(), false).unwrap();
        assert_eq!(found.script, host.join("winetricks"));
        assert!(found.path_dirs.is_empty());
    }

    /// The output is what winetricks 20260125 printed here: a download the
    /// network refused, and the message for a missing cabextract.
    #[test]
    fn the_outcome_says_why_something_is_missing() {
        let v = |verbs: &[&str]| verbs.iter().map(|v| v.to_string()).collect::<Vec<_>>();
        let offline = "Downloading https://files.holarse-linuxgaming.de/mirrors/microsoft/directx_Jun2010_redist.exe to /c/directx9\n\
             Proxy tunneling failed: ForbiddenUnable to establish SSL connection.\n\
             warning: Downloading https://web.archive.org/web/2000/https://files.holarse-linuxgaming.de/mirrors/microsoft/directx_Jun2010_redist.exe failed\n";
        let o = outcome(offline, v(&["winxp"]), v(&["d3dcompiler_43"]));
        assert_eq!(o.installed, ["winxp"]);
        assert_eq!(o.failed, ["d3dcompiler_43"]);
        assert!(o.offline);
        assert_eq!(o.missing_tool, None);

        let no_cab = "warning: Cannot find cabextract.  Please install it (e.g. 'sudo apt install cabextract' or 'sudo yum install cabextract').\n";
        let o = outcome(no_cab, Vec::new(), v(&["vcrun2010"]));
        assert_eq!(o.missing_tool.as_deref(), Some("cabextract"));
        assert!(!o.offline);
        let fallback = "warning: Cannot find 7z. Using Windows 7-Zip instead. (You can avoid this by installing 7z, e.g. 'sudo apt install 7zip')\n";
        assert_eq!(
            outcome(fallback, Vec::new(), Vec::new()).missing_tool,
            None,
            "not fatal"
        );
    }

    /// Each verb is its own call, and its exit status is its outcome — also
    /// after one failed, and whatever the prefix's record says.
    #[tokio::test]
    async fn each_verb_is_judged_by_its_own_call() {
        let tmp = tempfile::tempdir().unwrap();
        let job = Job {
            program: PathBuf::from("sh"),
            // `$1` is the verb: everything but `vcrun2010` goes in.
            args: vec![
                "-c".into(),
                "echo \"Executing w_do_call $1\"; [ \"$1\" != vcrun2010 ]".into(),
                "winetricks".into(),
            ],
            env: BTreeMap::new(),
            prefix: tmp.path().join("pfx"),
            verbs: ["directplay", "vcrun2010", "vd=1024x768"]
                .iter()
                .map(|v| v.to_string())
                .collect(),
            wine: PathBuf::from("true"),
        };
        let log = tmp.path().join("out.log");
        let o = run(&job, &log, std::time::Duration::from_secs(30))
            .await
            .unwrap();
        assert_eq!(o.installed, ["directplay", "vd=1024x768"]);
        assert_eq!(o.failed, ["vcrun2010"]);
        assert!(!o.timed_out);
        assert_eq!(
            std::fs::read_to_string(&log).unwrap().lines().count(),
            3,
            "one log for all calls"
        );
    }

    #[test]
    fn wines_own_processes_are_known_also_by_their_cut_name() {
        assert!(is_wine_system("wineserver"));
        assert!(is_wine_system("winemenubuilder"));
        assert!(is_wine_system("Services.exe"));
        assert!(!is_wine_system("quake3.exe"));
        assert!(!is_wine_system("winemenu"));
    }

    /// A process with the prefix as its `WINEPREFIX` keeps it in use, under
    /// either spelling of the path, and so does a `proton` still setting up
    /// its compat folder; one of another prefix does not, nor a `wineserver`
    /// lingering after the game.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_prefix_is_in_use_while_a_process_runs_in_it() {
        let tmp = tempfile::tempdir().unwrap();
        let wine = |prefix: PathBuf| Target::Wine {
            prefix,
            wine: PathBuf::from("/usr/bin/wine"),
            carry: BTreeMap::new(),
        };
        let prefix = tmp.path().join("pfx");
        std::fs::create_dir_all(&prefix).unwrap();
        let sleep = |program: &Path, name: &str, value: String| {
            std::process::Command::new(program)
                .arg("30")
                .env(name, value)
                .spawn()
                .unwrap()
        };
        let stop = |mut child: std::process::Child| {
            child.kill().unwrap();
            child.wait().unwrap();
        };
        assert!(!prefix_in_use(&wine(prefix.clone())));

        let relative = std::process::Command::new("sleep")
            .arg("30")
            .current_dir(tmp.path())
            .env("WINEPREFIX", "pfx")
            .spawn()
            .unwrap();
        let in_use = prefix_in_use(&wine(prefix.clone()));
        stop(relative);
        assert!(in_use, "relative to where the game runs");

        let game = sleep(
            Path::new("sleep"),
            "WINEPREFIX",
            format!("{}/", prefix.display()),
        );
        let in_use = prefix_in_use(&wine(prefix.clone()));
        let other = prefix_in_use(&wine(tmp.path().join("other")));
        stop(game);
        assert!(in_use);
        assert!(!other);
        assert!(!prefix_in_use(&wine(prefix.clone())));

        let compat = tmp.path().join("compat");
        let proton = Target::Proton {
            prefix: compat.join("pfx"),
            wine: PathBuf::from("/p/files/bin/wine"),
            wineserver: PathBuf::from("/p/files/bin/wineserver"),
            dll_paths: Vec::new(),
            lib_paths: Vec::new(),
        };
        let starting = sleep(
            Path::new("sleep"),
            "STEAM_COMPAT_DATA_PATH",
            compat.display().to_string(),
        );
        let in_use = prefix_in_use(&proton);
        stop(starting);
        assert!(in_use, "proton itself carries only the compat folder");

        let server = tmp.path().join("wineserver");
        let sleep_path = std::env::split_paths(&std::env::var("PATH").unwrap())
            .map(|d| d.join("sleep"))
            .find(|p| p.is_file())
            .unwrap();
        std::fs::copy(sleep_path, &server).unwrap();
        make_runnable(&server).unwrap();
        let lingering = sleep(&server, "WINEPREFIX", prefix.display().to_string());
        let in_use = prefix_in_use(&wine(prefix.clone()));
        stop(lingering);
        assert!(!in_use, "Wine's own processes outlive a game by seconds");
    }

    /// The stand-in starts Proton's binary of its own name with Proton's
    /// libraries, and winetricks around it keeps the host's.
    #[test]
    fn a_proton_stand_in_gives_only_wine_protons_libraries() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("files/bin");
        std::fs::create_dir_all(&bin).unwrap();
        for name in ["wine", "wineserver"] {
            std::fs::write(
                bin.join(name),
                format!("#!/bin/sh\necho \"{name} $LD_LIBRARY_PATH $*\"\n"),
            )
            .unwrap();
            make_runnable(&bin.join(name)).unwrap();
        }
        let shims = tmp.path().join("shims");
        write_shims(&shims).unwrap();
        write_shims(&shims).unwrap();
        let run = |name: &str| {
            let out = std::process::Command::new(shims.join(name))
                .args(["cmd", "/c", "ver"])
                .env("NLL_PROTON_BIN", &bin)
                .env("NLL_PROTON_LIBS", "/p/lib:/host/lib")
                .env("LD_LIBRARY_PATH", "/host/lib")
                .output()
                .unwrap();
            String::from_utf8(out.stdout).unwrap()
        };
        assert_eq!(run("wine"), "wine /p/lib:/host/lib cmd /c ver\n");
        assert_eq!(
            run("wineserver"),
            "wineserver /p/lib:/host/lib cmd /c ver\n"
        );
    }

    /// End to end with a real Wine and winetricks: `winxp` is a setting,
    /// no download, and lands in the prefix's record. Needs `wine`,
    /// `xvfb-run` and `NLL_TEST_WINETRICKS` pointing at the script.
    #[tokio::test]
    #[ignore = "needs wine, xvfb-run and NLL_TEST_WINETRICKS"]
    async fn a_real_winetricks_run_installs_into_the_prefix() {
        let script = PathBuf::from(std::env::var("NLL_TEST_WINETRICKS").unwrap());
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("pfx");
        let target = Target::Wine {
            prefix: prefix.clone(),
            wine: PathBuf::from("/usr/bin/wine"),
            carry: BTreeMap::new(),
        };
        let tools = Tools {
            script,
            path_dirs: Vec::new(),
            shims: None,
        };
        let mut job = job(
            &target,
            &["winxp".to_string()],
            false,
            &tools,
            &HostEnv::current(),
        );
        // Headless here; a desktop has a display of its own.
        let mut args = vec!["-a".to_string(), "sh".to_string()];
        args.extend(job.args.clone());
        job.program = PathBuf::from("xvfb-run");
        job.args = args;
        let log = tmp.path().join("winetricks.out");
        let o = run(&job, &log, std::time::Duration::from_secs(600))
            .await
            .unwrap();
        assert_eq!(
            o.installed,
            ["winxp"],
            "{}",
            std::fs::read_to_string(&log).unwrap()
        );
        assert!(o.failed.is_empty() && !o.timed_out);
        assert_eq!(installed(&prefix), ["winxp"]);
    }

    /// The same through the Proton stand-ins, with a Proton layout built
    /// from the host's Wine binaries: `NLL_TEST_WINE_BIN` names the folder
    /// holding the real `wine64` and `wineserver` (Debian: `/usr/lib/wine`,
    /// with `wineserver64`). The loader keeps its own name — Wine starts
    /// itself again by it.
    #[tokio::test]
    #[ignore = "needs wine, xvfb-run, NLL_TEST_WINETRICKS and NLL_TEST_WINE_BIN"]
    async fn a_real_winetricks_run_through_the_proton_stand_ins() {
        let script = PathBuf::from(std::env::var("NLL_TEST_WINETRICKS").unwrap());
        let host_bin = PathBuf::from(std::env::var("NLL_TEST_WINE_BIN").unwrap());
        let real = |names: &[&str]| {
            names
                .iter()
                .map(|n| host_bin.join(n))
                .find(|p| p.is_file())
                .unwrap()
        };
        let tmp = tempfile::tempdir().unwrap();
        let files = tmp.path().join("Proton/files");
        std::fs::create_dir_all(files.join("bin")).unwrap();
        std::fs::create_dir_all(files.join("lib/x86_64-linux-gnu")).unwrap();
        for (name, of) in [
            ("wine64", real(&["wine64"])),
            ("wineserver", real(&["wineserver64", "wineserver"])),
        ] {
            std::os::unix::fs::symlink(of, files.join("bin").join(name)).unwrap();
        }
        let prefix = tmp.path().join("compat/pfx");
        let target = Target::Proton {
            prefix: prefix.clone(),
            wine: files.join("bin/wine64"),
            wineserver: files.join("bin/wineserver"),
            dll_paths: Vec::new(),
            lib_paths: vec![files.join("lib/x86_64-linux-gnu")],
        };
        let shims = tmp.path().join("data/tools/proton-shims");
        write_shims(&shims).unwrap();
        let tools = Tools {
            script,
            path_dirs: Vec::new(),
            shims: Some(shims),
        };
        let mut job = job(
            &target,
            &["winxp".to_string()],
            false,
            &tools,
            &HostEnv::current(),
        );
        let mut args = vec!["-a".to_string(), "sh".to_string()];
        args.extend(job.args.clone());
        job.program = PathBuf::from("xvfb-run");
        job.args = args;
        let log = tmp.path().join("winetricks.out");
        let o = run(&job, &log, std::time::Duration::from_secs(600))
            .await
            .unwrap();
        let output = std::fs::read_to_string(&log).unwrap();
        assert_eq!(o.installed, ["winxp"], "{output}");
        assert!(!output.contains("Unknown file arch"), "{output}");
    }

    /// A run past its limit ends with everything it started: `sleep` stands
    /// in for an installer waiting for a click.
    #[tokio::test]
    async fn a_run_past_its_limit_ends_its_whole_process_group() {
        let tmp = tempfile::tempdir().unwrap();
        let marker = tmp.path().join("still-running");
        let job = Job {
            program: PathBuf::from("sh"),
            args: vec![
                "-c".into(),
                format!("(sleep 3; touch {}) & wait", marker.display()),
            ],
            env: BTreeMap::new(),
            prefix: tmp.path().join("pfx"),
            verbs: vec!["vcrun2010".into()],
            wine: PathBuf::from("true"),
        };
        let o = run(
            &job,
            &tmp.path().join("out.log"),
            std::time::Duration::from_millis(300),
        )
        .await
        .unwrap();
        assert!(o.timed_out);
        assert_eq!(o.failed, ["vcrun2010"]);
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        assert!(!marker.exists(), "the child of the child was ended too");
    }
}
