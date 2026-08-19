# Security policy

## Reporting a vulnerability

Report security issues privately, not as a public issue. Use
[private vulnerability reporting](https://github.com/altipard/aircord/security/advisories/new)
or email the maintainer listed in `git log`.

Please include what you did, what happened, and the affected version. Expect an
acknowledgement within a week.

## Supported versions

Only the latest release receives fixes. This is a small project; there are no
maintenance branches.

## What Aircord handles

Aircord is a local desktop tool. It opens no network sockets and talks only to
Bluetooth peripherals you select. Nonetheless, some of what it touches is
sensitive:

**Device PINs.** The PIN field is masked, and the PIN is sent to the device with
its echo masked, so it does not appear in the terminal or in a saved log. It
*is* stored in plain text inside a saved session, because re-typing it for every
device in the field is worse in practice than the risk of a local file. Session
files are therefore written user-readable only (mode `0600`) under
`<config>/aircord/sessions/`.

Treat session files as credentials: do not commit them, do not sync them to
shared storage, and do not attach them to a bug report.

**Terminal logs.** Saved logs contain every command sent and every reply
received. PINs are masked, but device configuration — APNs, server addresses,
IMEIs — is not. Read a log before attaching it to an issue.

**Device templates and playbooks** are executable in the sense that they send AT
commands to hardware. A template or playbook obtained from someone else can
reboot, reconfigure or factory-reset your device. Aircord asks before running a
playbook containing a known destructive command, and before a template action
marked `confirm = true` — but this is a convenience, not a sandbox. Read a
playbook before running it, the same way you would read a shell script.

## What is out of scope

- Physical or radio-layer attacks against the BLE link itself. Aircord uses
  whatever pairing and encryption the peripheral offers; it adds none of its own.
- Malicious device firmware. A device that floods the notify channel can make
  the UI busy; the log is capped at 2000 lines to bound the damage.
- The unsigned macOS build. It is unsigned by choice, and the README says so.
