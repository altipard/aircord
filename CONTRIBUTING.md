# Contributing

Thanks for your interest in Aircord. This is a small project; the bar is simply
"green gate, clear commits."

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).
Security issues go to [SECURITY.md](SECURITY.md), not the public tracker.

## Development

Requires **Go 1.25+**. On macOS everything builds out of the box; Linux needs
the Fyne GUI dev packages (`libgl1-mesa-dev`, `xorg-dev`).

```sh
task run      # build and run
task check    # gofmt check + go vet + golangci-lint + tests — the CI gate
task test     # tests only
task lint     # golangci-lint only
```

No `task`? The equivalents are `go run .`, `go vet ./...`, `go test ./...`, and
`gofmt -l .` (must print nothing).

Run `task check` before opening a pull request — CI runs the same gate.

## Code layout

GUI/BLE code lives in the root `main` package; reusable, GUI-free logic sits in
unit-tested `internal/` packages. New pure logic (parsing, persistence,
filtering) belongs in `internal/…` with tests, not in a `main`-package file.

Two conventions worth knowing before you touch the UI:

- **UI-thread discipline.** Anything touching a widget runs on the Fyne UI
  thread. BLE callbacks arrive on other goroutines and must be marshalled with
  `fyne.Do`. Functions that assume the UI thread say so in their doc comment —
  keep that up.
- **Errors go through `s.fail`,** not `s.setStatus`. The status line is
  rewritten twice a second while scanning, so anything posted there during a
  scan is gone before it can be read. `fail` colours the alert line *and*
  mirrors the message into the terminal, which is the durable record.

## User-facing text

The UI is English throughout. Keep it that way — a half-translated interface is
worse than either language alone.

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

The app icon is `assets/icon.svg`. After editing it, regenerate the embedded PNG:

```sh
go run ./tools/genicon
```

Two things to know before editing it:

- **oksvg**, the renderer Fyne uses, handles paths, circles and strokes
  faithfully but **drops the corner radius on `<rect rx="…">`** — it renders a
  sharp-cornered box instead. The squircle is therefore drawn as a `<path>` with
  arc segments. Stick to `<path>` and `<circle>`.
- **Proportions follow the macOS icon grid**: the squircle covers about 80% of
  the canvas, the rest is transparent padding. Fill the whole canvas and the
  icon sits visibly larger than its neighbours in the Dock and loses the system
  shadow.

Check the result at 32 px before committing — the dock icon is the size that
matters — and verify the bundled icon with `task package`, which converts the
PNG to `.icns`.

## Adding a device or UART profile

Read the GATT dump printed to the terminal on connect and add the device's
write/notify UUIDs to `uartProfiles` in `internal/ble/ble.go`.

A control panel for the device is a
[device template](docs/device-templates.md) — a TOML file. Templates that are
broadly useful can ship as presets in `internal/device/presets/`. Mark anything
destructive with `confirm = true`, and say in the pull request which firmware
revision you verified the AT syntax against.
