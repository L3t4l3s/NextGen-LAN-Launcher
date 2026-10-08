//! A game's launch configuration as testers edit it in the game details,
//! and the report that sends a working one back to the project.
//!
//! What a tester sets is a property of the game on a platform, not of their
//! machine, so it takes the form the project ships: a `[platform.<os>]`
//! block. It is stored on its own (`<data>/game-configs/<id>.toml`, a
//! [`ConfigOverlay`]) and laid over the profile in force when that is
//! loaded, so a later fix to the bundled or organiser profile still arrives.
//! Shared, it goes out as the whole profile with the block in it — the
//! `[platform.linux]` block of one tester and the `[platform.macos]` block of
//! another merge into one file for the next release.

use crate::manifest::{
    is_safe_relative, is_winetricks_verb, ConfigOverlay, Manifest, PlatformOverride, Runner,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Where the "send by mail" button sends a working configuration.
pub const REPORT_EMAIL: &str = "launcher@schimnick.de";
/// The repository a configuration can be reported to as an issue instead.
pub const REPORT_REPOSITORY: &str = "L3t4l3s/NextGen-LAN-Launcher";

/// The variable Wine and Proton read DLL overrides from; the editor gives it
/// a field of its own because LAN packages need it so often.
pub const DLL_OVERRIDES: &str = "WINEDLLOVERRIDES";

/// One game's launch settings on the current platform, as the editor shows
/// them. Argument lists are kept as typed (`-game cstrike "+name Neo"`) and
/// split into words only when saved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameConfig {
    /// Executable relative to `local/`.
    pub exe: String,
    pub args: String,
    /// Working folder relative to `local/`; empty means the exe's folder.
    pub workdir: String,
    pub runner: Runner,
    pub env: Vec<EnvVar>,
    /// `WINEDLLOVERRIDES`, e.g. `dinput8=n,b;ddraw=n`.
    pub dll_overrides: String,
    /// Programs in front of the start, e.g. `gamemoderun mangohud`.
    pub wrapper: String,
    /// Windows components for the prefix, as winetricks verbs separated by
    /// spaces: `directplay vcrun2010`.
    pub winetricks: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

/// Why a configuration cannot be saved, as an `err.<code>|detail` the UI
/// translates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl GameConfig {
    /// What actually starts on `platform` with the profile in force.
    ///
    /// `exe_override` is a choice stored in the install receipt. It wins
    /// over the profile when the game starts, and `resolve_exe` then starts
    /// it bare — no arguments, in its own folder — so that is what shows.
    /// A wrapper from a profile that may not have one (a game share's) is
    /// left out: it does not run, and saving it would make it run.
    pub fn from_manifest(
        manifest: Option<&Manifest>,
        platform: &str,
        exe_override: Option<&str>,
    ) -> Self {
        let spec = manifest.map(|m| m.launch_for(platform)).unwrap_or_default();
        let mut env = spec.env.clone();
        let dll_overrides = env.remove(DLL_OVERRIDES).unwrap_or_default();
        let trusted = manifest.is_some_and(|m| m.wrapper_is_trusted());
        // An empty working folder left in the spec can only be `[launch]`'s,
        // and there `resolve_exe` takes it as `local/` itself: shown as `.`,
        // since empty in the editor means the exe's folder.
        let workdir = match spec.workdir.as_deref() {
            Some("") => ".".to_string(),
            other => other.unwrap_or_default().to_string(),
        };
        let (exe, args, workdir) = match exe_override {
            Some(exe) => (exe.to_string(), String::new(), String::new()),
            None => (spec.exe, join_words(&spec.args), workdir),
        };
        GameConfig {
            exe,
            args,
            workdir,
            runner: spec.runner,
            env: env
                .into_iter()
                .map(|(name, value)| EnvVar { name, value })
                .collect(),
            dll_overrides,
            wrapper: if trusted {
                join_words(&spec.wrapper)
            } else {
                String::new()
            },
            winetricks: spec.winetricks.join(" "),
        }
    }

    /// The `[platform.<os>]` block for this configuration over `profile` —
    /// the profile it is laid over, without any configuration of its own.
    ///
    /// Only what differs goes in: a tester who changes the DLL overrides
    /// leaves the exe to the profile, and a later release that fixes the exe
    /// still reaches them. Variables the profile sets and the tester removed
    /// are listed in `unset_env`; a working folder is written in the form
    /// that means what the editor showed (`""` the exe's folder, `.` local/).
    pub fn to_block(
        &self,
        profile: Option<&Manifest>,
        game_id: &str,
        platform: &str,
    ) -> Result<PlatformOverride, ConfigError> {
        let exe = self.exe.trim().replace('\\', "/");
        if exe.is_empty() {
            return Err(ConfigError("err.config_exe_missing".into()));
        }
        if !is_safe_relative(&exe) {
            return Err(ConfigError(format!("err.config_path|{exe}")));
        }
        // `bin/` is `bin`, as when a profile is loaded.
        let typed = self.workdir.trim().replace('\\', "/");
        let workdir = match typed.trim_end_matches('/') {
            "" => typed.clone(),
            trimmed => trimmed.to_string(),
        };
        if !workdir.is_empty() && workdir != "." && !is_safe_relative(&workdir) {
            return Err(ConfigError(format!("err.config_path|{workdir}")));
        }
        let args =
            split_words(&self.args).map_err(|e| ConfigError(format!("err.config_quotes|{e}")))?;
        let wrapper = split_words(&self.wrapper)
            .map_err(|e| ConfigError(format!("err.config_quotes|{e}")))?;
        let verbs: Vec<String> = self
            .winetricks
            .split_whitespace()
            .map(String::from)
            .collect();
        if let Some(bad) = verbs.iter().find(|v| !is_winetricks_verb(v)) {
            return Err(ConfigError(format!("err.config_verb|{bad}")));
        }
        let mut env = BTreeMap::new();
        for var in &self.env {
            let name = var.name.trim();
            if name.is_empty() && var.value.trim().is_empty() {
                continue;
            }
            if !is_env_name(name) {
                return Err(ConfigError(format!("err.config_env_name|{name}")));
            }
            env.insert(name.to_string(), var.value.clone());
        }
        if !self.dll_overrides.trim().is_empty() {
            env.insert(DLL_OVERRIDES.into(), self.dll_overrides.trim().to_string());
        }

        let spec = profile.map(|m| m.launch_for(platform)).unwrap_or_default();
        let shown = GameConfig::from_manifest(profile, platform, None);
        let shown_wrapper = split_words(&shown.wrapper).unwrap_or_default();
        let block = PlatformOverride {
            exe: (exe != spec.exe.replace('\\', "/")).then_some(exe),
            args: (args != spec.args).then_some(args),
            // `.` and a path mean the same in both places; "" here is the
            // exe's folder, which a block has to say as `""`.
            workdir: (workdir != shown.workdir).then_some(workdir),
            runner: (self.runner != spec.runner).then_some(self.runner),
            wrapper: (wrapper != shown_wrapper).then_some(wrapper),
            winetricks: (verbs != spec.winetricks).then_some(verbs),
            unset_env: spec
                .env
                .keys()
                .filter(|name| !env.contains_key(*name))
                .cloned()
                .collect(),
            env: env
                .into_iter()
                .filter(|(name, value)| spec.env.get(name) != Some(value))
                .collect(),
        };
        // Read back through the parser the launcher loads profiles with, so
        // a block that saves is one that loads.
        let merged = with_block(profile, game_id, "", platform, &block);
        let text = to_toml(&merged)?;
        Manifest::parse(&text, Path::new(&format!("{game_id}.toml")))
            .map_err(|e| ConfigError(format!("err.config_invalid|{e}")))?;
        Ok(block)
    }
}

/// `profile` with `block` laid over its `[platform.<os>]` settings: what a
/// report sends, and what the launcher then starts. `[launch]`, `revisions`
/// and the other platforms stay as they were; the revision the tester ran
/// is in the report's text, since it is one platform's finding and
/// `revisions` speaks for all of them.
pub fn with_block(
    profile: Option<&Manifest>,
    game_id: &str,
    title: &str,
    platform: &str,
    block: &PlatformOverride,
) -> Manifest {
    let mut m = profile.cloned().unwrap_or_else(|| Manifest {
        id: game_id.to_string(),
        ..Default::default()
    });
    m.id = game_id.to_string();
    if m.title.is_none() && !title.is_empty() {
        m.title = Some(title.to_string());
    }
    let merged = block.layered_on(m.platform.get(platform));
    m.platform.insert(platform.to_string(), merged);
    m.user_config = true;
    m
}

/// A platform's block, as a configuration file: what the links carry — the
/// block as it belongs in the shipped profile, short enough for a `mailto:`
/// and an issue URL.
pub fn block_toml(
    game_id: &str,
    platform: &str,
    block: &PlatformOverride,
) -> Result<String, ConfigError> {
    let overlay = ConfigOverlay {
        id: game_id.to_string(),
        platform: BTreeMap::from([(platform.to_string(), block.clone())]),
        ..Default::default()
    };
    toml::to_string_pretty(&overlay).map_err(|e| ConfigError(format!("err.config_invalid|{e}")))
}

/// `overlay` with `block` stored for `platform` (`None` removes it), as the
/// file to write — or `Ok(None)` when nothing is left and the file can go.
/// A file that cannot be written is an error, never "nothing left".
///
/// A block that changes nothing is no configuration: it is removed, and the
/// game is back on its profile. `revision` is the package the block was
/// saved with.
pub fn updated_overlay(
    overlay: Option<ConfigOverlay>,
    game_id: &str,
    platform: &str,
    block: Option<PlatformOverride>,
    revision: &str,
) -> Result<Option<String>, ConfigError> {
    let mut overlay = overlay.unwrap_or_else(|| ConfigOverlay {
        id: game_id.to_string(),
        ..Default::default()
    });
    match block.filter(|b| !b.is_empty()) {
        Some(block) => {
            if !revision.is_empty() {
                overlay.revision = Some(revision.to_string());
            }
            overlay.platform.insert(platform.to_string(), block);
        }
        None => {
            overlay.platform.remove(platform);
        }
    }
    if overlay.platform.is_empty() {
        return Ok(None);
    }
    toml::to_string_pretty(&overlay)
        .map(Some)
        .map_err(|e| ConfigError(format!("err.config_invalid|{e}")))
}

/// The profile as a TOML file.
pub fn to_toml(manifest: &Manifest) -> Result<String, ConfigError> {
    toml::to_string_pretty(manifest).map_err(|e| ConfigError(format!("err.config_invalid|{e}")))
}

/// What the system takes as a variable name: anything without `=`, NUL or
/// whitespace. Profiles never restricted names further, and a stricter rule
/// here would let a variable the tester never touched block every save.
fn is_env_name(name: &str) -> bool {
    !name.is_empty()
        && !name
            .chars()
            .any(|c| c == '=' || c == '\0' || c.is_whitespace())
}

/// Split a line into words: whitespace separates, `'…'` and `"…"` keep
/// spaces together, and inside `"…"` a `\"` is a quote. A backslash is
/// otherwise just a character — these lines carry Windows paths
/// (`+exec configs\lan.cfg`, `C:\games\q3`) far more often than shell
/// escapes. An unclosed quote is an error rather than a guess.
pub fn split_words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err("'".into()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        // `\"` is a quote — unless it ends the word, where it
                        // is the backslash of a Windows path closing:
                        // `"C:\My Games\" "D:\Saves\"`. Paths ending in a
                        // backslash are what these lines carry; a quote
                        // right before a space goes in single quotes.
                        Some('\\') if chars.peek() == Some(&'"') => {
                            let mut ahead = chars.clone();
                            ahead.next();
                            if ahead.peek().is_none_or(|c| c.is_whitespace()) {
                                word.push('\\');
                            } else {
                                chars.next();
                                word.push('"');
                            }
                        }
                        Some(c) => word.push(c),
                        None => return Err("\"".into()),
                    }
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

/// The inverse of [`split_words`]: words that need it go in quotes.
pub fn join_words(words: &[String]) -> String {
    words
        .iter()
        .map(|w| {
            let plain = !w.is_empty()
                && !w
                    .chars()
                    .any(|c| c.is_whitespace() || matches!(c, '"' | '\''));
            if plain {
                w.clone()
            } else {
                // Single quotes take everything literally, a backslash before
                // the closing quote included; an apostrophe closes them, goes
                // in double quotes on its own, and they open again.
                format!("'{}'", w.replace('\'', "'\"'\"'"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// What the project needs to know about the machine a configuration was
/// found on: the same game can want other settings on NVIDIA than on AMD,
/// and a Proton version that is not everywhere.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestContext {
    pub launcher_version: String,
    pub platform: String,
    pub os: String,
    pub device: Option<String>,
    pub cpu: String,
    pub gpu: Vec<String>,
    /// The tool the last start ran with, as the plan names it.
    pub runner: String,
}

/// The test context of this machine. Reads `/sys` on Linux; elsewhere what
/// `sysinfo` knows. Everything is best effort — a field it cannot fill stays
/// empty rather than failing the report.
pub fn this_machine(launcher_version: &str, runner: &str) -> TestContext {
    let cpu = crate::lanpage::cpu_brand();
    TestContext {
        launcher_version: launcher_version.to_string(),
        platform: Manifest::current_platform().to_string(),
        os: sysinfo::System::long_os_version().unwrap_or_default(),
        device: device_name(Path::new("/sys/devices/virtual/dmi/id")),
        cpu,
        gpu: gpus(Path::new("/sys/class/drm")),
        runner: runner.to_string(),
    }
}

/// `Steam Deck (LCD)` and the like, from the DMI vendor and product name.
pub fn device_name(dmi: &Path) -> Option<String> {
    let read = |name: &str| {
        std::fs::read_to_string(dmi.join(name))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let vendor = read("sys_vendor").or_else(|| read("board_vendor"));
    let product = read("product_name")?;
    Some(match (vendor.as_deref(), product.as_str()) {
        (Some("Valve"), "Jupiter") => "Steam Deck (LCD)".into(),
        (Some("Valve"), "Galileo") => "Steam Deck (OLED)".into(),
        (Some(vendor), product) => format!("{vendor} {product}"),
        (None, product) => product.to_string(),
    })
}

/// The graphics cards under `/sys/class/drm`, by vendor and kernel driver
/// (`AMD (amdgpu)`, `NVIDIA (nvidia)`). The PCI ids are enough for the
/// question that matters here — which vendor's quirks apply.
pub fn gpus(drm: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(drm) else {
        return Vec::new();
    };
    let mut cards: Vec<_> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.starts_with("card") && !name.contains('-')
        })
        .map(|e| e.path())
        .collect();
    cards.sort();
    let mut out = Vec::new();
    for card in cards {
        let device = card.join("device");
        let vendor = std::fs::read_to_string(device.join("vendor"))
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let name = match vendor.as_str() {
            "0x1002" => "AMD".to_string(),
            "0x10de" => "NVIDIA".to_string(),
            "0x8086" => "Intel".to_string(),
            "" => continue,
            other => other.to_string(),
        };
        let driver = std::fs::read_link(device.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));
        let entry = match driver {
            Some(driver) => format!("{name} ({driver})"),
            None => name,
        };
        if !out.contains(&entry) {
            out.push(entry);
        }
    }
    out
}

/// A configuration report: what a tester sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub subject: String,
    pub body: String,
    pub toml: String,
    /// `mailto:` link with subject and body filled in.
    pub mailto: String,
    /// A new issue in the project repository with title and body filled in.
    pub issue_url: String,
    /// File name to offer when the report is saved instead of sent.
    pub file_name: String,
}

/// The report for a working configuration of `game_id`.
pub fn report(
    game_id: &str,
    title: &str,
    revision: &str,
    toml: &str,
    block: Option<&str>,
    context: &TestContext,
    comment: &str,
) -> Report {
    let subject = format!(
        "[game-config] {} ({game_id}) on {}",
        if title.is_empty() { game_id } else { title },
        context.platform
    );
    let mut head = String::new();
    head.push_str(&format!("Game: {title} ({game_id}), revision {revision}\n"));
    head.push_str(&format!("Launcher: {}\n", context.launcher_version));
    head.push_str(&format!(
        "Platform: {} – {}\n",
        context.platform, context.os
    ));
    if let Some(device) = &context.device {
        head.push_str(&format!("Device: {device}\n"));
    }
    if !context.cpu.is_empty() {
        head.push_str(&format!("CPU: {}\n", context.cpu));
    }
    if !context.gpu.is_empty() {
        head.push_str(&format!("GPU: {}\n", context.gpu.join(", ")));
    }
    if !context.runner.is_empty() {
        head.push_str(&format!("Ran with: {}\n", context.runner));
    }
    let comment = comment.trim();
    if !comment.is_empty() {
        head.push_str(&format!("\nNotes:\n{comment}\n"));
    }
    let body = format!("{head}\nProfile ({game_id}.toml):\n```toml\n{toml}```\n");
    // The links carry the platform's block only: the whole profile, notes in
    // two languages and all, overruns what mail programs and GitHub take in
    // a URL. And even the block may be too long — then the link says so and
    // the profile goes by copy or file.
    let short = match block {
        Some(block) => format!(
            "{head}\nLaunch configuration ({game_id}, [platform.{}]):\n```toml\n{block}```\n",
            context.platform
        ),
        None => body.clone(),
    };
    let link_body = if percent_encode(&short).len() <= LINK_BODY_LIMIT {
        short
    } else {
        // Without the profile — and without a note long enough to overrun
        // the link on its own.
        let head = cut_for_link(&head, LINK_BODY_LIMIT / 2);
        format!("{head}\n(The profile is too long for a link: please attach the saved file or paste the copied report.)\n")
    };
    Report {
        mailto: format!(
            "mailto:{REPORT_EMAIL}?subject={}&body={}",
            percent_encode(&subject),
            percent_encode(&link_body)
        ),
        issue_url: format!(
            "https://github.com/{REPORT_REPOSITORY}/issues/new?title={}&body={}",
            percent_encode(&subject),
            percent_encode(&link_body)
        ),
        file_name: format!("{game_id}-{}.toml", context.platform),
        subject,
        body,
        toml: toml.to_string(),
    }
}

/// The game a problem report is about.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BugGame {
    pub id: String,
    pub title: String,
    /// Installed package revision, else the catalog's.
    pub revision: String,
    /// Which start profile applies and whether it is confirmed.
    pub profile: String,
    /// What a start runs (runner and command line), when it can be planned.
    pub start: Option<String>,
}

/// A problem report: about `game`, or about the launcher in general.
/// `log` is an excerpt of the launcher's log; it goes into the copied text
/// and the saved file, not into the links (too long, and a link is sent
/// before the reporter could read it).
pub fn bug_report(
    game: Option<&BugGame>,
    context: &TestContext,
    comment: &str,
    log: &str,
) -> Report {
    let what = match game {
        Some(g) => format!(
            "{} ({})",
            if g.title.is_empty() { &g.id } else { &g.title },
            g.id
        ),
        None => "Launcher".to_string(),
    };
    let subject = format!("[bug] {what} on {}", context.platform);
    let mut head = String::new();
    let comment = comment.trim();
    head.push_str("What happened:\n");
    head.push_str(if comment.is_empty() {
        "(no description)"
    } else {
        comment
    });
    head.push_str("\n\n");
    if let Some(g) = game {
        head.push_str(&format!("Game: {what}, revision {}\n", g.revision));
        head.push_str(&format!("Profile: {}\n", g.profile));
        if let Some(start) = &g.start {
            head.push_str(&format!("Start: {start}\n"));
        }
    }
    head.push_str(&format!("Launcher: {}\n", context.launcher_version));
    head.push_str(&format!(
        "Platform: {} – {}\n",
        context.platform, context.os
    ));
    if let Some(device) = &context.device {
        head.push_str(&format!("Device: {device}\n"));
    }
    if !context.cpu.is_empty() {
        head.push_str(&format!("CPU: {}\n", context.cpu));
    }
    if !context.gpu.is_empty() {
        head.push_str(&format!("GPU: {}\n", context.gpu.join(", ")));
    }
    if !context.runner.is_empty() {
        head.push_str(&format!("Ran with: {}\n", context.runner));
    }
    let body = if log.trim().is_empty() {
        head.clone()
    } else {
        format!("{head}\nLog excerpt:\n```\n{}\n```\n", log.trim_end())
    };
    let link_body = format!(
        "{}\n(A log excerpt is in the copied report or the saved file; please paste or attach it.)\n",
        cut_for_link(&head, LINK_BODY_LIMIT - 400)
    );
    let name = game.map_or("launcher", |g| g.id.as_str());
    Report {
        mailto: format!(
            "mailto:{REPORT_EMAIL}?subject={}&body={}",
            percent_encode(&subject),
            percent_encode(&link_body)
        ),
        issue_url: format!(
            "https://github.com/{REPORT_REPOSITORY}/issues/new?title={}&body={}",
            percent_encode(&subject),
            percent_encode(&link_body)
        ),
        file_name: format!("bug-{name}-{}.txt", context.platform),
        toml: body.clone(),
        subject,
        body,
    }
}

/// `text` cut to `limit` bytes once encoded for a link: one emoji is twelve
/// bytes in a URL.
fn cut_for_link(text: &str, limit: usize) -> String {
    let mut kept = String::new();
    let mut size = 0;
    for c in text.chars() {
        size += percent_encode(c.encode_utf8(&mut [0; 4])).len();
        if size > limit {
            kept.push_str(" …\n");
            break;
        }
        kept.push(c);
    }
    kept
}

/// The last `max` lines of a log that concern `game` (all lines without
/// one), without the state machine's `tick` lines, and with anything that
/// looks like a key replaced: share secrets and API keys are long runs of
/// capitals and digits, and a report goes to a public issue tracker.
pub fn log_excerpt(log: &str, game: Option<&str>, max: usize) -> String {
    let wanted = |line: &str| {
        !line.contains("] tick: ")
            && game.is_none_or(|id| {
                line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                    .any(|word| word == id)
            })
    };
    let lines: Vec<&str> = log.lines().filter(|l| wanted(l)).collect();
    let from = lines.len().saturating_sub(max);
    lines[from..]
        .iter()
        .map(|line| redact_keys(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Keys in a log line blanked: long runs of capitals and digits (share
/// secrets, API keys) and dash-grouped CD keys (`ABCD-EFGH-1234-5678`).
fn redact_keys(line: &str) -> String {
    let grouped = |token: &str| {
        let groups: Vec<&str> = token.split('-').collect();
        groups.len() >= 3
            && groups.iter().all(|g| {
                (4..=6).contains(&g.len())
                    && g.chars().all(|c| c.is_ascii_alphanumeric())
                    && !g.chars().any(|c| c.is_ascii_lowercase())
            })
            && token.chars().any(|c| c.is_ascii_digit())
    };
    let line: String = line
        .split_inclusive(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .map(|piece| {
            let end = piece
                .char_indices()
                .last()
                .filter(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '-'))
                .map_or(piece.len(), |(i, _)| i);
            if grouped(&piece[..end]) {
                format!("[key]{}", &piece[end..])
            } else {
                piece.to_string()
            }
        })
        .collect();
    let mut out = String::with_capacity(line.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        let key = word.len() >= 20
            && word
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
            && word.chars().any(|c| c.is_ascii_digit());
        out.push_str(if key { "[key]" } else { word });
        word.clear();
    };
    for c in line.chars() {
        if c.is_ascii_alphanumeric() {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// How long an encoded body may be in a link. GitHub refuses issue URLs
/// past about 8 KB; some mail handlers cut far earlier, so this stays well
/// below both and the full report is always there to copy.
const LINK_BODY_LIMIT: usize = 6000;

/// Percent-encoding for a URL query value: everything but RFC 3986's
/// unreserved characters. Spaces become `%20`, not `+` — mail programs read
/// a `+` in a `mailto:` body as a plus sign.
pub fn percent_encode(text: &str) -> String {
    crate::transport::resilio::urlencoding(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{LaunchSpec, ManifestOrigin};

    #[test]
    fn a_bug_report_keeps_the_log_out_of_its_links() {
        let context = TestContext {
            launcher_version: "0.2.0 (abc)".into(),
            platform: "linux".into(),
            os: "Bazzite 44".into(),
            ..Default::default()
        };
        let game = BugGame {
            id: "flat2".into(),
            title: "FlatOut 2".into(),
            revision: "20160922".into(),
            profile: "bundled, confirmed".into(),
            start: Some("Proton 11.0: cmd.exe /c .nll-start-run.cmd".into()),
        };
        let r = bug_report(
            Some(&game),
            &context,
            "Freezes after the intro",
            "line one\nline two",
        );
        assert_eq!(r.subject, "[bug] FlatOut 2 (flat2) on linux");
        assert!(r.body.contains("Freezes after the intro"));
        assert!(r.body.contains("line two"));
        assert!(!r.issue_url.contains("line%20two"));
        assert!(r.issue_url.contains("Freezes%20after"));
        assert_eq!(r.file_name, "bug-flat2-linux.txt");
        let general = bug_report(None, &context, "", "");
        assert_eq!(general.subject, "[bug] Launcher on linux");
        assert_eq!(general.file_name, "bug-launcher-linux.txt");
    }

    #[test]
    fn a_log_excerpt_keeps_the_games_lines_and_hides_keys() {
        let log = "[x][INFO] tick: flat2 phase=Ready\n\
                   [x][INFO] starting flat2 via Proton\n\
                   [x][INFO] starting flat22 via Proton\n\
                   [x][INFO] share flat2 secret AB12CD34EF56GH78IJ90KL registered\n\
                   [x][INFO] key ABCD-EF12-GH34-IJ56 set\n\
                   [x][INFO] other game";
        let excerpt = log_excerpt(log, Some("flat2"), 10);
        assert_eq!(
            excerpt,
            "[x][INFO] starting flat2 via Proton\n[x][INFO] share flat2 secret [key] registered"
        );
        assert_eq!(log_excerpt(log, None, 1), "[x][INFO] other game");
        assert_eq!(
            log_excerpt(log, None, 2).lines().next(),
            Some("[x][INFO] key [key] set")
        );
    }

    #[test]
    fn words_split_on_spaces_keep_quotes_and_windows_paths() {
        let words =
            split_words(r#"-game cstrike "+name Neo" 'a b' "x\"y" +exec configs\lan.cfg"#).unwrap();
        assert_eq!(
            words,
            [
                "-game",
                "cstrike",
                "+name Neo",
                "a b",
                "x\"y",
                "+exec",
                r"configs\lan.cfg"
            ]
        );
        assert_eq!(
            split_words(r"-path C:\games\q3 \\server\lan").unwrap(),
            ["-path", r"C:\games\q3", r"\\server\lan"]
        );
        assert_eq!(
            split_words(r#"-path "C:\My Games\" -w"#).unwrap(),
            ["-path", r"C:\My Games\", "-w"],
            "a quoted path may end in a backslash"
        );
        assert_eq!(
            split_words(r#"-path "C:\My Games\" -save "D:\Saves\""#).unwrap(),
            ["-path", r"C:\My Games\", "-save", r"D:\Saves\"],
            "two quoted paths, each ending in a backslash"
        );
        assert_eq!(
            split_words(r#"'say "hello world" now' "a\"b""#).unwrap(),
            [r#"say "hello world" now"#, "a\"b"],
            "quotes inside: single quotes, or an escape not before a space"
        );
        for words in [
            words.clone(),
            vec![
                "it's".to_string(),
                r#"say "hi""#.into(),
                r"C:\a b\c".into(),
                r"C:\Bob's Games\".into(),
                String::new(),
            ],
        ] {
            assert_eq!(
                split_words(&join_words(&words)).unwrap(),
                words,
                "{words:?}"
            );
        }
        assert_eq!(split_words("  ").unwrap(), Vec::<String>::new());
        assert_eq!(
            split_words(r#""""#).unwrap(),
            [""],
            "an empty argument stays one"
        );
        assert!(split_words(r#"-name "Neo"#).is_err(), "unclosed quote");
        assert!(split_words("'open").is_err());
    }

    fn base() -> Manifest {
        let mut m = Manifest {
            id: "q3".into(),
            title: Some("Quake III".into()),
            revisions: vec!["20240101".into()],
            launch: LaunchSpec {
                exe: "quake3.exe".into(),
                args: vec!["+set".into(), "name".into(), "%player%".into()],
                workdir: Some("bin".into()),
                env: BTreeMap::from([("WINEDEBUG".into(), "-all".into())]),
                ..Default::default()
            },
            ..Default::default()
        };
        m.setup.notes.insert("de".into(), "Hinweis".into());
        m.platform.insert(
            "macos".into(),
            PlatformOverride {
                runner: Some(Runner::Crossover),
                ..Default::default()
            },
        );
        m.platform.insert(
            "linux".into(),
            PlatformOverride {
                runner: Some(Runner::Proton),
                env: BTreeMap::from([("PROFILE_VAR".into(), "1".into())]),
                ..Default::default()
            },
        );
        m
    }

    fn laid(profile: &Manifest, block: PlatformOverride) -> Manifest {
        let mut m = profile.clone();
        ConfigOverlay {
            id: "q3".into(),
            revision: Some("20240101".into()),
            platform: BTreeMap::from([("linux".into(), block)]),
            ..Default::default()
        }
        .overlay_onto(&mut m, "linux");
        m
    }

    #[test]
    fn the_editor_shows_what_actually_starts() {
        let mut m = base();
        let linux = m.platform.get_mut("linux").unwrap();
        linux.env.insert(DLL_OVERRIDES.into(), "dinput8=n,b".into());
        linux.wrapper = Some(vec!["gamemoderun".into()]);
        let config = GameConfig::from_manifest(Some(&m), "linux", None);
        assert_eq!(config.exe, "quake3.exe");
        assert_eq!(config.args, "+set name %player%");
        assert_eq!(config.workdir, "bin");
        assert_eq!(config.runner, Runner::Proton);
        assert_eq!(config.dll_overrides, "dinput8=n,b");
        assert_eq!(config.wrapper, "gamemoderun");
        assert_eq!(config.env.len(), 2, "[launch]'s and the platform's");

        // The receipt's choice starts bare, as `resolve_exe` starts it.
        let chosen = GameConfig::from_manifest(Some(&m), "linux", Some("tools/Config.exe"));
        assert_eq!(
            (
                chosen.exe.as_str(),
                chosen.args.as_str(),
                chosen.workdir.as_str()
            ),
            ("tools/Config.exe", "", "")
        );

        // `[launch] workdir = ""` is local/ — shown as `.`.
        let mut local = m.clone();
        local.launch.workdir = Some(String::new());
        assert_eq!(
            GameConfig::from_manifest(Some(&local), "linux", None).workdir,
            "."
        );

        // A share's wrapper does not run, so it is not shown — saving would
        // otherwise make it run.
        m.origin = ManifestOrigin::ShareOverlay;
        assert_eq!(
            GameConfig::from_manifest(Some(&m), "linux", None).wrapper,
            ""
        );
        assert_eq!(GameConfig::from_manifest(None, "linux", None).exe, "");
    }

    #[test]
    fn a_block_keeps_only_what_changed_so_profile_fixes_arrive() {
        let profile = base();
        let mut config = GameConfig::from_manifest(Some(&profile), "linux", None);
        assert!(
            config
                .to_block(Some(&profile), "q3", "linux")
                .unwrap()
                .is_empty(),
            "an unchanged editor changes nothing"
        );
        config.dll_overrides = "dinput8=n,b".into();
        config.wrapper = "gamemoderun".into();
        config.env.retain(|v| v.name != "WINEDEBUG");
        config.env.push(EnvVar {
            name: "PROTON_USE_WINED3D".into(),
            value: "1".into(),
        });
        let block = config.to_block(Some(&profile), "q3", "linux").unwrap();
        assert_eq!(block.exe, None);
        assert_eq!(block.args, None);
        assert_eq!(block.workdir, None);
        assert_eq!(block.runner, None);
        assert_eq!(block.wrapper.as_deref().unwrap(), ["gamemoderun"]);
        assert_eq!(block.unset_env, ["WINEDEBUG"]);
        assert_eq!(
            block.env.keys().collect::<Vec<_>>(),
            ["PROTON_USE_WINED3D", "WINEDLLOVERRIDES"]
        );

        let m = laid(&profile, block.clone());
        assert!(m.user_config && m.wrapper_is_trusted());
        let linux = m.launch_for("linux");
        assert_eq!(
            linux.runner,
            Runner::Proton,
            "the profile's own block stays"
        );
        assert_eq!(linux.env["PROFILE_VAR"], "1");
        assert!(!linux.env.contains_key("WINEDEBUG"));
        assert_eq!(linux.wrapper, ["gamemoderun"]);
        assert_eq!(m.launch_for("macos"), profile.launch_for("macos"));

        // The next release fixes the exe: it arrives, the tester's change stays.
        let mut fixed = profile.clone();
        fixed.launch.exe = "fixed/quake3.exe".into();
        let m = laid(&fixed, block);
        assert_eq!(m.launch_for("linux").exe, "fixed/quake3.exe");
        assert_eq!(m.launch_for("linux").wrapper, ["gamemoderun"]);

        // And the editor shows back what is in force.
        let back = GameConfig::from_manifest(Some(&m), "linux", None);
        assert_eq!(back.wrapper, "gamemoderun");
        assert_eq!(back.dll_overrides, "dinput8=n,b");
    }

    #[test]
    fn working_folders_keep_their_meaning() {
        let profile = base();
        let with = |workdir: &str| {
            let config = GameConfig {
                workdir: workdir.into(),
                ..GameConfig::from_manifest(Some(&profile), "linux", None)
            };
            let block = config.to_block(Some(&profile), "q3", "linux").unwrap();
            (
                block.workdir.clone(),
                laid(&profile, block).launch_for("linux").workdir,
            )
        };
        assert_eq!(with("bin"), (None, Some("bin".into())), "unchanged");
        assert_eq!(with(""), (Some(String::new()), None), "the exe's folder");
        assert_eq!(
            with("."),
            (Some(".".into()), Some(".".into())),
            "local/ itself"
        );
        assert_eq!(
            with(r"bin\"),
            (None, Some("bin".into())),
            "a trailing separator is the same folder"
        );

        // An exe chosen in the receipt started bare; the block says so.
        let chosen = GameConfig::from_manifest(Some(&profile), "linux", Some("tools/Config.exe"));
        let block = chosen.to_block(Some(&profile), "q3", "linux").unwrap();
        assert_eq!(block.exe.as_deref(), Some("tools/Config.exe"));
        assert_eq!(block.args.as_deref(), Some(&[][..]));
        assert_eq!(block.workdir.as_deref(), Some(""));
    }

    #[test]
    fn components_are_kept_as_verbs_and_refused_as_options() {
        let profile = base();
        let config = GameConfig {
            winetricks: "  directplay   vcrun2010 sound=alsa ".into(),
            ..GameConfig::from_manifest(Some(&profile), "linux", None)
        };
        let block = config.to_block(Some(&profile), "q3", "linux").unwrap();
        assert_eq!(
            block.winetricks.as_deref().unwrap(),
            ["directplay", "vcrun2010", "sound=alsa"]
        );
        let m = laid(&profile, block);
        assert_eq!(
            m.launch_for("linux").winetricks,
            ["directplay", "vcrun2010", "sound=alsa"]
        );
        assert_eq!(
            GameConfig::from_manifest(Some(&m), "linux", None).winetricks,
            "directplay vcrun2010 sound=alsa"
        );
        for bad in ["--force", "a;b", "$(x)"] {
            let config = GameConfig {
                winetricks: format!("directplay {bad}"),
                ..config.clone()
            };
            assert_eq!(
                config
                    .to_block(Some(&profile), "q3", "linux")
                    .unwrap_err()
                    .0,
                format!("err.config_verb|{bad}")
            );
        }
    }

    #[test]
    fn a_config_that_would_not_load_is_refused_with_a_reason() {
        let ok = GameConfig {
            exe: "a.exe".into(),
            ..Default::default()
        };
        let refuse = |config: GameConfig| config.to_block(None, "g", "linux").unwrap_err().0;
        assert_eq!(refuse(GameConfig::default()), "err.config_exe_missing");
        assert!(refuse(GameConfig {
            exe: "../x.exe".into(),
            ..ok.clone()
        })
        .starts_with("err.config_path|"));
        assert!(refuse(GameConfig {
            workdir: "/etc".into(),
            ..ok.clone()
        })
        .starts_with("err.config_path|"));
        assert!(refuse(GameConfig {
            args: "\"open".into(),
            ..ok.clone()
        })
        .starts_with("err.config_quotes|"));
        let env = |name: &str| GameConfig {
            env: vec![EnvVar {
                name: name.into(),
                value: "x".into(),
            }],
            ..ok.clone()
        };
        assert_eq!(refuse(env("A=B")), "err.config_env_name|A=B");
        assert!(
            env("dxvk.conf-path").to_block(None, "g", "linux").is_ok(),
            "a name the system takes is not refused"
        );
        let fresh = ok.to_block(None, "g", "linux").unwrap();
        assert_eq!(
            fresh.exe.as_deref(),
            Some("a.exe"),
            "a game without a profile"
        );
    }

    #[test]
    fn the_overlay_file_keeps_other_platforms_and_goes_when_empty() {
        let block = |exe: &str| PlatformOverride {
            exe: Some(exe.into()),
            ..Default::default()
        };
        let write = |overlay, platform, block| {
            updated_overlay(overlay, "q3", platform, block, "20250101").unwrap()
        };
        let parse = |text: &str| ConfigOverlay::parse(text, Path::new("q3.toml"), "q3").unwrap();
        let overlay = parse(&write(None, "linux", Some(block("a.exe"))).unwrap());
        assert_eq!(overlay.revision.as_deref(), Some("20250101"));
        let overlay = parse(&write(Some(overlay), "macos", Some(block("b.exe"))).unwrap());
        assert_eq!(overlay.platform.len(), 2);
        let overlay = parse(&write(Some(overlay), "macos", None).unwrap());
        assert_eq!(overlay.platform.keys().collect::<Vec<_>>(), ["linux"]);
        assert_eq!(
            write(
                Some(overlay.clone()),
                "linux",
                Some(PlatformOverride::default())
            ),
            None,
            "a block that changes nothing is no configuration"
        );
        assert_eq!(write(Some(overlay), "linux", None), None);

        let text = write(None, "linux", Some(block("a.exe"))).unwrap();
        assert!(
            ConfigOverlay::parse(&text, Path::new("x.toml"), "other").is_err(),
            "wrong id"
        );
        let escaping = "schema = 1\nid = \"q3\"\n[platform.linux]\nexe = \"../../bin/sh\"\n";
        assert!(ConfigOverlay::parse(escaping, Path::new("q3.toml"), "q3").is_err());
    }

    /// End to end through the store: a saved configuration is what the game
    /// then starts with, over the bundled profile, and a fix to the bundled
    /// profile still arrives.
    #[test]
    fn the_store_lays_a_saved_configuration_over_the_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let (bundled, configs) = (tmp.path().join("bundled"), tmp.path().join("configs"));
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::create_dir_all(&configs).unwrap();
        let write_bundled = |exe: &str, note: &str| {
            let mut m = base();
            m.launch.exe = exe.into();
            m.setup.notes.insert("de".into(), note.into());
            std::fs::write(bundled.join("q3.toml"), to_toml(&m).unwrap()).unwrap();
        };
        write_bundled("quake3.exe", "alt");
        let store = crate::manifest::ManifestStore::new(None, Some(bundled.clone()))
            .with_config_dir(configs.clone());
        let paths = crate::paths::GamePaths::new(&tmp.path().join("lib"), "q3");
        let platform = Manifest::current_platform();
        let profile = store.resolve_profile_for("q3", &paths).unwrap();
        assert!(!profile.user_config);

        let config = GameConfig {
            wrapper: "gamemoderun".into(),
            ..GameConfig::from_manifest(Some(&profile), platform, None)
        };
        let block = config.to_block(Some(&profile), "q3", platform).unwrap();
        let text = updated_overlay(store.load_config("q3"), "q3", platform, Some(block), "1")
            .unwrap()
            .unwrap();
        std::fs::write(store.config_path("q3").unwrap(), text).unwrap();

        write_bundled("fixed.exe", "neu");
        let after = store.resolve_for("q3", &paths).unwrap();
        assert!(after.user_config);
        assert_eq!(after.config_revision.as_deref(), Some("1"));
        assert_eq!(after.origin, ManifestOrigin::Bundled);
        assert_eq!(after.launch_for(platform).wrapper, ["gamemoderun"]);
        assert_eq!(
            after.launch_for(platform).exe,
            "fixed.exe",
            "the bundled fix arrives"
        );
        assert_eq!(after.setup.notes["de"], "neu");
        assert!(!store.resolve_profile_for("q3", &paths).unwrap().user_config);

        // A game without any profile gets one from the configuration alone.
        let solo = GameConfig {
            exe: "s.exe".into(),
            ..Default::default()
        }
        .to_block(None, "solo", platform)
        .unwrap();
        let text = updated_overlay(None, "solo", platform, Some(solo), "")
            .unwrap()
            .unwrap();
        std::fs::write(store.config_path("solo").unwrap(), text).unwrap();
        let solo_paths = crate::paths::GamePaths::new(&tmp.path().join("lib"), "solo");
        let solo = store.resolve_for("solo", &solo_paths).unwrap();
        assert_eq!(solo.launch_for(platform).exe, "s.exe");
        assert!(store.config_path("../evil").is_none());
    }

    #[test]
    fn the_shared_profile_is_a_file_the_launcher_loads() {
        let profile = base();
        let config = GameConfig {
            runner: Runner::Wine,
            wrapper: "gamescope -w 1280 -h 800 --".into(),
            ..GameConfig::from_manifest(Some(&profile), "linux", None)
        };
        let block = config.to_block(Some(&profile), "q3", "linux").unwrap();
        let shared = with_block(Some(&profile), "q3", "", "linux", &block);
        assert_eq!(
            shared.revisions,
            ["20240101"],
            "one platform's test speaks for no other"
        );
        let linux = &shared.platform["linux"];
        assert_eq!(linux.runner, Some(Runner::Wine));
        assert_eq!(
            linux.env["PROFILE_VAR"], "1",
            "merged with the profile's block"
        );
        let text = to_toml(&shared).unwrap();
        for noise in [
            "wrapper = []",
            "[platform.macos.env]",
            "touch = []",
            "copy = []",
        ] {
            assert!(!text.contains(noise), "empty `{noise}` in\n{text}");
        }
        let loaded = Manifest::parse(&text, Path::new("q3.toml")).unwrap();
        assert_eq!(
            loaded.launch_for("linux").wrapper,
            ["gamescope", "-w", "1280", "-h", "800", "--"]
        );
        let link = block_toml("q3", "linux", linux).unwrap();
        assert!(
            link.contains("[platform.linux]") && !link.contains("[launch]"),
            "{link}"
        );
    }

    #[test]
    fn the_report_carries_the_profile_the_context_and_working_links() {
        let context = TestContext {
            launcher_version: "0.2.0 (abc1234)".into(),
            platform: "linux".into(),
            os: "Linux 3.6 SteamOS".into(),
            device: Some("Steam Deck (OLED)".into()),
            cpu: String::new(),
            gpu: vec!["AMD (amdgpu)".into()],
            runner: "Proton 9.0 (Beta)".into(),
        };
        let r = report(
            "q3",
            "Quake III",
            "20240101",
            "id = \"q3\"\n",
            Some("[platform.linux]\nrunner = \"proton\"\n"),
            &context,
            "LAN mit 4 Leuten & Bots",
        );
        assert_eq!(r.subject, "[game-config] Quake III (q3) on linux");
        assert!(r.body.contains("Device: Steam Deck (OLED)"));
        assert!(r.body.contains("GPU: AMD (amdgpu)"));
        assert!(r.body.contains("Ran with: Proton 9.0 (Beta)"));
        assert!(r.body.contains("LAN mit 4 Leuten & Bots"));
        assert!(r.body.contains("```toml\nid = \"q3\"\n```"));
        assert!(!r.body.contains("CPU:"), "an empty field is left out");
        assert!(r
            .mailto
            .starts_with("mailto:launcher@schimnick.de?subject=%5Bgame-config%5D%20Quake"));
        assert!(!r.mailto.contains('+') && !r.mailto.contains(' '));
        assert!(
            r.mailto.contains("%26%20Bots"),
            "an ampersand must not end the body"
        );
        assert!(r
            .issue_url
            .starts_with("https://github.com/L3t4l3s/NextGen-LAN-Launcher/issues/new?title="));
        assert_eq!(r.file_name, "q3-linux.toml");
        let link = |url: &str| url.split("body=").nth(1).unwrap().to_string();
        assert!(
            link(&r.mailto).contains("runner%20%3D%20%22proton%22"),
            "the block rides in the link"
        );
        assert!(
            !link(&r.mailto).contains("id%20%3D%20%22q3%22%0A%60"),
            "the full profile does not"
        );

        // A block too long for a link leaves the link pointing at copy or file.
        let long = "x".repeat(LINK_BODY_LIMIT);
        let r = report("q3", "", "1", "id = \"q3\"\n", Some(&long), &context, "");
        assert!(r.mailto.len() < LINK_BODY_LIMIT + 300);
        assert!(r.mailto.contains("too%20long%20for%20a%20link"));
        assert!(r.body.contains("id = \"q3\""), "the copy keeps everything");
        let chatty = "Log: ".to_string() + &"zeile ".repeat(3000);
        let r = report(
            "q3",
            "",
            "1",
            "id = \"q3\"\n",
            Some(&long),
            &context,
            &chatty,
        );
        assert!(
            r.mailto.len() < LINK_BODY_LIMIT,
            "a long note does not overrun the link"
        );
        let emoji = "🎮".repeat(2000);
        let r = report(
            "q3",
            "",
            "1",
            "id = \"q3\"\n",
            Some(&long),
            &context,
            &emoji,
        );
        assert!(
            r.mailto.len() < LINK_BODY_LIMIT,
            "nor does one that encodes long"
        );
        assert!(r.body.contains(&emoji), "the copy keeps the whole note");
    }

    #[test]
    fn a_steam_deck_and_its_gpu_are_named_from_sys() {
        let tmp = tempfile::tempdir().unwrap();
        let dmi = tmp.path().join("dmi");
        std::fs::create_dir_all(&dmi).unwrap();
        std::fs::write(dmi.join("sys_vendor"), "Valve\n").unwrap();
        std::fs::write(dmi.join("product_name"), "Galileo\n").unwrap();
        assert_eq!(device_name(&dmi).as_deref(), Some("Steam Deck (OLED)"));
        std::fs::write(dmi.join("sys_vendor"), "LENOVO\n").unwrap();
        std::fs::write(dmi.join("product_name"), "21CB\n").unwrap();
        assert_eq!(device_name(&dmi).as_deref(), Some("LENOVO 21CB"));
        assert_eq!(device_name(&tmp.path().join("none")), None);

        let drm = tmp.path().join("drm");
        for (card, vendor) in [("card0", "0x1002"), ("card1", "0x10de")] {
            std::fs::create_dir_all(drm.join(card).join("device")).unwrap();
            std::fs::write(drm.join(card).join("device/vendor"), format!("{vendor}\n")).unwrap();
        }
        // A connector entry is no card.
        std::fs::create_dir_all(drm.join("card0-eDP-1")).unwrap();
        #[cfg(unix)]
        {
            let driver = tmp.path().join("drivers/amdgpu");
            std::fs::create_dir_all(&driver).unwrap();
            std::os::unix::fs::symlink(&driver, drm.join("card0/device/driver")).unwrap();
            assert_eq!(gpus(&drm), ["AMD (amdgpu)", "NVIDIA"]);
        }
    }
}
