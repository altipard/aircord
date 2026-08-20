package ble

import (
	"errors"
	"testing"
	"time"
)

type recordEvents struct {
	logs    []string
	dropped int
}

func (r *recordEvents) Log(line string) { r.logs = append(r.logs, line) }
func (r *recordEvents) LinkDropped()    { r.dropped++ }

func TestIsATDataLine(t *testing.T) {
	if !isATDataLine("<< AT+DATA=FE00EF") {
		t.Error("want match for AT+DATA line")
	}
	if isATDataLine(">> AT+TX=3,FE00EF") {
		t.Error("AT+TX line must not match")
	}
}

func TestSummarizeLine(t *testing.T) {
	got := summarizeLine("AT+TX=123,DEADBEEF\r\n")
	if got != "AT+TX (18 bytes)" {
		t.Errorf("summarizeLine = %q, want %q", got, "AT+TX (18 bytes)")
	}
	if summarizeLine("AT\r\n") != "AT" {
		t.Errorf("summarizeLine(AT) = %q, want AT", summarizeLine("AT\r\n"))
	}
}

func TestAwaitReplyMatch(t *testing.T) {
	lines := make(chan string, 4)
	lines <- "noise"
	lines <- "<< AT+DATA=FE00EF"
	sent := false
	got, err := awaitReply(
		func() error { sent = true; return nil },
		lines,
		isATDataLine,
		time.After(time.Second),
	)
	if err != nil {
		t.Fatalf("awaitReply: %v", err)
	}
	if !sent {
		t.Error("send was not called")
	}
	if got != "<< AT+DATA=FE00EF" {
		t.Errorf("got %q", got)
	}
}

func TestAwaitReplySendError(t *testing.T) {
	boom := errors.New("boom")
	_, err := awaitReply(
		func() error { return boom },
		make(chan string),
		isATDataLine,
		time.After(time.Second),
	)
	if !errors.Is(err, boom) {
		t.Fatalf("err = %v, want boom", err)
	}
}

func TestAwaitReplyTimeout(t *testing.T) {
	fired := make(chan time.Time, 1)
	fired <- time.Now() // already elapsed
	_, err := awaitReply(
		func() error { return nil },
		make(chan string),
		isATDataLine,
		fired,
	)
	if !errors.Is(err, errOTATimeout) {
		t.Fatalf("err = %v, want errOTATimeout", err)
	}
}

func TestAwaitReplyLinkDropped(t *testing.T) {
	lines := make(chan string)
	close(lines) // simulate the tap being torn down
	_, err := awaitReply(
		func() error { return nil },
		lines,
		isATDataLine,
		time.After(time.Second),
	)
	if !errors.Is(err, errLinkDropped) {
		t.Fatalf("err = %v, want errLinkDropped", err)
	}
}

func TestBeginOTABusy(t *testing.T) {
	c := New(&recordEvents{})
	s1, err := c.BeginOTA()
	if err != nil {
		t.Fatalf("first BeginOTA: %v", err)
	}
	if _, err := c.BeginOTA(); !errors.Is(err, ErrOTABusy) {
		t.Fatalf("second BeginOTA err = %v, want ErrOTABusy", err)
	}
	s1.Close()
	s2, err := c.BeginOTA() // tap freed, a new session is allowed
	if err != nil {
		t.Fatalf("BeginOTA after Close: %v", err)
	}
	s2.Close()
	s2.Close() // idempotent
}

// A live tap must receive reassembled notify lines; after Close it must not.
func TestNotifyTapFeedsSession(t *testing.T) {
	c := New(&recordEvents{})
	s, err := c.BeginOTA()
	if err != nil {
		t.Fatalf("BeginOTA: %v", err)
	}
	c.onNotify([]byte("<< AT+DATA=FE00EF\r\n"))
	select {
	case line := <-s.lines:
		if line != "<< AT+DATA=FE00EF" {
			t.Errorf("tapped %q", line)
		}
	default:
		t.Fatal("tap did not receive the line")
	}

	s.Close()
	c.onNotify([]byte("after close\r\n"))
	select {
	case line := <-s.lines:
		t.Errorf("received %q after Close; tap should be detached", line)
	default:
	}
}

// A full tap buffer must not block or panic the notify goroutine.
func TestNotifyTapNonBlocking(t *testing.T) {
	c := New(&recordEvents{})
	s, err := c.BeginOTA()
	if err != nil {
		t.Fatalf("BeginOTA: %v", err)
	}
	defer s.Close()
	for i := 0; i < otaLineBuffer+50; i++ {
		c.onNotify([]byte("line\r\n")) // must never block even past capacity
	}
}
