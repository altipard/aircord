package otanb

import (
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
	"hash/crc32"
	"strings"
	"testing"
	"time"
)

func TestDefaultPassword(t *testing.T) {
	pw := DefaultPassword()
	if len(pw) != 8 {
		t.Fatalf("len = %d, want 8", len(pw))
	}
	for i, b := range pw {
		if b != 0x66 {
			t.Errorf("byte %d = %#x, want 0x66", i, b)
		}
	}
}

func TestEncodePIN(t *testing.T) {
	pw, err := EncodePIN("349011")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if want := []byte("349011OO"); string(pw) != string(want) {
		t.Errorf("EncodePIN = %q, want %q", pw, want)
	}
	if len(pw) != 8 {
		t.Errorf("len = %d, want 8", len(pw))
	}
	full, _ := EncodePIN("12345678")
	if string(full) != "12345678" {
		t.Errorf("exact-width PIN = %q, want unchanged", full)
	}
}

func TestEncodePINTooLong(t *testing.T) {
	if _, err := EncodePIN("123456789"); err == nil {
		t.Error("EncodePIN with 9 chars: want error, got nil")
	}
}

// buildFrame must match the documented layout and use IEEE CRC32 (== zlib.crc32).
func TestBuildFrameLayout(t *testing.T) {
	pw := DefaultPassword()
	frame, err := buildFrame(cmdReboot, []byte{0x00}, pw)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	// START cmd pw(8) len(2) data(1) crc(4) END = 18 bytes.
	if len(frame) != 18 {
		t.Fatalf("len = %d, want 18", len(frame))
	}
	if frame[0] != startByte || frame[len(frame)-1] != endByte {
		t.Errorf("delimiters = %#x..%#x, want FE..EF", frame[0], frame[len(frame)-1])
	}
	if frame[1] != cmdReboot {
		t.Errorf("cmd = %d, want %d", frame[1], cmdReboot)
	}
	if binary.LittleEndian.Uint16(frame[10:12]) != 1 {
		t.Errorf("data_len = %d, want 1", binary.LittleEndian.Uint16(frame[10:12]))
	}
	body := frame[:13] // START..data
	wantCRC := crc32.ChecksumIEEE(body)
	if got := binary.LittleEndian.Uint32(frame[13:17]); got != wantCRC {
		t.Errorf("crc = %08X, want %08X (IEEE)", got, wantCRC)
	}
}

func TestBuildFrameBadPassword(t *testing.T) {
	if _, err := buildFrame(cmdReboot, nil, []byte{1, 2, 3}); err == nil {
		t.Error("buildFrame with 3-byte password: want error, got nil")
	}
}

// A frame built then parsed must round-trip its status and data.
func TestFrameRoundTrip(t *testing.T) {
	payload := []byte{0xDE, 0xAD, 0xBE, 0xEF, 0x11, 0x22}
	frame, err := buildFrame(StatusOK, payload, DefaultPassword())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	resp, err := parseFrame(frame)
	if err != nil {
		t.Fatalf("parseFrame: %v", err)
	}
	if resp.Status != StatusOK {
		t.Errorf("status = %d, want 0", resp.Status)
	}
	if string(resp.Data) != string(payload) {
		t.Errorf("data = %x, want %x", resp.Data, payload)
	}
}

func TestParseFrameRejectsCorruption(t *testing.T) {
	frame, _ := buildFrame(StatusOK, []byte{1, 2, 3, 4}, DefaultPassword())
	bad := append([]byte(nil), frame...)
	bad[5] ^= 0xFF // flip a byte inside the CRC-covered region
	if _, err := parseFrame(bad); err == nil {
		t.Error("parseFrame on corrupted frame: want CRC error, got nil")
	}
	short := frame[:minFrame-1]
	if _, err := parseFrame(short); err == nil {
		t.Error("parseFrame on short frame: want error, got nil")
	}
}

func TestATTXFormat(t *testing.T) {
	line := ATTX([]byte{0xFE, 0x0C, 0xEF})
	if line != "AT+TX=3,FE0CEF\r\n" {
		t.Errorf("ATTX = %q, want %q", line, "AT+TX=3,FE0CEF\r\n")
	}
}

func TestParseATData(t *testing.T) {
	frame, _ := buildFrame(StatusOK, []byte{0xAB}, DefaultPassword())
	line := "<< AT+DATA=" + strings.ToUpper(hex.EncodeToString(frame)) + "\r"
	resp, err := ParseATData(line)
	if err != nil {
		t.Fatalf("ParseATData: %v", err)
	}
	if resp.Status != StatusOK || len(resp.Data) != 1 || resp.Data[0] != 0xAB {
		t.Errorf("resp = %+v, want status 0 data [AB]", resp)
	}
}

func TestParseATDataRejectsNonData(t *testing.T) {
	if _, err := ParseATData("random log line"); err == nil {
		t.Error("ParseATData on non-AT+DATA line: want error, got nil")
	}
}

func TestLoadFirmwareEmpty(t *testing.T) {
	if _, err := LoadFirmware(nil); err == nil {
		t.Error("LoadFirmware(nil): want error")
	}
}

func TestLoadFirmwareTooLarge(t *testing.T) {
	if _, err := LoadFirmware(make([]byte, appMax+1)); err == nil {
		t.Error("LoadFirmware oversized: want error")
	}
}

func TestLoadFirmwareAlignsToEight(t *testing.T) {
	fw, err := LoadFirmware([]byte{1, 2, 3, 4, 5})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if len(fw.Image)%8 != 0 {
		t.Errorf("image len %d not 8-aligned", len(fw.Image))
	}
	if len(fw.Image) != 8 {
		t.Fatalf("len = %d, want 8", len(fw.Image))
	}
	for i := 5; i < 8; i++ {
		if fw.Image[i] != 0xFF {
			t.Errorf("pad byte %d = %#x, want 0xFF", i, fw.Image[i])
		}
	}
	if fw.Addr != AppStart {
		t.Errorf("addr = %#x, want %#x", fw.Addr, AppStart)
	}
}

func TestLoadFirmwareNoMutation(t *testing.T) {
	src := []byte{1, 2, 3, 4, 5}
	fw, _ := LoadFirmware(src)
	fw.Image[0] = 0xFF
	if src[0] != 1 {
		t.Error("LoadFirmware aliased the caller's slice")
	}
}

func TestPlanUpgradeDefaults(t *testing.T) {
	fw, _ := LoadFirmware([]byte{1, 2, 3, 4, 5, 6, 7, 8})
	plan, err := PlanUpgrade(fw, DefaultPassword(), 0)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if plan.ChunkSize != DefaultChunk {
		t.Errorf("chunk = %d, want %d", plan.ChunkSize, DefaultChunk)
	}
	if !plan.EraseFirst || !plan.RebootAfter {
		t.Error("defaults should erase-first and reboot-after")
	}
}

func TestPlanUpgradeBadChunk(t *testing.T) {
	fw, _ := LoadFirmware([]byte{1, 2, 3, 4, 5, 6, 7, 8})
	if _, err := PlanUpgrade(fw, DefaultPassword(), 100); err == nil { // not a multiple of 8
		t.Error("chunk 100: want error")
	}
	if _, err := PlanUpgrade(fw, DefaultPassword(), MaxChunk+8); err == nil {
		t.Error("chunk over max: want error")
	}
}

func TestPlanUpgradeBadPassword(t *testing.T) {
	fw, _ := LoadFirmware([]byte{1, 2, 3, 4, 5, 6, 7, 8})
	if _, err := PlanUpgrade(fw, []byte{1, 2, 3}, 0); err == nil {
		t.Error("3-byte password: want error")
	}
}

// fakeDevice is a frame-accurate bootloader: it decodes each AT+TX request,
// stores FLASH payloads, and verifies the CRC32 on VERIFY — exercising the whole
// pipeline end-to-end.
type fakeDevice struct {
	image      map[uint32]byte
	password   []byte
	sawErase   bool
	sawReboot  bool
	forceError byte // if non-zero, every reply carries this status
}

func newFakeDevice(password []byte) *fakeDevice {
	return &fakeDevice{image: map[uint32]byte{}, password: password}
}

func (d *fakeDevice) Roundtrip(line string, _ time.Duration) (string, error) {
	req, err := decodeATTX(line)
	if err != nil {
		return "", err
	}
	status := d.forceError
	if status == 0 {
		status = d.handle(req)
	}
	reply, err := buildFrame(status, nil, d.password)
	if err != nil {
		return "", err
	}
	return "AT+DATA=" + strings.ToUpper(hex.EncodeToString(reply)), nil
}

func (d *fakeDevice) handle(req parsedReq) byte {
	switch req.cmd {
	case cmdErase:
		d.sawErase = true
	case cmdFlash:
		addr := binary.LittleEndian.Uint32(req.data[0:4])
		n := binary.LittleEndian.Uint32(req.data[4:8])
		payload := req.data[8:]
		if int(n) != len(payload) {
			return 12 // ERR_SIZE
		}
		for i, b := range payload {
			d.image[addr+uint32(i)] = b
		}
	case cmdVerify:
		addr := binary.LittleEndian.Uint32(req.data[0:4])
		size := binary.LittleEndian.Uint32(req.data[4:8])
		wantCRC := binary.LittleEndian.Uint32(req.data[8:12])
		buf := make([]byte, size)
		for i := range buf {
			buf[i] = d.image[addr+uint32(i)]
		}
		if crc32.ChecksumIEEE(buf) != wantCRC {
			return 17 // ERR_VERIFY
		}
	case cmdReboot:
		d.sawReboot = true
	}
	return StatusOK
}

type parsedReq struct {
	cmd  byte
	data []byte
}

func decodeATTX(line string) (parsedReq, error) {
	const marker = "AT+TX="
	i := strings.Index(line, marker)
	if i < 0 {
		return parsedReq{}, fmt.Errorf("no AT+TX in %q", line)
	}
	rest := strings.TrimSpace(line[i+len(marker):])
	_, hexText, ok := strings.Cut(rest, ",")
	if !ok {
		return parsedReq{}, fmt.Errorf("malformed AT+TX line")
	}
	raw, err := hex.DecodeString(hexText)
	if err != nil {
		return parsedReq{}, err
	}
	if len(raw) < minFrame {
		return parsedReq{}, fmt.Errorf("request frame too short")
	}
	dataLen := int(binary.LittleEndian.Uint16(raw[10:12]))
	return parsedReq{cmd: raw[1], data: raw[12 : 12+dataLen]}, nil
}

// Full happy-path upgrade against the fake device: the reconstructed image must
// match and the CRC verify must pass.
func TestUpgradeEndToEnd(t *testing.T) {
	src := make([]byte, DefaultChunk*2+5) // two full chunks + a short one
	for i := range src {
		src[i] = byte(i*7 + 1)
	}
	fw, err := LoadFirmware(src)
	if err != nil {
		t.Fatalf("LoadFirmware: %v", err)
	}
	pw := DefaultPassword()
	plan, err := PlanUpgrade(fw, pw, 0)
	if err != nil {
		t.Fatalf("PlanUpgrade: %v", err)
	}

	dev := newFakeDevice(pw)
	var lastSent, lastTotal int
	if err := Upgrade(dev, plan, func(sent, total int) { lastSent, lastTotal = sent, total }); err != nil {
		t.Fatalf("Upgrade: %v", err)
	}

	if !dev.sawErase || !dev.sawReboot {
		t.Errorf("expected erase and reboot, got erase=%v reboot=%v", dev.sawErase, dev.sawReboot)
	}
	if lastSent != len(fw.Image) || lastTotal != len(fw.Image) {
		t.Errorf("progress ended at %d/%d, want %d/%d", lastSent, lastTotal, len(fw.Image), len(fw.Image))
	}
	for i, b := range fw.Image {
		if got := dev.image[fw.Addr+uint32(i)]; got != b {
			t.Fatalf("flashed byte %d = %#x, want %#x", i, got, b)
			break
		}
	}
}

// A device-reported failure status must abort the upgrade with an error.
func TestUpgradeAbortsOnStatus(t *testing.T) {
	fw, _ := LoadFirmware([]byte{1, 2, 3, 4, 5, 6, 7, 8})
	plan, _ := PlanUpgrade(fw, DefaultPassword(), 0)
	dev := newFakeDevice(DefaultPassword())
	dev.forceError = 16 // ERR_FLASH
	err := Upgrade(dev, plan, nil)
	if err == nil {
		t.Fatal("Upgrade: want error on non-zero status, got nil")
	}
	if !strings.Contains(err.Error(), "status 16") {
		t.Errorf("error = %v, want it to mention status 16", err)
	}
}

// A transport failure must surface as an error, not a silent success.
func TestUpgradeSurfacesTransportError(t *testing.T) {
	fw, _ := LoadFirmware([]byte{1, 2, 3, 4, 5, 6, 7, 8})
	plan, _ := PlanUpgrade(fw, DefaultPassword(), 0)
	err := Upgrade(transportErr{}, plan, nil)
	if !errors.Is(err, errBoom) {
		t.Fatalf("Upgrade error = %v, want errBoom", err)
	}
}

var errBoom = errors.New("boom")

type transportErr struct{}

func (transportErr) Roundtrip(string, time.Duration) (string, error) { return "", errBoom }
