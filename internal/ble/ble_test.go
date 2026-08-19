package ble

import (
	"reflect"
	"testing"

	"tinygo.org/x/bluetooth"
)

func TestAppendLinesPartial(t *testing.T) {
	rest, lines := appendLines(nil, []byte("AT+CFG"))
	if len(lines) != 0 {
		t.Fatalf("lines = %v, want none", lines)
	}
	if string(rest) != "AT+CFG" {
		t.Fatalf("rest = %q, want %q", rest, "AT+CFG")
	}
}

func TestAppendLinesTrimsCR(t *testing.T) {
	rest, lines := appendLines(nil, []byte("OK\r\n"))
	if !reflect.DeepEqual(lines, []string{"OK"}) {
		t.Fatalf("lines = %v, want [OK]", lines)
	}
	if len(rest) != 0 {
		t.Fatalf("rest = %q, want empty", rest)
	}
}

func TestAppendLinesMultiplePerChunk(t *testing.T) {
	_, lines := appendLines(nil, []byte("a\r\nb\nc\r\n"))
	want := []string{"a", "b", "c"}
	if !reflect.DeepEqual(lines, want) {
		t.Fatalf("lines = %v, want %v", lines, want)
	}
}

func TestAppendLinesReassemblesAcrossChunks(t *testing.T) {
	rest, lines := appendLines(nil, []byte("AT+"))
	if len(lines) != 0 {
		t.Fatalf("chunk 1 lines = %v, want none", lines)
	}
	rest, lines = appendLines(rest, []byte("VER\r\n"))
	if !reflect.DeepEqual(lines, []string{"AT+VER"}) {
		t.Fatalf("chunk 2 lines = %v, want [AT+VER]", lines)
	}
	if len(rest) != 0 {
		t.Fatalf("rest = %q, want empty", rest)
	}
}

func TestSelectProfileNordic(t *testing.T) {
	present := map[bluetooth.UUID]bool{nusRXUUID: true, nusTXUUID: true}
	w, n, name, ok := selectProfile(func(u bluetooth.UUID) bool { return present[u] })
	if !ok || name != "Nordic" || w != nusRXUUID || n != nusTXUUID {
		t.Fatalf("got (%v,%v,%q,%v), want Nordic RX/TX", w, n, name, ok)
	}
}

func TestSelectProfileHM10(t *testing.T) {
	present := map[bluetooth.UUID]bool{hmUARTData: true}
	w, n, name, ok := selectProfile(func(u bluetooth.UUID) bool { return present[u] })
	if !ok || name != "HM-10" || w != hmUARTData || n != hmUARTData {
		t.Fatalf("got (%v,%v,%q,%v), want HM-10 0xFFE1", w, n, name, ok)
	}
}

func TestSelectProfileNordicWinsWhenBothPresent(t *testing.T) {
	present := map[bluetooth.UUID]bool{nusRXUUID: true, nusTXUUID: true, hmUARTData: true}
	_, _, name, ok := selectProfile(func(u bluetooth.UUID) bool { return present[u] })
	if !ok || name != "Nordic" {
		t.Fatalf("name = %q ok = %v, want Nordic (first match wins)", name, ok)
	}
}

func TestSelectProfileNoneMatches(t *testing.T) {
	if _, _, _, ok := selectProfile(func(bluetooth.UUID) bool { return false }); ok {
		t.Fatal("ok = true, want false when no profile present")
	}
	// A half-present profile (write only) must not match.
	half := map[bluetooth.UUID]bool{nusRXUUID: true}
	if _, _, _, ok := selectProfile(func(u bluetooth.UUID) bool { return half[u] }); ok {
		t.Fatal("ok = true with only the write characteristic present, want false")
	}
}

func TestRecordAndSnapshot(t *testing.T) {
	c := New(nil)
	c.record(bluetooth.Address{}, "AA", "D20S-NB", -50)
	got := c.Snapshot()
	if len(got) != 1 {
		t.Fatalf("len = %d, want 1", len(got))
	}
	if got[0].Addr != "AA" || got[0].Name != "D20S-NB" || got[0].RSSI != -50 {
		t.Fatalf("device = %+v", got[0])
	}
}

func TestRecordEmptyNameKeepsKnownName(t *testing.T) {
	c := New(nil)
	c.record(bluetooth.Address{}, "AA", "D20S-NB", -50)
	c.record(bluetooth.Address{}, "AA", "", -40) // later advert without a name
	got := c.Snapshot()
	if len(got) != 1 {
		t.Fatalf("len = %d, want 1", len(got))
	}
	if got[0].Name != "D20S-NB" {
		t.Fatalf("name = %q, want kept D20S-NB", got[0].Name)
	}
	if got[0].RSSI != -40 {
		t.Fatalf("rssi = %d, want updated -40", got[0].RSSI)
	}
}

func TestClearDevices(t *testing.T) {
	c := New(nil)
	c.record(bluetooth.Address{}, "AA", "x", -50)
	c.ClearDevices()
	if got := c.Snapshot(); len(got) != 0 {
		t.Fatalf("after clear = %v, want empty", got)
	}
}
