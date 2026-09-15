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
use std::sync::Mutex;
use std::time::Duration;
use tokio::process::{Child, Command};

pub const DEFAULT_LAN_PORT: u16 = 8889;
/// Pid file inside the storage dir, named as in ETI's config; written by the
/// engine (`pid_file`) and read by orphan cleanup.
pub const PID_FILE: &str = "rslsync.pid";

/// Resilio API key the ETI LAN Launcher ships in its `config.json` to every
/// client (`%ProgramFiles%\eti\LAN Launcher\sync\config.json`). It enables
/// the documented `/api` surface; the engine config mirrors ETI's file, which
/// is known to start Resilio 2.8.1 on the same machines.
pub const ETI_API_KEY: &str = "REDACTED-RESILIO-API-KEY";
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
            api_key: Some(ETI_API_KEY.into()),
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

/// Minimal HTTP client for the Resilio API surfaces.
#[derive(Debug, Clone)]
pub struct ResilioClient {
    pub base: String,
    pub login: String,
    pub password: String,
    pub api_key: Option<String>,
    http: reqwest::Client,
    gui_token: Arc<Mutex<Option<String>>>,
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
                .build()
                .expect("reqwest client"),
            gui_token: Arc::new(Mutex::new(None)),
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
        if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 {
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
        let v = self.gui("getversion", &[]).await?;
        Ok(v.get("version")
            .map(|x| x.to_string().trim_matches('"').to_string())
            .unwrap_or("?".into()))
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
            match self.api("add_folder", &params).await {
                Ok(_) => {}
                // error 5 = folder already added; treat as success
                Err(Error::Transport(m)) if m.contains("error 5") => {}
                Err(e) => return Err(e),
            }
            let mut p2 = vec![("dir", dir_s.as_str()), ("secret", key.expose())];
            p2.extend(folder_prefs(lan_only));
            let _ = self.api("set_folder_prefs", &p2).await;
            Ok(())
        } else {
            self.gui(
                "addsyncfolder",
                &[
                    ("name", dir_s.as_str()),
                    ("secret", key.expose()),
                    ("selectivesync", "0"),
                ],
            )
            .await
            .map(|_| ())
        }
    }

    pub async fn remove_folder(&self, dir: &Path, key: Option<&ShareKey>) -> Result<()> {
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

    /// All folders known to the engine, keyed by directory.
    pub async fn folders(&self) -> Result<HashMap<PathBuf, ShareStatus>> {
        if self.has_api_key() {
            let v = self.api("get_folders", &[]).await?;
            let mut out = HashMap::new();
            for f in v.as_array().cloned().unwrap_or_default() {
                let dir = PathBuf::from(f.get("dir").and_then(Value::as_str).unwrap_or(""));
                let peers = match self
                    .api(
                        "get_folder_peers",
                        &[(
                            "secret",
                            f.get("secret").and_then(Value::as_str).unwrap_or(""),
                        )],
                    )
                    .await
                {
                    Ok(p) => p.as_array().map(|a| a.len() as u32).unwrap_or(0),
                    Err(_) => 0,
                };
                let status = parse_api_folder(&f, peers);
                out.insert(dir, status);
            }
            Ok(out)
        } else {
            let v = self.gui("getsyncfolders", &[("discovery", "1")]).await?;
            Ok(parse_gui_folders(&v))
        }
    }
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

/// `get_folders` entry: `{dir, secret, size, type, files, error, indexing}`.
/// The documented API gives no per-folder progress, so `bytes_done` is only
/// known when `error == 0 && !indexing` and no peers report pending data; we
/// report `Complete` only when the engine has no error and nothing is indexing.
pub fn parse_api_folder(f: &Value, peers: u32) -> ShareStatus {
    let size = f.get("size").and_then(Value::as_u64).unwrap_or(0);
    let files = f.get("files").and_then(Value::as_u64).unwrap_or(0);
    let error = f.get("error").and_then(Value::as_i64).unwrap_or(0);
    let indexing = f.get("indexing").and_then(Value::as_i64).unwrap_or(0) != 0;
    let state = if error != 0 {
        ShareState::Error
    } else if indexing {
        ShareState::Indexing
    } else if size == 0 {
        ShareState::Pending
    } else {
        ShareState::Downloading
    };
    ShareStatus {
        dir: PathBuf::from(f.get("dir").and_then(Value::as_str).unwrap_or("")),
        state,
        bytes_done: 0,
        bytes_total: size,
        files_total: files,
        peers,
        download_bps: 0,
        upload_bps: 0,
        error: (error != 0).then(|| format!("resilio error {error}")),
    }
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
        let path = f
            .get("path")
            .or_else(|| f.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("")
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
        let (state, done) = if paused {
            (ShareState::Paused, 0)
        } else if error.is_some() {
            (ShareState::Error, 0)
        } else if status_text.contains("index") {
            (ShareState::Indexing, 0)
        } else if let Some(p) = progress {
            if p >= 100 {
                (ShareState::Complete, size)
            } else {
                (ShareState::Downloading, size * p / 100)
            }
        } else if status_text.contains("synced") || status_text.contains("up to date") {
            (ShareState::Complete, size)
        } else if size == 0 {
            (ShareState::Pending, 0)
        } else {
            (ShareState::Downloading, 0)
        };
        out.insert(
            PathBuf::from(&path),
            ShareStatus {
                dir: PathBuf::from(&path),
                state,
                bytes_done: done,
                bytes_total: size,
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
        for (pid, proc_) in sys.processes() {
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
    for (pid, p) in sys.processes() {
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
        let _ = tokio::time::timeout(Duration::from_secs(5), self.client.shutdown()).await;
        let child = self.child.lock().ok().and_then(|mut c| c.take());
        if let Some(mut child) = child {
            match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
                Ok(_) => {}
                Err(_) => {
                    let _ = child.kill().await;
                }
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
        let summary = self
            .client
            .folders()
            .await
            .ok()
            .map(|f| super::peer_summary(f.values()));
        TransportHealth {
            kind: TransportKind::Resilio,
            running,
            api_reachable: version.is_some(),
            version,
            peers: summary.map(|s| s.total).unwrap_or(0),
            catalog_peers: summary.and_then(|s| s.catalog).unwrap_or(0),
            server_found: summary.and_then(|s| s.catalog).map(|n| n > 0),
            lan_mode: *self.lan_only.lock().unwrap_or_else(|e| e.into_inner()),
            detail: None,
        }
    }

    async fn add_share(&self, key: &ShareKey, dir: &Path, opts: &ShareOptions) -> Result<()> {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        self.client.add_folder(key, dir, opts.lan_only).await?;
        if let Ok(mut k) = self.keys.lock() {
            k.insert(dir.to_path_buf(), key.clone());
        }
        Ok(())
    }

    async fn remove_share(&self, dir: &Path) -> Result<()> {
        let key = self.keys.lock().ok().and_then(|k| k.get(dir).cloned());
        self.client.remove_folder(dir, key.as_ref()).await?;
        if let Ok(mut k) = self.keys.lock() {
            k.remove(dir);
        }
        Ok(())
    }

    async fn set_paused(&self, dir: &Path, paused: bool) -> Result<()> {
        let dir_s = dir.to_string_lossy().to_string();
        if self.client.has_api_key() {
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
        }
    }

    async fn share_status(&self, dir: &Path) -> Result<Option<ShareStatus>> {
        let folders = self.client.folders().await?;
        Ok(folders.get(dir).cloned().or_else(|| {
            folders
                .values()
                .find(|s| normalise_dir(&s.dir) == normalise_dir(dir))
                .cloned()
        }))
    }

    async fn list_shares(&self) -> Result<Vec<ShareStatus>> {
        Ok(self.client.folders().await?.into_values().collect())
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
    pub fn from_process() -> Self {
        let mut env = WinEnv::default();
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(v) = std::env::var_os(var).filter(|v| !v.is_empty()) {
                env.program_files.push(PathBuf::from(v));
            }
        }
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
            std::process::Command::new("reg")
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

/// Process table refreshed with exactly the fields the caller reads.
pub fn scan_processes_with(kind: sysinfo::ProcessRefreshKind) -> sysinfo::System {
    let mut sys = sysinfo::System::new();
    sys.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, kind);
    sys
}

/// Executables of sync engines that are running right now (any user), a
/// reliable hint where an installation lives.
pub fn running_binaries() -> Vec<PathBuf> {
    let sys = scan_processes();
    let mut out: Vec<PathBuf> = sys
        .processes()
        .values()
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
    let found = candidates.iter().find(|p| p.is_file()).cloned();
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
    fn config_uses_only_keys_eti_ships() {
        let cfg = ResilioConfig::new(PathBuf::from("/bin/rslsync"), PathBuf::from("/tmp/st"));
        let json = cfg.to_json();
        for key in json.as_object().unwrap().keys() {
            assert!(
                ETI_CONFIG_KEYS.contains(&key.as_str()),
                "unexpected key {key}"
            );
        }
        // ETI's API key plus our own login: the API stays password-protected.
        assert_eq!(json["webui"]["api_key"], ETI_API_KEY);
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
            user_profiles: vec![PathBuf::from(r"C:\Users\schim")],
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
            "C:/Users/schim/AppData/Roaming/Resilio Sync/Resilio Sync.exe"
        ));
        assert!(has("C:/Program Files/eti/lan launcher/btsync.exe"));
        assert!(has("C:/Tools/rslsync.exe"));
        assert!(has(
            "C:/Users/schim/AppData/Local/Resilio Sync/Resilio Sync.exe"
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
        let out = "\r\nHKEY_CURRENT_USER\\Software\\...\\Resilio Sync\r\n    InstallLocation    REG_SZ    C:\\Users\\schim\\AppData\\Local\\Resilio Sync\\\r\n\r\n";
        assert_eq!(
            parse_reg_query_output(out),
            Some(PathBuf::from(r"C:\Users\schim\AppData\Local\Resilio Sync"))
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
            {"name":"/lan/idx","size":10,"status":"Indexing..."}
        ]});
        let f = parse_gui_folders(&v);
        assert_eq!(super::super::peer_summary(f.values()).catalog, None);
        let q = &f[Path::new("/lan/quake3")];
        assert_eq!(q.state, ShareState::Complete);
        assert_eq!(q.peers, 2);
        assert_eq!(q.bytes_total, 910_000_000);
        let c = &f[Path::new("/lan/cod4")];
        assert_eq!(c.state, ShareState::Downloading);
        assert_eq!(c.bytes_done, 4950);
        assert_eq!(f[Path::new("/lan/paused")].state, ShareState::Paused);
        assert_eq!(f[Path::new("/lan/idx")].state, ShareState::Indexing);
    }

    #[test]
    fn gui_folders_report_catalog_peers() {
        let v = json!({"folders":[
            {"name":"/lan/quake3","size":"1","files":1,"status":"Synced","peers":[{},{}]},
            {"name":"/lan/eti_launcher","size":"1","files":1,"status":"Synced","peers":[{}]}
        ]});
        let s = super::super::peer_summary(parse_gui_folders(&v).values());
        assert_eq!((s.total, s.catalog), (3, Some(1)));
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
                    let mut buf = vec![0u8; 8192];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let line = req.lines().next().unwrap_or("").to_string();
                    let body = routes
                        .iter()
                        .find(|(needle, _)| line.contains(needle))
                        .map(|(_, b)| *b)
                        .unwrap_or(r#"{"error":404}"#);
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}")
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
