//! Heuristic reader for ETI `game_start.cmd` scripts.
//!
//! Most ETI launch scripts follow one template:
//!
//! ```bat
//! set game_path=%1 ... cd local
//! netsh advfirewall firewall add rule name="%game_id%" dir=in action=allow program="%game_path%\local\quake3.exe" ...
//! "quake3.exe" +set fs_game baseq3
//! netsh advfirewall firewall delete rule name="%game_id%" >nul
//! ```
//!
//! From that we can derive the executable and its arguments without running
//! any batch code, which is what macOS/Linux need and what lets Windows users
//! see *what* will be started. Scripts with interactive menus (`set /P`) or
//! several executables are reported as such so the UI can ask the user.

use crate::manifest::{Alternative, LaunchSpec, Manifest, ManifestOrigin, Runner};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

static FIREWALL_PROGRAM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)program="%game_path%\\local\\([^"]+)""#).expect("valid regex")
});
/// A line that invokes a quoted executable, optionally via `start`.
static INVOKE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)^\s*(?:start\s+(?:(?:/\w+\s+)|(?:"[^"]*"\s+))*)?"([^"\n]+?\.exe)"\s*(.*?)\s*$"#,
    )
    .expect("valid regex")
});
static INTERACTIVE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*set\s+/p\s").expect("valid regex"));
static REGISTRY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*reg(?:\.exe)?\s+add\s").expect("valid regex"));
static LANG_BRANCH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)%game_lang%").expect("valid regex"));

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeCandidate {
    /// Path relative to `local/` using forward slashes.
    pub exe: String,
    pub args: Vec<String>,
    /// Seen both in the firewall rule and an invocation line → high.
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptProbe {
    pub candidates: Vec<ProbeCandidate>,
    /// Script asks the user something (`set /P`) → cannot be automated.
    pub interactive: bool,
    /// Script writes to the registry → needs Windows or manual steps elsewhere.
    pub writes_registry: bool,
    /// Script branches on the game language.
    pub language_dependent: bool,
    /// Script uses the player name (`%player%`) beyond the header.
    pub uses_player_name: bool,
}

fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for ch in s.chars() {
        match ch {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    // drop trailing redirections like `>nul`
    out.retain(|a| !a.starts_with('>') && !a.starts_with("2>"));
    out
}

fn normalise(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").to_string()
}

impl ScriptProbe {
    pub fn analyse(script: &str) -> Self {
        let mut probe = ScriptProbe::default();
        let mut firewall: Vec<String> = Vec::new();
        let mut invoked: Vec<(String, Vec<String>)> = Vec::new();
        let mut header_done = false;

        for raw in script.lines() {
            let line = raw.trim_end_matches('\r');
            let lower = line.trim().to_ascii_lowercase();
            if !header_done {
                if lower.starts_with("set player=") {
                    header_done = true;
                    continue;
                }
                if lower.starts_with("set game_") {
                    continue;
                }
            }
            if lower.starts_with("rem ") || lower.starts_with("::") || lower.starts_with("echo ") {
                continue;
            }
            if INTERACTIVE_RE.is_match(line) {
                probe.interactive = true;
            }
            if REGISTRY_RE.is_match(line) {
                probe.writes_registry = true;
            }
            if LANG_BRANCH_RE.is_match(line) {
                probe.language_dependent = true;
            }
            if header_done && lower.contains("%player%") {
                probe.uses_player_name = true;
            }
            for cap in FIREWALL_PROGRAM_RE.captures_iter(line) {
                let exe = normalise(&cap[1]);
                if !firewall.contains(&exe) {
                    firewall.push(exe);
                }
            }
            if lower.contains("netsh ") || lower.contains("unrar.exe") || lower.contains("fnr.exe")
            {
                continue;
            }
            if let Some(cap) = INVOKE_RE.captures(line) {
                let exe = normalise(&cap[1]);
                if exe.contains("%programfiles%") || exe.contains("%windir%") || exe.contains(":/")
                {
                    continue;
                }
                let args = split_args(&cap[2]);
                if !invoked.iter().any(|(e, _)| *e == exe) {
                    invoked.push((exe, args));
                }
            }
        }

        for exe in &firewall {
            let inv = invoked.iter().find(|(e, _)| {
                e.eq_ignore_ascii_case(exe)
                    || exe.ends_with(&format!("/{}", e.to_ascii_lowercase()))
                    || e.ends_with(&format!("/{}", exe.to_ascii_lowercase()))
            });
            probe.candidates.push(ProbeCandidate {
                exe: exe.clone(),
                args: inv.map(|(_, a)| a.clone()).unwrap_or_default(),
                confidence: if inv.is_some() {
                    Confidence::High
                } else {
                    Confidence::Medium
                },
            });
        }
        for (exe, args) in invoked {
            if !probe.candidates.iter().any(|c| {
                c.exe.eq_ignore_ascii_case(&exe)
                    || c.exe
                        .to_ascii_lowercase()
                        .ends_with(&format!("/{}", exe.to_ascii_lowercase()))
            }) {
                probe.candidates.push(ProbeCandidate {
                    exe,
                    args,
                    confidence: Confidence::Low,
                });
            }
        }
        probe
            .candidates
            .sort_by(|a, b| b.confidence.cmp(&a.confidence));
        probe
    }

    /// Build a fallback manifest when no curated manifest exists. Returns
    /// `None` if the script gives no usable executable.
    pub fn to_manifest(&self, game_id: &str) -> Option<Manifest> {
        let first = self.candidates.first()?;
        let mut manifest = Manifest {
            id: game_id.to_string(),
            origin: ManifestOrigin::DerivedFromScript,
            source: Some("derived from game_start.cmd".into()),
            ..Default::default()
        };
        manifest.launch = LaunchSpec {
            exe: first.exe.clone(),
            args: first.args.clone(),
            runner: Runner::Auto,
            required_files: vec![first.exe.clone()],
            alternatives: self
                .candidates
                .iter()
                .skip(1)
                .map(|c| Alternative {
                    name: c
                        .exe
                        .rsplit('/')
                        .next()
                        .unwrap_or(&c.exe)
                        .trim_end_matches(".exe")
                        .to_string(),
                    exe: c.exe.clone(),
                    args: c.args.clone(),
                    workdir: None,
                })
                .collect(),
            ..Default::default()
        };
        if self.interactive {
            manifest.setup.notes.insert(
                "en".into(),
                "The original Windows script asks questions interactively; pick the entry point yourself if the default does not work.".into(),
            );
            manifest.setup.notes.insert(
                "de".into(),
                "Das originale Windows-Skript stellt Rückfragen; wähle den Startpunkt selbst, falls der Standard nicht passt.".into(),
            );
        }
        Some(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUAKE3: &str = "set game_path=%1\nset game_id=%2\nset game_lang=%3\nset player=%4\n\necho off\ncls\nsetlocal EnableDelayedExpansion\ncd /d \"%~dp0\"\ncd local\n\n\nnetsh advfirewall firewall add rule name=\"%game_id%\" dir=in action=allow program=\"%game_path%\\local\\quake3.exe\" profile=any enable=yes >nul\n\n\"quake3.exe\"\n\nnetsh advfirewall firewall delete rule name=\"%game_id%\" >nul\n\nexit";

    #[test]
    fn simple_script_yields_high_confidence_candidate() {
        let p = ScriptProbe::analyse(QUAKE3);
        assert_eq!(p.candidates.len(), 1);
        assert_eq!(p.candidates[0].exe, "quake3.exe");
        assert_eq!(p.candidates[0].confidence, Confidence::High);
        assert!(p.candidates[0].args.is_empty());
        assert!(!p.interactive && !p.writes_registry && !p.language_dependent);
        let m = p.to_manifest("quake3").unwrap();
        assert_eq!(m.launch.exe, "quake3.exe");
        assert_eq!(m.origin, ManifestOrigin::DerivedFromScript);
    }

    #[test]
    fn captures_args_and_subfolders_and_start_syntax() {
        let s = "set player=%4\ncd local\nnetsh advfirewall firewall add rule name=\"%game_id%\" dir=in action=allow program=\"%game_path%\\local\\Bin\\Win32\\Game.exe\" profile=any enable=yes >nul\nstart /wait \"LAN Launcher\" \"Bin\\Win32\\Game.exe\" -window -name \"%player%\" >nul\n";
        let p = ScriptProbe::analyse(s);
        assert_eq!(p.candidates[0].exe, "Bin/Win32/Game.exe");
        assert_eq!(p.candidates[0].args, vec!["-window", "-name", "%player%"]);
        assert!(p.uses_player_name);
    }

    #[test]
    fn detects_interactive_menus_registry_and_language() {
        let s = "set player=%4\nreg.exe add HKCU\\Software\\X /v Y /d 1 /f\nif %game_lang%==de copy de.ini game.ini\nset /P wahl=Auswahl:\nnetsh advfirewall firewall add rule name=\"%game_id%\" dir=in action=allow program=\"%game_path%\\local\\a.exe\" profile=any enable=yes >nul\nnetsh advfirewall firewall add rule name=\"%game_id%\" dir=in action=allow program=\"%game_path%\\local\\server.exe\" profile=any enable=yes >nul\n\"a.exe\"\n";
        let p = ScriptProbe::analyse(s);
        assert!(p.interactive && p.writes_registry && p.language_dependent);
        assert_eq!(p.candidates.len(), 2);
        assert_eq!(p.candidates[0].exe, "a.exe");
        assert_eq!(p.candidates[1].confidence, Confidence::Medium);
        let m = p.to_manifest("x").unwrap();
        assert_eq!(m.launch.alternatives.len(), 1);
        assert_eq!(m.launch.alternatives[0].name, "server");
    }

    #[test]
    fn ignores_helper_tools() {
        let s = "set player=%4\n\"%programfiles%\\eti\\lan launcher\\unrar.exe\" x -o -y \"x.eti\" local\\\n";
        let p = ScriptProbe::analyse(s);
        assert!(p.candidates.is_empty());
        assert!(p.to_manifest("x").is_none());
    }
}
