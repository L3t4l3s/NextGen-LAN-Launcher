//! ETI LANPage compatibility: event configuration and the statistics beacon.
//!
//! * `GET http://<host>/launcher.ini` – event configuration (see
//!   [`crate::launcher_ini`]). ETI hard-codes the host `launcher.lan`.
//! * `GET http://<host>/launcher.css` – optional legacy stylesheet.
//! * `GET http://<host>/logo.png` – the event logo (LANPage default `$logo`).
//! * `GET http://<host>/theme.json` – our own optional theme (new).
//! * `GET <stats_url>?hostname=...&macaddr1=...` – statistics beacon. The
//!   PHP side decodes every parameter as ISO-8859-15, so we percent-encode
//!   Latin-9 bytes rather than UTF-8 to keep umlauts intact.

use crate::error::{Error, Result};
use crate::launcher_ini::LanConfig;
use crate::theme::Theme;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventBundle {
    pub config: Option<LanConfig>,
    pub legacy_css: Option<String>,
    pub theme: Option<Theme>,
    /// `logo.png` at the LANPage root, the image the ETI client shows next to
    /// the event name (the LANPage's own `$logo` default).
    pub logo: Option<String>,
    /// Which URLs answered, for the diagnostics page.
    pub fetched: Vec<String>,
    pub errors: Vec<String>,
    /// Server time from the HTTP `Date` header, to detect clock skew.
    pub server_time: Option<chrono::DateTime<chrono::Utc>>,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .no_proxy()
        .build()
        .expect("reqwest client")
}

pub fn base_url(host: &str) -> String {
    let h = host.trim().trim_end_matches('/');
    if h.starts_with("http://") || h.starts_with("https://") {
        h.to_string()
    } else {
        format!("http://{h}")
    }
}

/// Fetch launcher.ini, launcher.css and theme.json from the LANPage host.
/// Missing files are not errors: a LAN without a LANPage is fine.
pub async fn fetch_event(host: &str) -> EventBundle {
    let base = base_url(host);
    let client = client();
    let mut bundle = EventBundle::default();

    // Whether the host answered at all; when the ini request fails at the
    // transport level the optional files are not probed (each has its own
    // timeout and the host is simply not there).
    let mut reachable = true;
    match client.get(format!("{base}/launcher.ini")).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Some(date) = resp
                .headers()
                .get(reqwest::header::DATE)
                .and_then(|d| d.to_str().ok())
            {
                bundle.server_time = chrono::DateTime::parse_from_rfc2822(date)
                    .ok()
                    .map(|d| d.with_timezone(&chrono::Utc));
            }
            match resp.text().await {
                Ok(text) => match LanConfig::parse(&text) {
                    Ok(cfg) => {
                        bundle.fetched.push("launcher.ini".into());
                        bundle.config = Some(cfg);
                    }
                    Err(e) => bundle.errors.push(format!("launcher.ini: {e}")),
                },
                Err(e) => bundle.errors.push(format!("launcher.ini: {e}")),
            }
        }
        Ok(resp) => bundle
            .errors
            .push(format!("launcher.ini: HTTP {}", resp.status())),
        Err(e) => {
            reachable = false;
            bundle.errors.push(format!("launcher.ini: {e}"));
        }
    }
    if !reachable {
        return bundle;
    }

    // The optional files are independent; fetch them concurrently.
    let theme_url = bundle
        .config
        .as_ref()
        .and_then(|c| c.extra.get("theme_url").cloned())
        .unwrap_or_else(|| format!("{base}/theme.json"));
    let logo_url = format!("{base}/logo.png");
    let (css, logo, theme) = tokio::join!(
        client.get(format!("{base}/launcher.css")).send(),
        client.get(&logo_url).send(),
        client.get(&theme_url).send(),
    );

    if let Ok(resp) = css {
        if resp.status().is_success() {
            if let Ok(css) = resp.text().await {
                if css.len() < 200_000 {
                    bundle.fetched.push("launcher.css".into());
                    bundle.legacy_css = Some(css);
                }
            }
        }
    }

    // The ETI standard: logo.png next to launcher.ini. The URL is handed to
    // the UI as is; the CSP allows images from the LANPage host.
    if let Ok(resp) = logo {
        let is_image = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|ct| ct.get(..6))
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("image/"));
        if resp.status().is_success() && is_image {
            bundle.fetched.push("logo.png".into());
            bundle.logo = Some(logo_url);
        }
    }

    if let Ok(resp) = theme {
        if resp.status().is_success() {
            if let Ok(text) = resp.text().await {
                match Theme::parse(&text) {
                    Ok(t) => {
                        bundle.fetched.push("theme.json".into());
                        bundle.theme = Some(t);
                    }
                    Err(e) => bundle.errors.push(format!("theme.json: {e}")),
                }
            }
        }
    }
    bundle
}

/// Hardware/identity report the ETI LANPage expects.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StatsReport {
    pub hostname: String,
    pub macaddr1: String,
    pub macaddr2: String,
    pub board_manufacturer: String,
    pub baseboard: String,
    pub system_product_name: String,
    pub bios_release: String,
    pub cpu: String,
    pub gpu: String,
    pub windows_edition: String,
    pub player_name: String,
    pub current_game: String,
}

/// Percent-encode a string as ISO-8859-15 bytes (what `stats.php` decodes).
pub fn encode_latin9(value: &str) -> String {
    let (bytes, _, _) = encoding_rs::ISO_8859_15.encode(value);
    let mut out = String::with_capacity(bytes.len() * 3);
    for b in bytes.iter() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

impl StatsReport {
    /// Collect system information (best effort, never fails).
    pub fn collect(player_name: &str, current_game: Option<&str>) -> Self {
        let mut sys = sysinfo::System::new();
        sys.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
        let networks = sysinfo::Networks::new_with_refreshed_list();
        let mut macs: Vec<String> = networks
            .iter()
            .filter(|(name, _)| {
                !name.starts_with("lo") && !name.to_lowercase().contains("loopback")
            })
            .map(|(_, data)| data.mac_address().to_string())
            .filter(|m| m != "00:00:00:00:00:00")
            .collect();
        macs.sort();
        macs.dedup();
        let cpu = sys
            .cpus()
            .first()
            .map(|c| c.brand().trim().to_string())
            .unwrap_or_default();
        let os = format!(
            "{} {}",
            sysinfo::System::name().unwrap_or_default(),
            sysinfo::System::os_version().unwrap_or_default()
        )
        .trim()
        .to_string();
        Self {
            hostname: sysinfo::System::host_name().unwrap_or_default(),
            macaddr1: macs.first().cloned().unwrap_or_default(),
            macaddr2: macs.get(1).cloned().unwrap_or_default(),
            board_manufacturer: String::new(),
            baseboard: String::new(),
            system_product_name: String::new(),
            bios_release: String::new(),
            cpu,
            gpu: String::new(),
            windows_edition: os,
            player_name: player_name.to_string(),
            current_game: current_game.unwrap_or_default().to_string(),
        }
    }

    pub fn to_url(&self, stats_url: &str) -> String {
        let sep = if stats_url.contains('?') { '&' } else { '?' };
        let pairs = [
            ("hostname", &self.hostname),
            ("macaddr1", &self.macaddr1),
            ("macaddr2", &self.macaddr2),
            ("board_manufacturer", &self.board_manufacturer),
            ("baseboard", &self.baseboard),
            ("system_product_name", &self.system_product_name),
            ("bios_release", &self.bios_release),
            ("cpu", &self.cpu),
            ("gpu", &self.gpu),
            ("windows_edition", &self.windows_edition),
            ("player_name", &self.player_name),
            ("current_game", &self.current_game),
        ];
        let query: Vec<String> = pairs
            .iter()
            .map(|(k, v)| format!("{k}={}", encode_latin9(v)))
            .collect();
        format!("{stats_url}{sep}{}", query.join("&"))
    }

    /// Send the beacon. `Ok(true)` when the server answered `ok`.
    pub async fn send(&self, stats_url: &str) -> Result<bool> {
        if self.macaddr1.is_empty() {
            return Err(Error::Http("no MAC address available for stats".into()));
        }
        let resp = client().get(self.to_url(stats_url)).send().await?;
        let text = resp.text().await?;
        Ok(text.trim().eq_ignore_ascii_case("ok"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin9_encoding_keeps_umlauts_single_byte() {
        assert_eq!(encode_latin9("Jörg Müller"), "J%F6rg+M%FCller");
        assert_eq!(encode_latin9("a&b=c"), "a%26b%3Dc");
        assert_eq!(encode_latin9("€"), "%A4");
    }

    #[test]
    fn stats_url_contains_all_parameters() {
        let r = StatsReport {
            hostname: "pc-1".into(),
            macaddr1: "AA:BB".into(),
            player_name: "Jörg".into(),
            current_game: "quake3".into(),
            ..Default::default()
        };
        let url = r.to_url("http://launcher.lan/stats.php");
        assert!(url.starts_with("http://launcher.lan/stats.php?hostname=pc-1&macaddr1=AA%3ABB&"));
        assert!(url.contains("player_name=J%F6rg"));
        assert!(url.ends_with("current_game=quake3"));
        assert!(url.contains("gpu=&"));
    }

    #[test]
    fn base_url_normalises_hosts() {
        assert_eq!(base_url("launcher.lan"), "http://launcher.lan");
        assert_eq!(base_url("https://x.y/"), "https://x.y");
    }

    #[test]
    fn collect_never_panics() {
        let r = StatsReport::collect("Tester", Some("quake3"));
        assert_eq!(r.player_name, "Tester");
        assert_eq!(r.current_game, "quake3");
    }
}
