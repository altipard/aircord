# Firmware update

Aircord can flash a new application firmware onto a Dragino NB-IoT node over
Bluetooth, the way Dragino's own Sensor Management Tool does. It speaks the
node's bootloader protocol over the same serial-over-BLE channel the console
uses.

**Supported:** Dragino -NB / -NS nodes with the STM32 bootloader (D20-NB,
D23-NB, S31-NB, SDI-12-NB, …). The protocol follows Dragino's published
updater; the first flash on a new node type belongs on a bench, not in the
field.

## What you need

- The firmware as a **`.bin` of the application only**, without the
  bootloader. Dragino publishes both variants; the one with the bootloader is
  for the UART/ST-Link path. A `.bin` larger than the application region is
  rejected with a hint that it probably still contains the bootloader.
- The node's **IMEI**. It is on the label, and `AT+CFG` prints it. The
  bootloader advertises under the bare IMEI, and that is how Aircord finds it.
- The node's **PIN** in the PIN field of the main window, if the node has one.
  Bootloader v1.3 and later check it; older bootloaders ignore it.

## How it works

The bootloader only listens for a short window right after a reset. For
8 to 15 seconds it advertises under the IMEI instead of the node's usual name,
then it starts the application as normal. Aircord has to catch that window:

1. **Reset.** Either Aircord sends `ATZ` over the open connection, or you press
   the RESET key on the node (holding ACT for three seconds does the same).
2. **Find.** Aircord scans and waits up to 20 seconds for a device that
   advertises as the IMEI and was seen *after* the reset. A stale entry from
   an earlier attempt never counts.
3. **Flash.** It connects, erases the application region, writes the image in
   224-byte frames, verifies the CRC32 and tells the node to reboot. The
   bootloader region is never written, so a failed attempt can be repeated.

## Step by step

1. Press **Firmware** in the top row.
2. **Choose .bin…** and pick the image. It is validated on the spot.
3. Enter the **IMEI**, or press **Read** while connected to the node to pull
   it from `AT+CFG`. A node already sitting in its bootloader shows up in the
   device list under its IMEI; select it and the field is pre-filled.
4. Pick how the node is **reset**. Over an open connection `ATZ` is the
   default; otherwise you press the key yourself when the progress dialog says
   so.
5. **Start update**, confirm. The dialog closes and a progress window takes
   its place; the terminal logs every step.

The progress window's **Cancel** drops the link. Cancelling during the flash
leaves the node in its bootloader with a half-written application; reset it
and run the update again.

## When it does not work

**"bootloader … not seen within 20s".** The node did not advertise under the
IMEI in time. Check the IMEI, then reset the node again and retry. Dragino
notes that a node reset less than two minutes ago may not enter the bootloader
a second time: wait, then retry.

**Rejected with a status.** The bootloader refused a frame. With bootloader
v1.3+ the usual cause is a wrong PIN — the same one that opens the console.

**The node keeps restarting after the update.** The `.bin` contained the
bootloader. Flash the application-only image.

**Nothing at all happens after Start.** With the `ATZ` option the link must be
open; if the node dropped it in the meantime, connect again or switch to the
RESET-key option.
