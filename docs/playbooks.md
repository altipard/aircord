# Playbooks

A playbook is a named sequence of AT commands that Aircord replays in order,
waiting for the device to acknowledge each step before sending the next. It
exists because provisioning is the same eight commands every time, and typing
them by hand at the water's edge is how a `900` becomes a `9000`.

Open the editor with **Playbooks** in the top-right corner.

## Syntax

One command per line:

```
# comments start with a hash and are ignored
AT+QBAND=2,8,20 ?OK
AT+APN=iot.1nce.net ?OK
AT+SERVADDR=udp.os.1nce.com,4445 ?OK
AT+PRO=2,5 ?OK
AT+TDC=900 ?OK
AT+CFG ?OK
ATZ
```

| Suffix | Meaning |
| ------ | ------- |
| `?PATTERN` | Wait until a reply line matches `PATTERN` before continuing |
| `?` | Shorthand for `?OK` |
| `@MS` | Pause `MS` milliseconds after the step |
| `#` at line start | Comment — the line is skipped |

`PATTERN` is a regular expression. A pattern that fails to compile falls back to
a literal substring match, so a typo can never make a playbook unrunnable.

Both suffixes can appear on one line: `AT+TDC=3600 ?OK @1500` waits for `OK`,
then pauses another 1.5 seconds.

## Why wait for a reply

Without `?`, the runner fires commands as fast as the link accepts them. A busy
console drops what it cannot process, and the failure is silent — the playbook
reports success while half the settings never landed. Waiting for each
acknowledgement turns that into a visible abort.

Steps without `?` are appropriate for commands that answer nothing. `ATZ` is the
usual example: the device reboots instead of replying, so waiting would only
time out.

## Running one

Press **Run** next to a playbook.

- Aircord asks first if the playbook contains a reboot or reset command, naming
  the exact commands so you know what is about to happen. This matters for a
  device that is already mounted in the field.
- The dialog closes when the run starts, because progress is reported in the
  terminal underneath it.
- Each step is logged as `-- playbook <name> [3/7]: AT+TDC=900 --`, and the
  status line mirrors it.

A run aborts, loudly, when:

- a step waits more than **15 seconds** for its pattern, or
- the link drops mid-run.

Both leave the device half-configured. That is deliberate: continuing to send
settings down a link that is no longer answering produces a device you *believe*
is configured, which is worse than one you know is not. Reconnect, run the
playbook again — the commands are idempotent settings, so a second full pass is
harmless.

## The shipped playbook

Aircord ships one playbook, `dragino-nb-1nce`: full 1NCE-OS provisioning for
Dragino D2x-NB sensors. It is named after device *and* provider because a
different SIM means a different APN and server, and therefore a different
playbook.

It is a starting point, not a default to trust blindly — verify the AT syntax
against your device's firmware manual. The uplink interval (`AT+TDC=900`, 15
minutes) is the value most worth adjusting before first use.

## Storage

Playbooks are JSON files under `<config>/aircord/playbooks/`, one per playbook.
They are portable: copying a file into that directory on another machine makes
the playbook appear there.
