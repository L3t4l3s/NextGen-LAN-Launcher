//! Environment checks with actionable results.
//!
//! Every check yields a [`Problem`] (or nothing when healthy). The UI renders
//! them as a traffic light plus plain-language steps and, where possible, a
//! "Fix now" button that runs the attached [`FixAction`].

use crate::library::Library;
use crate::problem::{FixAction, Problem, Severity};
use crate::transport::{ShareState, ShareStatus, TransportHealth};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Missing data is not itself a network fault while a healthy catalog share
/// is connecting, syncing or waiting for the loader. Never invent a percent.
pub fn catalog_pending_problem(connected: bool, share: Option<&ShareStatus>) -> Problem {
    if connected
        && !share.is_some_and(|s| matches!(s.state, ShareState::Paused | ShareState::Error))
    {
        let state = match share.map(|s| s.state) {
            Some(ShareState::Downloading) => "downloading",
            Some(ShareState::Indexing) => "indexing",
            Some(ShareState::Complete) => "reading",
            _ => "connecting",
        };
        let mut problem = Problem::new("catalog.loading", Severity::Info).param("state", state);
        if let Some(s) = share.filter(|s| s.bytes_known && s.bytes_total > 0) {
            problem = problem.param("progress", format!("{:.0}", s.progress() * 100.0));
        }
        problem.step("catalog.loading.step.wait")
    } else {
        Problem::new("catalog.missing", Severity::Warning)
            .step("catalog.missing.step.wait")
            .step("catalog.missing.step.peers")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub problems: Vec<Problem>,
    /// Problems the user has hidden (see [`Problem::dismiss_key`]). They are
    /// kept out of `problems` so they colour no traffic light and raise no
    /// badge, but the page can still list them and offer to show them again.
    #[serde(default)]
    pub ignored: Vec<Problem>,
    pub checks_run: Vec<String>,
    pub generated_at: chrono::DateTime<chrono::Utc>,
}

impl Report {
    pub fn new(problems: Vec<Problem>, checks_run: Vec<String>) -> Self {
        Self {
            problems,
            ignored: Vec::new(),
            checks_run,
            generated_at: chrono::Utc::now(),
        }
    }

    /// Move every problem the user has hidden into `ignored`.
    pub fn hide_ignored(mut self, is_ignored: impl Fn(&str) -> bool) -> Self {
        let (ignored, rest) = self
            .problems
            .into_iter()
            .partition(|p| p.dismiss_key.as_deref().is_some_and(&is_ignored));
        self.problems = rest;
        self.ignored = ignored;
        self
    }

    pub fn worst(&self) -> Option<Severity> {
        self.problems
            .iter()
            .map(|p| p.severity)
            .max_by_key(|s| match s {
                Severity::Info => 0,
                Severity::Warning => 1,
                Severity::Error => 2,
            })
    }
}

/// Windows network profile of an adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkProfile {
    pub interface_index: u32,
    pub interface_alias: String,
    pub name: String,
    /// `Public`, `Private`, `DomainAuthenticated`.
    pub category: String,
    /// Best of `IPv4Connectivity`/`IPv6Connectivity`: 0 Disconnected, 1 NoTraffic,
    /// 2 Subnet, 3 LocalNetwork, 4 Internet. `None` when the query did not report it.
    #[serde(default)]
    pub connectivity: Option<u8>,
}

impl NetworkProfile {
    pub fn is_public(&self) -> bool {
        self.category.eq_ignore_ascii_case("Public")
    }

    /// `Private` or `DomainAuthenticated`: Windows allows discovery and inbound
    /// connections. Anything else (`Public`, `Unknown`) is not trusted.
    pub fn is_trusted(&self) -> bool {
        self.category.eq_ignore_ascii_case("Private")
            || self.category.eq_ignore_ascii_case("DomainAuthenticated")
    }

    /// Adapters without traffic (an idle second NIC, a docking station, a
    /// virtual switch) cannot be the LAN link; their profile is irrelevant.
    /// Unknown connectivity counts as active.
    pub fn carries_traffic(&self) -> bool {
        self.connectivity.is_none_or(|c| c >= 2)
    }

    fn is_connected(&self) -> bool {
        self.connectivity.is_none_or(|c| c >= 1)
    }
}

fn connectivity_level(v: &serde_json::Value) -> Option<u8> {
    match v {
        serde_json::Value::Number(n) => n.as_u64().map(|n| n.min(4) as u8),
        serde_json::Value::String(s) => match s.as_str() {
            "Disconnected" => Some(0),
            "NoTraffic" => Some(1),
            "Subnet" => Some(2),
            "LocalNetwork" => Some(3),
            "Internet" => Some(4),
            _ => None,
        },
        _ => None,
    }
}

/// Parse `Get-NetConnectionProfile | ConvertTo-Json` output. Accepts a single
/// object or an array; category may be numeric (0 = Public, 1 = Private,
/// 2 = Domain) or a string.
pub fn parse_net_profiles(json: &str) -> Vec<NetworkProfile> {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let items: Vec<serde_json::Value> = match v {
        serde_json::Value::Array(a) => a,
        other => vec![other],
    };
    items
        .into_iter()
        .filter_map(|it| {
            let cat = match it.get("NetworkCategory")? {
                serde_json::Value::Number(n) => match n.as_i64()? {
                    0 => "Public".to_string(),
                    1 => "Private".to_string(),
                    2 => "DomainAuthenticated".to_string(),
                    _ => "Unknown".to_string(),
                },
                serde_json::Value::String(s) => s.clone(),
                _ => return None,
            };
            Some(NetworkProfile {
                interface_index: it
                    .get("InterfaceIndex")
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0) as u32,
                interface_alias: it
                    .get("InterfaceAlias")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                name: it
                    .get("Name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                category: cat,
                connectivity: ["IPv4Connectivity", "IPv6Connectivity"]
                    .iter()
                    .filter_map(|k| it.get(k).and_then(connectivity_level))
                    .max(),
            })
        })
        .collect()
}

/// Problems for adapters on the "Public" profile. The public profile blocks
/// inbound connections and multicast discovery, which is exactly why Resilio
/// transfers failed for every Windows player at the LAN.
///
/// Only adapters that carry traffic count. When another active adapter is
/// already private or domain-joined, the LAN most likely runs over that one
/// and the public adapter is reported as a warning instead of an error.
pub fn check_network_profiles(profiles: &[NetworkProfile]) -> Vec<Problem> {
    let mut active: Vec<&NetworkProfile> =
        profiles.iter().filter(|p| p.carries_traffic()).collect();
    if active.is_empty() {
        // A LAN party without internet or DHCP can leave the only NIC at
        // "NoTraffic"; the check must not go silent in exactly that case.
        active = profiles.iter().filter(|p| p.is_connected()).collect();
    }
    let trusted = active.iter().find(|p| p.is_trusted());
    active
        .iter()
        .filter(|p| p.is_public())
        .map(|p| {
            let (code, severity) = match trusted {
                Some(_) => ("network.public_profile_secondary", Severity::Warning),
                None => ("network.public_profile", Severity::Error),
            };
            let mut problem = Problem::new(code, severity)
                .param("adapter", &p.interface_alias)
                .param("network", &p.name)
                .step("network.public_profile.step.fix")
                .step("network.public_profile.step.manual")
                .with_fix(FixAction::SetNetworkProfilePrivate {
                    interface_index: p.interface_index,
                });
            if let Some(t) = trusted {
                problem = problem.param("trusted_adapter", &t.interface_alias);
                // Only the secondary case: when no other adapter is trusted,
                // this is the LAN link itself and hiding it would hide the
                // reason nothing syncs. Keyed per adapter, because the next
                // one is a different decision.
                problem = problem.dismissible(format!("{code}:{}", p.interface_alias));
            }
            problem
        })
        .collect()
}

/// PowerShell to query profiles (run with `-NoProfile -NonInteractive`).
pub const PS_GET_PROFILES: &str = "Get-NetConnectionProfile | Select-Object InterfaceIndex,InterfaceAlias,Name,NetworkCategory,IPv4Connectivity,IPv6Connectivity | ConvertTo-Json -Compress";

/// PowerShell to switch an adapter to the private profile (needs elevation).
pub fn ps_set_private(interface_index: u32) -> String {
    format!("Set-NetConnectionProfile -InterfaceIndex {interface_index} -NetworkCategory Private")
}

/// Name of the inbound rule `firewall_rules` creates; `netsh … show rule
/// name=…` with it tells whether the rules exist.
pub const FIREWALL_RULE_IN: &str = "NextGen LAN Launcher Sync (in)";

/// Problem for a sync engine without Windows firewall rules: other players
/// cannot connect to this PC, LAN discovery answers may be dropped.
pub fn firewall_missing_problem(program: &std::path::Path) -> Problem {
    Problem::new("transport.firewall_missing", Severity::Warning)
        .param("program", program.display().to_string())
        .step("transport.firewall_missing.step.fix")
        // On a domain-joined PC group policy can forbid local rules, and the
        // fix then fails with a message from netsh that means nothing to a
        // player. Saying so up front is cheaper than a support call.
        .step("transport.firewall_missing.step.domain")
        .with_fix(FixAction::AddFirewallRules)
        // A managed PC may never get these rules; the warning is then
        // permanent and the user has no way to act on it.
        .dismissible("transport.firewall_missing")
}

/// `netsh` commands that drop every existing rule for the sync engine.
///
/// Windows keeps a block rule when a user once declined its firewall prompt
/// for a program, and a block rule wins over every allow rule, so the allow
/// rules from [`firewall_rules`] alone would not restore the transfers. These
/// commands must run before them.
///
/// `netsh` exits with 1 when no rule matches, which is the normal case on a
/// clean machine. Callers therefore ignore the exit status of these commands
/// and only require the ones from [`firewall_rules`] to succeed.
pub fn firewall_stale_rules(program: &Path) -> Vec<Vec<String>> {
    let prog = program.to_string_lossy().to_string();
    ["in", "out"]
        .iter()
        .map(|dir| {
            vec![
                "advfirewall".to_string(),
                "firewall".to_string(),
                "delete".to_string(),
                "rule".to_string(),
                "name=all".to_string(),
                format!("dir={dir}"),
                format!("program={prog}"),
            ]
        })
        .collect()
}

/// `netsh` commands that allow the sync engine on every profile, so a later
/// flip back to "Public" does not silently break transfers again.
///
/// Run [`firewall_stale_rules`] first; an old block rule would otherwise
/// override all of these.
pub fn firewall_rules(program: &Path, listening_port: u16) -> Vec<Vec<String>> {
    let prog = program.to_string_lossy().to_string();
    let mut rules: Vec<Vec<String>> = Vec::from([
        vec![
            "advfirewall",
            "firewall",
            "add",
            "rule",
            &format!("name={FIREWALL_RULE_IN}"),
            "dir=in",
            "action=allow",
            &format!("program={prog}"),
            "profile=any",
            "enable=yes",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        vec![
            "advfirewall",
            "firewall",
            "add",
            "rule",
            "name=NextGen LAN Launcher Sync (out)",
            "dir=out",
            "action=allow",
            &format!("program={prog}"),
            "profile=any",
            "enable=yes",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    ]);
    if listening_port > 0 {
        for proto in ["TCP", "UDP"] {
            rules.push(
                vec![
                    "advfirewall",
                    "firewall",
                    "add",
                    "rule",
                    &format!("name=NextGen LAN Launcher Sync {proto} {listening_port}"),
                    "dir=in",
                    "action=allow",
                    &format!("protocol={proto}"),
                    &format!("localport={listening_port}"),
                    "profile=any",
                    "enable=yes",
                ]
                .into_iter()
                .map(String::from)
                .collect(),
            );
        }
    }
    rules
}

pub fn check_disk_space(library: &Library, min_free_bytes: u64) -> Vec<Problem> {
    let mut out = Vec::new();
    for root in &library.roots {
        match crate::library::disk_space(&root.path) {
            Some((free, total)) => {
                if free < min_free_bytes {
                    out.push(
                        Problem::new("disk.low_space", Severity::Warning)
                            .param("path", root.path.display())
                            .param("free_bytes", free)
                            .param("total_bytes", total)
                            .step("disk.low_space.step.free")
                            .step("disk.low_space.step.add_root")
                            .with_fix(FixAction::OpenFolder {
                                path: root.path.to_string_lossy().to_string(),
                            }),
                    );
                }
            }
            None => out.push(
                Problem::new("disk.root_missing", Severity::Error)
                    .param("path", root.path.display())
                    .step("disk.root_missing.step.connect")
                    .step("disk.root_missing.step.remove"),
            ),
        }
    }
    if library.roots.is_empty() {
        out.push(Problem::new("library.no_root", Severity::Error).step("library.no_root.step.add"));
    }
    out
}

pub fn check_transport(health: &TransportHealth) -> Vec<Problem> {
    let mut out = Vec::new();
    match health.kind {
        crate::transport::TransportKind::Resilio => {
            if !health.running {
                out.push(
                    Problem::new("transport.not_running", Severity::Error)
                        .step("transport.not_running.step.restart")
                        .with_fix(FixAction::RestartTransport),
                );
            } else if !health.api_reachable {
                out.push(
                    Problem::new("transport.api_unreachable", Severity::Error)
                        .step("transport.api_unreachable.step.restart")
                        .step("transport.api_unreachable.step.antivirus")
                        .with_fix(FixAction::RestartTransport),
                );
            } else if let Some(activity) = health.activity {
                let state = match activity {
                    crate::transport::TransportActivity::Discovering => "discovering",
                    crate::transport::TransportActivity::Indexing => "indexing",
                };
                out.push(Problem::new("transport.preparing", Severity::Info).param("state", state));
            } else if health.peers == 0 {
                out.push(
                    Problem::new("transport.no_peers", Severity::Warning)
                        .param("detail", health.detail.clone().unwrap_or_default())
                        .step("transport.no_peers.step.server")
                        .step("transport.no_peers.step.network")
                        .step("transport.no_peers.step.webui")
                        .with_fix(FixAction::RestartTransport),
                );
            } else if health.server_found == Some(false) {
                // Other players are connected, but nobody serves the catalog
                // share, so the sync server itself is missing.
                out.push(
                    Problem::new("transport.no_server", Severity::Warning)
                        .param("detail", health.detail.clone().unwrap_or_default())
                        .step("transport.no_server.step.server")
                        .step("transport.no_server.step.wait")
                        .with_fix(FixAction::RestartTransport),
                );
            }
        }
        crate::transport::TransportKind::Folder => {
            out.push(
                Problem::new("transport.folder_mode", Severity::Info)
                    .step("transport.folder_mode.step.resilio"),
            );
        }
        crate::transport::TransportKind::Demo => {
            out.push(Problem::new("transport.demo_mode", Severity::Info));
        }
    }
    out
}

/// Warn when the clock differs from the LANPage server by more than 10 min.
/// Only informational: at the LAN this was never the actual cause.
pub fn check_clock(server_time: Option<chrono::DateTime<chrono::Utc>>) -> Vec<Problem> {
    let Some(server) = server_time else {
        return Vec::new();
    };
    let skew = (chrono::Utc::now() - server).num_seconds().abs();
    if skew > 600 {
        vec![Problem::new("clock.skew", Severity::Warning)
            .param("minutes", skew / 60)
            .step("clock.skew.step.sync")]
    } else {
        Vec::new()
    }
}

/// Resilio's LAN discovery port (multicast and broadcast, UDP).
pub const RESILIO_DISCOVERY_PORT: u16 = 3838;

/// One zone from `firewall-cmd --list-all-zones`, as far as the check needs it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FirewalldZone {
    pub name: String,
    pub active: bool,
    pub default: bool,
    /// `default`, `ACCEPT`, `DROP`, `%%REJECT%%`.
    pub target: String,
    /// Entries like `1025-65535/udp` or `3838/udp`, from `ports:` and from
    /// rich rules that accept a port.
    pub ports: Vec<String>,
}

/// The zones in the output of `firewall-cmd --list-all-zones`: a header line
/// per zone (`public (default, active)`), then indented `key: value` lines;
/// rich rules follow `rich rules:`, one per line, indented further.
///
/// Services are not resolved into ports (that is one more `firewall-cmd` per
/// service); a zone that opens the engine through a service of its own is
/// reported anyway, and the warning can be dismissed.
pub fn parse_firewalld_zones(listing: &str) -> Vec<FirewalldZone> {
    let mut zones: Vec<FirewalldZone> = Vec::new();
    for line in listing.lines() {
        if let Some(port) = rich_rule_port(line) {
            if let Some(zone) = zones.last_mut() {
                zone.ports.push(port);
            }
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(char::is_whitespace) {
            let (name, flags) = match line.split_once('(') {
                Some((name, flags)) => (name.trim(), flags),
                None => (line.trim(), ""),
            };
            zones.push(FirewalldZone {
                name: name.to_string(),
                active: flags.contains("active"),
                default: flags.contains("default"),
                ..Default::default()
            });
            continue;
        }
        let Some(zone) = zones.last_mut() else {
            continue;
        };
        if let Some((key, value)) = line.trim().split_once(':') {
            match key.trim() {
                "target" => zone.target = value.trim().to_string(),
                "ports" => zone.ports = value.split_whitespace().map(String::from).collect(),
                _ => {}
            }
        }
    }
    zones
}

/// `rule family="ipv4" port port="3838" protocol="udp" accept` → `3838/udp`.
fn rich_rule_port(line: &str) -> Option<String> {
    let line = line.trim();
    if !line.starts_with("rule ") || !line.ends_with("accept") {
        return None;
    }
    let value = |key: &str| {
        let start = line.find(&format!(" {key}=\""))? + key.len() + 3;
        let end = line[start..].find('"')?;
        Some(line[start..start + end].to_string())
    };
    Some(format!("{}/{}", value("port")?, value("protocol")?))
}

/// Whether `zone` lets in `protocol` traffic on every port of `from..=to`.
fn zone_admits(zone: &FirewalldZone, protocol: &str, from: u16, to: u16) -> bool {
    if zone.target.eq_ignore_ascii_case("ACCEPT") {
        return true;
    }
    zone.ports.iter().any(|entry| {
        let Some((range, proto)) = entry.split_once('/') else {
            return false;
        };
        let (low, high) = range.split_once('-').unwrap_or((range, range));
        match (low.parse::<u16>(), high.parse::<u16>()) {
            (Ok(low), Ok(high)) => proto == protocol && low <= from && to <= high,
            _ => false,
        }
    })
}

/// What an active firewalld zone keeps out of the sync engine.
///
/// SteamOS runs no firewall; Fedora-based systems such as Bazzite run
/// firewalld, and a zone that closes the engine's port leaves only the
/// connections this machine opens itself — fewer peers, slower downloads,
/// nothing passed on to others — and the LAN search goes unanswered.
///
/// `listening_port` 0 means the engine picked one at random; then only a zone
/// that admits every unprivileged port (as Fedora Workstation's does) is
/// known to be open, and the advice is to fix the port in the settings.
pub fn check_firewalld(zones: &[FirewalldZone], listening_port: u16) -> Vec<Problem> {
    // Interfaces fall into the default zone when none is active.
    let mut relevant: Vec<&FirewalldZone> = zones.iter().filter(|z| z.active).collect();
    if relevant.is_empty() {
        relevant = zones.iter().filter(|z| z.default).collect();
    }
    let mut needed = vec![("udp", RESILIO_DISCOVERY_PORT, RESILIO_DISCOVERY_PORT)];
    match listening_port {
        0 => needed.extend([("tcp", 1025, u16::MAX), ("udp", 1025, u16::MAX)]),
        port => needed.extend([("tcp", port, port), ("udp", port, port)]),
    }
    let mut out = Vec::new();
    for zone in relevant {
        let missing: Vec<_> = needed
            .iter()
            .filter(|(proto, from, to)| !zone_admits(zone, proto, *from, *to))
            .collect();
        if missing.is_empty() {
            continue;
        }
        let describe = |(proto, from, to): &&(&str, u16, u16)| {
            if from == to {
                format!("{from}/{proto}")
            } else {
                format!("{from}-{to}/{proto}")
            }
        };
        // Only single ports go into the command: a random sync port has no
        // number to open, and the whole unprivileged range is not advice.
        let mut open: Vec<String> = Vec::new();
        for (proto, from, to) in &missing {
            let port = format!("{from}/{proto}");
            if from == to && !open.contains(&port) {
                open.push(port);
            }
        }
        let mut problem = Problem::new("transport.firewalld_closed", Severity::Warning)
            .param("zone", &zone.name)
            .param(
                "missing",
                missing.iter().map(describe).collect::<Vec<_>>().join(", "),
            );
        if listening_port == 0 {
            problem = problem.step("transport.firewalld_closed.step.port");
        }
        if !open.is_empty() {
            let command = format!(
                "sudo firewall-cmd --permanent --zone={} {} && sudo firewall-cmd --reload",
                zone.name,
                open.iter()
                    .map(|p| format!("--add-port={p}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            problem = problem
                .param("command", command)
                .step("transport.firewalld_closed.step.command");
        }
        out.push(problem.dismissible(format!("transport.firewalld_closed:{}", zone.name)));
    }
    out
}

/// An AppImage whose runtime found no FUSE on this machine unpacks itself
/// into a temporary folder on every start instead of mounting (the type-2
/// runtime does that on its own and says so on a terminal nobody sees).
/// It works, but each start unpacks hundreds of megabytes first — on a
/// system with `/tmp` in memory, into memory. A start with
/// `APPIMAGE_EXTRACT_AND_RUN=1` chose this and is not told about it; the
/// `--appimage-extract-and-run` flag leaves no trace the launcher could see,
/// which is why the hint can be dismissed.
pub fn check_appimage_unpacked(
    appimage: Option<&str>,
    appdir: Option<&str>,
    chosen: Option<&str>,
) -> Vec<Problem> {
    let unpacked = appimage.is_some()
        && appdir.is_some_and(|dir| {
            Path::new(dir)
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("appimage_extracted_"))
        });
    // The runtime only asks whether the variable is set, not for a value.
    if !unpacked || chosen.is_some() {
        return Vec::new();
    }
    vec![Problem::new("appimage.no_fuse", Severity::Info)
        .step("appimage.no_fuse.step.install")
        .dismissible("appimage.no_fuse")]
}

/// Orphaned sync engine processes not started by this launcher instance.
pub fn check_orphans(our_pid: Option<u32>) -> Vec<Problem> {
    let sys = crate::transport::resilio::scan_processes();
    let strangers: Vec<String> = crate::transport::resilio::real_processes(&sys)
        .filter(|(pid, p)| {
            Some(pid.as_u32()) != our_pid && crate::transport::resilio::is_sync_engine(p.name())
        })
        .map(|(pid, p)| format!("{} ({})", p.name().to_string_lossy(), pid.as_u32()))
        .collect();
    if strangers.is_empty() {
        Vec::new()
    } else {
        vec![
            Problem::new("transport.foreign_instance", Severity::Warning)
                .param("processes", strangers.join(", "))
                .step("transport.foreign_instance.step.close")
                .with_fix(FixAction::RestartTransport),
        ]
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn catalog_loading_is_informational_but_offline_or_paused_needs_action() {
        let connecting = catalog_pending_problem(true, None);
        assert_eq!(connecting.code, "catalog.loading");
        assert_eq!(connecting.severity, Severity::Info);
        assert!(!connecting.params.contains_key("progress"));
        let mut share = ShareStatus {
            dir: "LAN/eti_launcher".into(),
            state: ShareState::Downloading,
            bytes_done: 8,
            bytes_total: 1000,
            bytes_received: 430,
            bytes_known: true,
            finished_known: true,
            files_total: 2,
            peers: 1,
            download_bps: 100,
            upload_bps: 0,
            error: None,
        };
        let loading = catalog_pending_problem(true, Some(&share));
        assert_eq!(loading.params["progress"], "43");
        assert_eq!(loading.params["state"], "downloading");
        share.bytes_known = false;
        assert!(!catalog_pending_problem(true, Some(&share))
            .params
            .contains_key("progress"));
        for state in [ShareState::Paused, ShareState::Error] {
            share.state = state;
            let problem = catalog_pending_problem(true, Some(&share));
            assert_eq!(problem.code, "catalog.missing");
            assert_eq!(problem.severity, Severity::Warning);
            assert!(!problem.steps.is_empty());
        }
        assert_eq!(
            catalog_pending_problem(false, None).severity,
            Severity::Warning
        );
    }
    use super::*;

    /// The shape `firewall-cmd --list-all-zones` prints, trimmed to three
    /// zones: Fedora Workstation's open one, a closed `public`, `trusted`.
    const ZONES: &str = "FedoraWorkstation (default)
  target: default
  interfaces:
  services: dhcpv6-client mdns samba-client ssh
  ports: 1025-65535/udp 1025-65535/tcp
  protocols:

public (active)
  target: default
  interfaces: enp3s0
  services: dhcpv6-client ssh
  ports: 3838/udp 55000/tcp
  protocols:

trusted
  target: ACCEPT
  interfaces:
  ports:
";

    #[test]
    fn firewalld_zones_are_read_with_their_ports_and_flags() {
        let zones = parse_firewalld_zones(ZONES);
        assert_eq!(zones.len(), 3);
        assert!(zones[0].default && !zones[0].active);
        assert_eq!(zones[0].ports, ["1025-65535/udp", "1025-65535/tcp"]);
        assert!(zones[1].active);
        assert_eq!(zones[2].target, "ACCEPT");
        assert!(zones[2].ports.is_empty());
    }

    #[test]
    fn a_closed_active_zone_names_what_is_missing_and_how_to_open_it() {
        let zones = parse_firewalld_zones(ZONES);
        let problems = check_firewalld(&zones, 55000);
        assert_eq!(problems.len(), 1, "only the active zone counts");
        let p = &problems[0];
        assert_eq!(p.code, "transport.firewalld_closed");
        assert_eq!(p.params["zone"], "public");
        assert_eq!(p.params["missing"], "55000/udp");
        assert_eq!(
            p.params["command"],
            "sudo firewall-cmd --permanent --zone=public --add-port=55000/udp \
             && sudo firewall-cmd --reload",
            "only what is missing, and nothing twice"
        );
        assert!(!p.steps.iter().any(|s| s.ends_with("step.port")));
    }

    #[test]
    fn an_open_zone_or_no_firewall_is_not_a_problem() {
        let open: Vec<_> = parse_firewalld_zones(ZONES)
            .into_iter()
            .map(|mut z| {
                z.active = z.default;
                z
            })
            .collect();
        assert!(
            check_firewalld(&open, 0).is_empty(),
            "Fedora Workstation's range"
        );
        assert!(check_firewalld(&open, 40000).is_empty());
        let trusted = FirewalldZone {
            name: "trusted".into(),
            active: true,
            target: "ACCEPT".into(),
            ..Default::default()
        };
        assert!(check_firewalld(&[trusted], 0).is_empty());
        assert!(check_firewalld(&[], 0).is_empty(), "firewalld not running");
    }

    #[test]
    fn a_random_port_behind_a_closed_zone_asks_for_a_fixed_one() {
        let zones = parse_firewalld_zones(ZONES);
        let problems = check_firewalld(&zones, 0);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].steps, ["transport.firewalld_closed.step.port"]);
        assert!(
            !problems[0].params.contains_key("command"),
            "discovery is open, and a random port has no number to open"
        );

        let closed = FirewalldZone {
            name: "public".into(),
            active: true,
            ..Default::default()
        };
        let problems = check_firewalld(&[closed], 3838);
        assert_eq!(
            problems[0].params["command"],
            "sudo firewall-cmd --permanent --zone=public --add-port=3838/udp \
             --add-port=3838/tcp && sudo firewall-cmd --reload"
        );
    }

    #[test]
    fn a_port_a_rich_rule_accepts_counts_as_open() {
        let listing = "public (active)
  target: default
  ports: 55000/tcp 55000/udp
  rich rules:
\trule family=\"ipv4\" port port=\"3838\" protocol=\"udp\" accept
\trule family=\"ipv4\" source address=\"10.0.0.9\" port port=\"22\" protocol=\"tcp\" reject
";
        let zones = parse_firewalld_zones(listing);
        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0].ports, ["55000/tcp", "55000/udp", "3838/udp"]);
        assert!(check_firewalld(&zones, 55000).is_empty());
    }

    #[test]
    fn only_an_appimage_the_runtime_had_to_unpack_is_reported() {
        let image = Some("/home/deck/NextGen.AppImage");
        let unpacked = Some("/tmp/appimage_extracted_6c58ef22f21d7fce44c5a769fac9bf78");
        assert_eq!(check_appimage_unpacked(image, unpacked, None).len(), 1);
        assert!(check_appimage_unpacked(image, Some("/tmp/.mount_NextGeAbC123"), None).is_empty());
        assert!(
            check_appimage_unpacked(image, unpacked, Some("1")).is_empty(),
            "chosen"
        );
        assert!(check_appimage_unpacked(image, unpacked, Some("yes")).is_empty());
        assert!(
            check_appimage_unpacked(None, None, None).is_empty(),
            "not an AppImage"
        );
    }

    #[test]
    fn parses_powershell_profiles_numeric_and_string() {
        let json = r#"[{"InterfaceIndex":12,"InterfaceAlias":"Ethernet","Name":"Netzwerk 3","NetworkCategory":0},
                       {"InterfaceIndex":5,"InterfaceAlias":"WLAN","Name":"Home","NetworkCategory":"Private"}]"#;
        let p = parse_net_profiles(json);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].category, "Public");
        // Without connectivity data both adapters count as active; the private
        // WLAN makes the public Ethernet a warning rather than an error.
        let problems = check_network_profiles(&p);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, "network.public_profile_secondary");
        assert_eq!(problems[0].severity, Severity::Warning);
        assert_eq!(problems[0].params["adapter"], "Ethernet");
        assert_eq!(
            check_network_profiles(&p[..1])[0].code,
            "network.public_profile"
        );
        assert_eq!(
            problems[0].fix,
            Some(FixAction::SetNetworkProfilePrivate {
                interface_index: 12
            })
        );
        // single object form
        let one = parse_net_profiles(
            r#"{"InterfaceIndex":1,"InterfaceAlias":"E","Name":"N","NetworkCategory":1}"#,
        );
        assert_eq!(one[0].category, "Private");
        assert!(check_network_profiles(&one).is_empty());
    }

    #[test]
    fn idle_public_adapter_next_to_domain_network_is_ignored() {
        // Test system: Ethernet 3 is the domain network in use, Ethernet 6 is
        // plugged in but idle ("Kein Internet") and on the public profile.
        let json = r#"[{"InterfaceIndex":3,"InterfaceAlias":"Ethernet 3","Name":"corp.local","NetworkCategory":2,"IPv4Connectivity":4,"IPv6Connectivity":1},
                       {"InterfaceIndex":6,"InterfaceAlias":"Ethernet 6","Name":"Netzwerk","NetworkCategory":0,"IPv4Connectivity":1,"IPv6Connectivity":0}]"#;
        let p = parse_net_profiles(json);
        assert_eq!(p[0].connectivity, Some(4));
        assert_eq!(p[1].connectivity, Some(1));
        assert!(check_network_profiles(&p).is_empty());

        // The same public adapter with LAN traffic is still worth a warning,
        // but not an error, because the domain adapter is active too.
        let json = json.replace(
            r#""IPv4Connectivity":1,"IPv6Connectivity":0"#,
            r#""IPv4Connectivity":"LocalNetwork","IPv6Connectivity":"NoTraffic""#,
        );
        let problems = check_network_profiles(&parse_net_profiles(&json));
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, "network.public_profile_secondary");
        assert_eq!(problems[0].severity, Severity::Warning);
        assert_eq!(problems[0].params["trusted_adapter"], "Ethernet 3");

        // A lone public adapter at the LAN party (no internet) stays an error,
        // even when Windows only reports "NoTraffic" for it.
        for level in ["3", "1"] {
            let lan = format!(
                r#"{{"InterfaceIndex":4,"InterfaceAlias":"Ethernet","Name":"Netzwerk 2","NetworkCategory":0,"IPv4Connectivity":{level},"IPv6Connectivity":0}}"#
            );
            let problems = check_network_profiles(&parse_net_profiles(&lan));
            assert_eq!(problems.len(), 1, "level {level}");
            assert_eq!(problems[0].code, "network.public_profile");
            assert_eq!(problems[0].severity, Severity::Error);
        }

        // An "Unknown" category never counts as the trusted LAN link.
        let odd = r#"[{"InterfaceIndex":4,"InterfaceAlias":"Ethernet","Name":"N","NetworkCategory":0,"IPv4Connectivity":3},
                      {"InterfaceIndex":9,"InterfaceAlias":"vEthernet","Name":"V","NetworkCategory":7,"IPv4Connectivity":2}]"#;
        let problems = check_network_profiles(&parse_net_profiles(odd));
        assert_eq!(problems[0].code, "network.public_profile");
    }

    #[test]
    fn check_transport_reports_missing_server() {
        let health = |peers: u32, server_found: Option<bool>| TransportHealth {
            activity: None,
            kind: crate::transport::TransportKind::Resilio,
            running: true,
            api_reachable: true,
            version: Some("2.8.1".into()),
            peers,
            catalog_peers: 0,
            server_found,
            lan_mode: true,
            peer_details: false,
            detail: None,
            download_bps: 0,
            upload_bps: 0,
            web_ui: None,
        };
        let codes = |h: &TransportHealth| -> Vec<String> {
            check_transport(h).into_iter().map(|p| p.code).collect()
        };
        assert_eq!(codes(&health(2, Some(false))), vec!["transport.no_server"]);
        assert_eq!(codes(&health(0, Some(false))), vec!["transport.no_peers"]);
        assert!(codes(&health(2, None)).is_empty());
        assert!(codes(&health(2, Some(true))).is_empty());
        let mut preparing = health(0, Some(false));
        preparing.activity = Some(crate::transport::TransportActivity::Discovering);
        let problems = check_transport(&preparing);
        assert_eq!(problems[0].code, "transport.preparing");
        assert_eq!(problems[0].severity, Severity::Info);
        preparing.api_reachable = false;
        assert_eq!(
            check_transport(&preparing)[0].code,
            "transport.api_unreachable"
        );
    }

    #[test]
    fn transport_problems_carry_the_engine_detail() {
        let health = crate::transport::TransportHealth {
            activity: None,
            kind: crate::transport::TransportKind::Resilio,
            running: true,
            api_reachable: true,
            version: Some("2.8.1".into()),
            peers: 0,
            catalog_peers: 0,
            server_found: Some(false),
            lan_mode: true,
            peer_details: false,
            detail: Some("E:\\LAN\\eti_launcher: 0 peers, Indexing".into()),
            download_bps: 0,
            upload_bps: 0,
            web_ui: Some("http://launcher:s3cr3t@127.0.0.1:8888/gui/".into()),
        };
        let problems = check_transport(&health);
        let p = problems
            .iter()
            .find(|p| p.code == "transport.no_peers")
            .expect("no_peers");
        assert_eq!(
            p.params.get("detail").map(String::as_str),
            Some("E:\\LAN\\eti_launcher: 0 peers, Indexing")
        );
        assert_eq!(p.fix, Some(FixAction::RestartTransport));
        let fw = firewall_missing_problem(Path::new(r"C:\App\Resilio Sync.exe"));
        assert_eq!(fw.code, "transport.firewall_missing");
        assert!(matches!(fw.fix, Some(FixAction::AddFirewallRules)));
        assert_eq!(
            fw.params.get("program").map(String::as_str),
            Some(r"C:\App\Resilio Sync.exe")
        );
    }

    #[test]
    fn only_the_secondary_public_adapter_can_be_hidden() {
        let json = r#"[{"InterfaceIndex":5,"InterfaceAlias":"WLAN","Name":"Gast","NetworkCategory":0,"IPv4Connectivity":4},
                       {"InterfaceIndex":7,"InterfaceAlias":"Ethernet","Name":"LAN","NetworkCategory":1,"IPv4Connectivity":2}]"#;
        let problems = check_network_profiles(&parse_net_profiles(json));
        assert_eq!(problems[0].code, "network.public_profile_secondary");
        // Keyed per adapter: hiding the warning for the guest WLAN must not
        // hide it for a second public adapter later.
        assert_eq!(
            problems[0].dismiss_key.as_deref(),
            Some("network.public_profile_secondary:WLAN")
        );

        // The adapter that carries the LAN itself stays visible.
        let only_public = r#"[{"InterfaceIndex":5,"InterfaceAlias":"WLAN","Name":"Gast","NetworkCategory":0,"IPv4Connectivity":4}]"#;
        let problems = check_network_profiles(&parse_net_profiles(only_public));
        assert_eq!(problems[0].code, "network.public_profile");
        assert_eq!(problems[0].dismiss_key, None);
    }

    #[test]
    fn hidden_problems_leave_the_traffic_light() {
        let report = Report::new(
            vec![
                firewall_missing_problem(Path::new(r"C:\App\sync.exe")),
                Problem::new("catalog.missing", Severity::Warning),
            ],
            vec!["firewall".into()],
        )
        .hide_ignored(|k| k == "transport.firewall_missing");
        assert_eq!(report.ignored.len(), 1);
        assert_eq!(report.problems.len(), 1);
        assert_eq!(report.problems[0].code, "catalog.missing");
        // The hidden warning no longer decides the page's colour, but the
        // remaining one still does.
        assert_eq!(report.worst(), Some(Severity::Warning));
    }

    #[test]
    fn firewall_rules_cover_all_profiles() {
        let rules = firewall_rules(Path::new(r"C:\App\rslsync.exe"), 55555);
        assert_eq!(rules.len(), 4);
        assert!(rules.iter().all(|r| r.contains(&"add".to_string())));
        assert!(rules.iter().all(|r| r.contains(&"profile=any".to_string())));
        assert!(rules[2].iter().any(|a| a == "localport=55555"));
    }

    #[test]
    fn stale_rules_are_dropped_for_both_directions() {
        // A block rule from a declined Windows prompt outranks every allow
        // rule, so both directions are cleared before the rules above are set.
        let stale = firewall_stale_rules(Path::new(r"C:\App\rslsync.exe"));
        assert_eq!(stale.len(), 2);
        assert!(stale
            .iter()
            .all(|r| r.contains(&"delete".to_string()) && r.contains(&"name=all".to_string())));
        assert!(stale[0].contains(&"dir=in".to_string()));
        assert!(stale[1].contains(&"dir=out".to_string()));
        assert!(stale
            .iter()
            .all(|r| r.contains(&r"program=C:\App\rslsync.exe".to_string())));
    }

    #[test]
    fn clock_skew_only_warns_when_large() {
        assert!(check_clock(Some(chrono::Utc::now())).is_empty());
        let old = chrono::Utc::now() - chrono::Duration::hours(3);
        let p = check_clock(Some(old));
        assert_eq!(p[0].code, "clock.skew");
        assert_eq!(p[0].params["minutes"], "180");
    }

    #[test]
    fn empty_library_is_an_error() {
        let p = check_disk_space(&Library::default(), 0);
        assert_eq!(p[0].code, "library.no_root");
    }
}
