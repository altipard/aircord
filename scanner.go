package main

import (
	"sync"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/widget"

	"tinygo.org/x/bluetooth"

	"github.com/altipard/aircord/internal/ble"
	"github.com/altipard/aircord/internal/device"
	"github.com/altipard/aircord/internal/history"
	"github.com/altipard/aircord/internal/playbook"
	"github.com/altipard/aircord/internal/session"
)

// scanner owns the UI-thread state and widgets and drives a ble.Client for all
// Bluetooth work. It implements ble.Events (see connect.go) to receive BLE
// activity, marshalling each callback onto the UI thread.
type scanner struct {
	win       fyne.Window // parent for confirmation dialogs
	ble       *ble.Client
	hist      *history.Store
	templates *device.Set     // device control-panel templates (presets + user overrides)
	sess      *session.Store  // saved working sessions
	plays     *playbook.Store // saved command sequences

	// discovery view (UI-thread only)
	display []ble.Device
	filter  string
	total   int

	// device-table sort order, set by clicking a column header (UI-thread only)
	sortCol int
	sortAsc bool

	// selection (UI-thread only)
	hasSel  bool
	selAddr bluetooth.Address
	selName string
	selKey  string // d.Addr of the selected device; survives re-sorts (see reselect)

	// pending state after loading a session: the wanted device is re-selected and
	// the wanted template applied once a live scan / connect makes them real.
	wantAddr     string
	wantTemplate string

	// reselecting guards the programmatic re-selection during a repaint so the
	// table's OnSelected side effects don't fire on every refresh.
	reselecting bool

	// terminal buffer. termDirty marks a repaint already scheduled for the
	// current burst of lines (UI-thread only; see scheduleTermPaint).
	logMu     sync.Mutex
	logs      []string
	termDirty bool

	// widgets
	deviceTbl   *widget.Table
	filterEntry *widget.Entry
	statusLbl   *widget.Label
	alert       *alertLine    // errors and confirmations; see widgets.go
	emptyHint   *widget.Label // shown while the device list is empty
	scanBtn     *widget.Button
	connectBtn  *widget.Button
	disconnBtn  *widget.Button
	sendBtn     *widget.Button
	pinEntry    *widget.Entry
	cmdEntry    *historyEntry
	term        *readOnlyEntry // selectable, copyable, non-editable log pane

	// control panel (see panel.go)
	activeTmpl  *device.Template
	panelBox    *fyne.Container // holds the rendered action controls
	panelTitle  *widget.Label
	templateSel *widget.Select

	// pending value capture for read actions (UI-thread only): the next reply line
	// matching capMatch feeds capOnValue; capGen invalidates a stale timeout.
	capMatch   func(string) (string, bool)
	capOnValue func(string)
	capGen     int

	// lineTap, when set, receives every raw device reply line ("<< " payload) on
	// the BLE goroutine — the playbook runner uses it to wait for command
	// acknowledgements without going through the UI thread.
	tapMu   sync.Mutex
	lineTap func(line string)

	// firmware update (see ota_ui.go; UI-thread only). otaImage is the last
	// chosen .bin, otaIMEI the last node named, so a retry does not start from
	// an empty dialog. otaRunning keeps a second update — and a manual Connect —
	// off a link the updater owns.
	otaImage   []byte
	otaFile    string
	otaIMEI    string
	otaRunning bool
}

// setLineTap installs (or, with nil, removes) the reply-line tap.
func (s *scanner) setLineTap(tap func(line string)) {
	s.tapMu.Lock()
	s.lineTap = tap
	s.tapMu.Unlock()
}

func newScanner(win fyne.Window) *scanner {
	s := &scanner{
		win:       win,
		hist:      history.NewStore(),
		templates: device.Load(),
		sess:      session.NewStore(),
		plays:     playbook.NewStore(),
		// Strongest signal first: during a scan the device in the user's hand is
		// usually the closest one.
		sortCol: colRSSI,
		sortAsc: false,
	}
	// Shipped default: full 1NCE-OS provisioning for Dragino D2x-NB sensors.
	// Named device+provider because a different SIM means a different playbook
	// (APN and server change). Every set-command waits for its OK so a busy
	// console can't swallow steps; the final ATZ reboots into the new config
	// and answers nothing.
	s.plays.EnsureDefault(playbook.Playbook{
		Name: "dragino-nb-1nce",
		Steps: playbook.ParseSteps(
			"AT+QBAND=2,8,20 ?OK\n" +
				"AT+APN=iot.1nce.net ?OK\n" +
				"AT+SERVADDR=udp.os.1nce.com,4445 ?OK\n" +
				"AT+PRO=2,5 ?OK\n" +
				"AT+TDC=900 ?OK\n" + // 15 min uplink interval; change it in the playbook editor
				"AT+CFG ?OK\n" +
				"ATZ"),
	})
	s.ble = ble.New(s)
	return s
}
