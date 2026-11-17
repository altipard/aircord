# Gerät einstellen ohne Befehle

Aircord öffnet mit dem Reiter **Einstellungen**. Dort wird ein Gerät über ein Formular eingestellt; AT-Befehle muss
niemand kennen. Die bisherige Ansicht mit Terminal, Bedienfeld, Playbooks, Sessions und Firmware liegt im Reiter
**Experte**. Sie arbeitet wie bisher, mit einem Unterschied: Beim Verbinden liest Aircord die Einstellungen des Geräts
jetzt von selbst. Wer in dieser Zeit einen Befehl sendet oder ein Playbook startet, bricht das Lesen ab; es lässt sich
im Reiter **Einstellungen** nachholen.

## In vier Schritten

1. **Suchen** drücken. Links erscheinen die Geräte in der Nähe als Karten, mit Signalbalken. Das Gerät muss
   eingeschaltet sein. Die Adresse eines Geräts steht im Tooltip der Karte.
2. Gerät anklicken, **Verbinden** drücken. Aircord liest die Einstellungen selbst aus und zeigt oben Modell und
   Firmware.
3. Werte im Formular ändern. Ein Punkt vor dem Namen markiert, was vom Gerät abweicht; der Pfeil daneben setzt das
   Feld zurück. Unten steht, wie viele Änderungen anstehen.
4. **Übertragen** drücken. Ein Dialog zeigt jede Änderung als Vorher und Nachher. Erst nach der Bestätigung wird
   gesendet.

Ist **Übertragen** gesperrt und steht unten „bitte zuerst die Einstellungen neu lesen“, hat Aircord keinen
aktuellen Stand des Geräts: mit dem Kreispfeil oben rechts neu lesen.

## Das Ergebnis

Nach der Übertragung liest Aircord das Gerät noch einmal und meldet je Einstellung:

- **übernommen** – der Wert steht so im Gerät.
- **abgelehnt** – das Gerät hat den Wert nicht angenommen. Die Eingabe bleibt im Formular und kann berichtigt
  werden. Die genaue Meldung des Geräts steht im Tooltip der Zeile und im Terminal des Reiters **Experte**.
- **nicht bestätigt** – gesendet, aber das Gerät ließ sich danach nicht lesen. Mit dem Kreispfeil oben rechts neu
  lesen.

Manche Einstellungen wirken erst nach einem Neustart. Dann fragt Aircord „Neu starten, damit alle Änderungen
wirken?“. Bei **Ja** startet das Gerät neu und die Verbindung endet; nach etwa einer Minute lässt es sich wieder
verbinden. Bei **Später** bleibt alles, wie es ist. Aircord startet ein Gerät nie von selbst neu.

## Status

Die Statuskarte zeigt, was das Gerät von sich aus meldet. Beim Start: Passwort angenommen, SIM-Karte erkannt, Netz
mit Güte, Pegel und Funkband, Zugangspunkt gesetzt. Sobald das Gerät Messwerte sendet, kommen dazu: Batterie in Volt
mit Einstufung (gut, schwach, fast leer), Temperatur, Luftfeuchte, der externe Fühler oder „nicht angeschlossen“ und
die letzte Sendung mit Uhrzeit und Ausgang (erfolgreich, fehlgeschlagen, abgebrochen). Die Werte stehen erst da, wenn
das Gerät während der Verbindung gesendet hat. Die Einstufung der Batterie ist eine Faustregel nach der Spannung. Meldet das Gerät mehrere Minuten lang kein Netz, erscheint der Hinweis „Kein Netz –
Antenne angeschraubt? Gerät näher ans Fenster?“.

## Passwort

Ein Passwortfeld erscheint nur, wenn das Gerät auf die Frage nach seinen Einstellungen nicht antwortet oder keine
nennt. War schon beim Verbinden ein Passwort eingetragen, steht dort, dass es nicht angenommen wurde. Das Passwort
steht auf dem Etikett des Geräts. Es wird verdeckt eingegeben und erscheint weder im Terminal noch in einem
gespeicherten Protokoll oder im Befehlsverlauf.

Eine gespeicherte Session enthält das Passwort dagegen wie bisher im Klartext, damit sie das Gerät wieder öffnen
kann. Die Datei ist nur für das eigene Benutzerkonto lesbar (Rechte 0600) und gehört nicht in fremde Hände.

Passwortfelder im Formular sind leer. Leer heißt: unverändert. Wo ein Passwort gelöscht werden darf, gibt es dafür
den Knopf **Löschen**.

## Weitere Einstellungen

Unter den Karten klappt **Weitere Einstellungen** die selten gebrauchten Werte auf. Im Normalfall bleiben sie, wie
sie sind.

## Warnungen

Vor Änderungen, die das Gerät vom Netz oder von seinem Server trennen können, warnt der Dialog: „Das Gerät kann
danach das Netz verlieren“. Ein falscher Wert lässt sich dann nur noch vor Ort beheben. Ein geändertes
Gerätepasswort lässt sich nicht zurückholen.

## Wenn es kein Formular gibt

Kennt Aircord das Modell nicht, steht dort ein Hinweis mit dem Knopf **Experten-Ansicht öffnen**. Dort lässt sich
das Gerät wie bisher über Befehle einstellen.
