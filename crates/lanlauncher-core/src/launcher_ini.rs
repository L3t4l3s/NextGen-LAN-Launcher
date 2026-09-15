//! Parser for the ETI `launcher.ini` event configuration.
//!
//! The format is not INI at all but a block syntax:
//!
//! ```text
//! lan_title ### Name of the event {
//! My LAN Party
//! }
//! link_1 ### Custom link 1 {
//! Label|http://example/
//! }
//! ```
//!
//! Everything after `###` up to `{` is a comment. A value spans all lines until
//! the closing `}` on its own line. Unknown keys are preserved in `extra` so an
//! organiser can add custom keys without breaking older launchers.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub label: String,
    pub url: String,
}

/// Event configuration as understood by the launcher.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanConfig {
    pub title: Option<String>,
    /// Three-letter event tag (`MLP`).
    pub tag: Option<String>,
    pub website: Option<String>,
    pub force_lan_mode: bool,
    /// Upload limit in KB/s for LAN mode; `0` or `None` means unlimited.
    pub lan_upload_limit_kbs: Option<u32>,
    pub stats_url: Option<String>,
    pub ts3_server: Option<String>,
    pub discord_url: Option<String>,
    pub dc_hub: Option<String>,
    pub links: Vec<Link>,
    pub disabled_games: Vec<String>,
    /// Non-standard keys (e.g. `theme_url`) kept verbatim.
    pub extra: BTreeMap<String, String>,
}

/// Parse the raw block syntax into ordered key/value pairs.
pub fn parse_blocks(input: &str) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut current: Option<(String, Vec<String>)> = None;
    for (lineno, raw) in input.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        match &mut current {
            None => {
                let t = line.trim();
                if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
                    continue;
                }
                let Some(head) = t.strip_suffix('{') else {
                    return Err(Error::LauncherIni(format!(
                        "line {}: expected `key ### comment {{`, got `{t}`",
                        lineno + 1
                    )));
                };
                let key = head.split("###").next().unwrap_or("").trim().to_string();
                if key.is_empty() || key.contains(char::is_whitespace) {
                    return Err(Error::LauncherIni(format!(
                        "line {}: invalid key `{key}`",
                        lineno + 1
                    )));
                }
                current = Some((key, Vec::new()));
            }
            Some((key, lines)) => {
                if line.trim() == "}" {
                    let value = lines.join("\n").trim().to_string();
                    out.push((std::mem::take(key), value));
                    current = None;
                } else {
                    lines.push(line.to_string());
                }
            }
        }
    }
    if let Some((key, _)) = current {
        return Err(Error::LauncherIni(format!("unterminated block `{key}`")));
    }
    Ok(out)
}

fn non_empty(v: String) -> Option<String> {
    let t = v.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn parse_link(v: &str) -> Option<Link> {
    let v = v.trim();
    if v.is_empty() {
        return None;
    }
    match v.split_once('|') {
        Some((label, url)) if !url.trim().is_empty() => Some(Link {
            label: label.trim().to_string(),
            url: url.trim().to_string(),
        }),
        Some(_) => None,
        None => Some(Link {
            label: v.to_string(),
            url: v.to_string(),
        }),
    }
}

impl LanConfig {
    pub fn parse(input: &str) -> Result<Self> {
        let mut cfg = LanConfig::default();
        let mut links: BTreeMap<u32, Link> = BTreeMap::new();
        for (key, value) in parse_blocks(input)? {
            match key.as_str() {
                "lan_title" => cfg.title = non_empty(value),
                "lan_id" => cfg.tag = non_empty(value),
                "lan_url" => cfg.website = non_empty(value),
                "force_lan_mode" => {
                    cfg.force_lan_mode = matches!(value.trim(), "1" | "true" | "yes")
                }
                "lan_upload_limit" => {
                    cfg.lan_upload_limit_kbs = value.trim().parse::<u32>().ok().filter(|v| *v > 0)
                }
                "stats_url" => cfg.stats_url = non_empty(value),
                "ts3_server" => cfg.ts3_server = non_empty(value),
                "discord_url" => cfg.discord_url = non_empty(value),
                "dc_hub" => cfg.dc_hub = non_empty(value),
                "disable_games" => {
                    cfg.disabled_games = value
                        .split(|c: char| c == ',' || c.is_whitespace())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect()
                }
                k if k.starts_with("link_") => {
                    if let (Ok(n), Some(link)) = (k[5..].parse::<u32>(), parse_link(&value)) {
                        links.insert(n, link);
                    }
                }
                _ => {
                    cfg.extra.insert(key, value);
                }
            }
        }
        cfg.links = links.into_values().collect();
        Ok(cfg)
    }

    pub fn is_game_disabled(&self, game_id: &str) -> bool {
        self.disabled_games.iter().any(|g| g == game_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../tests/fixtures/launcher.sample.ini");

    #[test]
    fn parses_official_sample() {
        let cfg = LanConfig::parse(SAMPLE).unwrap();
        assert_eq!(cfg.title.as_deref(), Some("My LAN Party"));
        assert_eq!(cfg.tag.as_deref(), Some("MLP"));
        assert_eq!(cfg.website.as_deref(), Some("http://myparty.lan"));
        assert!(cfg.force_lan_mode);
        assert_eq!(
            cfg.stats_url.as_deref(),
            Some("http://launcher.lan/stats.php")
        );
        assert_eq!(
            cfg.ts3_server.as_deref(),
            Some("ts3server://1.2.3.4?port=9987")
        );
        assert_eq!(
            cfg.discord_url.as_deref(),
            Some("https://discord.gg/abcde12345")
        );
        assert_eq!(cfg.dc_hub.as_deref(), Some("dchub://1.2.3.4:1234"));
        assert_eq!(cfg.links.len(), 5);
        assert_eq!(cfg.links[0].label, "Test 1");
        assert_eq!(cfg.links[4].url, "http://link5/");
        assert_eq!(cfg.disabled_games, vec!["gameid", "anothergameid"]);
        assert!(cfg.is_game_disabled("gameid"));
        assert!(!cfg.is_game_disabled("quake3"));
    }

    #[test]
    fn handles_crlf_extra_keys_and_upload_limit() {
        let ini = "lan_title ### x {\r\nSuper LAN\r\n}\r\nlan_upload_limit ### {\r\n50\r\n}\r\ntheme_url ### ours {\r\nhttp://launcher.lan/theme.json\r\n}\r\n";
        let cfg = LanConfig::parse(ini).unwrap();
        assert_eq!(cfg.title.as_deref(), Some("Super LAN"));
        assert_eq!(cfg.lan_upload_limit_kbs, Some(50));
        assert_eq!(
            cfg.extra.get("theme_url").map(String::as_str),
            Some("http://launcher.lan/theme.json")
        );
    }

    #[test]
    fn empty_values_become_none() {
        let cfg = LanConfig::parse("ts3_server ### {\n\n}\nlink_1 ### {\n\n}\n").unwrap();
        assert!(cfg.ts3_server.is_none());
        assert!(cfg.links.is_empty());
    }

    #[test]
    fn reports_unterminated_blocks() {
        let err = LanConfig::parse("lan_title ### {\nOops\n").unwrap_err();
        assert!(err.to_string().contains("unterminated"));
    }

    #[test]
    fn multi_line_values_are_joined() {
        let cfg = LanConfig::parse("motd ### {\nline one\nline two\n}\n").unwrap();
        assert_eq!(cfg.extra["motd"], "line one\nline two");
    }
}
