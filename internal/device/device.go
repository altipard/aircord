// Package device describes a BLE device type as a declarative template of
// friendly operations that map to AT commands, so a user need not know the raw
// syntax. Templates ship as embedded presets and can be overridden or extended
// by TOML files in the user config dir (see Load).
//
// The template is intentionally UI-agnostic: it says what actions exist and what
// command each sends, not how they are rendered. The main package turns a
// Template into a Fyne control panel.
package device

import (
	"fmt"
	"regexp"
	"strings"

	"github.com/BurntSushi/toml"
)

// ActionType enumerates how a control renders and behaves.
type ActionType string

const (
	// ActionButton sends a fixed command when clicked (e.g. "AT+CFG").
	ActionButton ActionType = "button"
	// ActionInput pairs a value field with a command template whose <placeholder>
	// is substituted with the entered value (e.g. "AT+TDC=<seconds>").
	ActionInput ActionType = "input"
	// ActionSelect offers a fixed set of options, each mapping to its own command.
	ActionSelect ActionType = "select"
	// ActionRead sends a command, then extracts a value from the device's reply
	// with the action's Parse regexp and shows it in a field (v2 dashboard).
	ActionRead ActionType = "read"
)

// Option is one choice of a select action.
type Option struct {
	Text string `toml:"text"` // label shown to the user
	Cmd  string `toml:"cmd"`  // command sent when chosen
}

// Action is one control in a device's panel.
type Action struct {
	Label   string     `toml:"label"`
	Cmd     string     `toml:"cmd"`
	Type    ActionType `toml:"type"`
	Options []Option   `toml:"options"`
	// Parse is reserved for the v2 parsed-value dashboard: a regexp with one
	// capture group applied to the device reply. Unused (and unvalidated beyond
	// being optional) in v1.
	Parse string `toml:"parse"`
	// Confirm makes the panel ask before sending. Set it on anything a user
	// would not want to trigger by a stray click on a device that is already
	// deployed — reboots, factory resets, band changes that drop the uplink.
	Confirm bool `toml:"confirm"`
}

// placeholder matches the first <name> token in a command template.
var placeholder = regexp.MustCompile(`<[^>]*>`)

// Render substitutes the action's <placeholder> with value. Commands without a
// placeholder (button, select) are returned unchanged.
func (a Action) Render(value string) string {
	return placeholder.ReplaceAllString(a.Cmd, value)
}

// Extract applies the action's Parse regexp to a device reply line, returning the
// first capture group (or the whole match when there are no groups) and whether
// it matched. Read actions use it to pull a value out of the reply.
func (a Action) Extract(line string) (string, bool) {
	if a.Parse == "" {
		return "", false
	}
	re, err := regexp.Compile(a.Parse)
	if err != nil {
		return "", false
	}
	m := re.FindStringSubmatch(line)
	if m == nil {
		return "", false
	}
	if len(m) > 1 {
		return m[1], true
	}
	return m[0], true
}

// Template describes a device type and the actions its control panel exposes.
type Template struct {
	Name    string   `toml:"name"`
	Match   string   `toml:"match"` // regexp tested against the advertised device name
	Actions []Action `toml:"action"`

	re *regexp.Regexp // compiled Match; nil when Match is empty (manual-select only)
}

// Matches reports whether the template's Match regexp matches the device name.
// A template with an empty Match never auto-matches; it can still be chosen
// manually.
func (t *Template) Matches(name string) bool {
	if t.re == nil {
		return false
	}
	return t.re.MatchString(name)
}

// Parse decodes and validates a single template from TOML.
func Parse(data []byte) (*Template, error) {
	var t Template
	if err := toml.Unmarshal(data, &t); err != nil {
		return nil, err
	}
	if err := t.validate(); err != nil {
		return nil, err
	}
	return &t, nil
}

// validate checks required fields, compiles the Match regexp, and enforces the
// per-type invariants (input needs a placeholder, select needs options). It sets
// t.re on success.
func (t *Template) validate() error {
	if strings.TrimSpace(t.Name) == "" {
		return fmt.Errorf("template: name is required")
	}
	if t.Match != "" {
		re, err := regexp.Compile(t.Match)
		if err != nil {
			return fmt.Errorf("template %q: invalid match regexp: %w", t.Name, err)
		}
		t.re = re
	}
	if len(t.Actions) == 0 {
		return fmt.Errorf("template %q: no actions", t.Name)
	}
	for _, a := range t.Actions {
		if strings.TrimSpace(a.Label) == "" {
			return fmt.Errorf("template %q: an action is missing its label", t.Name)
		}
		switch a.Type {
		case ActionButton:
			if strings.TrimSpace(a.Cmd) == "" {
				return fmt.Errorf("template %q action %q: button needs a cmd", t.Name, a.Label)
			}
		case ActionInput:
			if strings.TrimSpace(a.Cmd) == "" {
				return fmt.Errorf("template %q action %q: input needs a cmd", t.Name, a.Label)
			}
			if !placeholder.MatchString(a.Cmd) {
				return fmt.Errorf("template %q action %q: input cmd needs a <placeholder>", t.Name, a.Label)
			}
		case ActionSelect:
			if len(a.Options) == 0 {
				return fmt.Errorf("template %q action %q: select needs options", t.Name, a.Label)
			}
			for _, o := range a.Options {
				if strings.TrimSpace(o.Text) == "" || strings.TrimSpace(o.Cmd) == "" {
					return fmt.Errorf("template %q action %q: every option needs text and cmd", t.Name, a.Label)
				}
			}
		case ActionRead:
			if strings.TrimSpace(a.Cmd) == "" {
				return fmt.Errorf("template %q action %q: read needs a cmd", t.Name, a.Label)
			}
			if strings.TrimSpace(a.Parse) == "" {
				return fmt.Errorf("template %q action %q: read needs a parse regexp", t.Name, a.Label)
			}
			if _, err := regexp.Compile(a.Parse); err != nil {
				return fmt.Errorf("template %q action %q: invalid parse regexp: %w", t.Name, a.Label, err)
			}
		default:
			return fmt.Errorf("template %q action %q: unknown type %q (want button|input|select)", t.Name, a.Label, a.Type)
		}
	}
	return nil
}
