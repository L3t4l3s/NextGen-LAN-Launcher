# Resilio Sync binary

The release workflow and the full CI matrix place the official Resilio Sync
binary for the target platform in this folder (`rslsync`, `Resilio Sync.app/…`
or `Resilio Sync.exe`), downloaded from the URLs pinned in
`../../../resilio.lock.json` by `tools/fetch-resilio.mjs`.

The binary is **not** committed to git (ignored via `.gitignore`). The launcher
prefers this copy over any Resilio installed on the system and starts it in
place (`/noinstall /config …` on Windows). Without it the launcher looks for a
system-wide installation, offers the pinned download on first start, or falls
back to folder mode.
