package playbook

import (
	"reflect"
	"testing"
)

func TestRiskyFlagsDestructiveCommands(t *testing.T) {
	risky := []string{
		"ATZ",
		"atz",
		"  ATZ  ",
		"AT+FDR",
		"AT+FACTORY",
		"AT+RESET",
		"AT+REBOOT",
		"AT+QRST",
	}
	for _, cmd := range risky {
		if !Risky(cmd) {
			t.Errorf("Risky(%q) = false, want true", cmd)
		}
	}
}

func TestRiskyLeavesOrdinaryCommandsAlone(t *testing.T) {
	safe := []string{
		"AT+CFG",
		"AT+TDC=900",
		"AT+APN=iot.1nce.net",
		"AT+QBAND=2,8,20",
		"AT+VER",
		"",           // empty step
		"ATZED",      // word boundary: not ATZ
		"AT+FDRINFO", // word boundary: not AT+FDR
	}
	for _, cmd := range safe {
		if Risky(cmd) {
			t.Errorf("Risky(%q) = true, want false", cmd)
		}
	}
}

func TestRiskyStepsListsEachCommandOnce(t *testing.T) {
	steps := []Step{
		{Cmd: "AT+CFG"},
		{Cmd: "ATZ"},
		{Cmd: "AT+TDC=900"},
		{Cmd: "AT+FDR"},
		{Cmd: "ATZ"}, // duplicate
	}
	got := RiskySteps(steps)
	want := []string{"ATZ", "AT+FDR"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("RiskySteps = %v, want %v", got, want)
	}
}

func TestRiskyStepsEmptyWhenNothingDestructive(t *testing.T) {
	steps := []Step{{Cmd: "AT+CFG"}, {Cmd: "AT+VER"}}
	if got := RiskySteps(steps); len(got) != 0 {
		t.Fatalf("RiskySteps = %v, want empty", got)
	}
}
