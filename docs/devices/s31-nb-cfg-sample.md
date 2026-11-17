# S31-NB: Startbanner und `AT+CFG` (Gerätetest)

Mitschrift eines echten Geräts, Grundlage für die Einstellungen in Aircord
(`crates/core`: `cfg`, `banner`, `transfer`, Preset
`presets/dragino-s31-nb.toml`).

- Gerät: Dragino S31-NB, Firmware v1.4.0, Modul BC660K-GL
- SIM: 1NCE
- Datum: 2026-11-17
- Verbindung über BLE, danach Eingabe des Passworts vom Etikett

IMEI, IMSI und Passwort sind durch Beispielwerte bzw. `******` ersetzt. Die
Testdaten unter `crates/core/testdata/` enthalten dieselbe Ausgabe mit
einem erfundenen Klartext-Passwort, damit der Test der Maskierung etwas prüft.

## Startbanner

Kommt nach Verbindung und Passwort von selbst, ohne Befehl. Die Zahl in eckigen
Klammern ist ein Zeitstempel des Geräts; die Einheit ist nicht dokumentiert
(vermutlich Millisekunden).

```text
[6902]NB module is initializing...
[6940]Password Correct
[8266]NBIOT has responded.
[12597]Echo mode turned off successfully.
[13943]Disable the reporting of deep sleep event URC.
[15302]Model information:BC660K-GL.
[16642]The IMEI number is:866207050000001.
[17989]The IMSI number is:901405100000001.
[19536]Set the data format for sending and receiving.
Currently set frequency band: 8,20
[22233]Set APN successfully
[24264]Exit sleep mode
[28891]Signal Strength:99
```

Die Zeile `Signal Strength:<n>` wiederholt das Gerät etwa alle 10 Sekunden.
`99` heißt kein Netz; Werte 0–31 sind laut Inventar die übliche CSQ-Skala (im
zweiten Test mit `17` beobachtet, siehe [Upload](#erfolgreicher-upload)). Die
Zeile mit dem Band hat keinen Zeitstempel.

## `AT+CFG`

```text
AT+MODEL=S31-NB,v1.4.0
AT+CFGMOD=1
AT+DEUI=866207050000001
AT+PWORD=******
AT+SERVADDR=udp.os.1nce.com,4445

AT+CLIENT=NULL
AT+UNAME=NULL
AT+PWD=NULL
AT+PUBTOPIC=NULL
AT+SUBTOPIC=NULL
AT+TDC=900
AT+INTMOD=0
AT+APN=iot.1nce.net
AT+5VT=0
AT+PRO=2,5
AT+RXDL=0
AT+EXT=0
AT+LDATA=NULL
AT+SHTEMP=0,0
AT+SHHUM=0,0
AT+DNSCFG="8.8.8.8","8.8.4.4"
AT+CSQTIME=5
AT+BKDNS=1,0,NULL
AT+TLSMOD=0,0
AT+SLEEP=0
AT+MQOS=2
AT+CLOCKLOG=1,65535,15,8
AT+URI1=NULL
AT+URI2=NULL
AT+URI3=NULL
AT+URI4=NULL
AT+DOWNTE=0,0
AT+REDPT=0
AT+NTP=NULL
AT+QCOPS=NULL

OK
```

Die leeren Zeilen nach `AT+SERVADDR` und vor `OK` stehen so in der Ausgabe.

## Erfolgreicher Upload

Dasselbe Gerät am selben Tag (19:52), diesmal mit angeschraubter Antenne. Ohne
Antenne blieb das Signal bei `99`, das war die Ursache im ersten Test. Die SIM
war laut 1NCE-Portal aktiv, ohne IMEI-Sperre, eingebucht über Telekom.de.
Mitschnitt ab der IMSI-Zeile, ohne Befehl:

```text
[22688]The IMSI number is:901405100000001.
[22734]Password Correct
[24262]Set the data format for sending and receiving.
Currently set frequency band: 8,20
[26959]Set APN successfully
[28990]Exit sleep mode
[33617]Signal Strength:17
[38851]Network Information:"RSSI:-80","SINR:5","BAND:8"
[40212]PSM mode configured
[41544]DNS configuration is successful
[45889]Resolving domain name...
[48925]Domain IP:10.60.2.239,4445
[51888]*****Upload start:0*****
[51923]remaining battery =3541 mv
[51963]DS18B20(1) temp is -409.5
[52067]adc_mV(1):0.00
[52193]Humidity =46.08 %rh
[52223]tem =23.56 C
[53761]Open a Socket Service successfully
[57841]Upload data successfully
[60880]Datagram is sent by RF
[62414]Close the port successfully
[63453]Send complete
[64477]*****End of upload*****
```

Dieselbe Ausgabe steht in `crates/core/testdata/s31-nb-v1.4.0-upload.txt`.
Die Zeile `PSM mode configured` endet dort wie im Original mit einem
Leerzeichen.

- `DS18B20(1) temp is -409.5`: Es war kein externer Fühler angeschlossen.
  `-409.5` ist der Platzhalterwert, keine Temperatur.
- `Humidity` und `tem` stammen vom eingebauten Sensor, `remaining battery` ist
  die Batteriespannung in mV.
- Eine Fehlermeldung im Upload-Block wurde nicht beobachtet. Aircord wertet
  einen Block als fehlgeschlagen, wenn er ohne `Upload data successfully`,
  `Datagram is sent by RF` oder `Send complete` endet.

## Folgerungen

Das beantwortet die offenen Punkte 1, 2 und 9 des Inventars
(`dragino-nb-at-commands.md`, Abschnitt 5) für das S31-NB:

- **Format:** `AT+CFG` liefert den vollständigen Ist-Stand, eine Zeile je
  Einstellung im Setzformat `AT+<KEY>=<wert>`, Abschluss `OK`. Aircord liest
  einmal, sendet nur die Änderungen und liest danach erneut zur Verifikation.
- **Modell und Firmware** stehen in der ersten Zeile `AT+MODEL=<modell>,v<version>`.
  Die Vorlage wird darüber gewählt, nicht über den BLE-Namen, der die IMEI ist.
  `AT+VER` ist nicht nötig.
- **`NULL`** heißt leer bzw. unbenutzt. `AT+BKDNS=1,0,NULL` enthält `NULL` als
  Teilwert und bleibt so stehen.
- **Passwörter im Klartext:** Das Gerät gibt `AT+PWORD` im Klartext aus. Aircord
  maskiert `AT+PWORD`, `AT+PIN`, `AT+PWD` und `AT+OTAPWD` im Log der Verbindung
  (gesendet und empfangen) und in allen Ergebnissen der Übertragung.
- **Band** steht nicht in `AT+CFG`, nur im Startbanner. Das Setzen per
  `AT+QBAND` ist nicht getestet und fehlt deshalb im Preset.
- **Statuskarte:** Modul, IMEI, IMSI, Band, APN gesetzt und Signal kommen aus dem
  Startbanner, ohne Befehl. Netzdetails (RSSI, SINR, Band), der Weg zum Server
  und jeder Upload mit Batterie und Messwerten kommen aus den Zeilen, die das
  Gerät selbst ausgibt (`banner::NodeStatus`). Die Einstufung der Batterie
  (ab 3,3 V gut, ab 3,1 V schwach) ist eine Annahme und noch nicht geprüft.
- Nicht in der Ausgabe: `AT+TR` (Messintervall). Das Preset bietet es für das
  S31-NB deshalb nicht an.
