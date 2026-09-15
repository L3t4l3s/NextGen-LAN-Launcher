#!/usr/bin/env sh
# Refresh assets/covers/ from the public ETI LAN-Launcher repository.
# Usage: tools/update-covers.sh [branch|tag|commit]   (default: main)
set -eu
ref="${1:-main}"
repo="https://github.com/eti-lan/LAN-Launcher.git"
root="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# fetch + FETCH_HEAD accepts branches, tags and commit SHAs alike
# (`clone --branch` would reject the SHA recorded in UPSTREAM).
git clone --quiet --depth 1 --no-checkout --filter=blob:none "$repo" "$tmp/eti"
git -C "$tmp/eti" fetch --quiet --depth 1 origin "$ref"
git -C "$tmp/eti" sparse-checkout set assets
git -C "$tmp/eti" checkout --quiet FETCH_HEAD

find "$root/assets/covers" -maxdepth 1 -name '*.jpg' -delete
find "$tmp/eti/assets" -maxdepth 1 -name '*.jpg' -exec cp {} "$root/assets/covers/" \;
git -C "$tmp/eti" rev-parse HEAD > "$root/assets/covers/UPSTREAM"

printf '%s covers from %s@%s\n' "$(find "$root/assets/covers" -name '*.jpg' | wc -l)" "$repo" "$(cat "$root/assets/covers/UPSTREAM")"
