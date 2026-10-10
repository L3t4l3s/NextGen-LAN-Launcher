# CLAUDE.md – Betriebswissen für dieses Repository

Diese Datei richtet sich an Claude Code (und Menschen), die am NextGen LAN Launcher arbeiten.
Sie fasst zusammen, wie das Projekt gebaut, getestet und ausgeliefert wird und welche
Stolperfallen bereits bekannt sind.

## Pflichtregel vor jedem Push

**Vor jedem `git push` muss ein Review der anstehenden Änderungen ausgeführt werden:
Claude Code verwendet `/code-review`, Codex verwendet `/review`.
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
| `src/` | Svelte-5-Frontend (Runes). `src/lib/mock.ts` simuliert das Backend im Browser (Chat: `mock-chat.ts`). |
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
  `<XDG_DATA_HOME>/xyz.nextgen-lan.launcher/logs/launcher.log`, ältere Teile als
  `launcher_<Datum>.log` (2 MB je Datei, drei bleiben; Tauris Standard war eine Datei mit 40 kB,
  die ein langes Setup in Minuten mit `tick`-Zeilen füllte) (Windows:
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
- **Farb-Emojis im AppImage (COLRv1):** Das AppImage bringt WebKitGTK samt Skia vom Build-Rechner
  mit, FreeType und Schriften kommen vom Zielrechner. Fedora 44/Bazzite liefert
  `Noto-COLRv1.ttf`; ein Emoji mit Farbverlauf (😀, Emoji-Menü im Chat) löst in Skia
  `colrv1_configure_skpaint … Assertion '__n < this->size()' failed` aus (`webview.log`), der
  Web-Prozess stirbt, das Fenster bleibt leer. Auf der VM nachgestellt (eigenes Xwayland-Display
  `Xwayland :98 -geometry …` plus abgeschottete Kopie mit eigenen `XDG_*`-Ordnern, Klick aufs
  „+“) und mit einer Fontconfig-Datei ohne diese Schrift gegengeprüft. `fonts.rs` findet
  COLR-Version ≥ 1 (Tabellenverzeichnis, auch `.ttc`), `leave_out_fonts_that_crash_the_webview`
  schreibt nur unter `APPDIR` eine Fontconfig-Datei, die die Sitzungskonfiguration einbindet
  (`fonts::session_config`: `FONTCONFIG_FILE`, `FONTCONFIG_PATH`, `/etc/fonts`; fehlt sie, wird
  nichts gesetzt — ein `include` ins Leere ließe das Fenster ganz ohne Schriften) und
  diese Dateien per `<pattern>` auf `file` ausschließt (Globs kennen kein Escape), und setzt
  `FONTCONFIG_FILE` über `force` (landet in `NLL_FORCED_ENV`, Spiele bekommen ihren Wert
  zurück). Die `webview:`-Zeilen nennen die ausgelassenen Dateien. Ob die Distributions-WebKit
  dasselbe Problem hat, ist offen. Dazu `reload_a_dead_web_process`: `web-process-terminated` →
  `reload()`, beim ersten Mal sofort, danach im Abstand von `RELOAD_SPACING` (30 s), höchstens
  `RELOAD_LIMIT` (3) Mal pro Lauf. Kandidaten liefert `fc-list :color=true` über
  `launch::host_command` mit der Sitzungskonfiguration (nur für diesen Aufruf gesetzt); eine
  leere Antwort heißt „keine Farbschriften“, nur ein fehlendes oder scheiterndes `fc-list`
  kostet den Durchlauf der Schriftordner. Bewusst keine Sprosse der Grafik-Leiter: Die Leiter
  erkennt ein Fenster, das nicht zeichnet; dieser Absturz kommt erst beim ersten solchen Emoji,
  oft Minuten später, und ist hier nachgestellt statt vermutet. Programme, die der Launcher
  ohne `host_command` startet (Browser über `open_url`), erben `FONTCONFIG_FILE` wie schon die
  `WEBKIT_*`-Variablen der Leiter.
- **Umgebung beim Spielstart:** Was der Launcher für sein eigenes Fenster setzt, steht als JSON in
  `NLL_FORCED_ENV` (`launch::FORCED_ENV`, Name → vorheriger Wert) und wird beim Spielstart wieder
  hergestellt — sonst läuft ein Spiel mit `LIBGL_ALWAYS_SOFTWARE=1` auf der CPU. Unter einem
  AppImage entfernt `without_appimage_paths` zusätzlich alle Pfade unter `$APPDIR`, sonst lädt das
  Spiel die Bibliotheken aus dem Abbild. `prefer_a_renderer_that_draws` läuft vor dem Log-Plugin:
  dort loggen bringt nichts, die `webview:`-Zeile beim Start sagt, was gilt.
- **`size` von `get_folders`:** zählt nur *fertige* Dateien. Ein Spiel, dessen einziges großes
  Paket noch lädt, meldet die acht Byte der `version.ini` daneben — für den ganzen Download.
  `ShareStatus` trennt das deshalb: `bytes_done` ist die Zahl der Engine (fertige Dateien) und
  entscheidet zusammen mit `finished_known`, ob geprüft werden darf; die Web-UI kann „fertig“ nicht
  beantworten und setzt `finished_known = false`, dann entscheidet ihre Prozentzahl.
  Der Fortschritt kommt bevorzugt aus `getsyncfolders.down_status` (0–100, auch Nachkommastellen),
  genau wie in der Web-UI von Resilio 2.8.1.1390. Auch mit API-Key wird diese Zahl gelesen;
  API-Rate und fertige Bytes bleiben getrennt erhalten. Ohne Web-UI dienen nur die Zuwächse
  der Peer-Zähler als Rückfall, deren erste Messung ist eine Baseline. Fertige Dateien und
  alte Sitzungszähler dürfen keinen Anfangsfortschritt erzeugen. `down_speed > 0` gilt im
  Zustand `Downloading` als Lebenszeichen, nicht beim Indizieren. Eine gerundete Prozentzahl
  kann bei großen Paketen länger als zwei Minuten gleich bleiben, obwohl Daten ankommen.
- **Setup-Skripte unter macOS/Linux:** `game_setup.cmd` läuft nach dem Entpacken im Prefix des
  Spiels, mit Wines eigener `cmd.exe` und demselben Runner wie der Spielstart
  (`launch::unix::setup_script_plan`, gemeinsamer Teil `wine_plan`). Kein eigener cmd-Nachbau:
  Wine kann `set`/`if`/`for`/`%~dp0`/`reg add`. `launch::setup_script::filter` macht aus Zeilen,
  die ein Prefix nicht braucht oder die hängen würden, `rem`-Zeilen und nennt sie im Log
  (`netsh`, `dism`, `taskkill`, `pause`/`timeout`/`choice`, Programme außerhalb des Spielordners).
  Gesperrte Programme und Programme außerhalb des Spielordners zählen dort, wo ein Befehl
  beginnt: am Zeilenanfang, nach `&`/`|` (ein `^` davor zählt nicht), nach einer Blockklammer,
  nach `do`/`else` (`for /f … do taskkill`, `)else(pause`) – nicht als Argument (`set choice=1`,
  `reg add … /v Timeout`, `goto pause`, der Pfad eines `if exist`). Die Klammern von
  `%ProgramFiles(x86)%` sind keine Blockklammern. Eine Zeile ohne Blockklammern wird ganz zur
  `rem`-Zeile; in einer Zeile mit ihnen wird nur der gesperrte Einzelbefehl bis zum nächsten
  `&`/`|` durch `ver>nul` ersetzt (`neutralize`), so bleibt jeder Block heil und ein verketteter
  `& reg add` erhalten. Ein Versuch
  mit Platzhalter-`.cmd`-Dateien vorn im `PATH` scheiterte: eine Batchdatei, ohne `call` aus einer
  anderen gestartet, kehrt nie zurück und beendet das Skript (mit Proton 11 nachgestellt). In den
  `rem`-Text kommen keine eigenen Klammern (Pfade in Anführungszeichen). Die gefilterte Kopie
  liegt als `.nll-setup.cmd` im Spielordner, damit `%~dp0` stimmt; `.nll-setup-run.cmd` ruft sie
  mit ETIs vier Argumenten auf, ohne `call` (`call` expandiert `%` ein zweites Mal und verdoppelt
  `^`, ein Pfad mit `100%` ging verloren; mit Proton 11 nachgestellt). `cmd.exe /c` bekommt den Wrapper beim Namen, aus dem Spielordner
  als Arbeitsverzeichnis: ein absoluter Pfad mit `(`/`&` („Games (LAN)“) verliert bei `/c` seine
  Anführungszeichen und wird nicht gefunden (mit Proton 11 nachgestellt). Pfade und Spielername gehen als Umgebungsvariablen
  (`NLL_SETUP_SCRIPT`, `NLL_GAME_PATH`, `NLL_PLAYER`) hinein, nicht in die Datei: cmd liest
  Batchdateien in der Konsolen-Codepage, ein `ä` im Pfad wurde dort zu Mojibake und das Skript
  lief gar nicht (mit Proton 11 nachgestellt). CD-Keys stehen in den Skripten im Klartext; sie
  bleiben auf dem Sync-Server und gehören nie in ein Profil oder einen Test. `Receipt::script_setup_prefixes` vermerkt das Setup pro Prefix (`launch::unix::prefix_id`:
  Bottle-Name bzw. aufgelöster `STEAM_COMPAT_DATA_PATH`/`WINEPREFIX`), nicht pro Installation: eine
  gepinnte Proton-Version bekommt einen eigenen, leeren Prefix, und auch der braucht die Keys.
  Leer in Receipts von vor dieser Version (macOS/Linux hatten `setup_done` gesetzt, ohne das Skript
  auszuführen) und in übernommenen ETI-Installationen; `wine_setup::catch_up` holt das Setup vor
  dem Start nach, für den Prefix des Startplans. Vermerkt wird in `Started::wait`, bevor die
  Sperre fällt. Unter Windows wird das Feld nie gesetzt (eine mit Linux
  geteilte Bibliothek bekäme sonst nie ein Wine-Setup). Ein laufendes Setup belegt das Spiel
  (`prefix_use.busy` und `setups`: winetricks wartet, Start, Reparatur und Deinstallation melden
  „Einrichtung läuft noch“, siehe `PrefixUse::mark_busy`), startet nicht in ein laufendes Spiel
  (`prefix_in_use`) und nicht, solange winetricks im selben Prefix arbeitet. Nach dem Zeitlimit
  bleibt die Sperre bis zum Prozessende stehen, das Spiel gilt bis dahin nicht als eingerichtet;
  ebenso, wenn der Installationsjob abgebrochen wird (`Drop` für `wine_setup::Started`). Endet
  der Prozess dann doch, wird das Setup nachträglich vermerkt. Ein Setup, das nach der Installation gar nicht laufen kann (kein
  Runner), ist keine Setup-Warnung; der nächste Start holt es nach. Vor dem Start hält nur ein
  beschäftigter Prefix den Start auf (auch ein wiederholtes Setup); ein
  Setup, das gar nicht laufen kann (kein Runner, Ordner nicht schreibbar), blockiert ein
  startbares Spiel nicht. Ein natives Spiel wird nicht markiert und bekommt keine Dateien; wird es
  später auf Wine umgestellt, läuft sein Setup dann. Die Diagnose-Seite zeigt ein Setup nach
  Installation oder vor dem Start nur, wenn es nicht starten oder nicht rechtzeitig enden konnte
  (ein Exit-Code ≠ 0 sagt unter Wine wenig, er steht im Log), „Setup wiederholen“ immer.
- **Startskripte unter macOS/Linux:** `launch::unix::starts_through_script` entscheidet: Stammt
  die Startdatei nur aus `game_start.cmd` (`exe_from_script`, oder kein Profil) und hat niemand
  gewählt (keine Alternative, kein `exe_override`, keine eigene Konfiguration), startet das ganze
  gefilterte Skript (`Script::Start`, `.nll-start*.cmd`), sonst die Startdatei nach der
  Vorbereitung (`Script::Preparation`: der Teil vor der Zeile, die eine Startdatei aus den
  Firewall-Regeln oder die des Profils aufruft, `setup_script::preparation`; mit `set /p`,
  `goto`/Sprungmarke oder offenem Block keine). Kein `start "…" /wait`: Proton gibt `cmd.exe`
  ohnehin ein Konsolenfenster, und ein Menü wird dort beantwortet, auch mit `/dev/null` als
  Eingabe (mit Proton 11 und einer echten Eingabe geprüft); `start` ergab zwei Fenster.
  `fnr.exe` (Kopie des alten ETI-Launchers oder im Spielordner) wird im Filter zu `"%NLL_FNR%"`;
  der Launcher schreibt `launch::fnr` (Shell + Perl) nach `<data>/tools/fnr` und setzt die
  Variable auf dessen `Z:`-Pfad. Wine startet ein Host-Programm über einen solchen Pfad und
  wartet darauf (Exit-Code geht verloren) — aber nur mit einer Endung außer
  `.com/.exe/.bat/.cmd` (ohne Endung sucht cmd genau diese und findet nichts), daher
  `nll-fnr.sh`. `$0` ist dort ein Pfad über `dosdevices/z:`. Proton gibt sein `LD_LIBRARY_PATH`
  mit, das Skript startet Perl ohne. Verhalten wie fnr: ohne Groß-/Kleinschreibung, `^`/`$` pro
  Zeile, `.` nimmt `\r` mit, `Name=.*` trifft auch `ServerName=` — die Pakete vom Sync-Server
  tragen genau diese Spuren eines Windows-Laufs, also nicht „verbessern“. Eine Datei behält ihre
  Kodierung (BOM UTF-16/UTF-8, sonst gültiges UTF-8 jenseits ASCII, sonst Windows-ANSI — auch
  reines ASCII, denn dafür sind die Spiele geschrieben): ein „Jürgen“ landet in einer ANSI-Ini
  als `\xFC`, nicht als UTF-8. Fehlt Perl, warnt das Log einmal. `starts_through_script_for`
  ist dieselbe Regel für „Startdatei wählen“ (`needs_exe_choice`); ein Profil ohne Startdatei
  startet über das Skript. Die Vorbereitung belegt das Spiel wie ein Setup (`SetupClaim`, vom
  Start gehalten) und hält den Start nach 2 Minuten mit `err.prepare_timeout` an, die Belegung
  bleibt bis zum Prozessende. Startplan und Vorbereitung kommen aus einer Auflösung
  (`build_start`, jede sucht alle Steam-Bibliotheken ab). Die Skriptdateien werden nur bei
  geändertem Inhalt neu geschrieben (`write_if_changed`): cmd liest per Offset weiter, ein
  zweiter Start darf die Datei des ersten nicht verschieben. Fragt ein Skript die Auflösung per
  `wmic path Win32_VideoController get CurrentHorizontalResolution,… /format:value` ab (AoM
  Titans), setzt `with_screen_query` vor die erste Zeile dieselbe Abfrage in Tabellenform
  (nur beim ganzen Startskript): Wines `wmic` kennt `/format:value` nicht („Ungültige Anfrage“), die Schleife setzt dann
  nichts, und das Spiel bekam `xres=`. Die Tabellenform liefert, was das Spiel sieht; die
  bevorzugte Auflösung des Bildschirms aus `/sys/class/drm` war auf einem kleineren Desktop
  falsch (nachgestellt: 1280×800 statt 1280×731, „Initialization Failed“).
- **Spielername und Sprache (`player_settings`):** vor jedem Start, auf allen Plattformen,
  zusätzlich zum ETI-Skript (`player::apply` in `play_game`, nach Setup/Vorbereitung, vor dem
  Spawn). `[[settings]]` im Profil: INI-Schlüssel im Abschnitt, `line = "seta name"`, ganze Datei,
  Registry (Windows `reg add`, sonst `Script::Settings` im Prefix mit den Werten als Variablen
  `NLL_VALUE_n` — ein Name mit Umlaut in der Batchdatei käme in der Konsolen-Codepage an).
  Dateien werden in jeder Schreibweise gefunden und mitsamt Ordnern angelegt (CoD2 legt sein
  Spielerprofil sonst erst auf Nachfrage an); ein vorhandener Schlüssel behält Schreibweise und
  Abstände. Kodierung wie beim fnr-Ersatz, ASCII gilt als ANSI (Quake 3, UT2004 lesen Latin-1) —
  außer Goldbergs Dateien, die es als UTF-8 liest. Ohne Profil: Goldberg (DLL nennt
  `force_account_name.txt` → `steam_settings/force_*.txt`; auch als `steamclient(64)(.ccl).dll`
  hinter dem ColdClientLoader), gbe_fork (DLL nennt `configs.user.ini` → `[user::general]
  account_name/language`, UTF-8; nur wo dessen `configs.*.ini` liegen), älteres Goldberg ohne
  `force_*` (DLL nennt nur `account_name.txt` → nur `settings/`, AvP), RELOADED (`steam_rld.ini`
  `[Settings] UserName/Language`) und SmartSteamEmu (jede
  `SmartSteamEmu.ini`: `[SmartSteamEmu] PersonaName/Language`). Fehlt die Sprache in der Tabelle
  eines Profils, bleibt der Wert stehen. Der Ort, wo ein Spiel den Namen liest, steht oft woanders,
  als das ETI-Skript schreibt (UT2004 `User.ini`), also am Paket nachsehen, nicht abschreiben.
  Registry-Werte als Text oder `type = "dword"` (Zahl, beim Laden geprüft). `quote = false`
  schreibt eine Zeile `KEY Wert` ohne Anführungszeichen (Armagetron `user.cfg`). `xml = "element"`
  setzt den Text eines vorhandenen Elements (Groß-/Kleinschreibung egal, Anno schreibt
  `<LanguageTAG>…</LanguageTag>`); eine Datei ohne das Element bleibt, wie sie ist, und wird nie
  angelegt (AoM Titans: `<profilelanname>` in `Default.prf`, UTF-16). Lizenzdialoge alter
  Microsoft-Spiele schreiben beim Annehmen ein DWORD `FIRSTRUN=1` (AoE III unter
  `…\Age of Empires 3 Expansion Pack 2\1.0`, AoE II Conquerors unter `…\1.0\EULA`; bei AoE III
  beidseitig geprüft) — gesetzt vorab, erscheint der Dialog nicht, der sonst gern hinter dem
  Vollbild hängt. Herausfinden: `user.reg`/`system.reg` des Prefix vor und nach dem Klick
  vergleichen (der Wineserver schreibt die Dateien erst beim Beenden bzw. zeitverzögert).
- **Einstellungen im Windows-Benutzerordner:** `folder = "locallow"` usw. (`player_settings::Folder`)
  plus `json = "a.b"` (`set_json`, fehlende Ebenen werden angelegt, Nicht-JSON bleibt stehen, gleicher
  Wert schreibt nicht). Der Ordner ist `launch::windows_profile(plan)`: Windows `USERPROFILE`,
  Proton `<pfx>/drive_c/users/steamuser` (auch ohne Prefix bekannt, nicht über `winetricks::target`,
  das einen fertigen Prefix verlangt), CrossOver `<bottle>/drive_c/users/crossover` (ungetestet),
  Wine der einzige Benutzer im Prefix oder `USER`. Im Benutzerordner keine Link-Prüfung: Wine
  verlinkt `Documents` & Co. absichtlich ins Home (`set_at` statt `set_inside`). Fehlt der
  Prefix (nie gestartet), legt `player::windows_profile` ihn per leerem `Script::Settings`-Lauf an;
  dafür baut `build_start` den Registry-Plan auch bei `Manifest::uses_windows_profile`. Gefunden mit
  Among Us: `player.amogus` → `onboarding.privacyPolicyVersion = 4` (Zahl) und
  `customization.name`; mit einer von Grund auf angelegten Datei geprüft (kein Dialog, Name in der
  Lobby). Vor dem Überschreiben eines Profils in `manifests/` immer nachsehen, ob es schon eins gibt.
- **Argumente im Startskript (`[[script_args]]`):** Ein Profil ohne eigene Startdatei kann
  Programmen, die das Startskript *als Befehl* aufruft, Argumente anhängen
  (`setup_script::with_arguments`, gleiche Befehlsposition-Erkennung wie der fnr-Tausch,
  `command_spans`). Nur reine Wörter (`ScriptArgs::check`), sie landen in einer Batchdatei.
  Anlass: AoE II Classic spielt Intro-Videos in VP7, Wines MCI zeigt einen Fehler hinter dem
  Vollbild, das Spiel wartet — `NOSTARTUP` lässt die Videos aus, das Auswahlmenü bleibt.
  Ein Klick unter Wayland lässt sich hier nicht simulieren (`xdotool` erreicht Xwayland-Fenster
  der Sitzung nicht); ein eigenes `Xwayland :98 -geometry 1280x800 -decorate` mit
  `DISPLAY=:98` und ohne `WAYLAND_DISPLAY` nimmt Klicks an.
- **Setup-Skripte lesen:** `launch::windows::missing_paths` folgt einem ETI-Skript wie cmd:
  `set`-Variablen, `cd`/`pushd` (`%~dp0` = Spielordner), Programme relativ zum aktuellen Ordner,
  bloße Namen über den `PATH` (sonst gilt `reg.exe` als fehlend), `md` angelegte Ordner, `if`/
  `goto` machen das Arbeitsverzeichnis unbekannt. Einmalig gegen alle 32 `game_setup.cmd` aus dem
  öffentlichen Repo eti-lan/LAN-Launcher laufen gelassen (nicht im Repo, das Prüfgerüst war ein
  Wegwerf-Beispiel); im Test steht ein Skript, das deren Formen zusammenfasst.
- **Vorbelegte Zieldatei:** Resilio legt die Datei sofort in voller Größe an und füllt sie dann.
  Der Ordner meldet also 85 GB, während acht Byte angekommen sind. Nur die Zahl der Engine taugt
  als Fortschritt. Bei verwaltetem Resilio wird deshalb auch beim Indizieren und während
  API-Ausfällen nie auf Dateigrößen zurückgefallen. Die Kataloggröße ist eine Schätzung und
  darf einen echten Resilio-Prozentwert nicht verzerren. Vor dem Prüfen muss die Engine
  bestätigen, dass der Großteil da ist (`Observation::bytes_on_disk`, `engine_mostly_done`).
- **Beschäftigte Engine:** Schreibt Resilio einen großen Download, beantwortet seine API mitunter
  eine halbe Minute lang nichts (auf der VM nachgestellt: 17-GB-Archiv, Schreib-Thread in
  `rq_qos_wait`, Listen-Backlog voll, danach wieder normal). Eine Zeitüberschreitung ist
  `Error::NoAnswer` (`From<reqwest::Error>` per `is_timeout`), verweigerte Verbindungen und
  Fehlerstatus bleiben `Error::Http`. `health` meldet `TransportActivity::Busy`, solange der Prozess
  läuft und die letzte Antwort weniger als `BUSY_GRACE` (2 min) zurückliegt (`start`/`stop` setzen
  das zurück); `api_reachable` bleibt dabei ehrlich `false`, Statusleiste („Resilio ist
  beschäftigt …“) und Diagnose (`transport.busy`, Info) prüfen `Busy` vorher. Danach greift
  `transport.api_unreachable` mit „Neu starten“. Bekommt `install`, `repair` oder
  `adopt_existing` beim Anmelden der Freigabe `NoAnswer`, setzt der Tracker `share_retry_at`; `tick`
  meldet höchstens eine solche Freigabe pro Durchlauf an, sobald `list_shares` antwortet (listet die
  Engine sie schon, kam die Anfrage an und nur die Antwort nicht), und nimmt sie zurück, wenn das
  Spiel inzwischen abgebrochen wurde. Eine echte Ablehnung beim Nachholen macht die Installation
  zu `Failed` mit `sync.share_error` und „Reparieren“. Lücke: `share_retry_at` lebt nur im
  Speicher; ein Neustart von Engine oder Launcher, bevor die Freigabe ankam, vergisst die
  Installation (der leere Ordner gilt dann als nicht installiert), der Spieler klickt erneut.
- **Resilio-Identität:** Die Web-UI von 2.8.1.1390 verwendet `setuseridentity&username=…`,
  danach `getmasterfolder` und nur bei fehlendem Schlüssel `setmfsecret`. Diese Folge wurde
  mit einem separaten Windows-Testprofil ausgeführt. Vorher `useridentity` lesen: Der
  Endpunkt verweigert Änderungen bestehender Identitäten mit „Can not apply identity“.
  Deshalb wird nur eine fehlende Identität angelegt, keine bestehende ersetzt. Wenn der
  Einrichtungsassistent den Spielernamen speichert, startet die verwaltete Engine neu.
  Web-UI-Login und Identität sind getrennt.
- **Spieleordner:** Einstellungen veröffentlichen die Bibliotheksliste sofort. Ein
  `.nll-download`-Marker hält die gewählte Wurzel fest, wenn ein alter leerer `.sync`-Ordner
  noch gesperrt ist. Unfertige Downloads dürfen bei Platzmangel auf eine passende Wurzel
  wechseln; `local/` und Receipts sperren installierte Spiele gegen automatisches Verschieben.
  Beim Laufwerkswechsel wird zuerst vollständig kopiert und erst dann der neue Ordner
  veröffentlicht. Die Engine muss die alte Freigabe vorher freigeben.
- **Tray:** Schließen versteckt das Fenster (`tray::hide_on_close`), Beenden nur über das
  Tray-Menü (`app.exit`, dann `RunEvent::Exit` stoppt Chat und Engine). Das Verstecken greift nur,
  wenn das Symbol gebaut wurde, sonst wäre der Launcher unerreichbar. Unter Linux zusätzlich:
  kein Symbol unter gamescope (Game Mode: kein Infobereich, Steam hielte den versteckten Launcher
  für ein laufendes Spiel) und ohne ladbare appindicator-Bibliothek (`libappindicator-sys`
  *panict* sonst); versteckt wird nur, solange jemand `org.kde.StatusNotifierWatcher` auf dem
  Sitzungsbus besitzt — beim Schließen gefragt, nicht beim Start. Reines GNOME nimmt das Symbol
  an und zeigt es nie. Zweiter Start: Windows/macOS über `tauri-plugin-single-instance` (als
  erstes Plugin), Linux über `lanlauncher_core::instance` (`flock` + Unix-Socket in
  `$XDG_RUNTIME_DIR/xyz.nextgen-lan.launcher/`, ganz am Anfang von `run`). Das Plugin taugt unter
  Linux nicht: Es `unwrap`t einen D-Bus-Sitzungsbus, und der Nachfolger der Grafik-Leiter würde
  sich an den Vorgänger übergeben, der gerade geht. Deshalb wartet der Nachfolger (`RETRY_ENV`)
  auf die Sperre und fragt erst, wenn der Vorgänger (`NLL_GRAPHICS_PREDECESSOR`, PID) weg ist —
  nicht über `getppid`, im AppImage ist der Elternprozess die AppImage-Laufzeit. Ein Launcher, der gerade beendet (`tray::QUITTING`),
  antwortet „busy“; der neue Start wartet dann auf die Sperre und übernimmt. Ein Start mit eigenen
  Optionen (`--safe-graphics`, `--demo`) bittet den laufenden Launcher zu beenden (`Ask::Replace`)
  und übernimmt — sonst käme `--safe-graphics` nach einem weißen Fenster nie an, denn das
  Schließen hat den Launcher nur versteckt. Nachstellen unter
  Xvfb: `dbus-run-session`, ein kleines Programm, das den Watcher-Namen per `g_bus_own_name`
  besitzt, ein Fenstermanager (`openbox`) und `wmctrl -c` zum Schließen (`xdotool windowclose`
  zerstört das Fenster, statt es zu schließen).
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
- **GStreamer im AppImage — halb ist schlimmer als gar nicht:** `libwebkit2gtk-4.1` linkt hart
  gegen zehn GStreamer-Kernbibliotheken, linuxdeploy packt sie also ein (sie stehen auf keiner
  Ausschlussliste). Die *Plugins* kommen aber nur mit `bundleMediaFramework: true` mit, und ein
  Plugin lädt ausschließlich in die Kernversion, gegen die es gebaut wurde. Ubuntus Kern plus die
  Plugins des Zielrechners ist **keine** Medienbasis: WebKit findet nicht einmal `autoaudiosink`,
  ruft `g_signal_connect_data` auf dem Null-Zeiger und der Renderer stirbt — das Fenster bleibt als
  Standbild stehen. Auf dem Steam Deck war das die einfrierende Oberfläche beim Durchklicken
  mehrerer Spiele *und* die fehlenden Vorschauvideos, ein Fehler mit zwei Gesichtern.
  **Also entweder beides mitliefern oder beides weglassen, nie halb.** Nachstellen lässt sich der
  Zustand mit `GST_PLUGIN_SYSTEM_PATH_1_0=/nonexistent GST_PLUGIN_PATH_1_0=/nonexistent` — dann
  zeigt jeder Build ohne gebündelte Plugins die Deck-Meldung.
- **AppImage und FUSE:** Das Abbild nutzt die statische type2-Laufzeit (`file` sagt
  „static-pie linked“, `readelf -d` zeigt kein `NEEDED`). **libfuse2 wird nicht gebraucht**, nur
  `fusermount`/`fusermount3` und `/dev/fuse`. Fehlt das, entpackt sich das Abbild selbst nach
  `$TMPDIR/appimage_extracted_<hash>` und startet trotzdem (hier nachgestellt mit einem `PATH` ohne
  `fusermount`). Eine Prüfung *vor* dem Start kann der Launcher nicht leisten, sein Code läuft erst
  danach; `diagnostics::check_appimage_unpacked` erkennt den entpackten Lauf an `APPDIR`.
- **Startkonfiguration von Testern:** Der Editor in den Spieldetails (`GameConfigDialog.svelte`,
  Kern in `game_config.rs`) speichert **nur einen `[platform.<os>]`-Block** nach
  `<data>/game-configs/<id>.toml` (`ConfigOverlay`, mit der Paket-Revision); `resolve_for` legt ihn
  Feld für Feld über den Block des Profils (`PlatformOverride::layered_on`,
  `Manifest::user_config`). Der Block enthält **nur, was vom Profil abweicht** (`to_block` vergleicht
  mit `resolve_profile_for`, dem Profil ohne Konfiguration). Bewusst keine Kopie des ganzen
  Profils und keine vollständige Plattform: beides würde spätere Korrekturen am mitgelieferten
  Profil dauerhaft verdecken. `[launch]` wird nie angefasst: entfernte Variablen stehen in
  `unset_env`, ein leerer Arbeitsordner wird als `""` (Ordner der EXE) geschrieben, `.` ist `local/`
  (und `[launch] workdir = ""` ebenfalls `local/`, im Editor als `.` gezeigt). Die eigene
  Konfiguration gilt nur für die Revision als geprüft, mit der sie gespeichert wurde. Der Editor zeigt, was *wirklich*
  startet — eine Startdatei-Wahl im Receipt startet nackt (ohne Argumente, im eigenen Ordner) —, und
  beim Speichern wird diese Wahl aus dem Receipt gelöscht, sonst gewänne sie weiter. Geteilt wird
  ebenfalls, was startet, auch ungespeichert, und zwar auf dem Profil *wie ausgeliefert*
  (`ManifestStore::resolve`, ohne die Skript-Vermutungen, die `resolve_for` einträgt). `.` als
  Arbeitsordner ist `local/`, `""` der Ordner der EXE — nicht ineinander umwandeln. Mail- und
  Issue-Link tragen nur den Block (`block_toml`, Längengrenze `LINK_BODY_LIMIT`, eine lange Notiz
  wird im Link gekürzt). **Wrapper und Host-Variablen (`LD_PRELOAD`, `PATH`, … `HOST_HOOK_ENV`) aus
  einem Spiel-Share-Profil werden schon beim Laden entfernt** (`Manifest::drop_host_hooks` in
  `resolve`) — ein Profil soll keinen Weg an Wine vorbei hinzufügen, und ein Block ohne eigenen
  Wrapper fiele sonst auf `[launch].wrapper` des Shares zurück. Das macht Share-Inhalte nicht
  harmlos (Wine ist keine Sandbox), also nirgends so behaupten. Eine Konfigurationsdatei, die sich
  nicht lesen lässt, wird nie überschrieben (`load_config_for_update`). Als geprüft gilt die
  eigene Konfiguration für die *installierte* Revision (Receipt), nicht die des Katalogs.
  „Startdatei wählen" schreibt in eine vorhandene Konfiguration statt ins Receipt.
  `Manifest::wrapper_is_trusted` bleibt als zweite Sperre. Das Speichern einer Datei geht über den nativen Dialog in
  Rust (`export_game_config`), nie über einen Pfad aus der Web-Ansicht. Der Wrapper steckt in `LaunchPlan::wrapper` und wird erst in
  `spawn` vorangestellt; `program` bleibt Wine/Proton, sonst sähen Prefix-Aufzeichnung und
  `cxbottle` das Falsche. Berichte gehen an `game_config::REPORT_EMAIL` oder als Issue an
  `REPORT_REPOSITORY`; `mailto:` ist dafür in `open_url` erlaubt.
- **Windows-Komponenten (winetricks):** Profile nennen winetricks-Verben (`winetricks = […]`,
  `manifest::is_winetricks_verb`: nie eine Option mit `-` und keiner der eigenen Befehle von
  winetricks — `annihilate` löscht den Prefix samt Spielständen, und `--unattended` beantwortet die
  Rückfrage mit Ja; `prefix=`, `shell`, `*.verb` ebenso gesperrt). Installiert wird **nur per
  Knopf** (`install_components`) oder nach Rückfrage vor dem Start, nie von selbst — dauert
  Minuten und braucht meist einmal Internet. Im Demo-Modus gesperrt.
  - *Vor dem Start:* `play_game` meldet `err.components_needed|<verben>`, wenn der Prefix Verben
    des Profils nicht hat (`winetricks::missing`: was der Launcher dort installiert hat, steht in
    `<prefix>/.nll-components`, `winetricks::remember`; ein gelöschter oder neuer Prefix startet
    leer). Das Frontend fragt „Installieren / Ohne starten“ und startet danach mit
    `skipComponents`. Ein Proton-Prefix, den es noch nicht gibt, legt `make_proton_prefix` mit
    einem leeren `Script::Settings`-Lauf an. Anlass: FlatOut 2 endet ohne Microsofts
    `d3dx9_30` mit „Failed to create effect“ (Wines eigene DLL baut die Effekte nicht).
  - *Ziel:* `launch/winetricks.rs` nimmt den Prefix aus dem Startplan: Wine `WINEPREFIX`, Proton
    `<compat>/pfx` mit Protons Wine (`files/bin/wine`, früher `dist/`), wie protontricks. Proton
    erkennt `target` am Programm `proton`, nicht an `STEAM_COMPAT_DATA_PATH` (das kann ein Profil
    auch einem Wine-Start mitgeben). Vor dem ersten Proton-Start gibt es keinen Prefix
    (`err.components_start_first`), CrossOver wird abgelehnt. Ein Wine-Start gibt `WINEARCH`,
    `WINEDLLPATH`, `WINELOADER`, `WINESERVER` weiter (`CARRIED`), sonst würde ein `win32`-Prefix
    64-bittig angelegt; liegt neben dem Wine ein `wineserver`, gilt der (winetricks sucht sonst
    `${WINE}server` und dann im `PATH` — „version mismatch“).
  - *Proton:* **nur Wine** bekommt Protons Bibliotheken (Proton 8+: `lib/x86_64-linux-gnu`,
    `lib/i386-linux-gnu`; davor `lib64`/`lib`). `WINE`/`WINESERVER` zeigen auf Platzhalter-Skripte
    in `<data>/tools/proton-shims` (`SHIM`), die Protons gleichnamige Binärdatei mit
    `LD_LIBRARY_PATH` starten; `WINE_BIN`/`WINESERVER_BIN` nennen winetricks die echten
    (Architektur-Erkennung). `LD_LIBRARY_PATH` für den ganzen Lauf würde curl, sha256sum und
    cabextract Protons Bibliotheken unterschieben. `WINELOADER` bleibt die echte Datei, Wine sucht
    daneben seinen Preloader — und der Platzhalter muss so heißen wie die Binärdatei, Wine startet
    sich unter diesem Namen neu.
  - *Ergebnis:* **ein winetricks-Aufruf pro Verb, der Exit-Code entscheidet.** `winetricks.log`
    taugt dafür nicht: winetricks trägt ein Verb ein, *bevor* es prüft, ob es wirklich drin ist,
    Einstellungen und Aliase stehen unter anderem Namen darin (`vd`, `native d3d9`, `dotnet20`),
    und Abhängigkeiten tauchen als eigene Aufrufe auf. Nach einem Fehler laufen die übrigen Verben
    trotzdem. winetricks überspringt Installiertes selbst und wendet Einstellungen (`winxp`) erneut
    an; „Erneut installieren“ hängt `--force` an. Die Ausgabe sagt nur, *warum* etwas fehlt
    (`Downloading … failed` = offline, `Cannot find cabextract`).
  - *Mitgeliefert:* winetricks fest auf `winetricks.lock.json`, unter Linux cabextract samt
    libmspack hinter einem Wrapper (`tools/fetch-winetricks.mjs`, SteamOS hat kein cabextract).
    Der Wrapper (`tools/cabextract-wrapper.sh`) entpackt `-d` über einen Zwischenordner im Ziel
    und verschiebt nur bei Erfolg: In einem Proton-Prefix sind die eingebauten DLLs in
    `system32`/`syswow64` Symlinks in die Proton-Installation, cabextract schrieb hindurch
    (dort schreibgeschützt, `directplay` brach bei `dplaysvr.exe` ab); `mv` ersetzt den Link wie
    winetricks' `w_try_cp_dll`. Proton lässt ersetzte DLLs bei Updates stehen
    (`update_builtin_libs`: „builtin library was replaced“), nur ein Wechsel auf eine ältere
    Version räumt sie ab. Vor dem Lauf nach `<data>/tools/winetricks` kopiert, weil Ressourcen das Exec-Bit verlieren und
    ein AppImage schreibgeschützt ist; unveränderte Dateien bleiben stehen (ein laufender Job führt
    sie aus), was das Paket nicht mehr mitbringt, fliegt raus. Läuft per `sh`.
  - *Abbruch:* eigene Prozessgruppe je Aufruf; nach 45 Minuten (für alle Verben zusammen) wird die
    Gruppe beendet und `wine wineboot -k` räumt den Prefix
    (`a_run_past_its_limit_ends_its_whole_process_group`); wird der Lauf verworfen, beendet
    `GroupGuard` die Gruppe ebenso (einmal, per `libc::kill`; danach ist die ID vergessen, sie
    wird nach dem Abholen des Prozesses neu vergeben). Beendet sich der Launcher selbst mitten im
    Lauf, laufen die Installer weiter — dagegen hilft nichts, was im Prozess steckt.
  - *Sperren* (`AppState::prefix_use`): immer nur ein Lauf. Start, Update, Reparieren,
    Deinstallieren, Abbrechen, Wahl des Werkzeugs und Speichern/Zurücksetzen der Konfiguration
    (`GameUse`) verweigern das Spiel, dessen Prefix gerade befüllt wird; `play_game` vergleicht
    zusätzlich den Prefix des Plans (Profile dürfen sich einen teilen; Schlüssel über den nächsten
    existierenden Ordner kanonisiert, `prefix_key`, weil winetricks den Prefix erst anlegt).
    Umgekehrt verweigert der Lauf, solange irgendein Spiel einen `GameUse` hält, ein Spiel ohne
    Receipt oder beim Prüfen/Entpacken/Setup (ein laufender Update-Download lässt die alte Version
    stehen, die startet), und einen Prefix, in dem noch ein Prozess läuft:
    `winetricks::prefix_in_use` liest `WINEPREFIX` aus der Umgebung aller Prozesse (relativ zu
    deren Arbeitsordner), bei Proton auch `STEAM_COMPAT_DATA_PATH` (das Python-`proton` trägt nur
    das), und übergeht Wines eigene Prozesse wie `wineserver`, die ein Spiel um Sekunden überleben
    — nicht die PID des Starts, viele Spiele starten über ein Programm, das sich beendet.
    Das sind Sperren pro Befehl; ein neuer Befehl, der Spielordner anfasst, braucht `GameUse`.
  - Ein Lauf zählt für die Prefix-Aufzeichnung wie ein Start (`remember_default_prefix_user`),
    denn winetricks legt den Prefix eines nie gestarteten Wine-Spiels an.
  - Echter Lauf mit Wine: `NLL_TEST_WINETRICKS=<script> NLL_TEST_WINE_BIN=/usr/lib/wine cargo
    test -p lanlauncher-core a_real_winetricks_run -- --ignored` (braucht `wine` und `xvfb-run`;
    der zweite Test baut aus den Wine-Binärdateien des Rechners ein Proton-Layout und läuft über
    die Platzhalter).
- **Proton liegt nie im `PATH`:** Es wohnt in einer Steam-Bibliothek, und ein Steam Deck hat
  mindestens zwei (intern und SD-Karte, letztere unter `/run/media/…`). Die Bibliotheken stehen in
  `<steam root>/steamapps/libraryfolders.vdf`; wer die nicht liest, findet auf einem Deck die
  Hälfte nicht. Dazu kommen mehrere Steam-Wurzeln (`~/.steam/steam` und `~/.steam/root` sind
  Symlinks auf eine der anderen, dazu `~/.local/share/Steam`, die Debian-Variante und
  Flatpak-Steam) — beim Entdoppeln deshalb **immer über `canonicalize` vergleichen**, sonst gilt
  dieselbe Installation unter zwei Schreibweisen als zwei Funde (genau daran ist der erste Entwurf
  gescheitert, der Test `a_symlinked_root_does_not_report_the_same_proton_twice` hält es fest).
  `STEAM_COMPAT_CLIENT_INSTALL_PATH` muss die Wurzel sein, zu der *dieses* Proton gehört; fest
  `~/.steam/steam` einzusetzen geht bei Flatpak-Steam schief. Systemweit installierte Tools (`/usr/share/steam/compatibilitytools.d`, Bazzite) laufen mit der
  ersten Steam-Wurzel, die ein `steamapps` hat (ein verwaistes `~/.steam/steam` zählt nicht); hat der
  Benutzer denselben Build selbst installiert, bleibt nur seine Kopie (gleicher Name); Tests rufen `find_protons_in(home, &[])`, sonst sähen sie, was der
  Testrechner installiert hat. Alles in
  `crates/lanlauncher-core/src/launch/proton.rs`, rein dateisystembasiert und damit testbar.
- **LAN-Chat** (`crates/lanlauncher-core/src/chat/`, Shell `src-tauri/src/chat.rs`, Panel
  `ChatPanel.svelte`): serverlos, UDP-Beacons und TCP auf Port 41950. Alles ist ein
  unveränderliches `Event`; zwei Launcher gleichen beim Treffen und alle zwei Minuten per `Hello`
  ab, was fehlt. **Nie ein Event verändern oder Reihenfolge voraussetzen** — Reaktionen und
  Stimmen entscheidet die höchste `seq` je Person, sonst kippt das Ergebnis je nach
  Ankunftsreihenfolge. Private Events gehen nur an Beteiligte (`Event::visible_to`), auch beim
  Abgleich; ein Event, das auf eine private Nachricht zeigt, muss in deren Unterhaltung bleiben.
  Eine TCP-Verbindung, die die Gegenseite beim Neustart geschlossen hat, nimmt den ersten Schreib-
  vorgang noch „erfolgreich“ an — deshalb liest `link_writer` seine Verbindung mit (der Leser setzt
  `closed`, wenn die Gegenseite schließt; vor jedem Schreiben geprüft), und
  ein wieder auftauchender Peer bekommt eine frische Verbindung (der Test
  `a_late_arrival_reads_the_history_and_gets_what_was_sent_to_it` war genau daran wackelig).
  Verbindungen tragen Rahmen in beide Richtungen: Ein `Hello` mit `duplex` wird über die
  Verbindung beantwortet, auf der es kam (`answer` verhindert Ping-Pong), und wer einen Peer nicht
  erreicht, schreibt über dessen Verbindung (`back_links`). Vorher brauchte das Nachholen beide
  Richtungen, Live-Nachrichten nur eine — genau das Bild „live geht, Verlauf fehlt“ beim
  Test mit zwei PCs. Test `one_launcher_reaching_the_other_is_enough` (`advertise_port` nennt
  einen geschlossenen Port). Die Firewall-Hinweise der Diagnose gelten nur, solange
  `Chat::heard_from_others` falsch ist: Eine fehlende eigene Regel beweist nichts, die
  Windows-Abfrage beim ersten Start legt eine Programmregel unter anderem Namen an.
  „Spielt gerade“ in der Personenliste ist dieselbe Angabe wie `current_game` an die LANPage
  (`AppState::running`, Beacon-Feld `playing`); nichts Eigenes erfinden. `chat::prune_running`
  trägt beendete Spiele aus (`launch::runs_from`). Spamschutz: `Flood`, nur beim Sender.
  Spieler ohne Chat (ETI-Launcher) kommen aus `stats.php?online=1` der LANPage (eigene Erweiterung im
  Repo `L3t4l3s/nextgen-lanpage`); „online“ ist deren Maß, nicht unseres.
  Die Peer-ID ist ein Ed25519-Schlüssel (`chat/crypto.rs`): jedes Event ist signiert, private
  Bodies sind versiegelt (`Body::Sealed`); der Zustand faltet die geöffnete Form, gespeichert und
  weitergegeben wird nur die Wire-Form. Ein `Hello` ist unsigniert und darf einen online
  gemeldeten Peer weder umadressieren noch umbenennen. Spitznamen sind nicht eindeutig — nur die
  ID dahinter ist fälschungssicher; nirgends anders behaupten. Der Verlauf ist auf 5000 Events
  begrenzt (`trim`, `floor`, `Hello.since`), sonst sprengt die ID-Liste eines `Hello` die
  Rahmengrenze. Was dieser Launcher vor mehr als fünf Tagen bekommen hat (`KEEP_FOR`, **eigene
  Ankunftszeit**, nie die Uhr des Absenders — die geht auf LAN-Rechnern gern Tage falsch), fliegt
  beim Start und jede Minute raus. Beim Weiterreichen reist das Alter mit (`Frame::Events.ages`,
  eine Dauer, also uhrunabhängig), sonst startet ein spät nachgeholter Beitrag seine fünf Tage neu
  und taucht auf der nächsten LAN wieder auf. Die IDs merkt sich `expired.json`
  (`ChatState::gone`).
  Themen: `Body::Topic` eröffnet eins, öffentliche Events tragen `Event::topic`; die Signatur
  nimmt das Feld nur auf, wenn es gesetzt ist (`crypto::signed_bytes`), sonst verlören ältere
  Events ihre Gültigkeit. Unterhaltungen heißen im Frontend `null`, `#<id>` oder Peer-ID, genau so
  nimmt sie `Chat::send_text`. Das Browser-Kontextmenü unterdrückt `main.ts` (außer im Dev-Build
  und in Eingabefeldern); Nachrichten öffnen ihr eigenes.
  `nll-chat-relay` (`src/bin/`) ist derselbe Code mit `ChatConfig::relay`: keine
  Person, bekommt private Events (versiegelt, `insert_opaque`) nur, wenn `launcher.ini` seine ID
  nennt (`chat_relay`, `Chat::set_trusted_relays`) — das `relay`-Flag im Beacon beweist nichts —,
  und reicht sie per `Hello` weiter; es leitet nichts live weiter, im flachen LAN erreicht jeder Launcher jeden direkt.
  Tests laufen mit mehreren Chats auf 127.0.0.1 und direkten Beacon-Zielen
  (`ChatConfig::beacon_targets`), nicht über Broadcast.
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
PowerShell-/netsh-Fixes, die Startprofile unter Wine/CrossOver sowie der LAN-Chat zwischen
mehreren echten Rechnern (Broadcasts über Switches/WLAN, Windows-Firewall). Änderungen an diesen Stellen
im Commit als „ungetestet, auf der LAN prüfen“ kennzeichnen und in `README.md` unter
„Offene Punkte“ nachhalten.

## Versionierung

`0.<feature>.<korrektur>`; die 0 bleibt, bis der Launcher als fertig gilt (diesen Schritt nur auf
ausdrückliche Ansage, mit `node tools/bump-version.mjs 1.0.0`). **Jeder Commit, der etwas
Sichtbares ändert, hebt die Version mit an:**

- neues Feature → `node tools/bump-version.mjs minor` (0.3.4 → 0.4.0)
- Korrektur, neues oder geändertes Startprofil, Text-/Stilkorrektur → `node tools/bump-version.mjs patch`
  (0.3.0 → 0.3.1)
- reine Interna (Refactoring, Tests, CI, Doku) → keine neue Version

Das Skript ändert `package.json`, `package-lock.json`, `tauri.conf.json`, `Cargo.toml` und
`Cargo.lock` zusammen und macht aus „## Unreleased“ im `CHANGELOG.md` den Abschnitt der neuen
Version (optional mit Titel als zweitem Argument). Der CHANGELOG-Eintrag gehört also vorher unter
„Unreleased“. `src/lib/version.test.ts` (Teil von `npm test`) scheitert, wenn die Dateien
auseinanderlaufen oder der CHANGELOG keinen Abschnitt für die Version hat. Mehrere Commits einer
Aufgabe heben die Version nur einmal an. Ein Release-Tag `v<version>` entspricht der Version im
Repo.

## Commits

Aussagekräftige Commit-Nachrichten in Englisch, die das *Warum* nennen. Keine Modellnamen im
Commit-Text, in Code oder Kommentaren; der von der Tooling-Umgebung angehängte
`Co-Authored-By`-Trailer ist davon ausgenommen. Niemals `game.db` mit echten Keys,
`.eti`-Archive oder die Resilio-Binärdatei einchecken.
