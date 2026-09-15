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

`assets.eti` next to it is a tar archive with `assets/<game_id>.jpg|png` covers.

## Game share `<root>/<game_id>/`

| File | Meaning |
|---|---|
| `<id>.eti` | RAR archive of the game (large dictionary; UnRAR 7 handles it). Resilio writes `<id>.eti.!sync` while downloading. |
| `version.ini` | Single line = package revision (equals `game_version`). |
| `game_start.cmd` | Windows launch script, called as `"<game_path>" <id> <lang> "<player>"`. |
| `game_setup.cmd` | Optional one-time setup: `"<game_path>" <id>`. |
| `server_start.cmd`, `game_uninst.cmd` | Optional. |
| `.SyncIgnore` | Usually `local/*`. |
| `local/` | Extraction target. Scripts `cd local`. |

Scripts expect `%programfiles%\eti\lan launcher\unrar.exe` and `fnr.exe`; the Windows
installer of this launcher must provide them at that path (planned in `release.yml`).

## LANPage

* `GET http://launcher.lan/launcher.ini` – block format, parser in `launcher_ini.rs`.
  Keys: `lan_title, lan_id, lan_url, force_lan_mode, lan_upload_limit, stats_url, ts3_server,
  discord_url, dc_hub, link_1..5 ("Label|URL"), disable_games`. Unknown keys are kept
  (`theme_url` is ours).
* `GET http://launcher.lan/launcher.css` – legacy stylesheet, scoped to the background layer.
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
