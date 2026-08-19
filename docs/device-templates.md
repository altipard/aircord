# Device templates

A device template turns a device's AT vocabulary into a control panel: labelled
buttons, input fields, dropdowns and read-outs. The point is that the person
configuring a sensor should not have to remember whether the uplink interval is
`AT+TDC=` or `AT+TXP=`.

Templates are TOML. Aircord ships presets inside the binary, and reads your own
from `<config>/aircord/templates/`. A file whose `name` matches a preset
replaces it, so you can adapt a shipped template without patching the source.

## Structure

```toml
name  = "Dragino NB-IoT"
match = "(?i)^(d20|d23|wqs)"

[[action]]
label = "Show configuration"
cmd   = "AT+CFG"
type  = "button"
```

| Field | Meaning |
| ----- | ------- |
| `name` | Shown in the panel header and the template dropdown. Required. |
| `match` | Regexp tested against the advertised device name. A template without `match` never auto-selects but can still be chosen manually. |
| `[[action]]` | One control. At least one is required. |

On connect, Aircord picks the first template whose `match` fits the device name.
You can always override the choice from the dropdown in the panel header.

## Action types

### `button` — send a fixed command

```toml
[[action]]
label = "Show configuration"
cmd   = "AT+CFG"
type  = "button"
```

### `input` — substitute a value into a command

```toml
[[action]]
label = "Uplink interval (seconds)"
cmd   = "AT+TDC=<seconds>"
type  = "input"
```

The `<…>` placeholder is replaced with what the user types. The name inside the
angle brackets is free-form; it exists to document the expected value.

### `select` — a fixed set of choices

```toml
[[action]]
label = "NB-IoT band (Germany)"
type  = "select"

  [[action.options]]
  text = "B8 (900 MHz)"
  cmd  = "AT+QBAND=1,8"

  [[action.options]]
  text = "B20 (800 MHz)"
  cmd  = "AT+QBAND=1,20"
```

Each option carries its own complete command. Every option needs both `text` and
`cmd`.

### `read` — send a query and show the answer

```toml
[[action]]
label = "Firmware version"
cmd   = "AT+VER"
type  = "read"
parse = "([0-9]+\\.[0-9]+\\.[0-9]+)"
```

Pressing **Read** sends `cmd` and applies `parse` to incoming reply lines. The
first capture group becomes the displayed value; with no capture group, the whole
match is used. If nothing matches within 15 seconds the field shows
`(no reply)` rather than leaving a stale value on screen.

## Marking destructive actions

```toml
[[action]]
label   = "Reboot device"
cmd     = "ATZ"
type    = "button"
confirm = true
```

`confirm = true` makes the panel ask before sending, showing the exact command.
Use it for anything a user would not want to trigger with a stray click on a
device that is already deployed: reboots, factory resets, and band changes that
can take the device off the network.

A control panel is a column of similar-looking buttons. Without `confirm`, the
difference between "show configuration" and "wipe configuration" is two
millimetres of mouse travel.

## Validation

Templates are validated on load. A file with a problem is skipped and the reason
is reported in the terminal at startup — one bad template never prevents the
others from loading. The rules:

- `name` is required, and at least one action must exist
- every action needs a `label`
- `button` and `read` need a `cmd`; `input` needs a `cmd` containing a `<…>`
  placeholder; `select` needs `options`, each with `text` and `cmd`
- `read` needs a `parse` regexp, and it must compile
- `match`, when present, must compile

## Adding a template for new hardware

1. Connect to the device and read the GATT dump in the terminal. If no known
   UART profile is found, add its write/notify UUIDs to `uartProfiles` in
   `internal/ble/ble.go` first.
2. Work out the commands by hand in the console.
3. Write the TOML into `<config>/aircord/templates/<something>.toml`.
4. Reconnect. The template loads at startup, so restart Aircord after editing.

If the template is broadly useful, contribute it as a preset — see
[CONTRIBUTING.md](../CONTRIBUTING.md).
