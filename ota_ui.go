package main

import (
	"errors"
	"fmt"
	"io"
	"regexp"
	"strings"
	"time"

	"fyne.io/fyne/v2"
	"fyne.io/fyne/v2/container"
	"fyne.io/fyne/v2/dialog"
	"fyne.io/fyne/v2/layout"
	"fyne.io/fyne/v2/storage"
	"fyne.io/fyne/v2/widget"

	"github.com/altipard/aircord/internal/ble"
	"github.com/altipard/aircord/internal/otanb"
)

// Firmware update over BLE for Dragino NB-IoT nodes.
//
// The node's bootloader only listens right after a reset: for 8–15 s it
// advertises under the node's IMEI and accepts the otanb flash protocol over
// the same HM-10 UART the console uses. Everything here is the dance around
// that window, in the order Dragino's own tool performs it:
//
//	1. reset the node — ATZ over the open link, or the user presses RESET
//	2. scan, and wait for a device advertising as the IMEI (seen after the reset)
//	3. connect to it and run otanb.Upgrade: erase, flash, verify, reboot
//
// The protocol lives in internal/otanb, the transport in internal/ble; this
// file wires them and reports progress. The bootloader region is never
// written, so a failed or cancelled attempt is repeatable.

const (
	// bootloaderWindow bounds the wait for the node to re-advertise under its
	// IMEI. Dragino documents the bootloader's BLE as on for 8–15 s after a
	// reset; the headroom covers the reboot itself and scan latency.
	bootloaderWindow = 20 * time.Second
	// resetDropWait is how long the ATZ path waits for the node to drop the link
	// before closing it from this side.
	resetDropWait = 3 * time.Second
	// otaResetCmd reboots the node, which takes it through the bootloader.
	otaResetCmd = "ATZ"
	// otaReadIMEICmd prints the configuration, which includes the IMEI.
	otaReadIMEICmd = "AT+CFG"
	// otaImageLimit caps how much of a chosen file is read: the application
	// region is ~223 KiB, so anything past a mebibyte is not a firmware image.
	otaImageLimit = 1 << 20
)

// firmwareHint is shown in the dialog for as long as it is open, like the
// playbook syntax help: the reset step is timing-critical and a user should
// not have to remember it.
const firmwareHint = "1. Start — Aircord scans for the node's bootloader\n" +
	"2. The node resets: ATZ over the link, or press its RESET key\n" +
	"3. For 8–15 s it advertises as the IMEI — Aircord connects\n" +
	"4. Erase, flash, verify, reboot\n" +
	"\n" +
	"A node reset less than two minutes ago may skip the bootloader.\n" +
	"The .bin must be the application only, without the bootloader."

// imeiRe pulls the IMEI out of a configuration dump line such as "IMEI: 8606…".
var imeiRe = regexp.MustCompile(`(?i)IMEI\D{0,8}(\d{15})`)

// extractIMEI is the capture matcher for the dialog's Read button.
func extractIMEI(line string) (string, bool) {
	m := imeiRe.FindStringSubmatch(line)
	if m == nil {
		return "", false
	}
	return m[1], true
}

// matchesIMEI reports whether an advertised name is the bootloader of the
// wanted node. The bootloader advertises the bare IMEI; an exact match is the
// normal case. A name merely containing the IMEI is accepted too, so a firmware
// that adds a prefix still matches — but only for an IMEI long enough that the
// substring cannot be a coincidence.
func matchesIMEI(name, imei string) bool {
	name = strings.TrimSpace(name)
	imei = strings.TrimSpace(imei)
	if imei == "" {
		return false
	}
	if strings.EqualFold(name, imei) {
		return true
	}
	return len(imei) >= 8 && strings.Contains(name, imei)
}

// looksLikeIMEI reports whether a device name is a bare IMEI, which is how a
// node already sitting in its bootloader shows up in the device list.
func looksLikeIMEI(name string) bool {
	if len(name) != 15 {
		return false
	}
	for _, r := range name {
		if r < '0' || r > '9' {
			return false
		}
	}
	return true
}

// otaPassword turns the console PIN into the bootloader's 8-byte password
// field: the fixed default for a node without one, else the 'O'-padded PIN.
func otaPassword(pin string) ([]byte, error) {
	pin = strings.TrimSpace(pin)
	if pin == "" {
		return otanb.DefaultPassword(), nil
	}
	return otanb.EncodePIN(pin)
}

// otaJob is everything the update goroutine needs, gathered on the UI thread
// before it starts so it never has to read a widget.
type otaJob struct {
	plan      otanb.Plan
	file      string // display name of the image
	imei      string
	sendReset bool // reset via ATZ over the open link, else the user presses RESET
}

// otaProgress is the modal that reports an update. Created and updated on the
// UI thread only; the goroutine marshals through fyne.Do.
type otaProgress struct {
	dlg    dialog.Dialog
	phase  *widget.Label
	bar    *widget.ProgressBar
	cancel *widget.Button
}

func newOTAProgress(win fyne.Window, onCancel func()) *otaProgress {
	p := &otaProgress{
		phase: widget.NewLabel("starting…"),
		bar:   widget.NewProgressBar(),
	}
	p.phase.Wrapping = fyne.TextWrapWord
	p.cancel = widget.NewButton("Cancel", onCancel)
	p.cancel.Importance = widget.DangerImportance
	body := container.NewVBox(p.phase, p.bar, container.NewCenter(p.cancel))
	p.dlg = dialog.NewCustom("Firmware update", "Hide", body, win)
	p.dlg.Resize(fyne.NewSize(460, 180))
	p.dlg.Show()
	return p
}

func (p *otaProgress) setPhase(text string) { p.phase.SetText(text) }

func (p *otaProgress) setProgress(sent, total int) {
	if total > 0 {
		p.bar.SetValue(float64(sent) / float64(total))
	}
}

// finish freezes the dialog on the outcome; the Cancel button has nothing left
// to cancel.
func (p *otaProgress) finish(err error) {
	p.cancel.Disable()
	p.dlg.SetDismissText("Close")
	if err != nil {
		p.setPhase("failed: " + err.Error())
		return
	}
	p.bar.SetValue(1)
	p.setPhase("update successful — the node is rebooting into the new firmware")
}

// openFirmware shows the firmware-update dialog: pick the image, name the node
// by IMEI, choose how it gets reset, start.
func (s *scanner) openFirmware() {
	if s.otaRunning {
		s.alert.warn("a firmware update is already running")
		return
	}
	s.showModal("Firmware update", s.firmwareBody)
}

// firmwareBody builds the dialog content. Split from openFirmware so a test can
// lay it out at dialog size.
func (s *scanner) firmwareBody(close func()) fyne.CanvasObject {
	msg := newAlertLine()

	fileLbl := widget.NewLabel(s.otaFileCaption())
	fileLbl.Truncation = fyne.TextTruncateEllipsis
	chooseBtn := widget.NewButton("Choose .bin…", func() {
		s.chooseFirmware(func(err error) {
			if err != nil {
				msg.fail(err.Error())
				return
			}
			fileLbl.SetText(s.otaFileCaption())
			msg.ok("image loaded: " + s.otaFile)
		})
	})

	imeiEntry := widget.NewEntry()
	imeiEntry.SetPlaceHolder("IMEI — the bootloader advertises under it")
	imeiEntry.SetText(s.defaultIMEI())
	readBtn := widget.NewButton("Read", func() {
		if !s.ble.IsConnected() {
			msg.warn("not connected — connect to the node first, or type the IMEI from its label")
			return
		}
		s.armCapture(extractIMEI, func(v string) {
			if v == "(no reply)" {
				msg.warn("no IMEI in the reply — type it from the node's label")
				return
			}
			imeiEntry.SetText(v)
			msg.ok("IMEI read from device")
		})
		s.sendCmd(otaReadIMEICmd)
	})

	const (
		resetATZ = "Send ATZ over the current connection"
		resetKey = "I press the RESET key on the node"
	)
	resetSel := widget.NewRadioGroup([]string{resetATZ, resetKey}, nil)
	resetSel.Required = true
	if s.ble.IsConnected() {
		resetSel.SetSelected(resetATZ)
	} else {
		resetSel.SetSelected(resetKey)
	}

	pinNote := widget.NewLabel("Password: the PIN from the main window (bootloader v1.3+ checks it).")
	pinNote.Wrapping = fyne.TextWrapWord

	hint := widget.NewLabel(firmwareHint)
	hint.TextStyle = fyne.TextStyle{Monospace: true}

	startBtn := widget.NewButton("Start update", func() {
		job, err := s.buildOTAJob(imeiEntry.Text, resetSel.Selected == resetATZ)
		if err != nil {
			msg.fail(err.Error())
			return
		}
		s.confirm("Flash "+job.file+"?",
			"This erases and rewrites the application firmware of node\n\n"+
				job.imei+"\n\n"+
				"using "+job.file+". The node will reboot. Aircord only writes the\n"+
				"application region, so a failed attempt can be retried.",
			func() {
				close()
				s.runFirmwareUpdate(job)
			})
	})
	startBtn.Importance = widget.HighImportance

	form := container.New(layout.NewFormLayout(),
		widget.NewLabel("Image"), container.NewBorder(nil, nil, nil, chooseBtn, fileLbl),
		widget.NewLabel("IMEI"), container.NewBorder(nil, nil, nil, readBtn, imeiEntry),
		widget.NewLabel("Reset"), resetSel,
	)
	top := container.NewVBox(form, pinNote, hint)
	return container.NewBorder(top, container.NewVBox(msg, startBtn), nil, nil, nil)
}

// otaFileCaption names the loaded image for the dialog, or says none is.
func (s *scanner) otaFileCaption() string {
	if s.otaImage == nil {
		return "(no image chosen)"
	}
	return fmt.Sprintf("%s (%d bytes)", s.otaFile, len(s.otaImage))
}

// defaultIMEI pre-fills the IMEI field: the last one used, else the selected
// device's name if that already is an IMEI (a node sitting in its bootloader).
func (s *scanner) defaultIMEI() string {
	if s.otaIMEI != "" {
		return s.otaIMEI
	}
	if s.hasSel && looksLikeIMEI(s.selName) {
		return s.selName
	}
	return ""
}

// chooseFirmware opens the file picker, reads the chosen .bin and validates it
// as an application image. done runs on the UI thread with the outcome.
func (s *scanner) chooseFirmware(done func(error)) {
	fd := dialog.NewFileOpen(func(r fyne.URIReadCloser, err error) {
		if err != nil {
			done(fmt.Errorf("open failed: %w", err))
			return
		}
		if r == nil {
			return // cancelled
		}
		defer func() { _ = r.Close() }()
		data, err := io.ReadAll(io.LimitReader(r, otaImageLimit))
		if err != nil {
			done(fmt.Errorf("read failed: %w", err))
			return
		}
		if _, err := otanb.LoadFirmware(data); err != nil {
			done(err)
			return
		}
		s.otaImage = data
		s.otaFile = r.URI().Name()
		done(nil)
	}, s.win)
	fd.SetFilter(storage.NewExtensionFileFilter([]string{".bin"}))
	fd.Show()
}

// buildOTAJob validates the dialog's inputs into a job. UI-thread only.
func (s *scanner) buildOTAJob(imei string, sendReset bool) (otaJob, error) {
	imei = strings.TrimSpace(imei)
	if s.otaImage == nil {
		return otaJob{}, errors.New("choose a firmware image first")
	}
	if imei == "" {
		return otaJob{}, errors.New("enter the node's IMEI — it is on the label and in AT+CFG")
	}
	if sendReset && !s.ble.IsConnected() {
		return otaJob{}, errors.New("not connected — connect first, or reset the node by its key")
	}
	fw, err := otanb.LoadFirmware(s.otaImage)
	if err != nil {
		return otaJob{}, err
	}
	pw, err := otaPassword(s.pinEntry.Text)
	if err != nil {
		return otaJob{}, err
	}
	plan, err := otanb.PlanUpgrade(fw, pw, 0)
	if err != nil {
		return otaJob{}, err
	}
	s.otaIMEI = imei
	return otaJob{plan: plan, file: s.otaFile, imei: imei, sendReset: sendReset}, nil
}

// runFirmwareUpdate drives the whole update on a background goroutine; every
// UI touch is marshalled onto the UI thread. UI-thread only to start.
func (s *scanner) runFirmwareUpdate(job otaJob) {
	if s.otaRunning {
		s.alert.warn("a firmware update is already running")
		return
	}
	s.otaRunning = true
	s.alert.clear()

	// Cancel closes stop (ends the bootloader wait) and drops the link, which
	// makes the next protocol round-trip fail and otanb.Upgrade abort. Both
	// flags are touched on the UI thread only.
	stop := make(chan struct{})
	var cancelled bool
	prog := newOTAProgress(s.win, func() {
		if cancelled {
			return
		}
		cancelled = true
		close(stop)
		s.ble.Disconnect()
		s.logLine("-- firmware update cancelled by user --")
	})
	wasCancelled := func() bool {
		select {
		case <-stop:
			return true
		default:
			return false
		}
	}

	ui := func(f func()) { fyne.Do(f) }
	finish := func(err error) {
		if err != nil && wasCancelled() {
			err = errors.New("cancelled")
		}
		ui(func() {
			s.otaRunning = false
			s.setDisconnectedUI("firmware update finished")
			prog.finish(err)
			if err != nil {
				s.fail("firmware update failed: " + err.Error())
				s.setStatus("firmware update failed")
				return
			}
			s.logLine("-- firmware update successful — node rebooting --")
			s.ok("firmware update successful")
			s.setStatus("firmware update successful")
		})
	}

	go func() {
		ui(func() {
			s.logLine(fmt.Sprintf("-- firmware update: %s (%d bytes) -> node %s --",
				job.file, len(job.plan.Firmware.Image), job.imei))
		})

		since := time.Now()
		if err := s.otaReset(job, prog); err != nil {
			finish(err)
			return
		}

		// Scan for the bootloader. startScan/stopScan run the table repaint
		// loop, so the IMEI device is visible in the list while it is looked for.
		ui(func() {
			prog.setPhase("waiting for the bootloader to advertise as " + job.imei + "…")
			s.setStatus("firmware: waiting for bootloader " + job.imei)
			s.startScan()
		})
		dev, found := s.ble.AwaitDevice(func(d ble.Device) bool { return matchesIMEI(d.Name, job.imei) },
			since, bootloaderWindow, stop)
		ui(s.stopScan)
		if !found {
			finish(fmt.Errorf("bootloader %s not seen within %s — reset the node again (a node reset less than two minutes ago may not re-enter the bootloader)",
				job.imei, bootloaderWindow))
			return
		}

		ui(func() {
			s.logLine("-- bootloader found: " + dev.Name + " (" + dev.Addr + ") — connecting --")
			prog.setPhase("connecting to " + dev.Name + "…")
		})
		if _, ok := s.ble.Connect(dev.Address); !ok {
			finish(errors.New("connect to bootloader failed (see terminal)"))
			return
		}
		ui(func() { s.setOTAUI(dev.Name) })

		sess, err := s.ble.BeginOTA()
		if err != nil {
			s.ble.Disconnect()
			finish(err)
			return
		}
		defer sess.Close()

		total := len(job.plan.Firmware.Image)
		ui(func() {
			prog.setPhase(fmt.Sprintf("erasing and flashing %d bytes…", total))
			s.setStatus("firmware: flashing " + job.imei)
		})
		err = otanb.Upgrade(sess, job.plan, func(sent, total int) {
			ui(func() {
				prog.setProgress(sent, total)
				if sent >= total {
					prog.setPhase("verifying and rebooting…")
				} else {
					prog.setPhase(fmt.Sprintf("flashing %d / %d bytes", sent, total))
				}
			})
		})
		sess.Close()
		// The REBOOT reply is the last thing the bootloader says; the node drops
		// the link itself right after. Close it from this side if it has not.
		s.ble.Disconnect()
		finish(err)
	}()
}

// otaReset takes the node through a reset so it enters the bootloader. Over an
// open link it sends ATZ and waits for the drop; otherwise it asks the user to
// press the key. Runs on the update goroutine.
func (s *scanner) otaReset(job otaJob, prog *otaProgress) error {
	if !job.sendReset {
		// Scanning and a live link are kept exclusive elsewhere in the app;
		// drop a leftover link first, the reset would kill it anyway.
		if s.ble.Disconnect() {
			fyne.Do(func() {
				s.logLine("-- disconnected before the manual reset --")
				s.setDisconnectedUI("firmware: press RESET on the node")
			})
		}
		fyne.Do(func() {
			s.logLine("-- press the RESET key on the node now (or hold ACT for 3 s) --")
			prog.setPhase("press the RESET key on the node now")
		})
		return nil
	}

	if !s.ble.IsConnected() {
		return errors.New("link is gone — connect again, or reset the node by its key")
	}
	fyne.Do(func() {
		s.logLine("-- sending " + otaResetCmd + " to reset the node into its bootloader --")
		prog.setPhase("resetting the node with " + otaResetCmd + "…")
	})
	s.ble.Send(otaResetCmd)

	// The node drops the link as it reboots; LinkDropped then resets the UI.
	// If it does not within the grace period, close it from this side so the
	// scan below is not started on top of a live connection.
	deadline := time.Now().Add(resetDropWait)
	for s.ble.IsConnected() && time.Now().Before(deadline) {
		time.Sleep(100 * time.Millisecond)
	}
	if s.ble.Disconnect() {
		fyne.Do(func() {
			s.logLine("-- link still up after " + otaResetCmd + "; closed it to scan for the bootloader --")
			s.setDisconnectedUI("firmware: scanning for bootloader")
		})
	}
	return nil
}

// setOTAUI is the connected state for a bootloader link: Disconnect works as
// an emergency stop, but the console stays closed — a stray AT line in the
// middle of a flash frame would corrupt the transfer. UI-thread only.
func (s *scanner) setOTAUI(name string) {
	s.connectBtn.Disable()
	s.disconnBtn.Enable()
	s.sendBtn.Disable()
	s.cmdEntry.Disable()
	s.setStatus("firmware: connected to bootloader " + name)
}
