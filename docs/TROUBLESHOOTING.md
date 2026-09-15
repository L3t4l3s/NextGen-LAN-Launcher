# Troubleshooting (for organisers and helpers)

The launcher's **Diagnose** tab shows the same checks with one-click fixes. This page explains
the background.

| Symptom | Cause | What the launcher does |
|---|---|---|
| Sync stuck at 99 %, game never becomes playable | Old launcher waited for the sync engine to report 100 %. | Verifies the archive itself once `<id>.eti` exists and is stable, then extracts. Use **Reparieren** to force it. |
| Nothing downloads on Windows | Network profile is **Public** → inbound + discovery blocked. | Diagnose: `network.public_profile` with "Jetzt beheben" (`Set-NetConnectionProfile … Private`). Adapters without traffic (idle second NIC, docking station) are ignored; a public adapter next to an active private/domain one is only a warning (`network.public_profile_secondary`). Firewall rules are added with `profile=any`. |
| "Too many workers" | Several sync engine instances. | Only one managed instance; orphans (PID file/process list) are killed at start. `transport.foreign_instance` warns about others. |
| No peers | Sync server off / other subnet / LAN mode without a seed. | `sync.no_peers` after 2 min without progress. |
| Archive incomplete after 100 % | Partial download, disk error. | `install.archive_incomplete`; re-verifies automatically when the file changes. |
| Disk full | – | `install.disk_full` before download, `disk.low_space` in Diagnose; add a second library root. |
| Antivirus quarantines unrar / SmartSteamLoader | Heuristics. | Fix action `AddDefenderExclusion` for app and library folders (Windows). |
| Wrong system clock | Resilio refuses peers with large skew. | `clock.skew` informational (was never the actual cause at our LANs). |
| "Resilio Sync nicht verfügbar" although Resilio or the ETI launcher is installed | The binary is outside the probed locations. | The search covers `%LOCALAPPDATA%`/`%ProgramFiles%` Resilio installs, the ETI launcher's `btsync.exe` in `%ProgramFiles%\eti\lan launcher`, `PATH`, other user profiles and the registry; every probed path is logged. Settings → Advanced → "Resilio executable" pins a path by hand. |
| Games installed by the ETI launcher get re-extracted / "Einrichtungsskript meldete einen Fehler" on start | Older builds started every found install from scratch. | Existing installs are adopted: the archive listing is compared with `local/` (sizes, no CRC) and a receipt with `adopted: true` is written; nothing is re-extracted and no setup runs. **Reparieren** forces the full verify + extract. |
| Status bar shows "N Hinweise" but Downloads looks empty | Playable games with a warning were filtered out. | Downloads has a "Hinweise" section listing playable games with a problem card. |
| No covers | `eti_launcher/update/assets.eti` missing, unreadable, or not a (gzip) tar. | Extraction result is logged (`covers: …`); Diagnose shows `catalog.covers_missing` when the file exists but the cache is empty. |
| Game starts but LAN browser empty (Mac) | Bonjour service inside the bottle, firewall. | See manifest notes (e.g. wc3). |

Logs: **Diagnose → Log-Ordner öffnen**. Windows: `%LOCALAPPDATA%\xyz.nextgen-lan.launcher\logs\launcher.log`
(not the Roaming folder that holds `settings.json`), macOS: `~/Library/Logs/xyz.nextgen-lan.launcher/`,
Linux: `~/.local/share/xyz.nextgen-lan.launcher/logs/`. The first line names version and commit.

## Folder mode

If Resilio Sync cannot be started by the launcher, it falls back to folder mode: run Resilio
yourself, add the key shown in the game details, choose the shown folder, keep selective sync
off. Verification and extraction still work the same way.
