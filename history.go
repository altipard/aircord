package main

import (
	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/widget"

	"github.com/altipard/aircord/internal/filter"
)

// filterCmds keeps commands matching the filter (see filter.Match for the
// regex/substring semantics).
func filterCmds(all []string, f string) []string {
	return filter.Match(all, f, func(c string) string { return c })
}

// pickerSearch is the filter entry inside the history popup. It forwards
// navigation/commit keys to the popup instead of the default editing behaviour:
// Up/Down move the selection, Enter runs it, Right inserts it into the command
// line for editing, Escape closes.
type pickerSearch struct {
	widget.Entry
	moveSel func(int)
	accept  func() // Enter: pick + execute
	insert  func() // Right: pick into the input, no execute
	cancel  func()
}

func newPickerSearch() *pickerSearch {
	e := &pickerSearch{}
	e.ExtendBaseWidget(e)
	return e
}

func (e *pickerSearch) TypedKey(key *fyne.KeyEvent) {
	switch key.Name {
	case fyne.KeyUp:
		if e.moveSel != nil {
			e.moveSel(-1)
		}
	case fyne.KeyDown:
		if e.moveSel != nil {
			e.moveSel(1)
		}
	case fyne.KeyReturn, fyne.KeyEnter:
		if e.accept != nil {
			e.accept()
		}
	case fyne.KeyRight:
		if e.insert != nil {
			e.insert()
		}
	case fyne.KeyEscape:
		if e.cancel != nil {
			e.cancel()
		}
	default:
		e.Entry.TypedKey(key)
	}
}

// historyEntry is a single-line command entry. Up opens a searchable history
// picker (fzf-style); executed commands are recorded via add(). onExec runs the
// current line when the user picks a history item with Enter.
type historyEntry struct {
	widget.Entry
	history []string
	onExec  func()
}

func newHistoryEntry() *historyEntry {
	e := &historyEntry{}
	e.ExtendBaseWidget(e)
	return e
}

func (e *historyEntry) add(cmd string) {
	if n := len(e.history); n == 0 || e.history[n-1] != cmd {
		e.history = append(e.history, cmd)
	}
}

// setHistory replaces the in-memory history, e.g. when connecting to a device
// with previously persisted commands.
func (e *historyEntry) setHistory(h []string) { e.history = h }

func (e *historyEntry) setAndCursorEnd(text string) {
	e.SetText(text)
	e.CursorColumn = len(text)
	e.Refresh()
}

func (e *historyEntry) TypedKey(key *fyne.KeyEvent) {
	if key.Name == fyne.KeyUp {
		e.openPicker()
		return
	}
	e.Entry.TypedKey(key)
}

// openPicker shows a searchable, keyboard-navigable list of prior commands
// anchored above the input.
func (e *historyEntry) openPicker() {
	if len(e.history) == 0 {
		return
	}
	drv := fyne.CurrentApp().Driver()
	cv := drv.CanvasForObject(e)
	if cv == nil {
		return
	}

	// newest-first snapshot
	all := filter.Reversed(e.history)

	items := all
	sel := 0
	programmatic := false

	list := widget.NewList(
		func() int { return len(items) },
		func() fyne.CanvasObject { return widget.NewLabel("") },
		func(i widget.ListItemID, o fyne.CanvasObject) {
			if i >= 0 && i < len(items) {
				o.(*widget.Label).SetText(items[i])
			}
		},
	)

	var popup *widget.PopUp
	closeP := func() {
		if popup != nil {
			popup.Hide()
		}
		cv.Focus(e)
	}
	// pickInto drops the selected command into the input line (no execute).
	pickInto := func() {
		if sel >= 0 && sel < len(items) {
			e.setAndCursorEnd(items[sel])
		}
		closeP()
	}
	// pickExec inserts the selected command and runs it immediately.
	pickExec := func() {
		pickInto()
		if e.onExec != nil {
			e.onExec()
		}
	}

	// selectProg highlights a row programmatically without triggering the
	// mouse-click path in OnSelected.
	selectProg := func(i int) {
		if i < 0 || i >= len(items) {
			return
		}
		programmatic = true
		list.Select(i)
		list.ScrollTo(i)
	}
	list.OnSelected = func(id widget.ListItemID) {
		sel = id
		if programmatic {
			programmatic = false
			return
		}
		pickInto() // real mouse click: fill the line, let the user run it
	}

	search := newPickerSearch()
	search.SetPlaceHolder("filter history — regex/substring")
	search.OnChanged = func(f string) {
		items = filterCmds(all, f)
		switch {
		case len(items) == 0:
			sel = -1 // no selection when nothing matches
		case sel >= len(items):
			sel = len(items) - 1
		case sel < 0:
			sel = 0
		}
		list.Refresh()
		if sel >= 0 {
			selectProg(sel)
		} else {
			list.UnselectAll()
		}
	}
	search.moveSel = func(d int) {
		if len(items) == 0 {
			return
		}
		sel += d
		if sel < 0 {
			sel = 0
		}
		if sel >= len(items) {
			sel = len(items) - 1
		}
		selectProg(sel)
	}
	search.accept = pickExec
	search.insert = pickInto
	search.cancel = closeP

	header := widget.NewLabel("History — ↑/↓ select · Enter run · → edit · Esc close")
	content := container.NewBorder(
		container.NewVBox(header, search),
		nil, nil, nil,
		list,
	)

	width := e.Size().Width
	if width < 320 {
		width = 320
	}
	height := float32(260)

	popup = widget.NewPopUp(content, cv)
	popup.Resize(fyne.NewSize(width, height))
	pos := drv.AbsolutePositionForObject(e)
	popup.ShowAtPosition(fyne.NewPos(pos.X, pos.Y-height))

	cv.Focus(search)
	selectProg(0)
}
