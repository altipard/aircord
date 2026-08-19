package device

import (
	"os"
	"path/filepath"
	"testing"
)

const validTOML = `
name  = "Dragino NB-IoT"
match = "(?i)^d20"

[[action]]
label = "Einstellungen anzeigen"
cmd   = "AT+CFG"
type  = "button"

[[action]]
label = "Intervall (s)"
cmd   = "AT+TDC=<n>"
type  = "input"

[[action]]
label = "Band"
type  = "select"
  [[action.options]]
  text = "B8"
  cmd  = "AT+QBAND=1,8"
`

func TestParseValid(t *testing.T) {
	tpl, err := Parse([]byte(validTOML))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if tpl.Name != "Dragino NB-IoT" {
		t.Errorf("Name = %q", tpl.Name)
	}
	if len(tpl.Actions) != 3 {
		t.Fatalf("actions = %d, want 3", len(tpl.Actions))
	}
	if tpl.Actions[0].Type != ActionButton {
		t.Errorf("action0 type = %q, want button", tpl.Actions[0].Type)
	}
	if tpl.Actions[2].Type != ActionSelect || len(tpl.Actions[2].Options) != 1 {
		t.Errorf("select action not parsed: %+v", tpl.Actions[2])
	}
}

func TestParseRejects(t *testing.T) {
	cases := map[string]string{
		"missing name": `
[[action]]
label = "x"
cmd   = "AT"
type  = "button"`,
		"no actions": `name = "x"`,
		"unknown type": `
name = "x"
[[action]]
label = "y"
cmd   = "AT"
type  = "toggle"`,
		"input without placeholder": `
name = "x"
[[action]]
label = "y"
cmd   = "AT+TDC=5"
type  = "input"`,
		"select without options": `
name = "x"
[[action]]
label = "y"
type  = "select"`,
		"button without cmd": `
name = "x"
[[action]]
label = "y"
type  = "button"`,
		"invalid match regexp": `
name  = "x"
match = "([unclosed"
[[action]]
label = "y"
cmd   = "AT"
type  = "button"`,
	}
	for name, src := range cases {
		if _, err := Parse([]byte(src)); err == nil {
			t.Errorf("%s: Parse succeeded, want error", name)
		}
	}
}

func TestMatches(t *testing.T) {
	tpl, err := Parse([]byte(validTOML))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if !tpl.Matches("D20-NB") {
		t.Error("D20-NB should match ^d20 (case-insensitive)")
	}
	if tpl.Matches("iPhone") {
		t.Error("iPhone should not match")
	}
}

func TestMatchesEmptyMatchNeverMatches(t *testing.T) {
	tpl, err := Parse([]byte(`
name = "manual only"
[[action]]
label = "x"
cmd   = "AT"
type  = "button"`))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if tpl.Matches("anything") {
		t.Error("empty match must never auto-match")
	}
}

func TestParseReadAction(t *testing.T) {
	tpl, err := Parse([]byte(`
name = "x"
[[action]]
label = "Firmware"
cmd   = "AT+VER"
type  = "read"
parse = "([0-9]+\\.[0-9]+\\.[0-9]+)"`))
	if err != nil {
		t.Fatalf("Parse read: %v", err)
	}
	if tpl.Actions[0].Type != ActionRead {
		t.Fatalf("type = %q, want read", tpl.Actions[0].Type)
	}
}

func TestReadRejectsMissingParse(t *testing.T) {
	_, err := Parse([]byte(`
name = "x"
[[action]]
label = "y"
cmd   = "AT+VER"
type  = "read"`))
	if err == nil {
		t.Fatal("read without parse should error")
	}
}

func TestParseConfirmFlag(t *testing.T) {
	tpl, err := Parse([]byte(`
name = "x"
[[action]]
label   = "Reboot"
cmd     = "ATZ"
type    = "button"
confirm = true

[[action]]
label = "Show config"
cmd   = "AT+CFG"
type  = "button"`))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if !tpl.Actions[0].Confirm {
		t.Error("confirm = true was not decoded")
	}
	if tpl.Actions[1].Confirm {
		t.Error("confirm defaulted to true; want false when omitted")
	}
}

func TestPresetMarksDestructiveActions(t *testing.T) {
	set := Load()
	tpl := set.Get("Dragino NB-IoT")
	if tpl == nil {
		t.Fatal("Dragino NB-IoT preset missing")
	}
	for _, a := range tpl.Actions {
		if a.Cmd == "ATZ" && !a.Confirm {
			t.Errorf("action %q sends ATZ without confirm", a.Label)
		}
	}
}

func TestExtract(t *testing.T) {
	a := Action{Type: ActionRead, Cmd: "AT+VER", Parse: `v?([0-9]+\.[0-9]+\.[0-9]+)`}
	got, ok := a.Extract("<< firmware v1.1.0 ready")
	if !ok || got != "1.1.0" {
		t.Fatalf("Extract = %q,%v want 1.1.0,true", got, ok)
	}
	if _, ok := a.Extract("no version here"); ok {
		t.Error("Extract should not match a line without a version")
	}
}

func TestExtractWholeMatchWhenNoGroup(t *testing.T) {
	a := Action{Type: ActionRead, Parse: `OK`}
	got, ok := a.Extract(">> OK")
	if !ok || got != "OK" {
		t.Fatalf("Extract = %q,%v want OK,true", got, ok)
	}
}

func TestRender(t *testing.T) {
	input := Action{Cmd: "AT+TDC=<seconds>", Type: ActionInput}
	if got := input.Render("60"); got != "AT+TDC=60" {
		t.Errorf("Render input = %q, want AT+TDC=60", got)
	}
	button := Action{Cmd: "AT+CFG", Type: ActionButton}
	if got := button.Render("ignored"); got != "AT+CFG" {
		t.Errorf("Render button = %q, want AT+CFG unchanged", got)
	}
}

// --- Set (embedded presets + user overrides) ---

func TestLoadEmbeddedPresets(t *testing.T) {
	set := loadFrom(presetFS, "presets", filepath.Join(t.TempDir(), "nonexistent"))
	if len(set.LoadErrs()) != 0 {
		t.Fatalf("LoadErrs = %v, want none", set.LoadErrs())
	}
	if set.Get("Dragino NB-IoT") == nil {
		t.Fatal("embedded Dragino preset missing")
	}
	if got := set.Suggest("D23-NB"); got == nil || got.Name != "Dragino NB-IoT" {
		t.Errorf("Suggest(D23-NB) = %v, want Dragino preset", got)
	}
	if set.Suggest("SomeRandomWatch") != nil {
		t.Error("Suggest for unknown device should be nil")
	}
}

func TestUserTemplateOverridesPreset(t *testing.T) {
	dir := t.TempDir()
	override := `
name  = "Dragino NB-IoT"
match = "(?i)^d20"
[[action]]
label = "Only Reset"
cmd   = "AT+RESET"
type  = "button"`
	if err := os.WriteFile(filepath.Join(dir, "mine.toml"), []byte(override), 0o644); err != nil {
		t.Fatal(err)
	}
	set := loadFrom(presetFS, "presets", dir)
	tpl := set.Get("Dragino NB-IoT")
	if tpl == nil {
		t.Fatal("template missing after override")
	}
	if len(tpl.Actions) != 1 || tpl.Actions[0].Cmd != "AT+RESET" {
		t.Errorf("user override not applied: %+v", tpl.Actions)
	}
}

func TestBadUserFileIsCollectedNotFatal(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "broken.toml"), []byte("{ not toml"), 0o644); err != nil {
		t.Fatal(err)
	}
	set := loadFrom(presetFS, "presets", dir)
	if len(set.LoadErrs()) == 0 {
		t.Error("bad user file should surface a LoadErr")
	}
	// A broken user file must not blank the embedded presets.
	if set.Get("Dragino NB-IoT") == nil {
		t.Error("presets lost because of one bad user file")
	}
}

func TestNamesSorted(t *testing.T) {
	dir := t.TempDir()
	extra := `
name = "Aardvark Sensor"
[[action]]
label = "x"
cmd   = "AT"
type  = "button"`
	if err := os.WriteFile(filepath.Join(dir, "a.toml"), []byte(extra), 0o644); err != nil {
		t.Fatal(err)
	}
	set := loadFrom(presetFS, "presets", dir)
	names := set.Names()
	if len(names) < 2 || names[0] != "Aardvark Sensor" {
		t.Errorf("Names not sorted or incomplete: %v", names)
	}
}
