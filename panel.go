package main

import (
	"time"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/dialog"
	"fyne.io/fyne/v2/layout"
	"fyne.io/fyne/v2/widget"

	"github.com/altipard/aircord/internal/device"
)

const noTemplate = "— no template —"

// readTimeout bounds how long a read action waits for the value it asked for.
// It matches the playbook runner's reply wait, so "how long before this gives
// up" means the same thing everywhere in the app.
const readTimeout = waitTimeout

// buildPanel is the device control panel: a header with the active template name
// and a manual template selector, above a scrollable column of action controls.
// It stays empty until a device is connected (see applyTemplateFor).
func (s *scanner) buildPanel() fyne.CanvasObject {
	// A form layout, not a VBox: it aligns every action label in one column and
	// gives every control the same width in the other. A VBox stretched each
	// control to the full panel width, so the panel's button sizes swung with
	// the label text.
	s.panelBox = container.New(layout.NewFormLayout())
	s.panelTitle = widget.NewLabel("Control panel — not connected")
	s.panelTitle.TextStyle = fyne.TextStyle{Bold: true}

	s.templateSel = widget.NewSelect(nil, s.onTemplatePick)
	s.templateSel.PlaceHolder = "Template…"
	s.templateSel.Disable()

	header := container.NewBorder(nil, nil, s.panelTitle, s.templateSel)
	return container.NewBorder(header, nil, nil, nil, container.NewVScroll(s.panelBox))
}

// applyTemplateFor runs on connect: it fills the selector with every known
// template, auto-suggests one from the device name, and renders it. The user can
// override the choice via the selector. UI-thread only.
func (s *scanner) applyTemplateFor(deviceName string) {
	s.templateSel.Options = append([]string{noTemplate}, s.templates.Names()...)
	s.templateSel.Enable()
	// A loaded session's template wins over the name-based guess; otherwise
	// auto-suggest from the device name.
	pick := noTemplate
	if s.wantTemplate != "" && s.templates.Get(s.wantTemplate) != nil {
		pick = s.wantTemplate
	} else if sug := s.templates.Suggest(deviceName); sug != nil {
		pick = sug.Name
	}
	s.wantTemplate = ""
	s.templateSel.SetSelected(pick) // fires onTemplatePick -> showTemplate
}

// clearPanel resets the panel to the disconnected state. UI-thread only.
func (s *scanner) clearPanel() {
	s.templateSel.Disable()
	s.showTemplate(nil)
	s.panelTitle.SetText("Control panel — not connected")
}

func (s *scanner) onTemplatePick(name string) {
	if name == "" || name == noTemplate {
		s.showTemplate(nil)
		return
	}
	s.showTemplate(s.templates.Get(name))
}

// showTemplate renders a template's actions into the panel, replacing whatever
// was there. A nil template empties it. UI-thread only.
func (s *scanner) showTemplate(t *device.Template) {
	s.activeTmpl = t
	s.panelBox.RemoveAll()
	if t == nil {
		s.panelTitle.SetText("Control panel — no template")
		s.panelBox.Refresh()
		return
	}
	s.panelTitle.SetText("Panel: " + t.Name)
	for _, a := range t.Actions {
		label, control := s.renderAction(a)
		s.panelBox.Add(label)
		s.panelBox.Add(control)
	}
	s.panelBox.Refresh()
}

// sendAction dispatches an action's command, asking first when the template
// marked the action as destructive. A control panel is a column of similar
// buttons; without this, one misplaced click can reboot or wipe a sensor that is
// already mounted in the field. UI-thread only.
func (s *scanner) sendAction(a device.Action, cmd string) {
	if !a.Confirm {
		s.sendCmd(cmd)
		return
	}
	dialog.ShowConfirm(
		a.Label+"?",
		"This sends:\n\n"+cmd+"\n\nThe template marks this action as destructive.",
		func(ok bool) {
			if ok {
				s.sendCmd(cmd)
			}
		},
		s.win,
	)
}

// actionLabel is the left-hand column of a panel row: the action's name, so the
// control beside it can carry a short, uniform verb instead of a long caption.
func actionLabel(a device.Action) fyne.CanvasObject {
	return widget.NewLabel(a.Label)
}

// actionImportance colours a control by consequence: a destructive action reads
// as destructive before it is clicked, not only in the confirmation dialog.
func actionImportance(a device.Action) widget.Importance {
	if a.Confirm {
		return widget.DangerImportance
	}
	return widget.MediumImportance
}

// renderAction turns one template action into a label/control pair for the
// panel's form layout. UI-thread only.
func (s *scanner) renderAction(a device.Action) (fyne.CanvasObject, fyne.CanvasObject) {
	switch a.Type {
	case device.ActionButton:
		btn := widget.NewButton("Send", func() { s.sendAction(a, a.Cmd) })
		btn.Importance = actionImportance(a)
		return actionLabel(a), btn

	case device.ActionInput:
		entry := widget.NewEntry()
		entry.SetPlaceHolder("value")
		send := func() { s.sendAction(a, a.Render(entry.Text)) }
		entry.OnSubmitted = func(string) { send() }
		btn := widget.NewButton("Set", send)
		btn.Importance = actionImportance(a)
		return actionLabel(a), container.NewBorder(nil, nil, nil, btn, entry)

	case device.ActionSelect:
		opts := a.Options
		texts := make([]string, len(opts))
		for i, o := range opts {
			texts[i] = o.Text
		}
		sel := widget.NewSelect(texts, func(chosen string) {
			for _, o := range opts {
				if o.Text == chosen {
					s.sendAction(a, o.Cmd)
					return
				}
			}
		})
		sel.PlaceHolder = "choose…"
		return actionLabel(a), sel

	case device.ActionRead:
		act := a
		val := widget.NewLabel("—")
		btn := widget.NewButton("Read", func() {
			val.SetText("…")
			s.armCapture(act.Extract, val.SetText)
			s.sendAction(act, act.Cmd)
		})
		btn.Importance = actionImportance(a)
		return actionLabel(a), container.NewBorder(nil, nil, nil, btn, val)
	}
	return actionLabel(a), widget.NewLabel("(unknown action type)")
}

// armCapture waits for the next reply line that match extracts a value from and
// feeds it to onValue. A timeout disarms and reports no reply, so a missed
// answer can't hijack a later unrelated line. UI-thread only.
func (s *scanner) armCapture(match func(string) (string, bool), onValue func(string)) {
	s.capGen++
	gen := s.capGen
	s.capMatch = match
	s.capOnValue = onValue
	time.AfterFunc(readTimeout, func() {
		fyne.Do(func() {
			if s.capGen == gen && s.capMatch != nil {
				s.capMatch = nil
				s.capOnValue = nil
				onValue("(no reply)")
			}
		})
	})
}

// tryCapture is called for every reply line (from Log). On the first match it
// delivers the value and disarms. UI-thread only.
func (s *scanner) tryCapture(line string) {
	if s.capMatch == nil {
		return
	}
	val, ok := s.capMatch(line)
	if !ok {
		return
	}
	onValue := s.capOnValue
	s.capGen++ // invalidate the pending timeout
	s.capMatch = nil
	s.capOnValue = nil
	if onValue != nil {
		onValue(val)
	}
}
