# AT-Befehle der Dragino-NB-IoT-Sensoren (D2x-NB, WQS-NB)

Verbindliches Inventar als Grundlage für die Klick-Oberfläche in Aircord: Welche
Einstellung gibt es, wie wird sie gesetzt und zurückgelesen, wie riskant ist sie,
und welche davon braucht ein Laie im Feld.

Stand der Recherche: 2026-11-17. Ausgewertet wurde ausschließlich offizielle
Dragino-Dokumentation (siehe Quellen). Nichts davon ist am Gerät geprüft.

**Lesehilfe**

- Wörtlich ist nur die Befehlssyntax in `Code`. Beschreibungen sind aus der Doku
  übertragen bzw. paraphrasiert.
- Die Webseiten wurden über eine automatische Extraktion gelesen, die
  zusammenfasst und je Abruf bei etwa 100 000 Zeichen abschneidet (Rest per
  weiterem Abruf). Q6 wurde als PDF-Text gelesen. Die Widersprüche in Abschnitt 5
  sind am Original mit gezielten Zitaten gegengeprüft.
- **unsicher** markiert Angaben, die die Doku nicht eindeutig hergibt, die sich
  zwischen Quellen widersprechen oder die nur aus einem anderen Dragino-Gerät
  übertragen sind.
- „nur Index“ heißt: Der Befehl steht nur mit einer Zeile in der Befehlsliste,
  Syntax und Wertebereich sind nicht dokumentiert. Diese Lücken sind nicht
  aufgefüllt.
- Die Spalten **Laien-Bezeichnung**, **Bedienelement** und **Risiko** sind eine
  Einschätzung für Aircord, keine Dragino-Angabe. Wo die Doku das Risiko belegt
  (z. B. `AT+FDR` vs. `AT+FDR1`), steht die Quelle dabei.
- Gerätevarianten: **D2x** = D20-NB, D20S-NB, D22-NB, D23-NB (ein gemeinsames
  Handbuch). **WQS** = WQS-NB/NS. **NB allg.** = gilt laut der allgemeinen
  Konfigurationsseite für alle -NB/-NS-Modelle mit BC660K-GL-Modul.

## 1. Quellen

| ID | URL | Gerät | Firmware / Stand der Doku |
| -- | --- | ----- | ------------------------- |
| Q1 | https://docs.dragino.com/docs/NB-IoT/temperature-humidity-sensors/d20-d22-d23-nb/ (inhaltsgleich: https://wiki.dragino.com/docs/NB-IoT/temperature-humidity-sensors/d20-d22-d23-nb/) | D20-NB, D20S-NB, D22-NB, D23-NB | Kein Revisionsdatum (Footer © 2026). Nennt Firmware v1.1.2, v1.2.1, v1.3.0, v1.3.4. Abgerufen 2026-11-17. |
| Q2 | https://wiki.dragino.com/docs/NB-IoT/general-configuration/nb-ns-models-nb-iot/ | alle -NB/-NS (BC660K-GL) | Kein Revisionsdatum. Enthält Stack-Changelog BC660K 004/005/006 und Versionstabelle FDR1. Abgerufen 2026-11-17. |
| Q3 | https://docs.dragino.com/docs/NB-IoT/water-quality-sensors/wqs-nb/ | WQS-NB/NS | Kein Revisionsdatum. Nennt Firmware v1.0.2 und v1.1.0. Abgerufen 2026-11-17. |
| Q4 | https://docs.dragino.com/docs/NB-IoT/firmware-update/ota-upgrade-for-nb-iot-lte-m-end-node/ | -NB/-NS/-NB2, -CB/-CS/-CB2 | Kein Revisionsdatum. OTA über MQTT-Server. Abgerufen 2026-11-17. |
| Q5 | https://docs.dragino.com/docs/NB-IoT/firmware-update/ble-upgrade-for-nb-iot-lte-m-end-node/ | -NB/-NS, -CB/-CS | Kein Revisionsdatum. Firmware-Update per BLE (App „Devices.tool“). Abgerufen 2026-11-17. |
| Q6 | https://www.dragino.com/downloads/downloads/NB-IoT/NBSN95/DRAGINO_NBSN95-NB_AT%20Commands_v1.2.0.pdf | NBSN50-95 (**anderes Gerät**) | AT Command Sets V1.2, 2021-06-28. Nur als Analogie für Antwortformat und `AT+CFG`-Ausgabe, **unsicher** für D2x/WQS. |
| Q7 | https://wiki.dragino.com/docs/NB-IoT/temperature-humidity-sensors/s31-nb/ | S31-NB (gleiche -NB-Plattform) | Kein Revisionsdatum. Nur für die Passwort-Regel herangezogen. Abgerufen 2026-11-17. |
| Q8 | https://docs.dragino.com/docs/NB-IoT/firmware-update/ | -NB/-CB, BC660K-GL | Übersicht der Update-Wege. Abgerufen 2026-11-17. |

Nicht verwendet: D20-CB/D2x-CB (https://docs.dragino.com/docs/NB-IoT/temperature-humidity-sensors/d20-d22-d23-cb/).
Die -CB-Varianten haben ein BG95-Modul (LTE-M) und einen abweichenden Befehlssatz.
Sie fallen nicht unter dieses Inventar.

## 2. Allgemeines zur Befehlsschnittstelle

Diese Punkte betreffen jede Einstellung und bestimmen, wie die Oberfläche Werte
liest und schreibt.

- **Abfrageform:** `AT+<CMD>=?` liefert den aktuellen Wert, `AT+<CMD>?` die
  Hilfe zum Befehl, `AT+<CMD>=<wert>` setzt ihn, `AT+<CMD>` führt ihn aus (Q1,
  Abschnitt AT-Befehlssatz). Ausnahme: `AT+QBAND?` ist laut Q2 die Abfrage der
  Bänder.
- **Antwortformat:** Wert, Zeilenumbruch, danach Status `OK`. Fehler:
  `AT_ERROR`, `AT_PARAM_ERROR`, `AT_TEST_PARAM_OVERFLOW`, `AT_RX_ERROR`. `ATZ`
  liefert keinen Status (Q6). **unsicher**, weil nur für NBSN95 (2021)
  dokumentiert. Q1/Q2 zeigen für D2x nur einzelne Antworten wie `0,0` + `OK`.
- **`AT+CFG`-Ausgabe:** Für D2x und WQS ist keine Beispielausgabe dokumentiert.
  Q6 zeigt für NBSN95 eine Zeile je Einstellung im Setzformat, z. B.
  `AT+CFGMOD=1`, `AT+SERVADDR=123.57.29.36:5683`, `AT+TDC=600`, abgeschlossen mit
  `OK`. Ob D2x/WQS dasselbe Format ausgeben, ist **offen** (siehe Abschnitt 5).
  In den Tabellen steht deshalb bei „Rücklesen“ die Abfrage `=?` und nur dort eine
  Beispielantwort, wo Q1–Q3 eine zeigen.
- **Zugang:** Nach dem Verbinden muss das sechsstellige Passwort vom Etikett
  eingegeben werden (Etikett zeigt `AT+PIN=xxxxxx`, eingegeben werden nur die
  sechs Zeichen) (Q1, Q3, Q5).
- **Zeilenende:** Jeder Befehl braucht einen Zeilenumbruch am Ende (Q1, Q3).
- **BLE-Fenster:** BLE ist nur nach Tastendruck (Uplink oder Aktivierung),
  Einschalten oder Reset aktiv und schaltet sich nach 60 s ohne Verbindung ab
  (Q1, Q3). Q5 nennt für den Update-Fall ein Fenster von 8–15 s.
- **Tasten (D2x):** 1–3 s drücken sendet einen Uplink (blaue LED). Über 3 s
  drücken öffnet ein OTA-Fenster von 12 s (grüne LED). Fünfmal drücken versetzt
  das Gerät in Tiefschlaf (rote LED 5 s) (Q1, Q2). Den Tiefschlaf verlässt es nur
  per Reset-Befehl über die serielle Schnittstelle oder durch 5 s Halten der
  ACT-Taste (Q2).
- **Selbst-Reset:** Meldet das Gerät 12 TDC-Zyklen in Folge Signalstärke 99,
  setzt es sich selbst zurück (Q2, Stack 005/006).
- **Fernkonfiguration:** Die meisten Befehle gibt es auch als Downlink (Hex oder
  JSON `{"Config":"[AT+TDC=360;AT+CSQTIME=5]"}`, ab Stack 005) (Q2). Für Aircord
  (lokal per BLE) ist das nur Hintergrund.

## 3. Befehle nach Aufgaben

Spalten in allen Tabellen: **Befehl** (Setzen / Abfragen) · **Bedeutung** ·
**Wertebereich / Einheit / Default** · **Laien-Bezeichnung** · **Bedienelement**
· **Rücklesen** (mit Beispielantwort, wo dokumentiert) · **Risiko** ·
**Varianten** · **Quelle**.

### 3.1 Gerät identifizieren und Zustand ablesen

| Befehl | Bedeutung | Wertebereich / Einheit / Default | Laien-Bezeichnung | Bedienelement | Rücklesen | Risiko | Varianten | Quelle |
| ------ | --------- | -------------------------------- | ----------------- | ------------- | --------- | ------ | --------- | ------ |
| `AT` | Verbindungstest („Attention“) | – | Verbindung prüfen | Aktion | Antwort `OK` (Q6, Analogie) | harmlos | D2x, WQS | Q1, Q3 |
| `AT?` | Kurzhilfe, listet alle Befehle | – | – (nur Experte) | Aktion | Hilfetext | harmlos | D2x, WQS | Q1 |
| `AT+CFG` | Gibt alle Einstellungen aus | – | Einstellungen anzeigen / sichern | Aktion + Nur-Anzeige | Format für D2x/WQS **nicht dokumentiert**. Analogie Q6: eine Zeile je Einstellung, z. B. `AT+TDC=600`, dann `OK` (**unsicher**) | harmlos | D2x, WQS, NB allg. | Q1, Q2, Q3, Q6 |
| `AT+DEUI=<id>` / `AT+DEUI=?` | Geräte-ID lesen oder setzen | Q2: `AT+CFG` zeigt eine 8-Byte-HEX-DEUI. Länge beim Setzen für D2x/WQS **nicht dokumentiert** | Geräte-ID | Nur-Anzeige | `AT+DEUI=?`, keine Beispielantwort für D2x/WQS | harmlos (Lesen) / Gerät verliert Netz (Setzen: Server ordnet Daten evtl. nicht mehr zu, Einschätzung) | D2x, WQS | Q1, Q2, Q3 |
| `AT+MODEL` / `AT+MODEL=?` | Modul-/Modellinformation | nur Index | Gerätetyp und Firmware | Nur-Anzeige | Für WQS keine Beispielantwort. Analogie Q6: `NBSN95,v1.2.0` (**unsicher**) | harmlos | WQS (in Q1 für D2x nicht gelistet) | Q3, Q6 |
| `AT+LDATA` / `AT+LDATA=?` | Zuletzt gesendete Nutzdaten | nur Index | Letzte Sendung | Nur-Anzeige | Keine Beispielantwort für D2x/WQS. Analogie Q6: Hex-String | harmlos | D2x, WQS | Q1, Q3 |
| `AT+GETSENSORVALUE` | Aktueller Messwert | nur Index, Syntax und Parameter **nicht dokumentiert** | Messwerte jetzt ablesen | Aktion + Nur-Anzeige | nicht dokumentiert | harmlos (vermutet) | D2x, WQS | Q1, Q3, Q7 |
| `AT+CDP` / `AT+CDP=0` | Zwischengespeicherte Datensätze lesen oder löschen | D2x: bis 32 Gruppen à ~100 Byte. Löschform `AT+CDP=0` nur aus Q6 (**unsicher**) | Gespeicherte Messungen | Nur-Anzeige (Lesen) | `AT+CDP`, Ausgabeformat für D2x/WQS nicht dokumentiert | harmlos (Lesen) / irreversibel (Löschen: Daten weg) | D2x, WQS | Q1, Q3, Q6 |
| `AT+GETLOG` / `AT+GETLOG=1` | Uplink-Log abfragen. `=1` lädt gespeicherte serielle Logs mit dem nächsten Paket hoch (Stack 006) | – | Diagnose-Log senden | Aktion | Log erscheint am Server, nicht in der Konsole (Q2) | harmlos (erzeugt Datenverkehr) | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+QENG=0` | Zeigt das tatsächlich genutzte Band nach der Einbuchung | – | Aktives Funkband | Nur-Anzeige | Ausgabeformat nicht dokumentiert | harmlos | NB allg. | Q2 |
| `AT+PLDTA=<n>` | Die letzten n Datensätze ausgeben | n: Anzahl, Grenze nicht dokumentiert | – (Experte) | Text mit Validierung (Zahl) | Ausgabe nicht dokumentiert | harmlos | D2x (ab v1.3.4, Datalog) | Q1 |
| `AT+PDTA=<start>,<stop>` | Datensätze nach Flash-Seiten ausgeben | Seitenbereich, Grenzen nicht dokumentiert | – (Experte) | Text mit Validierung | Ausgabe nicht dokumentiert | harmlos | D2x (ab v1.3.4) | Q1 |

Für **Firmware-Version** und **Signalstärke** gibt es in Q1–Q3 keinen eigenen
Abfragebefehl, siehe Abschnitt 5.

### 3.2 Netz und SIM

| Befehl | Bedeutung | Wertebereich / Einheit / Default | Laien-Bezeichnung | Bedienelement | Rücklesen | Risiko | Varianten | Quelle |
| ------ | --------- | -------------------------------- | ----------------- | ------------- | --------- | ------ | --------- | ------ |
| `AT+APN=<apn>` / `AT+APN=?` | APN der SIM | Text. Beispiele: `iot.1nce.net` (1NCE), `simbase`, `ctnb` (China Telecom). Default nicht dokumentiert | Zugangspunkt (APN) des Mobilfunkanbieters | Auswahl (Anbieter) + Text mit Validierung | `AT+APN=?`, keine Beispielantwort dokumentiert | Gerät verliert Netz | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+QCGDEFCONT=<…>` | PDN-Verbindung beim Einschalten | Beispielwert `iot.1nce.net/xxx`, Syntax sonst **nicht dokumentiert** | – (Experte) | Text mit Validierung | nicht dokumentiert | Gerät verliert Netz | NB allg. | Q2 |
| `AT+QBAND=<n>,<b1>[,<b2>…]` / `AT+QBAND?` | Funkbänder festlegen. Erstes Argument = Anzahl der Bänder | Modul kann B1, B2, B3, B4, B5, B8, B12, B13, B14, B17, B18, B19, B20, B25, B28, B66, B70, B85. Beispiele: `2,8,20` Europa, `10,8,20,28,2,4,12,13,66,85,5` global. Default nicht dokumentiert | Funkband / Region | Auswahl (Region) | `AT+QBAND?`, Antwortformat nicht dokumentiert | Gerät verliert Netz | NB allg. (in Q1/Q3 nicht gelistet, **unsicher** für D2x/WQS) | Q2 |
| `AT+CSQTIME=<min>` / `AT+CSQTIME=?` | Dauer der Netzsuche | Minuten, Default 5 | Wie lange nach Netz suchen | Dauer (Minuten) | `AT+CSQTIME=?`, keine Beispielantwort | harmlos (längere Suche kostet Batterie) | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+QCOPS=<mccmnc>` | Netzbetreiber fest vorgeben | Beispiel `46000` (China Mobile). Ab Stack 005 | Netzbetreiber fest einstellen | Text mit Validierung (5–6 Ziffern, **unsicher**) | nicht dokumentiert | Gerät verliert Netz | NB allg. | Q2 |
| `AT+DNSCFG=<…>` | DNS-Server | Bis 40 Zeichen (ab Stack 004, vorher 19). Syntax **nicht dokumentiert** | – (Experte) | Text mit Validierung | nicht dokumentiert | Gerät verliert Netz (Server bei falschem Wert nicht auflösbar) | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+DNSTIMER=<h>` | Regelmäßige Namensauflösung | Stunden, Default nicht dokumentiert | – (Experte) | Dauer (Stunden) | nicht dokumentiert | harmlos | D2x | Q1 |
| `AT+BKDNS=<a>,<b>,<c>` / `AT+BKDNS=?` | Dynamische DNS-Auflösung: a 1 = keine Aktualisierung, 2 = periodisch; b Intervall; c Ersatz-IP im `SERVADDR`-Format | b in Stunden. Default D2x `1,0,NULL`, WQS laut Q3 `0,0,NULL` (**Widerspruch**). D2x ab v1.3.0, WQS ab v1.0.2 | – (Experte) | Auswahl + Dauer + Text mit Validierung | `AT+BKDNS=?` → `1,0,NULL` dann `OK` (Q1, Q2) | Gerät verliert Netz (Server bei falscher Ersatz-IP nicht erreichbar) | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+NTP="<server>"` | Eigener Zeitserver | Beispiel `"s1a.time.edu.cn"`. Ab Stack 005 | – (Experte) | Text mit Validierung | nicht dokumentiert | harmlos (falsche Zeit stört Zeitstempel) | NB allg. | Q2 |

### 3.3 Server und Protokoll

| Befehl | Bedeutung | Wertebereich / Einheit / Default | Laien-Bezeichnung | Bedienelement | Rücklesen | Risiko | Varianten | Quelle |
| ------ | --------- | -------------------------------- | ----------------- | ------------- | --------- | ------ | --------- | ------ |
| `AT+PRO=<x>,<y>` / `AT+PRO=?` | Übertragungsprotokoll und Nutzdatenformat | x: 1 CoAP, 2 UDP, 3 MQTT, 4 TCP. y: 0 HEX, 1 ThingSpeak, 3 ThingsBoard, 5 JSON. Q2 nennt für TCP-JSON `4,1` (**Widerspruch** zu y=5). Default nicht dokumentiert | Übertragungsart und Datenformat | Auswahl (zwei Felder oder fertige Kombinationen) | `AT+PRO=?`, keine Beispielantwort für D2x/WQS | Gerät verliert Netz (Server nicht erreichbar) | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+SERVADDR=<host>,<port>` / `AT+SERVADDR=?` | Serveradresse und Port | Host (IP oder Name) und Port. Leerzeichen werden ab Stack 004 entfernt. Maximallänge für D2x/WQS **nicht dokumentiert**. Für CoAP Port fest 5683 (Q2) | Server-Adresse und Port | Text mit Validierung (Host; Port 1–65535) | `AT+SERVADDR=?`, keine Beispielantwort für D2x/WQS | Gerät verliert Netz (Server nicht erreichbar) | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+CLIENT=<id>` / `AT+CLIENT=?` | MQTT-Client-ID | Text. Beispiel `NULL` wenn unbenutzt (ThingsEye). Länge **nicht dokumentiert** für D2x/WQS | MQTT-Client-ID | Text mit Validierung | `AT+CLIENT=?`, keine Beispielantwort | Gerät verliert Netz (Server nicht erreichbar) | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+UNAME=<user>` / `AT+UNAME=?` | MQTT-Benutzername | Text, wie oben | MQTT-Benutzer | Text mit Validierung | `AT+UNAME=?`, keine Beispielantwort | Gerät verliert Netz (Server nicht erreichbar) | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+PWD=<pw>` / `AT+PWD=?` | MQTT-Passwort (nicht das Gerätepasswort) | Text, wie oben. Bei Tago: Geräte-Token | MQTT-Passwort | Text mit Validierung (maskiert) | `AT+PWD=?`, keine Beispielantwort | Gerät verliert Netz (Server nicht erreichbar) | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+PUBTOPIC=<topic>` / `AT+PUBTOPIC=?` | MQTT-Topic zum Senden | Text | MQTT-Sende-Topic | Text mit Validierung | `AT+PUBTOPIC=?`, keine Beispielantwort | harmlos (Daten landen im falschen Topic, korrigierbar) | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+SUBTOPIC=<topic>` / `AT+SUBTOPIC=?` | MQTT-Topic für Downlinks | Text | MQTT-Empfangs-Topic | Text mit Validierung | `AT+SUBTOPIC=?`, keine Beispielantwort | harmlos (Fernkonfiguration kommt nicht an, korrigierbar) | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+MQOS=<0..2>` | MQTT-QoS | 0–2. Default ab Stack 006 `0`, vorher `2` | MQTT-Zustellgarantie | Auswahl (0/1/2) | nicht dokumentiert | harmlos | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+TLSMOD=1,<m>` | TLS-Modus | m: 0 TLS ohne Zertifikatsprüfung, 1 nur Server, 2 Server und Client. Für 1/2 müssen Zertifikate vorher geschrieben sein. Default nicht dokumentiert | Verschlüsselung (TLS) | Auswahl | nicht dokumentiert | Gerät verliert Netz (Server ohne Zertifikate nicht erreichbar) | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+URI1=<…>` … `AT+URIn=<…>` | CoAP-Optionen (Pfad) | Q1: `URI1`–`URI4`, Form `name,length,"value"`. Q2: `AT+URI1=11,38,"i/…"`. Q3: `URI1`–`URI8`, `AT+URI1=11,"i"` (**Widerspruch**) | CoAP-Pfad | Text mit Validierung | nicht dokumentiert | Gerät verliert Netz (Server nicht erreichbar) | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+DOWNTE=0,<a>` / `AT+DOWNTE=?` | Downlink-Debugmodus | a: 0 aus, 1 an. Default `0,0`. D2x ab v1.3.0 | – (Experte) | Schalter | `AT+DOWNTE=?` → `0,0` dann `OK` | harmlos | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+RXDL=<ms>` / `AT+RXDL=?` | Empfangsfenster nach dem Senden verlängern | ms, Beispiel `1000`. Default nicht dokumentiert | – (Experte) | Dauer (ms) | `AT+RXDL=?`, keine Beispielantwort für D2x/WQS | harmlos (Batterie) | D2x, WQS | Q1, Q3 |
| `AT+REDPT=<0\|1>` / `AT+REDPT=?` | Unbestätigte Pakete erneut senden | 0 aus (Default), 1 an. Bis 1504 Datensätze offline | Verlorene Daten nachsenden | Schalter | `AT+REDPT=?` → `0` dann `ok` (Q1) | harmlos | D2x, NB allg. | Q1, Q2 |

### 3.4 Messen und Senden

| Befehl | Bedeutung | Wertebereich / Einheit / Default | Laien-Bezeichnung | Bedienelement | Rücklesen | Risiko | Varianten | Quelle |
| ------ | --------- | -------------------------------- | ----------------- | ------------- | --------- | ------ | --------- | ------ |
| `AT+TDC=<s>` / `AT+TDC=?` | Sendeintervall | Sekunden, Default 7200 (2 h). Minimum **nicht dokumentiert** | Sendeintervall | Dauer | `AT+TDC=?`, keine Beispielantwort für D2x/WQS. Analogie Q6: `600` dann `OK` | harmlos (kurz = Batterie leer) | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+TR=<s>` / `AT+TR=?` | Messintervall (Aufzeichnung) | Sekunden, Minimum 180. Default Q1 900, Q2 1800 (**Widerspruch**) | Messintervall | Dauer | `AT+TR=?`, keine Beispielantwort | harmlos | D2x, NB allg. | Q1, Q2 |
| `AT+NOUD=<n>` / `AT+NOUD=?` | Anzahl gespeicherter Messungen je Sendung | Default 8, Maximum 32, Untergrenze nicht dokumentiert. Laut Q1 durch Clock Logging abgelöst | Messungen pro Sendung | Text mit Validierung (max. 32) | `AT+NOUD=?`, keine Beispielantwort | harmlos | D2x, NB allg. | Q1, Q2 |
| `AT+CLOCKLOG=<a>,<b>,<c>,<d>` | Zeitgesteuerte Messung | a 0/1 an; b Startsekunde 0–3599 oder 65535 (= nach erster Sendung); c Intervall 0–255 min; d Einträge je Sendung, max. 32. „Aus“ laut Doku `1,65535,0,0`. Setzt synchronisierte Serverzeit voraus. D2x ab v1.2.1 | Messung zur vollen Zeit | Schalter + Dauer + Text mit Validierung (Anzahl) | nicht dokumentiert | harmlos | D2x, WQS | Q1, Q3 |
| `AT+POWER_SAVE=<a>,<b>,<c>` / `AT+POWER_SAVE=?` | Senden nur in einem Zeitfenster (UTC) | a: 0 = Zeitplan an, 1 = aus; b/c Start-/Endstunde. Default `1,0,0`. Beispiel `0,8,16`. D2x ab v1.3.4 | Sendezeiten einschränken | Schalter + Auswahl (Start- und Endstunde) | `AT+POWER_SAVE=?` → `1,0,0` dann `ok` (Q1) | harmlos (außerhalb des Fensters keine Daten) | D2x | Q1 |
| `AT+CFGMOD=<n>` / `AT+CFGMOD=?` | Arbeitsmodus | WQS: 1 Interrupt (Default), 2 Zählen. Für D2x nur Index | – (Experte) | Auswahl | `AT+CFGMOD=?`, keine Beispielantwort | harmlos | D2x (nur Index), WQS | Q1, Q3 |
| `AT+INTMOD=<0..3>` / `AT+INTMOD=?` | Auslöser des externen Interrupts | 0 aus, 1 steigende und fallende Flanke, 2 fallend, 3 steigend. D2x: Pin ab v1.1.2 PA8 statt PB15 | Sendung bei externem Signal | Auswahl | `AT+INTMOD=?`, keine Beispielantwort | harmlos | D2x, WQS | Q1, Q3 |
| `AT+EXT=<n>` / `AT+EXT=?` | Zählerstand setzen (Zählmodus) | Default 0. Beispiel `100` | Zählerstand | Text mit Validierung (Zahl) | `AT+EXT=?` → `0` dann `OK` (Q3; Q7 ebenso) | harmlos | D2x (nur Index), WQS | Q1, Q3 |
| `AT+5VT=<ms>` / `AT+5VT=?` | Einschaltdauer der 5-V-Sensorversorgung vor der Messung | ms. Beispiel `2000`. Default nicht dokumentiert | Vorlaufzeit Sensor | Dauer (ms) | `AT+5VT=?`, keine Beispielantwort | harmlos (zu kurz = falsche Messwerte, Einschätzung) | D2x (nur Index), WQS | Q1, Q3 |

Hinweis: Einen Befehl, der sofort eine Testsendung auslöst, nennen Q1–Q3 nicht.
Die Doku beschreibt dafür den Tastendruck 1–3 s (Q1).

### 3.5 Sicherheit

| Befehl | Bedeutung | Wertebereich / Einheit / Default | Laien-Bezeichnung | Bedienelement | Rücklesen | Risiko | Varianten | Quelle |
| ------ | --------- | -------------------------------- | ----------------- | ------------- | --------- | ------ | --------- | ------ |
| sechs Zeichen vom Etikett (`AT+PIN=xxxxxx` auf dem Etikett) | Schaltet die AT-Kommandozeile frei | 6 Zeichen, gerätespezifisch | Gerätepasswort | Text mit Validierung (6 Zeichen, maskiert) | kein Rücklesen. Bestätigung „Password correct“ laut Q5 nur in der Dragino-App beschrieben | harmlos | D2x, WQS, NB allg. | Q1, Q3, Q5 |
| `AT+PWORD=<xxxxxx>` | Gerätepasswort ändern | 6 Zeichen. Nur Kleinbuchstaben laut Q7 (S31-NB), in Q1/Q3 nicht erwähnt (**unsicher**) | Gerätepasswort ändern | Text mit Validierung (zweimal eingeben) | Abfrage nicht dokumentiert. Q6 zeigt `AT+PWORD=?` mit Klartext-Antwort, **unsicher** für D2x/WQS | irreversibel im Feld (vergessenes Passwort lässt sich laut Q2 nur per STM32CubeProgrammer auslesen; Einschätzung) | D2x, WQS | Q1, Q3, Q7, Q2 |
TLS (`AT+TLSMOD`) steht unter 3.3. Zertifikate für TLS werden laut Q2 mit „Standard-AT-Befehlen“ geschrieben (ab
Stack 006). Die Syntax steht nicht auf den ausgewerteten Seiten.

### 3.6 Wartung

| Befehl | Bedeutung | Wertebereich / Einheit / Default | Laien-Bezeichnung | Bedienelement | Rücklesen | Risiko | Varianten | Quelle |
| ------ | --------- | -------------------------------- | ----------------- | ------------- | --------- | ------ | --------- | ------ |
| `ATZ` | MCU-Neustart, übernimmt Einstellungen | – | Neu starten | Aktion (mit Bestätigung) | Keine `OK`-Antwort, das Gerät startet neu. Startbanner mit Image-/Stack-Version laut Q6 (**unsicher**) | Neustart (BLE-Verbindung bricht ab) | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+FDR` | Alle Parameter auf Werkszustand | – | Auf Werkseinstellungen zurücksetzen | Aktion (doppelte Bestätigung) | Startbanner. Nach Reset ist die Konfiguration weg | irreversibel (APN, Server, Intervall weg). Ob das Passwort auf das Etikett zurückgeht, ist **nicht dokumentiert** | D2x, WQS, NB allg. | Q1, Q2, Q3 |
| `AT+FDR1` | Werkszustand, Passwort bleibt | D20-NB ab v1.2.1 (Versionstabelle Q2). Für WQS in Q3 beschrieben | Zurücksetzen, Passwort behalten | Aktion (doppelte Bestätigung) | wie `AT+FDR` | irreversibel (Konfiguration weg) | D20-NB ≥ v1.2.1, WQS (D22/D23 **unsicher**) | Q2, Q3 |
| `AT+UPGRADE` | Startet OTA-Update vom MQTT-OTA-Server | Ab Stack 006. Braucht externen SPI-Flash (Seriennr. ab BC660GL26110001) und OTA-Firmware | Firmware aus dem Netz aktualisieren | Aktion (mit Bestätigung) | Fehler/Erfolg gehen als JSON an den Datenserver (Codes 1–7, 100) (Q4) | Neustart; irreversibel bei Modellwechsel (löscht Einstellungen, Q4) | NB allg. (Hardware-abhängig) | Q2, Q4 |
| `AT+OTASER=<host>,<port>` | OTA-Server | z. B. Port 1885 (ThingsEye) | – (Experte) | Text mit Validierung | nicht dokumentiert | harmlos | NB allg. (OTA-Hardware) | Q4 |
| `AT+OTACLT=<id>` | OTA-Client-ID/Token | Text | – (Experte) | Text mit Validierung | nicht dokumentiert | harmlos | NB allg. (OTA-Hardware) | Q4 |
| `AT+OTAUNAME=<user>` | OTA-Benutzername | Text | – (Experte) | Text mit Validierung | nicht dokumentiert | harmlos | NB allg. (OTA-Hardware) | Q4 |
| `AT+OTAPWD=<pw>` | OTA-Passwort | Text oder `NULL` | – (Experte) | Text mit Validierung (maskiert) | nicht dokumentiert | harmlos | NB allg. (OTA-Hardware) | Q4 |
| `AT+OTATITLE=<titel>` | Firmware-Titel, muss exakt zum Server passen | Groß-/Kleinschreibung zählt. Default modellabhängig (Beispiel `s31-nb`) | – (Experte) | Text mit Validierung | nicht dokumentiert | irreversibel (Titel eines anderen Modells führt zum Modellwechsel und löscht Einstellungen, Q4) | NB allg. (OTA-Hardware) | Q4 |
| `AT+OTAVER=<version>` | Ziel-/Vergleichsversion, optional | Standard: laufende Version | – (Experte) | Text mit Validierung | nicht dokumentiert | harmlos | NB allg. (OTA-Hardware) | Q4 |
| `AT+CLRDTA` | Datenspeicher (Datalog) löschen | – | Gespeicherte Messungen löschen | Aktion (mit Bestätigung) | nicht dokumentiert | irreversibel (Daten weg) | D2x (ab v1.3.4) | Q1 |
| `AT+SLEEP` | Schlafmodus lesen/setzen | nur Index | – (Experte) | Nur-Anzeige (bis Syntax geklärt) | nicht dokumentiert | Gerät verliert Netz (**unsicher**; Tiefschlaf lässt sich laut Q2 nur per Reset oder Taste verlassen) | WQS | Q3, Q2 |

Firmware-Update per BLE (Taste über 3 s bzw. Reset, dann Upload einer `.bin`)
läuft über die Dragino-App oder das Sensor Management Tool, nicht über AT-Befehle
der Firmware (Q1, Q5, Q8). Aircord bildet diesen Weg in `docs/firmware-update.md`
eigenständig nach. Q5: Die Passwortprüfung beim Update gilt erst ab Dragino-NB-
Bootloader v1.3.

### 3.7 Sensorspezifisch

| Befehl | Bedeutung | Wertebereich / Einheit / Default | Laien-Bezeichnung | Bedienelement | Rücklesen | Risiko | Varianten | Quelle |
| ------ | --------- | -------------------------------- | ----------------- | ------------- | --------- | ------ | --------- | ------ |
| `AT+TEMPALARM1=<min>,<max>` | Temperaturalarm Fühler 1 | Ganzzahlig. Einheit nicht ausdrücklich genannt, vermutlich °C (**unsicher**). min=0: Alarm über max; max=0: Alarm unter min; beide ≠0: außerhalb. Beispiel `20,20` = Alarm aus | Temperaturalarm Fühler 1 | Schalter + Text mit Validierung (min, max) | nicht dokumentiert | harmlos | D2x | Q1 |
| `AT+TEMPALARM2=<min>,<max>` | Temperaturalarm Fühler 2 | wie oben | Temperaturalarm Fühler 2 | wie oben | nicht dokumentiert | harmlos | D22-NB, D23-NB (Fühlerzahl laut Q1) | Q1 |
| `AT+TEMPALARM3=<min>,<max>` | Temperaturalarm Fühler 3 | wie oben | Temperaturalarm Fühler 3 | wie oben | nicht dokumentiert | harmlos | D23-NB | Q1 |
| `AT+CALPH=<4\|6\|9>` | pH-Kalibrierung, 3 Punkte | 4 = pH 4,00; 6 = 6,86; 9 = 9,18 | pH-Sonde kalibrieren | Aktion (Assistent je Pufferlösung) | Antwort z. B. `HEX:10 06 00 22 07 24 28 aa` dann `OK` | irreversibel (überschreibt die Kalibrierung; Einschätzung) | WQS (pH-Sonde) | Q3 |
| `AT+CALORP=<86\|256>` | ORP-Kalibrierung, 2 Punkte | 86 mV, 256 mV | ORP-Sonde kalibrieren | Aktion (Assistent) | Antwort z. B. `HEX:13 06 00 24 07 dc c9 1a` dann `OK` | wie oben | WQS (ORP-Sonde) | Q3 |
| `AT+CALEC=<1\|10>` | EC-Kalibrierung, 1 Punkt | 1 = 1413 µS/cm, 10 = 12,88 mS/cm | Leitfähigkeit kalibrieren | Aktion (Assistent) | Antwort z. B. `HEX:12 01 00 26 00 02 A2 A0` dann `OK` | wie oben | WQS (EC-Sonde) | Q3 |
| `AT+CALNTU=<0\|2\|4\|6\|8\|10>` | Trübungs-Kalibrierung | Stufen 0–10 (Bereich 0–1000 NTU) | Trübung kalibrieren | Aktion (Assistent) | Antwort z. B. `HEX:15 06 00 5E 00 01 2A CC` dann `OK` | wie oben | WQS (Trübungssonde) | Q3 |
| `AT+CALZD2=<n>` / `AT+CALZD4=<n>` | Trübungs-Kalibrierung TS200 / TS4000 | Abschnitt 4.5.5: 0 oder 1. Abschnitt 3.21: 0/2 bzw. 0/4 (**Widerspruch**) | Trübung kalibrieren (TS200/TS4000) | Aktion (Assistent) | nicht dokumentiert | wie oben | WQS (TS200/TS4000) | Q3 |

Für WQS gibt es keinen Befehl zur Auswahl des Sondentyps. Welche Sonden
angeschlossen sind, steht in einem Flag-Byte der Uplink-Nutzdaten (Q3). Für DO-
und Chlor-Sonden sind keine Kalibrierbefehle dokumentiert (Q3, Abschnitt Chlor
in der Abfrage abgeschnitten).

## 4. Empfehlung: Laien-Oberfläche und Expertenmodus

### Laien-Oberfläche (13 Einstellungen)

Ein Eintrag ist ein Bedienblock. Der Block kann mehrere Befehle bündeln, etwa
beim MQTT-Zugang.

1. **Verbindung prüfen** (`AT`): zeigt dem Laien sofort, ob Passwort und Link stimmen.
2. **Geräte-ID und Einstellungen anzeigen** (`AT+CFG`, `AT+DEUI=?`): Abgleich mit dem Etikett und Vorher-Stand für das Protokoll.
3. **Messwerte jetzt ablesen** (`AT+GETSENSORVALUE`, ersatzweise `AT+LDATA`): sichtbarer Funktionsnachweis im Feld, Syntax vorher am Gerät klären.
4. **Mobilfunkanbieter / APN** (`AT+APN`): ohne passenden APN kein Netz; als Anbieterauswahl mit Freitext-Ausweg.
5. **Funkband / Region** (`AT+QBAND`): Auswahl „Europa (B8, B20)“ usw. beschleunigt die Einbuchung und ist für Laien als Region verständlich.
6. **Übertragungsart** (`AT+PRO`): fertige Kombinationen wie „MQTT, JSON“ statt zweier Zahlen.
7. **Server und Port** (`AT+SERVADDR`): ohne korrekte Adresse kommen keine Daten an; Validierung von Host und Port.
8. **MQTT-Zugang** (`AT+CLIENT`, `AT+UNAME`, `AT+PWD`, `AT+PUBTOPIC`, `AT+SUBTOPIC`): als ein Block, nur sichtbar wenn MQTT gewählt ist.
9. **Sendeintervall** (`AT+TDC`): wichtigster Hebel für Batterielaufzeit und Datenmenge.
10. **Messintervall** (`AT+TR`, nur D2x): bestimmt die Auflösung der Messreihe unabhängig vom Senden.
11. **Temperaturalarm** (`AT+TEMPALARM1..3`, nur D2x, Anzahl je Fühlerzahl): typischer Anwenderwunsch, klare Min/Max-Semantik.
12. **Sonde kalibrieren** (`AT+CAL*`, nur WQS, als Assistent): wiederkehrende Feldaufgabe, braucht geführte Schritte statt Befehlen.
13. **Neu starten** (`ATZ`, mit Bestätigung): schließt jede Konfiguration ab und ist der erste Schritt bei Problemen.

### Nur im Expertenmodus

- `AT+PWORD`: vergessenes Passwort ist im Feld nicht behebbar.
- `AT+FDR`, `AT+FDR1`: löscht die gesamte Konfiguration.
- `AT+CDP` (Löschen), `AT+CLRDTA`: löscht Messdaten unwiderruflich.
- `AT+DEUI` setzen: kann die Zuordnung am Server brechen.
- `AT+UPGRADE`, `AT+OTA*`: hardwareabhängig, falscher Titel löscht Einstellungen.
- `AT+TLSMOD`: Modus 1/2 ohne vorher geschriebene Zertifikate trennt vom Server.
- `AT+URI1..n`: nur für CoAP, Syntax zwischen Quellen widersprüchlich.
- `AT+MQOS`: Default ist für fast alle Fälle passend.
- `AT+QCGDEFCONT`, `AT+QCOPS`: nur bei Netzproblemen mit bestimmten Anbietern nötig.
- `AT+CSQTIME`: nur bei schwachem Netz relevant, kostet Batterie.
- `AT+DNSCFG`, `AT+DNSTIMER`, `AT+BKDNS`, `AT+NTP`: Netzwerkdetails, Syntax teils undokumentiert.
- `AT+NOUD`, `AT+CLOCKLOG`, `AT+POWER_SAVE`: setzen Verständnis von Zwischenspeicher und Zeitsynchronisation voraus.
- `AT+REDPT`: sinnvoll, aber Auswirkung auf Batterie und Datenvolumen für Laien schwer abschätzbar.
- `AT+RXDL`, `AT+DOWNTE`: Debug- und Timing-Parameter.
- `AT+CFGMOD`, `AT+INTMOD`, `AT+EXT`, `AT+5VT`: nur mit externer Beschaltung relevant.
- `AT+GETLOG`, `AT+QENG=0`, `AT+PLDTA`, `AT+PDTA`, `AT?`, `AT+MODEL`, `AT+SLEEP`: Diagnose.

## 5. Unterschiede und offene Punkte

### Unterschiede zwischen Firmware-Versionen und Geräten

- **D2x-Firmware:** ab v1.1.2 Interrupt-Pin PA8 statt PB15; ab v1.2.1
  `AT+CLOCKLOG` und `AT+FDR1` (Versionstabelle Q2 nennt D20-NB); ab v1.3.0
  `AT+DOWNTE`, `AT+BKDNS`; ab v1.3.4 Datalog (`AT+PLDTA`, `AT+PDTA`,
  `AT+CLRDTA`) und `AT+POWER_SAVE` (Q1).
- **WQS-Firmware:** `AT+BKDNS` ab v1.0.2; v1.1.0 meldet zusätzlich die
  Innentemperatur einiger Sonden (Q3).
- **Passwort nach Update:** Neuere -NB-Firmware speichert das Passwort an einer
  anderen Adresse. Nach einem Update von alter auf neue Firmware ist das
  Passwort laut Q2 ungültig. Wurde es nie geändert, gilt das Etikett. Sonst lässt
  es sich mit STM32CubeProgrammer auslesen (alte Adresse `0x08019000`, neue
  `0x08025D00`). Welches Passwort das Gerät danach tatsächlich annimmt, ist
  **unsicher**.
- **BC660K-Stack 004/005/006** (unabhängig von der Anwendungs-Firmware, Q2):
  004 bringt `AT+DOWNTE`, `AT+BKDNS` statt `AT+DNSTIME`, `AT+DNSCFG` mit 40
  Zeichen, Leerzeichen-Filter in `AT+SERVADDR`. 005 bringt `AT+NTP`,
  `AT+QCOPS`, `AT+REDPT`, JSON-Downlinks und den Selbst-Reset nach 12 Zyklen
  ohne Signal. 006 bringt `AT+UPGRADE`, `AT+GETLOG=1`, TLS-Zertifikate per AT
  und ändert den `AT+MQOS`-Default von 2 auf 0.
- **D2x vs. WQS:** `AT+TR`, `AT+NOUD`, `AT+POWER_SAVE`, `AT+REDPT`,
  `AT+TEMPALARMx` und die Datalog-Befehle stehen nur in Q1, nicht in Q3.
  `AT+CAL*`, `AT+MODEL`, `AT+SLEEP` stehen nur in Q3.
- **Varianten innerhalb D2x:** D20-NB und D20S-NB haben einen Fühler, D22-NB
  zwei, D23-NB drei (Q1). Davon hängt ab, welche `AT+TEMPALARMx` sinnvoll sind.

### Widersprüche in der Doku

Alle Punkte am 2026-11-17 mit gezielten Zitaten am Original gegengeprüft. Es
handelt sich um Widersprüche in Dragino-Quellen, nicht um Extraktionsfehler.

- `AT+TR`-Default: 900 s (Q1) vs. 1800 s (Q2, FAQ).
- `AT+BKDNS`-Default: `1,0,NULL` (Q1, Q2) vs. `0,0,NULL` (Q3). Q1 und Q3 nennen
  unterschiedliche Mindestversionen.
- `AT+DNSTIMER`: steht in Q1, obwohl Q2 sagt, `AT+BKDNS` ersetze `AT+DNSTIME`.
- `AT+URI`: Anzahl (1–4 in Q1, 1–8 in Q3) und Syntax (`11,38,"…"` vs. `11,"i"`).
- `AT+PRO`: Q3 beschreibt 1 und 2 beide als UDP. Q2 nennt für TCP-JSON `4,1`
  statt `4,5`.
- `AT+CLOCKLOG`: „Aus“ wird als `1,65535,0,0` angegeben, obwohl a=1 „an“ heißt.
- `AT+POWER_SAVE`: a=0 schaltet den Zeitplan ein, a=1 aus, also gegen die
  übliche Logik. Für die Oberfläche als Schalter umkehren.
- Downlink-Code `0x0A` steht in Q1 sowohl für `AT+CLOCKLOG` als auch für
  `AT+TEMPALARMx`.
- Fühlerfarben: Q1 ordnet `TEMPALARM1` dem roten Fühler zu, die Variantenliste in
  Q1 nennt für D22/D23 aber Weiß = C1, Rot = C2.
- `AT+CALZD2`/`AT+CALZD4`: Werte 0/1 (Abschnitt 4.5.5) vs. 0/2 bzw. 0/4
  (Abschnitt 3.21) in Q3.
- `AT+PWORD`: Kleinbuchstaben-Regel nur in Q7 (S31-NB), nicht in Q1/Q3.

### Offene Punkte, die sich aus der Doku nicht klären lassen

Diese Punkte sollte jemand am Gerät prüfen und die Antworten mitschreiben, bevor
die Oberfläche gebaut wird.

1. **Format der `AT+CFG`-Ausgabe auf D2x/WQS.** Davon hängt ab, ob die Oberfläche
   alle Werte mit einem Befehl lesen kann oder je Feld `=?` senden muss. Nur Q6
   (anderes Gerät, 2021) zeigt ein Beispiel.
2. **Firmware-Version.** Kein dokumentierter Abfragebefehl für D2x. `AT+VER` aus
   dem Aircord-Preset `dragino-nb.toml` steht in keiner ausgewerteten Quelle. In
   Frage kommen `AT+MODEL=?` (nur in Q3), das Startbanner nach `ATZ` (nur in Q6)
   oder die JSON-Statusabfrage per Downlink (Q2). Am Gerät prüfen.
3. **Signalstärke.** Kein Abfragebefehl dokumentiert. Bekannt sind nur der Wert
   0–31 bzw. 99 und die Startzeile `Network Information:"RSSI:-63","SINR:8","BAND:8"`
   (Q2). Ob `AT+CSQ` durchgereicht wird, ist offen.
4. **`AT+QBAND` auf D2x/WQS.** Nur in Q2 dokumentiert, nicht in den
   Befehlslisten von Q1/Q3. Das Antwortformat von `AT+QBAND?` ist offen. Das
   Aircord-Preset nutzt `AT+QBAND=1,8` / `1,20`, der Playbook `AT+QBAND=2,8,20`.
5. **Welche Einstellungen ein `ATZ` brauchen.** Die Beispiele in Q2 hängen `ATZ`
   an Server- und Protokolländerungen an. Eine vollständige Liste fehlt.
6. **Minimum von `AT+TDC`** und Längengrenzen von `AT+SERVADDR`, `AT+APN`,
   MQTT-Feldern und `AT+PWORD`-Zeichensatz für D2x/WQS.
7. **Syntax** von `AT+GETSENSORVALUE`, `AT+DNSCFG`, `AT+SLEEP`, `AT+CDP`
   (Löschform), TLS-Zertifikatsbefehlen und `AT+QCGDEFCONT`.
8. **`AT+FDR` und Passwort:** Setzt `AT+FDR` das Passwort auf das Etikett
   zurück? `AT+FDR1` auf D22-NB, D23-NB vorhanden?
9. **Abfrage von `AT+PWORD`:** Gibt `AT+PWORD=?` das Passwort im Klartext aus
   (wie in Q6)? Dann darf die Oberfläche die Antwort nicht ins Protokoll
   schreiben.
10. **Einheit von `AT+TEMPALARMx`** (°C ganzzahlig vermutet) und Zuordnung
    Fühler ↔ Farbe.
11. **Modellerkennung in Aircord:** Das Preset matcht `(?i)^(d20|d23|wqs)`, also
    nicht D22-NB. Ob D20S-NB über `d20` erfasst wird, hängt vom BLE-Namen ab,
    der in der Doku nicht steht (Q5 sagt nur, der BLE-Name entspreche der DEV
    EUI).
