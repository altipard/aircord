package main

import (
	"fmt"
	"strings"
	"time"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/widget"

	"github.com/altipard/aircord/internal/playbook"
)

// waitTimeout bounds how long a step with a "?<pattern>" reply-wait may take.
// Serial-over-BLE consoles answer set-commands within a second or two when idle;
// 15 s covers busy phases (boot, TX window) without hanging a playbook forever.
const waitTimeout = 15 * time.Second

// playbookSyntax is shown above the editor for as long as it is open. The old
// version put it in the entry's placeholder, where it vanished on the first
// keystroke — exactly when the user starts needing it.
//
// The lines are pre-broken and rendered without word wrap: a wrapped label's
// height depends on the width it is given, which makes the dialog's minimum
// size unpredictable.
const playbookSyntax = "one command per line\n" +
	"?PATTERN   wait for a matching reply   (bare ? = OK)\n" +
	"@MS        pause afterwards\n" +
	"#          comment"

// runPlaybook replays a playbook's steps in order on a background goroutine.
// A step with WaitFor blocks until the device reply matches (or the run aborts
// on timeout/link loss — better than silently racing a busy console); WaitMs
// then adds a fixed settle pause. All UI/log touches are marshalled onto the
// UI thread.
func (s *scanner) runPlaybook(pb playbook.Playbook) {
	if !s.ble.IsConnected() {
		s.alert.warn("not connected — connect to a device first")
		return
	}
	if s.otaRunning {
		s.alert.warn("a firmware update is running — the link belongs to the bootloader")
		return
	}
	if len(pb.Steps) == 0 {
		s.alert.warn("playbook has no steps")
		return
	}
	s.setStatus("playbook running: " + pb.Name)
	s.alert.clear()
	total := len(pb.Steps)
	go func() {
		lines := make(chan string, 64)
		s.setLineTap(func(l string) {
			select {
			case lines <- l:
			default: // runner busy — dropping is fine, matcher only needs fresh lines
			}
		})
		defer s.setLineTap(nil)

		for i, st := range pb.Steps {
			if !s.ble.IsConnected() {
				fyne.Do(func() { s.fail("playbook " + pb.Name + " stopped: link lost") })
				return
			}
			cmd := strings.TrimSpace(st.Cmd)
			if cmd == "" {
				continue
			}
			// Drop replies of earlier steps so e.g. the "OK" closing an AT+CFG
			// dump can't satisfy the next step's wait.
		drain:
			for {
				select {
				case <-lines:
				default:
					break drain
				}
			}
			step := i + 1
			fyne.Do(func() {
				s.setStatus(fmt.Sprintf("playbook %s — step %d/%d: %s", pb.Name, step, total, cmd))
				s.logLine(fmt.Sprintf("-- playbook %s [%d/%d]: %s --", pb.Name, step, total, cmd))
			})
			s.ble.Send(cmd)
			if st.WaitFor != "" {
				re := playbook.CompileWait(st.WaitFor)
				if !playbook.AwaitMatch(lines, re, waitTimeout, s.ble.IsConnected) {
					reason := "no reply matching " + st.WaitFor
					if !s.ble.IsConnected() {
						reason = "link lost"
					}
					fyne.Do(func() {
						s.fail(fmt.Sprintf("playbook %s aborted at step %d/%d (%s): %s",
							pb.Name, step, total, cmd, reason))
						s.setStatus("playbook aborted: " + pb.Name)
					})
					return
				}
			}
			if st.WaitMs > 0 {
				time.Sleep(time.Duration(st.WaitMs) * time.Millisecond)
			}
		}
		fyne.Do(func() {
			s.logLine("-- playbook " + pb.Name + " finished --")
			s.setStatus("playbook finished: " + pb.Name)
			s.ok("playbook finished: " + pb.Name)
		})
	}()
}

// startPlaybook confirms any destructive step, then closes the dialog before
// running: the run reports its progress into the terminal, which the dialog
// would otherwise cover for its whole duration.
func (s *scanner) startPlaybook(pb playbook.Playbook, close func()) {
	run := func() {
		close()
		s.runPlaybook(pb)
	}
	risky := playbook.RiskySteps(pb.Steps)
	if len(risky) == 0 {
		run()
		return
	}
	s.confirm("Run \""+pb.Name+"\"?",
		"This playbook contains commands that reboot or reset the device:\n\n"+
			strings.Join(risky, "\n")+
			"\n\nA device already deployed in the field will drop its link and restart.",
		run)
}

// openPlaybooks shows the dialog for command-sequence playbooks.
func (s *scanner) openPlaybooks() { s.showModal("Playbooks", s.playbookBody) }

// playbookBody builds the playbook dialog: the saved playbooks and the editor,
// as two tabs.
//
// They used to be stacked in a VSplit, whose minimum size is the sum of both
// halves. The editor half alone — name field, wrapped syntax help, a six-row
// text area and a button — was taller than the dialog, so the list underneath
// was squeezed out of view entirely. Tabs make the dialog's minimum size the
// larger of the two halves rather than their sum, and give whichever half is
// showing the whole area.
func (s *scanner) playbookBody(close func()) fyne.CanvasObject {
	msg := newAlertLine()

	nameEntry := widget.NewEntry()
	nameEntry.SetPlaceHolder("Playbook name")

	editor := widget.NewMultiLineEntry()
	editor.SetPlaceHolder("AT+CFG\nAT+APN=iot.1nce.net ?OK\nAT+TDC=3600 ? @1500")
	editor.SetMinRowsVisible(7)

	syntax := widget.NewLabel(playbookSyntax)
	syntax.TextStyle = fyne.TextStyle{Monospace: true}

	// Assigned below; the list's Edit buttons switch to the editor tab.
	var tabs *container.AppTabs
	var editorTab *container.TabItem

	listBox := container.NewVBox()
	var refresh func()
	refresh = func() {
		listBox.RemoveAll()
		names := s.plays.List()
		if len(names) == 0 {
			listBox.Add(widget.NewLabel("No playbooks yet — create one in the Editor tab."))
		}
		for _, n := range names {
			n := n
			caption := n
			if pb, err := s.plays.Load(n); err == nil {
				caption = fmt.Sprintf("%s  ·  %d steps", n, len(pb.Steps))
			}

			run := widget.NewButton("Run", func() {
				pb, err := s.plays.Load(n)
				if err != nil {
					msg.fail("load failed: " + err.Error())
					return
				}
				s.startPlaybook(pb, close)
			})
			run.Importance = widget.HighImportance

			edit := widget.NewButton("Edit", func() {
				pb, err := s.plays.Load(n)
				if err != nil {
					msg.fail("load failed: " + err.Error())
					return
				}
				nameEntry.SetText(pb.Name)
				editor.SetText(playbook.FormatSteps(pb.Steps))
				msg.ok("editing: " + pb.Name)
				if tabs != nil && editorTab != nil {
					tabs.Select(editorTab)
				}
			})

			del := widget.NewButton("Delete", func() {
				s.confirm("Delete playbook?",
					"Delete the playbook \""+n+"\"?\n\nThis cannot be undone.",
					func() {
						if err := s.plays.Delete(n); err != nil {
							msg.fail("delete failed: " + err.Error())
							return
						}
						msg.ok("deleted: " + n)
						refresh()
					})
			})
			del.Importance = widget.DangerImportance

			row := container.NewBorder(nil, nil,
				widget.NewLabel(caption),
				container.NewHBox(run, edit, del))
			listBox.Add(row)
		}
		listBox.Refresh()
	}
	refresh()

	saveBtn := widget.NewButton("Save", func() {
		name := strings.TrimSpace(nameEntry.Text)
		if name == "" {
			msg.warn("enter a name first")
			return
		}
		pb := playbook.Playbook{Name: name, Steps: playbook.ParseSteps(editor.Text)}
		if len(pb.Steps) == 0 {
			msg.warn("no steps — add at least one command")
			return
		}
		if err := s.plays.Save(pb); err != nil {
			msg.fail("save failed: " + err.Error())
			return
		}
		msg.ok("saved: " + name)
		refresh()
	})
	saveBtn.Importance = widget.HighImportance

	editorTab = container.NewTabItem("Editor", container.NewBorder(
		container.NewVBox(nameEntry, syntax),
		saveBtn,
		nil, nil,
		editor,
	))
	tabs = container.NewAppTabs(
		container.NewTabItem("Saved", container.NewVScroll(listBox)),
		editorTab,
	)
	// The alert sits outside the tabs so a message raised on one is still
	// readable after switching to the other.
	return container.NewBorder(nil, msg, nil, nil, tabs)
}
