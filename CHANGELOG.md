# Changelog

## Unreleased

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
