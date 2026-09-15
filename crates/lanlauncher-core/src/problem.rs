//! User-facing problem descriptions.
//!
//! Every failure the launcher can detect is expressed as a [`Problem`]: a short
//! title, the likely cause, concrete steps for a beginner, and optionally a
//! machine-executable fix the UI can offer as a "Fix now" button. Texts are
//! keyed by message id so the frontend can localise them; `params` carry the
//! dynamic values (paths, sizes, adapter names).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Purely informational; nothing is broken.
    Info,
    /// Something is degraded but the current operation can continue.
    Warning,
    /// The current operation cannot succeed until this is resolved.
    Error,
}

/// A fix the launcher can apply on behalf of the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FixAction {
    /// Set the Windows network profile of the given interface to "Private".
    SetNetworkProfilePrivate { interface_index: u32 },
    /// Add inbound/outbound firewall rules for the sync engine on all profiles.
    AddFirewallRules,
    /// Terminate orphaned sync engine processes and restart the transport.
    RestartTransport,
    /// Re-run verification and extraction for a game ("Repair").
    RepairGame { game_id: String },
    /// Open a folder in the OS file manager.
    OpenFolder { path: String },
    /// Open a URL in the browser.
    OpenUrl { url: String },
    /// Add a Windows Defender exclusion for a path.
    AddDefenderExclusion { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    /// Stable identifier, e.g. `network.public_profile`. Used for i18n and for
    /// de-duplicating repeated reports.
    pub code: String,
    pub severity: Severity,
    /// Values interpolated into localised texts (adapter name, size, path...).
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    /// Steps a beginner can follow, as message ids (`code.step.1` etc. are
    /// implied when empty).
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<FixAction>,
}

impl Problem {
    pub fn new(code: impl Into<String>, severity: Severity) -> Self {
        Self {
            code: code.into(),
            severity,
            params: BTreeMap::new(),
            steps: Vec::new(),
            fix: None,
        }
    }

    pub fn param(mut self, key: &str, value: impl ToString) -> Self {
        self.params.insert(key.to_string(), value.to_string());
        self
    }

    pub fn step(mut self, step: impl Into<String>) -> Self {
        self.steps.push(step.into());
        self
    }

    pub fn with_fix(mut self, fix: FixAction) -> Self {
        self.fix = Some(fix);
        self
    }
}
