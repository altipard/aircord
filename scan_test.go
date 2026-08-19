package main

import (
	"testing"

	"github.com/altipard/aircord/internal/ble"
)

func TestIndexOfAddr(t *testing.T) {
	devs := []ble.Device{
		{Addr: "AA", Name: "first"},
		{Addr: "BB", Name: "second"},
		{Addr: "CC", Name: "third"},
	}
	cases := []struct {
		addr string
		want int
	}{
		{"AA", 0},
		{"BB", 1},
		{"CC", 2},
		{"ZZ", -1}, // not present
		{"", -1},   // empty key never matches a real device
	}
	for _, c := range cases {
		if got := indexOfAddr(devs, c.addr); got != c.want {
			t.Errorf("indexOfAddr(%q) = %d, want %d", c.addr, got, c.want)
		}
	}
}

func TestIndexOfAddrEmptySlice(t *testing.T) {
	if got := indexOfAddr(nil, "AA"); got != -1 {
		t.Errorf("indexOfAddr(nil) = %d, want -1", got)
	}
}

// addrs renders a sorted slice as its address order, which is what the
// assertions below compare.
func addrs(devs []ble.Device) []string {
	out := make([]string, len(devs))
	for i, d := range devs {
		out[i] = d.Addr
	}
	return out
}

func sample() []ble.Device {
	return []ble.Device{
		{Addr: "CC", Name: "beta", RSSI: -70},
		{Addr: "AA", Name: "Alpha", RSSI: -50},
		{Addr: "DD", Name: "", RSSI: -40}, // unnamed, but the strongest signal
		{Addr: "BB", Name: "alpha", RSSI: -70},
	}
}

func TestSortDevices(t *testing.T) {
	cases := []struct {
		name string
		col  int
		asc  bool
		want []string
	}{
		{"rssi strongest first", colRSSI, false, []string{"DD", "AA", "BB", "CC"}},
		{"rssi weakest first", colRSSI, true, []string{"BB", "CC", "AA", "DD"}},
		{"name a-z", colName, true, []string{"AA", "BB", "CC", "DD"}},
		// "Alpha" and "alpha" tie, and the tiebreak stays ascending by address
		// in both directions — so AA leads BB here too, and toggling the header
		// does not reorder rows that compare equal.
		{"name z-a", colName, false, []string{"CC", "AA", "BB", "DD"}},
		{"address a-z", colAddr, true, []string{"AA", "BB", "CC", "DD"}},
		{"address z-a", colAddr, false, []string{"DD", "CC", "BB", "AA"}},
	}
	for _, c := range cases {
		devs := sample()
		sortDevices(devs, c.col, c.asc)
		got := addrs(devs)
		for i := range c.want {
			if got[i] != c.want[i] {
				t.Errorf("%s: order = %v, want %v", c.name, got, c.want)
				break
			}
		}
	}
}

// Unnamed devices carry no information for a name sort, and a field scan turns
// up dozens of them — they must not lead the list in either direction.
func TestSortByNameKeepsUnnamedLast(t *testing.T) {
	for _, asc := range []bool{true, false} {
		devs := sample()
		sortDevices(devs, colName, asc)
		if last := devs[len(devs)-1]; last.Name != "" {
			t.Errorf("asc=%v: last row is %q, want the unnamed device", asc, last.Name)
		}
	}
}

// Equal keys must resolve to a fixed order, or the twice-a-second repaint would
// shuffle rows under the pointer.
func TestSortTiebreakIsStable(t *testing.T) {
	devs := sample() // BB and CC both have RSSI -70
	sortDevices(devs, colRSSI, false)
	first := addrs(devs)
	for i := 0; i < 5; i++ {
		devs = sample()
		sortDevices(devs, colRSSI, false)
		for j := range first {
			if addrs(devs)[j] != first[j] {
				t.Fatalf("order changed between runs: %v then %v", first, addrs(devs))
			}
		}
	}
	// The tiebreak is the address, ascending.
	sortDevices(devs, colRSSI, false)
	if got := addrs(devs); got[2] != "BB" || got[3] != "CC" {
		t.Errorf("tied rows = %v, want BB before CC", got[2:])
	}
}

func TestNextSort(t *testing.T) {
	cases := []struct {
		name          string
		curCol        int
		curAsc        bool
		click         int
		wantCol       int
		wantAscending bool
	}{
		{"same column flips", colRSSI, false, colRSSI, colRSSI, true},
		{"flips back", colRSSI, true, colRSSI, colRSSI, false},
		{"name starts ascending", colRSSI, false, colName, colName, true},
		{"address starts ascending", colName, true, colAddr, colAddr, true},
		{"rssi starts descending", colName, true, colRSSI, colRSSI, false},
	}
	for _, c := range cases {
		col, asc := nextSort(c.curCol, c.curAsc, c.click)
		if col != c.wantCol || asc != c.wantAscending {
			t.Errorf("%s: nextSort = (%d, %v), want (%d, %v)", c.name, col, asc, c.wantCol, c.wantAscending)
		}
	}
}

func TestHeaderTitleMarksTheActiveColumn(t *testing.T) {
	s := &scanner{sortCol: colRSSI, sortAsc: false}
	if got := s.headerTitle(colRSSI); got != "RSSI  ▼" {
		t.Errorf("active descending header = %q, want the down arrow", got)
	}
	s.sortAsc = true
	if got := s.headerTitle(colRSSI); got != "RSSI  ▲" {
		t.Errorf("active ascending header = %q, want the up arrow", got)
	}
	if got := s.headerTitle(colName); got != "Name" {
		t.Errorf("inactive header = %q, want no arrow", got)
	}
}
