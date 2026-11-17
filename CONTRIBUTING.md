# Contributing

Thanks for your interest in Aircord. This is a small project; the bar is simply
"green gate, clear commits."

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).
Security issues go to [SECURITY.md](SECURITY.md), not the public tracker.

## Development

Requires **Rust 1.95+** (install via [rustup](https://rustup.rs)). On macOS
everything builds out of the box; Linux needs these system packages:

```sh
sudo apt install pkg-config libdbus-1-dev libxkbcommon-dev libgl1-mesa-dev \
  libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev
```

```sh
task run         # build and run
task check       # fmt check + clippy + tests — the CI gate
task test        # tests only
task lint        # clippy with -D warnings
task fmt         # format everything
task package     # native package in dist/
```

No `task`? The equivalents are `cargo run -p aircord`,
`cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo fmt --all --check`.

Run `task check` before opening a pull request — CI runs the same gate.

## Code layout

A Cargo workspace (edition 2024): `crates/core` holds everything that needs
neither a window nor a radio, `crates/ble` the btleplug radio, `crates/ui` the
egui window, `apps/aircord` the binary. See the
[project layout](README.md#project-layout) for the module map.

New pure logic (parsing, persistence, filtering, a long run against the device)
belongs in `aircord-core` with tests, written against the `Link` trait so it can
be tested with `link::fake::FakeLink` instead of a radio. The UI crate draws
what the core does; it should not grow logic of its own.

The workspace lints are strict and part of the gate:

- `unsafe_code` is **forbidden**.
- `clippy::unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented` and
  `unreachable` are **deny**. Return an error, or fall back, instead. Tests may
  unwrap, expect and panic (`clippy.toml`).

Two conventions worth knowing before you touch the UI:

- **The window's state changes only on the window's thread.** The radio's
  callbacks and the long jobs (connect, a playbook, a firmware update, a
  settings transfer) run on threads of their own and report through
  `inbox::Outbox::post`, which also wakes the window. The window drains the
  inbox at the start of each frame. The `Link` calls that wait (`connect`,
  `send_raw`) are made from a job's thread, never from the window's.
- **Errors go through `App::fail`,** not `set_status`. The status line is
  rewritten twice a second while scanning, so anything posted there during a
  scan is gone before it can be read. `fail` colours the alert line *and*
  mirrors the message into the terminal, which is the durable record.

### Tests

`crates/ui/src/tests.rs` drives the window as a user would (egui_kittest) over
the fake link. Screenshots of the window are rendered by ignored tests behind a
feature:

```sh
cargo test -p aircord-ui --features screenshot -- --ignored screenshot
```

## User-facing text

The UI is English, with one deliberate exception: the guided view and the input
errors of the settings form are German, as their users are. Keep each part in
its language — a half-translated interface is worse than either language alone.

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org):
`<type>(<scope>): <imperative summary>` — e.g. `fix(connect): reset rxBuf before
enabling notifications`. Types: `feat`, `fix`, `refactor`, `perf`, `docs`,
`test`, `chore`, `build`, `ci`. Keep the subject ≤50 chars, imperative mood, no
trailing period. Add a body when the *why* isn't obvious.

## Merge requests

- Branch off `main`; keep pull requests focused.
- Ensure `task check` passes.
- Describe what changed and how you verified it. GUI and BLE behaviour cannot be
  exercised in CI, so state which device you tested against, or say that you did
  not.

## Icon

The app icon is `assets/icon.svg`; the window loads `assets/icon.png`, which is
rendered from it. After editing the SVG, regenerate the PNG, for example:

```sh
rsvg-convert -w 1024 -h 1024 assets/icon.svg -o assets/icon.png
```

**Proportions follow the macOS icon grid**: the squircle covers about 80% of the
canvas, the rest is transparent padding. Fill the whole canvas and the icon sits
visibly larger than its neighbours in the Dock and loses the system shadow.

Check the result at 32 px before committing — the dock icon is the size that
matters — and check the packaged icon in the `Aircord.app` that `task package`
builds.

## Adding a device or UART profile

Read the GATT dump printed to the terminal on connect and add the device's
write/notify UUIDs to `PROFILES` in `crates/ble/src/lib.rs`.

A control panel for the device is a
[device template](docs/device-templates.md) — a TOML file. Templates that are
broadly useful can ship as presets in `crates/core/presets/`; add the file to
`PRESETS` in `crates/core/src/device.rs`, which embeds it in the binary. Mark
anything destructive with `confirm = true`, and say in the pull request which
firmware revision you verified the AT syntax against.
