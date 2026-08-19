# Provisioning a sensor

This is the end-to-end field workflow: take a sensor out of the box, configure
it for the network, verify it, and leave it running. The example uses a Dragino
NB-IoT sensor on a 1NCE SIM, but the shape is the same for any device.

## Before you leave

Field work goes badly when the thinking happens on site. Do this at a desk:

1. **Get the playbook right.** Open **Playbooks**, load `dragino-nb-1nce`, and
   check the APN, server address and uplink interval against your deployment.
   Save it under a new name if your setup differs — the shipped one is a
   starting point, not a policy.
2. **Check the AT syntax** against your device's firmware manual. It varies
   between firmware revisions, and a rejected command produces a device that
   looks configured but is not.
3. **Do a dry run** on a bench device if you have one.

## On site

### 1. Find the device

Press **Scan**. If several sensors are within range, filter by a fragment of the
IMEI or the name — the address column is what you can match against a label on
the housing.

Signal strength sorting helps: the device in your hand is usually at the top.
Verify against the address anyway. Configuring the neighbour's sensor is a
recoverable mistake, but only if you notice.

### 2. Connect

Select the row, type the PIN if the device needs one, press **Connect**.

Watch for the line confirming the profile:

```
-- connected via HM-10 UART profile, ready --
```

If you entered a PIN, wait for the settle period to pass before doing anything
else — the status line tells you it is running.

### 3. Read the current state

Before changing anything, record what was there. In the control panel press
**Show configuration**, or type `AT+CFG` in the console. The reply is your
rollback reference, and it is in the transcript you can save at the end.

Also read the firmware version. If it differs from the device you tested the
playbook on, treat the AT syntax as unverified.

### 4. Apply the configuration

Press **Run** next to your playbook.

The dialog closes and the terminal shows each step:

```
-- playbook dragino-nb-1nce [1/7]: AT+QBAND=2,8,20 --
-- playbook dragino-nb-1nce [2/7]: AT+APN=iot.1nce.net --
…
-- playbook dragino-nb-1nce finished --
```

If it aborts, the message names the step and the reason. The device is now
half-configured — reconnect and run the playbook again from the start. The steps
are settings, so repeating them is harmless.

### 5. Verify

Do not trust "finished". Press **Show configuration** again and read the values
back. This is the step that catches a firmware whose `AT+TDC` takes minutes
rather than seconds.

The last playbook step is usually `ATZ`, which reboots the device. The link
drops — that is expected, and Aircord reports it as such:

```
-- link dropped (device reset or out of range) — reconnect --
```

Reconnect after the reboot and confirm the settings survived it.

### 6. Leave a record

Press **Save** in the terminal header. You get a timestamped file containing the
before-state, every command sent, and every reply — including the abort, if
there was one. The PIN is not in it.

Note the file path against the device in whatever inventory you keep. When the
sensor misbehaves in three months, this transcript is the only account of what
it was actually told to do.

## Sessions

If you are configuring several devices in one outing, **Sessions** saves the
device list, the selected target and the active template under a name.

Restoring a session brings back the list for reference, but the saved entries
cannot connect on their own — a stored snapshot has no live Bluetooth handle.
Press **Scan**; once the target device is rediscovered, Aircord promotes it to a
real selection and says so:

```
-- session target found: D20S-NB — connect ready --
```

Sessions store the PIN, so their files are written user-readable only (`0600`).
Treat them as credentials: do not commit them, and do not attach them to a bug
report.
