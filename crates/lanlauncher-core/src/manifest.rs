//! Cross-platform game manifests.
//!
//! ETI describes how to start a game only in Windows batch files. To start the
//! same packages on macOS and Linux (and to show meaningful information on
//! Windows) we keep small TOML manifests:
//!
//! ```toml
//! schema = 1
//! id = "goldsrc"
//! revisions = ["20240623"]
//! source = "macETI-LAN v0.6.1 (MIT)"
//!
//! [launch]
//! exe = "hl-cs16/SmartSteamLoader.exe"
//! args = ["-game", "cstrike"]
//! required_files = ["hl-cs16/hl.exe"]
//!
//! [[launch.alternatives]]
//! name = "Half-Life"
//! exe = "hl-cs16/SmartSteamLoader.exe"
//! args = ["-game", "valve"]
//!
//! [setup]
//! copy = [{ from = "SmartSteamEmu.ini", to = "hl-cs16/SmartSteamEmu.ini" }]
//! ```
//!
//! Resolution order: user override → organiser overlay inside the game share
//! (`nll-manifest.toml`) → bundled manifest → manifest derived from
//! `game_start.cmd` (see [`crate::script_probe`]).

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const SHARE_MANIFEST_FILE: &str = "nll-manifest.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Runner {
    /// Pick the best available runner for the platform.
    #[default]
    Auto,
    /// Windows executable through Wine (Linux/macOS).
    Wine,
    /// Windows executable through CrossOver (macOS).
    Crossover,
    /// Windows executable through Proton (Linux, Steam Deck).
    Proton,
    /// Native executable / .app for the current platform.
    Native,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct LaunchSpec {
    /// Executable relative to the extracted `local/` folder.
    pub exe: String,
    pub args: Vec<String>,
    /// Working directory relative to `local/`; defaults to the exe's folder.
    pub workdir: Option<String>,
    pub runner: Runner,
    /// Files that must exist after extraction for the install to count as complete.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub required_files: Vec<String>,
    /// Extra environment variables.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Programs the start runs through: `["gamemoderun"]`,
    /// `["gamescope", "-w", "1280", "-h", "800", "--"]`. Honoured only from
    /// the user's own and the bundled profiles — a profile from a game share
    /// or one guessed from `game_start.cmd` must not start host programs.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub wrapper: Vec<String>,
    /// Windows components the game needs in its prefix, as winetricks verbs
    /// (`directplay`, `vcrun2010`, `d3dx9`). Installed on request from the
    /// game details, never on their own.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub winetricks: Vec<String>,
    /// Alternative entry points (e.g. several games in one package).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<Alternative>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct Alternative {
    pub name: String,
    pub exe: String,
    pub args: Vec<String>,
    pub workdir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CopyStep {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct SetupSpec {
    /// Files copied inside `local/` after extraction.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub copy: Vec<CopyStep>,
    /// Files created empty (e.g. `bin/steam_settings/disable_overlay.txt`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub touch: Vec<String>,
    /// Human-readable notes by language.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub notes: BTreeMap<String, String>,
    /// Notes for one platform only (`[setup.platform_notes.macos]`, by
    /// language), shown after [`Self::notes`] there: a CrossOver step is
    /// noise on Linux.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub platform_notes: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct PlatformOverride {
    pub exe: Option<String>,
    pub args: Option<Vec<String>>,
    /// `""` means the exe's own folder, also when `[launch]` names another.
    pub workdir: Option<String>,
    pub runner: Option<Runner>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    pub wrapper: Option<Vec<String>>,
    pub winetricks: Option<Vec<String>>,
    /// Variables of `[launch].env` this platform goes without.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unset_env: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub title: Option<String>,
    /// Package revisions this manifest was verified against. Empty = any.
    pub revisions: Vec<String>,
    /// The platforms (`linux`, `macos`, `windows`) on which a profile
    /// without an executable of its own was checked starting through
    /// `game_start.cmd`, for [`Self::revisions`] (which must name them): the
    /// script is then the entry point there, not a guess (see
    /// [`Self::exe_from_script`] and [`Self::verified_for`]).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub script_start_checked: Vec<String>,
    /// What the profile was tested with, per platform (`linux = ["Proton
    /// 11.0"]`, `macos`): shown to the player beside what the game needs,
    /// while the profile is confirmed for the package.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub tested_with: BTreeMap<String, Vec<String>>,
    pub source: Option<String>,
    /// Where this manifest came from (filled at load time).
    #[serde(skip)]
    pub origin: ManifestOrigin,
    /// The entry point was taken from `game_start.cmd` because the manifest
    /// itself names none (guidance-only profile). What starts is then a guess,
    /// whatever revisions the notes were written for.
    #[serde(skip)]
    pub exe_from_script: bool,
    /// The current platform's settings come from the user's own launch
    /// configuration (`<data>/game-configs/<id>.toml`), laid over the
    /// profile at load time.
    #[serde(skip)]
    pub user_config: bool,
    /// The package revision the user's configuration was saved with.
    #[serde(skip)]
    pub config_revision: Option<String>,
    pub launch: LaunchSpec,
    pub setup: SetupSpec,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub platform: BTreeMap<String, PlatformOverride>,
    /// The player's name and language in the game's own settings, set before
    /// every start on every platform (`player_settings`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<crate::player_settings::PlayerSetting>,
    /// Arguments added where the game's start script calls a program, on
    /// macOS and Linux, where the script runs filtered in the prefix
    /// (`launch::setup_script`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub script_args: Vec<ScriptArgs>,
}

/// `[[script_args]]`: what the start script's call of `exe` gets added.
/// AoE II's classic programs, for one, wait behind their full-screen window
/// on an error about intro videos Wine cannot play, unless started with
/// `NOSTARTUP`; the script's menu of which program to start stays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ScriptArgs {
    /// The program's file name (`empires2.exe`), matched in any case and
    /// with or without its extension against what the script calls —
    /// wherever that lies (`age2_x1\age2_x1.exe` matches `age2_x1.exe`).
    /// A program counts where the script starts it as a command: first on
    /// its line, after `&`/`|`, inside a block, after `do`/`else` — not
    /// behind `if …`, `start` or `call`; the log says when an entry found
    /// nothing.
    pub exe: String,
    pub args: Vec<String>,
}

impl ScriptArgs {
    /// Written into a batch file: a file name and plain words only, nothing
    /// cmd would read as an operator, a variable or a block.
    pub fn check(&self) -> std::result::Result<(), &'static str> {
        let name_ok = !self.exe.is_empty()
            && self
                .exe
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._- ".contains(c));
        if !name_ok {
            return Err("script_args.exe must be a plain file name");
        }
        let word_ok = |a: &String| {
            !a.is_empty()
                && a.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_+/.:=,".contains(c))
        };
        if self.args.is_empty() || !self.args.iter().all(word_ok) {
            return Err("script_args.args must be plain words");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ManifestOrigin {
    #[default]
    Bundled,
    UserOverride,
    ShareOverlay,
    DerivedFromScript,
}

impl PlatformOverride {
    /// Nothing set: a block that changes nothing.
    pub fn is_empty(&self) -> bool {
        *self == PlatformOverride::default()
    }

    /// This block over `below` (the profile's own block for the platform):
    /// what is set here wins field by field, what is not stays the profile's.
    /// So a configuration that changes the DLL overrides keeps the exe a
    /// later release fixes.
    pub fn layered_on(&self, below: Option<&PlatformOverride>) -> PlatformOverride {
        let mut out = below.cloned().unwrap_or_default();
        if self.exe.is_some() {
            out.exe = self.exe.clone();
        }
        if self.args.is_some() {
            out.args = self.args.clone();
        }
        if self.workdir.is_some() {
            out.workdir = self.workdir.clone();
        }
        if self.runner.is_some() {
            out.runner = self.runner;
        }
        if self.wrapper.is_some() {
            out.wrapper = self.wrapper.clone();
        }
        if self.winetricks.is_some() {
            out.winetricks = self.winetricks.clone();
        }
        for name in &self.unset_env {
            out.env.remove(name);
            if !out.unset_env.contains(name) {
                out.unset_env.push(name.clone());
            }
        }
        out.env.extend(self.env.clone());
        out
    }
}

/// Log a warning once per key for the whole session: profiles are loaded on
/// every tick of the install manager, and a line per tick buries the log.
fn warn_once(key: String, message: impl FnOnce() -> String) {
    static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if !seen.contains(&key) {
        seen.push(key);
        log::warn!("{}", message());
    }
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            schema: 1,
            id: String::new(),
            title: None,
            revisions: Vec::new(),
            script_start_checked: Vec::new(),
            tested_with: BTreeMap::new(),
            source: None,
            origin: ManifestOrigin::Bundled,
            exe_from_script: false,
            user_config: false,
            config_revision: None,
            launch: LaunchSpec::default(),
            setup: SetupSpec::default(),
            platform: BTreeMap::new(),
            settings: Vec::new(),
            script_args: Vec::new(),
        }
    }
}

/// A winetricks verb as a profile may name one: `directplay`, `vcrun2010`,
/// `sound=alsa`. Never an option (`--force`, `-q`) — the launcher decides
/// how winetricks runs — nothing a shell would read as more than a word, and
/// none of winetricks' own commands: `annihilate` deletes the prefix with
/// the savegames in it (and `--unattended` answers its question with yes),
/// `prefix=` moves the install elsewhere, `shell` or `winecfg` wait for a
/// person, and a `*.verb` file is a script of the profile's choosing.
pub fn is_winetricks_verb(verb: &str) -> bool {
    const COMMANDS: [&str; 20] = [
        "annihilate",
        "apps",
        "attended",
        "benchmarks",
        "dlls",
        "explorer",
        "folder",
        "fonts",
        "help",
        "list",
        "main",
        "prefix",
        "regedit",
        "settings",
        "shell",
        "taskmgr",
        "unattended",
        "uninstaller",
        "winecfg",
        "winecmd",
    ];
    let word = verb.split('=').next().unwrap_or(verb);
    !verb.is_empty()
        && !verb.starts_with('-')
        && verb
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '=' | '+' | '-'))
        && !COMMANDS.contains(&word)
        && !word.starts_with("list-")
        && !matches!(word, "arch" | "wine_misc_exe")
        && !verb.ends_with(".verb")
}

/// Reject relative paths that escape `local/`.
pub fn is_safe_relative(path: &str) -> bool {
    let p = path.replace('\\', "/");
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains(':')
        && !p
            .split('/')
            .any(|seg| seg == ".." || seg.is_empty() && p.len() > 1)
}

impl Manifest {
    /// Whether a setting lies in the Windows user's folders, which on
    /// macOS/Linux are in the game's prefix: it then needs the same plan as
    /// registry values, to make that prefix before the first start.
    pub fn uses_windows_profile(&self) -> bool {
        self.settings.iter().any(|s| {
            s.folder
                .is_some_and(|f| f != crate::player_settings::Folder::Game)
        })
    }

    /// Whether a `[[settings]]` entry sets a registry value.
    pub fn sets_registry(&self) -> bool {
        self.settings
            .iter()
            .any(|s| matches!(s.kind(), Ok(crate::player_settings::Kind::Registry { .. })))
    }

    pub fn parse(text: &str, path: &Path) -> Result<Self> {
        let m: Manifest = toml::from_str(text).map_err(|e| Error::Manifest {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        m.validate(path)?;
        Ok(m)
    }

    fn validate(&self, path: &Path) -> Result<()> {
        let err = |message: &str| Error::Manifest {
            path: path.to_path_buf(),
            message: message.to_string(),
        };
        if self.schema != 1 {
            return Err(err("unsupported schema version"));
        }
        if let Some(bad) = self
            .tested_with
            .iter()
            .find(|(p, with)| {
                // Shown on macOS and Linux; Windows starts the package as it is.
                !matches!(p.as_str(), "linux" | "macos")
                    || with.is_empty()
                    || with.iter().any(|w| {
                        w.is_empty() || w.chars().count() > 40 || w.chars().any(char::is_control)
                    })
            })
            .map(|(p, _)| p)
        {
            return Err(err(&format!(
                "tested_with.{bad}: linux or macos, with one or more short names"
            )));
        }
        if let Some(bad) = self
            .setup
            .platform_notes
            .keys()
            .find(|p| !matches!(p.as_str(), "linux" | "macos" | "windows"))
        {
            return Err(err(&format!(
                "setup.platform_notes: `{bad}` is no platform (linux, macos, windows)"
            )));
        }
        if let Some(bad) = self
            .script_start_checked
            .iter()
            .find(|p| !matches!(p.as_str(), "linux" | "macos" | "windows"))
        {
            return Err(err(&format!(
                "script_start_checked: `{bad}` is no platform (linux, macos, windows)"
            )));
        }
        if !crate::catalog::GAME_ID_RE.is_match(&self.id) {
            return Err(err("invalid id"));
        }
        if !self.launch.exe.is_empty() && !is_safe_relative(&self.launch.exe) {
            return Err(err("launch.exe must be relative to local/"));
        }
        if let Some(bad) = self.settings.iter().find_map(|s| s.kind().err()) {
            return Err(err(bad));
        }
        if let Some(bad) = self.script_args.iter().find_map(|a| a.check().err()) {
            return Err(err(bad));
        }
        let mut names: Vec<String> = self
            .script_args
            .iter()
            .map(|a| {
                a.exe
                    .to_ascii_lowercase()
                    .trim_end_matches(".exe")
                    .to_string()
            })
            .collect();
        names.sort();
        if names.windows(2).any(|w| w[0] == w[1]) {
            return Err(err("script_args: one entry per program"));
        }
        // A working folder may be `local/` itself, but never outside it.
        // A trailing separator (`bin/`) is the same folder.
        let workdir_ok = |w: &Option<String>| {
            w.as_deref().is_none_or(|w| {
                let w = w.trim_end_matches(['/', '\\']);
                w.is_empty() || w == "." || is_safe_relative(w)
            })
        };
        if !workdir_ok(&self.launch.workdir) {
            return Err(err("launch.workdir must be relative to local/"));
        }
        if let Some(bad) = self
            .launch
            .winetricks
            .iter()
            .find(|v| !is_winetricks_verb(v))
        {
            return Err(err(&format!(
                "launch.winetricks: `{bad}` is no winetricks verb"
            )));
        }
        for (name, o) in &self.platform {
            if let Some(bad) = o
                .winetricks
                .iter()
                .flatten()
                .find(|v| !is_winetricks_verb(v))
            {
                return Err(err(&format!(
                    "platform.{name}.winetricks: `{bad}` is no winetricks verb"
                )));
            }
            if o.exe
                .as_deref()
                .is_some_and(|e| !e.is_empty() && !is_safe_relative(e))
            {
                return Err(err(&format!(
                    "platform.{name}.exe must be relative to local/"
                )));
            }
            if !workdir_ok(&o.workdir) {
                return Err(err(&format!(
                    "platform.{name}.workdir must be relative to local/"
                )));
            }
        }
        for a in &self.launch.alternatives {
            if !is_safe_relative(&a.exe) {
                return Err(err("alternative exe must be relative to local/"));
            }
        }
        for c in &self.setup.copy {
            if !is_safe_relative(&c.from) || !is_safe_relative(&c.to) {
                return Err(err("setup.copy paths must be relative to local/"));
            }
        }
        for t in &self.setup.touch {
            if !is_safe_relative(t) {
                return Err(err("setup.touch paths must be relative to local/"));
            }
        }
        for f in &self.launch.required_files {
            if !is_safe_relative(f) {
                return Err(err("required_files must be relative to local/"));
            }
        }
        Ok(())
    }

    /// Whether what starts on `platform` is confirmed for the package
    /// `revision`. A tester's own settings speak for the package they were
    /// saved with. A profile whose entry point had to come from the Windows
    /// script is a guess, whatever revisions its notes name — unless it was
    /// checked starting that way on this platform, for named revisions. A
    /// manifest that is *only* the script says so in its origin already.
    pub fn verified_for(&self, revision: &str, platform: &str) -> bool {
        if self.user_config {
            return self.config_revision.as_deref() == Some(revision);
        }
        if !self.matches_revision(revision) {
            return false;
        }
        if !self.exe_from_script || self.origin == ManifestOrigin::DerivedFromScript {
            return true;
        }
        !self.revisions.is_empty() && self.script_start_checked.iter().any(|p| p == platform)
    }

    /// The notes a player on `platform` reads, in `lang` (else English):
    /// the general ones, then the platform's own.
    pub fn notes_for(&self, platform: &str, lang: &str) -> Option<String> {
        let pick = |by: &BTreeMap<String, String>| by.get(lang).or_else(|| by.get("en")).cloned();
        let general = pick(&self.setup.notes);
        let own = self.setup.platform_notes.get(platform).and_then(pick);
        match (general, own) {
            (Some(g), Some(o)) => Some(format!("{g} {o}")),
            (g, o) => g.or(o),
        }
    }

    pub fn matches_revision(&self, revision: &str) -> bool {
        self.revisions.is_empty() || self.revisions.iter().any(|r| r == revision)
    }

    /// Effective launch spec for the given platform (`windows`, `macos`, `linux`).
    pub fn launch_for(&self, platform: &str) -> LaunchSpec {
        let mut spec = self.launch.clone();
        if let Some(o) = self.platform.get(platform) {
            if let Some(exe) = &o.exe {
                spec.exe = exe.clone();
            }
            if let Some(args) = &o.args {
                spec.args = args.clone();
            }
            if let Some(workdir) = &o.workdir {
                spec.workdir = (!workdir.is_empty()).then(|| workdir.clone());
            }
            if let Some(wrapper) = &o.wrapper {
                spec.wrapper = wrapper.clone();
            }
            if let Some(verbs) = &o.winetricks {
                spec.winetricks = verbs.clone();
            }
            if let Some(r) = o.runner {
                spec.runner = r;
            }
            for name in &o.unset_env {
                spec.env.remove(name);
            }
            spec.env.extend(o.env.clone());
        }
        spec
    }

    pub fn current_platform() -> &'static str {
        if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        }
    }
}

/// Variables a game share's profile may not set: they make the host load
/// or run code of the share's choosing directly — a library, a Python
/// module for Proton's launcher, a Wine server, a Vulkan layer, a GStreamer
/// plugin — instead of a Windows program inside Wine. Wine is no sandbox,
/// so this does not make a share's content harmless; it keeps a profile
/// from adding a way around Wine to whatever the share already ships.
const HOST_HOOK_ENV: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "PATH",
    "PYTHONPATH",
    "PYTHONHOME",
    "PYTHONSTARTUP",
    "WINESERVER",
    "WINELOADER",
    "WINEDLLPATH",
    "VK_ADD_LAYER_PATH",
    "VK_LAYER_PATH",
    "VK_ICD_FILENAMES",
    "VK_DRIVER_FILES",
    "GST_PLUGIN_PATH",
    "GST_PLUGIN_PATH_1_0",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
    "LIBGL_DRIVERS_PATH",
    "__EGL_VENDOR_LIBRARY_FILENAMES",
    "DYLD_FALLBACK_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
    "BASH_ENV",
    "ENV",
    "GCONV_PATH",
    "LIBVA_DRIVERS_PATH",
    "GTK_PATH",
    "GTK_MODULES",
    "GIO_MODULE_DIR",
    // `cxbottle` is a Perl program.
    "PERL5LIB",
    "PERL5OPT",
    "PERLLIB",
    "NODE_OPTIONS",
    "RUBYLIB",
];

/// A tester's launch configuration for a game: `[platform.<os>]` blocks
/// only, laid over the profile in force when it is loaded. Kept apart from
/// the profiles so that a fix to the bundled or organiser profile — notes,
/// setup steps, another platform — still reaches a machine that has one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct ConfigOverlay {
    pub schema: u32,
    pub id: String,
    /// The package revision it was last saved with: a configuration speaks
    /// for that package, not for the one an update brings.
    pub revision: Option<String>,
    pub platform: BTreeMap<String, PlatformOverride>,
}

impl Default for ConfigOverlay {
    fn default() -> Self {
        Self {
            schema: 1,
            id: String::new(),
            revision: None,
            platform: BTreeMap::new(),
        }
    }
}

impl ConfigOverlay {
    pub fn parse(text: &str, path: &Path, game_id: &str) -> Result<Self> {
        let overlay: ConfigOverlay = toml::from_str(text).map_err(|e| Error::Manifest {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        if overlay.id != game_id {
            return Err(Error::Manifest {
                path: path.to_path_buf(),
                message: format!(
                    "configuration id `{}` does not match game `{game_id}`",
                    overlay.id
                ),
            });
        }
        // The same path rules as a profile: checked on the profile it makes.
        let probe = Manifest {
            id: game_id.to_string(),
            launch: LaunchSpec {
                exe: "probe.exe".into(),
                ..Default::default()
            },
            platform: overlay.platform.clone(),
            ..Default::default()
        };
        probe.validate(path)?;
        Ok(overlay)
    }

    /// Lay this configuration's block for `platform` over `manifest`'s.
    pub fn overlay_onto(&self, manifest: &mut Manifest, platform: &str) {
        if let Some(block) = self.platform.get(platform) {
            let merged = block.layered_on(manifest.platform.get(platform));
            manifest.platform.insert(platform.to_string(), merged);
            manifest.user_config = true;
            manifest.config_revision = self.revision.clone();
        }
    }
}

impl Manifest {
    /// Take out of a profile that may not have them, once, where it is
    /// loaded, everything that reaches the host directly rather than through
    /// Wine: wrappers, and the variables that load or find host programs and
    /// libraries. A game share's profile is written by whoever fills the
    /// share; a game it starts runs inside Wine, a wrapper or `LD_PRELOAD`
    /// would not. Done here, so no later layer — a configuration without a
    /// wrapper of its own falls back to `[launch]` — can let one through.
    fn drop_host_hooks(&mut self) {
        let hook = |name: &String| HOST_HOOK_ENV.contains(&name.as_str());
        let mut dropped: Vec<String> = Vec::new();
        if !self.launch.wrapper.is_empty()
            || self
                .platform
                .values()
                .any(|o| o.wrapper.as_ref().is_some_and(|w| !w.is_empty()))
        {
            dropped.push("wrapper".into());
        }
        self.launch.wrapper.clear();
        dropped.extend(self.launch.env.keys().filter(|n| hook(n)).cloned());
        self.launch.env.retain(|n, _| !hook(n));
        for block in self.platform.values_mut() {
            block.wrapper = None;
            dropped.extend(block.env.keys().filter(|n| hook(n)).cloned());
            block.env.retain(|n, _| !hook(n));
        }
        if !dropped.is_empty() {
            let id = self.id.clone();
            warn_once(format!("share-hooks:{id}"), || {
                format!(
                    "{id}: the game share's profile sets {}; not used",
                    dropped.join(", ")
                )
            });
        }
    }

    /// Whether this profile may put a host program in front of the start:
    /// the user's own settings and the bundled profiles may, a profile from a
    /// game share — written by whoever fills the share — or one guessed from
    /// `game_start.cmd` may not.
    pub fn wrapper_is_trusted(&self) -> bool {
        self.user_config
            || matches!(
                self.origin,
                ManifestOrigin::UserOverride | ManifestOrigin::Bundled
            )
    }
}

/// Loads manifests from a prioritised list of directories.
#[derive(Debug, Clone, Default)]
pub struct ManifestStore {
    /// Highest priority first.
    pub dirs: Vec<(PathBuf, ManifestOrigin)>,
    /// Where the launch configurations from the game details live
    /// (`<id>.toml`, one [`ConfigOverlay`] each).
    pub config_dir: Option<PathBuf>,
}

impl ManifestStore {
    pub fn new(user_dir: Option<PathBuf>, bundled_dir: Option<PathBuf>) -> Self {
        let mut dirs = Vec::new();
        if let Some(d) = user_dir {
            dirs.push((d, ManifestOrigin::UserOverride));
        }
        if let Some(d) = bundled_dir {
            dirs.push((d, ManifestOrigin::Bundled));
        }
        Self {
            dirs,
            config_dir: None,
        }
    }

    pub fn with_config_dir(mut self, dir: PathBuf) -> Self {
        self.config_dir = Some(dir);
        self
    }

    /// The file a game's launch configuration is stored in.
    pub fn config_path(&self, game_id: &str) -> Option<PathBuf> {
        if !crate::catalog::GAME_ID_RE.is_match(game_id) {
            return None;
        }
        Some(self.config_dir.as_ref()?.join(format!("{game_id}.toml")))
    }

    /// A game's launch configuration for changing it: `Ok(None)` only when
    /// there is none. A file that exists but does not read is an error —
    /// writing over it would drop whatever else it held.
    pub fn load_config_for_update(&self, game_id: &str) -> Result<Option<ConfigOverlay>> {
        let Some(path) = self.config_path(game_id) else {
            return Ok(None);
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => ConfigOverlay::parse(&text, &path, game_id).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::io(&path, e)),
        }
    }

    /// A game's launch configuration, if one was saved and still reads.
    pub fn load_config(&self, game_id: &str) -> Option<ConfigOverlay> {
        let path = self.config_path(game_id)?;
        let text = std::fs::read_to_string(&path).ok()?;
        ConfigOverlay::parse(&text, &path, game_id)
            .inspect_err(|e| {
                warn_once(format!("config:{game_id}"), || {
                    format!("launch configuration for {game_id} ignored: {e}")
                })
            })
            .ok()
    }

    /// Resolve the manifest for a game. `share_dir` is checked for an
    /// organiser overlay between user overrides and bundled manifests.
    pub fn resolve(&self, game_id: &str, share_dir: Option<&Path>) -> Result<Option<Manifest>> {
        let mut candidates: Vec<(PathBuf, ManifestOrigin)> = Vec::new();
        for (dir, origin) in &self.dirs {
            if *origin == ManifestOrigin::UserOverride {
                candidates.push((dir.join(format!("{game_id}.toml")), *origin));
            }
        }
        if let Some(share) = share_dir {
            candidates.push((
                share.join(SHARE_MANIFEST_FILE),
                ManifestOrigin::ShareOverlay,
            ));
        }
        for (dir, origin) in &self.dirs {
            if *origin == ManifestOrigin::Bundled {
                candidates.push((dir.join(format!("{game_id}.toml")), *origin));
            }
        }
        for (path, origin) in candidates {
            if path.is_file() {
                let text = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
                let mut m = Manifest::parse(&text, &path)?;
                if m.id != game_id {
                    return Err(Error::Manifest {
                        path,
                        message: format!("manifest id `{}` does not match game `{game_id}`", m.id),
                    });
                }
                m.origin = origin;
                if origin == ManifestOrigin::ShareOverlay {
                    m.drop_host_hooks();
                }
                return Ok(Some(m));
            }
        }
        Ok(None)
    }

    /// The manifest to use for an installed game: the store's, with the
    /// Windows start script as the fallback — and as the source of the entry
    /// point for a manifest that carries only guidance (`launch.exe` empty,
    /// e.g. a game whose working executable nobody has established yet). Both
    /// the UI and the install manager resolve through this, so "what starts"
    /// and "can it start" never disagree.
    pub fn resolve_for(&self, game_id: &str, paths: &crate::paths::GamePaths) -> Option<Manifest> {
        let profile = self.resolve_profile_for(game_id, paths);
        let platform = Manifest::current_platform();
        // The tester's own settings for this platform, over whatever profile
        // applies — or as the only one, for a game nobody profiled yet.
        match self.load_config(game_id) {
            Some(config) if config.platform.contains_key(platform) => {
                let mut m = profile.unwrap_or_else(|| Manifest {
                    id: game_id.to_string(),
                    origin: ManifestOrigin::UserOverride,
                    ..Default::default()
                });
                config.overlay_onto(&mut m, platform);
                Some(m)
            }
            _ => profile,
        }
    }

    /// [`Self::resolve_for`] without the user's launch configuration: the
    /// profile that configuration is laid over and compared against.
    pub fn resolve_profile_for(
        &self,
        game_id: &str,
        paths: &crate::paths::GamePaths,
    ) -> Option<Manifest> {
        let from_script = || {
            std::fs::read_to_string(&paths.start_script)
                .ok()
                .and_then(|s| crate::script_probe::ScriptProbe::analyse(&s).to_manifest(game_id))
        };
        // The platform's own entry point counts: a profile may name an exe
        // only under `[platform.macos]`, and that is not guidance-only.
        let platform = Manifest::current_platform();
        match self.resolve(game_id, Some(&paths.share_dir)) {
            Ok(Some(mut m)) if m.launch_for(platform).exe.is_empty() => {
                if let Some(probe) = from_script() {
                    m.launch.exe = probe.launch.exe;
                    m.exe_from_script = !m.launch.exe.is_empty();
                    if m.launch.args.is_empty() {
                        m.launch.args = probe.launch.args;
                    }
                    if m.launch.required_files.is_empty() {
                        m.launch.required_files = probe.launch.required_files;
                    }
                    // Without these the "choose another executable" list is
                    // empty for exactly the games whose note says to pick the
                    // multiplayer binary by hand.
                    if m.launch.alternatives.is_empty() {
                        m.launch.alternatives = probe.launch.alternatives;
                    }
                }
                Some(m)
            }
            Ok(Some(m)) => Some(m),
            Ok(None) => from_script(),
            Err(e) => {
                log::warn!("manifest for {game_id} could not be read: {e}");
                from_script()
            }
        }
    }

    /// List all bundled/user manifests (for the UI's compatibility overview).
    pub fn list(&self) -> Vec<Manifest> {
        let mut seen = std::collections::BTreeMap::new();
        for (dir, origin) in self.dirs.iter().rev() {
            let Ok(rd) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in rd.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if let Ok(mut m) = Manifest::parse(&text, &path) {
                        m.origin = *origin;
                        seen.insert(m.id.clone(), m);
                    }
                }
            }
        }
        seen.into_values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A game share's profile loses its wrappers where it is loaded: a
    /// configuration of the tester's without a wrapper of its own must not
    /// fall back to the share's.
    #[test]
    fn a_share_profile_cannot_bring_a_wrapper_along() {
        let share = tempfile::tempdir().unwrap();
        std::fs::write(
            share.path().join(SHARE_MANIFEST_FILE),
            "schema = 1\nid = \"g\"\n[launch]\nexe = \"a.exe\"\nwrapper = [\"/tmp/x\"]\n\
             [launch.env]\nLD_PRELOAD = \"x.so\"\nDXVK_HUD = \"1\"\n\
             [platform.linux]\nwrapper = [\"/tmp/y\"]\n[platform.linux.env]\nPATH = \"/evil\"\n",
        )
        .unwrap();
        let store = ManifestStore::new(None, None);
        let mut m = store.resolve("g", Some(share.path())).unwrap().unwrap();
        assert_eq!(m.origin, ManifestOrigin::ShareOverlay);
        assert!(m.launch.wrapper.is_empty());
        assert!(m.launch_for("linux").wrapper.is_empty());
        let env = m.launch_for("linux").env;
        assert_eq!(
            env.keys().collect::<Vec<_>>(),
            ["DXVK_HUD"],
            "only what stays inside Wine"
        );
        // Even with the tester's own block on top, nothing comes back.
        ConfigOverlay {
            id: "g".into(),
            platform: BTreeMap::from([("linux".into(), PlatformOverride::default())]),
            ..Default::default()
        }
        .overlay_onto(&mut m, "linux");
        assert!(m.wrapper_is_trusted());
        assert!(m.launch_for("linux").wrapper.is_empty());
    }

    #[test]
    fn a_script_start_is_confirmed_only_where_and_for_what_it_was_checked() {
        let parse = |extra: &str| {
            Manifest::parse(
                &format!("schema = 1\nid = \"g\"\n{extra}[launch]\nexe = \"\"\n"),
                Path::new("g.toml"),
            )
        };
        let mut checked =
            parse("revisions = [\"1\"]\nscript_start_checked = [\"linux\"]\n").unwrap();
        checked.exe_from_script = true;
        assert!(checked.verified_for("1", "linux"));
        assert!(!checked.verified_for("1", "macos"));
        assert!(!checked.verified_for("2", "linux"));
        // Checked for no revision in particular: not for every one.
        let mut any = parse("script_start_checked = [\"linux\"]\n").unwrap();
        any.exe_from_script = true;
        assert!(!any.verified_for("1", "linux"));
        // Without the claim, a script guess stays unconfirmed; an exe of the
        // profile's own is confirmed by its revisions alone.
        let mut guessed = parse("revisions = [\"1\"]\n").unwrap();
        guessed.exe_from_script = true;
        assert!(!guessed.verified_for("1", "linux"));
        guessed.exe_from_script = false;
        assert!(guessed.verified_for("1", "linux"));
        // Only the script, no profile: its origin says so, no warning on top.
        guessed.exe_from_script = true;
        guessed.origin = ManifestOrigin::DerivedFromScript;
        assert!(guessed.verified_for("1", "linux"));
        assert!(parse("script_start_checked = [\"ubuntu\"]\n").is_err());
    }

    #[test]
    fn tested_with_names_a_platform_and_short_names() {
        let parse = |t: &str| {
            Manifest::parse(
                &format!("schema = 1\nid = \"g\"\n[launch]\nexe = \"g.exe\"\n[tested_with]\n{t}\n"),
                Path::new("g.toml"),
            )
        };
        let m = parse("linux = [\"Proton 11.0\"]").unwrap();
        assert_eq!(m.tested_with["linux"], vec!["Proton 11.0".to_string()]);
        assert!(parse("steamos = [\"Proton 11.0\"]").is_err());
        assert!(parse("linux = [\"\"]").is_err());
        assert!(parse("linux = []").is_err());
        assert!(parse("windows = [\"Windows 11\"]").is_err());
    }

    #[test]
    fn a_platform_reads_the_general_notes_and_its_own() {
        let m = Manifest::parse(
            "schema = 1\nid = \"g\"\n[launch]\nexe = \"g.exe\"\n[setup]\nnotes.de = \"Alle.\"\n\
             [setup.platform_notes.macos]\nde = \"Mac.\"\nen = \"Mac (en).\"\n",
            Path::new("g.toml"),
        )
        .unwrap();
        assert_eq!(m.notes_for("macos", "de").as_deref(), Some("Alle. Mac."));
        assert_eq!(m.notes_for("linux", "de").as_deref(), Some("Alle."));
        assert_eq!(m.notes_for("macos", "fr").as_deref(), Some("Mac (en)."));
    }

    #[test]
    fn winetricks_verbs_are_words_and_never_options() {
        for ok in [
            "directplay",
            "vcrun2010",
            "d3dx9_43",
            "sound=alsa",
            "dotnet4.8",
            "vc++",
        ] {
            assert!(is_winetricks_verb(ok), "{ok}");
        }
        for bad in [
            "",
            "--force",
            "-q",
            "a b",
            "x;rm",
            "$(id)",
            "a/b",
            "annihilate",
            "shell",
            "winecmd",
            "prefix=other",
            "arch=win32",
            "list-installed",
            "evil.verb",
            "winecfg",
        ] {
            assert!(!is_winetricks_verb(bad), "{bad}");
        }
        let text =
            "schema = 1\nid = \"g\"\n[launch]\nexe = \"a.exe\"\nwinetricks = [\"--force\"]\n";
        assert!(Manifest::parse(text, Path::new("g.toml")).is_err());
    }

    #[test]
    fn platform_paths_outside_local_are_refused() {
        let base = "schema = 1\nid = \"g\"\n[launch]\nexe = \"a.exe\"\n";
        for bad in [
            "[platform.linux]\nexe = \"../a.exe\"\n",
            "[platform.linux]\nworkdir = \"/etc\"\n",
            "[launch.env]\n[platform.macos]\nworkdir = \"../..\"\n",
        ] {
            let text = format!("{base}{bad}");
            assert!(
                Manifest::parse(&text, Path::new("g.toml")).is_err(),
                "{bad}"
            );
        }
        let fine =
            format!("{base}[platform.linux]\nworkdir = \".\"\nwrapper = [\"gamemoderun\"]\n");
        let m = Manifest::parse(&fine, Path::new("g.toml")).unwrap();
        assert_eq!(m.launch_for("linux").wrapper, ["gamemoderun"]);
        assert!(m.launch_for("macos").wrapper.is_empty());
    }

    const GOLDSRC: &str = r#"
schema = 1
id = "goldsrc"
revisions = ["20240623"]
source = "macETI-LAN"

[launch]
exe = "hl-cs16/SmartSteamLoader.exe"
args = ["-game", "cstrike"]
required_files = ["hl-cs16/hl.exe", "hl-cs16/cstrike/liblist.gam"]

[[launch.alternatives]]
name = "Half-Life"
exe = "hl-cs16/SmartSteamLoader.exe"
args = ["-game", "valve"]

[setup]
copy = [{ from = "SmartSteamEmu.ini", to = "hl-cs16/SmartSteamEmu.ini" }]

[platform.macos]
runner = "crossover"
"#;

    #[test]
    fn parses_and_applies_platform_override() {
        let m = Manifest::parse(GOLDSRC, Path::new("goldsrc.toml")).unwrap();
        assert_eq!(m.launch.alternatives.len(), 1);
        assert!(m.matches_revision("20240623"));
        assert!(!m.matches_revision("20990101"));
        assert_eq!(m.launch_for("linux").runner, Runner::Auto);
        assert_eq!(m.launch_for("macos").runner, Runner::Crossover);
        assert_eq!(m.launch_for("macos").args, vec!["-game", "cstrike"]);
    }

    #[test]
    fn rejects_path_escapes() {
        let bad = GOLDSRC.replace("hl-cs16/SmartSteamLoader.exe", "../../evil.exe");
        assert!(Manifest::parse(&bad, Path::new("x.toml")).is_err());
        let bad = GOLDSRC.replace(
            r#"to = "hl-cs16/SmartSteamEmu.ini""#,
            r#"to = "C:/Windows/x.ini""#,
        );
        assert!(Manifest::parse(&bad, Path::new("x.toml")).is_err());
        assert!(is_safe_relative("Among Us.exe"));
        assert!(is_safe_relative("bin\\x64\\game.exe"));
        assert!(!is_safe_relative("/abs"));
    }

    #[test]
    fn resolution_order_user_share_bundled() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("user");
        let bundled = tmp.path().join("bundled");
        let share = tmp.path().join("share");
        for d in [&user, &bundled, &share] {
            std::fs::create_dir_all(d).unwrap();
        }
        let mk = |exe: &str| GOLDSRC.replace("hl-cs16/SmartSteamLoader.exe", exe);
        std::fs::write(bundled.join("goldsrc.toml"), mk("bundled.exe")).unwrap();
        let store = ManifestStore::new(Some(user.clone()), Some(bundled.clone()));
        assert_eq!(
            store
                .resolve("goldsrc", Some(&share))
                .unwrap()
                .unwrap()
                .launch
                .exe,
            "bundled.exe"
        );
        std::fs::write(share.join(SHARE_MANIFEST_FILE), mk("share.exe")).unwrap();
        let m = store.resolve("goldsrc", Some(&share)).unwrap().unwrap();
        assert_eq!(m.launch.exe, "share.exe");
        assert_eq!(m.origin, ManifestOrigin::ShareOverlay);
        std::fs::write(user.join("goldsrc.toml"), mk("user.exe")).unwrap();
        assert_eq!(
            store
                .resolve("goldsrc", Some(&share))
                .unwrap()
                .unwrap()
                .origin,
            ManifestOrigin::UserOverride
        );
        assert!(store.resolve("unknown", None).unwrap().is_none());
        assert_eq!(store.list().len(), 1);
    }
}
