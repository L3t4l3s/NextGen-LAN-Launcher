# Architecture

```
┌─────────────────────────────── Tauri window (WebView) ───────────────────────────────┐
│ Svelte 5 UI  ·  i18n de/en  ·  theme engine  ·  browser mock for development          │
└──────────────────────────────▲──────────────────────────────────────────────────────┘
                               │ invoke() commands / emitted events (install-status, transport-health, event-updated)
┌──────────────────────────────┴──────────────────────────────────────────────────────┐
│ src-tauri  (thin shell)                                                               │
│  commands.rs  fixes.rs (PowerShell/netsh)  lib.rs (service loops, sidecar, demo mode) │
└──────────────────────────────▲──────────────────────────────────────────────────────┘
                               │
┌──────────────────────────────┴──────────────────────────────────────────────────────┐
│ crates/lanlauncher-core (no GUI, fully unit-tested)                                   │
│  catalog ─ launcher_ini ─ theme ─ settings ─ library ─ manifest ─ script_probe        │
│  transport { resilio | folder | demo }  ─▶  install (state machine)  ─▶  extract      │
│  launch { windows scripts | unix runners }   lanpage (ini, stats)   diagnostics       │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

## Data flow of an installation

1. UI calls `install_game` → `InstallManager::install` adds the Resilio share for the game
   (`<root>/<game_id>`) via the transport and marks the tracker *wanted*.
2. Every 2 s `InstallManager::tick` observes each tracked game (`Observation::from_disk` +
   transport status) and runs `Tracker::step`, a pure function that decides the next action.
3. **Syncing → Verifying** happens when `<id>.eti` exists, no `<id>.eti.!sync` partial file
   exists, `version.ini` is present and the archive size has been stable for 15 s — *or* when
   the transport reports completion. The engine's percentage is never required.
4. **Verifying** runs a full UnRAR CRC test in a blocking task. Failure → back to Syncing with
   the problem `install.archive_incomplete` (retry on size change or after 2 min).
5. **Extracting** unpacks into `.nll-staging` and swaps it into `local/` atomically.
6. **Setup** applies manifest steps (copy/touch) and on Windows runs `game_setup.cmd`.
7. A receipt `.nll-install.json` (revision, size, files, setup_done, exe_override) is written.
   **Ready** ⇔ receipt exists, required files exist. `UpdateAvailable` ⇔ receipt revision ≠
   catalog revision (or a newer `version.ini` arrived).
8. **Repair** works from any phase: cancels running work, re-adds the share, resets the
   stability timer and re-runs 3–7.

## Transport

`Transport` is a trait. `ResilioTransport` spawns the official binary with a generated
`config.json` (API on 127.0.0.1 with a random password, storage in the app data dir,
`sync_max_time_diff` 48 h, LAN discovery mode 3, tracker/relay off in LAN mode), cleans up
orphaned instances (PID file + process list) and talks to the documented Sync API when an
`api_key` is configured, otherwise to the GUI endpoints. `FolderTransport` only watches the
folders and hands out keys. `DemoTransport` simulates downloads including the "stuck at 99 %"
case and is used by `--demo` / `LANLAUNCHER_DEMO=1`.

## Launching

* Windows: `cmd.exe /C "game_start.cmd" "<game_path>" <id> <lang> "<player>"`, elevated via
  the application manifest (the scripts use `netsh advfirewall` and `reg add HKLM`).
  `game_setup.cmd "<game_path>" <id>` runs once after extraction.
* macOS/Linux: `launch::unix::plan` resolves exe/args from the manifest (user override →
  organiser overlay `nll-manifest.toml` in the share → bundled → derived from the script),
  picks CrossOver / Proton / Wine and uses one prefix per game (`<share>/.nll-prefix`).

## Media

Covers are extracted from `eti_launcher/update/assets.eti` into the cache directory. Preview
videos are looked up at `eti_launcher/video/<id>.mp4` (also `videos/`, `update/video/`, `.webm`)
inside the default library root and played muted in the detail header. Library roots are added to
the Tauri asset-protocol scope at runtime so the WebView can load them.

## Diagnostics and fixes

`diagnostics.rs` produces `Problem { code, severity, params, steps, fix }`. The UI localises
by code (`problem.<code>.title/.cause`, `problem.<step>`). Command errors and fix results are
codes as well (`err.*`, `msg.*`, optional `|detail`) resolved by `userText()` in the frontend. `fixes.rs` executes `FixAction`s:
PowerShell `Set-NetConnectionProfile`, `netsh advfirewall` rules for all profiles, Defender
exclusions, transport restart, repair, open folder/URL.

## Directories

* Settings: `<config dir>/settings.json` (atomic writes).
* Data: `<data dir>/transport` (Resilio storage), `manifests/` (user overrides), `demo/`.
* Cache: `<cache dir>/covers/<game_id>.jpg` from `assets.eti`.
* Library root(s): ETI layout `<root>/<game_id>/{<id>.eti, version.ini, game_start.cmd, local/}`.
