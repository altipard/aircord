package main

import (
	"strings"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/dialog"
	"fyne.io/fyne/v2/widget"

	"gitlab.com/zilicon-it-services/petriheil/aircord/internal/ble"
	"gitlab.com/zilicon-it-services/petriheil/aircord/internal/session"
)

// showModal presents a dialog built by build, which receives a close func so an
// action inside the dialog can dismiss it — starting a playbook, above all, has
// to get out of the way of the terminal it writes its progress to.
// UI-thread only.
func (s *scanner) showModal(title string, build func(close func()) fyne.CanvasObject) {
	var d dialog.Dialog
	body := build(func() {
		if d != nil {
			d.Hide()
		}
	})
	d = dialog.NewCustom(title, "Close", body, s.win)
	d.Resize(modalSize)
	d.Show()
}

// modalSize is the requested size for every dialog. Fyne treats it as a floor
// of the content's minimum size, so a dialog whose content demands more will
// grow past it — keep dialog content's minimum size below this.
var modalSize = fyne.NewSize(620, 520)

// confirm asks a yes/no question before an irreversible action.
func (s *scanner) confirm(title, message string, onYes func()) {
	dialog.ShowConfirm(title, message, func(ok bool) {
		if ok {
			onYes()
		}
	}, s.win)
}

// captureSession snapshots the current working state into a named session.
func (s *scanner) captureSession(name string) session.Session {
	devs := make([]session.DeviceSnapshot, len(s.display))
	for i, d := range s.display {
		devs[i] = session.DeviceSnapshot{Addr: d.Addr, Name: d.Name, RSSI: d.RSSI}
	}
	tmpl := ""
	if s.activeTmpl != nil {
		tmpl = s.activeTmpl.Name
	}
	return session.Session{
		Name:     name,
		SelAddr:  s.selKey,
		SelName:  s.selName,
		PIN:      s.pinEntry.Text,
		Template: tmpl,
		Devices:  devs,
	}
}

// applySession restores a saved session: the device list is repopulated from the
// (stale) snapshot for reference, the PIN is filled, and the selected device plus
// template become pending — they activate once a live scan / connect makes them
// real (see resolveWanted, applyTemplateFor). UI-thread only.
func (s *scanner) applySession(sess session.Session) {
	disp := make([]ble.Device, len(sess.Devices))
	for i, d := range sess.Devices {
		disp[i] = ble.Device{Addr: d.Addr, Name: d.Name, RSSI: d.RSSI}
	}
	s.display = disp
	s.total = len(disp)
	s.deviceTbl.UnselectAll()
	s.deviceTbl.Refresh()
	s.updateEmptyHint()

	s.pinEntry.SetText(sess.PIN)
	s.hasSel = false // a stale snapshot can't connect; wait for live rediscovery
	s.wantAddr = sess.SelAddr
	s.wantTemplate = sess.Template

	msg := "session loaded: " + sess.Name
	if sess.SelName != "" {
		msg += " — target " + sess.SelName + " (press Scan to reconnect)"
	}
	s.setStatus(msg)
	s.ok(msg)
	s.logLine("-- " + msg + " --")
}

// openSessions shows the save/load/delete dialog for named sessions.
func (s *scanner) openSessions() {
	s.showModal("Sessions", func(close func()) fyne.CanvasObject {
		msg := newAlertLine()

		nameEntry := widget.NewEntry()
		def := s.selName
		if def == "" {
			def = "session"
		}
		nameEntry.SetText(def)

		listBox := container.NewVBox()
		var refresh func()
		refresh = func() {
			listBox.RemoveAll()
			names := s.sess.List()
			if len(names) == 0 {
				listBox.Add(widget.NewLabel("(no saved sessions)"))
			}
			for _, n := range names {
				n := n
				load := widget.NewButton("Load", func() {
					ses, err := s.sess.Load(n)
					if err != nil {
						msg.fail("load failed: " + err.Error())
						return
					}
					s.applySession(ses)
					close() // the restored device list sits behind this dialog
				})
				load.Importance = widget.HighImportance
				del := widget.NewButton("Delete", func() {
					s.confirm("Delete session?",
						"Delete the saved session \""+n+"\"?\n\nThis cannot be undone.",
						func() {
							if err := s.sess.Delete(n); err != nil {
								msg.fail("delete failed: " + err.Error())
								return
							}
							msg.ok("deleted: " + n)
							refresh()
						})
				})
				del.Importance = widget.DangerImportance
				row := container.NewBorder(nil, nil, widget.NewLabel(n), container.NewHBox(load, del))
				listBox.Add(row)
			}
			listBox.Refresh()
		}
		refresh()

		saveBtn := widget.NewButton("Save current", func() {
			name := strings.TrimSpace(nameEntry.Text)
			if name == "" {
				msg.warn("enter a name first")
				return
			}
			if err := s.sess.Save(s.captureSession(name)); err != nil {
				msg.fail("save failed: " + err.Error())
				return
			}
			msg.ok("saved: " + name)
			refresh()
		})
		saveBtn.Importance = widget.HighImportance

		top := container.NewVBox(
			container.NewBorder(nil, nil, nil, saveBtn, nameEntry),
			msg,
		)
		return container.NewBorder(top, nil, nil, nil, container.NewVScroll(listBox))
	})
}
