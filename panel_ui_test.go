package main

import (
	"testing"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/test"
	"fyne.io/fyne/v2/widget"

	"github.com/altipard/aircord/internal/device"
	"github.com/altipard/aircord/internal/playbook"
)

// dialogChrome is roughly the vertical space a dialog spends on its own title
// bar, dismiss button and padding.
const dialogChrome float32 = 120

// minListHeight is the height below which the saved-playbook list is there but
// unusable — a couple of rows have to be visible without scrolling.
const minListHeight float32 = 150

// findScroll returns the first scroll container in the tree, which in the
// playbook dialog is the saved-playbook list.
func findScroll(o fyne.CanvasObject) *container.Scroll {
	switch v := o.(type) {
	case *container.Scroll:
		return v
	case *container.AppTabs:
		for _, it := range v.Items {
			if s := findScroll(it.Content); s != nil {
				return s
			}
		}
	case *container.Split:
		for _, c := range []fyne.CanvasObject{v.Leading, v.Trailing} {
			if s := findScroll(c); s != nil {
				return s
			}
		}
	case *fyne.Container:
		for _, c := range v.Objects {
			if s := findScroll(c); s != nil {
				return s
			}
		}
	}
	return nil
}

// layOutInDialog renders content at the size a dialog would give it and returns
// the height the saved-playbook list actually received.
func layOutInDialog(content fyne.CanvasObject) float32 {
	w := test.NewWindow(content)
	defer w.Close()
	w.Resize(fyne.NewSize(modalSize.Width, modalSize.Height-dialogChrome))
	sc := findScroll(content)
	if sc == nil {
		return 0
	}
	return sc.Size().Height
}

// TestPlaybookListGetsUsableHeight guards the regression that made saved
// playbooks invisible.
//
// The content used to be a VSplit of editor over list. A split cannot shrink a
// child below its minimum, and the editor half — name field, wrapped syntax
// help, a six-row text area and a button — demanded nearly the whole dialog.
// The list was allotted the remainder, some thirty pixels: present, laid out,
// and impossible to see. Note that the total minimum still fit the dialog, so
// asserting on MinSize would have caught nothing; the allotted height is the
// property that matters.
func TestPlaybookListGetsUsableHeight(t *testing.T) {
	test.NewApp()

	st := playbook.NewStoreAt(t.TempDir())
	for _, name := range []string{"alpha", "beta", "gamma"} {
		if err := st.Save(playbook.Playbook{
			Name:  name,
			Steps: playbook.ParseSteps("AT+CFG ?OK\nATZ"),
		}); err != nil {
			t.Fatalf("seed %s: %v", name, err)
		}
	}

	s := &scanner{plays: st}
	if got := layOutInDialog(s.playbookBody(func() {})); got < minListHeight {
		t.Errorf("saved-playbook list got %.0f px of height, want at least %.0f", got, minListHeight)
	}
}

// TestPlaybookBodyFitsWhenEmpty covers the first-run case, where the list holds
// a single hint line instead of rows.
func TestPlaybookBodyFitsWhenEmpty(t *testing.T) {
	test.NewApp()

	s := &scanner{plays: playbook.NewStoreAt(t.TempDir())}
	if got := layOutInDialog(s.playbookBody(func() {})); got < minListHeight {
		t.Errorf("empty list got %.0f px of height, want at least %.0f", got, minListHeight)
	}
}

// TestRenderActionReturnsLabelAndControl checks that every action type produces
// the label/control pair the panel's form layout consumes. A nil in either slot
// would silently misalign every row below it.
func TestRenderActionReturnsLabelAndControl(t *testing.T) {
	test.NewApp()
	s := &scanner{}

	actions := map[string]device.Action{
		"button": {Label: "Show config", Cmd: "AT+CFG", Type: device.ActionButton},
		"input":  {Label: "Interval", Cmd: "AT+TDC=<s>", Type: device.ActionInput},
		"select": {Label: "Band", Type: device.ActionSelect,
			Options: []device.Option{{Text: "B8", Cmd: "AT+QBAND=1,8"}}},
		"read": {Label: "Firmware", Cmd: "AT+VER", Type: device.ActionRead, Parse: `(\d+)`},
		"junk": {Label: "Unknown", Type: device.ActionType("nonsense")},
	}
	for name, a := range actions {
		label, control := s.renderAction(a)
		if label == nil || control == nil {
			t.Errorf("%s: renderAction returned (%v, %v), want both non-nil", name, label, control)
		}
	}
}

// TestDestructiveActionsLookDestructive ties the confirm flag to the control's
// appearance, so a dangerous action reads as dangerous before it is clicked and
// not only in the confirmation dialog.
func TestDestructiveActionsLookDestructive(t *testing.T) {
	if got := actionImportance(device.Action{Confirm: true}); got != widget.DangerImportance {
		t.Errorf("confirm action importance = %v, want danger", got)
	}
	if got := actionImportance(device.Action{}); got == widget.DangerImportance {
		t.Error("ordinary action must not be styled as destructive")
	}
}
