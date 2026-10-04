# Compatibility with the ETI ecosystem

Everything below was verified against the public eti-lan repositories (Unlicense) and the
catalog contained in `sync_server.tar`. The ETI Windows client itself is closed source.

## Sync Server = Resilio Sync

The "Sync Server" is a Linux machine running Resilio Sync that joins every share of the
catalog. Clients do not talk to it directly; they add the same read-only shares and Resilio
finds peers via LAN multicast/broadcast. Keys look like `B` + 32 Base32 characters.

## Catalog `eti_launcher/update/game.db` (SQLite)

```sql
games(game_id, db_id PK, game_title, game_key, game_release, game_publisher, game_size NUMERIC /*GB*/,
      game_readme_de, game_readme_en, game_readme_fr, game_maxplayers, game_master_req, genre_id,
      game_version /*YYYYMMDD package revision*/)
genre(genre_id, genre_de, genre_en, genre_fr)
discarded(del_id, game_id, game_key)
tools(tool_id, db_id, tool_name, tool_key, tool_maintainer, tool_size, tool_readme_*, tool_disabled)
```

`assets.eti` next to it holds the covers as `assets/<game_id>.jpg|png`. Like every other `.eti` it
is a RAR archive; the launcher sniffs the format and also accepts a plain or gzip-compressed tar.
The same covers are published in <https://github.com/eti-lan/LAN-Launcher> (`assets/`); the
launcher bundles that set (`assets/covers/` in this repository) and shows it until `assets.eti`
has been extracted into the cover cache, whose files take precedence per game id.

## Sync engine configuration

The ETI client runs `btsync.exe /config <file>` with a `config.json` whose keys are listed in
`crates/lanlauncher-core/src/transport/resilio.rs` (`ETI_CONFIG_KEYS`). Resilio 2.8.1 exits with
code 1 when the config contains keys it does not know, so the launcher emits exactly that key set
with its own values (storage in the app data dir, random ports, LAN-only switches). A Resilio API
key (documented `/api` surface) is not part of the repository; the launcher takes it from the
settings, from an installed ETI client (`<Program Files>\eti\LAN Launcher\sync\config.json`) or
from a `resilio_api_key` block in `launcher.ini` (a NextGen extension; the ETI client ignores
unknown blocks). Without a key the web-UI endpoints with login and password are used.

## Download rate and sources

The documented Sync API (`get_folders`) reports neither progress nor transfer rates, so the
launcher measures the rate from the growth of `<id>.eti`/`<id>.eti.!sync` on disk, which is true
for every transport. Per-share peers come from `get_folder_peers` (API key required); their
`download`/`upload` fields are read as cumulative counters and turned into rates from two samples.

## Runtime package installer

`eti_launcher/bin/preqsetup.exe` (about 3.3 GB) installs the runtimes most games need (.NET 4.8,
VC++ redistributables, DirectX 11, PhysX and more). The ETI client offers it in its settings as
"Paket installieren" behind a confirmation; the launcher does the same and starts the file as is.

## Game share `<root>/<game_id>/`

| File | Meaning |
|---|---|
| `<id>.eti` | RAR archive of the game (large dictionary; UnRAR 7 handles it). Resilio writes `<id>.eti.!sync` while downloading. |
| `version.ini` | Single line = package revision (equals `game_version`). |
| `game_start.cmd` | Windows launch script, called as `"<game_path>" <id> <lang> "<player>"`. |
| `game_setup.cmd` | Optional one-time setup, called with the same four arguments as `game_start.cmd` (`"<game_path>" <id> <lang> "<player>"`; a quarter of the official scripts read `%3`/`%4`). |
| `game_start.cmd` and administrator rights | The ETI client runs elevated for every start. The launcher runs its start scripts as the user: the `netsh advfirewall firewall add rule` lines are executed once at setup (elevated) so they persist, the matching `delete rule` at exit fails harmlessly, and only scripts that write HKLM or import `.reg` files are started with a UAC prompt. |
| `server_start.cmd`, `game_uninst.cmd` | Optional. When `server_start.cmd` exists the detail page offers "Start server" and runs it with the four `game_start.cmd` arguments. |
| `keygen.exe` | Optional key generator (some setup scripts start it as `..\keygen.exe` from `local/`). When present in the game folder (or `local/`), the detail page offers a "Keygen" button. |
| `.SyncIgnore` | Usually `local/*`. |
| `local/` | Extraction target. Scripts `cd local`. |

Scripts expect `%programfiles%\eti\lan launcher\unrar.exe` and `fnr.exe`; the Windows
installer of this launcher must provide them at that path (planned in `release.yml`).

## LANPage

* `GET http://launcher.lan/launcher.ini` – block format, parser in `launcher_ini.rs`.
  Keys: `lan_title, lan_id, lan_url, force_lan_mode, lan_upload_limit, stats_url, ts3_server,
  discord_url, dc_hub, link_1..5 ("Label|URL"), disable_games`. Unknown keys are kept
  (`theme_url` and the `theme_*` colour keys are ours, see THEMING.md).
* `GET http://launcher.lan/launcher.css` – legacy stylesheet, scoped to the background layer
  (the body becomes transparent while it is active so the gradient shows through).
* `GET http://launcher.lan/logo.png` – the event logo the LANPage itself uses (`$logo` default);
  shown in the top bar in automatic theme mode.
* `GET http://launcher.lan/theme.json` – **new**: full theme, version 2 adds the colours of the
  top and status bars, an overlay over the background image and a web font (see THEMING.md).
* Stats beacon: `GET <stats_url>?hostname&macaddr1&macaddr2&board_manufacturer&baseboard&
  system_product_name&bios_release&cpu&gpu&windows_edition&player_name&current_game`, values
  ISO-8859-15 percent-encoded, response `ok`/`error`. Sent every ~3 minutes whenever the LANPage
  names a `stats_url` (the LANPage's player list is the point of the beacon; there is no setting
  for it, as there is none in the ETI launcher).

## Manifests (`manifests/<id>.toml`)

```toml
schema = 1
id = "goldsrc"
revisions = ["20240623"]      # verified package revisions; empty = any
source = "macETI-LAN (MIT)"

[launch]
exe = "hl-cs16/SmartSteamLoader.exe"   # relative to local/
args = ["-game", "cstrike"]
workdir = "hl-cs16"                    # optional
runner = "auto"                        # auto | wine | crossover | proton | native
required_files = ["hl-cs16/hl.exe"]    # install counts as complete only if these exist
env = {}
wrapper = []                           # programs in front of the start, e.g. ["gamemoderun"]

[[launch.alternatives]]
name = "Half-Life"
exe = "hl-cs16/SmartSteamLoader.exe"
args = ["-game", "valve"]

[setup]
copy = [{ from = "SmartSteamEmu.ini", to = "hl-cs16/SmartSteamEmu.ini" }]
touch = ["bin/steam_settings/disable_overlay.txt"]
notes.de = "…"
notes.en = "…"

[platform.macos]
runner = "crossover"
args = ["-window"]

[platform.linux]                       # exe, args, workdir, runner, env, wrapper, unset_env
runner = "proton"
workdir = ""                           # "" = the exe's own folder, whatever [launch] says
wrapper = ["gamescope", "-w", "1280", "-h", "800", "--"]
env = { WINEDLLOVERRIDES = "dinput8=n,b", PROTON_USE_WINED3D = "1" }
unset_env = ["WINEDEBUG"]              # variables of [launch].env this platform goes without
```

`winetricks = ["directplay", "vcrun2010"]` (in `[launch]` or `[platform.<os>]`) names Windows
components as winetricks verbs. They are never installed on their own: the launch configuration
has a button that runs winetricks on the prefix the game starts in (`launch/winetricks.rs`) — Wine's
`WINEPREFIX`, or Proton's `<compat>/pfx` with Proton's own Wine (`files/` or `dist/`), as
protontricks does it; Proton must have started the game once to create that prefix. CrossOver is
refused: its own "Install Software" does this for a bottle. winetricks runs once per verb, and its exit
status says whether that verb went in; it skips what the prefix's `winetricks.log` records unless
"reinstall" (`--force`) is ticked, and settings such as `winxp` apply again on every run. winetricks' own commands
(`annihilate`, `shell`, `prefix=`, a `*.verb` file …) are refused as verbs. One installation runs at
a time, the game does not start meanwhile (and a running game blocks it), and after 45 minutes the run is ended with every process
it started (an installer waiting for a click). winetricks downloads most installers; without internet the launcher
says so and the components can be installed once elsewhere — they stay in the prefix. The builds
bundle winetricks (pinned in `winetricks.lock.json`) and, on Linux, `cabextract` with `libmspack`
(`tools/fetch-winetricks.mjs`), copied to `<data dir>/tools/winetricks` before use. Under Proton
only its Wine gets Proton's libraries, through small stand-in scripts for `wine` and `wineserver`;
the downloads and checksums winetricks runs keep the host's.

`[platform.<os>]` replaces `exe`, `args`, `workdir`, `runner`, `wrapper` and `winetricks` of `[launch]`, removes
the names in `unset_env` from its `env` and adds its own. Windows never reads these profiles; it
runs `game_start.cmd`.

`wrapper` is honoured only from the user's own configuration and profiles and the bundled ones.
A profile from a game share (`nll-manifest.toml`) loses every wrapper where it is loaded, together
with the variables that load or find host programs and libraries (`LD_PRELOAD`,
`PATH`, `PYTHONPATH`, `WINESERVER`, Vulkan layer and GStreamer plugin paths, … — `HOST_HOOK_ENV`);
one derived from `game_start.cmd` never has any. That keeps a profile from adding a way around
Wine; it does not make a share's content harmless — Wine is no sandbox, and a game from the share
can reach the host on its own. The wrapper is applied at spawn time (`LaunchPlan::wrapper`); `program` stays
the Wine/Proton/CrossOver the plan runs.

### Launch configurations from testers

The game details on macOS and Linux offer a **launch configuration** editor (executable,
arguments, working folder, compatibility layer, wrapper, DLL overrides, environment). It saves a
`[platform.<os>]` block to `<data dir>/game-configs/<id>.toml` (`ConfigOverlay`, with the package
revision it was saved for). The block holds only what differs from the profile, and
`ManifestStore::resolve_for` lays it over the profile's own block field by field — so a fix to the
bundled or organiser profile still arrives for everything the tester left alone — or uses it alone
for a game without a profile (`game_config.rs`). A block that changes nothing is removed. The
configuration counts as verified only for the installed package revision it was saved with, and
"choose executable" writes into it when there is one (in the receipt, a choice would start bare).
"Works – share it" sends the profile in force with that block, as it starts (an executable chosen
in the receipt included), plus the test context (launcher version, OS, device, CPU, GPU, the tool
the last start ran with): by mail to `game_config::REPORT_EMAIL`, as a GitHub issue, to the
clipboard or to a file chosen in the system's save dialog. The mail and issue links carry only the
`[platform.<os>]` block (a URL has a length limit); copy and file carry the whole profile. The
profile's `revisions` stay as they were — the tested revision is in the report's text. A maintainer takes the `[platform.<os>]` block into
`manifests/<id>.toml` for the next release.

Resolution order: `<data dir>/manifests/<id>.toml` (user) → `<share>/nll-manifest.toml`
(organiser) → bundled → derived from `game_start.cmd` (`script_probe.rs`).
A manifest may leave `exe` empty: it then only carries setup notes (`flat2`, `cod2` — games whose
working entry point nobody has established yet), and the executable still comes from the script
probe or from the user's choice.
Arguments may use `%player%`, `%game_lang%`, `%game_id%`, `%game_path%`.
