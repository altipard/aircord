package playbook

import (
	"reflect"
	"testing"
	"time"
)

func tempStore(t *testing.T) *Store {
	t.Helper()
	return newStoreAt(t.TempDir())
}

func TestSaveLoadRoundTrip(t *testing.T) {
	s := tempStore(t)
	want := Playbook{
		Name: "Provision D23",
		Steps: []Step{
			{Cmd: "AT+CFG"},
			{Cmd: "AT+TDC=3600", WaitMs: 1500},
		},
	}
	if err := s.Save(want); err != nil {
		t.Fatalf("Save: %v", err)
	}
	got, err := s.Load(want.Name)
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("round trip mismatch:\n got %+v\nwant %+v", got, want)
	}
}

func TestSaveRejectsEmptyName(t *testing.T) {
	s := tempStore(t)
	if err := s.Save(Playbook{Steps: []Step{{Cmd: "AT"}}}); err == nil {
		t.Fatal("Save with blank name should error")
	}
}

func TestListSortedAndDelete(t *testing.T) {
	s := tempStore(t)
	for _, n := range []string{"Beta", "Alpha"} {
		_ = s.Save(Playbook{Name: n, Steps: []Step{{Cmd: "AT"}}})
	}
	if got := s.List(); !reflect.DeepEqual(got, []string{"Alpha", "Beta"}) {
		t.Fatalf("List = %v", got)
	}
	if err := s.Delete("Alpha"); err != nil {
		t.Fatalf("Delete: %v", err)
	}
	if got := s.List(); !reflect.DeepEqual(got, []string{"Beta"}) {
		t.Fatalf("List after delete = %v", got)
	}
}

func TestParseSteps(t *testing.T) {
	text := `
# provision sequence
AT+CFG
AT+TDC=3600 @1500
   AT+VER

# trailing comment
`
	got := ParseSteps(text)
	want := []Step{
		{Cmd: "AT+CFG", WaitMs: 0},
		{Cmd: "AT+TDC=3600", WaitMs: 1500},
		{Cmd: "AT+VER", WaitMs: 0},
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("ParseSteps = %+v, want %+v", got, want)
	}
}

func TestParseStepsIgnoresBadWait(t *testing.T) {
	// "@later" is not a number, so the whole line is kept as the command.
	got := ParseSteps("AT+SEND=hello@later")
	want := []Step{{Cmd: "AT+SEND=hello@later", WaitMs: 0}}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("ParseSteps = %+v, want %+v", got, want)
	}
}

func TestFormatStepsRoundTrips(t *testing.T) {
	steps := []Step{
		{Cmd: "AT+CFG"},
		{Cmd: "AT+TDC=3600", WaitMs: 1500},
		{Cmd: "AT+APN=iot.1nce.net", WaitFor: "OK"},
		{Cmd: "AT+PRO=2,5", WaitFor: "OK", WaitMs: 500},
	}
	if got := ParseSteps(FormatSteps(steps)); !reflect.DeepEqual(got, steps) {
		t.Fatalf("Parse(Format(steps)) = %+v, want %+v", got, steps)
	}
}

func TestParseStepsWaitFor(t *testing.T) {
	text := "AT+APN=iot.1nce.net ?OK\n" +
		"AT+TDC=3600 ? @1500\n" + // bare "?" defaults to OK, pause combined
		"AT+QBAND?\n" + // query syntax without space is a command, not a wait
		"ATZ"
	got := ParseSteps(text)
	want := []Step{
		{Cmd: "AT+APN=iot.1nce.net", WaitFor: "OK"},
		{Cmd: "AT+TDC=3600", WaitFor: "OK", WaitMs: 1500},
		{Cmd: "AT+QBAND?"},
		{Cmd: "ATZ"},
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("ParseSteps = %+v, want %+v", got, want)
	}
}

func TestCompileWaitFallsBackToLiteral(t *testing.T) {
	if !CompileWait("OK").MatchString("OK") {
		t.Fatal("valid pattern should match")
	}
	// "(" alone is an invalid regexp — must fall back to a literal match.
	if !CompileWait("(").MatchString("ERROR (2)") {
		t.Fatal("invalid regexp should degrade to literal substring match")
	}
}

func TestAwaitMatch(t *testing.T) {
	lines := make(chan string, 4)
	lines <- "Set APN successfully"
	lines <- "OK"
	if !AwaitMatch(lines, CompileWait("OK"), time.Second, nil) {
		t.Fatal("should match the OK line")
	}
	// No matching line → timeout.
	empty := make(chan string, 1)
	empty <- "noise"
	if AwaitMatch(empty, CompileWait("OK"), 50*time.Millisecond, nil) {
		t.Fatal("should time out without a match")
	}
	// Dropped link aborts before the timeout.
	start := time.Now()
	if AwaitMatch(make(chan string), CompileWait("OK"), 5*time.Second, func() bool { return false }) {
		t.Fatal("dropped link should abort the wait")
	}
	if time.Since(start) > 2*time.Second {
		t.Fatal("link-drop abort should beat the timeout")
	}
}

func TestEnsureDefaultDoesNotOverwrite(t *testing.T) {
	s := tempStore(t)
	user := Playbook{Name: "Setup", Steps: []Step{{Cmd: "AT+MINE"}}}
	if err := s.Save(user); err != nil {
		t.Fatalf("Save: %v", err)
	}
	s.EnsureDefault(Playbook{Name: "Setup", Steps: []Step{{Cmd: "AT+SHIPPED"}}})
	got, err := s.Load("Setup")
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if !reflect.DeepEqual(got, user) {
		t.Fatalf("EnsureDefault overwrote user playbook: %+v", got)
	}
	s.EnsureDefault(Playbook{Name: "Fresh", Steps: []Step{{Cmd: "AT+NEW"}}})
	if _, err := s.Load("Fresh"); err != nil {
		t.Fatalf("EnsureDefault should create missing playbook: %v", err)
	}
}
