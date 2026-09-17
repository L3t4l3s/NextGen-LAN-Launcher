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
| Window opens with the right title but stays white (Linux, e.g. SteamOS) | WebKitGTK and the machine's graphics stack disagree, in one of several ways. | The launcher works through its renderer settings by itself, restarting on the next one each time the interface fails to report, and remembers the one that draws. See below. |

Logs: **Diagnose → Log-Ordner öffnen**. Windows: `%LOCALAPPDATA%\xyz.nextgen-lan.launcher\logs\launcher.log`
(not the Roaming folder that holds `settings.json`), macOS: `~/Library/Logs/xyz.nextgen-lan.launcher/`,
Linux: `~/.local/share/xyz.nextgen-lan.launcher/logs/`. The first line names version and commit.

## A white window on Linux

The window appears, the title is right, the content never comes. That is the webview, not the
launcher, and which setting gets WebKitGTK to draw depends on the driver, the session and — in an
AppImage — on which libraries the image brought along. There is no single answer to ship, so the
launcher looks for one on the machine itself.

### What it does on its own

Each start applies one set of renderer settings. The launcher then restarts itself with the next
set — at once where the webview has said it gave up (WebKitGTK prints a line such as "Could not
create default EGL display … Aborting…" within a second and never draws again), otherwise after 30
seconds, or 15 on every start after the first. **Let it run.** A whole climb takes well under two
minutes and the window closes and reopens for each step; closing it early is how one Steam Deck
test ended 28 seconds into the first 30-second wait, before the ladder had climbed anything. It
works down this ladder:

| Step | What it does |
|---|---|
| `no-dmabuf` | WebKitGTK's DMA-BUF renderer off. Fixes the majority of white GTK webviews — and is what made a Steam Deck white. |
| `native` | Nothing forced at all — WebKitGTK's own renderer on the session's own backend. **This is the one a Steam Deck lands on.** |
| `wayland` | Window and EGL both on Wayland, which undoes the `GDK_BACKEND=x11` the AppImage forces on every session. Only in a Wayland session. |
| `x11` | Window and EGL both on X11. Only with an X server. |
| `software` | No compositing, software GL (llvmpipe) on X11. Only with an X server. |
| `software-surfaceless` | The same, plus an EGL display that needs no display server. Always available, so the ladder always ends here. |

The step that draws is remembered in `~/.config/xyz.nextgen-lan.launcher/graphics.json` and used
directly on every later start, so the climb costs its couple of minutes once per machine. Such a
remembered step gets 60 seconds rather than 30 before the launcher gives up on it, so one slow
morning does not push a machine that works down the ladder; and a remembered step that really has
stopped drawing — a driver update, a different session — sends the climb back to the top rather
than one rung further down.

A step can also fail harder than white: settings that make GTK or the driver give up take the
process down before there is any window, and then nothing is left running to notice. The launcher
writes down which step it is about to try before it tries it, and clears that note the moment a
window exists — so a step that killed the last run is left out of the next one (`graphics step …
left out` in the log), while a window the user simply closed early counts as a normal run and
costs nothing.

The `webview:` line in `launcher.log` names the step in force; a `graphics step … drew nothing`
line marks each restart.

If the whole ladder draws nothing, a message box says so — it comes from the window manager rather
than from the webview, so it is visible even then — and names the file described below. The
climb is then forgotten rather than pinned at the bottom, so a machine that is fixed later starts
over from the top by itself.

### What WebKitGTK says about it

Those messages never reached `launcher.log`: the web process writes them to standard error, and a
launcher started from a desktop entry or from Steam has no terminal. Since 0.1.0 they are kept in

```
~/.local/share/xyz.nextgen-lan.launcher/logs/webview.log
```

with a header per step of the climb, so one white-window run produces the whole record. **That file
plus `launcher.log` from the same folder is what a report needs.** Started from a terminal the
output stays in the terminal instead, unchanged:

```bash
./NextGen*.AppImage 2>&1 | tee ~/nll-terminal.log
```

`libGL`, `EGL`, `Gdk` and `WebKit` lines in there name the piece that fails.

### Steering it by hand

```bash
./NextGen*.AppImage --safe-graphics     # straight to the bottom of the ladder
./NextGen*.AppImage --no-safe-graphics  # forget this machine, climb again from the top
```

The choice is remembered, so it also applies when the launcher is started from Steam or a desktop
entry afterwards. Any of the variables a step sets can also be set by hand, and such a value is
left alone:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=0 ./NextGen*.AppImage   # keep the accelerated path
EGL_PLATFORM=wayland ./NextGen*.AppImage               # pin the EGL platform
./NextGen*.AppImage --appimage-extract-and-run         # when FUSE is the problem
```

A pin does not silently disable half a step: a step that wants one of the pinned variables set to
something else is **skipped whole**, and the climb goes on to the next. Half of `x11` would be a
window on X11 with EGL left on Wayland, which is the mismatch the ladder exists to undo. Pinning
`WEBKIT_DISABLE_DMABUF_RENDERER=0` therefore leaves `native` as the only step on the ladder, which
is exactly what "keep the accelerated path" should mean.

The one exception is `GDK_BACKEND` (and `GTK_THEME`) under an AppImage: the image's own GTK hook
sets them for every session, unasked, so they are not treated as anyone's choice — and taking the
forced `GDK_BACKEND=x11` back is precisely what the `wayland` step is for.

### When the AppImage brings its own Wayland

Measured on a built image: `libEGL`, `libGL`, `libgbm` and `libdrm` are correctly left out, so
they come from the machine — but `libwayland-client`, `-server`, `-egl` and `-cursor` were packed
into it, and `LD_LIBRARY_PATH` puts the image first. The machine's Mesa then runs against Ubuntu's
Wayland, which the AppImage exclude list names `libwayland-client` specifically to prevent.

This was **not** what made the Steam Deck white — that was the renderer setting above, settled on
the device — and no machine is known to need this. It is worth knowing about anyway, because it is
the one failure a renderer setting cannot cure.

CI builds have the four taken back out again since this change
(`tools/appimage-unbundle-wayland.mjs`); **release builds do not yet**, so an AppImage from a
GitHub release still carries them. On any build that does, the same thing by hand — and this is
the check worth running, because it is what confirms the cause:

```bash
./NextGen*.AppImage --appimage-extract
rm squashfs-root/usr/lib/libwayland-*
./squashfs-root/AppRun
```

If that draws and the AppImage does not, this was it.

### One failure with a name of its own

```
Could not create default EGL display: EGL_BAD_PARAMETER. Aborting...
```

WebKitGTK gave up before drawing anything: it could not get an EGL display at all. **On a Steam
Deck this line means the launcher turned off a renderer the machine needed.** The *default* display
is what the path **without** the DMA-BUF renderer asks for, so the setting shipped to cure white
windows — `WEBKIT_DISABLE_DMABUF_RENDERER=1` — is what produces this abort here. The ladder's
`native` step, which forces nothing at all, is the answer, and the launcher reaches it by itself
within a second or two of seeing this line.

If you are on a build old enough to force the setting unconditionally, the same thing by hand:

```bash
WEBKIT_DISABLE_DMABUF_RENDERER=0 ./NextGen*.AppImage
```

Note the `0`. Anything the ladder would set is left alone when it comes from outside, so this pins
the accelerated path and the launcher stops arguing with it.

## Folder mode

If Resilio Sync cannot be started by the launcher, it falls back to folder mode: run Resilio
yourself, add the key shown in the game details, choose the shown folder, keep selective sync
off. Verification and extraction still work the same way.
