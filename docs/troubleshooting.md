# Troubleshooting

## No devices appear when scanning

**Bluetooth permission.** macOS asks once, on the first scan. If it was denied,
the scan runs and finds nothing, with no error. Check
*System Settings → Privacy & Security → Bluetooth* and enable Aircord.

**Bluetooth off, or adapter busy.** A scan error is reported on the alert line
below the filter and mirrored into the terminal with a `!!` prefix.

**The device is not advertising.** Many sensors advertise only for a window
after power-up or after a magnet/button wake. Re-trigger the device and scan
again.

**The filter is hiding it.** An old filter string persists in the box. Clear it —
the status line shows `shown/total`, so `0/23` means the filter is the problem,
not the radio.

## The device appears but will not connect

**Something else is connected to it.** BLE peripherals usually accept one
central at a time. Close other tools, and check whether a phone app still holds
the link.

**Out of range or weak signal.** RSSI is in the list. Below roughly −90 dBm,
connections become unreliable. Move closer.

**Connect fails immediately.** The reason is logged with `!!`. `enable failed`
points at the adapter, `connect failed` at the device.

## Connected, but no UART profile

```
!! no known UART profile (Nordic/HM-10) in GATT above
```

The device does not expose a serial service Aircord recognises. The GATT dump
above that line lists every service and characteristic found. If your device
speaks a transparent-UART profile under different UUIDs, add them to
`uartProfiles` in `internal/ble/ble.go`.

Some devices expose their serial service only after authentication, so try again
after the PIN has been accepted.

## The device does not answer commands

**The PIN never arrived.** It is sent about four seconds after the link comes up.
If the link dropped during that window you get:

```
-- PIN skipped: link dropped during init --
```

Reconnect. If the device needs longer than four seconds, send the PIN by hand
from the command line.

**Wrong line ending or wrong command.** Aircord sends `\r\n`. If a device echoes
your command but never answers, the command itself is likely rejected — check it
against the firmware manual.

**The console is busy.** During a TX window or a boot phase, some devices ignore
input entirely. Wait for the current activity to finish and retry.

## A playbook aborts

```
-- playbook … aborted at step 3/7 (AT+APN=…): no reply matching OK --
```

The step waited 15 seconds without a matching reply.

- **The command was rejected.** Send it by hand and read the actual reply. Some
  firmwares answer `ERROR` instead of `OK`, which no `?OK` wait will ever match.
- **The pattern is too strict.** If the device answers `+APN: OK`, `?OK` matches
  fine, but `?^OK$` does not.
- **The device is slow.** Add a pause to the previous step: `AT+CFG ?OK @2000`.

`link lost` as the reason means the device dropped the connection — often the
correct behaviour if the previous step rebooted it. Move the reboot to the end
of the playbook.

## Read-out shows "(no reply)"

The `read` action waited 15 seconds without a reply line matching its `parse`
regexp. Send the command by hand and look at the raw reply: the regexp usually
assumes a format the firmware does not produce. See
[Device templates](device-templates.md).

## The control panel stays empty

The panel fills only when a device is connected **and** a template is selected.
If the header says *no template*, no shipped template matched the device name —
pick one from the dropdown, or
[write one](device-templates.md).

If a template you wrote does not appear, it failed validation. The reason is
logged at startup:

```
!! template load skipped: template "x": read needs a parse regexp
```

Templates load at startup only, so restart Aircord after editing one.

## Old data disappeared after the rename

Aircord was called `blescan` before release. On first launch the old config
directory is moved across automatically — but only if the new one does not exist
yet. If you launched the new version before the old data was in place, both
directories now exist side by side:

```
<config>/blescan/    ← your old data, untouched
<config>/aircord/    ← the new, empty one
```

Nothing was deleted. Quit Aircord, move the files across by hand, and restart.

## Getting help

Open an issue with the saved terminal log attached — **Save** in the terminal
header writes it to `<config>/aircord/logs/`. The PIN is masked in the
transcript, but check the log before attaching it anyway, and never attach a
session file.
