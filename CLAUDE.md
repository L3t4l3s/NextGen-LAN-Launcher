# CLAUDE.md – Betriebswissen für dieses Repository

Diese Datei richtet sich an Claude Code (und Menschen), die am NextGen LAN Launcher arbeiten.
Sie fasst zusammen, wie das Projekt gebaut, getestet und ausgeliefert wird und welche
Stolperfallen bereits bekannt sind.

## Pflichtregel vor jedem Push

**Vor jedem `git push` muss `/code-review` auf den anstehenden Änderungen ausgeführt werden.
Alle Findings werden behoben (oder mit Begründung im Commit dokumentiert, falls sie bewusst
nicht umgesetzt werden), bevor gepusht wird.** Danach müssen die Prüfungen aus dem Abschnitt
„Prüfungen vor einem Commit“ grün sein.

## Was das Projekt ist

Plattformübergreifender Nachfolger des ETI LAN Launchers (eti-lan.xyz). Kompatibel zum
bestehenden Ökosystem: Resilio-Sync-Shares als Transport, `game.db` als Katalog, `.eti`-Pakete
(RAR) mit `version.ini` und `game_start.cmd`, `launcher.ini`/`stats.php` der LANPage.
Details: `docs/ARCHITECTURE.md`, `docs/COMPATIBILITY.md`.

Kernprinzip, das nicht aufgeweicht werden darf: **Ein Spiel wird nur „spielbereit“, wenn das
Archiv auf der Platte per CRC geprüft, atomar entpackt und ein Receipt geschrieben wurde.**
Der Fortschrittswert des Sync-Engines ist reine Anzeige (`crates/lanlauncher-core/src/install.rs`).

## Aufbau

| Pfad | Inhalt |
|---|---|
| `crates/lanlauncher-core/` | Rust-Bibliothek ohne GUI. Alles Testbare gehört hierhin. |
| `src-tauri/` | Tauri-2-Schale: Commands (`commands.rs`), Fix-Aktionen (`fixes.rs`), Dienst-Schleifen (`lib.rs`). |
| `src/` | Svelte-5-Frontend (Runes). `src/lib/mock.ts` simuliert das Backend im Browser. |
| `manifests/` | TOML-Startprofile für macOS/Linux, eines pro ETI-Game-ID. |
| `themes/`, `tools/dev-lanpage/` | Beispiel-Themes, lokale LANPage-Attrappe. |
| `.github/workflows/` | `ci.yml` (Tests + Builds für Win/macOS/Linux), `release.yml` (Installer bei Tag `v*`). |

Sprachen: Code, Kommentare, `README.md` und `docs/` Englisch; UI-Texte in `src/lib/i18n/{de,en}.ts`
(Deutsch ist Standard); `CHANGELOG.md` Deutsch.

## Prüfungen vor einem Commit

```bash
cargo fmt --all --check
cargo clippy -p lanlauncher-core --all-targets -- -D warnings
cargo clippy -p nextgen-lan-launcher -- -D warnings
cargo test -p lanlauncher-core
npm run check          # svelte-check
npm test               # vitest
npm run build          # vite build
```

`cargo test` im Workspace-Root baut auch die Tauri-Schale; unter Linux braucht das die
WebKitGTK-Entwicklungspakete (siehe unten).

## Entwicklung

- `npm run dev` startet die Oberfläche im Browser mit simuliertem Backend (`http://localhost:1420`,
  `?wizard` zeigt den Einrichtungsassistenten). Damit lassen sich UI-Änderungen ohne Tauri prüfen.
- `npm run tauri dev -- -- --demo` startet die echte App im Demo-Modus (simulierter Sync, Katalog
  aus `src-tauri/demo/demo_catalog.sql`, Archiv aus
  `crates/lanlauncher-core/tests/fixtures/demo_amongus.rar`).
- `cargo run -p lanlauncher-core --release --example demo_pipeline` fährt die Install-Pipeline
  ohne GUI durch.
- Manuelle End-to-End-Prüfung unter Linux ohne Bildschirm: `Xvfb :99`, App mit `--demo` starten,
  mit `xdotool` klicken, mit `import -window root` (ImageMagick) Screenshots ziehen. Log liegt in
  `<XDG_DATA_HOME>/xyz.nextgen-lan.launcher/logs/launcher.log`.

## Bekannte Stolperfallen

- **Demo-Timing:** Der Demo-Download dauert 25 s, danach wartet der Zustandsautomat 15 s auf eine
  stabile Archivgröße (`Policy::stable_for`), erst dann folgen Prüfen, Entpacken, Setup. Ein
  Test, der früher als ~45 s nach „Installieren“ abbricht, sieht fälschlich einen „Hänger“.
- **Logging:** Die Kernbibliothek loggt über das `log`-Crate; `tracing` wird nicht ins Tauri-Log
  gebrückt. Neue Log-Zeilen im Core immer mit `log::…` schreiben.
- **Vite 8:** `minify: "esbuild"` schlägt fehl (esbuild ist nicht mehr enthalten). `minify: true`
  nutzt den eingebauten Minifier. `vite.config.ts` importiert `defineConfig` aus `vitest/config`.
- **Rust-Toolchain** ist per `rust-toolchain.toml` und in den Workflows auf 1.98.1 gepinnt, damit CI
  und Entwickler dieselben Clippy-Lints sehen (eine neuere Clippy-Version hat die CI schon einmal
  mit einem hier unsichtbaren Lint gebrochen). Beim Anheben lokal `cargo +<version> clippy` prüfen.
- **sysinfo** ist auf 0.38 gepinnt, weil 0.39 einen neueren Rust-Compiler verlangt.
- **rand 0.10:** Der Trait heißt `RngExt`, nicht `Rng`.
- **Tauri:** `tauri.conf.json` aktiviert das Asset-Protokoll (Cover-Bilder), daher braucht die
  `tauri`-Abhängigkeit das Feature `protocol-asset`. `bundle.resources` ist eine Map
  (`../manifests/` → `manifests/`); ein Glob auf leere Ordner bricht den Build ab.
- **Resilio-Binärdatei** liegt nie im Repo. `release.yml` lädt sie per `tools/fetch-resilio.mjs`
  nach `src-tauri/resources/resilio/`. Hashes in `resilio.lock.json` sind noch nicht gepinnt.
- **Test-Fixtures:** `sample_game.rar`, `truncated_game.rar` und `demo_amongus.rar` wurden mit
  `rar a -ep1 -r -m5 -ma5` erzeugt. `*.eti` und `game.db` sind per `.gitignore` ausgeschlossen,
  damit nie echte Resilio-Keys oder Spielarchive committet werden.
- **Windows-Skripte** erwarten `%programfiles%\eti\lan launcher\unrar.exe` und `fnr.exe` sowie
  Adminrechte (`netsh`, `reg add HKLM`). Die App fordert per Manifest Elevation an
  (`src-tauri/build.rs`, abschaltbar mit `NLL_NO_ELEVATION=1`).
- **Clippy:** In `resilio.rs` müssen alle Items vor `mod tests` stehen (`items_after_test_module`).
- **Fehlertexte:** Tauri-Commands und Fix-Aktionen geben keine deutschen Sätze zurück, sondern Codes
  (`err.<name>` bzw. `msg.<name>`, optional mit `|detail`). Das Frontend übersetzt sie mit
  `userText()` aus `src/lib/i18n/index.ts`; neue Codes brauchen Einträge in `de.ts` und `en.ts`.
- **Nebenläufige Jobs:** Prüf-, Entpack- und Setup-Jobs tragen eine Generation. `tick()` verwirft
  Ergebnisse, deren Generation nicht mehr zum aktuellen Job passt (z. B. nach „Reparieren“ während
  einer laufenden Prüfung). Beim Ändern der Job-Logik diese Zuordnung beibehalten.
- **Windows-Skriptstart:** `cmd.exe /S /C "<script> …"` wird als *eine* rohe Zeichenkette übergeben
  (`LaunchPlan::raw_command_line`), weil die Argument-Escapes der Standardbibliothek die
  cmd-Quoting-Regeln brechen.

## Linux-Build-Abhängigkeiten

```bash
apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf libssl-dev
```

Für End-to-End-Tests zusätzlich `xvfb imagemagick xdotool`, für Fixtures `rar unrar`.

## Was hier nicht geprüft werden kann

Echter Resilio-Betrieb gegen einen Sync-Server, Windows-Installer mit Resilio-Sidecar,
PowerShell-/netsh-Fixes sowie die Startprofile unter Wine/CrossOver. Änderungen an diesen Stellen
im Commit als „ungetestet, auf der LAN prüfen“ kennzeichnen und in `README.md` unter
„Offene Punkte“ nachhalten.

## Commits

Aussagekräftige Commit-Nachrichten in Englisch, die das *Warum* nennen. Keine Modellnamen im
Commit-Text, in Code oder Kommentaren; der von der Tooling-Umgebung angehängte
`Co-Authored-By`-Trailer ist davon ausgenommen. Niemals `game.db` mit echten Keys,
`.eti`-Archive oder die Resilio-Binärdatei einchecken.
