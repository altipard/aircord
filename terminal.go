package main

import (
	"path/filepath"
	"strings"
	"time"

	"fyne.io/fyne/v2"

	"gitlab.com/zilicon-it-services/petriheil/aircord/internal/config"
)

const (
	maxLogLines = 2000 // cap terminal history
	// termPaintDelay coalesces a burst of log lines into one repaint. Setting the
	// whole buffer costs O(lines), so a GATT dump or a playbook run used to pay
	// that price per line. Short enough to feel immediate for a single command.
	termPaintDelay = 60 * time.Millisecond
)

// logLine appends a timestamped line to the terminal buffer and schedules a
// repaint. UI-thread only.
func (s *scanner) logLine(line string) {
	s.logMu.Lock()
	s.logs = append(s.logs, ts()+" "+line)
	if len(s.logs) > maxLogLines {
		s.logs = s.logs[len(s.logs)-maxLogLines:]
	}
	s.logMu.Unlock()
	s.scheduleTermPaint()
}

// scheduleTermPaint arranges exactly one repaint for the current burst of lines.
// UI-thread only.
func (s *scanner) scheduleTermPaint() {
	if s.termDirty {
		return
	}
	s.termDirty = true
	time.AfterFunc(termPaintDelay, func() { fyne.Do(s.paintTerm) })
}

// paintTerm writes the buffer into the terminal widget and scrolls to the
// bottom. UI-thread only.
func (s *scanner) paintTerm() {
	s.termDirty = false
	text := s.logText()
	s.term.SetText(text)
	s.term.CursorRow = strings.Count(text, "\n") // scroll to bottom
	s.term.Refresh()
}

// fail reports an error on the alert line and mirrors it into the terminal.
// The terminal is the durable record: an alert can be superseded, the log can be
// scrolled back through and saved. UI-thread only.
func (s *scanner) fail(msg string) {
	s.alert.fail(msg)
	s.logLine("!! " + msg)
}

// ok reports a completed action; it clears itself after a few seconds.
// UI-thread only.
func (s *scanner) ok(msg string) { s.alert.ok(msg) }

func (s *scanner) clearLog() {
	s.logMu.Lock()
	s.logs = nil
	s.logMu.Unlock()
	s.term.SetText("")
}

func (s *scanner) setStatus(msg string) { s.statusLbl.SetText(msg) }

func ts() string { return time.Now().Format("15:04:05") }

// logText returns the whole terminal buffer as a single string.
func (s *scanner) logText() string {
	s.logMu.Lock()
	defer s.logMu.Unlock()
	return strings.Join(s.logs, "\n")
}

// copyLog puts the entire log on the clipboard (regardless of selection).
func (s *scanner) copyLog() {
	fyne.CurrentApp().Clipboard().SetContent(s.logText())
	s.ok("log copied to clipboard")
}

// saveLog writes the log to a timestamped file under the user config dir and
// reports the path in the terminal.
func (s *scanner) saveLog() {
	text := s.logText()
	if text == "" {
		s.alert.warn("log is empty — nothing to save")
		return
	}
	path := filepath.Join(config.Dir("logs"), "aircord-"+time.Now().Format("20060102-150405")+".log")
	if err := config.AtomicWriteFile(path, []byte(text+"\n"), 0o644); err != nil {
		s.fail("save failed: " + err.Error())
		return
	}
	s.ok("log saved")
	s.logLine("-- log saved: " + path + " --")
}
