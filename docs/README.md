# Aircord documentation

| Guide | Read it when |
| ----- | ------------ |
| [Getting started](getting-started.md) | You have just installed Aircord and want a first connection |
| [Provisioning a sensor](provisioning.md) | You are about to configure a device for deployment |
| [Playbooks](playbooks.md) | You want to replay the same command sequence reliably |
| [Device templates](device-templates.md) | You want buttons instead of AT syntax for your hardware |
| [Firmware update](firmware-update.md) | You want to flash a new firmware onto a Dragino NB node over BLE |
| [Troubleshooting](troubleshooting.md) | Something does not show up, connect, or answer |

Project overview and build instructions live in the
[README](../README.md). Contribution rules are in
[CONTRIBUTING.md](../CONTRIBUTING.md).

## The short version

Aircord talks to BLE peripherals that expose a serial port over a GATT profile.
Everything it does reduces to sending AT command lines and reading the replies —
the console does that by hand, the control panel does it behind labelled
buttons, and playbooks do it as a scripted sequence.

```
Scan  →  select device  →  Connect  →  ┬─ type AT commands in the console
                                       ├─ press buttons in the control panel
                                       └─ run a playbook

Firmware  →  reset node  →  catch its bootloader  →  erase, flash, verify, reboot
```

Nothing is sent to the device that you did not trigger, with one exception: the
PIN, if you entered one, is sent automatically about four seconds after the link
comes up.
