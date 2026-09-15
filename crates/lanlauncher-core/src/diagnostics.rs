//! Environment checks with actionable results.
//!
//! Every check yields a [`Problem`] (or nothing when healthy). The UI renders
//! them as a traffic light plus plain-language steps and, where possible, a
//! "Fix now" button that runs the attached [`FixAction`].

use crate::library::Library;
use crate::problem::{FixAction, Problem, Severity};
use crate::transport::TransportHealth;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub problems: Vec<Problem>,
    pub checks_run: Vec<String>,
    pub generated_at: chrono::DateTime<chrono::Utc>,
}

impl Report {
    pub fn new(problems: Vec<Problem>, checks_run: Vec<String>) -> Self {
        Self {
            problems,
            checks_run,
            generated_at: chrono::Utc::now(),
        }
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
            })
        })
        .collect()
}

/// Problems for adapters on the "Public" profile. The public profile blocks
/// inbound connections and multicast discovery, which is exactly why Resilio
/// transfers failed for every Windows player at the LAN.
pub fn check_network_profiles(profiles: &[NetworkProfile]) -> Vec<Problem> {
    profiles
        .iter()
        .filter(|p| p.category.eq_ignore_ascii_case("Public"))
        .map(|p| {
            Problem::new("network.public_profile", Severity::Error)
                .param("adapter", &p.interface_alias)
                .param("network", &p.name)
                .step("network.public_profile.step.fix")
                .step("network.public_profile.step.manual")
                .with_fix(FixAction::SetNetworkProfilePrivate {
                    interface_index: p.interface_index,
                })
        })
        .collect()
}

/// PowerShell to query profiles (run with `-NoProfile -NonInteractive`).
pub const PS_GET_PROFILES: &str =
    "Get-NetConnectionProfile | Select-Object InterfaceIndex,InterfaceAlias,Name,NetworkCategory | ConvertTo-Json -Compress";

/// PowerShell to switch an adapter to the private profile (needs elevation).
pub fn ps_set_private(interface_index: u32) -> String {
    format!("Set-NetConnectionProfile -InterfaceIndex {interface_index} -NetworkCategory Private")
}

/// `netsh` commands that allow the sync engine on every profile, so a later
/// flip back to "Public" does not silently break transfers again.
pub fn firewall_rules(program: &Path, listening_port: u16) -> Vec<Vec<String>> {
    let prog = program.to_string_lossy().to_string();
    let mut rules = vec![
        vec![
            "advfirewall",
            "firewall",
            "add",
            "rule",
            "name=NextGen LAN Launcher Sync (in)",
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
    ];
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
            } else if health.peers == 0 {
                out.push(
                    Problem::new("transport.no_peers", Severity::Warning)
                        .step("transport.no_peers.step.server")
                        .step("transport.no_peers.step.network"),
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

/// Orphaned sync engine processes not started by this launcher instance.
pub fn check_orphans(our_pid: Option<u32>) -> Vec<Problem> {
    let names = crate::transport::resilio::process_names();
    let mut sys = sysinfo::System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let strangers: Vec<String> = sys
        .processes()
        .iter()
        .filter(|(pid, p)| {
            Some(pid.as_u32()) != our_pid
                && names
                    .iter()
                    .any(|n| p.name().to_string_lossy().eq_ignore_ascii_case(n))
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
    use super::*;

    #[test]
    fn parses_powershell_profiles_numeric_and_string() {
        let json = r#"[{"InterfaceIndex":12,"InterfaceAlias":"Ethernet","Name":"Netzwerk 3","NetworkCategory":0},
                       {"InterfaceIndex":5,"InterfaceAlias":"WLAN","Name":"Home","NetworkCategory":"Private"}]"#;
        let p = parse_net_profiles(json);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].category, "Public");
        let problems = check_network_profiles(&p);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, "network.public_profile");
        assert_eq!(problems[0].params["adapter"], "Ethernet");
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
    fn firewall_rules_cover_all_profiles() {
        let rules = firewall_rules(Path::new(r"C:\App\rslsync.exe"), 55555);
        assert_eq!(rules.len(), 4);
        assert!(rules.iter().all(|r| r.contains(&"profile=any".to_string())));
        assert!(rules[2].iter().any(|a| a == "localport=55555"));
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
