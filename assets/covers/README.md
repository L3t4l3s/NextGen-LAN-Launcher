# Bundled covers

Cover images for the ETI game catalog, one `<game_id>.jpg` per game, shipped with
the launcher so the library shows artwork before (or without) a sync server
delivering `eti_launcher/update/assets.eti`.

Source: <https://github.com/eti-lan/LAN-Launcher> (`assets/`), released into the
public domain by ETI (see the LICENSE in that repository). Refresh with
`tools/update-covers.sh`; the script records the upstream commit in `UPSTREAM`.

At runtime a cover extracted from `assets.eti` into the cover cache wins over the
bundled file of the same game id.
