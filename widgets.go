package main

import (
	"image/color"
	"time"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/canvas"
	"fyne.io/fyne/v2/theme"
	"fyne.io/fyne/v2/widget"
)

// okAlertTTL is how long a success message stays before clearing itself.
// Failures never auto-clear: the user must be able to look away, look back, and
// still see what went wrong.
const okAlertTTL = 6 * time.Second

// alertLine is a one-line, severity-coloured message area. It exists because a
// plain status label is the wrong channel for errors here: the scan repaint
// rewrites the status twice a second, so anything important posted there is gone
// before it is read. Alerts live on their own line and stay put.
//
// One is shown in the main window; each modal carries its own, so a failure
// caused inside a dialog is reported inside that dialog rather than behind it.
// UI-thread only.
type alertLine struct {
	*canvas.Text
	gen int // invalidates a pending auto-clear when a newer message arrives
}

func newAlertLine() *alertLine {
	t := canvas.NewText("", theme.Color(theme.ColorNameForeground))
	t.TextSize = theme.TextSize()
	return &alertLine{Text: t}
}

// fail shows a sticky error.
func (a *alertLine) fail(msg string) {
	a.set(msg, theme.Color(theme.ColorNameError), 0)
}

// warn shows a sticky caution.
func (a *alertLine) warn(msg string) {
	a.set(msg, theme.Color(theme.ColorNameWarning), 0)
}

// ok shows a success confirmation that clears itself.
func (a *alertLine) ok(msg string) {
	a.set(msg, theme.Color(theme.ColorNameSuccess), okAlertTTL)
}

func (a *alertLine) clear() { a.set("", theme.Color(theme.ColorNameForeground), 0) }

// set replaces the message. A non-zero ttl schedules a clear that a later
// message cancels via the generation counter.
func (a *alertLine) set(msg string, c color.Color, ttl time.Duration) {
	a.gen++
	gen := a.gen
	a.Text.Text = msg
	a.Color = c
	a.Refresh()
	if ttl <= 0 {
		return
	}
	time.AfterFunc(ttl, func() {
		fyne.Do(func() {
			if a.gen == gen {
				a.clear()
			}
		})
	})
}

// forceDark wraps the default theme but pins the dark variant, giving the whole
// app a terminal look regardless of the OS appearance setting.
type forceDark struct{}

func (forceDark) Color(n fyne.ThemeColorName, _ fyne.ThemeVariant) color.Color {
	return theme.DefaultTheme().Color(n, theme.VariantDark)
}
func (forceDark) Font(s fyne.TextStyle) fyne.Resource     { return theme.DefaultTheme().Font(s) }
func (forceDark) Icon(n fyne.ThemeIconName) fyne.Resource { return theme.DefaultTheme().Icon(n) }
func (forceDark) Size(n fyne.ThemeSizeName) float32       { return theme.DefaultTheme().Size(n) }

// readOnlyEntry is a multi-line Entry that cannot be edited but stays fully
// selectable and copyable. Fyne 2.7 offers no such mode — Disable() greys the
// widget out and kills selection — so this swallows the input events instead.
// The terminal needs it: a transcript the user can accidentally type into is a
// transcript they can no longer trust.
type readOnlyEntry struct {
	widget.Entry
}

func newReadOnlyEntry() *readOnlyEntry {
	e := &readOnlyEntry{}
	e.MultiLine = true
	e.Wrapping = fyne.TextWrapOff
	e.TextStyle = fyne.TextStyle{Monospace: true}
	e.ExtendBaseWidget(e)
	return e
}

// TypedRune drops every printable character.
func (e *readOnlyEntry) TypedRune(rune) {}

// TypedKey passes through caret movement and selection keys only.
func (e *readOnlyEntry) TypedKey(k *fyne.KeyEvent) {
	switch k.Name {
	case fyne.KeyUp, fyne.KeyDown, fyne.KeyLeft, fyne.KeyRight,
		fyne.KeyHome, fyne.KeyEnd, fyne.KeyPageUp, fyne.KeyPageDown:
		e.Entry.TypedKey(k)
	}
}

// TypedShortcut blocks the mutating clipboard shortcuts; copy and select-all
// still work.
func (e *readOnlyEntry) TypedShortcut(sh fyne.Shortcut) {
	switch sh.(type) {
	case *fyne.ShortcutPaste, *fyne.ShortcutCut:
		return
	}
	e.Entry.TypedShortcut(sh)
}

// tableFill lays out a single widget.Table to fill the available space and
// distributes its column widths by weight, so the columns always span the full
// window width (no dead gap, no horizontal scrollbar).
type tableFill struct {
	tbl     *widget.Table
	weights []float32
}

func (t *tableFill) MinSize(_ []fyne.CanvasObject) fyne.Size { return fyne.NewSize(380, 120) }

func (t *tableFill) Layout(objs []fyne.CanvasObject, size fyne.Size) {
	for _, o := range objs {
		o.Move(fyne.NewPos(0, 0))
		o.Resize(size)
	}
	var sum float32
	for _, w := range t.weights {
		sum += w
	}
	// Leave a hair for inter-column separators so we never overflow into a
	// horizontal scrollbar.
	avail := size.Width - float32(len(t.weights))
	if avail < 0 {
		avail = 0
	}
	for i, w := range t.weights {
		t.tbl.SetColumnWidth(i, avail*w/sum)
	}
}
