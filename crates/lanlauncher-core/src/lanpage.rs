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
use base64::Engine;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const MAX_THEME_FONTS: usize = 8;
const MAX_CONCURRENT_FONT_FETCHES: usize = 3;
const MAX_FONT_BYTES: usize = 1_400_000;
const MAX_THEME_FONT_BYTES: usize = 4_200_000;
const FONT_INLINE_BUDGET: Duration = Duration::from_millis(500);
const MAX_FONT_CACHE_ENTRIES: usize = 16;
const MAX_FONT_CACHE_BYTES: usize = 12_000_000;
const FONT_CACHE_TTL: Duration = Duration::from_secs(60 * 60);

struct CachedFont {
    data: String,
    source_bytes: usize,
    stored_at: Instant,
}

fn font_cache() -> &'static Mutex<HashMap<String, CachedFont>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedFont>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

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
    let explicit_theme_url = bundle
        .config
        .as_ref()
        .and_then(|c| c.extra.get("theme_url").cloned());
    let theme_url = explicit_theme_url
        .clone()
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
        let says_image = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|ct| ct.get(..6))
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("image/"));
        // The bytes decide, not the content type: a server that answers with
        // `application/octet-stream` is still serving the logo, and the index
        // page many LANPages return for a missing file is not an image even
        // when it arrives as `image/png`. Only an unreadable body falls back
        // to what the header claims.
        if resp.status().is_success() {
            let body = resp.bytes().await;
            // The bytes are the better answer, but they only *reject* what
            // is plainly a web page: BMP, AVIF and an SVG with a doctype are
            // images too, and a server that says so is taken at its word.
            let is_image = match &body {
                Ok(bytes) => looks_like_image(bytes) || (says_image && !looks_like_html(bytes)),
                Err(_) => says_image,
            };
            if is_image {
                bundle.fetched.push("logo.png".into());
                bundle.logo = Some(logo_url);
            } else if says_image {
                log::warn!("{logo_url}: served as an image but is none, ignored");
            }
        }
    }

    if let Ok(resp) = theme {
        if resp.status().is_success() {
            if let Ok(text) = resp.text().await {
                // Web servers often answer the probed default theme.json with
                // an HTML page and status 200; that is no error, it just is
                // not a theme. A theme_url the organiser configured must
                // parse, or the error is kept.
                if explicit_theme_url.is_some() || looks_like_theme(&text) {
                    match Theme::parse_at(&text, Some(&theme_url)) {
                        Ok(mut t) => {
                            inline_theme_fonts(&client, &mut t, &mut bundle.errors).await;
                            bundle.fetched.push("theme.json".into());
                            bundle.theme = Some(t);
                        }
                        Err(e) => bundle.errors.push(format!("theme.json: {e}")),
                    }
                } else if text.trim_start().starts_with('{') {
                    // Someone meant to put a theme here. Saying so is the
                    // only way an organiser learns about a trailing comma;
                    // the file is not applied either way.
                    log::warn!("{theme_url}: not a theme, ignored");
                }
            }
        }
    }

    // No theme.json: the colours may still be in launcher.ini. The file wins
    // where both exist — it is the deliberate one and can say more.
    if bundle.theme.is_none() {
        if let Some(mut theme) = bundle
            .config
            .as_ref()
            .and_then(|c| Theme::from_ini(&c.extra))
        {
            // `theme_logo = logo.png` means the file next to launcher.ini.
            theme.normalise_for(&format!("{base}/launcher.ini"));
            inline_theme_fonts(&client, &mut theme, &mut bundle.errors).await;
            log::info!("theme from launcher.ini: {}", theme.name);
            bundle.theme = Some(theme);
        }
    }

    bundle
}

/// Fonts referenced by a theme are cross-origin from Tauri's webview. Most
/// small LAN web servers do not send `Access-Control-Allow-Origin`, so the
/// browser rejects the file and quietly uses the fallback face. Fetch valid
/// font files here and pass them to the UI as data URLs; an unavailable or
/// unusual file keeps its original URL, which still works on a CORS-aware
/// server.
async fn inline_theme_fonts(client: &reqwest::Client, theme: &mut Theme, errors: &mut Vec<String>) {
    let eligible = theme
        .font_faces
        .iter()
        .enumerate()
        .map(|(index, face)| (index, face.family.clone(), face.src.trim().to_string()))
        .filter(|(_, _, src)| src.starts_with("http://") || src.starts_with("https://"))
        .collect::<Vec<_>>();
    if eligible.len() > MAX_THEME_FONTS {
        log::warn!(
            "theme declares {} remote fonts; only the first {MAX_THEME_FONTS} are inlined",
            eligible.len()
        );
    }

    let requests = futures::stream::iter(eligible.into_iter().take(MAX_THEME_FONTS).map(
        |(index, family, src)| async move { (index, family, fetch_theme_font(client, &src).await) },
    ))
    .buffer_unordered(MAX_CONCURRENT_FONT_FETCHES);
    tokio::pin!(requests);

    let mut total_bytes = 0;
    let result = tokio::time::timeout(FONT_INLINE_BUDGET, async {
        while let Some((index, family, result)) = requests.next().await {
            match result {
                Ok((data, bytes)) if total_bytes + bytes <= MAX_THEME_FONT_BYTES => {
                    total_bytes += bytes;
                    theme.font_faces[index].src = data;
                }
                Ok((_, _)) => {
                    let message =
                        format!("theme font {family}: total exceeds {MAX_THEME_FONT_BYTES} bytes");
                    log::warn!("{message}");
                    errors.push(message);
                }
                Err(e) => {
                    log::warn!("theme font {family}: {e}");
                    errors.push(format!("theme font {family}: {e}"));
                }
            }
        }
    })
    .await;
    if result.is_err() {
        // Theme fonts are decoration. Never let them consume the startup
        // refresh's five-second budget and delay the Resilio configuration.
        log::warn!("theme font loading exceeded 500 ms; remaining URLs left unchanged");
    }
}

async fn fetch_theme_font(
    client: &reqwest::Client,
    url: &str,
) -> std::result::Result<(String, usize), String> {
    if let Ok(cache) = font_cache().lock() {
        if let Some(font) = cache
            .get(url)
            .filter(|font| font.stored_at.elapsed() < FONT_CACHE_TTL)
        {
            return Ok((font.data.clone(), font.source_bytes));
        }
    }

    let mut resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    if resp
        .content_length()
        .is_some_and(|len| len > MAX_FONT_BYTES as u64)
    {
        return Err(format!("larger than {MAX_FONT_BYTES} bytes"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        if bytes.len() + chunk.len() > MAX_FONT_BYTES {
            return Err(format!("larger than {MAX_FONT_BYTES} bytes"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let mime = font_mime(&bytes).ok_or_else(|| "not a supported font file".to_string())?;
    let byte_count = bytes.len();
    let data = format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    );
    if let Ok(mut cache) = font_cache().lock() {
        cache.retain(|_, font| font.stored_at.elapsed() < FONT_CACHE_TTL);
        let cached_bytes = cache.values().map(|font| font.data.len()).sum::<usize>();
        if cache.len() >= MAX_FONT_CACHE_ENTRIES || cached_bytes + data.len() > MAX_FONT_CACHE_BYTES
        {
            cache.clear();
        }
        cache.insert(
            url.to_string(),
            CachedFont {
                data: data.clone(),
                source_bytes: byte_count,
                stored_at: Instant::now(),
            },
        );
    }
    Ok((data, byte_count))
}

fn font_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"wOF2") {
        Some("font/woff2")
    } else if bytes.starts_with(b"wOFF") {
        Some("font/woff")
    } else if bytes.starts_with(b"\0\x01\0\0") {
        Some("font/ttf")
    } else if bytes.starts_with(b"OTTO") {
        Some("font/otf")
    } else {
        None
    }
}

/// The first bytes of the common image formats, plus SVG's opening tag.
fn looks_like_image(bytes: &[u8]) -> bool {
    const MAGIC: [&[u8]; 5] = [
        b"\x89PNG",
        b"\xff\xd8\xff", // JPEG
        b"GIF8",
        b"RIFF",             // WebP (RIFF....WEBP)
        b"\x00\x00\x01\x00", // ICO
    ];
    if MAGIC.iter().any(|m| bytes.starts_with(m)) {
        return true;
    }
    // SVG, but only when the document *is* one: an HTML page with an inline
    // icon in it contains `<svg` too, and that page is what a LANPage answers
    // for a file it does not have.
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
    let head = head.trim_start();
    head.starts_with("<svg") || (head.starts_with("<?xml") && head.contains("<svg"))
}

/// The page a LANPage answers with for a file it does not have.
fn looks_like_html(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
    let head = head.trim_start();
    head.starts_with("<!doctype html") || head.starts_with("<html") || head.starts_with("<?php")
}

/// A body that can be a theme at all.
///
/// The probed `/theme.json` catches whatever a LANPage answers for unknown
/// paths: an HTML index page, or a JSON error body. Every theme field has a
/// default, so any JSON object would otherwise parse into the default theme
/// and silently displace the colours from `launcher.ini`. A theme has to say
/// something themable.
pub fn looks_like_theme(body: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    [
        "colors",
        "mode",
        "radius",
        "logo",
        "backgroundImage",
        "backgroundOverlay",
        "surfacePattern",
        "fontFamily",
        "headingFontFamily",
        "fontFaces",
        "icons",
    ]
    .iter()
    .any(|key| object.contains_key(*key))
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

    /// A LANPage on a throwaway port: `routes` are matched as substrings of
    /// the request line, in order.
    async fn lanpage(routes: Vec<(&'static str, &'static str, &'static str)>) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let line = String::from_utf8_lossy(&buf[..n])
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    let (status, ctype, body) = routes
                        .iter()
                        .find(|(path, _, _)| line.contains(path))
                        .map(|(_, ctype, body)| (200, *ctype, *body))
                        .unwrap_or((404, "text/plain", "no"));
                    let resp = format!(
                        "HTTP/1.1 {status} OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}")
    }

    const INI_WITH_COLOURS: &str =
        "lan_title ### t {\nNext Generation LAN\n}\n\ntheme_primary ### c {\n#29b6f6\n}\n";

    #[tokio::test]
    async fn a_logo_served_without_an_image_type_is_still_the_logo() {
        // A LANPage that answers `logo.png` as `application/octet-stream` was
        // serving its logo all along; the first bytes are the honest answer.
        let base = lanpage(vec![
            ("launcher.ini", "text/plain", INI_WITH_COLOURS),
            ("logo.png", "application/octet-stream", "GIF89a...."),
        ])
        .await;
        let bundle = fetch_event(&base).await;
        assert_eq!(
            bundle.logo.as_deref(),
            Some(format!("{base}/logo.png").as_str())
        );

        // A page returned for a missing file is not an image, whatever it says.
        let base = lanpage(vec![
            ("launcher.ini", "text/plain", INI_WITH_COLOURS),
            ("logo.png", "image/png", "<!DOCTYPE html><html>404</html>"),
        ])
        .await;
        assert_eq!(fetch_event(&base).await.logo, None);
    }

    #[tokio::test]
    async fn colours_from_the_ini_survive_a_json_error_body_for_theme_json() {
        // The other way a LANPage answers an unknown path.
        let base = lanpage(vec![
            ("launcher.ini", "text/plain", INI_WITH_COLOURS),
            (
                "theme.json",
                "application/json",
                r#"{"error":"not found","code":404}"#,
            ),
        ])
        .await;
        let bundle = fetch_event(&base).await;
        assert_eq!(
            bundle.theme.as_ref().map(|t| t.colors.primary.as_str()),
            Some("#29b6f6")
        );
    }

    #[tokio::test]
    async fn colours_from_the_ini_survive_a_soft_404_for_theme_json() {
        // Many LANPages answer any unknown path with their index page and
        // status 200. That is not a theme, and it must not cost the ini its
        // colours either.
        let base = lanpage(vec![
            ("launcher.ini", "text/plain", INI_WITH_COLOURS),
            ("theme.json", "text/html", "<!DOCTYPE html><html>404</html>"),
        ])
        .await;
        let bundle = fetch_event(&base).await;
        assert_eq!(
            bundle.theme.as_ref().map(|t| t.colors.primary.as_str()),
            Some("#29b6f6")
        );
        assert_eq!(
            bundle.config.unwrap().title.as_deref(),
            Some("Next Generation LAN")
        );
    }

    #[tokio::test]
    async fn a_served_theme_json_wins_over_the_ini() {
        let base = lanpage(vec![
            ("launcher.ini", "text/plain", INI_WITH_COLOURS),
            (
                "theme.json",
                "application/json",
                r##"{"name":"File","colors":{"primary":"#ff0000"}}"##,
            ),
        ])
        .await;
        let bundle = fetch_event(&base).await;
        assert_eq!(
            bundle.theme.as_ref().map(|t| t.colors.primary.as_str()),
            Some("#ff0000")
        );
    }

    #[tokio::test]
    async fn served_fonts_are_inlined_so_the_webview_needs_no_cors_header() {
        let base = lanpage(vec![
            ("launcher.ini", "text/plain", INI_WITH_COLOURS),
            (
                "theme.json",
                "application/json",
                r#"{"fontFamily":"LAN","fontFaces":[{"family":"LAN","src":"fonts/lan.woff2"}]}"#,
            ),
            ("fonts/lan.woff2", "font/woff2", "wOF2font-data"),
        ])
        .await;
        let bundle = fetch_event(&base).await;
        let face = &bundle.theme.unwrap().font_faces[0];
        assert_eq!(face.src, "data:font/woff2;base64,d09GMmZvbnQtZGF0YQ==");
        assert!(bundle.errors.is_empty(), "{:?}", bundle.errors);
    }

    #[test]
    fn only_real_font_signatures_are_inlined() {
        assert_eq!(font_mime(b"wOF2rest"), Some("font/woff2"));
        assert_eq!(font_mime(b"wOFFrest"), Some("font/woff"));
        assert_eq!(font_mime(b"\0\x01\0\0rest"), Some("font/ttf"));
        assert_eq!(font_mime(b"OTTOrest"), Some("font/otf"));
        assert_eq!(font_mime(b"<!doctype html>"), None);
    }

    #[test]
    fn only_a_themable_body_counts_as_a_theme() {
        assert!(!looks_like_theme("<!DOCTYPE html><html>404</html>"));
        // Every theme field has a default, so an error body would otherwise
        // parse into the default theme and displace the ini colours.
        assert!(!looks_like_theme(r#"{"error":"not found"}"#));
        assert!(!looks_like_theme(r#"{"version": 1}"#));
        assert!(looks_like_theme(r##"  {"colors": {"primary": "#fff"}}"##));
        assert!(looks_like_theme(r#"{"mode":"light"}"#));
    }

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
