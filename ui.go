package main

import (
	"fmt"
	"image/color"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/canvas"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/widget"
)

// buildUI constructs every widget, wires callbacks, and returns the root
// content. Widget references are stored on the scanner so the behaviour methods
// (scan/connect/terminal) can update them.
func (s *scanner) buildUI() fyne.CanvasObject {
	s.statusLbl = widget.NewLabel("idle")
	s.alert = newAlertLine()
	s.buildDeviceTable()

	top := container.NewBorder(
		container.NewVBox(s.buildScanRow(), s.buildFilter(), s.statusLbl, s.alert),
		nil, nil, nil,
		s.tableBox(),
	)
	panelTerm := container.NewVSplit(s.buildPanel(), s.buildTerminal())
	panelTerm.Offset = 0.4
	bottom := container.NewBorder(
		s.buildConnRow(),
		s.buildCmdRow(),
		nil, nil,
		panelTerm,
	)

	split := container.NewVSplit(top, bottom)
	split.Offset = 0.45

	s.updateEmptyHint()

	// The terminal exists now, so surface a corrupt-history load (the bad file
	// was backed up as .corrupt and an empty history started).
	if s.hist != nil {
		if err := s.hist.LoadErr(); err != nil {
			s.fail("history load failed, backed up as .corrupt: " + err.Error())
		}
	}
	for _, err := range s.templates.LoadErrs() {
		s.fail("template load skipped: " + err.Error())
	}
	return split
}

// buildDeviceTable builds the Name | RSSI | Address table with a header row and
// zebra striping.
func (s *scanner) buildDeviceTable() {
	zebra := color.NRGBA{R: 255, G: 255, B: 255, A: 20} // subtle stripe on dark bg
	s.deviceTbl = widget.NewTable(
		func() (int, int) { return len(s.display), 3 },
		func() fyne.CanvasObject {
			bg := canvas.NewRectangle(color.Transparent)
			l := widget.NewLabel("")
			l.Truncation = fyne.TextTruncateEllipsis
			return container.NewStack(bg, l)
		},
		func(id widget.TableCellID, o fyne.CanvasObject) {
			c := o.(*fyne.Container)
			bg := c.Objects[0].(*canvas.Rectangle)
			l := c.Objects[1].(*widget.Label)
			if id.Row < 0 || id.Row >= len(s.display) {
				l.SetText("")
				bg.FillColor = color.Transparent
				bg.Refresh()
				return
			}
			if id.Row%2 == 1 {
				bg.FillColor = zebra
			} else {
				bg.FillColor = color.Transparent
			}
			bg.Refresh()
			d := s.display[id.Row]
			switch id.Col {
			case 0:
				name := d.Name
				if name == "" {
					name = "(no name)"
				}
				l.SetText(name)
			case 1:
				l.SetText(fmt.Sprintf("%d", d.RSSI))
			case 2:
				l.SetText(d.Addr)
			}
		},
	)
	s.deviceTbl.ShowHeaderRow = true
	// Headers are buttons so the table can be re-sorted by clicking one. Low
	// importance keeps them flat, so they still read as headers rather than as
	// three controls sitting above the list.
	s.deviceTbl.CreateHeader = func() fyne.CanvasObject {
		b := widget.NewButton("", nil)
		b.Importance = widget.LowImportance
		b.Alignment = widget.ButtonAlignLeading
		return b
	}
	s.deviceTbl.UpdateHeader = func(id widget.TableCellID, o fyne.CanvasObject) {
		b := o.(*widget.Button)
		col := id.Col
		b.OnTapped = func() { s.sortByColumn(col) }
		b.SetText(s.headerTitle(col))
	}
	s.deviceTbl.OnSelected = func(id widget.TableCellID) {
		// Programmatic re-selection during a repaint only restores the highlight;
		// the selection state below is already set from the user's real click.
		if s.reselecting {
			return
		}
		if id.Row < 0 || id.Row >= len(s.display) {
			return
		}
		d := s.display[id.Row]
		s.hasSel = true
		s.selAddr = d.Address
		s.selName = d.Name
		s.selKey = d.Addr
		if s.selName == "" {
			s.selName = d.Addr
		}
		s.setStatus("selected: " + s.selName)
		if !s.ble.IsConnected() {
			s.connectBtn.Enable()
		}
	}
}

// tableBox stacks an empty-state hint behind the device table, so a user facing
// a blank list is told what to do instead of wondering whether the app works.
// The table's columns always fill the window width.
func (s *scanner) tableBox() fyne.CanvasObject {
	s.emptyHint = widget.NewLabel(
		"No devices yet.\n\nPress Scan to discover nearby devices.\nmacOS asks for Bluetooth permission on the first scan.")
	s.emptyHint.Alignment = fyne.TextAlignCenter
	// Hint last, so it draws on top: behind the table its column separators run
	// straight through the text.
	return container.NewStack(
		container.New(
			&tableFill{tbl: s.deviceTbl, weights: []float32{0.42, 0.16, 0.42}},
			s.deviceTbl,
		),
		container.NewCenter(s.emptyHint),
	)
}

// updateEmptyHint shows the placeholder only while the list is genuinely empty.
// UI-thread only.
func (s *scanner) updateEmptyHint() {
	if s.emptyHint == nil {
		return
	}
	if len(s.display) == 0 {
		s.emptyHint.Show()
	} else {
		s.emptyHint.Hide()
	}
}

func (s *scanner) buildScanRow() fyne.CanvasObject {
	s.scanBtn = widget.NewButton("Scan", s.toggleScan)
	s.scanBtn.Importance = widget.HighImportance
	right := container.NewHBox(
		widget.NewButton("Sessions", s.openSessions),
		widget.NewButton("Playbooks", s.openPlaybooks),
		widget.NewButton("Firmware", s.openFirmware),
		widget.NewButton("Clear list", s.clearList),
	)
	// Scan goes in the left slot, not the centre. A Border's centre stretches to
	// fill, which made the primary button span half the row while its neighbours
	// kept their natural width.
	return container.NewBorder(nil, nil, s.scanBtn, right, nil)
}

func (s *scanner) buildFilter() fyne.CanvasObject {
	s.filterEntry = widget.NewEntry()
	s.filterEntry.SetPlaceHolder("filter devices — regex or substring (e.g. 8606, ^D20, (?i)nb)")
	s.filterEntry.OnChanged = func(t string) {
		s.filter = t
		s.deviceTbl.UnselectAll()
		s.snapshot()
		s.deviceTbl.Refresh()
		s.setStatus(fmt.Sprintf("%d/%d shown", len(s.display), s.total))
	}
	return s.filterEntry
}

func (s *scanner) buildConnRow() fyne.CanvasObject {
	s.connectBtn = widget.NewButton("Connect", s.connect)
	s.connectBtn.Importance = widget.HighImportance
	s.connectBtn.Disable()
	s.disconnBtn = widget.NewButton("Disconnect", s.disconnect)
	s.disconnBtn.Disable()
	// A password entry: the PIN is a shared secret for the device, and this
	// window gets screen-shared and photographed during field work.
	s.pinEntry = widget.NewPasswordEntry()
	s.pinEntry.SetPlaceHolder("PIN (optional, sent on connect)")
	return container.NewBorder(nil, nil,
		container.NewHBox(s.connectBtn, s.disconnBtn, widget.NewLabel("PIN")),
		nil,
		s.pinEntry,
	)
}

func (s *scanner) buildCmdRow() fyne.CanvasObject {
	s.cmdEntry = newHistoryEntry()
	s.cmdEntry.SetPlaceHolder("AT command (e.g. AT+CFG) — Enter to send, ↑ history")
	s.cmdEntry.OnSubmitted = func(string) { s.sendFromEntry() }
	s.cmdEntry.onExec = s.sendFromEntry
	s.cmdEntry.Disable()
	s.sendBtn = widget.NewButton("Send", s.sendFromEntry)
	s.sendBtn.Importance = widget.HighImportance
	s.sendBtn.Disable()
	return container.NewBorder(nil, nil, nil, s.sendBtn, s.cmdEntry)
}

// buildTerminal is the monospace log pane. It uses a read-only multi-line Entry
// so text can still be selected with the mouse and copied (Cmd+C) while typing
// into the transcript is not possible; long lines scroll horizontally rather
// than wrap.
func (s *scanner) buildTerminal() fyne.CanvasObject {
	s.term = newReadOnlyEntry()
	termButtons := container.NewHBox(
		widget.NewButton("Copy", s.copyLog),
		widget.NewButton("Save", s.saveLog),
		widget.NewButton("Clear log", s.clearLog),
	)
	termHeader := container.NewBorder(nil, nil, widget.NewLabel("Terminal"), termButtons)
	return container.NewBorder(termHeader, nil, nil, nil, s.term)
}
