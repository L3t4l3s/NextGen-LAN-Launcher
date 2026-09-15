# Changelog

## Unreleased

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
