# winetricks

The release workflow and the full CI matrix place the pinned winetricks script
here (`winetricks`, version and SHA-256 in `../../../winetricks.lock.json`),
and on Linux also `bin/cabextract` with the `libmspack` it needs, copied from
the build runner by `tools/fetch-winetricks.mjs`.

Nothing of it is committed to git (ignored via `.gitignore`). The launcher
copies this folder into its data directory before a run (a resource folder or
an AppImage mount may be read-only or lose the exec bit) and uses it to
install the Windows components a game's profile names. Without it, a
`winetricks` and `cabextract` on the system's `PATH` are used.

Licences: winetricks is LGPL-2.1-or-later
(<https://github.com/Winetricks/winetricks>); cabextract is GPL-3.0-or-later
and libmspack LGPL-2.1, both from the Ubuntu 22.04 packages of the same name
(sources: <https://www.cabextract.org.uk/>, <https://www.cabextract.org.uk/libmspack/>,
`apt-get source cabextract libmspack`).
