# NextGen LAN Launcher

Ein plattformübergreifender Nachfolger des **ETI LAN Launchers** (eti-lan.xyz) für LAN-Partys:
Spiele aus dem bestehenden ETI-Ökosystem (Sync-Server, `game.db`, `.eti`-Pakete, LANPage)
installieren und starten – auf **Windows 10/11, macOS und Linux**, mit einem modernen,
pro LAN anpassbaren Interface und klaren Hilfetexten, wenn etwas nicht funktioniert.

> Status: **0.1.0 – Grundstein.** Kernlogik und Oberfläche sind fertig und getestet, der
> Betrieb gegen einen echten Sync-Server muss auf der nächsten LAN verifiziert werden
> (siehe [Offene Punkte](#offene-punkte)).

![Bibliothek](screenshots/01-library.png)

## Was anders ist als beim alten Launcher

| Problem auf der letzten LAN | Lösung hier |
|---|---|
| Sync hängt bei 99 %, „Installieren“ wird nie „Spielen“ | Der Launcher **prüft das Archiv selbst** (CRC-Test) und entpackt, sobald die Datei vollständig auf der Platte liegt – unabhängig davon, was der Sync anzeigt. `crates/lanlauncher-core/src/install.rs` |
| „Too many workers“ beim Start | Genau eine vom Launcher gesteuerte Sync-Instanz; verwaiste Prozesse werden beim Start aufgeräumt. |
| Windows stellt das LAN auf „Öffentliches Netzwerk“ | Diagnose erkennt das Profil und stellt es per Klick auf „Privat“; Firewall-Regeln gelten für alle Profile. |
| Unklare Icon-Leiste | Oben beschriftete Reiter: Bibliothek · Downloads · LAN · Diagnose, rechts Einstellungen. |
| Reparieren-Button versteckt | „Reparieren“, „Sync anhalten“, „Ordner öffnen“, „Entfernen“ sind **immer** sichtbar. |
| Nur ein Spiele-Ordner | Mehrere Library-Ordner (verschiedene SSDs), neue Spiele landen dort, wo Platz ist. |
| Mac/Linux | Startprofile (`manifests/*.toml`) plus Wine/CrossOver/Proton; Windows nutzt weiterhin `game_start.cmd`. |

## Aufbau

```
crates/lanlauncher-core/   Rust-Bibliothek ohne GUI: Katalog, launcher.ini, Transport (Resilio/Ordner/Demo),
                           Install-Zustandsautomat, UnRAR, Start, Diagnose – 60+ Tests
src-tauri/                 Tauri-2-App (Commands, Events, Sidecar, Demo-Modus)
src/                       Svelte-5-Frontend (Deutsch/Englisch, Theme-Engine, Browser-Mock für Entwicklung)
manifests/                 Startprofile für Mac/Linux (Start: amongus, rocket, goldsrc, wc3, quake3, l4d2)
themes/                    Beispiel-Themes
tools/dev-lanpage/         Mini-LANPage für lokale Tests (launcher.ini, launcher.css, stats.php)
docs/                      Architektur, Kompatibilität, Troubleshooting, Theming, Lizenzen
```

Details: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) · [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) ·
[docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) · [docs/THEMING.md](docs/THEMING.md) ·
[docs/LICENSING.md](docs/LICENSING.md)

## Entwickeln

Voraussetzungen: Rust (stable), Node 22, unter Linux `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev`.

```bash
npm install
cargo test -p lanlauncher-core          # Kernlogik
npm run check && npm test               # Frontend
npm run dev                             # UI im Browser mit simuliertem Backend (http://localhost:1420)
npm run tauri dev -- -- --demo          # echte App im Demo-Modus (simulierter Sync)
npm run tauri build                     # Installer für die aktuelle Plattform
node tools/dev-lanpage/server.mjs       # lokale LANPage-Attrappe auf Port 8080
```

Im Browser zeigt `http://localhost:1420/?wizard` den Ersteinrichtungs-Assistenten.

## Offene Punkte

- **Echter Resilio-Betrieb:** Der Client spricht die dokumentierte Sync-API (`/api`, mit API-Key)
  und als Fallback die GUI-Endpunkte. Beides ist nur gegen einen laufenden Sync-Server prüfbar.
  Die Install-Logik hängt bewusst nicht davon ab.
- **Windows-Installer mit Resilio:** `release.yml` lädt die offizielle Binärdatei; Hashes in
  `resilio.lock.json` sind noch nicht gepinnt. Silent-Install unter Windows ist ungetestet.
- **Mac/Linux-Startprofile:** Sechs Spiele haben Profile; alle anderen erhalten ein aus
  `game_start.cmd` abgeleitetes Profil (91 von 158 ETI-Skripten haben genau eine Exe) oder
  die Exe wird beim ersten Start gewählt.
- Roadmap: LANPage im Launcher, LAN-Share-Oberfläche, TS3/Discord-Integration.

## Lizenz

MIT – siehe [LICENSE](LICENSE) und [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
