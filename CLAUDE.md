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
| `assets/covers/` | Mitgelieferte Cover (`<id>.jpg`) aus dem öffentlichen Repo eti-lan/LAN-Launcher (Public Domain). Aktualisieren mit `tools/update-covers.sh`, Upstream-Commit steht in `UPSTREAM`. Zur Laufzeit gewinnt der Cover-Cache aus `assets.eti` (`AppState::cover_dirs`). |
| `themes/`, `tools/dev-lanpage/` | Beispiel-Themes, lokale LANPage-Attrappe. |
| `screenshots/` | Bilder für die README, erzeugt aus der Browser-Attrappe: `npm run dev`, dann `node tools/screenshots.mjs` (Chromium über `PLAYWRIGHT_CHROMIUM`, falls schon eins da ist). |
| `src-tauri/icons/` | App-Icon. `app-icon.png` ist die Quelle, aus der alle PNG-Größen stammen; `icon.ico` und `icon.icns` sind die mitgelieferten Mehrgrößen-Dateien. Die Kopfzeile nutzt `src/lib/assets/app-icon.png`, wenn die LANPage kein Logo liefert. |
| `.github/workflows/` | `ci.yml` (jeder Push: Core-Tests Linux + Frontend; volle 3-OS-Matrix und Installer nur bei PR, Push auf `main`, manuellem Start oder `[full-ci]` in der Commit-Nachricht; Tags baut `release.yml`), `release.yml` (Installer bei Tag `v*`), `resilio-lock.yml` (Hashes/Version für `resilio.lock.json`, optional für einen festen Build). |

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

Diese Liste kompiliert **keinen** `#[cfg(windows)]`-Zweig. Wer daran etwas ändert, prüft
zusätzlich wie unter „Windows-Code hier prüfen“ beschrieben, sonst bricht erst die CI.

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
  `<XDG_DATA_HOME>/xyz.nextgen-lan.launcher/logs/launcher.log` (Windows:
  `%LOCALAPPDATA%\xyz.nextgen-lan.launcher\logs\launcher.log`, macOS:
  `~/Library/Logs/xyz.nextgen-lan.launcher/`; das ist Tauris App-Log-Ordner, nicht der
  Roaming-Datenordner mit `settings.json`). Die Statusleiste zeigt `v<version> (<commit>)`.

## Bekannte Stolperfallen

- **Windows-Langform (`\\?\C:\…`):** Tauris `resource_dir()` liefert sie, `canonicalize()` auch,
  und jeder daraus gebaute Pfad erbt das Präfix. Windows selbst nimmt es, `netsh` nicht: Die
  Firewall-Reparatur scheiterte damit auf jedem installierten Build mit Exit-Code 1. `resource_dir`
  und der gefundene Resilio-Pfad laufen deshalb durch `paths::strip_verbatim`; jeder weitere Pfad,
  der den Launcher Richtung externes Werkzeug verlässt, gehört ebenfalls dort hindurch.
- **Ausgabe erhöhter Läufe:** Der Admin-Prozess hat seine eigene Konsole, `.output()` sieht davon
  nichts. Hilfsbefehle laufen über `elevate::write_batch_logged` und hängen ihre Ausgabe an eine
  Protokolldatei; `elevate::last_section` liest daraus die Meldung des Befehls, der den Exit-Code
  bestimmt hat. Zeilen, deren Ausgabe der Benutzer sehen muss, sind
  `elevate::BatchLine::console` — `game_setup.cmd` gibt Hinweise aus und wartet auf einen
  Tastendruck. Die Marker der Zeile landen trotzdem im Protokoll, sonst zitiert die Meldung den
  Befehl davor.
- **Mehrzeilige Admin-Läufe:** Alle Zeilen laufen, der Rückgabewert ist der *erste* Fehlschlag
  (`NLL_RC` im erzeugten Batch). Zeilen, die scheitern dürfen, sind `elevate::BatchLine::optional`
  — `netsh … delete rule` endet mit 1, wenn keine Regel passte. Neue Zeilen ohne diese
  Kennzeichnung gelten als Pflicht.
- **Weißes Fenster auf Linux — nicht raten:** Vier Releases lang wurde je eine Vermutung
  ausgeliefert und auf dem Steam Deck getestet; keine hat getroffen. Hier ist das Problem nicht
  reproduzierbar (es braucht einen echten Compositor, unter Xvfb zeichnet WebKitGTK). Deshalb gibt
  es die Leiter in `crates/lanlauncher-core/src/graphics.rs`: eine geordnete Liste von
  Renderer-Einstellungen, die der Launcher selbst durchprobiert. Meldet sich die Oberfläche nicht
  (`FRONTEND_READY`, 30 s beim ersten Start, danach 15 s), startet er sich auf der nächsten Sprosse
  neu; die Sprosse, die zeichnet, landet in `~/.config/xyz.nextgen-lan.launcher/graphics.json`.
  **Neue Erkenntnisse gehören als Sprosse in diese Liste, nicht als neuer Sonderfall in
  `lib.rs`.** Zwei Details, die beim Ändern leicht kaputtgehen: (1) Der Neustart stellt *alle*
  Namen aus `graphics::touched_names()` auf ihren Wert von vor dem Launcher zurück und lässt erst
  dann das Kind die neue Sprosse anwenden — sonst erbt `native` (die Sprosse, die nichts setzt) die
  Einstellungen der Sprosse davor. (2) Ein Wert, den der Benutzer selbst gesetzt hat, bleibt
  stehen; Ausnahme ist `GDK_BACKEND`/`GTK_THEME` unter einem AppImage, denn die setzt der
  linuxdeploy-Hook ungefragt (`is_a_choice`).
- **Wayland-Bibliotheken im AppImage (halb mitgeliefert):** Die AppImage-Ausschlussliste
  (`pkg2appimage/excludelist`) nennt `libwayland-client.so.0`, `libEGL`, `libGL`, `libgbm`,
  `libdrm` — die kommen also vom Rechner. `libwayland-server.so.0`, `libwayland-egl.so.1` und
  `libwayland-cursor.so.0` stehen **nicht** darauf und werden von `libwebkit2gtk-4.1` bzw.
  `libgdk-3` hereingezogen, landen also im Image. Ergebnis: `libwayland-client` vom Rechner,
  `libwayland-server` aus dem Image — obwohl beide aus demselben Paket stammen und zusammen
  versioniert sind. Mesas `libEGL_mesa` bindet `libwayland-server`; passt die Version nicht,
  scheitert schon das Laden, und zwar **unabhängig von `EGL_PLATFORM` und von
  `LIBGL_ALWAYS_SOFTWARE`**. Genau dieses Muster zeigt das Deck-Log (Sitzung Wayland, Backend x11,
  EGL-Plattform x11, Software-GL — und trotzdem „Could not create default EGL display:
  EGL_BAD_PARAMETER"). An einem hier gebauten Abbild nachgemessen: Es sind **alle vier**
  Wayland-Bibliotheken drin, `libwayland-client` eingeschlossen — und die ist der einzige Eintrag
  der ganzen Ausschlussliste, den Tauris eigener linuxdeploy-Build falsch behandelt.
  `tools/appimage-unbundle-wayland.mjs` entfernt alle vier nach dem Bundling wieder und bricht ab,
  wenn es weniger als vier findet (sonst ginge die Nichtübereinstimmung unbemerkt wieder mit raus).
  **Das war aber nicht die Ursache des weißen Fensters** — das war `WEBKIT_DISABLE_DMABUF_RENDERER`,
  siehe unten. Der Schritt bleibt, weil die Ausschlussliste recht hat (Mesa und die Wayland-
  Bibliotheken, gegen die es gelinkt ist, gehören demselben Rechner), nicht weil er ein Symptom
  geheilt hätte.
  `ci.yml` und `release.yml` rufen es auf. In `release.yml` sind Bauen und Hochladen dafür getrennt
  (`tauri-action` baut, das Skript läuft, `softprops/action-gh-release` lädt hoch); ein manueller
  Lauf legt die Installer als Workflow-Artefakt ab, statt aus einem Branch-Namen ein Release zu
  machen. Prüfen lässt sich das hier nur strukturell (die Dateien sind weg, das Abbild startet und
  zeichnet weiterhin); die Upload-Hälfte von `release.yml` ist ungetestet, ein Tag lässt sich von
  hier nicht schieben.
- **`WEBKIT_DISABLE_DMABUF_RENDERER=1` war die Ursache, nicht die Lösung — auf dem Steam Deck
  bestätigt.** Das Gerät zeichnet auf der Sprosse `native`, also mit **gar nichts** erzwungen
  (`graphics.json`: `{"good":"native"}`, Logzeile: `forced []`, Sitzung Wayland, Backend x11,
  EGL-Plattform auto). Vier Builds lang wurde die Variable unbedingt auf jedem Linux gesetzt, weil
  sie die Mehrheit der weißen GTK-Webviews repariert — auf diesem Gerät hat sie das weiße Fenster
  *erzeugt*. Der Abbruch „Could not create default EGL display" ist die Anforderung, die der Pfad
  **ohne** DMA-BUF-Renderer stellt; jeder Folge-„Fix" hat also Einstellungen auf die eigentliche
  Ursache gestapelt, und `--safe-graphics` machte es schlimmer statt besser. Die Sprosse bleibt an
  erster Stelle der Leiter (sie hilft der Mehrheit), aber sie ist eine Vermutung, kein Gesetz. **Nie
  wieder eine solche Einstellung unbedingt erzwingen — sie gehört als Sprosse in die Leiter, wo ein
  Rechner, dem sie nicht bekommt, in etwa einer Sekunde daran vorbeikommt.**
- **Die Lehre aus fünf Runden:** Vier Releases lang wurde je eine Hypothese ausgeliefert und auf dem
  Deck getestet; keine hat getroffen, jede kostete eine Runde. Was es gelöst hat, war nicht die
  fünfte Vermutung, sondern zwei Mechanismen: die Leiter (der Rechner probiert selbst durch) und das
  Mitlesen der Standardfehlerausgabe (`webview.log`, plus sofortiger Sprossenwechsel bei
  `graphics::looks_fatal`). Bei einem Problem, das hier nicht reproduzierbar ist, ist der Bau eines
  Suchmechanismus billiger als die nächste Vermutung.
- **Terminal-Meldungen des Webviews:** Der Web-Prozess schreibt sie auf Standardfehler, nicht ins
  Tauri-Log; ein über Desktop-Symbol oder Steam gestarteter Launcher hat kein Terminal, und genau
  deshalb musste bisher jeder Bericht von Hand aus einer Shell wiederholt werden.
  `keep_what_the_webview_says` in `src-tauri/src/lib.rs` hängt Standardfehler per `dup2` an
  `<Log-Ordner>/webview.log` — außer auf einem TTY, dort bleibt die Ausgabe sichtbar. Läuft nach
  `prefer_a_renderer_that_draws` (die Kopfzeile nennt die Sprosse) und vor dem Tauri-Builder.
- **Umgebung beim Spielstart:** Was der Launcher für sein eigenes Fenster setzt, steht als JSON in
  `NLL_FORCED_ENV` (`launch::FORCED_ENV`, Name → vorheriger Wert) und wird beim Spielstart wieder
  hergestellt — sonst läuft ein Spiel mit `LIBGL_ALWAYS_SOFTWARE=1` auf der CPU. Unter einem
  AppImage entfernt `without_appimage_paths` zusätzlich alle Pfade unter `$APPDIR`, sonst lädt das
  Spiel die Bibliotheken aus dem Abbild. `prefer_a_renderer_that_draws` läuft vor dem Log-Plugin:
  dort loggen bringt nichts, die `webview:`-Zeile beim Start sagt, was gilt.
- **Vorbelegte Zieldatei:** Resilio legt die Datei sofort in voller Größe an und füllt sie dann.
  Der Ordner meldet also 85 GB, während acht Byte angekommen sind. Nur die Zahl der Engine taugt
  als Fortschritt, sobald sie die Freigabe eingelesen hat (`bytes_total` passt dann zur
  Katalogangabe); vorher ist der Ordner die bessere Quelle. Vor dem Prüfen muss die Engine
  bestätigen, dass der Großteil da ist (`Observation::bytes_on_disk`, `engine_mostly_done`).
- **NSIS-Hooks:** `NSIS_HOOK_PREINSTALL` läuft *vor* Tauris Frage „Anwendung beenden?“. Wer dort
  die Sync-Engine beendet, während der Launcher noch läuft, startet sie nur neu — deshalb beendet
  `installer-hooks.nsh` erst den Launcher, dann die Engine.
- **Pfade von der Engine:** Resilio antwortet mit der Windows-Langform, auch für Ordner, die ohne
  sie angemeldet wurden. `transport::normalise_dir` entfernt das Präfix, sonst findet der
  Zustandsautomat die Freigabe des Spiels nicht.
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
- **Icon-Cache von Windows:** Die EXE trägt `icons/icon.ico` als Ressource 32512 (zu sehen in der
  von `tauri-build` erzeugten `resource.rc` unter `target/<target>/*/build/nextgen-lan-launcher-*/out/`).
  Zeigt Windows nach einem Update trotzdem das alte Icon, liegt das am Cache pro Pfad, nicht am
  Build: Die Vorschau liest die Datei direkt und zeigt das neue. `src-tauri/installer-hooks.nsh`
  ruft nach der Installation `SHChangeNotify`; von Hand hilft `ie4uinit.exe -show`.
- **Windows-Bundle-Größe:** `webviewInstallMode: offlineInstaller` bettet den WebView2-Installer
  (~150 MB) ein, damit die Installation auf einer LAN ohne Internet klappt; `embedBootstrapper`
  wäre 1,8 MB, lädt WebView2 aber bei Bedarf herunter. `bundle.targets` nennt für Windows nur
  `nsis` (kein MSI), sonst liegt der Installer doppelt im Artefakt. `tauri.conf.json` verträgt keine
  unbekannten Schlüssel (auch keine `_comment`).
- **Tauri:** `tauri.conf.json` aktiviert das Asset-Protokoll (Cover-Bilder), daher braucht die
  `tauri`-Abhängigkeit das Feature `protocol-asset`. `bundle.resources` ist eine Map
  (`../manifests/` → `manifests/`, `../assets/covers/` → `covers/`); ein Glob auf leere Ordner
  bricht den Build ab. Der Ordner mit den gebündelten Covern wird beim Start per
  `asset_protocol_scope().allow_directory` freigegeben, die statische Scope umfasst nur die
  App-Datenordner.
- **Resilio-Binärdatei** liegt nie im Repo (`.gitignore`). `release.yml` und die volle CI-Matrix
  laden sie per `tools/fetch-resilio.mjs` nach `src-tauri/resources/resilio/`; unter Windows ist
  der Download das Programm selbst und wird in `Resilio Sync.exe` umbenannt. Der Launcher
  bevorzugt diese Kopie vor Systeminstallationen und startet sie mit `/noinstall /config …`
  (kein Silent-Install). Das Skript verweigert Downloads ohne SHA-256 in
  `resilio.lock.json`. Pin-Prozess: Workflow „Resilio lock“ manuell starten → Artefakt
  `resilio.lock.proposed.json` prüfen → über `resilio.lock.json` kopieren → committen. Der Workflow
  nimmt optional einen Build (`version`, z. B. `2.8.1.1390`) statt `stable`; ab 3.0 verlangt die
  kostenlose Lizenz ein Resilio-Konto, der ETI-Sync-Server läuft mit 2.8.1.1390. Die leichten
  Push-Läufe von `ci.yml` bauen keine Installer; volle Läufe und Releases enthalten Resilio. Das
  Resilio-CDN ist aus der Claude-Sandbox nicht erreichbar, Hashes lassen sich nur auf
  GitHub-Runnern ermitteln.
- **Resilio-Konfiguration:** `ResilioConfig::to_json` spiegelt die `config.json` des ETI-Launchers
  (gleiche Schlüssel, eigene Werte; Test `config_uses_only_keys_eti_ships`). Unbekannte Schlüssel
  lassen Resilio 2.8.1 mit Exit-Code 1 abbrechen, bevor die API antwortet. Neue Schlüssel nur mit
  Nachweis, dass Resilio sie akzeptiert. Der Resilio-API-Key steht nicht im Repo: Reihenfolge
  Einstellung → installierter ETI-Client (`%ProgramFiles%\eti\LAN Launcher\sync\config.json`) →
  `resilio_api_key` in `launcher.ini` der LANPage → ohne Key Web-UI-Endpunkte.
- **Katalog-Key:** `BUILTIN_CATALOG_KEY` in `crates/lanlauncher-core/src/catalog.rs` ist der
  öffentliche Read-only-Key von `eti_launcher` aus ETIs `sync_server.tar`
  (`/root/eti-config.conf`, `eti_call`). Er ist für alle ETI-Clients gleich. Override für LANs mit
  eigenem Katalog über `settings.catalogKey`. Der Pfad „kein gültiger Key“ (Statusleiste
  „Server-Key fehlt“, Diagnose `catalog.key_missing`) ist mit dem eingebauten Key normalerweise
  unerreichbar und bleibt als Absicherung, falls die Konstante in einem Fork geleert wird.
- **Test-Fixtures:** `sample_game.rar`, `truncated_game.rar`, `demo_amongus.rar` und
  `assets_covers.rar` (Cover-Layout `assets/<id>.jpg`) wurden mit `rar a -ep1 -r -m5 -ma5` erzeugt. `*.eti` und `game.db` sind per `.gitignore` ausgeschlossen,
  damit nie echte Resilio-Keys oder Spielarchive committet werden.
- **Windows-Skripte** erwarten `%programfiles%\eti\lan launcher\unrar.exe` und `fnr.exe` sowie
  teils Adminrechte (`netsh`, `reg add HKLM`). Der Launcher selbst läuft ohne Adminrechte
  (Manifest `asInvoker`, `src-tauri/build.rs`; `NLL_REQUIRE_ADMIN=1` baut die ETI-Variante mit
  Abfrage beim Start). Elevation nur bei Bedarf über `launch::elevate` (Batch-Datei in
  `<data>/run/`, PowerShell `Start-Process -Verb RunAs` mit `-EncodedCommand`): `game_setup.cmd`
  samt der Firewall-Regeln aus `game_start.cmd` einmal beim Setup, Startskripte nur, wenn sie
  HKLM schreiben (`script_needs_admin`), Server-Skripte, Diagnose-Reparaturen, Programme mit
  eigenem Admin-Manifest (Fehler 740). Einstellung `allowElevation=false` unterdrückt jede Abfrage.
- **Prozesstabelle enthält Threads:** `sysinfo` listet unter Linux pro Thread einen Eintrag neben
  dem des Prozesses, und ein Thread trägt den Namen seines Prozesses (hier gemessen: 121 Einträge,
  davon 108 Threads). Wer die Tabelle nach „läuft so etwas?" durchsucht, macht aus einer
  Sync-Engine zwanzig — auf dem Steam Deck meldete die Diagnose 21 „fremde" `rslsync`-Prozesse mit
  lückenlosen PIDs, allesamt Threads der eigenen Engine. Ein `own_pid`-Ausschluss hilft nicht, jeder
  Thread hat eine eigene ID. **Die Tabelle immer über `resilio::real_processes(&sys)` durchlaufen,
  nie über `sys.processes()` direkt.** `ProcessRefreshKind::without_tasks` ist dabei nur eine
  Sparmaßnahme, kein Ersatz: gemessen wurden damit 80 statt 121 Einträge und 67 statt 108 Threads
  bei halber Scan-Zeit — ein Teil der Threads bleibt also drin. Regressionstest:
  `threads_named_like_a_sync_engine_are_not_taken_for_processes` erzeugt Threads namens `rslsync`
  und prüft die Differenz zwischen roher und gefilterter Tabelle (kein absoluter Wert, sonst
  scheitert er auf einem Rechner, auf dem wirklich Resilio läuft).
- **Clippy:** In `resilio.rs` müssen alle Items vor `mod tests` stehen (`items_after_test_module`).
- **Fehlertexte:** Tauri-Commands und Fix-Aktionen geben keine deutschen Sätze zurück, sondern Codes
  (`err.<name>` bzw. `msg.<name>`, optional mit `|detail`). Das Frontend übersetzt sie mit
  `userText()` aus `src/lib/i18n/index.ts`; neue Codes brauchen Einträge in `de.ts` und `en.ts`.
- **Nebenläufige Jobs:** Prüf-, Entpack- und Setup-Jobs tragen eine Generation. `tick()` verwirft
  Ergebnisse, deren Generation nicht mehr zum aktuellen Job passt (z. B. nach „Reparieren“ während
  einer laufenden Prüfung). Beim Ändern der Job-Logik diese Zuordnung beibehalten.
- **Windows-Skriptstart:** `cmd.exe /S /C "<script> …"` wird als *eine* rohe Zeichenkette übergeben
  (`LaunchPlan::raw_command_line`), weil die Argument-Escapes der Standardbibliothek die
  cmd-Quoting-Regeln brechen. Das gilt auch für `game_setup.cmd` (Setup-Hook in
  `src-tauri/src/lib.rs`), das denselben Vier-Argumente-Vertrag wie `game_start.cmd` bekommt.
- **Bestand übernehmen:** `adopt_existing` darf vorhandene Installationen nie neu entpacken
  (`local/` enthält Spielstände). Mit Receipt → `Queued` → Ready; ohne Receipt vergleicht
  `Action::Adopt` die Archivliste mit `local/` (`extract::matches_extracted`) und schreibt ein
  Receipt mit `adopted: true`. Nur „Reparieren“ prüft per CRC und entpackt neu.
- **Resilio-Suche:** `locate_binary_detailed` liefert alle geprüften Pfade; `build_transport` loggt
  sie. Windows-Kandidaten sind in `windows_candidates(WinEnv)` testbar, inklusive ETI-`btsync.exe`,
  PATH, anderen Profilen und Registry (`reg query`). Override: `settings.resilioBinary`.

## Linux-Build-Abhängigkeiten

```bash
apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf libssl-dev
```

Für End-to-End-Tests zusätzlich `xvfb imagemagick xdotool`, für Fixtures `rar unrar`.

## Windows-Code hier prüfen

Die Prüfliste oben kompiliert die `#[cfg(windows)]`-Zweige nicht; ein dort ungenutzter Import
bricht erst die CI. Einmal einrichten:

```bash
apt-get install -y gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64
rustup target add x86_64-pc-windows-gnu
mkdir -p /tmp/wininc
printf '#include <powrprof.h>\n' > /tmp/wininc/PowrProf.h
printf '#include <wbemidl.h>\n'  > /tmp/wininc/Wbemidl.h   # mingw-Header sind kleingeschrieben
```

Danach prüfen (Tests laufen so nicht, nur Clippy und Compiler):

```bash
export CXXFLAGS_x86_64_pc_windows_gnu=-I/tmp/wininc
cargo clippy -p lanlauncher-core --all-targets --target x86_64-pc-windows-gnu -- -D warnings
cargo clippy -p nextgen-lan-launcher --target x86_64-pc-windows-gnu -- -D warnings
```

Die CI baut mit MSVC, nicht mit mingw; kleine Unterschiede bleiben möglich, die Lints sind
dieselben.

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
