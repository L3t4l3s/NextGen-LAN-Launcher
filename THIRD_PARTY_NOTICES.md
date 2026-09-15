# Third-party notices

This project is MIT licensed. It builds on and interoperates with the following work:

- **ETI LAN Launcher game scripts, cover art, Sync Server scripts and LANPage** by the eti Team
  (https://github.com/eti-lan) – released under **The Unlicense** (public domain). The
  `launcher.ini` format, the `game.db` schema, the `stats.php` beacon and the
  `game_start.cmd` contract are re-implemented here for compatibility. The ETI LAN Launcher
  *client itself* is closed-source freeware and is not part of this project.
- **macETI-LAN** by kaapyth0n (https://github.com/kaapyth0n/macETI-LAN) – **MIT License**.
  The game manifests marked `source = "macETI-LAN"` and parts of the CrossOver launch
  approach follow its recipes and documentation.
- **pETI-server** by Poeschl (https://github.com/Poeschl/pETI-server) – Apache-2.0. Used as a
  reference for the Resilio Sync API calls of the ETI sync server; no code copied.
- **UnRAR** (via the `unrar` / `unrar_sys` crates) – © Alexander Roshal. The UnRAR source is
  freeware: it may be used and redistributed freely, but must not be used to develop a
  RAR-compatible *compressor*. See https://www.rarlab.com/license.htm.
- **Resilio Sync** – proprietary software by Resilio Inc. It is **not** contained in this
  repository. Release builds download the official binary from Resilio's servers
  (`resilio.lock.json`) and ship it as a sidecar; users may instead use their own installation
  (folder mode).
- **Tauri**, **Svelte**, **Vite** and the Rust crates listed in `Cargo.lock` under their
  respective MIT/Apache-2.0 licenses.
