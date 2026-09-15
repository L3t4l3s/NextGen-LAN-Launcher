# Changelog

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
