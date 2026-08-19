// Aircord — a desktop GUI to scan, connect and talk to BLE devices that expose
// a serial-over-BLE interface: NB-IoT and LoRaWAN modems, sensors, and any
// peripheral speaking AT commands over a Nordic UART or HM-10 GATT profile.
//
// Stack:
//   - BLE:  tinygo.org/x/bluetooth  (CoreBluetooth on macOS)
//   - GUI:  fyne.io/fyne/v2
//
// Flow:
//  1. "Scan" discovers advertising devices (name / RSSI / address).
//  2. Select a device, hit "Connect" — opens the device's serial-over-BLE
//     service (Nordic UART or HM-10), auto-sends the PIN, and streams the
//     device's notify output into the terminal pane.
//  3. Type AT commands and press Enter (or "Send"); replies stream back.
//     Up opens a searchable history picker.
//
// The code is split by responsibility:
//   - internal/ble  BLE adapter, discovery, connection + AT I/O (UI-agnostic)
//   - scanner.go    UI state + widgets; wires the ble.Client
//   - scan.go       scan controls + device-table snapshot/filter
//   - connect.go    connect/disconnect flow + ble.Events handling
//   - terminal.go   terminal log + status line
//   - ui.go         widget construction + layout
//   - widgets.go    reusable UI pieces (dark theme, table-fill layout)
//   - history.go    fzf-style command history picker
package main

import (
	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/app"

	"gitlab.com/zilicon-it-services/petriheil/aircord/internal/config"
)

func main() {
	// Carry a pre-rename config directory over before any store reads it.
	config.Migrate()

	a := app.New()
	a.Settings().SetTheme(forceDark{})
	a.SetIcon(appIcon)

	w := a.NewWindow("Aircord")
	w.SetIcon(appIcon)
	s := newScanner(w)
	w.SetContent(s.buildUI())

	w.Resize(fyne.NewSize(900, 780))
	w.ShowAndRun()
}
