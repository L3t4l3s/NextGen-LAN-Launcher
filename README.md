# NextGen LAN Launcher

A cross-platform successor to the **ETI LAN Launcher** (eti-lan.xyz) for LAN parties: install and
start games from the existing ETI ecosystem (sync server, `game.db`, `.eti` packages, LANPage) on
**Windows 10/11, macOS and Linux**, with a modern interface that organisers can re-brand per
event and with clear instructions whenever something does not work.

> Status: **0.1.0 – foundation.** Core logic and UI are complete and tested; operation against a
> real sync server still has to be verified at the next LAN (see [Open items](#open-items)).

![Library](screenshots/01-library.png)

## What is different from the old launcher

| Problem at the last LAN | Solution here |
|---|---|
| Sync stuck at 99 %, "Install" never turns into "Play" | The launcher **verifies the archive itself** (CRC test) and extracts as soon as the file is complete on disk, regardless of what the sync engine reports. `crates/lanlauncher-core/src/install.rs` |
| "Too many workers" at startup | Exactly one sync engine instance, controlled by the launcher; orphaned processes are cleaned up at start. |
| Windows switches the LAN to a "Public" network profile | Diagnostics detects the profile and switches it to "Private" with one click; firewall rules apply to all profiles. |
| Unclear row of icons at the top | Labelled tabs: Library · Downloads · LAN · Diagnostics, plus Settings on the right. |
| Only one game folder | Several library folders (e.g. different SSDs); new games go where there is most space. |
| Windows only | Launch profiles (`manifests/*.toml`) plus Wine/CrossOver/Proton on macOS and Linux; Windows keeps running `game_start.cmd`. |
| Administrator rights for everything | The launcher runs as a normal user. UAC appears only for a game's one-time setup (which also registers the firewall rules its start script would add), for the few start scripts that write HKLM, for server scripts and for repairs. |

Design decision from our planning: every game has **one** primary button (Install / Downloading… /
Play / Update), but the secondary actions **Repair**, **Pause sync**, **Open folder** and **Remove**
stay visible at all times, whatever the launcher is currently doing. Repair stops the sync,
verifies the archive and re-extracts it.

## Layout

```
crates/lanlauncher-core/   Rust library without GUI: catalog, launcher.ini, transport (Resilio/folder/demo),
                           install state machine, UnRAR, launching, diagnostics – 60+ tests
src-tauri/                 Tauri 2 app (commands, events, sidecar, demo mode)
src/                       Svelte 5 frontend (German/English, theme engine, browser mock for development)
manifests/                 Launch profiles for macOS/Linux (initially: amongus, rocket, goldsrc, wc3, quake3, l4d2)
assets/covers/             Cover images bundled with the app (from eti-lan/LAN-Launcher, public domain)
themes/                    Example themes
tools/dev-lanpage/         Minimal LANPage stand-in for local testing (launcher.ini, launcher.css, stats.php)
docs/                      Architecture, compatibility, troubleshooting, theming, licensing
```

Details: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) · [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) ·
[docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) · [docs/THEMING.md](docs/THEMING.md) ·
[docs/LICENSING.md](docs/LICENSING.md) · [CLAUDE.md](CLAUDE.md) (operational notes for contributors)

## Development

Prerequisites: Rust (stable), Node 22, and on Linux
`libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev`.

```bash
npm install
cargo test -p lanlauncher-core          # core logic
npm run check && npm test               # frontend
npm run dev                             # UI in the browser with a simulated backend (http://localhost:1420)
npm run tauri dev -- -- --demo          # the real app in demo mode (simulated sync)
npm run tauri build                     # installer for the current platform
node tools/dev-lanpage/server.mjs       # local LANPage stand-in on port 8080
```

In the browser, `http://localhost:1420/?wizard` shows the first-run wizard.

CI runs the Linux core tests and the frontend checks on every push. The full three-OS matrix and
the installer builds (artifacts `bundles-<os>` on the run page) only run for pull requests, pushes
to `main`, a manual run of the *CI* workflow, or a commit whose message contains `[full-ci]`.
Tags `v*` are built by `release.yml` instead.

## Open items

- **Real Resilio operation:** the client speaks the documented Sync API (`/api`, with an API key)
  and falls back to the GUI endpoints. Both can only be verified against a running sync server.
  The install logic deliberately does not depend on either.
- **Bundled Resilio:** release builds and the full CI matrix ship the official binary pinned by
  version and SHA-256 in `resilio.lock.json`, currently build 2.8.1.1390 for all four platforms.
  Resilio 3.x requires a Resilio account for its free licence, 2.8.1 does not, the ETI sync
  server runs the same build, and the 3.x change logs list no fix that matters for LAN
  transfers; both versions sync with each other. Re-pin with the "Resilio lock" workflow (input
  `version`). The bundled copy is preferred over system installs and started in place; on
  Windows with `/noinstall /config …` (Resilio's download is the program itself). Untested on
  Windows hardware: that 2.8.1.1390 honours `/noinstall` and runs in place from the install
  folder instead of copying itself to `%APPDATA%`, that a bundled copy runs next to a user's own
  Resilio Sync, and launching scripts via `cmd.exe /S /C`.
- **Engine config:** the generated `config.json` mirrors the ETI launcher's file (same keys, own
  values). The earlier key set made Resilio 2.8.1 exit with code 1 on the test PC; whether the
  mirrored set starts it there is the next thing to test. The API key is discovered from an
  installed ETI client or the LANPage (`resilio_api_key`); the web-UI fallback without a key is
  untested against a real engine.
- **Engine start diagnostics:** when the sync engine exits before its API answers, the error names
  other processes running the same executable and, failing that, every other running sync engine
  with pid and path (a user's own Resilio Sync or the ETI launcher's engine may block ours if
  Resilio allows one instance per machine). Whether two copies of Resilio 2.8.1 can run side by
  side, and whether the process table exposes executable paths on Windows, is untested.
- **LANPage logo and stylesheet:** `logo.png` and `launcher.css` from `launcher.lan` are applied
  in automatic theme mode; verified against ETI's LANPage template only, not yet against a live
  page.
- **Runtime package installer:** Settings offers ETI's `eti_launcher/bin/preqsetup.exe` (about
  3.3 GB, .NET 4.8, VC++, DirectX 11, PhysX) behind a confirmation like the ETI client; whether
  the installer needs arguments is unknown and untested on hardware.
- **Firewall rule check:** diagnostics ask `Get-NetFirewallRule` for the launcher's own rule and
  offer the fix only for the bundled or downloaded engine; the PowerShell output format and the
  behaviour with a stopped firewall service are untested on hardware.
- **On-demand elevation:** the manifest is `asInvoker`; setup scripts, firewall rules, HKLM
  scripts and repairs run through PowerShell `Start-Process -Verb RunAs`. The UAC flow, the exit
  code hand-over and scripts that need admin in places the detection does not see are untested
  on hardware.
- **Keygen and server start:** the detail page offers `keygen.exe` (started from `local/` like
  ETI's setup scripts do) and `server_start.cmd` (same four-argument contract as
  `game_start.cmd`) when the package ships them. Both run through `cmd.exe`/Windows and are
  untested on hardware.
- **Windows network check:** the diagnostics now read `IPv4Connectivity`/`IPv6Connectivity` from
  `Get-NetConnectionProfile`, skip idle adapters and downgrade a public adapter next to a
  private/domain one to a warning. Written against a test system with a domain network plus an
  idle second NIC; untested at a LAN party where the only adapter has no internet.
- **Catalog share and server detection:** in managed mode the launcher adds the `eti_launcher`
  share itself and reports "sync server found" when that share has a peer. The built-in key
  (`BUILTIN_CATALOG_KEY` in `crates/lanlauncher-core/src/catalog.rs`) is the public read-only key
  from ETI's `sync_server.tar`; a LAN with its own catalog can override it in the settings.
  Registration and peer-based detection are untested against a real sync server. Resilio cannot
  tell a server from another client that already holds the catalog, so a second player with a
  complete `eti_launcher` also counts as "server found".
- **ETI installs and setup scripts:** existing installations are adopted by comparing the
  archive listing with `local/`, and `game_setup.cmd` now receives the four-argument contract
  through the raw command line. Both untested on Windows hardware, as is the broadened Resilio
  search (ETI `btsync.exe`, PATH, other profiles, registry).
- **Covers and videos:** the installer ships the covers from the public
  [eti-lan/LAN-Launcher](https://github.com/eti-lan/LAN-Launcher) repository (`assets/covers/`,
  refreshed with `tools/update-covers.sh`), so the library shows artwork without a sync server.
  Covers from the LAN's `eti_launcher/update/assets.eti` replace them; videos still come only
  from the share (`video/<id>.mp4`).
- **macOS/Linux launch profiles:** six games have curated profiles; every other game gets a profile
  derived from `game_start.cmd` (91 of 158 ETI scripts start exactly one executable) or the user
  picks the executable on first launch.
- Roadmap: LANPage inside the launcher, a LAN-Share file-sharing view, TS3/Discord integration.

## License

MIT – see [LICENSE](LICENSE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
