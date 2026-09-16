# Changelog

## Unreleased

- Die LANPage kann ihre Farben jetzt in der `launcher.ini` nennen (`theme_primary`,
  `theme_background`, `theme_mode` und die übrigen `theme_*`-Schlüssel), ohne eine `theme.json`
  zu hinterlegen. Eine vorhandene `theme.json` hat weiterhin Vorrang. Ein fehlerhafter Wert kostet
  nur sich selbst und steht im Log; ohne Farbangabe bleibt das gewählte Schema unangetastet.

- Jeder Fortschrittsbalken stand immer auf voll, egal welcher Wert danebenstand. Die Breite kam
  als „23 %“ aus der Anzeige-Formatierung, und CSS verwirft eine Länge mit Leerzeichen vor dem
  Prozentzeichen. Balken und Zahl kommen jetzt aus getrennten Funktionen.
- Downloads zeigten „0 B von 782 B“ und meldeten nach zwei Minuten „Download hängt“, während der
  Sync-Server mit voller Rate lieferte. Zwei Ursachen: Die Größe der Engine (die beim Indizieren
  erst ein paar hundert Byte kennt) galt als Gesamtgröße, obwohl der Katalog die echte kennt; und
  gezählt wurden nur `<id>.eti` und dessen `.!sync`-Datei, nicht der Ordner. Jetzt ist die
  Katalogangabe die Untergrenze, und gezählt wird, was im Spielordner liegt.
- Die Kachel in der Bibliothek zeigt unter dem Balken Prozent und Laderate. Liegt es daran, dass
  niemand das Spiel anbietet, steht dort „Keine Quelle“, bei einem hängenden Download „Hängt“ —
  beides mit der ausführlichen Erklärung als Tooltip. Welcher Fall vorliegt, entscheidet der
  Kern, nicht die Oberfläche.
- „Der ausgewählte Ordner wurde bereits zu Resilio Sync hinzugefügt“ (API-Fehler 200) brach das
  Hinzufügen einer Freigabe ab. Der Fall gilt jetzt wie Fehler 5 als Erfolg, sofern die Freigabe
  denselben Schlüssel trägt — geprüft wird das über beide Antwortformate.
- Die Prüfung auf der Diagnoseseite dauerte mehrere Sekunden: `Get-NetFirewallRule` lädt erst ein
  PowerShell-Modul. Die Firewall-Regel wird jetzt per `netsh` über den Exit-Code geprüft, und die
  Abfrage der Netzwerkprofile läuft parallel zur Engine-Abfrage.
- Überarbeitetes App-Icon (freigestellt statt auf schwarzem Quadrat).

- Nach einem Update zeigte Windows im Explorer, auf der Verknüpfung und in der Taskleiste weiter
  das alte Icon, obwohl die neue EXE das richtige enthält (die Vorschau zeigte es). Das ist der
  Icon-Cache pro Pfad. Der Installer meldet dem System jetzt, dass sich Symbole geändert haben;
  die README beschreibt, wie man einen hartnäckigen Cache von Hand leert. Ungetestet gegen ein
  echtes Update, auf der LAN prüfen.

- Eigenes App-Icon für Windows, macOS und Linux; ohne Logo von der LANPage steht es auch oben
  links in der Kopfzeile und als Symbol im Browser-Tab.
- Die Resilio-Oberfläche öffnet sich per Knopf in der Diagnose und meldet sich selbst an. Das
  Kennwort der Engine wird je Installation zufällig erzeugt, tippen konnte man es also nicht.
- Resilio meldet Ordner in der Windows-Langform (`\\?\C:\LAN\…`) zurück, auch wenn wir sie ohne
  angemeldet haben. Der Abgleich mit unseren Ordnern scheiterte daran: kein Fortschritt, keine
  Gegenstellen, obwohl die Freigabe lief. Pfadvergleiche ignorieren das Präfix jetzt.
- Ein Admin-Lauf mit mehreren Befehlen meldete nur das Ergebnis des letzten. Vier Firewall-Regeln,
  von denen die erste scheiterte, galten als Erfolg. Jetzt zählt der erste Fehlschlag, und die
  Meldung zitiert genau den Befehl, der ihn ausgelöst hat. Zeilen, die scheitern dürfen (das
  Löschen nicht vorhandener Regeln), sind als solche gekennzeichnet.

- Die Firewall-Reparatur scheiterte auf einem installierten Launcher mit „Code 1“. Ursache: Tauri
  liefert den Programmpfad in der Windows-Langform (`\\?\C:\…`), und `netsh` lehnt die ab. Der
  Pfad wird jetzt an der Quelle normalisiert. Auf einem Domänen-PC geprüft: Die Reparatur läuft
  durch.
- Schlägt eine Aktion mit Adminrechten fehl, stand im Fehler nur der Exit-Code: Die erhöhte
  Konsole gehört dem neuen Prozess, ihre Ausgabe war für den Launcher verloren. Sie landet jetzt in
  einer Protokolldatei neben dem Skript; die letzte Zeile daraus steht in der Meldung, das ganze
  Protokoll im Log.
- Auf einem Domänen-PC darf die Firewall je nach Richtlinie nur die IT ändern. Die Diagnose sagt
  das jetzt vorab und nennt, was die IT braucht. (Die Domäne allein verhindert die Regeln nicht,
  das ist geprüft.)
- Warnungen der Diagnose lassen sich ausblenden („Ignorieren“), zum Beispiel der zweite Adapter auf
  „Öffentlich“ oder eine Firewall, die per Richtlinie gesperrt ist. Sie zählen dann nicht mehr für
  die Ampel und stehen aufklappbar unter „Ignorierte Meldungen“. Nur Warnungen, die man bewusst
  hinnehmen kann; ein LAN, das gar nicht funktionieren kann, bleibt sichtbar.
- „Kann Katalog-Freigabe nicht registrieren: HTTP 500“ beim zweiten Start: Die Web-Oberfläche
  antwortet so für einen Ordner, den die Engine schon kennt. Der Fall gilt jetzt wie bei der
  dokumentierten API als Erfolg.
- Die Installer liegen im CI-Artefakt direkt im Zip statt unter `release/bundle/nsis/`.

- Ohne Resilio-API-Key war der Launcher auf einem frischen PC unbrauchbar: Der Rückfall auf
  Resilios Web-Oberfläche scheiterte mit HTTP 400. Zwei Ursachen sind behoben: Die Sitzungs-Cookies
  der Oberfläche werden jetzt mitgeschickt (ohne sie verwirft Resilio den eigenen Token), und wenn
  `getversion` abgelehnt wird, gilt eine beantwortete Ordnerliste als „Engine erreichbar“.
  Ungetestet gegen eine echte Engine, auf der LAN prüfen.
- Die Karte „Resilio Sync nicht verfügbar“ nennt jetzt den API-Key als Ursache und wo er hingehört.
  `README.md` beschreibt die drei Quellen (Einstellung, ETI-Installation, `launcher.ini`).
- Downloads zeigen jetzt eine echte Laderate: Die dokumentierte Resilio-API liefert keine, deshalb
  misst der Launcher das Wachstum der Datei auf der Platte (geglättet). Pro Eintrag lässt sich
  „Details“ aufklappen: Verlauf der Laderate der letzten zwei Minuten als Diagramm und die Quellen
  mit Name, Verbindungsart und Rate.
- Der Windows-Lauf der CI scheiterte an einem ungenutzten Import im Windows-Zweig; der Import
  entfällt, `tokio::process::Command` bringt `creation_flags` selbst mit.
- Downloads lassen sich abbrechen, ohne den Umweg über „Entfernen“ in der Bibliothek. Läuft nur
  ein Update, bleibt die installierte Version samt Spielständen erhalten; nur das heruntergeladene
  Archiv verschwindet.
- Bibliothek: Sortierung nach Katalog, Titel, Spielerzahl, Größe oder Erscheinungsjahr.
- Beim Beenden des Launchers wird die eigene Sync-Engine heruntergefahren (erst freundlich, nach
  drei Sekunden hart), statt weiterzulaufen.
- „Entfernen“ scheiterte auf Windows mit „Zugriff verweigert“, solange die Engine den Ordner noch
  offen hatte. Der Launcher wartet jetzt kurz nach dem Entfernen der Freigabe und versucht das
  Löschen mehrfach; schreibgeschützte Dateien werden dabei freigegeben.
- Diagnose und ihre Reparaturen öffnen kein PowerShell-Fenster mehr; alle Hilfsprozesse laufen
  unsichtbar.
- Die Firewall-Reparatur löscht zuerst alle vorhandenen Regeln für die Sync-Engine. Eine einmal
  abgelehnte Windows-Abfrage hinterlässt eine Blockregel, die jede Freigabe aussticht — das war
  vermutlich der Grund, warum „Jetzt beheben“ nach der Admin-Abfrage wirkungslos blieb
  (ungetestet, auf der LAN prüfen).
- Unter Windows meldet der Launcher kein „Startprofil passt nicht zum Paket“ mehr. Dort startet
  ausschließlich `game_start.cmd`; das Profil wird aus eben diesem Skript abgeleitet und taugt
  nicht als Prüfkriterium. Auf macOS und Linux bleibt die Prüfung, denn dort startet das Profil.
- Diagnose zeigt den Zustand des Katalog-Ordners laut Sync-Engine (Peers, Status, Fehlercode) und
  die Adresse der Resilio-Oberfläche; Änderungen landen im Log. Fehlende Windows-Firewall-Regeln
  für die mitgelieferte Engine werden erkannt und per „Jetzt beheben“ angelegt. Eine LANPage, die
  unter `theme.json` HTML liefert, erzeugt keinen Fehler mehr.
- Der Launcher startet ohne Adminrechte. Die Windows-Abfrage erscheint nur noch bei Bedarf: beim
  einmaligen Setup eines Spiels (dabei legt der Launcher die Firewall-Regeln aus dem Startskript
  gleich mit an), bei Startskripten, die in HKLM schreiben, bei Server-Skripten, bei
  Diagnose-Reparaturen und bei Programmen, die selbst Adminrechte verlangen. Zum reinen Spielen
  ist keine Abfrage mehr nötig.
- Der Resilio-API-Key steht nicht mehr im Quellcode. Der Launcher liest ihn aus den Einstellungen,
  aus einer installierten ETI-Version oder aus `launcher.ini` der LANPage (`resilio_api_key`);
  ohne Key nutzt er Resilios Web-Oberfläche. Die LANPage wird dafür vor dem Sync-Start abgefragt.
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
