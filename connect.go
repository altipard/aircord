package main

import (
	"strings"
	"time"

	"fyne.io/fyne/v2"
)

// settleDelay is how long the app waits after a link comes up before sending the
// PIN. Serial-over-BLE peripherals typically finish their own init in that
// window; sending earlier gets the PIN swallowed.
const settleDelay = 4 * time.Second

// Log implements ble.Events. It is called from BLE goroutines, so it marshals the
// line onto the UI thread before touching the terminal widget. Device replies are
// additionally fed to the playbook line tap on this goroutine, so a waiting
// playbook step sees them without a UI-thread round trip.
func (s *scanner) Log(line string) {
	if rest, ok := strings.CutPrefix(line, "<< "); ok {
		s.tapMu.Lock()
		tap := s.lineTap
		s.tapMu.Unlock()
		if tap != nil {
			tap(rest)
		}
	}
	fyne.Do(func() {
		s.logLine(line)
		s.tryCapture(line)
	})
}

// LinkDropped implements ble.Events: an asynchronous disconnect (device reset or
// out of range). Reflect it so the UI doesn't look frozen.
func (s *scanner) LinkDropped() {
	fyne.Do(func() {
		s.logLine("-- link dropped (device reset or out of range) — reconnect --")
		s.setDisconnectedUI("link dropped — reconnect")
	})
}

func (s *scanner) connect() {
	if s.ble.IsConnected() {
		return
	}
	if !s.hasSel {
		s.alert.warn("select a device in the list first")
		return
	}
	// Can't scan and connect at once — stop discovery.
	s.stopScan()
	addr := s.selAddr
	name := s.selName
	// Read the PIN widget here on the UI thread; the connect goroutine must not
	// touch widgets directly.
	pin := strings.TrimSpace(s.pinEntry.Text)
	s.connectBtn.Disable()

	go func() {
		fyne.Do(func() { s.logLine("-- connecting to " + name + " (" + addr.String() + ") --") })

		prof, ok := s.ble.Connect(addr)
		if !ok {
			// ble.Connect already logged the "!! …" cause.
			fyne.Do(func() { s.connectBtn.Enable() })
			return
		}
		fyne.Do(func() {
			s.logLine("-- connected via " + prof + " UART profile, ready --")
			s.setConnectedUI(name, s.ble.CurrentAddr())
		})

		// Give the device a moment, then auto-send the PIN if provided — but only
		// if the link survived the init window. The wait is announced: four silent
		// seconds after "connected" read as a hang.
		if pin != "" {
			fyne.Do(func() {
				s.setStatus("connected: " + name + " — settling, PIN in " + settleDelay.String())
				s.logLine("-- waiting " + settleDelay.String() + " for the device to settle, then sending PIN --")
			})
		}
		time.Sleep(settleDelay)
		if pin == "" {
			return
		}
		if s.ble.IsConnected() {
			// SendSecret keeps the PIN out of the log, which gets saved and shared.
			s.ble.SendSecret(pin)
			fyne.Do(func() { s.setStatus("connected: " + name) })
		} else {
			fyne.Do(func() { s.logLine("-- PIN skipped: link dropped during init --") })
		}
	}()
}

func (s *scanner) disconnect() {
	if !s.ble.Disconnect() {
		return
	}
	s.logLine("-- disconnected --")
	s.setDisconnectedUI("disconnected")
}

// setConnectedUI switches the connection controls to the connected state and
// restores the device's saved command history. UI-thread only; mirror of
// setDisconnectedUI so the enable/disable pairs can't drift.
func (s *scanner) setConnectedUI(name, addr string) {
	s.connectBtn.Disable()
	s.disconnBtn.Enable()
	s.sendBtn.Enable()
	s.cmdEntry.Enable()
	s.cmdEntry.setHistory(s.hist.Get(addr))
	s.applyTemplateFor(name)
	s.setStatus("connected: " + name)
}

// setDisconnectedUI resets the connection controls to the disconnected state.
func (s *scanner) setDisconnectedUI(status string) {
	s.connectBtn.Enable()
	s.disconnBtn.Disable()
	s.sendBtn.Disable()
	s.cmdEntry.Disable()
	s.clearPanel()
	s.setStatus(status)
}

func (s *scanner) sendFromEntry() {
	cmd := strings.TrimSpace(s.cmdEntry.Text)
	if cmd == "" {
		return
	}
	if !s.ble.IsConnected() {
		s.alert.warn("not connected — connect to a device first")
		return
	}
	s.cmdEntry.add(cmd)
	s.cmdEntry.SetText("")
	s.sendCmd(cmd)
}

// sendCmd sends an AT command to the connected device and persists it to the
// per-device history. Shared by the command line (sendFromEntry) and the control
// panel (panel.go). It is a no-op with a status hint when nothing is connected.
// UI-thread only; the actual write runs on a background goroutine.
func (s *scanner) sendCmd(cmd string) {
	cmd = strings.TrimSpace(cmd)
	if cmd == "" {
		return
	}
	if !s.ble.IsConnected() {
		s.alert.warn("not connected — connect to a device first")
		return
	}
	if addr := s.ble.CurrentAddr(); addr != "" {
		if err := s.hist.Append(addr, cmd); err != nil { // persist per-device
			s.fail("history save failed: " + err.Error())
		}
	}
	go s.ble.Send(cmd)
}
