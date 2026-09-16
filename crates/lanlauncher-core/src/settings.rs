//! Persistent user settings.

use crate::error::{Error, Result};
use crate::library::Library;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SETTINGS_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TransportMode {
    /// The launcher runs and controls its own Resilio Sync instance.
    #[default]
    Managed,
    /// The user runs Resilio (or anything else) themselves; the launcher only
    /// watches the library folders and offers keys to copy.
    Folder,
    /// Simulated transport for development and screenshots.
    Demo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    pub library: Library,
    pub player_name: String,
    /// UI language (`de`, `en`).
    pub language: String,
    /// Game language passed to scripts (`de`, `en`, `fr`).
    pub game_language: String,
    pub transport: TransportMode,
    /// Host serving `launcher.ini`; ETI hard-codes `launcher.lan`.
    pub lanpage_host: String,
    /// Allow the launcher to send the ETI statistics beacon.
    pub send_stats: bool,
    /// Only talk to LAN peers, never to trackers/relays on the internet.
    pub lan_mode: bool,
    /// Id of a built-in theme (`src/lib/theme.ts`, mirrored in `themes/`);
    /// `None` = automatic (the LANPage's launcher.css/logo or theme.json,
    /// else the default).
    pub theme: Option<String>,
    /// First-run wizard finished.
    pub setup_complete: bool,
    /// Windows only: elevate to run game scripts (netsh/reg). Off means the
    /// launcher warns instead of failing.
    pub allow_elevation: bool,
    /// Extra folders/paths for Wine/CrossOver/Proton on macOS/Linux.
    pub runner_paths: RunnerPaths,
    /// Resilio listening port (0 = let the engine pick).
    pub sync_port: u16,
    /// Overrides the built-in read-only key of the catalog share
    /// (`eti_launcher`, see `catalog::BUILTIN_CATALOG_KEY`).
    pub catalog_key: Option<String>,
    /// Explicit Resilio Sync binary (e.g. the ETI launcher's `btsync.exe`)
    /// when the automatic search does not find one.
    pub resilio_binary: Option<PathBuf>,
    /// Resilio API key for the documented `/api` surface. Empty = automatic:
    /// the installed ETI client's `sync/config.json`, then `resilio_api_key`
    /// from the LANPage's `launcher.ini`; without any key the web-UI
    /// endpoints with login/password are used.
    pub resilio_api_key: Option<String>,
    /// Diagnostics warnings the user has hidden, by
    /// [`crate::problem::Problem::dismiss_key`]. Sorted and unique.
    pub ignored_problems: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RunnerPaths {
    pub wine: Option<PathBuf>,
    pub crossover_app: Option<PathBuf>,
    pub proton: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            library: Library::default(),
            player_name: String::new(),
            language: "de".into(),
            game_language: "de".into(),
            transport: TransportMode::Managed,
            lanpage_host: "launcher.lan".into(),
            send_stats: true,
            lan_mode: true,
            theme: None,
            setup_complete: false,
            allow_elevation: true,
            runner_paths: RunnerPaths::default(),
            sync_port: 0,
            catalog_key: None,
            resilio_binary: None,
            resilio_api_key: None,
            ignored_problems: Vec::new(),
        }
    }
}

impl Settings {
    /// Hide or show a diagnostics warning again. Returns whether the list
    /// changed, so the caller can skip writing an unchanged file.
    pub fn set_problem_ignored(&mut self, key: &str, ignored: bool) -> bool {
        match (self.ignored_problems.iter().position(|k| k == key), ignored) {
            (None, true) => {
                self.ignored_problems.push(key.to_string());
                self.ignored_problems.sort();
                true
            }
            (Some(i), false) => {
                self.ignored_problems.remove(i);
                true
            }
            _ => false,
        }
    }

    pub fn problem_ignored(&self, key: &str) -> bool {
        self.ignored_problems.iter().any(|k| k == key)
    }

    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let mut s: Settings = serde_json::from_str(&text)?;
                s.migrate();
                Ok(s)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(Error::io(path, e)),
        }
    }

    /// Write atomically (temp file + rename) so a crash never leaves a
    /// half-written settings file behind.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&tmp, json).map_err(|e| Error::io(&tmp, e))?;
        std::fs::rename(&tmp, path).map_err(|e| Error::io(path, e))?;
        Ok(())
    }

    fn migrate(&mut self) {
        if self.version < SETTINGS_VERSION {
            self.version = SETTINGS_VERSION;
        }
        if self.language.is_empty() {
            self.language = "de".into();
        }
        if !matches!(self.game_language.as_str(), "de" | "en" | "fr") {
            self.game_language = "en".into();
        }
        // Not a setting any more (the ETI client has none either): the
        // LANPage is expected at its fixed host. Old settings files may still
        // carry another value.
        self.lanpage_host = "launcher.lan".into();
        self.normalise_catalog_key();
        self.normalise_resilio_binary();
        self.normalise_resilio_api_key();
    }

    /// Trim the API key and turn an empty value into `None` (= automatic).
    /// Shared by `migrate` and the settings command so both paths agree.
    pub fn normalise_resilio_api_key(&mut self) {
        self.resilio_api_key = self
            .resilio_api_key
            .take()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
    }

    /// An empty or whitespace-only binary path means "search automatically".
    pub fn normalise_resilio_binary(&mut self) {
        self.resilio_binary = self
            .resilio_binary
            .take()
            .map(|p| PathBuf::from(p.to_string_lossy().trim()))
            .filter(|p| !p.as_os_str().is_empty());
    }

    /// Trim the catalog key override and turn an empty value into `None`.
    /// Shared by `migrate` and the settings command so both paths agree.
    pub fn normalise_catalog_key(&mut self) {
        self.catalog_key = self
            .catalog_key
            .take()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
    }

    /// Sanitised player name for scripts and the stats beacon: no quotes,
    /// no control characters, max 32 chars.
    pub fn safe_player_name(&self) -> String {
        let cleaned: String = self
            .player_name
            .chars()
            .filter(|c| !c.is_control() && !matches!(c, '"' | '%' | '&' | '|' | '<' | '>' | '^'))
            .take(32)
            .collect();
        let t = cleaned.trim().to_string();
        if t.is_empty() {
            "Player".to_string()
        } else {
            t
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn ignoring_a_problem_is_idempotent_and_reversible() {
        let mut s = Settings::default();
        assert!(s.set_problem_ignored("network.public_profile_secondary:WLAN", true));
        assert!(!s.set_problem_ignored("network.public_profile_secondary:WLAN", true));
        assert!(s.problem_ignored("network.public_profile_secondary:WLAN"));
        assert!(!s.problem_ignored("network.public_profile_secondary:Ethernet"));
        assert!(s.set_problem_ignored("network.public_profile_secondary:WLAN", false));
        assert!(s.ignored_problems.is_empty());
        assert!(!s.set_problem_ignored("never.ignored", false));
    }
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("cfg").join("settings.json");
        let loaded = Settings::load(&p).unwrap();
        assert_eq!(loaded, Settings::default());
        let mut s = loaded;
        s.player_name = "Tester".into();
        s.save(&p).unwrap();
        assert_eq!(Settings::load(&p).unwrap().player_name, "Tester");
        assert!(!p.with_extension("json.tmp").exists());
    }

    #[test]
    fn migrates_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(
            &p,
            r#"{"version":0,"language":"","gameLanguage":"xx","lanpageHost":" ","catalogKey":"  ","resilioApiKey":"  "}"#,
        )
        .unwrap();
        let s = Settings::load(&p).unwrap();
        assert_eq!(s.version, SETTINGS_VERSION);
        assert_eq!(s.language, "de");
        assert_eq!(s.game_language, "en");
        assert_eq!(s.lanpage_host, "launcher.lan");
        assert_eq!(s.catalog_key, None);
        assert_eq!(s.resilio_api_key, None);
    }

    #[test]
    fn catalog_key_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        let s = Settings {
            catalog_key: Some(" BABCDEFGHIJKLMNOPQRSTUVWXYZ234567 ".into()),
            ..Settings::default()
        };
        s.save(&p).unwrap();
        assert_eq!(
            Settings::load(&p).unwrap().catalog_key.as_deref(),
            Some("BABCDEFGHIJKLMNOPQRSTUVWXYZ234567")
        );
    }

    #[test]
    fn player_name_is_sanitised() {
        let mut s = Settings {
            player_name: " Bad\"Name%&|<>^\u{7} ".into(),
            ..Default::default()
        };
        assert_eq!(s.safe_player_name(), "BadName");
        s.player_name = "   ".into();
        assert_eq!(s.safe_player_name(), "Player");
    }
}
