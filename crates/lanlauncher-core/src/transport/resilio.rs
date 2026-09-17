//! Resilio Sync transport.
//!
//! The launcher ships (or downloads) the official Resilio Sync binary and runs
//! it as a child process with its own `config.json`:
//!
//! * web UI / API bound to `127.0.0.1:<port>` with a random password,
//! * `storage_path` inside the launcher's data directory (never the library),
//! * `folder_defaults.lan_discovery_mode = 3` (multicast + broadcast),
//! * a large `sync_max_time_diff` so a wrong system clock does not silently
//!   stall a sync.
//!
//! Two HTTP surfaces exist. The documented *Sync API* (`/api?method=...`)
//! requires an `api_key` in the config. Without one we fall back to the GUI
//! endpoints (`/gui/...`) that the web interface itself uses. Both are wrapped
//! by [`ResilioClient`]; the install logic never depends on their accuracy.
//!
//! Real Resilio behaviour can only be validated on a LAN. Everything that can
//! be tested offline (config generation, request building, response parsing,
//! process bookkeeping) is covered by unit tests against a mock HTTP server.

use super::*;
use crate::error::Error;
use rand::RngExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};

pub const DEFAULT_LAN_PORT: u16 = 8889;
/// Pid file inside the storage dir, named as in ETI's config; written by the
/// engine (`pid_file`) and read by orphan cleanup.
pub const PID_FILE: &str = "rslsync.pid";

static API_KEY_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r#""api_key"\s*:\s*"([A-Za-z0-9]{16,})""#).expect("valid regex")
});

/// `api_key` from a Resilio config file, e.g. the ETI client's
/// `sync/config.json` (which has `//` comments, hence no strict JSON parse).
/// Comment lines are skipped so a commented-out old key never wins.
pub fn parse_api_key(config_text: &str) -> Option<String> {
    config_text
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .find_map(|l| API_KEY_RE.captures(l).map(|c| c[1].to_string()))
}

/// The API key of an installed ETI LAN Launcher on this PC
/// (`<Program Files>\eti\LAN Launcher\sync\config.json`), so a player who
/// already has ETI's client need not type it. Returns the key and the file.
pub fn eti_client_api_key(program_files: &[PathBuf]) -> Option<(String, PathBuf)> {
    program_files.iter().find_map(|pf| {
        let file = pf
            .join("eti")
            .join("LAN Launcher")
            .join("sync")
            .join("config.json");
        let text = std::fs::read_to_string(&file).ok()?;
        parse_api_key(&text).map(|k| (k, file))
    })
}
/// Resilio process name per platform (used for orphan cleanup).
pub fn process_names() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["Resilio Sync.exe", "rslsync.exe", "btsync.exe"]
    } else if cfg!(target_os = "macos") {
        &["Resilio Sync", "rslsync"]
    } else {
        &["rslsync", "btsync"]
    }
}

#[derive(Debug, Clone)]
pub struct ResilioConfig {
    pub binary: PathBuf,
    pub storage_dir: PathBuf,
    pub api_port: u16,
    pub listening_port: u16,
    pub login: String,
    pub password: String,
    /// Optional official API key (enables `/api`).
    pub api_key: Option<String>,
    pub lan_only: bool,
    pub upload_limit_kbs: u32,
}

impl ResilioConfig {
    pub fn new(binary: PathBuf, storage_dir: PathBuf) -> Self {
        let mut rng = rand::rng();
        let password: String = (0..24)
            .map(|_| {
                let idx = rng.random_range(0..62u8);
                match idx {
                    0..=9 => (b'0' + idx) as char,
                    10..=35 => (b'a' + idx - 10) as char,
                    _ => (b'A' + idx - 36) as char,
                }
            })
            .collect();
        Self {
            binary,
            storage_dir,
            api_port: 0,
            listening_port: 0,
            login: "launcher".into(),
            password,
            api_key: None,
            lan_only: true,
            upload_limit_kbs: 0,
        }
    }

    /// Resilio `config.json` contents. The key set mirrors the ETI launcher's
    /// config (proven to start Resilio 2.8.1 with `/config` on the LAN PCs);
    /// only our own values (storage, ports, LAN-only switches, API access)
    /// differ. Keys Resilio does not know can make the engine exit with
    /// code 1 before its API is up, so nothing is added without need.
    pub fn to_json(&self) -> Value {
        // Login and password protect the control API on 127.0.0.1 from other
        // local processes; the API key additionally enables the documented
        // /api surface. All three are keys Resilio knows.
        let mut webui = json!({
            "listen": format!("127.0.0.1:{}", self.api_port),
            "login": self.login,
            "password": self.password,
        });
        if let Some(k) = &self.api_key {
            webui["api_key"] = json!(k);
        }
        let internet = !self.lan_only;
        json!({
            "storage_path": self.storage_dir.to_string_lossy(),
            "pid_file": self.storage_dir.join(PID_FILE).to_string_lossy(),
            "check_for_updates": false,
            "use_gui": false,
            "listening_port": self.listening_port,
            "agree_to_EULA": "yes",
            "use_upnp": internet,
            "download_limit": 0,
            "upload_limit": self.upload_limit_kbs,
            "rate_limit_local_peers": false,
            "enable_warning_no_source": false,
            "lan_encrypt_data": false,
            // 48h: a wrong clock must never silently stall a LAN sync.
            "sync_max_time_diff": 172800,
            "folder_rescan_interval": 0,
            "peer_expiration_days": 3,
            "folder_defaults.use_relay": internet,
            "folder_defaults.use_tracker": internet,
            "folder_defaults.lan_discovery_mode": 3,
            "service_folders.use_relay": internet,
            "service_folders.use_tracker": internet,
            "sync_trash_ttl": 1,
            "send_statistics": false,
            "overwrite_changes": true,
            "prefer_utp2_lan": false,
            "log_ttl": 1,
            "log_size": 1,
            "enable_file_system_notifications": false,
            "sync_extended_attributes": false,
            "webui": webui,
        })
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| Error::io(p, e))?;
        }
        std::fs::create_dir_all(&self.storage_dir).map_err(|e| Error::io(&self.storage_dir, e))?;
        std::fs::write(path, serde_json::to_string_pretty(&self.to_json())?)
            .map_err(|e| Error::io(path, e))
    }
}

/// Folder options passed to `add_folder`/`set_folder_prefs`, mirroring the
/// options ETI's sync server uses.
pub fn folder_prefs(lan_only: bool) -> Vec<(&'static str, &'static str)> {
    let net = if lan_only { "0" } else { "1" };
    vec![
        ("use_tracker", net),
        ("use_dht", net),
        ("use_relay_server", net),
        ("use_hosts", "1"),
        ("search_lan", "1"),
        ("selective_sync", "0"),
        ("use_sync_trash", "0"),
        ("overwrite_changes", "1"),
    ]
}

/// How long a folder snapshot may serve several callers. Short enough that
/// the status bar (once a second) still shows fresh rates, long enough that
/// the loops asking in the same moment share one round of requests.
const FOLDER_CACHE: Duration = Duration::from_millis(900);

/// The folder list as it was at that moment.
type FolderSnapshot = (Instant, HashMap<PathBuf, ShareStatus>);

/// Minimal HTTP client for the Resilio API surfaces.
#[derive(Debug, Clone)]
pub struct ResilioClient {
    pub base: String,
    pub login: String,
    pub password: String,
    pub api_key: Option<String>,
    http: reqwest::Client,
    gui_token: Arc<Mutex<Option<String>>>,
    /// Cleared once a build has refused `getversion`, so the probe does not
    /// pay for a request that will never work on this engine.
    gui_has_getversion: Arc<AtomicBool>,
    /// What the peers' counters stood at when the share's current transfer
    /// began, per folder. The counters are cumulative for the engine's
    /// session and do not start again for a second transfer of the same
    /// share, so an update would otherwise report the first download's bytes
    /// as its own.
    transfers: Arc<Mutex<HashMap<String, Transfer>>>,
    /// Held for the length of a folder reading, see [`ResilioClient::folders`].
    reading: Arc<tokio::sync::Mutex<()>>,
    /// Last folder snapshot and when it was taken. A snapshot costs one
    /// `get_folders` plus one `get_folder_peers` per share, and three loops
    /// (install tick, rates, health) ask for it independently; within this
    /// window they share one answer.
    folder_cache: Arc<Mutex<Option<FolderSnapshot>>>,
}

use std::sync::Arc;

impl ResilioClient {
    pub fn new(
        base: impl Into<String>,
        login: impl Into<String>,
        password: impl Into<String>,
        api_key: Option<String>,
    ) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            login: login.into(),
            password: password.into(),
            api_key,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(8))
                .no_proxy()
                // The web UI sets a session cookie together with the token in
                // `token.html` and answers a token without it with HTTP 400.
                .cookie_store(true)
                .build()
                .expect("reqwest client"),
            gui_token: Arc::new(Mutex::new(None)),
            gui_has_getversion: Arc::new(AtomicBool::new(true)),
            transfers: Arc::new(Mutex::new(HashMap::new())),
            reading: Arc::new(tokio::sync::Mutex::new(())),
            folder_cache: Arc::new(Mutex::new(None)),
        }
    }

    /// Documented Sync API call: `/api?method=<m>&<params>`.
    pub async fn api(&self, method: &str, params: &[(&str, &str)]) -> Result<Value> {
        let mut url = url::Url::parse(&format!("{}/api", self.base))
            .map_err(|e| Error::Transport(e.to_string()))?;
        url.query_pairs_mut().append_pair("method", method);
        for (k, v) in params {
            url.query_pairs_mut().append_pair(k, v);
        }
        let resp = self
            .http
            .get(url)
            .basic_auth(&self.login, Some(&self.password))
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(Error::Transport(format!(
                "API {method}: HTTP {status}: {text}"
            )));
        }
        let v: Value = serde_json::from_str(&text)
            .map_err(|e| Error::Transport(format!("API {method}: bad JSON: {e}")))?;
        if let Some(err) = v.get("error").and_then(Value::as_i64) {
            if err != 0 {
                let msg = v.get("message").and_then(Value::as_str).unwrap_or("");
                return Err(Error::Transport(format!("API {method}: error {err} {msg}")));
            }
        }
        Ok(v)
    }

    async fn gui_token(&self) -> Result<String> {
        if let Some(t) = self.gui_token.lock().ok().and_then(|g| g.clone()) {
            return Ok(t);
        }
        let html = self
            .http
            .get(format!("{}/gui/token.html", self.base))
            .basic_auth(&self.login, Some(&self.password))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let token =
            parse_gui_token(&html).ok_or_else(|| Error::Transport("GUI token not found".into()))?;
        if let Ok(mut g) = self.gui_token.lock() {
            *g = Some(token.clone());
        }
        Ok(token)
    }

    /// Undocumented GUI endpoint used by the web interface. Kept as fallback
    /// when no API key is configured.
    pub async fn gui(&self, action: &str, params: &[(&str, &str)]) -> Result<Value> {
        let token = self.gui_token().await?;
        let mut url = url::Url::parse(&format!("{}/gui/", self.base))
            .map_err(|e| Error::Transport(e.to_string()))?;
        url.query_pairs_mut()
            .append_pair("token", &token)
            .append_pair("action", action);
        for (k, v) in params {
            url.query_pairs_mut().append_pair(k, v);
        }
        let resp = self
            .http
            .get(url)
            .basic_auth(&self.login, Some(&self.password))
            .send()
            .await?;
        // 400 belongs in this list: that is what the web UI answers when the
        // token no longer matches its session cookie. Without dropping the
        // cached token, a session that goes stale while the launcher runs
        // would make every later call fail the same way.
        if matches!(resp.status().as_u16(), 400 | 401 | 403) {
            if let Ok(mut g) = self.gui_token.lock() {
                *g = None;
            }
        }
        let text = resp.error_for_status()?.text().await?;
        serde_json::from_str(&text)
            .map_err(|e| Error::Transport(format!("GUI {action}: bad JSON: {e}")))
    }

    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    pub async fn version(&self) -> Result<String> {
        if self.has_api_key() {
            let v = self.api("get_version", &[]).await?;
            return Ok(v
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("?")
                .to_string());
        }
        // Not every build answers `getversion`; some reply with HTTP 400.
        // The folder list is what the fallback actually needs, so an engine
        // that serves it counts as reachable even with an unknown version.
        // A build that refuses `getversion` is remembered, otherwise every
        // probe from here on would pay for two requests instead of one.
        if self.gui_has_getversion.load(Ordering::Relaxed) {
            match self.gui("getversion", &[]).await {
                Ok(v) => {
                    return Ok(v
                        .get("version")
                        .map(|x| x.to_string().trim_matches('"').to_string())
                        .unwrap_or("?".into()))
                }
                Err(first) => {
                    self.gui_has_getversion.store(false, Ordering::Relaxed);
                    return match self.gui("getsyncfolders", &[]).await {
                        Ok(_) => Ok("?".into()),
                        // The engine is unreachable either way; the first
                        // error is the one that describes the probe.
                        Err(_) => Err(first),
                    };
                }
            }
        }
        self.gui("getsyncfolders", &[])
            .await
            .map(|_| "?".to_string())
    }

    pub async fn shutdown(&self) -> Result<()> {
        if self.has_api_key() {
            self.api("shutdown", &[]).await.map(|_| ())
        } else {
            self.gui("shutdown", &[]).await.map(|_| ())
        }
    }

    pub async fn add_folder(&self, key: &ShareKey, dir: &Path, lan_only: bool) -> Result<()> {
        let dir_s = dir.to_string_lossy().to_string();
        if self.has_api_key() {
            let mut params = vec![
                ("dir", dir_s.as_str()),
                ("secret", key.expose()),
                ("force", "1"),
            ];
            let prefs = folder_prefs(lan_only);
            params.extend(prefs.iter().copied());
            if let Err(e) = self.api("add_folder", &params).await {
                // The engine keeps its folders across launcher restarts, so
                // "already added" is the normal second start. It only counts
                // as success when the folder carries the key we asked for:
                // a changed catalog key has to surface.
                if !(is_already_added(&e) && self.holds_folder(dir, key.expose()).await) {
                    return Err(e);
                }
            }
            let mut p2 = vec![("dir", dir_s.as_str()), ("secret", key.expose())];
            p2.extend(folder_prefs(lan_only));
            let _ = self.api("set_folder_prefs", &p2).await;
            Ok(())
        } else {
            // The web UI answers HTTP 500 for a folder it already has — the
            // same case, checked the same way.
            match self
                .gui(
                    "addsyncfolder",
                    &[
                        ("name", dir_s.as_str()),
                        ("secret", key.expose()),
                        ("selectivesync", "0"),
                    ],
                )
                .await
            {
                Ok(_) => Ok(()),
                Err(e) => {
                    if self.holds_folder(dir, key.expose()).await {
                        Ok(())
                    } else {
                        Err(e)
                    }
                }
            }
        }
    }

    /// Does the engine already hold `dir` with this secret?
    async fn holds_folder(&self, dir: &Path, secret: &str) -> bool {
        let answer = if self.has_api_key() {
            self.api("get_folders", &[]).await
        } else {
            self.gui("getsyncfolders", &[]).await
        };
        match answer {
            Ok(v) if folder_has_secret(&v, dir, secret) => {
                log::debug!("{} is already a sync folder", dir.display());
                true
            }
            _ => false,
        }
    }

    pub async fn remove_folder(&self, dir: &Path, key: Option<&ShareKey>) -> Result<()> {
        // Whatever its peers had sent belongs to a download that is over: a
        // share added again starts from nothing, not from those bytes.
        if let Ok(mut transfers) = self.transfers.lock() {
            // Not removed: the engine's counters survive a share being taken
            // away and added again, so the bookkeeping has to as well — only
            // what it counted belongs to the download that was cancelled.
            if let Some(transfer) = transfers.get_mut(&crate::transport::normalise_dir(dir)) {
                transfer.start_over();
            }
        }
        let dir_s = dir.to_string_lossy().to_string();
        if self.has_api_key() {
            let secret = key.map(|k| k.expose().to_string()).unwrap_or_default();
            self.api(
                "remove_folder",
                &[("dir", dir_s.as_str()), ("secret", secret.as_str())],
            )
            .await
            .map(|_| ())
        } else {
            let secret = key.map(|k| k.expose().to_string()).unwrap_or_default();
            self.gui(
                "removefolder",
                &[("name", dir_s.as_str()), ("secret", secret.as_str())],
            )
            .await
            .map(|_| ())
        }
    }

    /// Raw `get_folder_peers` entries for a share secret (documented API).
    pub async fn folder_peers(&self, secret: &str) -> Result<Vec<Value>> {
        let v = self.api("get_folder_peers", &[("secret", secret)]).await?;
        Ok(v.as_array().cloned().unwrap_or_default())
    }

    /// What has arrived of a share.
    ///
    /// Where the engine answered, the peers' counters move this share's
    /// bookkeeping on; where it did not, what that bookkeeping already holds
    /// stands, because dropping back to the finished files would put a
    /// running download at "8 B of 150.9 GB" again for one tick.
    fn received(
        &self,
        dir: &Path,
        status: &ShareStatus,
        samples: Option<&[PeerSample]>,
    ) -> (u64, bool) {
        let Ok(mut transfers) = self.transfers.lock() else {
            return (status.bytes_done, false);
        };
        let transfer = transfers
            .entry(crate::transport::normalise_dir(dir))
            .or_default();
        // Countable where at least one peer brings a counter, or where
        // something has been counted for this transfer already: a tick the
        // engine did not answer keeps what is known, an engine whose peer
        // entries hold no counters is one this launcher cannot count with —
        // and then the folder answers instead of a zero of ours.
        let countable = samples.is_some_and(|s| s.iter().any(|p| p.counted));
        let figure = match samples {
            Some(samples) => transfer.step(status.bytes_total, status.bytes_done, samples),
            None => transfer.arrived(status.bytes_total, status.bytes_done),
        };
        (figure, countable || transfer.counted > 0)
    }

    /// The folder snapshot, at most `max_age` old. Everything that only wants
    /// to display the state uses this; [`ResilioClient::folders`] itself is
    /// for the places that must see the engine as it is right now.
    pub async fn folders_cached(&self, max_age: Duration) -> Result<HashMap<PathBuf, ShareStatus>> {
        if let Some(folders) = self.cached(max_age) {
            return Ok(folders);
        }
        let _one_at_a_time = self.reading.lock().await;
        // Someone else may have been reading while this call waited for its
        // turn; that answer is as fresh as the one this would fetch.
        if let Some(folders) = self.cached(max_age) {
            return Ok(folders);
        }
        let folders = self.read_folders().await?;
        if let Ok(mut cache) = self.folder_cache.lock() {
            *cache = Some((Instant::now(), folders.clone()));
        }
        Ok(folders)
    }

    /// The snapshot if it is younger than `max_age`.
    fn cached(&self, max_age: Duration) -> Option<HashMap<PathBuf, ShareStatus>> {
        let cache = self.folder_cache.lock().ok()?;
        let (at, folders) = cache.as_ref()?;
        (at.elapsed() <= max_age).then(|| folders.clone())
    }

    /// Forget the snapshot: after adding, removing or pausing a share the
    /// next look must see the change, not the second before it.
    pub fn forget_folders(&self) {
        if let Ok(mut cache) = self.folder_cache.lock() {
            *cache = None;
        }
    }

    /// The folder list without the peer count: one request, whatever the
    /// number of shares. Used for the rates in the status bar.
    ///
    /// Without them there is nothing to count what has arrived with, so
    /// `bytes_received` is the coarse figure of the finished files. That is
    /// enough for what this listing is for and wrong for a progress bar, so
    /// the install tick takes [`ResilioClient::folders_cached`] instead.
    pub async fn folders_without_peers(&self) -> Result<HashMap<PathBuf, ShareStatus>> {
        if !self.has_api_key() {
            // The web UI answers everything in one document, and that
            // document is the expensive one: on this engine the rates are
            // a few seconds old rather than fetched every second.
            return self.folders_cached(Duration::from_secs(3)).await;
        }
        let v = self.api("get_folders", &[]).await?;
        Ok(v.as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|f| {
                let dir = crate::paths::strip_verbatim(PathBuf::from(
                    f.get("dir").and_then(Value::as_str).unwrap_or(""),
                ));
                let status = parse_api_folder(&f, 0);
                (dir, status)
            })
            .collect())
    }

    /// All folders known to the engine, keyed by directory. One request plus
    /// one per share for the peer counts, so callers that only display the
    /// state take [`ResilioClient::folders_cached`] instead.
    ///
    /// One reader at a time: three loops ask independently, and the counters
    /// are read as differences from the reading before. Two calls in flight
    /// together could deliver their readings out of order, and the same bytes
    /// would be counted twice.
    pub async fn folders(&self) -> Result<HashMap<PathBuf, ShareStatus>> {
        let _one_at_a_time = self.reading.lock().await;
        self.read_folders().await
    }

    /// The reading itself; the caller holds [`ResilioClient::reading`].
    async fn read_folders(&self) -> Result<HashMap<PathBuf, ShareStatus>> {
        if self.has_api_key() {
            let v = self.api("get_folders", &[]).await?;
            let mut out = HashMap::new();
            for f in v.as_array().cloned().unwrap_or_default() {
                let dir = crate::paths::strip_verbatim(PathBuf::from(
                    f.get("dir").and_then(Value::as_str).unwrap_or(""),
                ));
                let answered = self
                    .api(
                        "get_folder_peers",
                        &[(
                            "secret",
                            f.get("secret").and_then(Value::as_str).unwrap_or(""),
                        )],
                    )
                    .await;
                // How many the engine listed, and how many of those carry
                // counters we can read: a peer entry this parser does not
                // recognise is still a peer, and reporting none of them turns
                // a stalled download into "no sources".
                let mut listed = 0;
                let samples: Option<Vec<PeerSample>> = match answered {
                    Ok(p) => {
                        let entries = p.as_array().cloned().unwrap_or_default();
                        listed = entries.len() as u32;
                        Some(entries.iter().filter_map(parse_api_peer).collect())
                    }
                    // Without a peer count a download looks sourceless in the
                    // UI, so the reason belongs in the log.
                    Err(e) => {
                        log::debug!("no peer count for {}: {e}", dir.display());
                        None
                    }
                };
                let mut status = parse_api_folder(&f, listed);
                let (received, countable) = self.received(&dir, &status, samples.as_deref());
                if countable {
                    status.bytes_received = received;
                } else {
                    // Nothing to count with on this engine: the finished
                    // files are then the only figure there is — too small
                    // during a download and the old package during an update,
                    // but a figure, where a zero of ours would be a claim.
                    status.bytes_received = status.bytes_done;
                }
                status.bytes_known = countable || status.finished_known;
                out.insert(dir, status);
            }
            Ok(out)
        } else {
            let v = self.gui("getsyncfolders", &[("discovery", "1")]).await?;
            Ok(parse_gui_folders(&v))
        }
    }
}

/// Percent-encode a user info field so a generated password cannot break the
/// URL apart. Everything outside the unreserved set is escaped, which is
/// stricter than RFC 3986 needs and always safe.
fn urlencoding(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn parse_gui_token(html: &str) -> Option<String> {
    let idx = html
        .find("id='token'")
        .or_else(|| html.find("id=\"token\""))?;
    let rest = &html[idx..];
    let start = rest.find('>')? + 1;
    let end = rest[start..].find('<')? + start;
    let token = rest[start..end].trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// `get_folders` entry, as 2.8 answers it:
/// `{dir, secret, size, total_size, files, total_files, down_speed, up_speed,
/// error, indexing, paused, type}`.
///
/// `size` is what this PC already holds and `total_size` what the share
/// contains — reading `size` as the total is how a download of many gigabytes
/// came to read "0 B of 782 B" while it was still being indexed.
pub fn parse_api_folder(f: &Value, peers: u32) -> ShareStatus {
    let number = |key: &str| f.get(key).and_then(as_u64_lenient).unwrap_or(0);
    let done = number("size");
    // Without `total_size` (an older engine) there is no way to tell a
    // finished share from one that has just started; saying "complete" then
    // would send a half-downloaded archive to the CRC check.
    let announced = f.get("total_size").and_then(as_u64_lenient);
    // What the engine says the share holds, and 0 where it does not say: an
    // older build without `total_size` must not look as if the bytes it has
    // *are* the share, or every file it finishes reads as the share growing
    // and being replaced.
    let total = announced.unwrap_or(0);
    let for_state = announced.unwrap_or(done).max(done);
    let files = number("total_files").max(number("files"));
    let error = f.get("error").and_then(Value::as_i64).unwrap_or(0);
    let indexing = f.get("indexing").and_then(Value::as_i64).unwrap_or(0) != 0;
    let paused = f.get("paused").and_then(Value::as_i64).unwrap_or(0) != 0
        || f.get("paused").and_then(Value::as_bool).unwrap_or(false);
    let state = if error != 0 {
        ShareState::Error
    } else if paused {
        ShareState::Paused
    } else if indexing {
        ShareState::Indexing
    } else if announced.is_some_and(|t| t > 0 && done >= t)
        && number("total_files") > 0
        && number("files") >= number("total_files")
    {
        // Bytes and files both: while the engine is still learning what the
        // share contains, the few files it knows about can look complete on
        // their own, and the CRC check would then fail on a partial archive.
        ShareState::Complete
    } else if for_state == 0 {
        // The share is registered but no peer has announced its contents yet.
        ShareState::Pending
    } else {
        ShareState::Downloading
    };
    ShareStatus {
        // Like the GUI answer, the API reports the Windows long-path
        // spelling; it would reach the log and the diagnostics card.
        dir: crate::paths::strip_verbatim(PathBuf::from(
            f.get("dir").and_then(Value::as_str).unwrap_or(""),
        )),
        state,
        bytes_done: done,
        bytes_total: total,
        // Without the peers' counters (this is one request earlier) the
        // finished files are all there is to go by.
        bytes_received: done,
        // `size` is the engine's own count of finished files.
        bytes_known: f.get("size").is_some(),
        finished_known: f.get("size").is_some(),
        files_total: files,
        peers,
        download_bps: number("down_speed"),
        upload_bps: number("up_speed"),
        error: (error != 0).then(|| format!("resilio error {error}")),
    }
}

/// What one share's peers have sent during the transfer that is running now.
///
/// Their `download` counters are cumulative for the engine's *session*: they
/// begin again when it restarts or a peer drops out, they do not begin again
/// for a second transfer of the same share, and no reading of them on its own
/// says how much of this package is here. Their *increases* do, so those are
/// what is added up.
#[derive(Debug, Clone, Default)]
struct Transfer {
    seen: bool,
    total: u64,
    finished: u64,
    /// The last counter of each peer, by its id. Per peer, because a peer
    /// that drops out of the list and comes back brings its whole session
    /// total with it: against one sum over all of them that reads as tens of
    /// gigabytes arriving at once.
    last: HashMap<String, u64>,
    counted: u64,
}

impl Transfer {
    /// One reading of the counters; returns what has arrived.
    fn step(&mut self, total: u64, finished: u64, peers: &[PeerSample]) -> u64 {
        // A share starts over when it loses ground — the package it had is
        // replaced, so the finished files or the size fall back — or when a
        // share that *was* complete is given another size: that is an update,
        // and Resilio keeps the old files until the new package is whole, so
        // nothing falls at all. A total that merely grows while the share is
        // still incomplete is an engine learning the size (one without
        // `total_size` answers with what it has), and resetting on that would
        // keep the count at nothing for ever.
        let was_complete = self.total > 0 && self.finished >= self.total;
        // A total of 0 is "the engine did not say" (an older build without
        // `total_size`), not a share that shrank: nothing is started over for
        // a reading that holds no size at all.
        let size_changed = total > 0 && total != self.total;
        let started_over = self.seen
            && (finished < self.finished
                || (total > 0 && total < self.total)
                || (was_complete && size_changed));
        if started_over {
            self.start_over();
        }
        let mut arrived = 0u64;
        for peer in peers {
            match self.last.get_mut(&peer.id) {
                // Only what came in since this peer's own last reading. A
                // counter that fell — a reconnect — adds nothing and carries
                // on from the new figure.
                Some(before) => {
                    if !started_over {
                        arrived += peer.down.saturating_sub(*before);
                    }
                    *before = peer.down;
                }
                // A peer not counted yet. What it has sent arrived while it
                // was connected to us, so it counts — unless this transfer
                // has just started over, where its counter is the one from
                // the transfer before.
                None => {
                    if !started_over {
                        arrived += peer.down;
                    }
                    self.last.insert(peer.id.clone(), peer.down);
                }
            }
        }
        self.counted += arrived;
        self.seen = true;
        // A reading that holds no size does not overwrite the size this share
        // is known to have, or the update after it would go unnoticed.
        if total > 0 {
            self.total = total;
        }
        self.finished = finished;
        self.arrived(total, finished)
    }

    /// This share is being transferred again: what the counters hold belongs
    /// to the transfer before it. Their marks stay — the counters carry on,
    /// so what a peer sends *next* is measured from where it stands now, and
    /// a peer that is not in this reading keeps the mark it had rather than
    /// coming back as a new one with its whole session on the counter.
    fn start_over(&mut self) {
        self.counted = 0;
    }

    /// The figure without a fresh reading: what the peers sent during this
    /// transfer, and nothing else.
    ///
    /// Not the finished files, ever. On a first download they are the eight
    /// bytes of a `version.ini`; during an update they are the *old* package
    /// and would hold the bar at 100 % for the whole of it. Whether a
    /// transfer is an update cannot be told from the engine's answers — that
    /// was tried, and one re-indexing reading or a package that grew by a
    /// tenth is enough to fool any rule for it — so the finished files simply
    /// do not enter this figure. Where the peers cannot be read the count
    /// stays at nothing, which is then what the launcher knows.
    fn arrived(&self, total: u64, _finished: u64) -> u64 {
        if total > 0 {
            self.counted.min(total)
        } else {
            self.counted
        }
    }
}

/// A peer entry of `get_folder_peers` together with the engine's raw
/// counters (`download`/`upload`), which are cumulative bytes; the rates in
/// `peer` stay 0 here and are filled in from two samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSample {
    pub id: String,
    pub peer: SharePeer,
    pub down: u64,
    pub up: u64,
    /// The entry carried a `download` counter. Where no peer does, this
    /// engine cannot say what arrived, and 0 would be a claim, not a figure.
    pub counted: bool,
}

/// Parse one `get_folder_peers` entry:
/// `{id, connection, name, synced, download, upload}`.
pub fn parse_api_peer(v: &Value) -> Option<PeerSample> {
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let id = v
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| name.clone());
    if id.is_empty() {
        return None;
    }
    let counter = v.get("download").and_then(as_u64_lenient);
    let down = counter.unwrap_or(0);
    let up = v.get("upload").and_then(as_u64_lenient).unwrap_or(0);
    Some(PeerSample {
        id,
        peer: SharePeer {
            name,
            connection: v
                .get("connection")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            // `synced` is a timestamp of the last full sync in the API.
            synced: v.get("synced").and_then(as_u64_lenient).unwrap_or(0) > 0,
            download_bps: 0,
            upload_bps: 0,
        },
        down,
        up,
        counted: counter.is_some(),
    })
}

/// Rate from two samples of a cumulative counter. 0 while no usable previous
/// sample exists, and on a counter reset (engine restart, peer reconnect).
pub fn counter_rate(previous: Option<(u64, Instant)>, value: u64, now: Instant) -> u64 {
    let Some((before, at)) = previous else {
        return 0;
    };
    let seconds = now.duration_since(at).as_secs_f64();
    if seconds < 0.5 || value < before {
        return 0;
    }
    ((value - before) as f64 / seconds).round() as u64
}

/// Resilio's "this folder is already added": the documented API answers 200,
/// older builds 5. Matched on the parsed number, so `error 500` is not taken
/// for `error 5`.
fn is_already_added(e: &Error) -> bool {
    let Error::Transport(m) = e else {
        return false;
    };
    m.split("error ")
        .nth(1)
        .and_then(|rest| {
            rest.split_whitespace()
                .next()
                .and_then(|n| n.parse::<i64>().ok())
        })
        .is_some_and(|code| code == 5 || code == 200)
}

/// Does a folder listing already contain `dir` with this secret?
///
/// Takes both shapes: the API's top-level array of `{dir, secret}` and the
/// web UI's `{folders:[{name|path, secret}]}`. A folder whose entry carries
/// no secret at all counts as a match: some builds leave it out, and the
/// alternative would be to report a working share as broken on every start.
pub fn folder_has_secret(v: &Value, dir: &Path, secret: &str) -> bool {
    let want = super::normalise_dir(dir);
    let folders = v
        .as_array()
        .cloned()
        .or_else(|| v.get("folders").and_then(Value::as_array).cloned())
        .or_else(|| {
            v.get("value")
                .and_then(|x| x.get("folders"))
                .and_then(Value::as_array)
                .cloned()
        })
        .unwrap_or_default();
    folders.iter().any(|f| {
        let path = f
            .get("dir")
            .or_else(|| f.get("path"))
            .or_else(|| f.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if super::normalise_dir(Path::new(path)) != want {
            return false;
        }
        match f.get("secret").and_then(Value::as_str) {
            Some(s) => s.eq_ignore_ascii_case(secret),
            None => true,
        }
    })
}

/// `getsyncfolders` response: `{folders:[{name, path, size, files, status,
/// peers:[{status,...}], progress?, error?, paused?, ...}]}`. Field names have
/// changed between versions, so every lookup is defensive.
pub fn parse_gui_folders(v: &Value) -> HashMap<PathBuf, ShareStatus> {
    let mut out = HashMap::new();
    let folders = v
        .get("folders")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| {
            v.get("value")
                .and_then(|x| x.get("folders"))
                .and_then(Value::as_array)
                .cloned()
        })
        .unwrap_or_default();
    for f in folders {
        // Resilio answers with the Windows long-path spelling for a folder
        // registered without it; it would show up that way in the UI and in
        // the log.
        let path = crate::paths::strip_verbatim(PathBuf::from(
            f.get("path")
                .or_else(|| f.get("name"))
                .and_then(Value::as_str)
                .unwrap_or(""),
        ))
        .to_string_lossy()
        .to_string();
        let size = f.get("size").and_then(as_u64_lenient).unwrap_or(0);
        let files = f.get("files").and_then(as_u64_lenient).unwrap_or(0);
        let peers = f
            .get("peers")
            .and_then(Value::as_array)
            .map(|a| a.len() as u32)
            .unwrap_or(0);
        let paused = f.get("paused").and_then(Value::as_bool).unwrap_or(false)
            || f.get("paused").and_then(Value::as_i64).unwrap_or(0) != 0;
        let status_text = f
            .get("status")
            .map(|s| s.to_string().to_ascii_lowercase())
            .unwrap_or_default();
        let progress = f.get("progress").and_then(as_u64_lenient);
        let error = f
            .get("error")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        // The third value says whether the second is a figure from the
        // engine or a stand-in. The web UI answers for many shares with a
        // state and nothing countable, and a zero of ours read as "nothing
        // has arrived" would stop a finished download from ever being
        // verified.
        // Four values: the state, what is *finished* (which decides whether
        // an archive may be verified — below 100 % nothing is), what has
        // arrived (the web UI's own percentage covers the file in flight),
        // and whether either is a figure from the engine at all.
        let (state, finished, received, known) = if paused {
            (ShareState::Paused, 0, 0, false)
        } else if error.is_some() {
            (ShareState::Error, 0, 0, false)
        } else if status_text.contains("index") {
            (ShareState::Indexing, 0, 0, false)
        } else if let Some(p) = progress {
            if p >= 100 {
                (ShareState::Complete, size, size, true)
            } else {
                (ShareState::Downloading, 0, size * p / 100, true)
            }
        } else if status_text.contains("synced") || status_text.contains("up to date") {
            (ShareState::Complete, size, size, true)
        } else if size == 0 {
            (ShareState::Pending, 0, 0, false)
        } else {
            (ShareState::Downloading, 0, 0, false)
        };
        out.insert(
            PathBuf::from(&path),
            ShareStatus {
                dir: PathBuf::from(&path),
                state,
                bytes_done: finished,
                bytes_total: size,
                bytes_received: received,
                bytes_known: known,
                // Its percentage covers the file in flight, so below 100 %
                // the web UI has no answer to "how much is finished".
                finished_known: state == ShareState::Complete,
                files_total: files,
                peers,
                download_bps: f.get("down").and_then(as_u64_lenient).unwrap_or(0),
                upload_bps: f.get("up").and_then(as_u64_lenient).unwrap_or(0),
                error,
            },
        );
    }
    out
}

fn as_u64_lenient(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_f64().map(|f| f.max(0.0) as u64))
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// Managed Resilio process.
pub struct ResilioTransport {
    pub config: ResilioConfig,
    config_path: PathBuf,
    client: ResilioClient,
    child: Mutex<Option<Child>>,
    /// Keys per directory, needed for remove_folder.
    keys: Mutex<HashMap<PathBuf, ShareKey>>,
    lan_only: Mutex<bool>,
    /// Last health detail written to the log (logged only on change).
    last_detail: Mutex<Option<String>>,
    /// Last peer counters per `<share dir>\0<peer id>`, for the rates.
    peer_samples: Mutex<HashMap<String, (u64, u64, Instant)>>,
    /// One raw peer entry is logged per run so the field names of this engine
    /// build can be checked against what the parser expects.
    peer_shape_logged: std::sync::atomic::AtomicBool,
}

impl std::fmt::Debug for ResilioTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResilioTransport")
            .field("binary", &self.config.binary)
            .field("api_port", &self.config.api_port)
            .finish()
    }
}

impl ResilioTransport {
    pub fn new(mut config: ResilioConfig) -> Self {
        if config.api_port == 0 {
            config.api_port = pick_free_port().unwrap_or(DEFAULT_LAN_PORT);
        }
        let config_path = config.storage_dir.join("config.json");
        let client = ResilioClient::new(
            format!("http://127.0.0.1:{}", config.api_port),
            config.login.clone(),
            config.password.clone(),
            config.api_key.clone(),
        );
        let lan_only = config.lan_only;
        Self {
            config,
            config_path,
            client,
            child: Mutex::new(None),
            keys: Mutex::new(HashMap::new()),
            last_detail: Mutex::new(None),
            peer_samples: Mutex::new(HashMap::new()),
            peer_shape_logged: std::sync::atomic::AtomicBool::new(false),
            lan_only: Mutex::new(lan_only),
        }
    }

    pub fn client(&self) -> &ResilioClient {
        &self.client
    }

    /// Kill Resilio processes that were started by a previous launcher run
    /// (pid file) or are otherwise orphaned. Returns the number terminated.
    /// This replaces the "Too many workers" failure mode of the old launcher.
    pub fn cleanup_orphans(&self) -> usize {
        let mut killed = 0;
        let pid_file = self.config.storage_dir.join(PID_FILE);
        let pid_from_file: Option<u32> = std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|s| s.trim().parse().ok());
        let sys = scan_processes();
        for (pid, proc_) in real_processes(&sys) {
            let from_our_storage = proc_.cmd().iter().any(|a| {
                a.to_string_lossy()
                    .contains(&*self.config.storage_dir.to_string_lossy())
            });
            let is_ours = pid_from_file == Some(pid.as_u32()) || from_our_storage;
            if is_ours && is_sync_engine(proc_.name()) && proc_.kill() {
                killed += 1;
            }
        }
        let _ = std::fs::remove_file(&pid_file);
        killed
    }

    /// Windows: the downloaded `Resilio-Sync_x64.exe` is the program itself,
    /// not an installer. `/noinstall` keeps it from copying itself into
    /// `%APPDATA%` and showing the install dialog on first run; `/config`
    /// points at our generated config (storage_path, API), as the ETI
    /// launcher does with its `btsync.exe`.
    pub fn command_args(config_path: &Path) -> Vec<String> {
        if cfg!(target_os = "windows") {
            vec![
                "/noinstall".into(),
                "/config".into(),
                config_path.to_string_lossy().to_string(),
                "/minimized".into(),
            ]
        } else {
            vec![
                "--config".into(),
                config_path.to_string_lossy().to_string(),
                "--nodaemon".into(),
            ]
        }
    }

    async fn wait_for_api(&self, timeout: Duration) -> Result<()> {
        let start = std::time::Instant::now();
        loop {
            // The probe error goes into the timeout message: "connection
            // refused" means the engine is not up, an HTTP 401 means it is up
            // but rejects our API key or login.
            let last_probe = match self.client.version().await {
                Ok(_) => return Ok(()),
                Err(e) => e.to_string(),
            };
            // An engine that quit right away (another instance of the same
            // binary is running, bad config) is reported as such instead of
            // as a timeout.
            // The pid must be read before try_wait: once the exit has been
            // observed, tokio's Child::id() returns None.
            let (own_pid, exited) = self
                .child
                .lock()
                .ok()
                .and_then(|mut c| c.as_mut().map(|ch| (ch.id(), ch.try_wait().ok().flatten())))
                .unwrap_or((None, None));
            if let Some(status) = exited {
                let binary = self.config.binary.clone();
                let scan = tokio::task::spawn_blocking(move || engine_scan(&binary, own_pid))
                    .await
                    .unwrap_or_default();
                let (others, engines) = (scan.same_binary, scan.other_engines);
                let why = if others.is_empty() && engines.is_empty() {
                    "no other sync engine is running; see the engine log for the reason".to_string()
                } else if others.is_empty() {
                    format!(
                        "another Resilio is running from a different file ({}) and Resilio may allow only one instance per machine: exit it (tray icon, Exit) and retry, or use folder mode with that instance",
                        engines
                            .iter()
                            .map(|(pid, exe)| format!("pid {pid}: {exe}"))
                            .collect::<Vec<_>>()
                            .join("; ")
                    )
                } else {
                    format!(
                        "the same program is already running outside the launcher (pid {}) and Resilio allows one instance per binary: close it (tray icon, Exit) or choose another binary in Settings",
                        others
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                // The engine's own words are the best hint; both files are
                // tiny at this point.
                for name in ["engine-output.log", "sync.log"] {
                    let path = self.config.storage_dir.join(name);
                    if let Ok(text) = std::fs::read_to_string(&path) {
                        let tail: Vec<&str> = text.lines().rev().take(15).collect();
                        if !tail.is_empty() {
                            log::warn!(
                                "{name} after early exit:\n{}",
                                tail.into_iter().rev().collect::<Vec<_>>().join("\n")
                            );
                        }
                    }
                }
                return Err(Error::Transport(format!(
                    "Resilio Sync exited with {status} before its API came up; {why} ({})",
                    self.startup_hint()
                )));
            }
            if start.elapsed() > timeout {
                return Err(Error::Transport(format!(
                    "Resilio API did not become reachable within {}s; last probe: {last_probe} ({})",
                    timeout.as_secs(),
                    self.startup_hint()
                )));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Share secret for a directory: from the cache filled by `add_share`,
    /// else from the engine's own folder list (a share the ETI launcher or a
    /// previous run added).
    async fn secret_for(&self, dir: &Path) -> Option<String> {
        let want = super::normalise_dir(dir);
        let cached = self.keys.lock().ok().and_then(|k| {
            k.iter()
                .find(|(d, _)| super::normalise_dir(d) == want)
                .map(|(_, key)| key.expose().to_string())
        });
        if cached.is_some() {
            return cached;
        }
        let v = self.client.api("get_folders", &[]).await.ok()?;
        v.as_array()?.iter().find_map(|f| {
            let d = f.get("dir").and_then(Value::as_str)?;
            (super::normalise_dir(Path::new(d)) == want)
                .then(|| f.get("secret").and_then(Value::as_str))
                .flatten()
                .map(str::to_string)
        })
    }

    /// Where to look when the engine does not come up.
    fn startup_hint(&self) -> String {
        format!(
            "binary {}, api port {}, engine log {}",
            self.config.binary.display(),
            self.config.api_port,
            self.config.storage_dir.join("sync.log").display()
        )
    }
}

/// Pids running exactly `binary` (canonicalised), our own child excluded;
/// see [`engine_scan`].
pub fn foreign_instances(binary: &Path, own_pid: Option<u32>) -> Vec<u32> {
    engine_scan(binary, own_pid).same_binary
}

/// One pass over the process table for the engine-start diagnosis.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct EngineScan {
    /// Pids running exactly `binary` (canonicalised), our child excluded.
    pub same_binary: Vec<u32>,
    /// Other sync engines by process name (any path), our child excluded,
    /// as `(pid, executable or name)`.
    pub other_engines: Vec<(u32, String)>,
}

pub fn engine_scan(binary: &Path, own_pid: Option<u32>) -> EngineScan {
    // Process tables report the resolved executable; the configured binary
    // may be a symlink or a non-canonical spelling.
    let canon =
        |p: &Path| super::normalise_dir(&std::fs::canonicalize(p).unwrap_or(p.to_path_buf()));
    let want = canon(binary);
    let sys = scan_processes_with(
        sysinfo::ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::Always),
    );
    let mut scan = EngineScan::default();
    for (pid, p) in real_processes(&sys) {
        if Some(pid.as_u32()) == own_pid {
            continue;
        }
        if p.exe().is_some_and(|e| canon(e) == want) {
            scan.same_binary.push(pid.as_u32());
        } else if is_sync_engine(p.name()) {
            scan.other_engines.push((
                pid.as_u32(),
                p.exe()
                    .map(|e| e.display().to_string())
                    .unwrap_or_else(|| p.name().to_string_lossy().to_string()),
            ));
        }
    }
    scan.same_binary.sort_unstable();
    scan.other_engines.sort_unstable();
    scan
}

fn pick_free_port() -> Option<u16> {
    std::net::TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
}

#[async_trait]
impl Transport for ResilioTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::Resilio
    }

    fn process_id(&self) -> Option<u32> {
        self.child
            .lock()
            .ok()
            .and_then(|c| c.as_ref().and_then(|ch| ch.id()))
    }

    async fn start(&self) -> Result<()> {
        if !self.config.binary.is_file() {
            return Err(Error::Transport(format!(
                "Resilio Sync binary not found at {}",
                self.config.binary.display()
            )));
        }
        self.cleanup_orphans();
        self.config.write(&self.config_path)?;
        let args = Self::command_args(&self.config_path);
        log::info!(
            "starting sync engine: {} {} (storage {}, api 127.0.0.1:{})",
            self.config.binary.display(),
            args.join(" "),
            self.config.storage_dir.display(),
            self.config.api_port
        );
        // Engine output goes to a file next to its storage so a failed start
        // can be diagnosed from the data directory.
        let output = std::fs::File::create(self.config.storage_dir.join("engine-output.log")).ok();
        let mut cmd = Command::new(&self.config.binary);
        cmd.args(&args)
            .current_dir(&self.config.storage_dir)
            .stdin(Stdio::null())
            .stdout(match output.as_ref().and_then(|f| f.try_clone().ok()) {
                Some(f) => Stdio::from(f),
                None => Stdio::null(),
            })
            .stderr(match output {
                Some(f) => Stdio::from(f),
                None => Stdio::null(),
            })
            .kill_on_drop(true);
        let child = cmd
            .spawn()
            .map_err(|e| Error::Transport(format!("cannot start Resilio Sync: {e}")))?;
        if let Ok(mut c) = self.child.lock() {
            *c = Some(child);
        }
        self.wait_for_api(Duration::from_secs(20)).await
    }

    async fn stop(&self) -> Result<()> {
        // Short timeouts: this also runs while the launcher is closing, where
        // an engine that ignores the request is killed rather than waited for.
        let _ = tokio::time::timeout(Duration::from_secs(3), self.client.shutdown()).await;
        let child = self.child.lock().ok().and_then(|mut c| c.take());
        if let Some(mut child) = child {
            if tokio::time::timeout(Duration::from_secs(3), child.wait())
                .await
                .is_err()
            {
                log::info!("sync engine did not exit on request; killing it");
                let _ = child.kill().await;
                let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
            }
        }
        Ok(())
    }

    async fn health(&self) -> TransportHealth {
        let running = self
            .child
            .lock()
            .ok()
            .map(|mut c| {
                c.as_mut()
                    .map(|ch| matches!(ch.try_wait(), Ok(None)))
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let version = self.client.version().await.ok();
        // One folder query feeds both counters; an API failure leaves the
        // server state unknown rather than "not found".
        let folders = self.client.folders().await;
        let summary = folders
            .as_ref()
            .ok()
            .map(|f| super::peer_summary(f.values()));
        // What the engine reports for the catalog share(s), for the
        // diagnostics page and the log (only when it changes): peers, state,
        // error. Sorted so the text is stable between polls.
        let catalog = match &folders {
            Ok(f) => {
                let mut shares: Vec<String> = f
                    .values()
                    .filter(|s| super::is_catalog_share(&s.dir))
                    .map(|s| {
                        format!(
                            "{}: {} peers, {:?}{}",
                            s.dir.display(),
                            s.peers,
                            s.state,
                            s.error
                                .as_deref()
                                .map(|e| format!(", {e}"))
                                .unwrap_or_default()
                        )
                    })
                    .collect();
                shares.sort();
                if shares.is_empty() {
                    "catalog share not listed by the engine".to_string()
                } else {
                    shares.join(" | ")
                }
            }
            Err(e) => format!("folder list unavailable: {e}"),
        };
        let detail = format!(
            "{catalog}; web UI http://127.0.0.1:{}/gui/ (login {}, password in {})",
            self.config.api_port,
            self.config.login,
            self.config_path.display()
        );
        // The engine's password is random per installation, so nobody can be
        // expected to type it. The credentials ride along in the URL and the
        // browser logs in by itself, the way the ETI launcher did.
        let web_ui = Some(format!(
            "http://{}:{}@127.0.0.1:{}/gui/",
            urlencoding(&self.config.login),
            urlencoding(&self.config.password),
            self.config.api_port
        ));
        if let Ok(mut last) = self.last_detail.lock() {
            if last.as_deref() != Some(detail.as_str()) {
                log::info!("sync engine: {detail}");
                *last = Some(detail.clone());
            }
        }
        TransportHealth {
            kind: TransportKind::Resilio,
            running,
            api_reachable: version.is_some(),
            version,
            peers: summary.map(|s| s.total).unwrap_or(0),
            catalog_peers: summary.and_then(|s| s.catalog).unwrap_or(0),
            server_found: summary.and_then(|s| s.catalog).map(|n| n > 0),
            peer_details: self.client.has_api_key(),
            lan_mode: *self.lan_only.lock().unwrap_or_else(|e| e.into_inner()),
            detail: Some(detail),
            download_bps: summary.map(|s| s.download_bps).unwrap_or(0),
            upload_bps: summary.map(|s| s.upload_bps).unwrap_or(0),
            web_ui,
        }
    }

    async fn add_share(&self, key: &ShareKey, dir: &Path, opts: &ShareOptions) -> Result<()> {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        // A folder the engine still knows from an install the user removed
        // keeps its old state — "folder not found" once the directory was
        // deleted — and re-adding it is accepted as "already added" without
        // fixing anything: the share then sits there reporting a finished
        // download that never moves. Taking it out first is the only way back.
        if let Ok(Some(status)) = self.share_status(dir).await {
            if status.state == ShareState::Error {
                log::info!(
                    "sync engine reports an error for {}: {} — re-adding the share",
                    dir.display(),
                    status.error.as_deref().unwrap_or("no detail")
                );
                // With the key from the caller, not from the map of shares
                // this process added: after a restart that map is empty, and
                // that is exactly when a folder deleted in the meantime shows
                // up as an error.
                if let Err(e) = self.client.remove_folder(dir, Some(key)).await {
                    log::warn!("could not remove the broken share {}: {e}", dir.display());
                }
                if let Ok(mut k) = self.keys.lock() {
                    k.remove(dir);
                }
                self.client.forget_folders();
                // The engine needs a moment before it accepts the folder
                // again; without it the re-add is answered "already added"
                // and nothing has changed.
                tokio::time::sleep(Duration::from_millis(300)).await;
                std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
            }
        }
        self.client.add_folder(key, dir, opts.lan_only).await?;
        self.client.forget_folders();
        if let Ok(mut k) = self.keys.lock() {
            k.insert(dir.to_path_buf(), key.clone());
        }
        Ok(())
    }

    async fn remove_share(&self, dir: &Path) -> Result<()> {
        let key = self.keys.lock().ok().and_then(|k| k.get(dir).cloned());
        self.client.remove_folder(dir, key.as_ref()).await?;
        self.client.forget_folders();
        if let Ok(mut k) = self.keys.lock() {
            k.remove(dir);
        }
        Ok(())
    }

    async fn set_paused(&self, dir: &Path, paused: bool) -> Result<()> {
        let dir_s = dir.to_string_lossy().to_string();
        let result = if self.client.has_api_key() {
            let key = self.keys.lock().ok().and_then(|k| k.get(dir).cloned());
            let secret = key.map(|k| k.expose().to_string()).unwrap_or_default();
            self.client
                .api(
                    "set_folder_prefs",
                    &[
                        ("dir", dir_s.as_str()),
                        ("secret", secret.as_str()),
                        ("paused", if paused { "1" } else { "0" }),
                    ],
                )
                .await
                .map(|_| ())
        } else {
            self.client
                .gui(
                    "setfolderpref",
                    &[
                        ("name", dir_s.as_str()),
                        ("pref", "paused"),
                        ("value", if paused { "1" } else { "0" }),
                    ],
                )
                .await
                .map(|_| ())
        };
        // After the request, not before: a poll landing in between would put
        // the state from a moment ago back into the snapshot.
        self.client.forget_folders();
        result
    }

    fn invalidate(&self) {
        self.client.forget_folders();
    }

    async fn rates(&self) -> Result<TransportRates> {
        // `get_folders` alone: the peer count per share costs a request each
        // and the status bar gets that from the health poll, which is slower
        // for a reason.
        let folders = self.client.folders_without_peers().await?;
        Ok(super::peer_summary(folders.values()).into())
    }

    async fn share_status(&self, dir: &Path) -> Result<Option<ShareStatus>> {
        let folders = self.client.folders_cached(FOLDER_CACHE).await?;
        Ok(folders.get(dir).cloned().or_else(|| {
            folders
                .values()
                .find(|s| normalise_dir(&s.dir) == normalise_dir(dir))
                .cloned()
        }))
    }

    async fn share_peers(&self, dir: &Path) -> Result<Vec<SharePeer>> {
        // Only the documented API lists peers per share; the web-UI folder
        // list carries no usable per-peer figures.
        if !self.client.has_api_key() {
            return Ok(Vec::new());
        }
        let Some(secret) = self.secret_for(dir).await else {
            return Ok(Vec::new());
        };
        let raw = self.client.folder_peers(&secret).await?;
        if let Some(first) = raw.first() {
            if !self
                .peer_shape_logged
                .swap(true, std::sync::atomic::Ordering::Relaxed)
            {
                log::info!("peer entry as reported by the engine: {first}");
            }
        }
        let now = Instant::now();
        let key = super::normalise_dir(dir);
        let mut out = Vec::new();
        for sample in raw.iter().filter_map(parse_api_peer) {
            let id = format!("{key}\0{}", sample.id);
            let previous = self
                .peer_samples
                .lock()
                .ok()
                .and_then(|m| m.get(&id).copied());
            let mut peer = sample.peer;
            peer.download_bps = counter_rate(previous.map(|(d, _, at)| (d, at)), sample.down, now);
            peer.upload_bps = counter_rate(previous.map(|(_, u, at)| (u, at)), sample.up, now);
            if let Ok(mut m) = self.peer_samples.lock() {
                m.insert(id, (sample.down, sample.up, now));
            }
            out.push(peer);
        }
        out.sort_by(|a, b| {
            b.download_bps
                .cmp(&a.download_bps)
                .then(a.name.cmp(&b.name))
        });
        Ok(out)
    }

    async fn list_shares(&self) -> Result<Vec<ShareStatus>> {
        Ok(self
            .client
            .folders_cached(FOLDER_CACHE)
            .await?
            .into_values()
            .collect())
    }

    async fn set_lan_mode(&self, lan_only: bool) -> Result<()> {
        if let Ok(mut l) = self.lan_only.lock() {
            *l = lan_only;
        }
        let dirs: Vec<(PathBuf, ShareKey)> = self
            .keys
            .lock()
            .map(|k| k.iter().map(|(d, k)| (d.clone(), k.clone())).collect())
            .unwrap_or_default();
        for (dir, key) in dirs {
            let dir_s = dir.to_string_lossy().to_string();
            if self.client.has_api_key() {
                let mut p = vec![("dir", dir_s.as_str()), ("secret", key.expose())];
                p.extend(folder_prefs(lan_only));
                let _ = self.client.api("set_folder_prefs", &p).await;
            }
        }
        Ok(())
    }
}

/// Binary names accepted on Windows. `btsync.exe` is the renamed engine the
/// original ETI launcher ships; it speaks the same API.
pub const WINDOWS_BINARY_NAMES: &[&str] = &["Resilio Sync.exe", "rslsync.exe", "btsync.exe"];
/// File name of Resilio's Windows download (the program itself, see
/// `resilio.lock.json`).
pub const WINDOWS_DOWNLOAD_NAME: &str = "Resilio-Sync_x64.exe";

/// Environment for the Windows binary search, injectable for tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WinEnv {
    /// `%ProgramFiles%`, `%ProgramFiles(x86)%`.
    pub program_files: Vec<PathBuf>,
    /// `%LOCALAPPDATA%` of the current (possibly elevated) account.
    pub local_app_data: Option<PathBuf>,
    /// `%APPDATA%` (Roaming): the per-user Resilio installer puts
    /// `Resilio Sync.exe` there, next to its storage folder.
    pub roaming_app_data: Option<PathBuf>,
    /// Entries of `%PATH%`.
    pub path: Vec<PathBuf>,
    /// Profile directories under `C:\Users`, for a per-user Resilio installed
    /// by another account than the elevated one running the launcher.
    pub user_profiles: Vec<PathBuf>,
    /// `InstallLocation` values from the uninstall registry keys.
    pub registry_install_dirs: Vec<PathBuf>,
}

impl WinEnv {
    /// Collect the search environment from the running process.
    /// Only the Program Files folders, without the registry and profile
    /// probing `from_process` does; cheap enough for the async threads.
    pub fn program_files_from_env() -> Vec<PathBuf> {
        ["ProgramFiles", "ProgramFiles(x86)"]
            .iter()
            .filter_map(|var| std::env::var_os(var).filter(|v| !v.is_empty()))
            .map(PathBuf::from)
            .collect()
    }

    pub fn from_process() -> Self {
        let mut env = WinEnv::default();
        env.program_files = Self::program_files_from_env();
        env.local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        env.roaming_app_data = std::env::var_os("APPDATA").map(PathBuf::from);
        if let Some(path) = std::env::var_os("PATH") {
            env.path = std::env::split_paths(&path)
                .filter(|p| !p.as_os_str().is_empty())
                .collect();
        }
        let users_root = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .and_then(|p| p.parent().map(Path::to_path_buf));
        if let Some(root) = users_root {
            if let Ok(rd) = std::fs::read_dir(root) {
                env.user_profiles = rd.flatten().map(|e| e.path()).collect();
            }
        }
        if cfg!(target_os = "windows") {
            env.registry_install_dirs = registry_install_dirs();
        }
        env
    }
}

/// Ordered Windows candidates: a regular Resilio install, the ETI launcher's
/// bundled engine, PATH, other user profiles, registry. Pure, no IO.
pub fn windows_candidates(env: &WinEnv) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(roaming) = &env.roaming_app_data {
        out.push(roaming.join("Resilio Sync").join("Resilio Sync.exe"));
    }
    if let Some(local) = &env.local_app_data {
        out.push(local.join("Resilio Sync").join("Resilio Sync.exe"));
    }
    for pf in &env.program_files {
        out.push(pf.join("Resilio Sync").join("Resilio Sync.exe"));
    }
    for pf in &env.program_files {
        let eti = pf.join("eti").join("lan launcher");
        out.extend(WINDOWS_BINARY_NAMES.iter().map(|n| eti.join(n)));
    }
    for dir in &env.path {
        out.extend(WINDOWS_BINARY_NAMES.iter().map(|n| dir.join(n)));
    }
    for profile in &env.user_profiles {
        for sub in ["Roaming", "Local"] {
            out.push(
                profile
                    .join("AppData")
                    .join(sub)
                    .join("Resilio Sync")
                    .join("Resilio Sync.exe"),
            );
        }
    }
    for dir in &env.registry_install_dirs {
        out.extend(WINDOWS_BINARY_NAMES.iter().map(|n| dir.join(n)));
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(super::normalise_dir(p)));
    out
}

/// `InstallLocation` of the Resilio Sync uninstall entries (per user and
/// machine-wide), read with `reg query` so no registry crate is needed.
fn registry_install_dirs() -> Vec<PathBuf> {
    const KEYS: &[&str] = &[
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Resilio Sync",
        r"HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\Resilio Sync",
        r"HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Resilio Sync",
    ];
    KEYS.iter()
        .filter_map(|key| {
            crate::launch::elevate::hide_window_std(&mut std::process::Command::new("reg"))
                .args(["query", key, "/v", "InstallLocation"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| parse_reg_query_output(&String::from_utf8_lossy(&o.stdout)))
        })
        .collect()
}

/// Extract the value from `reg query … /v X` output
/// (`    X    REG_SZ    C:\dir`).
pub fn parse_reg_query_output(out: &str) -> Option<PathBuf> {
    out.lines().find_map(|line| {
        let (_, value) = line.split_once("REG_SZ")?;
        let value = value.trim().trim_end_matches(['\\', '/']);
        (!value.is_empty()).then(|| PathBuf::from(value))
    })
}

/// Is this a sync engine process name (see [`process_names`])?
pub fn is_sync_engine(name: &std::ffi::OsStr) -> bool {
    let name = name.to_string_lossy();
    process_names().iter().any(|n| name.eq_ignore_ascii_case(n))
}

/// Process table with only what the launcher needs (name, executable,
/// command line), shared by orphan cleanup, diagnostics and binary discovery.
pub fn scan_processes() -> sysinfo::System {
    scan_processes_with(
        sysinfo::ProcessRefreshKind::nothing()
            .with_exe(sysinfo::UpdateKind::Always)
            .with_cmd(sysinfo::UpdateKind::Always),
    )
}

/// The process table with its threads left out.
///
/// On Linux `sysinfo` reports a task per thread next to the one per process,
/// and a thread carries its process's name — measured on one machine: 121
/// entries, of which 108 were threads. Every check in this file asks "is
/// something like this running", so counting threads turns a single sync
/// engine into twenty. That is what a Steam Deck's Diagnose panel showed:
/// "Fremde Sync-Prozesse laufen" listing 21 consecutive pids, all of them
/// threads of the launcher's own engine, which no `own_pid` test can exclude
/// because each thread has an id of its own.
///
/// Every caller that walks the table goes through here.
pub fn real_processes(
    sys: &sysinfo::System,
) -> impl Iterator<Item = (&sysinfo::Pid, &sysinfo::Process)> {
    sys.processes()
        .iter()
        .filter(|(_, p)| p.thread_kind().is_none())
}

/// Process table refreshed with exactly the fields the caller reads.
///
/// `without_tasks` spares most of the walk through `/proc/<pid>/task/<tid>/`:
/// measured on one machine, 121 entries and 108 threads become 80 and 67, and
/// a scan takes half as long. It does **not** leave every thread out, so it is
/// an economy and not the fix — [`real_processes`] is what the callers must go
/// through.
pub fn scan_processes_with(kind: sysinfo::ProcessRefreshKind) -> sysinfo::System {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, kind.without_tasks());
    sys
}

/// Executables of sync engines that are running right now (any user), a
/// reliable hint where an installation lives.
pub fn running_binaries() -> Vec<PathBuf> {
    let sys = scan_processes();
    let mut out: Vec<PathBuf> = real_processes(&sys)
        .map(|(_, p)| p)
        .filter(|p| is_sync_engine(p.name()))
        .filter_map(|p| p.exe().map(Path::to_path_buf))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Outcome of the binary search, including every path that was probed so a
/// failed search can be explained in the log.
#[derive(Debug, Clone, Default)]
pub struct LocateResult {
    pub found: Option<PathBuf>,
    pub probed: Vec<PathBuf>,
}

/// Where to find the Resilio Sync binary, in order: a user-configured path,
/// the copy bundled with the app (every CI and release build ships the build
/// pinned in `resilio.lock.json`), a previous download in the data dir, the
/// executables of running sync engines, and finally installations on the
/// system. The bundled copy ranks first on purpose: it is the same version
/// everywhere and does not collide with a user's own Resilio, which refuses
/// a second start of the same executable.
pub fn locate_binary_detailed(
    override_path: Option<&Path>,
    resource_dir: Option<&Path>,
    data_dir: &Path,
) -> LocateResult {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = override_path {
        candidates.push(p.to_path_buf());
    }
    let names: &[&str] = if cfg!(target_os = "windows") {
        WINDOWS_BINARY_NAMES
    } else if cfg!(target_os = "macos") {
        &[
            "Resilio Sync.app/Contents/MacOS/Resilio Sync",
            "Resilio Sync",
            "rslsync",
        ]
    } else {
        &["rslsync"]
    };
    if let Some(bundled) = resource_dir.map(|r| r.join("resilio")) {
        // The name the fetch script gives the bundled file comes from
        // resilio.lock.json (`install`), the single source of truth shared
        // with tools/fetch-resilio.mjs.
        if let Some(name) = bundled_install_name() {
            candidates.push(bundled.join(name));
        }
        for n in names {
            candidates.push(bundled.join(n));
        }
        if cfg!(target_os = "windows") {
            // The download name, in case the fetch script did not rename it.
            candidates.push(bundled.join(WINDOWS_DOWNLOAD_NAME));
        }
    }
    for n in names {
        candidates.push(data_dir.join("resilio").join(n));
    }
    // A running engine (the user's own Resilio, the ETI launcher's btsync)
    // ranks after the bundled/pinned binary but before guessing paths.
    candidates.extend(running_binaries());
    if cfg!(target_os = "windows") {
        candidates.extend(windows_candidates(&WinEnv::from_process()));
    } else if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from(
            "/Applications/Resilio Sync.app/Contents/MacOS/Resilio Sync",
        ));
        if let Some(h) = std::env::var_os("HOME") {
            candidates.push(
                PathBuf::from(h).join("Applications/Resilio Sync.app/Contents/MacOS/Resilio Sync"),
            );
        }
    } else {
        candidates.push(PathBuf::from("/usr/bin/rslsync"));
        candidates.push(PathBuf::from("/usr/local/bin/rslsync"));
        if let Some(path) = std::env::var_os("PATH") {
            candidates.extend(std::env::split_paths(&path).map(|d| d.join("rslsync")));
        }
    }
    // Tauri's `resource_dir()` is a verbatim path on Windows, and every
    // candidate built from it inherits the prefix. `netsh` rejects it, so the
    // firewall rules for the engine would fail on exactly the installed build.
    let found = candidates
        .iter()
        .find(|p| p.is_file())
        .cloned()
        .map(crate::paths::strip_verbatim);
    LocateResult {
        found,
        probed: candidates,
    }
}

/// The repository's `resilio.lock.json`, embedded so the app points users to
/// the same pinned build the release bundles instead of whatever `stable`
/// currently is (3.x requires a Resilio account).
const LOCK_JSON: &str = include_str!("../../../../resilio.lock.json");

/// Lock-file platform key for the running binary.
fn lock_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else if cfg!(target_arch = "aarch64") {
        "linux-arm64"
    } else {
        "linux-x64"
    }
}

static LOCK: std::sync::LazyLock<serde_json::Value> =
    std::sync::LazyLock::new(|| serde_json::from_str(LOCK_JSON).unwrap_or(serde_json::Value::Null));

/// File name (`install`) under which the fetch script places the bundled
/// binary for this platform, from `resilio.lock.json`.
pub fn bundled_install_name() -> Option<String> {
    LOCK.get("artifacts")?
        .get(lock_platform())?
        .get("install")?
        .as_str()
        .map(str::to_owned)
}

/// Pinned download URL from `resilio.lock.json` for the given platform key.
pub fn pinned_download_url(platform: &str) -> Option<String> {
    LOCK.get("artifacts")?
        .get(platform)?
        .get("url")?
        .as_str()
        .map(str::to_owned)
}

/// Official download location for this platform: the pinned build, falling
/// back to Resilio's download page when the lock has no URL.
pub fn official_download_url() -> String {
    pinned_download_url(lock_platform())
        .unwrap_or_else(|| "https://www.resilio.com/platforms/desktop/".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keys of the ETI launcher's config.json (known to start Resilio 2.8.1
    /// with `/config`); ours must stay a subset plus `webui`.
    const ETI_CONFIG_KEYS: &[&str] = &[
        "storage_path",
        "pid_file",
        "check_for_updates",
        "use_gui",
        "listening_port",
        "agree_to_EULA",
        "use_upnp",
        "download_limit",
        "upload_limit",
        "rate_limit_local_peers",
        "enable_warning_no_source",
        "lan_encrypt_data",
        "sync_max_time_diff",
        "folder_rescan_interval",
        "peer_expiration_days",
        "folder_defaults.use_relay",
        "folder_defaults.use_tracker",
        "folder_defaults.lan_discovery_mode",
        "service_folders.use_relay",
        "service_folders.use_tracker",
        "sync_trash_ttl",
        "send_statistics",
        "overwrite_changes",
        "prefer_utp2_lan",
        "log_ttl",
        "log_size",
        "enable_file_system_notifications",
        "sync_extended_attributes",
        "webui",
    ];

    #[test]
    fn api_key_is_read_from_eti_client_config() {
        // ETI's file carries `//` comments and tabs; only the key matters.
        let text = "{\n    // path\n   \"storage_path\" : \"C:\\\\x\",\n\t\"use_gui\": false,\n    \"webui\" : {\n       // \"api_key\" : \"OLDKEYOLDKEYOLDKEYOLDKEYOLDKEY\"\n       \"api_key\" : \"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567\"\n    }\n}";
        assert_eq!(
            parse_api_key(text).as_deref(),
            Some("ABCDEFGHIJKLMNOPQRSTUVWXYZ234567")
        );
        assert!(parse_api_key("{\"webui\": {\"listen\": \"127.0.0.1:1\"}}").is_none());
        let dir = tempfile::tempdir().unwrap();
        let sync = dir.path().join("eti").join("LAN Launcher").join("sync");
        std::fs::create_dir_all(&sync).unwrap();
        assert!(eti_client_api_key(&[dir.path().to_path_buf()]).is_none());
        std::fs::write(sync.join("config.json"), text).unwrap();
        let (key, file) = eti_client_api_key(&[dir.path().to_path_buf()]).unwrap();
        assert_eq!(key, "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567");
        assert_eq!(file, sync.join("config.json"));
    }

    #[test]
    fn peer_entries_and_counter_rates() {
        let v: Value = serde_json::from_str(
            r#"{"id":"PEER1","connection":"direct","name":"sync-server","synced":1757980000,"download":1000,"upload":0}"#,
        )
        .unwrap();
        let s = parse_api_peer(&v).expect("peer");
        assert_eq!(s.id, "PEER1");
        assert_eq!(s.peer.name, "sync-server");
        assert_eq!(s.peer.connection.as_deref(), Some("direct"));
        assert!(s.peer.synced);
        assert_eq!((s.down, s.up), (1000, 0));
        // Rates come from two samples; the first one yields nothing.
        let t0 = Instant::now();
        assert_eq!(counter_rate(None, 1000, t0), 0);
        let t1 = t0 + Duration::from_secs(2);
        assert_eq!(counter_rate(Some((1000, t0)), 5000, t1), 2000);
        // Counter reset and samples that are too close yield 0, never a
        // negative or absurd rate.
        assert_eq!(counter_rate(Some((9000, t0)), 10, t1), 0);
        assert_eq!(
            counter_rate(Some((0, t0)), 10_000, t0 + Duration::from_millis(100)),
            0
        );
        assert!(parse_api_peer(&serde_json::json!({})).is_none());
    }

    #[test]
    fn config_uses_only_keys_eti_ships() {
        let mut cfg = ResilioConfig::new(PathBuf::from("/bin/rslsync"), PathBuf::from("/tmp/st"));
        cfg.api_key = Some("KEY".into());
        let json = cfg.to_json();
        for key in json.as_object().unwrap().keys() {
            assert!(
                ETI_CONFIG_KEYS.contains(&key.as_str()),
                "unexpected key {key}"
            );
        }
        // An API key plus our own login: the API stays password-protected.
        assert_eq!(json["webui"]["api_key"], "KEY");
        assert_eq!(json["webui"]["login"], "launcher");
        // Path separators differ per OS; compare the joined path, not a literal.
        assert_eq!(
            json["pid_file"],
            PathBuf::from("/tmp/st")
                .join(PID_FILE)
                .to_string_lossy()
                .as_ref()
        );
        let mut plain = cfg.clone();
        plain.api_key = None;
        assert!(plain.to_json()["webui"].get("api_key").is_none());
    }

    /// The bug this guards against, seen on a Steam Deck: the Diagnose panel
    /// reported 21 "foreign" `rslsync` processes with consecutive pids, which
    /// were the threads of the launcher's own engine. Excluding `our_pid`
    /// cannot help — every thread has an id of its own.
    ///
    /// Linux names a thread after its process unless it says otherwise, and
    /// Rust's named threads do say otherwise, so threads carrying the name of
    /// a sync engine can be made here on purpose. The table is built with
    /// tasks on, which is what `scan_processes` now turns off, so that this
    /// tests the filter rather than the scan.
    #[cfg(target_os = "linux")]
    #[test]
    fn threads_named_like_a_sync_engine_are_not_taken_for_processes() {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut threads = Vec::new();
        for _ in 0..4 {
            let stop = stop.clone();
            threads.push(
                std::thread::Builder::new()
                    // What the engine is called on Linux, and short enough for
                    // the 15 characters a thread name may have.
                    .name("rslsync".into())
                    .spawn(move || {
                        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                    })
                    .expect("thread"),
            );
        }
        // Let the names reach the kernel before the table is read.
        std::thread::sleep(std::time::Duration::from_millis(200));

        let mut sys = sysinfo::System::new();
        sys.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            sysinfo::ProcessRefreshKind::nothing(),
        );
        let with_threads = sys
            .processes()
            .values()
            .filter(|p| is_sync_engine(p.name()))
            .count();
        let without_threads = real_processes(&sys)
            .filter(|(_, p)| is_sync_engine(p.name()))
            .count();

        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for t in threads {
            let _ = t.join();
        }

        // Counted as a difference, not as an absolute: a machine that really
        // is running Resilio (a developer with the launcher open) has engines
        // of its own, and those must still be counted.
        assert!(
            with_threads >= without_threads + 4,
            "the four threads just made should be in the raw table and gone from the \
             filtered one, but it went from {with_threads} to {without_threads}"
        );
    }

    /// Nothing that comes out of the filter is a thread, and this process
    /// appears exactly once rather than once per thread it happens to run.
    ///
    /// Note what is *not* asserted: that the raw scan holds no threads.
    /// `ProcessRefreshKind::without_tasks` was measured to remove only some of
    /// them (108 down to 67 on one machine), so the filter is what carries
    /// this, and a test that assumed otherwise would be testing a wish.
    #[test]
    fn the_filtered_table_holds_processes_only() {
        let sys = scan_processes();
        assert!(real_processes(&sys).all(|(_, p)| p.thread_kind().is_none()));
        let me = std::process::id();
        assert_eq!(
            real_processes(&sys)
                .filter(|(pid, _)| pid.as_u32() == me)
                .count(),
            1
        );
    }

    #[test]
    fn foreign_instances_excludes_our_own_pid() {
        let exe = std::env::current_exe().unwrap();
        let me = std::process::id();
        assert!(!foreign_instances(&exe, Some(me)).contains(&me));
        assert!(
            foreign_instances(&exe, None).contains(&me),
            "the test process itself runs this executable"
        );
    }

    #[test]
    fn windows_candidates_cover_eti_path_and_other_profiles() {
        let env = WinEnv {
            program_files: vec![
                PathBuf::from(r"C:\Program Files"),
                PathBuf::from(r"C:\Program Files (x86)"),
            ],
            local_app_data: Some(PathBuf::from(r"C:\Users\admin\AppData\Local")),
            roaming_app_data: Some(PathBuf::from(r"C:\Users\admin\AppData\Roaming")),
            path: vec![PathBuf::from(r"C:\Tools"), PathBuf::from(r"C:\Tools")],
            user_profiles: vec![PathBuf::from(r"C:\Users\player")],
            registry_install_dirs: vec![PathBuf::from(r"D:\Apps\Resilio Sync")],
        };
        // Path::join uses the host separator; compare separator-agnostic.
        let s: Vec<String> = windows_candidates(&env)
            .iter()
            .map(|p| super::super::normalise_dir(p))
            .collect();
        let has = |x: &str| s.iter().any(|c| c.eq_ignore_ascii_case(x));
        // The per-user installer's default (Roaming) comes first.
        assert!(s[0]
            .eq_ignore_ascii_case("C:/Users/admin/AppData/Roaming/Resilio Sync/Resilio Sync.exe"));
        assert!(has(
            "C:/Users/admin/AppData/Local/Resilio Sync/Resilio Sync.exe"
        ));
        assert!(has(
            "C:/Users/player/AppData/Roaming/Resilio Sync/Resilio Sync.exe"
        ));
        assert!(has("C:/Program Files/eti/lan launcher/btsync.exe"));
        assert!(has("C:/Tools/rslsync.exe"));
        assert!(has(
            "C:/Users/player/AppData/Local/Resilio Sync/Resilio Sync.exe"
        ));
        assert!(has("D:/Apps/Resilio Sync/Resilio Sync.exe"));
        // duplicate PATH entry is probed once
        assert_eq!(
            s.iter()
                .filter(|x| x.eq_ignore_ascii_case("C:/Tools/btsync.exe"))
                .count(),
            1
        );
        // regular install beats the ETI copy
        let eti = s.iter().position(|x| x.contains("lan launcher")).unwrap();
        let pf = s
            .iter()
            .position(|x| x.eq_ignore_ascii_case("C:/Program Files/Resilio Sync/Resilio Sync.exe"))
            .unwrap();
        assert!(pf < eti);
    }

    #[test]
    fn parses_reg_query_output() {
        let out = "\r\nHKEY_CURRENT_USER\\Software\\...\\Resilio Sync\r\n    InstallLocation    REG_SZ    C:\\Users\\player\\AppData\\Local\\Resilio Sync\\\r\n\r\n";
        assert_eq!(
            parse_reg_query_output(out),
            Some(PathBuf::from(r"C:\Users\player\AppData\Local\Resilio Sync"))
        );
        assert_eq!(
            parse_reg_query_output(
                "ERROR: The system was unable to find the specified registry key or value."
            ),
            None
        );
    }

    #[test]
    fn locate_binary_prefers_the_override() {
        let tmp = tempfile::tempdir().unwrap();
        let custom = tmp.path().join("btsync.exe");
        std::fs::write(&custom, "x").unwrap();
        let r = locate_binary_detailed(Some(&custom), None, tmp.path());
        assert_eq!(r.found.as_deref(), Some(custom.as_path()));
        assert_eq!(r.probed[0], custom);
        let r = locate_binary_detailed(Some(&tmp.path().join("missing.exe")), None, tmp.path());
        assert!(r.probed.len() > 1);
    }

    #[test]
    fn download_url_comes_from_the_lock_file() {
        // `url: null` is a legitimate intermediate state (release.yml refuses to
        // build then), so only pinned entries are checked here.
        for p in ["windows", "osx", "linux-x64", "linux-arm64"] {
            if let Some(url) = pinned_download_url(p) {
                assert!(
                    url.starts_with("https://download-cdn.resilio.com/"),
                    "{url}"
                );
                assert!(!url.contains("/stable/"), "{p} must pin a build, got {url}");
            }
        }
        assert!(official_download_url().starts_with("https://"));
        assert!(pinned_download_url("amiga").is_none());
    }

    #[test]
    fn config_json_is_lan_safe() {
        let cfg = ResilioConfig::new(PathBuf::from("/bin/rslsync"), PathBuf::from("/tmp/nll"));
        let json = cfg.to_json();
        assert_eq!(json["webui"]["listen"], "127.0.0.1:0");
        assert_eq!(json["sync_max_time_diff"], 172800);
        assert_eq!(json["folder_defaults.lan_discovery_mode"], 3);
        assert_eq!(json["folder_defaults.use_tracker"], false);
        assert_eq!(cfg.password.len(), 24);
        assert!(json.get("api_key").is_none());
    }

    #[test]
    fn parses_gui_token_html() {
        let html = "<html><div id='token' style='display:none;'>ABC123_-</div></html>";
        assert_eq!(parse_gui_token(html).as_deref(), Some("ABC123_-"));
        assert!(parse_gui_token("<html></html>").is_none());
    }

    #[test]
    fn parses_gui_folders_defensively() {
        let v = json!({"folders":[
            {"name":"/lan/quake3","size":"910000000","files":3,"status":"Synced","peers":[{},{}]},
            {"path":"/lan/cod4","size":5000,"files":2,"progress":99,"peers":[{}],"down":1000},
            {"name":"/lan/paused","size":10,"paused":true},
            {"name":"/lan/idx","size":10,"status":"Indexing..."},
            {"name":"/lan/quiet","size":5000,"files":2,"peers":[{}]}
        ]});
        let f = parse_gui_folders(&v);
        assert_eq!(super::super::peer_summary(f.values()).catalog, None);
        let q = &f[Path::new("/lan/quake3")];
        assert_eq!(q.state, ShareState::Complete);
        assert_eq!(q.peers, 2);
        assert_eq!(q.bytes_total, 910_000_000);
        let c = &f[Path::new("/lan/cod4")];
        assert_eq!(c.state, ShareState::Downloading);
        // 99 % arrived, nothing finished: the last percent is the file that
        // would be verified.
        assert_eq!((c.bytes_done, c.bytes_received), (0, 4950));
        assert_eq!(f[Path::new("/lan/paused")].state, ShareState::Paused);
        assert_eq!(f[Path::new("/lan/idx")].state, ShareState::Indexing);
        // A share the UI says nothing countable about: the zero below is
        // ours, and saying so keeps the installer from reading it as "not a
        // byte has arrived" for the whole download.
        let quiet = &f[Path::new("/lan/quiet")];
        assert_eq!(quiet.state, ShareState::Downloading);
        assert_eq!((quiet.bytes_done, quiet.bytes_known), (0, false));
        // The finished share's figure is one the engine gave; the one at
        // 99 % has a progress bar but no answer to "how much is finished".
        assert!(q.finished_known, "synced: its size is what it holds");
        assert!(
            c.bytes_known && !c.finished_known,
            "99 % is progress, and no answer to what is finished"
        );
    }

    #[test]
    fn gui_folders_report_catalog_peers() {
        let v = json!({"folders":[
            {"name":"/lan/quake3","size":"1","files":1,"status":"Synced","peers":[{},{}]},
            {"name":"/lan/eti_launcher","size":"1","files":1,"status":"Synced","peers":[{}]}
        ]});
        let s = super::super::peer_summary(parse_gui_folders(&v).values());
        // Two peers on one share and one on the other: at most two PCs, never
        // three. Summing is what turned one sync server into "28 participants".
        assert_eq!((s.total, s.catalog), (2, Some(1)));
    }

    #[test]
    fn parses_api_folder() {
        let f = json!({"dir":"/lan/x","secret":"B..","size":100,"type":"read_only","files":1,"error":0,"indexing":0});
        let s = parse_api_folder(&f, 2);
        assert_eq!(s.state, ShareState::Downloading);
        assert_eq!(s.peers, 2);
        let f = json!({"dir":"/lan/x","error":3});
        assert_eq!(parse_api_folder(&f, 0).state, ShareState::Error);
    }

    /// Tiny HTTP server that answers with canned JSON per query string.
    async fn mock_server(routes: Vec<(&'static str, &'static str)>) -> String {
        let with_status = routes.into_iter().map(|(n, b)| (n, 200u16, b)).collect();
        mock_server_full(with_status, None, None).await
    }

    /// Mock HTTP server for the client tests.
    ///
    /// `set_cookie` is handed out with `token.html`, `require_cookie` makes
    /// every `/gui/` request without that cookie fail with HTTP 400 — which is
    /// how Resilio's own web UI behaves.
    async fn mock_server_full(
        routes: Vec<(&'static str, u16, &'static str)>,
        set_cookie: Option<&'static str>,
        require_cookie: Option<&'static str>,
    ) -> String {
        mock_server_recording(routes, set_cookie, require_cookie)
            .await
            .0
    }

    /// As [`mock_server_full`], plus the request lines it has served, so a
    /// test can assert which calls were made and how often.
    async fn mock_server_recording(
        routes: Vec<(&'static str, u16, &'static str)>,
        set_cookie: Option<&'static str>,
        require_cookie: Option<&'static str>,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let routes = routes.clone();
                let log = log.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let line = req.lines().next().unwrap_or("").to_string();
                    if let Ok(mut l) = log.lock() {
                        l.push(line.clone());
                    }
                    let (mut status, mut body) = routes
                        .iter()
                        .find(|(needle, _, _)| line.contains(needle))
                        .map(|(_, s, b)| (*s, *b))
                        .unwrap_or((200, r#"{"error":404}"#));
                    if let Some(cookie) = require_cookie {
                        // reqwest writes the header name in lower case.
                        let has = req.lines().any(|l| {
                            l.to_ascii_lowercase().starts_with("cookie:") && l.contains(cookie)
                        });
                        if line.contains("/gui/?") && !has {
                            status = 400;
                            body = "cookie missing";
                        }
                    }
                    let cookie_header = match (set_cookie, line.contains("token.html")) {
                        (Some(c), true) => format!("Set-Cookie: {c}\r\n"),
                        _ => String::new(),
                    };
                    let resp = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{cookie_header}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}"), seen)
    }

    #[tokio::test]
    async fn api_client_talks_to_documented_endpoints() {
        let base = mock_server(vec![
            ("method=get_version", r#"{"version":"2.7.3"}"#),
            ("method=add_folder", r#"{"error":0}"#),
            ("method=set_folder_prefs", r#"{"error":0}"#),
            ("method=get_folders", r#"[{"dir":"/lan/quake3","secret":"BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","size":10,"files":1,"error":0,"indexing":0}]"#),
            ("method=get_folder_peers", r#"[{"id":"x"}]"#),
        ])
        .await;
        let c = ResilioClient::new(base, "u", "p", Some("KEY".into()));
        assert_eq!(c.version().await.unwrap(), "2.7.3");
        let key = ShareKey::parse("BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").unwrap();
        c.add_folder(&key, Path::new("/lan/quake3"), true)
            .await
            .unwrap();
        let f = c.folders().await.unwrap();
        assert_eq!(f[Path::new("/lan/quake3")].peers, 1);
    }

    #[tokio::test]
    async fn a_download_counts_what_the_peers_sent_not_the_finished_files() {
        // `size` is 8 — the `version.ini` beside the package — while 5 GB of
        // the package itself have arrived. Reading `size` as the progress
        // showed "8 B of 150.9 GB" at full speed for hours.
        let base = mock_server(vec![
            ("method=get_folders", r#"[{"dir":"/lan/siege","secret":"BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","size":8,"total_size":150900000000,"files":1,"total_files":2,"error":0,"indexing":0,"down_speed":229991773}]"#),
            ("method=get_folder_peers", r#"[{"id":"a","name":"sync-server-2","download":3000000000,"upload":0},{"id":"b","name":"pc-2","download":2000000000,"upload":17}]"#),
        ])
        .await;
        let c = ResilioClient::new(base, "u", "p", Some("KEY".into()));
        let f = c.folders().await.unwrap();
        let siege = &f[Path::new("/lan/siege")];
        assert_eq!(siege.bytes_received, 5_000_000_000, "what the peers sent");
        assert_eq!(siege.bytes_done, 8, "what the engine calls finished");
        assert_eq!(siege.bytes_total, 150_900_000_000);
        assert_eq!(siege.peers, 2);
        assert_eq!(siege.download_bps, 229_991_773);
        // An engine whose peer entries carry no counters cannot be counted
        // with: the figure is then not the engine's, and the share says so
        // instead of claiming that nothing has arrived.
        let base = mock_server(vec![
            ("method=get_folders", r#"[{"dir":"/lan/q","secret":"BAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","size":8,"total_size":1000,"files":1,"total_files":2,"error":0,"indexing":0}]"#),
            ("method=get_folder_peers", r#"[{"id":"a","name":"pc-2"}]"#),
        ])
        .await;
        let c = ResilioClient::new(base, "u", "p", Some("KEY".into()));
        let f = c.folders().await.unwrap();
        let q = &f[Path::new("/lan/q")];
        assert_eq!(
            (q.bytes_received, q.bytes_known),
            (8, true),
            "the finished files, the only figure such an engine gives"
        );
        assert_eq!(q.peers, 1, "it is still a peer");

        // An engine without `total_size` says what it holds and nothing
        // about the share: "unknown", not "this is all of it" — otherwise
        // every finished file reads as the share changing size.
        let older = parse_api_folder(
            &json!({"dir":"/lan/g","size":500,"files":1,"total_files":2,"error":0,"indexing":0}),
            1,
        );
        assert_eq!((older.bytes_done, older.bytes_total), (500, 0));
        assert_eq!(older.state, ShareState::Downloading);

        // The web UI answers with a percentage over the whole share, the
        // file in flight included: that is progress, and no answer to what is
        // finished — so the disk decides when to verify, as it did before
        // this launcher ever asked the engine.
        let ui = parse_gui_folders(&json!({"folders":[
            {"name":"/lan/siege","size":"150900000000","files":2,"progress":1,"peers":[{}]}
        ]}));
        let siege_ui = &ui[Path::new("/lan/siege")];
        assert_eq!(siege_ui.bytes_received, 1_509_000_000);
        assert!(siege_ui.bytes_known && !siege_ui.finished_known);

        // The bookkeeping behind it, on the readings that break a simpler
        // rule: a counter that starts again, a share fetched a second time,
        // and a tick without an answer from the engine.
        let peer = |id: &str, down: u64| PeerSample {
            id: id.to_string(),
            peer: SharePeer {
                name: id.to_string(),
                connection: None,
                synced: false,
                download_bps: 0,
                upload_bps: 0,
            },
            down,
            up: 0,
            counted: true,
        };
        let mut t = Transfer::default();
        // First sight: the engine's session began with this download.
        assert_eq!(t.step(1000, 0, &[peer("a", 300)]), 300);
        assert_eq!(t.step(1000, 0, &[peer("a", 500)]), 500);
        // A second peer joins with a counter of its own.
        assert_eq!(t.step(1000, 0, &[peer("a", 500), peer("b", 100)]), 600);
        // One drops out of the list and comes back with its total: only what
        // it sent since counts, not its whole session again.
        assert_eq!(t.step(1000, 0, &[peer("a", 600)]), 700);
        assert_eq!(t.step(1000, 0, &[peer("a", 600), peer("b", 150)]), 750);
        // A peer reconnects and starts at zero: nothing un-arrives.
        assert_eq!(t.step(1000, 0, &[peer("a", 0), peer("b", 150)]), 750);
        assert_eq!(t.step(1000, 0, &[peer("a", 50), peer("b", 150)]), 800);
        // Finished, and nothing above the share's own size.
        assert_eq!(t.step(1000, 1000, &[peer("a", 400)]), 1000);
        // An update: the share was complete and is given another size, and
        // Resilio keeps the old files until the new package is whole — so
        // nothing falls back, and that is the only sign that a second
        // transfer has begun. The counters carry on, this transfer does not,
        // and the old package's bytes are not counted as the new one's.
        assert_eq!(t.step(2000, 1000, &[peer("a", 420)]), 0);
        assert_eq!(t.step(2000, 1000, &[peer("a", 450)]), 30);
        assert_eq!(t.step(2000, 1000, &[peer("a", 500)]), 80);
        // An update that was already running when the launcher started: its
        // finished files are the package from before, nearly the whole size.
        // They do not enter the figure at all, so nothing has to recognise
        // the case.
        let mut running = Transfer::default();
        assert_eq!(
            running.step(1000, 950, &[peer("a", 300)]),
            300,
            "what the peers sent, not the package from before"
        );
        assert_eq!(running.step(1000, 950, &[peer("a", 400)]), 400);

        // A reading without a size (an engine without `total_size`) does not
        // make a share forget how big it is: the update after it is still
        // recognised.
        let mut u = Transfer::default();
        assert_eq!(u.step(1000, 0, &[peer("a", 100)]), 100);
        assert_eq!(u.step(1000, 1000, &[peer("a", 900)]), 900);
        assert_eq!(u.step(0, 1000, &[peer("a", 900)]), 900, "no size, no news");
        assert_eq!(u.step(2000, 1000, &[peer("a", 950)]), 0, "still an update");
        // A tick the engine did not answer keeps the figure.
        assert_eq!(t.arrived(2000, 1000), 80);
        // A share whose size the engine does not know yet is not capped.
        let mut unknown = Transfer::default();
        assert_eq!(unknown.step(0, 0, &[peer("a", 2000)]), 2000);
    }

    #[tokio::test]
    async fn api_error_codes_are_surfaced() {
        let base = mock_server(vec![(
            "method=remove_folder",
            r#"{"error":3,"message":"Folder is not known"}"#,
        )])
        .await;
        let c = ResilioClient::new(base, "u", "p", Some("KEY".into()));
        let err = c.remove_folder(Path::new("/x"), None).await.unwrap_err();
        assert!(err.to_string().contains("error 3"));
    }

    #[tokio::test]
    async fn gui_client_fetches_token_then_folders() {
        let base = mock_server(vec![
            (
                "/gui/token.html",
                "<div id='token' style='display:none;'>TOK</div>",
            ),
            (
                "action=getsyncfolders",
                r#"{"folders":[{"name":"/lan/a","size":5,"status":"Synced","peers":[]}]}"#,
            ),
            ("action=getversion", r#"{"version":"2.8.1"}"#),
        ])
        .await;
        let c = ResilioClient::new(base, "u", "p", None);
        assert_eq!(c.version().await.unwrap(), "2.8.1");
        let f = c.folders().await.unwrap();
        assert_eq!(f[Path::new("/lan/a")].state, ShareState::Complete);
    }

    #[tokio::test]
    async fn gui_client_keeps_the_session_cookie_and_survives_a_refused_getversion() {
        // A launcher without an API key only has the web UI: it hands out a
        // session cookie with the token and rejects a token that comes back
        // without it, and some builds answer `getversion` with HTTP 400.
        let base = mock_server_full(
            vec![
                (
                    "/gui/token.html",
                    200,
                    "<div id='token' style='display:none;'>TOK</div>",
                ),
                ("action=getversion", 400, "invalid request"),
                (
                    "action=getsyncfolders",
                    200,
                    r#"{"folders":[{"name":"/lan/a","size":5,"status":"Synced","peers":[]}]}"#,
                ),
            ],
            Some("GUID=abc; path=/"),
            Some("GUID=abc"),
        )
        .await;
        let c = ResilioClient::new(base, "u", "p", None);
        // Reachable although the version stays unknown.
        assert_eq!(c.version().await.unwrap(), "?");
        let f = c.folders().await.unwrap();
        assert_eq!(f[Path::new("/lan/a")].state, ShareState::Complete);
    }

    #[test]
    fn the_api_folder_entry_is_read_the_way_the_engine_writes_it() {
        // Verbatim shape from a 2.8.1 engine's own log.
        let catalog: Value = serde_json::from_str(
            r#"{"dir":"\\\\?\\E:\\LAN\\eti_launcher","down_speed":2500000,"error":0,
                 "files":169,"indexing":0,"paused":0,"secret":"[secret]","size":1213071231,
                 "total_files":170,"total_size":1262951709,"type":"read_only","up_speed":0}"#,
        )
        .unwrap();
        let s = parse_api_folder(&catalog, 1);
        assert_eq!(s.bytes_done, 1_213_071_231);
        assert_eq!(s.bytes_total, 1_262_951_709);
        assert_eq!(s.files_total, 170);
        assert_eq!(s.download_bps, 2_500_000);
        assert_eq!(s.state, ShareState::Downloading);
        assert_eq!(s.dir, PathBuf::from(r"E:\LAN\eti_launcher"));

        // A share nobody has announced anything for: not "complete", and not
        // a total of zero bytes to divide by either.
        let empty: Value = serde_json::from_str(
            r#"{"dir":"E:\\LAN\\bfbc2","files":0,"size":0,"total_files":0,"total_size":0,
                 "error":0,"indexing":0}"#,
        )
        .unwrap();
        assert_eq!(parse_api_folder(&empty, 0).state, ShareState::Pending);

        // Fully synced.
        let done: Value = serde_json::from_str(
            r#"{"dir":"E:\\LAN\\cnc4","files":4,"size":8617278764,"total_files":4,
                 "total_size":8617278764,"error":0,"indexing":0}"#,
        )
        .unwrap();
        let s = parse_api_folder(&done, 2);
        assert_eq!(s.state, ShareState::Complete);
        assert_eq!(s.bytes_done, s.bytes_total);

        // Every byte of the files it knows, but not every file of the share:
        // that is "still downloading", not "done".
        let partial: Value = serde_json::from_str(
            r#"{"dir":"E:\\LAN\\cnc4","files":1,"size":500,"total_files":4,
                 "total_size":500,"error":0,"indexing":0}"#,
        )
        .unwrap();
        assert_eq!(parse_api_folder(&partial, 1).state, ShareState::Downloading);
    }

    #[test]
    fn already_added_is_recognised_by_code_not_by_substring() {
        let err = |m: &str| Error::Transport(m.to_string());
        // What the documented API answers for a folder it already has.
        assert!(is_already_added(&err(
            "API add_folder: error 200 Der ausgewählte Ordner wurde bereits zu Resilio Sync hinzugefügt."
        )));
        assert!(is_already_added(&err("API add_folder: error 5 exists")));
        // Not a prefix match: 500 and 55 are different failures.
        assert!(!is_already_added(&err("API add_folder: error 500 nope")));
        assert!(!is_already_added(&err("API add_folder: error 55 nope")));
        assert!(!is_already_added(&err("no code at all")));
    }

    #[tokio::test]
    async fn an_added_folder_is_recognised_in_both_answer_shapes() {
        let secret = "BICDWADB4KCVNR6FCAGYTHEKZBYVUGTZX";
        let dir = Path::new("/lan/eti_launcher");
        // The API's top-level array …
        let api: Value = serde_json::from_str(&format!(
            r#"[{{"dir":"/lan/eti_launcher","secret":"{secret}"}}]"#
        ))
        .unwrap();
        assert!(folder_has_secret(&api, dir, secret));
        assert!(!folder_has_secret(&api, dir, "BOTHERKEY"));
        // … and the web UI's wrapper.
        let gui: Value = serde_json::from_str(&format!(
            r#"{{"folders":[{{"name":"/lan/eti_launcher","secret":"{secret}"}}]}}"#
        ))
        .unwrap();
        assert!(folder_has_secret(&gui, dir, secret));
    }

    #[tokio::test]
    async fn a_folder_the_engine_already_has_is_not_an_error() {
        // Resilio keeps its folders between launcher starts and answers the
        // second `addsyncfolder` with HTTP 500. The catalog share is fine,
        // so this must not surface as "cannot register catalog share".
        let base = mock_server_full(
            vec![
                (
                    "/gui/token.html",
                    200,
                    "<div id='token' style='display:none;'>TOK</div>",
                ),
                ("action=addsyncfolder", 500, "ERROR"),
                (
                    "action=getsyncfolders",
                    200,
                    r#"{"folders":[{"name":"/lan/eti_launcher","secret":"BICDWADB4KCVNR6FCAGYTHEKZBYVUGTZX","size":5,"status":"Synced","peers":[]}]}"#,
                ),
            ],
            None,
            None,
        )
        .await;
        let c = ResilioClient::new(base, "u", "p", None);
        let key = ShareKey::parse("BICDWADB4KCVNR6FCAGYTHEKZBYVUGTZX").unwrap();
        c.add_folder(&key, Path::new("/lan/eti_launcher"), true)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_folder_held_under_a_different_key_stays_an_error() {
        // Changing settings.catalogKey must not look like success while the
        // engine keeps syncing the old share.
        let base = mock_server_full(
            vec![
                (
                    "/gui/token.html",
                    200,
                    "<div id='token' style='display:none;'>TOK</div>",
                ),
                ("action=addsyncfolder", 500, "ERROR"),
                (
                    "action=getsyncfolders",
                    200,
                    r#"{"folders":[{"name":"/lan/eti_launcher","secret":"AWORXFGHDQZ3ZYLDBFKJVQ2XNGYVLGOOM","size":5,"status":"Synced","peers":[]}]}"#,
                ),
            ],
            None,
            None,
        )
        .await;
        let c = ResilioClient::new(base, "u", "p", None);
        let key = ShareKey::parse("BICDWADB4KCVNR6FCAGYTHEKZBYVUGTZX").unwrap();
        assert!(c
            .add_folder(&key, Path::new("/lan/eti_launcher"), true)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn a_folder_the_engine_does_not_have_stays_an_error() {
        let base = mock_server_full(
            vec![
                (
                    "/gui/token.html",
                    200,
                    "<div id='token' style='display:none;'>TOK</div>",
                ),
                ("action=addsyncfolder", 500, "ERROR"),
                ("action=getsyncfolders", 200, r#"{"folders":[]}"#),
            ],
            None,
            None,
        )
        .await;
        let c = ResilioClient::new(base, "u", "p", None);
        let key = ShareKey::parse("BICDWADB4KCVNR6FCAGYTHEKZBYVUGTZX").unwrap();
        assert!(c
            .add_folder(&key, Path::new("/lan/eti_launcher"), true)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn a_refused_getversion_is_asked_once_and_drops_the_stale_token() {
        let (base, seen) = mock_server_recording(
            vec![
                (
                    "/gui/token.html",
                    200,
                    "<div id='token' style='display:none;'>TOK</div>",
                ),
                ("action=getversion", 400, "invalid request"),
                ("action=getsyncfolders", 200, r#"{"folders":[]}"#),
            ],
            None,
            None,
        )
        .await;
        let c = ResilioClient::new(base, "u", "p", None);
        assert_eq!(c.version().await.unwrap(), "?");
        assert_eq!(c.version().await.unwrap(), "?");
        let lines = seen.lock().unwrap().clone();
        let count = |needle: &str| lines.iter().filter(|l| l.contains(needle)).count();
        // The build has no `getversion`; asking twice would double every probe.
        assert_eq!(count("action=getversion"), 1, "{lines:?}");
        // The 400 drops the cached token, so the session is fetched again
        // instead of every later call failing the same way.
        assert_eq!(count("token.html"), 2, "{lines:?}");
    }

    #[test]
    fn command_args_reference_config() {
        let args = ResilioTransport::command_args(Path::new("/tmp/config.json"));
        assert!(args.iter().any(|a| a.contains("config.json")));
        if cfg!(target_os = "windows") {
            assert_eq!(
                args[0], "/noinstall",
                "a fresh copy must not install itself"
            );
        }
    }

    #[test]
    fn bundled_binary_ranks_before_data_dir_and_system() {
        let dir = tempfile::tempdir().unwrap();
        let res = dir.path().join("res");
        let data = dir.path().join("data");
        let probed = locate_binary_detailed(None, Some(&res), &data).probed;
        let pos = |needle: &str| {
            probed
                .iter()
                .position(|p| super::super::normalise_dir(p).contains(needle))
                .unwrap_or_else(|| panic!("{needle} not probed: {probed:?}"))
        };
        assert!(pos("res/resilio/") < pos("data/resilio/"));
        // The lock file's `install` name is probed in the bundled folder.
        let install = bundled_install_name().expect("lock has install name");
        assert!(probed
            .iter()
            .any(|p| p.starts_with(&res) && p.ends_with(&install)));
        // An override always comes first.
        let with_override =
            locate_binary_detailed(Some(Path::new("/opt/x/rslsync")), Some(&res), &data).probed;
        assert_eq!(with_override[0], Path::new("/opt/x/rslsync"));
    }
}
