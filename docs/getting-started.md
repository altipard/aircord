# Getting started

## Install

Download the package for your platform from the
[releases page](https://gitlab.com/zilicon-it-services/petriheil/aircord/-/releases),
or build from source:

```sh
git clone https://gitlab.com/zilicon-it-services/petriheil/aircord.git
cd aircord
go run .
```

Building needs **Go 1.25+**. On Linux you also need the Fyne GUI development
packages (`libgl1-mesa-dev`, `xorg-dev`).

The macOS app is not code-signed. On first launch, right-click the app and
choose *Open* rather than double-clicking, otherwise Gatekeeper refuses it.

## First scan

1. Press **Scan**. On macOS the system asks for Bluetooth permission the first
   time — grant it, or the device list stays empty forever.
2. Advertising devices appear within a second or two, sorted by signal strength
   so the device in your hand tends to sit near the top.
3. The list refreshes about twice a second. Nearby-but-irrelevant devices are
   normal; use the filter box to cut them down.

The filter accepts a plain substring or a regular expression, matched against
both name and address:

| Filter | Matches |
| ------ | ------- |
| `8606` | anything containing 8606 — useful for an IMEI fragment |
| `^D20` | names starting with D20 |
| `(?i)nb` | "nb" in any capitalisation |

Click a column header to sort by it; click the same header again to reverse the
direction. An arrow marks the active column. The list starts on RSSI, strongest
first, because during a scan the device in your hand is usually the closest one.

Two details make the sorted list usable while it refreshes twice a second:
devices without a name always sort last under a name sort, in either direction,
so the dozens of anonymous peripherals in range cannot bury the ones you are
looking for; and rows that compare equal always fall back to address order, so
nothing shuffles under the pointer between repaints.

Press **Scan** again to stop. **Clear list** empties the table without stopping
discovery.

## First connection

1. Click a row to select the device. The status line confirms the selection and
   **Connect** becomes available.
2. If the device requires a PIN, type it into the PIN field. The field masks
   what you type, and the PIN never appears in the terminal or in a saved log.
3. Click **Connect**.

Discovery stops — a Bluetooth adapter cannot scan and hold a connection at the
same time — and the terminal shows what happened:

```
-- connecting to D20S-NB (…) --
-- GATT --
  svc 0000ffe0-…
    chr 0000ffe1-…
-- connected via HM-10 UART profile, ready --
```

If you entered a PIN, Aircord waits four seconds for the device to finish its
own initialisation and then sends it. The wait is announced in the status line;
sending earlier usually means the device swallows the PIN.

## Talking to the device

Type an AT command into the field at the bottom and press **Enter**, or click
**Send**.

- Outgoing lines are prefixed `>>`, replies `<<`.
- Press `Up` for a searchable history of the commands you have sent **to this
  device**. History is kept per device address, so a sensor you configured last
  month still remembers.
- The terminal is read-only: you can select and copy from it (`Cmd+C`), but not
  type into it. A transcript you can edit is a transcript you cannot trust.

**Copy** puts the entire transcript on the clipboard, **Save** writes it to a
timestamped file and prints the path.

## Where to go next

Once a connection works, the parts that save real time are the
[control panel](device-templates.md) and [playbooks](playbooks.md). If a device
never appears or never answers, see [Troubleshooting](troubleshooting.md).
