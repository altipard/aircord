# Device templates

A device template turns a device's AT vocabulary into a control panel: labelled
buttons, input fields, dropdowns and read-outs. The point is that the person
configuring a sensor should not have to remember whether the uplink interval is
`AT+TDC=` or `AT+TXP=`.

A template can also describe the device's **settings** as typed fields
(`[[setting]]`): the current values come from `AT+CFG`, only what changed is
sent, and `AT+CFG` is read again to check the result. See [Settings](#settings).
The Rust core (`rust/crates/core`: `settings`, `cfg`, `transfer`) implements
this; the settings form in the window is not built yet, and the Go build ignores
`[[setting]]`.

Templates are TOML. Aircord ships presets inside the binary, and reads your own
from `<config>/aircord/templates/`. A file whose `name` matches a preset
replaces it, so you can adapt a shipped template without patching the source.
The Rust build's presets live in `rust/crates/core/presets/`; the Go build's in
`internal/device/presets/`.

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
| `match` | Regexp tested against the advertised device name. A template without `match` never auto-selects by name but can still be chosen manually. |
| `model` | Regexp tested against the value of the `AT+MODEL` line of `AT+CFG`, e.g. `S31-NB,v1.4.0`. Anchor it (`'^S31-NB(,\|$)'`); it may pin a firmware version. Optional. |
| `[[action]]` | One control. |
| `[[setting]]` | One typed setting, see [Settings](#settings). |

A template needs at least one action or setting. Files with only `[[action]]`
tables, as written before settings existed, stay valid.

On connect, Aircord picks the first template whose `match` fits the device name.
You can always override the choice from the dropdown in the panel header.

`model` is for selection once `AT+CFG` has been read: the core's
`Templates::select` prefers the first template (in name order) whose `model`
fits the `AT+MODEL` value and falls back to `match`, because the advertised name
of a Dragino NB node is usually its IMEI and says nothing about the model. The
window does not call it yet.

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

## Settings

```toml
name  = "Dragino S31-NB"
model = '^S31-NB(,|$)'

[[setting]]
key          = "TDC"
label        = "Sendeintervall"
group        = "Messen & Senden"
help         = "Wie oft der Sensor Daten schickt."
type         = "duration"
unit         = "s"
presets      = [900, 1800, 3600, 7200]

[[setting]]
key          = "UNAME"
label        = "MQTT-Benutzer"
group        = "MQTT-Zugang"
type         = "text"
risk         = "network"
needs_reboot = true
visible_if   = "PRO=3,*"
```

A setting is one `AT+<KEY>=<value>` line of the device's `AT+CFG` output. Aircord
reads the value from there and sends `AT+<KEY>=<value>` to change it.

| Field | Meaning |
| ----- | ------- |
| `key` | The name as `AT+CFG` prints it, in capitals: `TDC`, `5VT`, `POWER_SAVE`. Unique per template. Required. |
| `label` | What the person sees, in plain words. Required. |
| `group` | The task it belongs to (`"Netz"`, `"Messen & Senden"`); the form groups by it. Required. |
| `help` | One sentence under the field. Also appended to the error when a value has the wrong format, so put the expected format here. |
| `type` | `duration`, `number`, `choice`, `toggle`, `text` or `secret`. Required. |
| `risk` | `none` (default), `network` (a wrong value takes the device off the network or away from its server), `reboot` (the change restarts the device), `irreversible` (cannot be undone in the field). Aircord asks before transferring a `network` or `irreversible` change. |
| `needs_reboot` | `true` when the value takes effect only after `ATZ`. Aircord then offers the template's restart action after the transfer; it never sends it on its own. |
| `visible_if` | `"KEY=pattern"`: shown only while `KEY` has a matching value, the form's value over the device's. `*` matches anything, the rest is literal, and the whole value must match: `"PRO=3,*"`. |
| `expert` | `true`: shown only in expert mode. |

Unknown fields are refused, so a typo such as `needs_rebot` does not silently
drop a flag. Fields that do not apply to the type (a `unit` on a `text`) are
refused too.

### Types

| `type` | Extra fields | Notes |
| ------ | ------------ | ----- |
| `duration` | `unit` (`ms`, `s`, `min`, `h`; required), `presets`, `min`, `max` | A whole number in the device's unit, never negative. Shown in words: `900` s is "15 Minuten". |
| `number` | `unit` (any display text, e.g. `°C`), `presets`, `min`, `max` | A whole number. |
| `choice` | `options = [{ text, value }]` (required), `custom`, `pattern`, `max_len` | One of the options. With `custom = true` any other value is allowed too, checked by `pattern` and `max_len` (only allowed with `custom`). Option values are unique. |
| `toggle` | `on`, `off` (both required, different) | The values the two positions send. Inverted device logic is just values: `on = "0,8,16"`, `off = "1,0,0"`. |
| `text` | `pattern`, `max_len`, `clearable` | Not empty, unless `clearable = true`: then an empty input clears the value and the device gets `AT+<KEY>=NULL`, the way `AT+CFG` prints an unset value. |
| `secret` | `pattern`, `max_len`, `clearable` | Like `text`, never shown or logged in clear. An empty input means "unchanged", because the field cannot show the current value; with `clearable = true` the value is cleared only on an explicit request (`transfer::plan_clearing`). |

`distinct = [2, 3]` (for `text`, `secret` and the free text of a `choice`)
refuses a value whose comma-separated parts at those positions, counted from 1,
are equal: `AT+POWER_SAVE=0,8,8` would be a window that starts where it ends.

A `pattern` must match the whole input; Aircord anchors it, so `'[a-z]{6}'`
means exactly six lower-case letters. Write regexps as TOML literal strings
(`'…'`) to avoid doubling every backslash.

Input errors are shown in plain German for the person in the field, e.g. "Bitte
mindestens 180 Sekunden eingeben." A line break in any value is refused.

### Reading, transferring, verifying

What the core does (`rust/crates/core/src/transfer.rs`); the window will build
on it:

1. `read_cfg` sends `AT+CFG` and reads every `AT+<KEY>=<value>` line up to `OK`.
   Blank lines, echoes, banners and the `Signal Strength:` lines a node prints
   every few seconds are skipped; `NULL` reads as empty. A dump without any
   setting or without its `AT+MODEL` line is an error, not an empty state.
2. `Template::settings_for` lists the settings whose `key` the device reported.
   A setting the device does not report is not available and is never sent
   blind.
3. `plan` compares the wanted values with the device's and checks only those
   that differ, in template order. Numbers compare by value (`0900` is `900`);
   text compares part by part between commas, without outer spaces and double
   quotes (`"8.8.8.8", "8.8.4.4"` is `"8.8.8.8","8.8.4.4"`); secrets compare
   exactly. `DNSCFG` goes out without spaces around its commas
   (`"1.1.1.1","8.8.4.4"`), the form `AT+CFG` prints; other text is sent as
   entered. Settings hidden by `visible_if` are left out. Empty input is
   refused, except for a `clearable` setting, which is then sent as `NULL`. An
   empty secret is no change at all: a password field left empty keeps the
   device's password. `plan_clearing` takes the keys the person explicitly asked
   to clear; that is the only way to send `NULL` for a secret, and it is refused
   for a setting that is not `clearable`. The presets mark the MQTT fields
   (`CLIENT`, `UNAME`, `PWD`, `PUBTOPIC`, `SUBTOPIC`) clearable.
   `needing_confirmation` names the `network` and `irreversible` changes to ask
   about.
4. `apply` sends each change and waits for `OK`. `AT_ERROR`, `AT_PARAM_ERROR` and
   the other `AT_…ERROR` lines mark that field as failed; the others are still
   sent. A command that gets no answer is not failed: the device may have taken
   it, so the read-back decides. After such a silence `apply` waits briefly and
   discards a late reply, so it cannot answer the next command. A lost link or
   a stop ends the transfer; the rest is reported as not sent.
5. It then reads `AT+CFG` again and reports each field as verified, different
   (the device kept another value), failed (refused or not sent), or unverified
   (the read-back did not show it or could not be read). The read-back must
   contain settings but not necessarily `AT+MODEL`; only the first read needs
   the model, to pick the template.
6. When a change that may have been taken (verified or unverified) has
   `needs_reboot`, the outcome says a restart is due and
   `Template::reboot_action` names the template's restart (a `button` with
   `cmd = "ATZ"`). Nothing in the core sends it.

A worked example of real `AT+CFG` output is in
[devices/s31-nb-cfg-sample.md](devices/s31-nb-cfg-sample.md).

### Passwords

`AT+PWORD` and `AT+PIN` (device password), `AT+PWD` (MQTT password) and
`AT+OTAPWD` are treated as secret whatever the template says, and so are
settings of `type = "secret"`. Some firmware prints `AT+PWORD` in clear in
`AT+CFG`, so this matters for any transcript you save or share.

- The link masks the fixed keys in every line it logs, sent (`>> `) and
  received (`<< `), so the terminal and a saved transcript show
  `AT+PWORD=******`. The BLE link and the test fake both log through
  `redact::log_line`. Reply lines reach the core unmasked, because verifying a
  change needs the value.
- A transfer sends secrets through `Link::send_secret`, and no change, result or
  `{:?}` output carries their value; the read-back masks the template's own
  secrets too.
- `redact::redact_line` (or `Template::redact`, which adds the template's
  secrets) masks the value in any line as `******`. The link knows only the
  fixed keys: a template's own secret keys are not masked in the link's log.

## Validation

Templates are validated on load. A file with a problem is skipped and the reason
is reported in the terminal at startup — one bad template never prevents the
others from loading. The rules:

- `name` is required, and at least one action or setting must exist
- every action needs a `label`
- `button` and `read` need a `cmd`; `input` needs a `cmd` containing a `<…>`
  placeholder; `select` needs `options`, each with `text` and `cmd`
- `read` needs a `parse` regexp, and it must compile
- `match` and `model`, when present, must compile
- every setting needs `key`, `label`, `group` and a known `type` with its
  required fields; `pattern`, `min`/`max`, `presets` and `visible_if` must make
  sense (see [Settings](#settings))

## Adding a template for new hardware

1. Connect to the device and read the GATT dump in the terminal. If no known
   UART profile is found, add its write/notify UUIDs to `uartProfiles` in
   `internal/ble/ble.go` first.
2. Work out the commands by hand in the console.
3. Write the TOML into `<config>/aircord/templates/<something>.toml`.
4. Reconnect. The template loads at startup, so restart Aircord after editing.

If the template is broadly useful, contribute it as a preset — see
[CONTRIBUTING.md](../CONTRIBUTING.md).
