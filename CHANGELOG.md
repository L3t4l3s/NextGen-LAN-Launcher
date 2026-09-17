# Changelog

## Unreleased

- Linux/AppImage: GStreamer wird jetzt vollständig mitgeliefert (`bundleMediaFramework: true`).
  Gemessen am gebauten Abbild: die **zehn Kernbibliotheken** (`libgstreamer-1.0`, `libgstvideo`,
  `libgstaudio` …) lagen drin — `libwebkit2gtk-4.1` linkt hart dagegen, sie sind nicht optional —
  aber **kein einziges Plugin**, also kein Decoder, kein Demuxer. Die Plugins kamen vom Rechner und
  sind an die dortige GStreamer-Version gebunden; in Ubuntus Kern lassen sie sich nicht laden.
  Damit hatte der Webview auf jedem Rechner mit anderer GStreamer-Version gar keine Medienbasis.
  Jetzt liegen 269 Plugins im Abbild (`libav`, `openh264`, `isomp4`, `matroska`, `vpx`,
  `playback`), Kern und Plugins passen also zusammen. Kosten: 94 → 161 MB.
  **Ungeprüft**, ob das die fehlenden Videos auf dem Steam Deck behebt — dieser Container hat
  weder D-Bus noch Audiogerät, dort scheitert die Wiedergabe aus anderem Grund.
- Entwicklungsbuilds zeigten keine Cover: der Rückfallpfad auf die mitgelieferten Bilder enthielt
  `..`, und das Asset-Protokoll von Tauri lehnt jeden Pfad mit Verzeichniswechsel ab („cannot
  traverse directory") — einmal pro Spiel im Log. Der Pfad wird jetzt aufgelöst. Ein Paketbau war
  nie betroffen, dort wird das Ressourcenverzeichnis gefunden.

## 0.2.0 – Ausbau

- Der Fortschritt eines Downloads kommt jetzt aus den Zählern der Gegenstellen. Resilios `size`
  zählt nur *fertige* Dateien: neben dem 150-GB-Paket liegt die acht Byte kleine `version.ini`,
  und genau „8 B von 150,9 GB“ stand stundenlang in der Zeile, während mit 244 MB/s geladen wurde.
  Die Laderate der Zeile kommt wieder von der Engine selbst, die ihre eigene Freigabe am besten
  kennt; gemessen wird nur noch, wo sie keine meldet (Ordner-Modus). Fangen die Zähler von vorn an
  (Neustart der Engine, eine Gegenstelle verschwindet), bleibt die Anzeige stehen statt
  zurückzuspringen. Bekannte Grenze: Wird der *Launcher* neu gestartet, beginnt auch die Engine
  neu, und was vor dem Neustart ankam, kann niemand mehr beziffern — die Anzeige eines laufenden
  Downloads fängt dann wieder unten an und zählt hoch. Am Download selbst ändert das nichts.
  Für die Frage „darf jetzt geprüft werden?“ zählt dagegen nur, was die Engine als *fertig*
  meldet — auch bei „Reparieren“: ein zur vollen Größe angelegter Platzhalter sieht fertig aus und
  ist es nicht, und eine halbe Stunde CRC-Prüfung darauf hilft niemandem.
- Scheitert die Einrichtung eines Spiels, nennt der Launcher den Pfad, an dem es hängt. Das Skript
  wird dafür gelesen wie cmd es liest: `set`-Variablen, `cd`/`pushd` (auch `%~dp0`) und
  Programmnamen relativ zum jeweiligen Ordner — `cd local`, dann `"OpenAL\oalinst.exe"` ist die
  übliche Form. Damit steht in der Meldung „E:\LAN\opencnc\local\OpenAL\oalinst.exe" statt
  Windows' „Das System kann den angegebenen Pfad nicht finden" ohne Pfad. Systemprogramme wie
  `reg.exe` werden auf dem `PATH` gesucht und nicht fälschlich als fehlend gemeldet. Einmal gegen
  alle 32 Einrichtungsskripte des ETI-Launchers laufen gelassen, damit die Meldung keine Pfade
  erfindet.
- Windows-Installer: Die Sync-Engine wird zusätzlich an ihrer Befehlszeile erkannt, damit auch eine
  Kopie beendet wird, die sich aus dem Programmordner herausgeschrieben hat.
- Themes können den Karten, Kacheln und dem Detailbereich eine Textur geben: `surfacePattern` in
  der `theme.json` (bzw. `theme_surface_pattern` in der `launcher.ini`) nennt eine der Figuren
  `scanlines`, `grid`, `dots`, `diagonal` oder `gradient` samt Farbe, Abstand und Winkel — die
  CSS-Regel dazu baut der Launcher selbst. Damit lassen sich die Scanlines einer LANPage nachbauen,
  die vorher in keiner Farbe ausdrückbar waren. Ohne den Schlüssel bleiben die Flächen glatt.
- Themes können jetzt eine eigene Schrift für die Überschriften nennen (`headingFontFamily` in der
  `theme.json`, `theme_heading_font_family` samt `theme_heading_font_src` in der `launcher.ini`).
  Bisher gab es genau einen Schriftstapel für die ganze Oberfläche; die schwere Schrift, die eine
  LANPage über ihre Überschriften legt, ließ sich damit gar nicht abbilden — als Textschrift wäre
  sie unlesbar. Sie gilt für `h1`–`h3` und den Veranstaltungsnamen in der Kopfzeile; ohne den
  Schlüssel bleibt alles bei der bisherigen Schrift.
- Diagnose: „Fremde Sync-Prozesse laufen" meldete unter Linux dauerhaft die eigene Engine.
  `sysinfo` listet pro Thread einen Eintrag, und ein Thread heißt wie sein Prozess — auf dem Steam
  Deck waren das 21 angeblich fremde `rslsync` mit lückenlosen PIDs, in Wahrheit die Threads der
  verwalteten Instanz. Die Warnung ließ sich nie wegklicken (die Fix-Aktion startet die eigene
  Engine neu) und verdeckte den Fall, für den sie gedacht ist: ein echtes zweites Resilio. Alle
  Stellen, die die Prozesstabelle durchsuchen, filtern Threads jetzt heraus — auch das Aufräumen
  verwaister Engines beim Start, das sonst Thread-IDs abzuschießen versuchte.
- Release-Workflow: Bauen und Hochladen sind getrennt, damit das AppImage dazwischen noch
  entbündelt werden kann; ein manueller Lauf legt die Installer als Artefakt ab, statt aus einem
  Branch-Namen ein Release zu erzeugen.
- **Linux: Das weiße Fenster auf dem Steam Deck ist gelöst — und die Ursache war unsere eigene
  Einstellung.** Das Gerät zeichnet auf der Sprosse `native`, also mit gar nichts erzwungen
  (`graphics.json`: `{"good":"native"}`, Logzeile `forced []`). Vier Builds lang wurde
  `WEBKIT_DISABLE_DMABUF_RENDERER=1` unbedingt auf jedem Linux gesetzt, weil es die Mehrheit der
  weißen GTK-Webviews repariert — auf dem Deck hat es das weiße Fenster erst erzeugt. Der Abbruch
  „Could not create default EGL display" ist die Anforderung des Pfades *ohne* DMA-BUF-Renderer,
  jeder Folge-Fix stapelte also Einstellungen auf die eigentliche Ursache, und `--safe-graphics`
  machte es schlimmer statt besser. Die Sprosse bleibt an erster Stelle (sie hilft der Mehrheit),
  aber ein Rechner, dem sie nicht bekommt, kommt jetzt in etwa einer Sekunde daran vorbei.
  Wer die beschleunigte Darstellung von Hand festnageln will: `WEBKIT_DISABLE_DMABUF_RENDERER=0`.
- Linux: Der Launcher liest jetzt mit, was der Webview auf die Standardfehlerausgabe schreibt, und
  steigt sofort auf die nächste Sprosse, wenn dort steht, dass aufgegeben wurde („Could not create
  default EGL display … Aborting…"). Vorher wartete er stur 30 Sekunden vor einem Fenster, das
  längst tot war — ein Steam-Deck-Test wurde nach 28 Sekunden abgebrochen, zwei Sekunden bevor die
  Leiter überhaupt losgestiegen wäre. Die Ausgabe geht dabei weiterhin unverändert ins Terminal,
  falls eins da ist, und zusätzlich nach `webview.log`. Harmlose Meldungen (etwa
  `libEGL warning: DRI3 error`) gelten ausdrücklich nicht als Aufgeben.
- Linux/AppImage: Die vier Wayland-Bibliotheken werden nicht mehr mitgeliefert
  (`tools/appimage-unbundle-wayland.mjs`, in `ci.yml` eingehängt). An einem gebauten Abbild
  gemessen: `libEGL`, `libGL`, `libgbm` und `libdrm` kommen korrekt vom Rechner, aber
  `libwayland-client`, `-server`, `-egl` und `-cursor` lagen im Image — `libwayland-client` steht
  sogar ausdrücklich auf der AppImage-Ausschlussliste und ist deren einziger Eintrag, den Tauris
  eigener linuxdeploy-Build falsch behandelt. Damit lief Mesa vom Rechner gegen ein Wayland aus
  Ubuntu, und `LD_LIBRARY_PATH` gab dem Image den Vorrang. Mesas `libEGL_mesa` bindet
  `libwayland-client` und `-server`; passt die Version nicht, lässt es sich gar nicht laden, und
  dann scheitert *jedes* `eglGetDisplay` mit EGL_BAD_PARAMETER — unabhängig von `EGL_PLATFORM` und
  von `LIBGL_ALWAYS_SOFTWARE`. **Das war jedoch nicht die Ursache des weißen Fensters auf dem Deck**
  (siehe oben); der Schritt bleibt, weil die Ausschlussliste recht hat — Mesa und die
  Wayland-Bibliotheken, gegen die es gelinkt ist, gehören demselben Rechner. Hier geprüft: die
  Dateien sind im Abbild, nach dem Entfernen startet und zeichnet es weiterhin.
- Linux: Der Launcher probiert die Renderer-Einstellungen jetzt selbst durch, statt sich auf eine
  festzulegen. Vier Versionen lang wurde je eine Vermutung ausgeliefert und auf dem Steam Deck
  getestet — jede Runde kostete ein Release, und keine hat getroffen. Stattdessen gibt es eine
  Leiter (`no-dmabuf` → `native` → `wayland` → `x11` → `software` → `software-surfaceless`): Meldet
  sich die Oberfläche nicht, startet sich der Launcher auf der nächsten Sprosse neu, und die
  Sprosse, die zeichnet, wird gemerkt (`~/.config/xyz.nextgen-lan.launcher/graphics.json`). Die
  Wartezeit fällt damit einmal pro Rechner an statt einmal pro Release. `--safe-graphics` springt
  ans untere Ende der Leiter, `--no-safe-graphics` vergisst alles wieder.
  Neu sind dabei drei Sprossen, die es vorher nicht gab: `native` lässt WebKitGTK seinen eigenen
  DMA-BUF-Renderer benutzen — genau den hat der Launcher bisher überall abgeschaltet, und der
  Abbruch „Could not create default EGL display" betrifft den Pfad *ohne* ihn, das bisherige
  „Gegenmittel" kommt also als Ursache infrage; `wayland` nimmt das `GDK_BACKEND=x11` zurück, das
  das AppImage jeder Sitzung aufzwingt; `software-surfaceless` kommt ganz ohne Display-Server für
  EGL aus.
  Eine Sprosse kann auch härter scheitern als weiß: Einstellungen, an denen GTK oder der Treiber
  ganz abbricht, nehmen den Prozess mit, bevor es ein Fenster gibt — dann läuft nichts mehr, das es
  merken könnte. Der Launcher notiert deshalb vor dem Fenster, welche Sprosse er versucht, und
  löscht die Notiz, sobald ein Fenster da ist; eine Sprosse, die den letzten Start umgebracht hat,
  fällt beim nächsten aus. Ein vom Benutzer früh geschlossenes Fenster zählt dagegen als normaler
  Lauf und kostet nichts.
- Linux: Was WebKitGTK über einen Renderer sagt, den es nicht benutzen kann, landet jetzt in
  `<Log-Ordner>/webview.log`. Diese Meldungen gingen bisher nur ins Terminal, und ein über ein
  Desktop-Symbol oder über Steam gestarteter Launcher hat keins — deshalb musste jeder Bericht von
  Hand aus einer Shell wiederholt werden. Wird der Launcher aus einem Terminal gestartet, bleibt
  die Ausgabe dort. Jede Sprosse der Leiter hängt ihre Zeilen an, mit Kopfzeile.
- Ein Spiel erbt die Umgebung des Launchers nicht mehr: `--safe-graphics` schaltet für dessen
  eigenes Fenster Software-Rendering ein, ein damit gestartetes Spiel lief bisher ebenfalls auf der
  CPU. Der Launcher merkt sich, was er selbst gesetzt hat — samt dem Wert, der vorher galt — und
  stellt genau das beim Spielstart wieder her. Ebenso fallen die Pfade des AppImage weg
  (`LD_LIBRARY_PATH`, `XDG_DATA_DIRS`, `GTK_PATH` …): sonst lädt ein Spiel die Ubuntu-Bibliotheken
  aus dem Abbild statt die des Rechners.

- Ein Paket, das die Engine anlegt, bevor sie es lädt, gilt nicht mehr als fertig: Resilio legt die
  Zieldatei sofort in voller Größe an (85,8 GB auf der Platte, 8 Byte angekommen), der Launcher hat
  daraus „fertig" gelesen und ist in eine endlose Prüfung gelaufen. Sobald die Engine die Freigabe
  eingelesen hat, zählt allein ihre Zahl, und geprüft wird erst, wenn sie den Großteil des Archivs
  bestätigt.
- Windows-Installer: Erst wird der Launcher beendet, dann seine Sync-Engine — andersherum hat der
  noch laufende Launcher die gerade beendete Engine sofort wieder gestartet, und die Installation
  scheiterte an der belegten Datei. Der Installer wartet danach, bis wirklich nichts mehr aus dem
  Programmordner läuft.
- Scheitert die Einrichtung eines Spiels, nennt der Launcher die Programme, die das Skript aufruft
  und die es auf diesem PC nicht gibt (`unrar.exe`, `fnr.exe` aus dem alten ETI-Launcher). Vorher
  stand dort nur Windows' „Das System kann den angegebenen Pfad nicht finden" ohne Pfad.

- Linux: `--safe-graphics` schaltet Software-Rendering, kein Compositing und X11 ein und merkt sich
  das (`--no-safe-graphics` nimmt es zurück) — für Geräte, auf denen WebKitGTK sonst ein weißes
  Fenster zeigt. Die Oberfläche meldet sich außerdem beim Start beim Kern; bleibt diese Meldung aus,
  startet der Launcher sich einmal mit den sicheren Einstellungen neu und sagt danach per
  Meldungsfenster Bescheid.
- Das leere cmd-Fenster beim Spielstart war hausgemacht: Der Launcher hat Aus- und Eingabe des
  Skripts nach NUL geleitet. Jetzt behält ein Startskript seine Konsole — Doom fragt dort wieder,
  ob Heretic, Hexen oder der GZDoom-Launcher starten soll, und wartet auf die Antwort. Wer die
  Ausgabe stattdessen schriftlich braucht, nimmt auf der Diagnoseseite „Mit Protokoll starten"
  (dann bleibt das Fenster des Spiels leer).
- Diagnose: Schlägt die Einrichtung eines Spiels fehl, steht sie jetzt als „Letzter Start" da —
  mit Skript, Befehlszeile und dem, was das Protokoll hergab — und lässt sich mit
  „Einrichtung mit Protokoll wiederholen" nachvollziehen.
- Die Laderate einer Zeile kommt jetzt aus dem, was der Spielordner wächst, nicht aus der Zahl des
  Sync-Dienstes: „177 MB/s" neben „577 B von 150,9 GB" war dessen Zähler für etwas anderes. Einmal
  pro Minute stehen beide Zahlen im Log nebeneinander.
- Passt ein Spiel in keinen Bibliotheksordner, wählt der Launcher den mit dem meisten freien Platz
  statt des Standardordners.
- Sortierung, Suche und Filter der Bibliothek bleiben erhalten, wenn man auf einen anderen Reiter
  wechselt. Die Überschrift „Einstellungen" entfällt wie die der anderen Seiten.

- Linux: Das Fenster blieb auf SteamOS (Steam Deck) weiß — WebKitGTK 2.42+ zeichnet über DMA-BUF,
  was etliche Grafikstacks mit einem leeren Fenster beantworten. Der Launcher schaltet den
  DMA-BUF-Renderer für sich ab (`WEBKIT_DISABLE_DMABUF_RENDERER=1`, außer der Benutzer setzt den
  Wert selbst) und schreibt in die zweite Logzeile, welche Einstellung galt und ob die Sitzung X11
  oder Wayland ist.

- Ein Download zählt jetzt alles, was in der Freigabe liegt, nicht nur die oberste Ebene: ein Paket,
  das als Ordner ankommt, stand sonst bei „1,6 KB von 115,5 GB", während der Sync mit voller Rate
  lief. Solange die Engine eine Rate meldet, gilt ein Download außerdem nie als hängend — ein
  einzelnes riesiges Paket zählt die Engine erst, wenn es fertig ist.
- Ein leerer Spielordner aus einem abgebrochenen Versuch bindet ein Spiel nicht mehr an diese
  Platte: die Wahl des Bibliotheksordners übergeht ihn, und beim Installieren wird er entfernt.
  Damit landet ein 115-GB-Spiel auf der Platte mit Platz, und der Fortschritt wird dort gemessen,
  wo wirklich geladen wird. Die Downloadseite zeigt den Zielordner.
- Meldet der Sync-Dienst für eine Freigabe einen Fehler („Ordner nicht gefunden", nachdem ein Spiel
  entfernt wurde), sagt der Launcher das mit einem Schritt dazu — und „Reparieren" meldet die
  Freigabe neu an, statt den kaputten Zustand als „schon hinzugefügt" zu übernehmen.
- Der Installer beendet die mitgelieferte Sync-Engine jetzt wirklich: die Zeichenkette für
  PowerShell war in NSIS falsch geschrieben (`\"` ist dort kein Escape), der Vergleich lief ins Leere.
- Das Logo der LANPage verschwand, wenn ein Farbschema gewählt war oder der Webserver
  `logo.png` ohne Bild-Typ auslieferte. Über das Logo entscheiden jetzt die ersten Bytes, und es
  gehört zur Marke, nicht zum Farbschema. Ein `theme.json` darf seine Dateien relativ nennen
  (`"logo": "logo.png"`), sie werden gegen die LANPage aufgelöst.
- Statusleiste: die Übertragungsraten kommen jetzt sekündlich statt alle 15 Sekunden (die
  Teilnehmerzahl weiter im 15-Sekunden-Takt — sie kostet eine Abfrage pro Freigabe).
- Diagnose und Downloads ohne Überschrift (die Reiterzeile sagt es schon); der Zustand der Prüfung
  steht als Haken oder Warndreieck oben in der Ecke, der Erklärtext darüber entfällt. Beide Seiten
  stehen als Spalte in der Mitte.
- Diagnose, „Letzter Start": zeigt jetzt auch die Ausgabe des Programms. Ein Startskript, das ein
  leeres cmd-Fenster öffnet und endet, hinterließ bisher nichts, worüber man reden konnte.
- Die Einstellung „Statistik an die LANPage senden" entfällt; die Statistik geht immer an die
  LANPage, sofern eine `stats_url` in der `launcher.ini` steht — auch auf einem PC, auf dem der
  Haken vorher aus war. (Die LANPage zeigt damit, wer gerade was spielt; der ETI-Launcher kennt
  dafür ebenfalls keine Einstellung.)
- Lässt sich ein leerer Spielordner nicht entfernen (der Sync-Dienst hält ihn noch), lädt der
  Launcher dorthin, wo er das Spiel später auch sucht, statt in zwei Ordner gleichzeitig zu
  zeigen. Die Wahl des Ordners trifft jetzt überall dieselbe Regel.

- `theme.json` in Version 2: eigene Farben für Kopf- und Statusleiste, eine Überlagerungsfarbe über
  dem Hintergrundbild und eine Schriftart der LANPage (`fontFamily` samt `fontFaces`, nur http(s)
  oder `data:`). Alles ist optional und fällt auf die bisherigen Werte zurück; dieselben Angaben gehen
  auch über `theme_*`-Schlüssel in der `launcher.ini`, wenn eine LANPage keine `theme.json` hat.
  Die Schriftart nennt ihre Dateien (`fontFaces`), die `@font-face`-Regeln schreibt der Launcher
  selbst — eine LANPage bekommt eine Schrift in den Launcher, aber kein Stylesheet.
- Startprofile für FlatOut 2 und Call of Duty 2 aus den macETI-LAN-Notizen. Beide nennen keine
  Startdatei (die ist für diese Pakete nicht belegt), sondern nur die Hinweise — die Startdatei
  kommt weiterhin aus dem Windows-Skript oder von Hand.
- README: Abschnitt „Installing a build" mit den drei Linux-Formaten und dem Weg über das AppImage
  auf dem Steam Deck.

- Die Statusleiste zeigt die Gesamt-Download- und -Uploadrate über alle Freigaben, auch wenn sie
  null ist: „nichts bewegt sich“ ist ebenfalls eine Antwort.
- Die Teilnehmerzahl war die Summe über alle Freigaben. Ein Sync-Server, der 28 Freigaben anbietet,
  wurde dadurch zu „28 Teilnehmer“. Gezählt wird jetzt die größte Zahl, die eine einzelne Freigabe
  meldet — ohne Geräte-IDs pro Freigabe ist das die ehrliche Zahl.
- Der Knopf „Katalog aktualisieren“ entfällt. Der Katalog wird geladen, sobald sich `game.db` oder
  `assets.eti` ändern, und zusätzlich alle fünf Minuten blind — eine Änderung innerhalb derselben
  Sekunde sieht man an Größe und Zeitstempel sonst nicht.
- Diagnoseseite: ein dauerhafter Knopf „Resilio-Oberfläche“ neben den Logs, nicht mehr nur als
  Reparatur eines Problems.
- Diagnoseseite: „Letzter Start“ nennt Programm, Befehlszeile, Arbeitsverzeichnis und Ergebnis
  (läuft noch, beendet mit Code n, oder der Fehler beim Starten). Ein Spiel, das nur ein leeres
  cmd-Fenster öffnet, ließ sich bisher nur im Log nachvollziehen.

- Die dokumentierte Resilio-API liefert `size` (was hier liegt), `total_size` (was die Freigabe
  hat) und `down_speed`. Gelesen wurde `size` als Gesamtgröße, die Rate gar nicht: daher „0 B von
  782 B“ und nie eine Übertragungsrate. Jetzt stimmen Fortschritt, Gesamtgröße und Rate.
- Während „Wird geprüft“ und „Wird entpackt“ zeigt die Downloadseite keine Rate, keine Teilnehmer
  und keine Quellen mehr — dort bewegt sich nichts über das Netz, und die Null sah aus wie ein
  hängender Download.
- Der Installer beendet vor dem Überschreiben die mitgelieferte Sync-Engine. Bisher schlug das
  Update an der laufenden `Resilio Sync.exe` fehl, obwohl der Launcher selbst sauber geschlossen
  wurde. Eine selbst installierte Resilio-Kopie bleibt unangetastet.
- Ein neues Spiel landet im Standardordner, solange dort Platz ist, sonst im Bibliotheksordner mit
  dem meisten freien Platz — bisher immer im obersten, auch wenn er zu klein war.
- Die Einstellung „Sync-Modus“ entfällt: Der Launcher steuert seine Engine selbst, und ob jemand
  im Netz ist, findet er von allein heraus. Der Haken „Nur im LAN synchronisieren“ bleibt.
- Downloadseite ohne den Erklärtext darüber; die Diagnose-Zusammenfassung nennt nicht mehr die
  Liste der geprüften Bereiche und sieht nicht mehr aus wie ein Hinweis unter Hinweisen.

- „Ordner öffnen“ und „Link öffnen“ meldeten „Erledigt“ — als wäre ein Download fertig. Sie sagen
  jetzt, was sie getan haben.
- Das Registrieren einer Spielfreigabe steht im Log, mit Ordner und Fehler. Ein Download, der nie
  anfängt, war bisher im Log nicht von einem unterschieden, der nie angefordert wurde.
- Farbschemata: Die Farben für Status-Abzeichen (läuft, spielbereit, angehalten, Fehler, Update)
  liegen in jedem Schema weit genug auseinander, um sie unterscheiden zu können; ein Test hält das
  fest. Im hellen Schema war der Akzent dem Warnton zu ähnlich.

- Farbschemata: „Pinkes Einhorn“ heißt jetzt „Pink“, „Beispiel-LAN Orange“ nur noch „Orange“, dazu
  kommen Blau, Grün und Rot. Eine gespeicherte Auswahl wird mit umbenannt.
- Die Bibliothek ist standardmäßig alphabetisch sortiert; die Sortierung „Reihenfolge des
  Katalogs“ entfällt.

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
