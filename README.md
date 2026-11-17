<div align="center">

<img src="assets/icon.png" alt="Aircord" width="120">

# Aircord

**The serial console for devices that no longer have a serial port.**

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

</div>

Aircord is a desktop GUI for BLE devices that expose a **serial-over-BLE**
interface — NB-IoT and LoRaWAN modems, field sensors, and any peripheral that
speaks AT commands over a Nordic UART or HM-10 (`0xFFE1`) GATT profile.

Scan for advertising devices, pick one, connect, and you have an interactive AT
console: type commands, watch replies stream back, and reuse a searchable
per-device history. Beyond the raw console, Aircord adds the three things that
make field work bearable — a **control panel** of named buttons instead of AT
syntax, **playbooks** that replay a provisioning sequence step by step, and
**sessions** that bring your working set back after a break.

Built in Rust with [egui/eframe](https://github.com/emilk/egui) and
[btleplug](https://github.com/deviceplug/btleplug).

## Features

- **Scan** — live device list (name / RSSI / address) with a regex or substring
  filter. Click a column header to sort by it, again to reverse; the default is
  strongest signal first.
- **Connect** — auto-detects the UART profile (Nordic UART or HM-10 `0xFFE1`),
  dumps the GATT table, and optionally sends a PIN once the link settles. The
  PIN is never written to the log.
- **AT console** — send commands, `\n`-reassembled replies stream into the
  terminal. Press `Up` for an fzf-style searchable history, kept per device.
- **Control panel** — a device template turns AT syntax into labelled buttons,
  inputs, dropdowns and read-outs. Actions can be marked as destructive, and
  are then confirmed before they fire. See [Device templates](docs/device-templates.md).
- **Playbooks** — replay a provisioning sequence, waiting for each device
  acknowledgement before moving on. See [Playbooks](docs/playbooks.md).
- **Firmware update** — flash a Dragino NB-IoT node over BLE: reset it into
  its bootloader, catch the IMEI advertisement, erase, flash, verify, reboot.
  See [Firmware update](docs/firmware-update.md).
- **Sessions** — save and restore the device list, selected target and template.
- **Log tools** — copy the transcript or save it to a timestamped file.
- **Resilient link handling** — device resets and out-of-range drops are
  reflected in the UI instead of freezing it.

## Documentation

| Guide | What it covers |
| ----- | -------------- |
| [Getting started](docs/getting-started.md) | Install, first scan, first connection |
| [Provisioning a sensor](docs/provisioning.md) | End-to-end field workflow |
| [Playbooks](docs/playbooks.md) | Command sequences and their syntax |
| [Device templates](docs/device-templates.md) | Building a control panel in TOML |
| [Firmware update](docs/firmware-update.md) | Flashing a Dragino NB node over BLE |
| [Troubleshooting](docs/troubleshooting.md) | When it doesn't connect or reply |

## Requirements

- **Rust 1.95+** via [rustup](https://rustup.rs) (to build from source). Linux
  additionally needs `pkg-config libdbus-1-dev libxkbcommon-dev libgl1-mesa-dev
  libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev`.
- **macOS** — developed and runtime-tested here (uses CoreBluetooth). CI builds
  Linux (BlueZ) and Windows (WinRT) packages too, but their runtime is
  **unverified**.
- Bluetooth enabled. macOS prompts for Bluetooth permission on the first scan.

## Download

Packages for macOS, Linux and Windows are attached to each
[tagged release](https://github.com/altipard/aircord/releases).
The macOS app is unsigned — right-click → *Open* on first launch.

## Build and run

Using [Task](https://taskfile.dev) (`brew install go-task/tap/go-task`):

```sh
task run          # build and run
task build        # -> target/release/aircord
task package      # native package in dist/ (macOS Aircord.app, Linux .tar.xz, Windows .exe)
task check        # fmt check + clippy + tests — the CI gate
task --list       # all tasks
```

Or plain Cargo:

```sh
cargo run -p aircord
cargo build --release -p aircord
cargo test --workspace
```

> `task run` and `cargo run -p aircord` start a bare binary; `task package`
> builds the packaged app (`Aircord.app` on macOS).

## Supported UART profiles

| Profile | Write UUID | Notify UUID |
| ------- | ---------- | ----------- |
| Nordic  | UART RX    | UART TX     |
| HM-10   | `0xFFE1`   | `0xFFE1`    |

The HM-10 (`0xFFE1`) profile covers TI CC254x transparent-UART modules — for
example the Dragino D20S-NB. To support another device, read the GATT dump in
the terminal and add its write/notify UUIDs to `PROFILES` in
`crates/ble/src/lib.rs`.

## Where your data lives

Everything Aircord persists sits under the OS user-config directory:

| Path | Contents |
| ---- | -------- |
| `<config>/aircord/history.json` | per-device command history |
| `<config>/aircord/sessions/` | saved sessions (mode `0600` — they contain the PIN) |
| `<config>/aircord/playbooks/` | saved command sequences |
| `<config>/aircord/templates/` | your own device templates |
| `<config>/aircord/logs/` | saved terminal logs |

`<config>` is `~/Library/Application Support` on macOS and
`$XDG_CONFIG_HOME` (default `~/.config`) on Linux.

> Upgrading from the pre-release name `blescan`? The old directory is moved
> across automatically on first launch, so history, sessions and playbooks
> survive.

## Project layout

A Cargo workspace. Everything that needs neither a window nor a radio lives in
`aircord-core` and is unit-tested against a fake device; the radio and the
window are thin crates on top.

| Path | Responsibility |
| ---- | -------------- |
| `apps/aircord` | the binary: migrates a pre-rename config dir, opens the stores, starts the radio, opens the window |
| `crates/ble` | `aircord-ble`: the `Link` over btleplug (CoreBluetooth, BlueZ, WinRT) on its own tokio runtime; UART profiles |
| `crates/core` | `aircord-core`: GUI-free logic, see below |
| `crates/core/presets/` | device templates shipped inside the binary |
| `crates/core/testdata/` | captured S31-NB output used by the parser tests |
| `crates/ui` | `aircord-ui`: the egui window, dialogs and the guided view |
| `crates/ui/assets/fonts/` | embedded Inter and JetBrains Mono |
| `assets/icon.svg` | editable icon source; `assets/icon.png` is rendered from it |

`crates/core/src`:

| Module | Responsibility |
| ------ | -------------- |
| `link`, `link/fake` | the `Link` trait the rest of the app talks to; a scripted fake device for tests |
| `lines` | notify chunks reassembled into lines; taps that let a run wait for a reply |
| `config` | config dir, atomic writes, legacy-dir migration |
| `device` | device template parsing and preset loading |
| `settings`, `cfg`, `transfer` | typed settings, the `AT+CFG` parser, sending only the changes and verifying them |
| `banner` | status lines a Dragino NB node prints by itself (IMEI, signal, uploads) |
| `playbook` | playbook persistence, text format, risky-command detection, runner |
| `session` | session persistence |
| `history` | per-device command history persistence |
| `filter`, `scan` | shared regex/substring filter; device table sort order |
| `ota`, `otanb` | firmware update flow; Dragino NB bootloader protocol (frames, CRC, upgrade sequence) |
| `redact` | password masking in logs, transcript and screen |
| `slug` | safe file names from user-given names |

`crates/ui/src`:

| Module | Responsibility |
| ------ | -------------- |
| `lib` | the `App`: layout, scan/connect flow, terminal, status bar, history picker |
| `inbox` | messages from the radio and the jobs to the window's thread |
| `guided` | guided view: device cards, status card, settings form (German) |
| `form`, `xfer` | settings form state; transferring the changes, with review and progress |
| `panel` | control panel rendered from a device template |
| `playbooks`, `sessions`, `firmware` | the Playbooks, Sessions and firmware update dialogs |
| `confirm`, `dialog` | confirmations before destructive actions; native file dialogs |
| `style`, `widgets`, `icons` | theme (light/dark), shared widgets, icon drawings |
| `tests` | the window driven as a user would (egui_kittest) over the fake link |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Security reports go to
[SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE) © 2026 Daniel Altiparmak
