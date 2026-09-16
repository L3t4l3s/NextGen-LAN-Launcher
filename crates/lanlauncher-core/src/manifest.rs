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
    pub required_files: Vec<String>,
    /// Extra environment variables.
    pub env: BTreeMap<String, String>,
    /// Alternative entry points (e.g. several games in one package).
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
    pub copy: Vec<CopyStep>,
    /// Files created empty (e.g. `bin/steam_settings/disable_overlay.txt`).
    pub touch: Vec<String>,
    /// Human-readable notes by language.
    pub notes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct PlatformOverride {
    pub exe: Option<String>,
    pub args: Option<Vec<String>>,
    pub runner: Option<Runner>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Manifest {
    pub schema: u32,
    pub id: String,
    pub title: Option<String>,
    /// Package revisions this manifest was verified against. Empty = any.
    pub revisions: Vec<String>,
    pub source: Option<String>,
    /// Where this manifest came from (filled at load time).
    #[serde(skip)]
    pub origin: ManifestOrigin,
    /// The entry point was taken from `game_start.cmd` because the manifest
    /// itself names none (guidance-only profile). What starts is then a guess,
    /// whatever revisions the notes were written for.
    #[serde(skip)]
    pub exe_from_script: bool,
    pub launch: LaunchSpec,
    pub setup: SetupSpec,
    pub platform: BTreeMap<String, PlatformOverride>,
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

impl Default for Manifest {
    fn default() -> Self {
        Self {
            schema: 1,
            id: String::new(),
            title: None,
            revisions: Vec::new(),
            source: None,
            origin: ManifestOrigin::Bundled,
            exe_from_script: false,
            launch: LaunchSpec::default(),
            setup: SetupSpec::default(),
            platform: BTreeMap::new(),
        }
    }
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
        if !crate::catalog::GAME_ID_RE.is_match(&self.id) {
            return Err(err("invalid id"));
        }
        if !self.launch.exe.is_empty() && !is_safe_relative(&self.launch.exe) {
            return Err(err("launch.exe must be relative to local/"));
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
            if let Some(r) = o.runner {
                spec.runner = r;
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

/// Loads manifests from a prioritised list of directories.
#[derive(Debug, Clone, Default)]
pub struct ManifestStore {
    /// Highest priority first.
    pub dirs: Vec<(PathBuf, ManifestOrigin)>,
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
        Self { dirs }
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
