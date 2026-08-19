package main

import (
	"fmt"
	"sort"
	"strings"
	"time"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/widget"

	"github.com/altipard/aircord/internal/ble"
	"github.com/altipard/aircord/internal/filter"
)

// Device-table columns, in display order.
const (
	colName = iota
	colRSSI
	colAddr
)

// columnTitles are the header captions, indexed by column.
var columnTitles = [...]string{colName: "Name", colRSSI: "RSSI", colAddr: "Address"}

// compareDevices orders two devices by one column and direction.
//
// Two rules deliberately ignore the direction:
//
// Unnamed devices always sort last on a name sort. A field scan turns up dozens
// of them, and letting them lead would bury the devices actually being sought.
//
// Ties always break on the address, ascending. The list repaints twice a second
// and RSSI readings tie constantly, so an unstable order would shuffle rows
// under the pointer — and a tiebreak that flipped with the direction would
// reorder equal rows on every header click, which reads as noise.
func compareDevices(a, b ble.Device, col int, asc bool) int {
	if col == colName {
		switch {
		case a.Name == "" && b.Name != "":
			return 1
		case a.Name != "" && b.Name == "":
			return -1
		}
	}

	var c int
	switch col {
	case colName:
		c = strings.Compare(strings.ToLower(a.Name), strings.ToLower(b.Name))
	case colRSSI:
		switch {
		case a.RSSI < b.RSSI:
			c = -1
		case a.RSSI > b.RSSI:
			c = 1
		}
	default: // colAddr
		c = strings.Compare(a.Addr, b.Addr)
	}
	if !asc {
		c = -c
	}
	if c == 0 {
		c = strings.Compare(a.Addr, b.Addr)
	}
	return c
}

// sortDevices orders devs in place by column and direction.
func sortDevices(devs []ble.Device, col int, asc bool) {
	sort.Slice(devs, func(i, j int) bool {
		return compareDevices(devs[i], devs[j], col, asc) < 0
	})
}

// snapshot copies the client's discovered devices into the filtered, sorted
// display slice. Runs on the UI thread.
func (s *scanner) snapshot() {
	all := s.ble.Snapshot()
	s.total = len(all)
	out := filter.Match(all, s.filter, func(d ble.Device) string { return d.Name + " " + d.Addr })
	sortDevices(out, s.sortCol, s.sortAsc)
	s.display = out
	s.updateEmptyHint()
}

// headerTitle is the caption for a column header, marked with the sort arrow
// when that column is the active one.
func (s *scanner) headerTitle(col int) string {
	if col != s.sortCol {
		return columnTitles[col]
	}
	if s.sortAsc {
		return columnTitles[col] + "  ▲"
	}
	return columnTitles[col] + "  ▼"
}

// nextSort is the state transition for a header click: clicking the active
// column flips its direction, clicking another starts it in the order that is
// useful for that column — strongest signal first, but names and addresses A→Z.
func nextSort(curCol int, curAsc bool, clicked int) (col int, asc bool) {
	if curCol == clicked {
		return curCol, !curAsc
	}
	return clicked, clicked != colRSSI
}

// sortByColumn handles a header click. UI-thread only.
func (s *scanner) sortByColumn(col int) {
	s.sortCol, s.sortAsc = nextSort(s.sortCol, s.sortAsc, col)
	s.snapshot()
	s.deviceTbl.Refresh()
	s.reselect() // the selected device moved; keep the highlight on it
}

func (s *scanner) startScan() {
	if s.ble.IsScanning() {
		return
	}
	s.scanBtn.SetText("Stop")
	s.setStatus("scanning…")

	s.ble.Scan(func(err error) {
		fyne.Do(func() {
			s.fail("scan failed: " + err.Error())
			s.scanBtn.SetText("Scan")
		})
	})

	// Throttled repaint: refresh the table ~2x/sec instead of per advertisement.
	go func() {
		t := time.NewTicker(500 * time.Millisecond)
		defer t.Stop()
		for range t.C {
			if !s.ble.IsScanning() {
				return
			}
			fyne.Do(func() {
				s.snapshot()
				s.deviceTbl.Refresh()
				s.resolveWanted()
				s.reselect()
				s.setStatus(fmt.Sprintf("scanning… %d/%d shown", len(s.display), s.total))
			})
		}
	}()
}

func (s *scanner) stopScan() {
	if !s.ble.IsScanning() {
		return
	}
	s.ble.StopScan()
	s.scanBtn.SetText("Scan")
	s.snapshot()
	s.deviceTbl.Refresh()
	s.resolveWanted()
	s.reselect()
	s.setStatus(fmt.Sprintf("stopped — %d/%d shown", len(s.display), s.total))
}

func (s *scanner) toggleScan() {
	if s.ble.IsScanning() {
		s.stopScan()
	} else {
		s.startScan()
	}
}

func (s *scanner) clearList() {
	s.ble.ClearDevices()
	s.display = nil
	s.total = 0
	s.deviceTbl.Refresh()
	s.updateEmptyHint()
	s.setStatus("cleared")
}

// reselect keeps the table highlight on the selected device across the RSSI
// re-sort that each repaint performs. Without it the index-based selection would
// drift onto whatever device now occupies the old row. UI-thread only.
func (s *scanner) reselect() {
	if !s.hasSel {
		return
	}
	i := indexOfAddr(s.display, s.selKey)
	if i < 0 {
		// The selected device left the current view (filtered out or not seen in
		// this scan window): drop the stale highlight but keep the selection so it
		// re-appears if the device does.
		s.deviceTbl.UnselectAll()
		return
	}
	s.reselecting = true
	s.deviceTbl.Select(widget.TableCellID{Row: i, Col: 0})
	s.reselecting = false
}

// resolveWanted promotes a session's pending target to a real selection once a
// live scan rediscovers the device (a loaded snapshot has no usable BLE handle,
// so the wanted address is matched against freshly discovered devices). UI-thread
// only; runs alongside the repaint.
func (s *scanner) resolveWanted() {
	if s.wantAddr == "" {
		return
	}
	i := indexOfAddr(s.display, s.wantAddr)
	if i < 0 {
		return
	}
	d := s.display[i]
	s.hasSel = true
	s.selAddr = d.Address
	s.selName = d.Name
	if s.selName == "" {
		s.selName = d.Addr
	}
	s.selKey = d.Addr
	if !s.ble.IsConnected() {
		s.connectBtn.Enable()
	}
	s.wantAddr = ""
	s.logLine("-- session target found: " + s.selName + " — connect ready --")
}

// indexOfAddr returns the index of the device with address key addr in devs, or
// -1 when absent.
func indexOfAddr(devs []ble.Device, addr string) int {
	for i, d := range devs {
		if d.Addr == addr {
			return i
		}
	}
	return -1
}
