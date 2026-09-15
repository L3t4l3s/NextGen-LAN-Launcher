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
    pub device_name: String,
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
            device_name: format!(
                "nll-{}",
                sysinfo::System::host_name().unwrap_or_else(|| "pc".into())
            ),
            lan_only: true,
            upload_limit_kbs: 0,
        }
    }

    /// Resilio `config.json` contents.
    pub fn to_json(&self) -> Value {
        let mut webui = json!({
            "listen": format!("127.0.0.1:{}", self.api_port),
            "login": self.login,
            "password": self.password,
        });
        if let Some(k) = &self.api_key {
            webui["api_key"] = json!(k);
        }
        let mut v = json!({
            "device_name": self.device_name,
            "storage_path": self.storage_dir.to_string_lossy(),
            "pid_file": self.storage_dir.join("sync.pid").to_string_lossy(),
            "listening_port": self.listening_port,
            "use_gui": false,
            "check_for_updates": false,
            "agree_to_EULA": "yes",
            "use_upnp": !self.lan_only,
            "download_limit": 0,
            "upload_limit": self.upload_limit_kbs,
            "rate_limit_local_peers": false,
            "lan_encrypt_data": false,
            "lan_use_tcp": true,
            // 48h: a wrong clock must never silently stall a LAN sync.
            "sync_max_time_diff": 172800,
            "folder_defaults.lan_discovery_mode": 3,
            "folder_defaults.use_lan_broadcast": true,
            "folder_defaults.use_tracker": !self.lan_only,
            "folder_defaults.use_relay": !self.lan_only,
            "folder_defaults.use_dht": !self.lan_only,
            "folder_defaults.delete_to_trash": false,
            "folder_defaults.use_sync_trash": false,
            "send_statistics": false,
            "sync_trash_ttl": 1,
            "webui": webui,
        });
        if self.lan_only {
            v["disable_tracker"] = json!(true);
        }
        v
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
        let pid_file = self.config.storage_dir.join("sync.pid");
        let pid_from_file: Option<u32> = std::fs::read_to_string(&pid_file)
            .ok()
            .and_then(|s| s.trim().parse().ok());
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        for (pid, proc_) in sys.processes() {
            let name = proc_.name().to_string_lossy().to_string();
            let from_our_storage = proc_.cmd().iter().any(|a| {
                a.to_string_lossy()
                    .contains(&*self.config.storage_dir.to_string_lossy())
            });
            let is_ours = pid_from_file == Some(pid.as_u32()) || from_our_storage;
            if is_ours
                && process_names().iter().any(|n| name.eq_ignore_ascii_case(n))
                && proc_.kill()
            {
                killed += 1;
            }
        }
        let _ = std::fs::remove_file(&pid_file);
        killed
    }

    pub fn command_args(config_path: &Path) -> Vec<String> {
        if cfg!(target_os = "windows") {
            vec![
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
            if self.client.version().await.is_ok() {
                return Ok(());
            }
            if start.elapsed() > timeout {
                return Err(Error::Transport(
                    "Resilio API did not become reachable".into(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
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
        let mut cmd = Command::new(&self.config.binary);
        cmd.args(Self::command_args(&self.config_path))
            .current_dir(&self.config.storage_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
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

/// Where to find the Resilio Sync binary: bundled resource, previous
/// download, or a system-wide installation.
pub fn locate_binary(resource_dir: Option<&Path>, data_dir: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let names: &[&str] = if cfg!(target_os = "windows") {
        &["Resilio Sync.exe", "rslsync.exe"]
    } else if cfg!(target_os = "macos") {
        &[
            "Resilio Sync.app/Contents/MacOS/Resilio Sync",
            "Resilio Sync",
            "rslsync",
        ]
    } else {
        &["rslsync"]
    };
    for base in [
        resource_dir.map(|r| r.join("resilio")),
        Some(data_dir.join("resilio")),
    ]
    .into_iter()
    .flatten()
    {
        for n in names {
            candidates.push(base.join(n));
        }
    }
    if cfg!(target_os = "windows") {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            candidates.push(
                Path::new(&local)
                    .join("Resilio Sync")
                    .join("Resilio Sync.exe"),
            );
        }
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Ok(pf) = std::env::var(var) {
                candidates.push(Path::new(&pf).join("Resilio Sync").join("Resilio Sync.exe"));
            }
        }
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
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Windows only: the release bundles Resilio's installer
/// (`Resilio-Sync_x64.exe`). When no installed engine is found, run it
/// silently into the launcher's data dir and return the resulting binary.
/// Untested on real hardware; see docs/ARCHITECTURE.md.
pub async fn install_bundled_windows(
    resource_dir: Option<&Path>,
    data_dir: &Path,
) -> Result<Option<PathBuf>> {
    if !cfg!(target_os = "windows") {
        return Ok(None);
    }
    let Some(installer) = resource_dir
        .map(|r| r.join("resilio").join("Resilio-Sync_x64.exe"))
        .filter(|p| p.is_file())
    else {
        return Ok(None);
    };
    let target = data_dir.join("resilio");
    std::fs::create_dir_all(&target).map_err(|e| Error::io(&target, e))?;
    let mut cmd = tokio::process::Command::new(&installer);
    cmd.arg("/S");
    // NSIS `/D=` must be the last argument and must not be quoted, even when
    // the path contains spaces; std would add quotes, so pass it raw.
    #[cfg(windows)]
    cmd.raw_arg(format!("/D={}", target.display()));
    #[cfg(not(windows))]
    cmd.arg(format!("/D={}", target.display()));
    let status = cmd
        .status()
        .await
        .map_err(|e| Error::Transport(format!("Resilio installer: {e}")))?;
    if !status.success() {
        return Err(Error::Transport(format!(
            "Resilio installer exited with {status}"
        )));
    }
    Ok(locate_binary(resource_dir, data_dir))
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
    }
}
