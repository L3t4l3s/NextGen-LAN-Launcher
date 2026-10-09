//! The player's name and language in a game's own settings, on every
//! platform, before every start.
//!
//! ETI's start scripts do some of this with `fnr.exe`, and not always right:
//! Unreal Tournament 2004's writes `Name=` into `UT2004.ini` — into every key
//! ending in it — while the game reads the player's name from `User.ini`.
//! A profile names the exact places instead (`[[settings]]`), and packages
//! built on the Goldberg Steam emulator get theirs without one
//! ([`goldberg`]). This comes on top of the start script, not instead of it.
//!
//! What a profile can name:
//!
//! ```toml
//! [[settings]]                 # an INI key, in its section
//! file = "System/User.ini"
//! section = "DefaultPlayer"
//! key = "Name"
//! value = "%player%"
//!
//! [[settings]]                 # a config line: `seta name "Player"`
//! file = "baseq3/q3config.cfg"
//! line = "seta name"
//! value = "%player%"
//!
//! [[settings]]                 # a whole file
//! file = "settings/language.txt"
//! value = { de = "german", en = "english", fr = "french" }
//!
//! [[settings]]                 # a registry value (HKCU or HKLM)
//! registry = 'HKCU\Software\Blizzard Entertainment\Warcraft III\String'
//! key = "userlocal"
//! value = "%player%"
//!
//! [[settings]]                 # a DWORD, decimal or 0x…: a licence
//! registry = 'HKCU\Software\Microsoft\Microsoft Games\Age of Empires II: The Conquerors Expansion\1.0\EULA'
//! key = "FIRSTRUN"             # dialog the game would otherwise show
//! type = "dword"               # behind its full-screen window
//! value = "1"
//!
//! [[settings]]                 # a JSON key, in the Windows user's folders
//! folder = "locallow"          # game (local/, the default), appdata,
//! file = "Innersloth/Among Us/player.amogus"   # localappdata, locallow,
//! json = "customization.name"  # documents — in the prefix on macOS/Linux
//! value = "%player%"
//!
//! [[settings]]                 # a JSON number (or `type = "bool"`)
//! folder = "locallow"
//! file = "Innersloth/Among Us/player.amogus"
//! json = "onboarding.privacyPolicyVersion"
//! type = "number"
//! value = "4"
//! ```
//!
//! `value` is text (`%player%`, `%game_lang%` filled in) or one per game
//! language; a language the table does not list leaves the setting alone —
//! a package without German stays as it is rather than broken.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One value a profile sets; see the module documentation for the forms.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct PlayerSetting {
    /// Relative to `local/`, found in any case.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<SettingValue>,
    /// A key in a JSON file, its levels joined by dots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<String>,
    /// Where `file` lies; the game's `local/` when not given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<Folder>,
    /// The value's type; text when not given.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub value_type: Option<ValueType>,
}

/// The folder a setting's `file` is relative to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Folder {
    /// The game's `local/`.
    #[default]
    Game,
    /// `%APPDATA%` (`AppData\Roaming`).
    AppData,
    /// `%LOCALAPPDATA%` (`AppData\Local`).
    LocalAppData,
    /// `AppData\LocalLow`, where Unity games keep their settings.
    LocalLow,
    /// `Documents` (`My Games` and the like).
    Documents,
}

impl Folder {
    /// Below the Windows user's profile folder.
    pub fn below_profile(self) -> Option<&'static str> {
        match self {
            Folder::Game => None,
            Folder::AppData => Some("AppData/Roaming"),
            Folder::LocalAppData => Some("AppData/Local"),
            Folder::LocalLow => Some("AppData/LocalLow"),
            Folder::Documents => Some("Documents"),
        }
    }
}

/// The types a profile can give a value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    /// Text: `REG_SZ` in the registry, a string in JSON.
    #[default]
    #[serde(alias = "sz")]
    Text,
    /// `REG_DWORD`: a number, decimal or `0x…`, as `reg add` takes it.
    Dword,
    /// A JSON number.
    Number,
    /// A JSON `true`/`false`.
    Bool,
}

impl ValueType {
    /// The registry type `reg add` gets (only text and DWORD reach it).
    pub fn reg_name(self) -> &'static str {
        match self {
            ValueType::Dword => "REG_DWORD",
            ValueType::Text | ValueType::Number | ValueType::Bool => "REG_SZ",
        }
    }
}

/// A DWORD as `reg add` takes it: decimal or `0x` hex, 32 bits.
/// Digits only: Rust's parsers take a leading `+`, `reg add` does not.
fn is_dword(text: &str) -> bool {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => {
            !hex.is_empty()
                && hex.chars().all(|c| c.is_ascii_hexdigit())
                && u32::from_str_radix(hex, 16).is_ok()
        }
        None => {
            !text.is_empty()
                && text.chars().all(|c| c.is_ascii_digit())
                && text.parse::<u32>().is_ok()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SettingValue {
    Text(String),
    /// By game language (`de`, `en`, `fr`).
    ByLanguage(BTreeMap<String, String>),
}

/// What a setting does, once checked ([`PlayerSetting::kind`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind<'a> {
    Ini {
        file: &'a str,
        section: Option<&'a str>,
        key: &'a str,
    },
    Line {
        file: &'a str,
        line: &'a str,
    },
    WholeFile {
        file: &'a str,
    },
    Registry {
        key: &'a str,
        name: &'a str,
    },
    Json {
        file: &'a str,
        path: &'a str,
    },
}

impl PlayerSetting {
    /// Which form this is, or why it is none. Checked when a profile loads.
    pub fn kind(&self) -> Result<Kind<'_>, &'static str> {
        let quoted = match &self.value {
            None => return Err("settings: a value is missing"),
            Some(SettingValue::Text(text)) => text.contains('"'),
            Some(SettingValue::ByLanguage(by)) => by.values().any(|v| v.contains('"')),
        };
        // A value goes between quotes into a config line or a batch.
        if quoted {
            return Err("settings: a value must not hold a double quote");
        }
        let all = |ok: fn(&str) -> bool| match &self.value {
            Some(SettingValue::Text(text)) => ok(text.as_str()),
            Some(SettingValue::ByLanguage(by)) => by.values().all(|v| ok(v.as_str())),
            None => false,
        };
        match self.value_type {
            Some(ValueType::Dword) if self.registry.is_none() => {
                return Err("settings: dword is for registry values")
            }
            Some(ValueType::Number | ValueType::Bool) if self.json.is_none() => {
                return Err("settings: number and bool are for JSON keys")
            }
            Some(ValueType::Number) if !all(|v| v.parse::<f64>().is_ok_and(f64::is_finite)) => {
                return Err("settings: a number value must be a number")
            }
            Some(ValueType::Bool) if !all(|v| v == "true" || v == "false") => {
                return Err("settings: a bool value must be true or false")
            }
            _ => {}
        }
        if self.folder.is_some_and(|f| f != Folder::Game) && self.file.is_none() {
            return Err("settings: folder is for files");
        }
        if let Some(path) = &self.json {
            if self.file.is_none()
                || self.section.is_some()
                || self.key.is_some()
                || self.line.is_some()
                || path.split('.').any(str::is_empty)
            {
                return Err("settings: json needs a file and a dotted key, nothing else");
            }
        }
        if self.value_type == Some(ValueType::Dword) && !all(is_dword) {
            return Err("settings: a dword value must be a number (decimal or 0x…)");
        }
        match (
            self.file.as_deref(),
            self.registry.as_deref(),
            self.line.as_deref(),
            self.key.as_deref(),
        ) {
            (Some(file), None, _, _) if !crate::manifest::is_safe_relative(file) => {
                Err("settings: file must be relative to local/")
            }
            (Some(file), None, None, None) if self.json.is_some() => Ok(Kind::Json {
                file,
                path: self.json.as_deref().unwrap_or_default(),
            }),
            (Some(file), None, Some(line), None) if self.section.is_none() => {
                Ok(Kind::Line { file, line })
            }
            (Some(file), None, None, Some(key)) => Ok(Kind::Ini {
                file,
                section: self.section.as_deref(),
                key,
            }),
            (Some(file), None, None, None) if self.section.is_none() => {
                Ok(Kind::WholeFile { file })
            }
            (None, Some(key), None, Some(name))
                if self.section.is_none()
                    // They stand in a batch file between quotes: no quotes,
                    // no variables, no backslash before the closing quote.
                    && !key.contains(['"', '%', '!'])
                    && !name.contains(['"', '%', '!'])
                    && !key.ends_with('\\')
                    && !name.ends_with('\\')
                    && [
                        "HKCU\\",
                        "HKLM\\",
                        "HKEY_CURRENT_USER\\",
                        "HKEY_LOCAL_MACHINE\\",
                    ]
                    .iter()
                    .any(|root| key.to_ascii_uppercase().starts_with(root)) =>
            {
                Ok(Kind::Registry { key, name })
            }
            (None, Some(_), _, _) => {
                Err("settings: registry needs an HKCU or HKLM key and a value name")
            }
            _ => {
                Err("settings: give file (with section/key, line or neither) or registry with key")
            }
        }
    }

    /// The text to set for this player, `None` where the table has no entry
    /// for the language.
    pub fn value_for(&self, player: &Player) -> Option<String> {
        let text = match self.value.as_ref()? {
            SettingValue::Text(text) => text.clone(),
            SettingValue::ByLanguage(by) => by.get(&player.lang.to_ascii_lowercase())?.clone(),
        };
        Some(
            text.replace("%player%", &player.name)
                .replace("%game_lang%", &player.lang),
        )
    }
}

/// Who is starting the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    /// Already free of what cmd would trip over
    /// (`Settings::safe_player_name`).
    pub name: String,
    /// The game language, `de`/`en`/`fr`.
    pub lang: String,
}

/// What happened to one setting, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    Written(PathBuf),
    Unchanged(PathBuf),
    /// Not set, and why: the file is not in the package, the language has
    /// no entry.
    Left(String),
}

/// A registry value to set, for the platform's own way of setting it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryValue {
    /// `HKCU\…`/`HKLM\…` as given.
    pub key: String,
    pub name: String,
    pub value: String,
    pub reg_type: ValueType,
}

/// macOS/Linux: the batch that sets `values` in the game's prefix, written
/// into the game's folder as `launch::setup_script::Script::Settings`, and
/// the variables it reads the values from: a name with an umlaut written
/// into a batch file would come out in the console's code page, the
/// environment Wine hands over in Unicode. The runner starts it as it starts
/// the other scripts (`launch::unix::script_plan`).
pub fn write_registry_script(
    share_dir: &Path,
    game_id: &str,
    lang: &str,
    values: &[RegistryValue],
) -> std::io::Result<Vec<(String, String)>> {
    use crate::launch::setup_script::{wrapper, write_if_changed, Script};
    // Delayed expansion: `!V!` is filled in after the line is read, so
    // nothing in a value (`&`, `|`) becomes part of the command.
    let mut script = String::from("@echo off\r\nsetlocal EnableDelayedExpansion\r\n");
    let mut env = Vec::new();
    for (i, v) in values.iter().enumerate() {
        let var = format!("NLL_VALUE_{i}");
        script.push_str(&format!(
            "reg add \"{}\" /v \"{}\" /t {} /d \"!{var}!\" /f\r\n",
            v.key,
            v.name,
            v.reg_type.reg_name()
        ));
        env.push((var, v.value.clone()));
    }
    write_if_changed(
        &share_dir.join(Script::Settings.filtered_name()),
        script.as_bytes(),
    )?;
    write_if_changed(
        &share_dir.join(Script::Settings.wrapper_name()),
        wrapper(game_id, lang).as_bytes(),
    )?;
    Ok(env)
}

/// Set the file settings of `settings` below `local` — or, for those in a
/// [`Folder`] of the Windows user, below `profile` (`C:\Users\<name>`, in
/// the game's prefix on macOS/Linux; `None` while there is none yet) — and
/// say what became of each; the registry ones are returned for the platform
/// to set.
pub fn apply(
    local: &Path,
    profile: Option<&Path>,
    settings: &[PlayerSetting],
    player: &Player,
) -> (Vec<Applied>, Vec<RegistryValue>) {
    let mut done = Vec::new();
    let mut registry = Vec::new();
    for setting in settings {
        let Ok(kind) = setting.kind() else {
            continue;
        };
        let Some(value) = setting.value_for(player) else {
            done.push(Applied::Left(format!(
                "no value for game language {:?}",
                player.lang
            )));
            continue;
        };
        let file = match kind {
            Kind::Registry { key, name } => {
                registry.push(RegistryValue {
                    key: key.to_string(),
                    name: name.to_string(),
                    value,
                    reg_type: setting.value_type.unwrap_or_default(),
                });
                continue;
            }
            Kind::Ini { file, .. }
            | Kind::Line { file, .. }
            | Kind::WholeFile { file }
            | Kind::Json { file, .. } => file,
        };
        let root = match setting.folder.unwrap_or_default().below_profile() {
            None => local.to_path_buf(),
            Some(below) => match profile {
                Some(profile) => existing_or_new(profile, below),
                None => {
                    done.push(Applied::Left(format!(
                        "{file}: the Windows user's folder is not there yet"
                    )));
                    continue;
                }
            },
        };
        // A file the package does not have yet is made, folders and all:
        // Call of Duty 2 makes its player profile only when asked for one.
        let path = existing_or_new(&root, file);
        let utf8 = matches!(kind, Kind::Json { .. });
        let typed = setting.value_type.unwrap_or_default();
        done.push(if root == local {
            set_inside(local, path, &kind, &value, utf8, typed)
        } else {
            // The user's folders come from Wine, not from the package, and
            // Wine links `Documents` and the like into the host's home on
            // purpose: no link check there. `file` stays relative all the
            // same (`is_safe_relative`, checked when the profile loads).
            set_at(path, &kind, &value, utf8, typed)
        });
    }
    (done, registry)
}

/// The file in whatever case it is there, or where a new one goes: below
/// the deepest of its folders that is there in some case, the rest as the
/// profile spells it. (`file` is relative and checked, `PlayerSetting::kind`.)
fn existing_or_new(local: &Path, file: &str) -> PathBuf {
    let parts: Vec<&str> = file
        .split(['/', '\\'])
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    for cut in (0..=parts.len()).rev() {
        let found = if cut == 0 {
            Some(local.to_path_buf())
        } else {
            crate::paths::find_ignoring_case(local, &parts[..cut].join("/"))
        };
        if let Some(found) = found {
            return parts[cut..]
                .iter()
                .fold(found, |path, part| path.join(part));
        }
    }
    local.join(file)
}

/// A file's text and how to write it back: its encoding and byte order
/// mark, and its line ends.
struct Text {
    text: String,
    encoding: &'static encoding_rs::Encoding,
    bom: &'static [u8],
    eol: &'static str,
}

/// As fnr.exe reads a file: a byte order mark says the encoding, valid
/// UTF-8 beyond ASCII says UTF-8, everything else is Windows' ANSI — what
/// these games were written for, plain ASCII included.
fn read(path: &Path) -> std::io::Result<Text> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    let (encoding, bom): (&'static encoding_rs::Encoding, &'static [u8]) =
        if bytes.starts_with(b"\xFF\xFE") {
            (encoding_rs::UTF_16LE, b"\xFF\xFE")
        } else if bytes.starts_with(b"\xFE\xFF") {
            (encoding_rs::UTF_16BE, b"\xFE\xFF")
        } else if bytes.starts_with(b"\xEF\xBB\xBF") {
            (encoding_rs::UTF_8, b"\xEF\xBB\xBF")
        } else if !bytes.is_ascii() && std::str::from_utf8(&bytes).is_ok() {
            (encoding_rs::UTF_8, b"")
        } else {
            (encoding_rs::WINDOWS_1252, b"")
        };
    let (text, _) = encoding.decode_without_bom_handling(&bytes[bom.len()..]);
    let text = text.into_owned();
    let eol = if text.contains("\r\n") || text.is_empty() {
        "\r\n"
    } else {
        "\n"
    };
    Ok(Text {
        text,
        encoding,
        bom,
        eol,
    })
}

/// Set `value` as `kind` says; `false` when the file already said it.
/// `utf8` for a file whose reader wants UTF-8 whatever it held before.
fn write(
    path: &Path,
    kind: &Kind<'_>,
    value: &str,
    utf8: bool,
    typed: ValueType,
) -> std::io::Result<bool> {
    let mut old = read(path)?;
    if utf8 && old.encoding != encoding_rs::UTF_8 {
        // Another encoding's byte order mark must not stand before UTF-8.
        old.encoding = encoding_rs::UTF_8;
        old.bom = b"";
    }
    let new = match kind {
        Kind::WholeFile { .. } => value.to_string(),
        Kind::Ini { section, key, .. } => set_ini(&old.text, *section, key, value, old.eol),
        Kind::Line { line, .. } => set_line(&old.text, line, value, old.eol),
        Kind::Json { path: key, .. } => match set_json(&old.text, key, value, typed) {
            Some(Some(new)) => new,
            // The key holds that value already: the file stays as it is,
            // whatever order or spacing the game wrote it in.
            Some(None) => return Ok(false),
            None => {
                return Err(std::io::Error::other(
                    "not JSON this launcher can set a key in; left as it is",
                ))
            }
        },
        Kind::Registry { .. } => return Ok(false),
    };
    if new == old.text && path.exists() {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut bytes = old.bom.to_vec();
    if old.encoding == encoding_rs::UTF_16LE || old.encoding == encoding_rs::UTF_16BE {
        // encoding_rs encodes into UTF-8 for these: write the units by hand.
        let big = old.encoding == encoding_rs::UTF_16BE;
        for unit in new.encode_utf16() {
            bytes.extend(if big {
                unit.to_be_bytes()
            } else {
                unit.to_le_bytes()
            });
        }
    } else {
        let (encoded, _, lossy) = old.encoding.encode(&new);
        if lossy {
            // A name ANSI cannot hold ("Łukasz", Cyrillic) would come out as
            // `&#321;`: such a file is better UTF-8, which the game may read.
            bytes = old.bom.to_vec();
            bytes.extend_from_slice(new.as_bytes());
        } else {
            bytes.extend_from_slice(&encoded);
        }
    }
    std::fs::write(path, bytes)?;
    Ok(true)
}

/// Whether `path` stays below `local` once its links are followed: a
/// package may hold a link that would send a write elsewhere. Asked for the
/// deepest part that is there, before anything is made.
fn stays_inside(local: &Path, path: &Path) -> bool {
    let Ok(root) = std::fs::canonicalize(local) else {
        return false;
    };
    let mut probe = path;
    loop {
        if let Ok(real) = std::fs::canonicalize(probe) {
            return real.starts_with(&root);
        }
        match probe.parent() {
            Some(parent) => probe = parent,
            None => return false,
        }
    }
}

/// [`write`], below `local` only; the outcome as the log wants it.
fn set_inside(
    local: &Path,
    path: PathBuf,
    kind: &Kind<'_>,
    value: &str,
    utf8: bool,
    typed: ValueType,
) -> Applied {
    if !stays_inside(local, &path) {
        return Applied::Left(format!("{} leads outside the game folder", path.display()));
    }
    set_at(path, kind, value, utf8, typed)
}

/// [`write`], the outcome as the log wants it.
fn set_at(path: PathBuf, kind: &Kind<'_>, value: &str, utf8: bool, typed: ValueType) -> Applied {
    match write(&path, kind, value, utf8, typed) {
        Ok(true) => Applied::Written(path),
        Ok(false) => Applied::Unchanged(path),
        Err(e) => Applied::Left(format!("{}: {e}", path.display())),
    }
}

/// `text` (a JSON object, or nothing yet) with `key` (levels joined by dots)
/// set to `value` as `typed` says: `Some(None)` when it holds that already,
/// `None` when it is not an object this can set a key in. Levels that are
/// missing are made; everything else stays, though the file is written in
/// key order — which the game reads by name, not by place.
fn set_json(text: &str, key: &str, value: &str, typed: ValueType) -> Option<Option<String>> {
    use serde_json::Value;
    let mut root: Value = if text.trim().is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?
    };
    let wanted = match typed {
        ValueType::Number => {
            let n: f64 = value.parse().ok()?;
            if n.fract() == 0.0 && n.abs() < 9.0e15 {
                Value::from(n as i64)
            } else {
                Value::from(n)
            }
        }
        ValueType::Bool => Value::Bool(value == "true"),
        ValueType::Text | ValueType::Dword => Value::String(value.to_string()),
    };
    let mut at = &mut root;
    let levels: Vec<&str> = key.split('.').collect();
    for level in &levels[..levels.len() - 1] {
        let object = at.as_object_mut()?;
        at = object
            .entry(level.to_string())
            .or_insert_with(|| Value::Object(Default::default()));
    }
    let object = at.as_object_mut()?;
    let last = levels[levels.len() - 1];
    // `4.0` written by the game is the `4` wanted: compared as numbers, or
    // the file would be rewritten on every start.
    let same = match (object.get(last), &wanted) {
        (Some(Value::Number(have)), Value::Number(want)) => have.as_f64() == want.as_f64(),
        (have, want) => have == Some(want),
    };
    if same {
        return Some(None);
    }
    object.insert(last.to_string(), wanted);
    serde_json::to_string_pretty(&root).ok().map(Some)
}

/// `key=value` in `[section]` (any case), the existing line replaced or a
/// new one put at the end of the section; a missing section is added.
/// Without a section, the first `key=` anywhere, or a line at the top.
fn set_ini(text: &str, section: Option<&str>, key: &str, value: &str, eol: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let is_key = |line: &str| {
        line.split_once('=')
            .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key))
    };
    let header = |line: &str| {
        let t = line.trim();
        (t.starts_with('[') && t.ends_with(']')).then(|| t[1..t.len() - 1].trim().to_string())
    };
    let wanted = format!("{key}={value}");
    // A key that is there keeps its spelling and its indentation.
    let replaced = |line: &str| {
        let (k, rest) = line.split_once('=').unwrap_or((line, ""));
        let pad = &rest[..rest.len() - rest.trim_start().len()];
        format!("{k}={pad}{value}")
    };
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    match section {
        None => match lines.iter().position(|l| is_key(l)) {
            Some(at) => out[at] = replaced(lines[at]),
            None => out.insert(0, wanted),
        },
        Some(section) => {
            let start = lines
                .iter()
                .position(|l| header(l).is_some_and(|h| h.eq_ignore_ascii_case(section)));
            match start {
                None => {
                    if out.last().is_some_and(|l| !l.trim().is_empty()) {
                        out.push(String::new());
                    }
                    out.push(format!("[{section}]"));
                    out.push(wanted);
                }
                Some(start) => {
                    let end = lines[start + 1..]
                        .iter()
                        .position(|l| header(l).is_some())
                        .map_or(lines.len(), |at| start + 1 + at);
                    match (start + 1..end).find(|&at| is_key(lines[at])) {
                        Some(at) => out[at] = replaced(lines[at]),
                        None => {
                            // After the section's last line with content.
                            let at = (start + 1..end)
                                .rev()
                                .find(|&at| !lines[at].trim().is_empty())
                                .map_or(start + 1, |at| at + 1);
                            out.insert(at, wanted);
                        }
                    }
                }
            }
        }
    }
    join(&out, text, eol)
}

/// `line "value"` (`seta name "Player"`): the line starting with `line`,
/// any spacing between its words, replaced, or added at the end.
fn set_line(text: &str, line: &str, value: &str, eol: &str) -> String {
    let words: Vec<String> = line
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    let starts = |l: &str| {
        let have: Vec<String> = l.split_whitespace().map(str::to_ascii_lowercase).collect();
        have.len() > words.len() && have[..words.len()] == words[..]
    };
    let wanted = format!("{line} \"{}\"", value.replace('"', ""));
    let mut out: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    match out.iter().position(|l| starts(l)) {
        Some(at) => out[at] = wanted,
        None => out.push(wanted),
    }
    join(&out, text, eol)
}

fn join(lines: &[String], before: &str, eol: &str) -> String {
    let mut text = lines.join(eol);
    if before.is_empty() || before.ends_with('\n') {
        text.push_str(eol);
    }
    text
}

/// The Goldberg Steam emulator's own way of being told the player's name
/// (and gbe_fork's `configs.user.ini`, see [`emulators`])
/// and language: `steam_settings/force_account_name.txt` and
/// `force_language.txt` next to its `steam_api(64).dll` (or `steamclient`
/// behind its ColdClientLoader), over whatever its
/// other settings say. Packages built on it get them without a profile;
/// which DLL is Goldberg's its text says (it names those files). Goldberg
/// reads them as UTF-8.
pub fn goldberg(local: &Path, player: &Player) -> Vec<Applied> {
    emulators(local, player).0
}

/// The SmartSteamEmu Steam emulator (started through `SmartSteamLoader.exe`,
/// the entry point of many ETI packages) reads the player's name and language
/// from `[SmartSteamEmu]` in its `SmartSteamEmu.ini`: `PersonaName`,
/// `Language`. Every such file in the package gets them, without a profile
/// — Counter-Strike 1.6's start script copies one over another before each
/// start, so both are set.
pub fn smart_steam_emu(local: &Path, player: &Player) -> Vec<Applied> {
    emulators(local, player).1
}

/// Both emulators from one walk through the package: [`goldberg`] and
/// [`smart_steam_emu`].
pub fn emulators(local: &Path, player: &Player) -> (Vec<Applied>, Vec<Applied>) {
    let language = steam_language(&player.lang);
    let found = find_files(local, 5, |name| {
        // Goldberg as `steam_api`, or as `steamclient` behind its
        // ColdClientLoader (`steamclient64.ccl.dll` in 7 Days to Die):
        // both read `steam_settings/` next to themselves.
        matches!(
            name,
            "steam_api.dll"
                | "steam_api64.dll"
                | "steamclient.dll"
                | "steamclient64.dll"
                | "steamclient.ccl.dll"
                | "steamclient64.ccl.dll"
                | "smartsteamemu.ini"
        )
    });
    let (mut goldberg, mut sse) = (Vec::new(), Vec::new());
    // Which emulator each DLL is, one read per file; a folder once.
    let mut gbe_dirs = std::collections::BTreeSet::new();
    let mut classic_dirs = std::collections::BTreeSet::new();
    for dll in found.iter().filter(|path| !is_ini(path)) {
        let (gbe, classic) = emulator_marks(dll);
        let Some(dir) = dll.parent() else { continue };
        // gbe_fork only where its own config files are: the text alone is
        // no proof, and the classic files below create `steam_settings/`.
        let configs = ["configs.user.ini", "configs.main.ini", "configs.app.ini"];
        if gbe
            && configs
                .iter()
                .any(|c| dir.join("steam_settings").join(c).is_file())
        {
            gbe_dirs.insert(dir.to_path_buf());
        } else if classic {
            classic_dirs.insert(dir.to_path_buf());
        }
    }
    // A build that names both is gbe_fork (it mentions the old files).
    classic_dirs.retain(|dir| !gbe_dirs.contains(dir));
    // gbe_fork, Goldberg's successor, reads `[user::general]` in
    // `steam_settings/configs.user.ini` instead (9-Bit Armies); as UTF-8.
    for dir in gbe_dirs {
        let file = dir.join("steam_settings").join("configs.user.ini");
        for (key, value) in [
            ("account_name", Some(player.name.as_str())),
            ("language", language),
        ] {
            if let Some(value) = value {
                let kind = Kind::Ini {
                    file: "configs.user.ini",
                    section: Some("user::general"),
                    key,
                };
                goldberg.push(set_inside(
                    local,
                    file.clone(),
                    &kind,
                    value,
                    true,
                    ValueType::Text,
                ));
            }
        }
    }
    for dir in classic_dirs {
        let settings = dir.join("steam_settings");
        for (name, value) in [
            ("force_account_name.txt", Some(player.name.as_str())),
            ("force_language.txt", language),
        ] {
            if let Some(value) = value {
                let kind = Kind::WholeFile { file: name };
                goldberg.push(set_inside(
                    local,
                    settings.join(name),
                    &kind,
                    value,
                    true,
                    ValueType::Text,
                ));
            }
        }
        // Packages that run Goldberg in its local-save mode (Left 4 Dead 2,
        // Counter-Strike: Source) carry its user settings in `settings/`
        // next to the DLL: those are set too, where the package has them.
        let local_save = dir.join("settings");
        for (name, value) in [
            ("account_name.txt", Some(player.name.as_str())),
            ("language.txt", language),
        ] {
            let path = local_save.join(name);
            if let (Some(value), true) = (value, path.is_file()) {
                let kind = Kind::WholeFile { file: name };
                goldberg.push(set_inside(local, path, &kind, value, true, ValueType::Text));
            }
        }
    }
    for ini in found.iter().filter(|path| is_ini(path)) {
        for (key, value) in [
            ("PersonaName", Some(player.name.as_str())),
            ("Language", language),
        ] {
            if let Some(value) = value {
                let kind = Kind::Ini {
                    file: "SmartSteamEmu.ini",
                    section: Some("SmartSteamEmu"),
                    key,
                };
                sse.push(set_inside(
                    local,
                    ini.clone(),
                    &kind,
                    value,
                    false,
                    ValueType::Text,
                ));
            }
        }
    }
    (goldberg, sse)
}

fn is_ini(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ini"))
}

/// Whether the DLL at `path` names gbe_fork's settings file
/// (`configs.user.ini`) and Goldberg's (`force_account_name.txt`), from one
/// read, remembered by size and time: reading the file before every start
/// is not needed when the package did not change.
fn emulator_marks(path: &Path) -> (bool, bool) {
    type Seen =
        std::collections::HashMap<PathBuf, (u64, Option<std::time::SystemTime>, (bool, bool))>;
    static SEEN: std::sync::Mutex<Option<Seen>> = std::sync::Mutex::new(None);
    let Ok(meta) = std::fs::metadata(path) else {
        return (false, false);
    };
    let stamp = (meta.len(), meta.modified().ok());
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let seen = seen.get_or_insert_with(Default::default);
    if let Some((len, time, answer)) = seen.get(path) {
        if (*len, *time) == stamp {
            return *answer;
        }
    }
    let has = |bytes: &[u8], marker: &[u8]| bytes.windows(marker.len()).any(|w| w == marker);
    let answer = if meta.len() < 32 << 20 {
        std::fs::read(path)
            .map(|b| {
                (
                    has(&b, b"configs.user.ini"),
                    has(&b, b"force_account_name.txt"),
                )
            })
            .unwrap_or((false, false))
    } else {
        (false, false)
    };
    seen.insert(path.to_path_buf(), (stamp.0, stamp.1, answer));
    answer
}

/// The game language as Steam names it.
fn steam_language(lang: &str) -> Option<&'static str> {
    match lang.to_ascii_lowercase().as_str() {
        "de" => Some("german"),
        "en" => Some("english"),
        "fr" => Some("french"),
        _ => None,
    }
}

/// Files below `dir` (at most `depth` folders down) whose lower-case name
/// `wanted` accepts, in a fixed order. Links to folders are not followed.
fn find_files(dir: &Path, depth: usize, wanted: impl Fn(&str) -> bool + Copy) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && depth > 0 {
            out.extend(find_files(&entry.path(), depth - 1, wanted));
        } else if kind.is_file()
            && wanted(&entry.file_name().to_string_lossy().to_ascii_lowercase())
        {
            out.push(entry.path());
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_json_key_is_set_in_the_users_folder_and_the_rest_stays() {
        let tmp = tempfile::tempdir().unwrap();
        let local = tmp.path().join("local");
        let profile = tmp.path().join("users/steamuser");
        std::fs::create_dir_all(&local).unwrap();
        std::fs::create_dir_all(&profile).unwrap();
        let json = |key: &str, typed: Option<ValueType>, value: &str| PlayerSetting {
            folder: Some(Folder::LocalLow),
            file: Some("Innersloth/Among Us/player.amogus".into()),
            json: Some(key.into()),
            value_type: typed,
            value: Some(SettingValue::Text(value.into())),
            ..Default::default()
        };
        let settings = [
            json("customization.name", None, "%player%"),
            json(
                "onboarding.privacyPolicyVersion",
                Some(ValueType::Number),
                "4",
            ),
        ];
        for s in &settings {
            assert!(s.kind().is_ok());
        }
        // Before the game ever ran: made, folders and all.
        let (done, _) = apply(&local, Some(&profile), &settings, &player());
        assert!(
            done.iter().all(|d| matches!(d, Applied::Written(_))),
            "{done:?}"
        );
        let file = profile.join("AppData/LocalLow/Innersloth/Among Us/player.amogus");
        let made: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(made["customization"]["name"], "Jürgen");
        assert_eq!(made["onboarding"]["privacyPolicyVersion"], 4);

        // What the game wrote around it stays; a value already there is not
        // written again.
        std::fs::write(
            &file,
            r#"{"customization":{"name":"Jürgen","hat":"hat_x"},"onboarding":{"privacyPolicyVersion":4},"dataVersion":1}"#,
        )
        .unwrap();
        let (again, _) = apply(&local, Some(&profile), &settings, &player());
        assert!(
            again.iter().all(|d| matches!(d, Applied::Unchanged(_))),
            "{again:?}"
        );
        let renamed = Player {
            name: "Bazzite".into(),
            lang: "de".into(),
        };
        apply(&local, Some(&profile), &settings, &renamed);
        let kept: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(kept["customization"]["name"], "Bazzite");
        assert_eq!(kept["customization"]["hat"], "hat_x");
        assert_eq!(kept["dataVersion"], 1);

        // Not JSON: left alone, never replaced.
        std::fs::write(&file, "garbage").unwrap();
        let (left, _) = apply(&local, Some(&profile), &settings, &renamed);
        assert!(matches!(left[0], Applied::Left(_)), "{left:?}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "garbage");

        // No user's folder yet (no prefix): said so, nothing written.
        let (none, _) = apply(&local, None, &settings, &renamed);
        assert!(matches!(none[0], Applied::Left(_)));
    }

    #[test]
    fn typed_values_are_checked_where_they_belong() {
        let base = || PlayerSetting {
            file: Some("a.json".into()),
            json: Some("a.b".into()),
            value: Some(SettingValue::Text("1".into())),
            ..Default::default()
        };
        assert!(PlayerSetting {
            value_type: Some(ValueType::Number),
            ..base()
        }
        .kind()
        .is_ok());
        let not_a_number = PlayerSetting {
            value_type: Some(ValueType::Number),
            value: Some(SettingValue::Text("%player%".into())),
            ..base()
        };
        assert!(not_a_number.kind().is_err());
        assert!(PlayerSetting {
            value_type: Some(ValueType::Bool),
            ..base()
        }
        .kind()
        .is_err());
        assert!(PlayerSetting {
            value_type: Some(ValueType::Dword),
            ..base()
        }
        .kind()
        .is_err());
        assert!(PlayerSetting {
            json: Some("a..b".into()),
            ..base()
        }
        .kind()
        .is_err());
        assert!(PlayerSetting {
            key: Some("k".into()),
            ..base()
        }
        .kind()
        .is_err());
        let ini_number = PlayerSetting {
            json: None,
            key: Some("k".into()),
            value_type: Some(ValueType::Number),
            ..base()
        };
        assert!(ini_number.kind().is_err());
    }

    #[test]
    fn a_dword_is_a_number_and_reaches_the_batch_as_one() {
        let dword = |value: &str| PlayerSetting {
            registry: Some("HKCU\\Software\\Game\\1.0\\EULA".into()),
            key: Some("FIRSTRUN".into()),
            value_type: Some(ValueType::Dword),
            value: Some(SettingValue::Text(value.into())),
            ..Default::default()
        };
        for good in ["1", "0", "4294967295", "0x1", "0XFF"] {
            assert!(dword(good).kind().is_ok(), "{good}");
        }
        for bad in [
            "",
            "-1",
            "+1",
            "0x+1",
            "4294967296",
            "0x",
            "%player%",
            "1 & x",
        ] {
            assert!(dword(bad).kind().is_err(), "{bad}");
        }
        let in_a_file = PlayerSetting {
            file: Some("a.ini".into()),
            key: Some("k".into()),
            value_type: Some(ValueType::Dword),
            value: Some(SettingValue::Text("1".into())),
            ..Default::default()
        };
        assert!(in_a_file.kind().is_err());

        let player = Player {
            name: "Jürgen".into(),
            lang: "de".into(),
        };
        let dir = tempfile::tempdir().unwrap();
        let (_, registry) = apply(dir.path(), None, &[dword("1")], &player);
        assert_eq!(registry[0].reg_type, ValueType::Dword);
        write_registry_script(dir.path(), "aoe3", "de", &registry).unwrap();
        let batch = std::fs::read_to_string(
            dir.path()
                .join(crate::launch::setup_script::Script::Settings.filtered_name()),
        )
        .unwrap();
        assert!(
            batch.contains("/v \"FIRSTRUN\" /t REG_DWORD /d \"!NLL_VALUE_0!\" /f"),
            "{batch}"
        );
    }

    fn player() -> Player {
        Player {
            name: "Jürgen".into(),
            lang: "de".into(),
        }
    }

    fn setting(toml_text: &str) -> PlayerSetting {
        toml::from_str(toml_text).unwrap()
    }

    #[test]
    fn the_forms_a_profile_can_give() {
        let ini = setting(
            "file = 'System/User.ini'\nsection = 'DefaultPlayer'\nkey = 'Name'\nvalue = '%player%'",
        );
        assert!(matches!(
            ini.kind(),
            Ok(Kind::Ini {
                section: Some("DefaultPlayer"),
                ..
            })
        ));
        let line = setting("file = 'baseq3/q3config.cfg'\nline = 'seta name'\nvalue = '%player%'");
        assert!(matches!(line.kind(), Ok(Kind::Line { .. })));
        let whole =
            setting("file = 'settings/language.txt'\nvalue = { de = 'german', en = 'english' }");
        assert!(matches!(whole.kind(), Ok(Kind::WholeFile { .. })));
        assert_eq!(whole.value_for(&player()).as_deref(), Some("german"));
        let fr = Player {
            lang: "fr".into(),
            ..player()
        };
        assert_eq!(whole.value_for(&fr), None);
        let reg = setting("registry = 'HKCU\\Software\\X'\nkey = 'userlocal'\nvalue = '%player%'");
        assert!(matches!(reg.kind(), Ok(Kind::Registry { .. })));
        for bad in [
            "file = '../x.ini'\nvalue = 'a'",
            "file = 'x.ini'",
            "registry = 'HKCR\\x'\nkey = 'a'\nvalue = 'b'",
            "file = 'x.ini'\nregistry = 'HKCU\\x'\nkey = 'a'\nvalue = 'b'",
        ] {
            assert!(setting(bad).kind().is_err(), "{bad}");
        }
    }

    #[test]
    fn an_ini_key_is_set_in_its_section_and_the_file_keeps_its_encoding() {
        let tmp = tempfile::tempdir().unwrap();
        let system = tmp.path().join("System");
        std::fs::create_dir_all(&system).unwrap();
        // UT2004's User.ini: a BOM, CRLF; UT2004.ini: ANSI, a key elsewhere.
        std::fs::write(
            system.join("User.ini"),
            b"\xEF\xBB\xBF[DefaultPlayer]\r\nName=Player\r\nClass=Engine.Pawn\r\n\r\n[Other]\r\nName=keep\r\nLanguage = int\r\n",
        )
        .unwrap();
        std::fs::write(system.join("UT2004.ini"), b"[URL]\r\nMap=Gr\xFCn\r\n").unwrap();
        let settings = [
            setting("file = 'system/user.ini'\nsection = 'defaultplayer'\nkey = 'name'\nvalue = '%player%'"),
            setting("file = 'System/UT2004.ini'\nsection = 'URL'\nkey = 'Name'\nvalue = '%player%'"),
            setting("file = 'System/UT2004.ini'\nsection = 'Engine.Engine'\nkey = 'Language'\nvalue = { de = 'det', en = 'int' }"),
            setting("file = 'System/Profiles/LAN/new.cfg'\nline = 'seta name'\nvalue = '%player%'"),
            setting("file = 'System/User.ini'\nsection = 'Other'\nkey = 'Language'\nvalue = { de = 'det' }"),
        ];
        let (done, registry) = apply(tmp.path(), None, &settings, &player());
        assert!(registry.is_empty());
        // Made where the package has nothing yet, folders and all.
        assert!(matches!(done[3], Applied::Written(_)));
        assert_eq!(
            std::fs::read(system.join("Profiles/LAN/new.cfg")).unwrap(),
            b"seta name \"J\xFCrgen\"\r\n"
        );
        assert_eq!(
            std::fs::read(system.join("User.ini")).unwrap(),
            // The spacing around `=` stays as the file had it.
            "\u{feff}[DefaultPlayer]\r\nName=Jürgen\r\nClass=Engine.Pawn\r\n\r\n[Other]\r\nName=keep\r\nLanguage = det\r\n".as_bytes()
        );
        assert_eq!(
            std::fs::read(system.join("UT2004.ini")).unwrap(),
            b"[URL]\r\nMap=Gr\xFCn\r\nName=J\xFCrgen\r\n\r\n[Engine.Engine]\r\nLanguage=det\r\n"
        );
        // Again: nothing to write.
        let (again, _) = apply(tmp.path(), None, &settings[..1], &player());
        assert!(matches!(again[0], Applied::Unchanged(_)));
    }

    #[test]
    fn a_config_line_and_a_registry_value() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("baseq3")).unwrap();
        std::fs::write(
            tmp.path().join("baseq3/q3config.cfg"),
            "seta r_mode \"4\"\nseta  name \"UnnamedPlayer\"\n",
        )
        .unwrap();
        let settings = [
            setting("file = 'baseq3/q3config.cfg'\nline = 'seta name'\nvalue = '%player%'"),
            setting("registry = 'HKCU\\Software\\Blizzard Entertainment\\Warcraft III\\String'\nkey = 'userlocal'\nvalue = '%player%'"),
        ];
        let (_, registry) = apply(tmp.path(), None, &settings, &player());
        // ASCII is ANSI here: Quake 3 reads Latin-1.
        assert_eq!(
            std::fs::read(tmp.path().join("baseq3/q3config.cfg")).unwrap(),
            b"seta r_mode \"4\"\nseta name \"J\xFCrgen\"\n"
        );
        assert_eq!(
            registry,
            [RegistryValue {
                key: "HKCU\\Software\\Blizzard Entertainment\\Warcraft III\\String".into(),
                name: "userlocal".into(),
                value: "Jürgen".into(),
                reg_type: ValueType::Text,
            }]
        );
    }

    #[test]
    fn a_name_ansi_cannot_hold_makes_the_file_utf8_and_quotes_are_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let polish = Player {
            name: "Łukasz".into(),
            lang: "de".into(),
        };
        let settings = [setting(
            "file = 'cfg/config.cfg'\nline = 'name'\nvalue = '%player%'",
        )];
        apply(tmp.path(), None, &settings, &polish);
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("cfg/config.cfg")).unwrap(),
            "name \"Łukasz\"\r\n"
        );
        assert!(setting("file = 'a.ini'\nkey = 'k'\nvalue = 'x\" & y'")
            .kind()
            .is_err());
        assert!(setting("registry = 'HKCU\\X\\'\nkey = 'k'\nvalue = 'v'")
            .kind()
            .is_err());
        assert!(setting("registry = 'HKCU\\X'\nkey = 'k!'\nvalue = 'v'")
            .kind()
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_game_folder_is_not_written_through() {
        let tmp = tempfile::tempdir().unwrap();
        let local = tmp.path().join("local");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(local.join("main")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, local.join("main/players")).unwrap();
        let settings = [setting("file = 'main/players/active.txt'\nvalue = 'LAN'")];
        let (done, _) = apply(&local, None, &settings, &player());
        assert!(matches!(done[0], Applied::Left(_)));
        assert!(!outside.join("active.txt").exists());
    }

    #[test]
    fn every_smart_steam_emu_ini_gets_the_name_and_the_language() {
        let tmp = tempfile::tempdir().unwrap();
        let ini = "[Launcher]\r\nTarget = hl.exe\r\n\r\n[SmartSteamEmu]\r\nLanguage = english\r\nPersonaName = ChangeMe\r\n\r\n[DLC]\r\n";
        std::fs::write(tmp.path().join("SmartSteamEmu.ini"), ini).unwrap();
        std::fs::create_dir_all(tmp.path().join("hl-cs16")).unwrap();
        std::fs::write(tmp.path().join("hl-cs16/smartsteamemu.ini"), ini).unwrap();
        let done = smart_steam_emu(tmp.path(), &player());
        assert_eq!(
            done.iter()
                .filter(|d| matches!(d, Applied::Written(_)))
                .count(),
            4
        );
        for file in ["SmartSteamEmu.ini", "hl-cs16/smartsteamemu.ini"] {
            assert_eq!(
                std::fs::read(tmp.path().join(file)).unwrap(),
                b"[Launcher]\r\nTarget = hl.exe\r\n\r\n[SmartSteamEmu]\r\nLanguage = german\r\nPersonaName = J\xFCrgen\r\n\r\n[DLC]\r\n"
            );
        }
    }

    #[test]
    fn gbe_fork_gets_the_name_in_its_user_config() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("steam_api64.dll"),
            b"MZ...configs.user.ini...",
        )
        .unwrap();
        let settings = tmp.path().join("steam_settings");
        std::fs::create_dir_all(&settings).unwrap();
        std::fs::write(
            settings.join("configs.user.ini"),
            "[user::general]\n# user account name\naccount_name=ChangeMe\nlanguage=english\n\n[user::saves]\nlocal_save_path=saves\n",
        )
        .unwrap();
        let player = Player {
            name: "Jürgen".into(),
            lang: "de".into(),
        };
        let done = goldberg(tmp.path(), &player);
        assert_eq!(done.len(), 2);
        let text = std::fs::read_to_string(settings.join("configs.user.ini")).unwrap();
        assert!(text.contains("account_name=Jürgen\n"), "{text}");
        assert!(text.contains("language=german\n"));
        assert!(text.contains("local_save_path=saves"));
        // Not the classic Goldberg files.
        assert!(!settings.join("force_account_name.txt").exists());
        // A build naming both is gbe_fork alone; without its config files
        // it is taken for classic Goldberg, also on the next start.
        let both = tempfile::tempdir().unwrap();
        std::fs::write(
            both.path().join("steam_api64.dll"),
            b"MZ...configs.user.ini...force_account_name.txt...",
        )
        .unwrap();
        assert_eq!(goldberg(both.path(), &player).len(), 2);
        assert!(!both.path().join("steam_settings/configs.user.ini").exists());
        let again = goldberg(both.path(), &player);
        assert!(again.iter().all(|a| matches!(a, Applied::Unchanged(_))));
        let settings = both.path().join("steam_settings");
        std::fs::remove_dir_all(&settings).unwrap();
        std::fs::create_dir_all(&settings).unwrap();
        std::fs::write(settings.join("configs.main.ini"), "").unwrap();
        assert_eq!(goldberg(both.path(), &player).len(), 2);
        assert!(settings.join("configs.user.ini").exists());
        assert!(!settings.join("force_account_name.txt").exists());
    }

    #[test]
    fn goldbergs_cold_client_loader_gets_the_name_too() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("steamclient64.ccl.dll"),
            b"MZ...force_account_name.txt...",
        )
        .unwrap();
        // A real Steam client next to it does not count.
        std::fs::write(tmp.path().join("steamclient64.dll"), b"MZ...valve...").unwrap();
        let player = Player {
            name: "Jürgen".into(),
            lang: "de".into(),
        };
        let done = goldberg(tmp.path(), &player);
        assert_eq!(done.len(), 2);
        let settings = tmp.path().join("steam_settings");
        assert_eq!(
            std::fs::read_to_string(settings.join("force_account_name.txt")).unwrap(),
            "Jürgen"
        );
        assert_eq!(
            std::fs::read_to_string(settings.join("force_language.txt")).unwrap(),
            "german"
        );
    }

    #[test]
    fn a_goldberg_dll_gets_the_name_and_the_language() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("steam_api.dll"), b"MZ...force_account_name.txt...").unwrap();
        std::fs::create_dir_all(tmp.path().join("other")).unwrap();
        std::fs::write(
            tmp.path().join("other/steam_api.dll"),
            b"MZ...another emulator",
        )
        .unwrap();
        // Local-save mode: its settings files are set where they are.
        std::fs::create_dir_all(bin.join("settings")).unwrap();
        std::fs::write(bin.join("settings/account_name.txt"), "ChangeMe").unwrap();
        let done = goldberg(tmp.path(), &player());
        assert_eq!(done.len(), 3);
        assert_eq!(
            std::fs::read(bin.join("settings/account_name.txt")).unwrap(),
            "Jürgen".as_bytes()
        );
        assert!(!bin.join("settings/language.txt").exists());
        assert_eq!(
            std::fs::read(bin.join("steam_settings/force_account_name.txt")).unwrap(),
            "Jürgen".as_bytes()
        );
        assert_eq!(
            std::fs::read_to_string(bin.join("steam_settings/force_language.txt")).unwrap(),
            "german"
        );
        assert!(!tmp.path().join("other/steam_settings").exists());
    }
}
