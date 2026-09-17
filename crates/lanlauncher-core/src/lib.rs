//! Core library of the NextGen LAN Launcher.
//!
//! Everything in this crate is GUI-independent and testable on any platform:
//!
//! * [`catalog`] reads the ETI `game.db` catalog and the `assets.eti` cover archive.
//! * [`launcher_ini`] parses the `launcher.ini` block format served by an ETI LANPage.
//! * [`theme`] models the per-event visual theme.
//! * [`settings`] / [`library`] hold user settings and the list of library roots.
//! * [`manifest`] loads cross-platform game manifests (TOML) and derives fallback
//!   manifests from the Windows `game_start.cmd` scripts shipped in each game share.
//! * [`transport`] abstracts the sync backend (Resilio Sync, plain folder, demo).
//! * [`install`] is the install state machine that decides when a game is playable
//!   based on what is actually on disk, never on what the sync engine reports.
//! * [`extract`] verifies and extracts `.eti` (RAR) archives.
//! * [`graphics`] holds the order in which the Linux shell tries to get its own
//!   window drawn.
//! * [`launch`] starts games per platform.
//! * [`lanpage`] talks to the ETI LANPage (launcher.ini, stats beacon).
//! * [`diagnostics`] runs environment checks and produces actionable problems.

pub mod catalog;
pub mod diagnostics;
pub mod error;
pub mod extract;
pub mod graphics;
pub mod install;
pub mod lanpage;
pub mod launch;
pub mod launcher_ini;
pub mod library;
pub mod manifest;
pub mod paths;
pub mod problem;
pub mod script_probe;
pub mod settings;
pub mod theme;
pub mod transport;

pub use error::{Error, Result};
