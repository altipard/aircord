package main

import (
	"bytes"
	"testing"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/test"
	"fyne.io/fyne/v2/widget"

	"github.com/altipard/aircord/internal/ble"
	"github.com/altipard/aircord/internal/otanb"
)

func TestMatchesIMEI(t *testing.T) {
	cases := []struct {
		name, imei string
		want       bool
	}{
		{"860600000000001", "860600000000001", true},
		{" 860600000000001 ", "860600000000001", true},  // advert padding
		{"NB-860600000000001", "860600000000001", true}, // prefixed name
		{"860600000000002", "860600000000001", false},
		{"D20S-NB", "860600000000001", false},
		{"", "860600000000001", false},
		{"860600000000001", "", false}, // empty IMEI must never match everything
		{"12345678", "1234", false},    // a short fragment cannot match by substring
		{"1234", "1234", true},         // but exact still does
	}
	for _, c := range cases {
		if got := matchesIMEI(c.name, c.imei); got != c.want {
			t.Errorf("matchesIMEI(%q, %q) = %v, want %v", c.name, c.imei, got, c.want)
		}
	}
}

func TestLooksLikeIMEI(t *testing.T) {
	if !looksLikeIMEI("860600000000001") {
		t.Error("15 digits must look like an IMEI")
	}
	for _, bad := range []string{"", "D20S-NB", "86060000000000", "8606000000000012", "86060000000000a"} {
		if looksLikeIMEI(bad) {
			t.Errorf("%q must not look like an IMEI", bad)
		}
	}
}

func TestExtractIMEI(t *testing.T) {
	cases := []struct {
		line string
		want string
		ok   bool
	}{
		{"IMEI: 860600000000001", "860600000000001", true},
		{"imei=860600000000001", "860600000000001", true},
		{"AT+CFG", "", false},
		{"IMSI: 901405000000001", "", false}, // a different 15-digit id
		{"IMEI: 8606", "", false},            // too short
	}
	for _, c := range cases {
		got, ok := extractIMEI(c.line)
		if got != c.want || ok != c.ok {
			t.Errorf("extractIMEI(%q) = (%q, %v), want (%q, %v)", c.line, got, ok, c.want, c.ok)
		}
	}
}

func TestOTAPassword(t *testing.T) {
	pw, err := otaPassword("  ")
	if err != nil || !bytes.Equal(pw, otanb.DefaultPassword()) {
		t.Fatalf("empty PIN = (%x, %v), want the default password", pw, err)
	}
	pw, err = otaPassword("123456")
	if err != nil || string(pw) != "123456OO" {
		t.Fatalf("PIN 123456 = (%q, %v), want 123456OO", pw, err)
	}
	if _, err := otaPassword("123456789"); err == nil {
		t.Fatal("a 9-char PIN must be rejected, the field is 8 bytes")
	}
}

// newTestScanner is the minimum a dialog-level test needs: widgets the body
// reads and a client that reports "not connected".
func newTestScanner() *scanner {
	test.NewApp()
	s := &scanner{pinEntry: widget.NewPasswordEntry()}
	s.alert = newAlertLine()
	s.ble = ble.New(s)
	return s
}

func TestBuildOTAJobValidation(t *testing.T) {
	s := newTestScanner()

	if _, err := s.buildOTAJob("860600000000001", false); err == nil {
		t.Error("no image chosen must be rejected")
	}
	s.otaImage = []byte{1, 2, 3}
	s.otaFile = "app.bin"
	if _, err := s.buildOTAJob("  ", false); err == nil {
		t.Error("empty IMEI must be rejected")
	}
	if _, err := s.buildOTAJob("860600000000001", true); err == nil {
		t.Error("ATZ reset without a connection must be rejected")
	}

	s.pinEntry.SetText("123456")
	job, err := s.buildOTAJob(" 860600000000001 ", false)
	if err != nil {
		t.Fatalf("buildOTAJob: %v", err)
	}
	if job.imei != "860600000000001" || job.file != "app.bin" || job.sendReset {
		t.Errorf("job = %+v", job)
	}
	if string(job.plan.Password) != "123456OO" {
		t.Errorf("password = %q, want the padded PIN", job.plan.Password)
	}
	if len(job.plan.Firmware.Image) != 8 {
		t.Errorf("image padded to %d bytes, want 8", len(job.plan.Firmware.Image))
	}
	if s.otaIMEI != "860600000000001" {
		t.Error("the IMEI must be remembered for the next dialog")
	}
}

func TestDefaultIMEIFromSelectedBootloader(t *testing.T) {
	s := &scanner{hasSel: true, selName: "860600000000001"}
	if got := s.defaultIMEI(); got != "860600000000001" {
		t.Errorf("defaultIMEI = %q, want the selected node's name", got)
	}
	s.selName = "D20S-NB"
	if got := s.defaultIMEI(); got != "" {
		t.Errorf("defaultIMEI = %q for an application name, want empty", got)
	}
	s.otaIMEI = "860600000000002"
	if got := s.defaultIMEI(); got != "860600000000002" {
		t.Errorf("defaultIMEI = %q, want the last used IMEI", got)
	}
}

// TestFirmwareBodyFitsDialog keeps the dialog's minimum height below the size
// showModal asks for: a body taller than that grows the dialog past the
// window on a small laptop screen, and the Start button ends up off-screen.
func TestFirmwareBodyFitsDialog(t *testing.T) {
	s := newTestScanner()
	body := s.firmwareBody(func() {})
	w := test.NewWindow(body)
	defer w.Close()
	limit := fyne.NewSize(modalSize.Width, modalSize.Height-dialogChrome)
	if min := body.MinSize(); min.Height > limit.Height || min.Width > limit.Width {
		t.Errorf("firmware dialog min size %v exceeds dialog %v", min, limit)
	}
}
