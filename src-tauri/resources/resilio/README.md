# Resilio Sync binary

The release workflow places the official Resilio Sync binary for the target
platform in this folder (`rslsync`, `Resilio Sync.app/…` or `Resilio Sync.exe`),
downloaded from the URLs pinned in `../../../resilio.lock.json`.

The binary is **not** committed to git. Without it the launcher looks for a
system-wide Resilio installation and otherwise offers to download it on first
start, or falls back to folder mode.
