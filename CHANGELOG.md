# Changelog

## Unreleased

- LANPage-Theme sichtbar: `launcher.css` färbte nur eine Ebene hinter dem deckenden Fenster;
  jetzt scheint der Hintergrund der LAN durch. Das Logo der LANPage (`logo.png`, wie beim
  ETI-Launcher) erscheint in der Kopfzeile. Das Log nennt, welche Dateien die LANPage lieferte.
- Neue Farbschemata „NextGen Light“ und „Pinkes Einhorn“.
- „Ordner-Modus“ heißt jetzt „Offline-Modus: Kein Sync-Server verfügbar“.
- Einstellungen: Karte „Häufig benötigte Systembibliotheken“ mit „Paket installieren“ startet nach
  Rückfrage ETIs `preqsetup.exe` aus dem Katalog-Share (.NET 4.8, VC++, DirectX 11, PhysX …), sobald
  die Datei synchronisiert ist.
- Resilio startete mit Exit-Code 1: Die erzeugte `config.json` enthielt Schlüssel, die Resilio
  2.8.1 nicht kennt. Sie entspricht jetzt der Konfiguration des ETI-Launchers (gleiche Schlüssel,
  eigene Werte) und nutzt wie dieser den API-Schlüssel statt eines Web-Logins. Bricht die Engine
  weiterhin ab, stehen die letzten Zeilen ihrer Logs im Launcher-Log.
- Videos in der Detailansicht waren durch die Content-Security-Policy blockiert (`media-src`).
- Die LANPage-Adresse ist keine Einstellung mehr; wie beim ETI-Launcher gilt `launcher.lan`.
- Detailseite: Beschriftung „Das wird ausgeführt“ entfernt, die Kommandozeile steht für sich.
- Log nennt beim Katalog-Laden die `tools`-Tabelle (Paket-Installer des ETI-Launchers) und die
  gefundenen Videos.
- Detailseite: Buttons „Keygen“ und „Server starten“ erscheinen nur, wenn das Paket `keygen.exe`
  bzw. `server_start.cmd` mitbringt (ETI-Konvention), und starten sie direkt. Der Hinweis auf das
  Startskript ist weg, die Kommandozeile steht bei spielbereiten Spielen immer unten.
- Tab „LAN“ und der Platzhalter „Dateien (Bald)“ sind entfernt; LANPage, TeamSpeak, Discord und
  Datei-Freigabe kommen später als eigene Punkte in die Kopfzeile.
- Der Katalog erscheint direkt nach dem Start, auch wenn die Sync-Engine noch startet oder
  scheitert (Ladevorgang parallel, Ereignis an die Oberfläche).
- Einstellungen: Eine geänderte LANPage-Adresse wird sofort abgefragt und meldet, ob dort eine
  `launcher.ini` liegt; der Hinweistext erklärt, was daraus geladen wird.
- Beendet sich die Sync-Engine sofort, nennt die Meldung jedes andere laufende Resilio mit PID und
  Pfad, auch wenn es aus einer anderen Datei stammt.
- Resilio Sync 2.8.1.1390 liegt jedem Installer aus der vollen CI und aus Releases bei. Der
  Launcher bevorzugt diese Kopie vor einer auf dem System installierten Resilio-Version und
  startet sie ohne Installation (`/noinstall`); der bisherige Silent-Install-Pfad entfällt. Die
  Linux- und macOS-Pakete enthalten nur das entpackte Programm, nicht zusätzlich das Download-Archiv.
- Bibliothekskacheln und das Detailbild sind Querformat (7:5) wie ETIs 140×100-Cover; bisher wurden
  die Bilder stark beschnitten. Läuft ein Vorschauvideo, ist das Detailbild 16:9.
- Beendet sich die Sync-Engine sofort, nennt die Meldung die PID eines bereits laufenden Resilio
  aus derselben Datei (Resilio startet pro Programmdatei nur einmal) und was zu tun ist.
- Wizard-Reload und Katalog-Watcher entpacken `assets.eti` nicht mehr doppelt.
- Die Cover aller Katalogspiele werden mit dem Launcher ausgeliefert (Quelle: öffentliches Repo
  eti-lan/LAN-Launcher) und erscheinen sofort, auch ohne Sync-Server oder `assets.eti`. Cover aus
  `assets.eti` ersetzen sie, sobald der Katalog-Share da ist.
- Cover: `assets.eti` wird wie jede andere `.eti`-Datei als RAR gelesen (Tar und gzip-Tar weiterhin
  möglich); das Layout der Member ist toleranter, fehlende Treffer stehen mit Beispielnamen im Log.
  Nach dem Speichern des Spiele-Ordners im Wizard lädt der Katalog sofort, nicht erst nach 10 s.
- Hinweis „Fremde Sync-Prozesse“ erklärt, dass der Launcher fremde Resilio-Instanzen nicht beendet.
- Startet die Sync-Engine nicht, nennt die Meldung Binary, API-Port und Engine-Log und
  unterscheidet „sofort beendet“ (andere Instanz läuft) von „keine Antwort“; die Engine-Ausgabe
  landet in `transport/engine-output.log`.
- Resilio-Suche prüft auch `%APPDATA%\Resilio Sync` (Standard des Resilio-Installers) und den
  Pfad laufender Sync-Prozesse. Statusleiste und Log nennen Version plus Commit; „Log-Ordner
  öffnen“ zeigt auf den tatsächlichen Log-Ordner. Ein beim Start nicht ladbarer Katalog wird
  automatisch erneut versucht.
- Vorhandene Installationen des ETI-Launchers werden übernommen statt bei jedem Start neu
  geprüft, entpackt und eingerichtet (Spielstände in `local/` bleiben erhalten). „Reparieren“
  erzwingt weiterhin die volle Prüfung.
- `game_setup.cmd` wird wie `game_start.cmd` mit vier Argumenten und roher Kommandozeile
  gestartet; Skriptausgabe landet bei Fehlern im Log.
- Resilio-Suche findet auch die `btsync.exe` des ETI-Launchers, Installationen in PATH, anderen
  Benutzerprofilen und der Registry; geprüfte Pfade stehen im Log, der Pfad lässt sich in den
  Einstellungen vorgeben.
- Downloads zeigt einen Abschnitt „Hinweise“ für spielbare Spiele mit Warnung; der Zähler in der
  Statusleiste führt dorthin.
- Cover: gzip-komprimiertes `assets.eti` wird gelesen, Fehler stehen im Log, Diagnose meldet
  fehlende Cover.
- Statusleiste und LAN-Ansicht zeigen, ob ein Sync-Server gefunden wurde (mindestens ein Peer
  am Katalog-Ordner `eti_launcher`). „Alle Spiele“ zeigt weiterhin den kompletten Katalog.
- Der Launcher fügt den Katalog-Ordner `eti_launcher` im verwalteten Modus selbst zu Resilio hinzu.
  Der Read-only-Key ist fest hinterlegt (`BUILTIN_CATALOG_KEY`, der öffentliche ETI-Key aus
  `sync_server.tar`) und kann in den Einstellungen überschrieben werden. Eine geänderte `game.db`
  wird automatisch nachgeladen.
- „Installieren“ ohne Sync-Server reiht den Download ein und zeigt einen Hinweis statt
  „Download gestartet“. Diagnose meldet `transport.no_server`; `catalog.key_missing` bleibt als
  Absicherung für Builds ohne gültigen Key.
- Netzwerk-Check: getrennte oder ungenutzte Adapter („Kein Internet“, kein Verkehr) werden nicht
  mehr als Fehler gemeldet. Ein öffentlicher Zweitadapter neben einem privaten/Domänen-Adapter
  ergibt nur noch eine Warnung.
- Größenangaben folgen der Plattform: unter Windows binär wie im Explorer (1 GB = 1024³ Byte),
  sonst dezimal.
- CI: pro Push nur noch Linux-Core-Tests und Frontend; volle Matrix und Installer auf Anfrage.
  GitHub-Actions angehoben: checkout v5, setup-node v6, upload-artifact v6 (Node 24) und
  download-artifact v6.
- Windows-Bundle: nur noch der NSIS-Installer (`*-setup.exe`), kein MSI mehr. Die Größe kommt vom
  eingebetteten WebView2-Offline-Installer, damit die Installation ohne Internet klappt.
- Resilio-Sidecar auf Build 2.8.1.1390 gepinnt (Version + SHA-256 für Windows, macOS, Linux x64
  und arm64): letzte Version ohne Kontopflicht, gleicher Build wie der ETI-Sync-Server.

## 0.1.0 – Grundstein

- Rust-Kernbibliothek: ETI-Katalog (`game.db`, `assets.eti`), `launcher.ini`-Parser, Themes,
  Einstellungen mit mehreren Library-Ordnern, TOML-Startprofile, Skript-Analyse.
- Transport-Abstraktion mit Resilio-Client (Sync-API + GUI-Fallback, Prozess-Lifecycle,
  Aufräumen verwaister Instanzen), Ordner-Modus und Demo-Modus.
- Install-Zustandsautomat, der die Spielbarkeit aus geprüften Dateien ableitet
  (UnRAR-CRC-Test, atomares Entpacken, Receipt), inkl. Stall-Erkennung und Reparieren.
- Spielstart: Windows über `game_start.cmd`/`game_setup.cmd`, macOS/Linux über
  CrossOver/Wine/Proton.
- Diagnose mit Ein-Klick-Fixes (Windows-Netzwerkprofil, Firewall, Defender, Neustart).
- LANPage-Kompatibilität: `launcher.ini`, `launcher.css`, Statistik-Beacon; neues `theme.json`.
- Svelte-Oberfläche (de/en): beschriftete Reiter, Spiele-Grid, Detailansicht mit immer
  sichtbaren Sekundäraktionen, Downloads, LAN-Links, Diagnose, Einstellungen, Assistent.
- CI für Windows/macOS/Linux, Release-Workflow mit Resilio-Sidecar.
