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
  (`theme_url` is ours).
* `GET http://launcher.lan/launcher.css` – legacy stylesheet, scoped to the background layer
  (the body becomes transparent while it is active so the gradient shows through).
* `GET http://launcher.lan/logo.png` – the event logo the LANPage itself uses (`$logo` default);
  shown in the top bar in automatic theme mode.
* `GET http://launcher.lan/theme.json` – **new**: full theme (see THEMING.md).
* Stats beacon: `GET <stats_url>?hostname&macaddr1&macaddr2&board_manufacturer&baseboard&
  system_product_name&bios_release&cpu&gpu&windows_edition&player_name&current_game`, values
  ISO-8859-15 percent-encoded, response `ok`/`error`. Sent every ~3 minutes when enabled.

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
```

Resolution order: `<data dir>/manifests/<id>.toml` (user) → `<share>/nll-manifest.toml`
(organiser) → bundled → derived from `game_start.cmd` (`script_probe.rs`).
Arguments may use `%player%`, `%game_lang%`, `%game_id%`, `%game_path%`.
