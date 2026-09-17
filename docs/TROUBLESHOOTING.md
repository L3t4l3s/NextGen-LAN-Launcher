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
| Window opens with the right title but stays white (Linux, e.g. SteamOS) | WebKitGTK 2.42+ draws through DMA-BUF, which several drivers answer with nothing. | The launcher sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` for itself before the window is created; the log's second line says so. See below when it is still white. |

Logs: **Diagnose → Log-Ordner öffnen**. Windows: `%LOCALAPPDATA%\xyz.nextgen-lan.launcher\logs\launcher.log`
(not the Roaming folder that holds `settings.json`), macOS: `~/Library/Logs/xyz.nextgen-lan.launcher/`,
Linux: `~/.local/share/xyz.nextgen-lan.launcher/logs/`. The first line names version and commit.

## A white window on Linux

The window appears, the title is right, the content never comes. That is the webview, not the
launcher: WebKitGTK 2.42 and newer render through DMA-BUF, and several Linux graphics stacks
(SteamOS on the Steam Deck among them) show nothing at all through that path.

The launcher sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` for itself at start. Whether a given build
does is in the log's second line (`webview: WEBKIT_DISABLE_DMABUF_RENDERER=…`); on a build from
before that, or to try it by hand, start the AppImage from a terminal:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=1 ./NextGen\ LAN\ Launcher_*.AppImage
```

Since 0.1.0 the launcher also has a switch for the case where WebKitGTK draws
nothing whatever the renderer does:

```bash
./NextGen*.AppImage --safe-graphics     # software rendering, no compositing, X11
./NextGen*.AppImage --no-safe-graphics  # back to the normal path
```

The choice is remembered (`~/.config/xyz.nextgen-lan.launcher/safe-graphics`), so it also applies
when the launcher is started from Steam or a desktop entry afterwards. The log's `webview:` line
names every setting in force.

Still white? The next levers, one at a time:

```bash
WEBKIT_DISABLE_COMPOSITING_MODE=1 ./NextGen*.AppImage   # older WebKitGTK, same symptom
GDK_BACKEND=x11 ./NextGen*.AppImage                     # on a Wayland session
./NextGen*.AppImage --appimage-extract-and-run          # when FUSE is the problem
```

The log's second line records which renderer setting was in force and whether the session is X11
or Wayland, so a report of a white window can say which combination it was. To keep the
accelerated path on a machine where it works, set `WEBKIT_DISABLE_DMABUF_RENDERER=0`.

One failure has a name of its own. If the terminal shows

```
Could not create default EGL display: EGL_BAD_PARAMETER. Aborting...
```

then WebKitGTK gave up before drawing anything: it could not get an EGL display at all. Mesa picks
the EGL platform from the environment, and `WAYLAND_DISPLAY` makes it choose Wayland even where the
window is an X11 one — which is what the AppImage arranges (its GTK hook sets `GDK_BACKEND=x11`),
and the Wayland libraries it then uses are the ones inside the image rather than the machine's.
Since 0.1.0 the launcher sets `EGL_PLATFORM=x11` itself for that combination — an X11 window
(`GDK_BACKEND=x11`, which the AppImage sets) in a Wayland session. The `webview:` line in the log
names the platform in force, and a platform set by hand is left alone, which is also how to undo
it:

```bash
EGL_PLATFORM=x11 ./NextGen*.AppImage        # by hand, on an older build
EGL_PLATFORM=wayland ./NextGen*.AppImage    # or the other way, to try Wayland's
```

If none of it helps, the interesting messages are the ones WebKitGTK writes to the terminal when
its web process gives up. They do not reach `launcher.log`, so run it once like this and keep the
file:

```bash
./NextGen*.AppImage --safe-graphics 2>&1 | tee ~/nll-terminal.log
```

`libGL`, `EGL`, `Gdk` and `WebKit` lines in there name the piece that fails; `launcher.log` on its
own only shows that the interface never reported for duty (no `interface ready` line).

## Folder mode

If Resilio Sync cannot be started by the launcher, it falls back to folder mode: run Resilio
yourself, add the key shown in the game details, choose the shown folder, keep selective sync
off. Verification and extraction still work the same way.
