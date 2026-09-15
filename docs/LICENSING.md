# Licensing notes

**Project licence: MIT.** Reasons:

* The ETI scripts, server and LANPage are public domain (Unlicense), macETI-LAN is MIT –
  both combine freely with MIT.
* The UnRAR library (statically linked through the `unrar` crate) has a freeware licence that
  forbids re-implementing RAR compression. It is compatible with MIT/Apache-style projects
  but **not** with the GPL when linked statically. Choosing GPL would force extraction through
  an external `unrar` process.
* Resilio Sync is proprietary and is **never** committed to this repository. Release builds
  download the official binary from Resilio and bundle it as a sidecar, pinned by version and
  SHA-256 in `resilio.lock.json`: run the "Resilio lock" workflow, copy its proposed file over
  `resilio.lock.json`, commit. `tools/fetch-resilio.mjs` refuses unpinned downloads, so a
  release can never silently pick up a different Resilio version. Before a public release,
  review Resilio's EULA regarding redistribution or switch to first-run download
  (`transport::resilio::official_download_url`), which the code already supports through
  `locate_binary` fallbacks.
* Future features: GPL code (e.g. DC++ clients for LAN-Share, Poeschl/LAN-Info-Page) can only
  be integrated as a separate process or service, not linked into this MIT code base.

Keep `THIRD_PARTY_NOTICES.md` up to date when adding dependencies or importing recipes.
