# Contributing

- Code and comments in English; UI strings in `src/lib/i18n/{de,en}.ts` (German is the default).
- Every change to `crates/lanlauncher-core` needs tests. Run `cargo fmt`, `cargo clippy -D warnings`, `cargo test`.
- Frontend: `npm run check`, `npm test`. Try changes in the browser mock (`npm run dev`) before `tauri dev`.
- New game manifests go to `manifests/<game_id>.toml`; name the source and the ETI package
  revision you tested with. See `docs/COMPATIBILITY.md`.
- Never commit `game.db` files with real Resilio keys, `.eti` archives or the Resilio binary.
