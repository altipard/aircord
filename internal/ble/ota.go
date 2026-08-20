package ble

import (
	"errors"
	"strconv"
	"strings"
	"sync"
	"time"
)

// OTA firmware-update wiring: an OTASession adapts the live connection to the
// request/response transport the otanb package drives. It taps reassembled
// notify lines to await bootloader replies and sends command lines raw (chunked
// to the BLE MTU, without logging the multi-kilobyte hex payload).
//
// *OTASession structurally satisfies otanb.Transport
// (Roundtrip(string, time.Duration) (string, error)); the main/UI layer wires the
// two together so package ble keeps no dependency on package otanb.

// atDataMarker identifies a bootloader response line. The device answers each
// AT+TX= command with an AT+DATA=<HEX> frame.
const atDataMarker = "AT+DATA="

// otaLineBuffer sizes the notify tap. A firmware upgrade is strictly lock-step
// (one command, then wait), so a handful of lines is the realistic depth; this
// is generous headroom.
const otaLineBuffer = 256

var (
	// ErrOTABusy is returned if an OTA session is already active on the client.
	ErrOTABusy = errors.New("ble: an OTA session is already active")
	// ErrNotConnected is returned when a raw write is attempted with no link.
	ErrNotConnected = errors.New("ble: not connected")
	// errOTATimeout is the timeout waiting for a bootloader reply.
	errOTATimeout = errors.New("ble: timeout waiting for AT+DATA reply")
	// errLinkDropped reports the connection dropped mid-round-trip.
	errLinkDropped = errors.New("ble: link dropped during OTA")
)

// OTASession is a firmware-update transport bound to the current connection.
// Close it when the upgrade finishes to stop tapping notify lines.
type OTASession struct {
	c        *Client
	lines    chan string
	closeOne sync.Once
}

// BeginOTA installs the notify tap and returns a session. It fails if one is
// already active. The caller must Close the returned session.
func (c *Client) BeginOTA() (*OTASession, error) {
	c.tapMu.Lock()
	defer c.tapMu.Unlock()
	if c.tap != nil {
		return nil, ErrOTABusy
	}
	ch := make(chan string, otaLineBuffer)
	c.tap = ch
	return &OTASession{c: c, lines: ch}, nil
}

// Close removes the tap. Safe to call more than once.
func (s *OTASession) Close() {
	s.closeOne.Do(func() {
		s.c.tapMu.Lock()
		if s.c.tap == s.lines {
			s.c.tap = nil
		}
		s.c.tapMu.Unlock()
	})
}

// Roundtrip sends one transport line (from otanb.ATTX) and returns the next
// AT+DATA reply line, or an error on timeout / link loss. Stale lines buffered
// before the send are discarded so a prior banner cannot be mistaken for this
// command's reply.
func (s *OTASession) Roundtrip(line string, timeout time.Duration) (string, error) {
	drain(s.lines)
	return awaitReply(
		func() error { return s.c.sendRaw(line) },
		s.lines,
		isATDataLine,
		time.After(timeout),
	)
}

// sendRaw writes line to the device exactly as given (the caller includes any
// CRLF), chunked to the BLE MTU. The payload is not logged — a firmware frame is
// kilobytes of hex — but a short marker records the send.
func (c *Client) sendRaw(line string) error {
	rx, ok := c.writeTarget()
	if !ok {
		return ErrNotConnected
	}
	c.events.Log(">> " + summarizeLine(line))
	data := []byte(line)
	for i := 0; i < len(data); i += mtuChunk {
		end := i + mtuChunk
		if end > len(data) {
			end = len(data)
		}
		if _, err := rx.WriteWithoutResponse(data[i:end]); err != nil {
			c.events.Log("!! write error: " + err.Error())
			return err
		}
	}
	return nil
}

// summarizeLine renders a loggable one-liner for a raw command without its hex
// body: "AT+TX (1234 bytes)" instead of the full frame.
func summarizeLine(line string) string {
	trimmed := strings.TrimRight(line, "\r\n")
	if verb, _, found := strings.Cut(trimmed, "="); found {
		return verb + " (" + strconv.Itoa(len(trimmed)) + " bytes)"
	}
	return trimmed
}

// isATDataLine reports whether a notify line is a bootloader response frame.
func isATDataLine(line string) bool { return strings.Contains(line, atDataMarker) }

// drain empties any buffered lines without blocking.
func drain(ch <-chan string) {
	for {
		select {
		case <-ch:
		default:
			return
		}
	}
}

// awaitReply sends, then returns the first line matching want, or an error on a
// closed channel (link dropped) or timeout firing. Pure over its inputs so the
// round-trip control flow is unit-tested without a radio.
func awaitReply(send func() error, lines <-chan string, want func(string) bool, timeout <-chan time.Time) (string, error) {
	if err := send(); err != nil {
		return "", err
	}
	for {
		select {
		case line, ok := <-lines:
			if !ok {
				return "", errLinkDropped
			}
			if want(line) {
				return line, nil
			}
		case <-timeout:
			return "", errOTATimeout
		}
	}
}
