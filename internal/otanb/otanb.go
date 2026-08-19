// Package otanb implements the Dragino NB-IoT bootloader firmware-update
// protocol on top of the serial-over-BLE channel package ble already owns.
//
// # Protocol
//
// The protocol is a binary, framed command protocol — NOT a plain hex dump. Each
// command is a frame:
//
//	START(0xFE) | cmd(1) | password(8) | data_len(uint16 LE) | data | crc32(uint32 LE) | END(0xEF)
//
// crc32 is the IEEE/zlib CRC32 over everything from START through data inclusive.
// The frame is carried over the transparent HM-10 (0xFFE1) UART as an ASCII line
// "AT+TX=<len>,<HEX>\r\n" (host -> device); the device replies with a line
// containing "AT+DATA=<HEX>" whose hex decodes to a response frame of the same
// shape, where byte[1] is a status (0 = OK).
//
// This mirrors Dragino's own open-source updater (github.com/dragino/MeshNode,
// tool/FirmwareUpdateUtility), which is the authoritative reference for every
// constant and layout here.
//
// # Flash layout (STM32L072 host MCU)
//
//	0x08000000..0x080077FF  bootloader   (never written by OTA)
//	0x08007800..0x0803DFFF  application  (the .bin lands here; 0x36800 bytes)
//	0x0803E000..0x0803E7FF  factory identity (protected)
//	0x0803F000..            boot control
//
// A .bin must be the raw application image starting at 0x08007800, WITHOUT the
// bootloader. The application start is 8-byte and page (0x800) aligned.
//
// # Upgrade sequence
//
//	[ERASE addr,size] -> FLASH chunks (<=2032 B, multiple of 8) -> VERIFY addr,size,crc32 -> REBOOT
//
// Each step is one request/response round-trip; a non-zero status aborts.
//
// # PIN / password
//
// The 8-byte password field authenticates the frame. Dragino's LA66/mesh tool
// uses a fixed 0x66*8. NB nodes (bootloader v1.3+) put the device AT PIN there,
// right-padded to 8 bytes with 'O' (CONFIRMED from the bootloader Changelog).
// Use DefaultPassword for a no-PIN node and EncodePIN for an NB node.
//
// # What is NB-specific and still unverified against hardware
//
// The frame format, commands, CRC, layout and AT+TX/AT+DATA transport are all
// taken from Dragino's shared STM32 bootloader tool. The NB path additionally
// requires entering the bootloader (reset within an 8 s / 12 s BLE window; the
// node re-advertises under its IMEI) — that reconnect dance lives in the ble/UI
// layer, not here. AT+TX vs any NB-specific transport verb should be confirmed
// on a real device before a production flash; the protocol logic itself is
// exercised end-to-end by the tests against a frame-accurate fake device.
package otanb

import (
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
	"hash/crc32"
	"strings"
	"time"
)

// Frame delimiters.
const (
	startByte = 0xFE
	endByte   = 0xEF
)

// Command opcodes used for a firmware upgrade. The bootloader defines more
// (SYNC=1, RESET=6, GET_APP_VERSION=17, GET_BOOTLOADER_VERSION=19, ...); only the
// flash-path subset is needed here.
const (
	cmdFlash  = 3
	cmdErase  = 4
	cmdVerify = 5
	cmdReboot = 12
)

// StatusOK is the success status byte in a response frame.
const StatusOK = 0

// Flash layout constants (CONFIRMED from the reference tool).
const (
	flashStart = 0x08000000
	flashSize  = 0x40000
	flashEnd   = flashStart + flashSize
	flashPage  = 0x800

	// AppStart is where the application image begins; the host writes here and
	// never touches the bootloader below it.
	AppStart = 0x08007800
	// appEnd is the factory-identity boundary; the application must stay below it.
	appEnd   = 0x0803E000
	appMax   = appEnd - AppStart // 0x36800, ~223 KiB
	minFrame = 17                // START+cmd+password(8)+len(2)+crc(4)+END, data_len 0
)

// Transfer sizing.
const (
	// MaxChunk is the largest FLASH payload the bootloader accepts per frame.
	MaxChunk = 2032
	// DefaultChunk is the reference tool's default FLASH payload size. Must be a
	// multiple of 8.
	DefaultChunk = 224
)

// pinPad is the byte the NB bootloader uses to right-pad a short PIN to the
// 8-byte password field. CONFIRMED from the bootloader Changelog.
const pinPad = 'O'

// ErrProtocol reports a malformed frame or a validation failure.
var ErrProtocol = errors.New("otanb: protocol error")

// DefaultPassword is the fixed 8-byte password for a node without a PIN (the
// value Dragino's LA66/mesh tool uses).
func DefaultPassword() []byte { return []byte{0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66} }

// EncodePIN turns a device AT PIN into the 8-byte password field, right-padding
// with 'O'. It errors if pin is longer than 8 bytes.
func EncodePIN(pin string) ([]byte, error) {
	if len(pin) > 8 {
		return nil, fmt.Errorf("%w: PIN %d chars exceeds 8-byte password field", ErrProtocol, len(pin))
	}
	out := make([]byte, 8)
	n := copy(out, pin)
	for i := n; i < 8; i++ {
		out[i] = pinPad
	}
	return out, nil
}

// crc is the IEEE CRC32 (identical to Python's zlib.crc32) used by the protocol.
func crc(b []byte) uint32 { return crc32.ChecksumIEEE(b) }

// buildFrame assembles one command frame. password must be exactly 8 bytes and
// data at most the frame maximum.
func buildFrame(cmd byte, data, password []byte) ([]byte, error) {
	if len(password) != 8 {
		return nil, fmt.Errorf("%w: password must be 8 bytes, got %d", ErrProtocol, len(password))
	}
	if len(data) > 0xFFFF {
		return nil, fmt.Errorf("%w: data too long: %d", ErrProtocol, len(data))
	}
	head := make([]byte, 0, 12+len(data))
	head = append(head, startByte, cmd)
	head = append(head, password...)
	head = binary.LittleEndian.AppendUint16(head, uint16(len(data)))
	head = append(head, data...)
	out := binary.LittleEndian.AppendUint32(head, crc(head))
	return append(out, endByte), nil
}

// Response is a parsed device reply frame.
type Response struct {
	Status byte
	Data   []byte
}

// parseFrame validates a response frame and extracts its status and data.
func parseFrame(frame []byte) (Response, error) {
	if len(frame) < minFrame {
		return Response{}, fmt.Errorf("%w: frame too short (%d)", ErrProtocol, len(frame))
	}
	if frame[0] != startByte {
		return Response{}, fmt.Errorf("%w: bad start byte", ErrProtocol)
	}
	dataLen := int(binary.LittleEndian.Uint16(frame[10:12]))
	if len(frame) != minFrame+dataLen {
		return Response{}, fmt.Errorf("%w: length mismatch: got %d want %d", ErrProtocol, len(frame), minFrame+dataLen)
	}
	if frame[len(frame)-1] != endByte {
		return Response{}, fmt.Errorf("%w: bad end byte", ErrProtocol)
	}
	body := frame[:12+dataLen]
	wantCRC := binary.LittleEndian.Uint32(frame[12+dataLen : 16+dataLen])
	if got := crc(body); got != wantCRC {
		return Response{}, fmt.Errorf("%w: crc mismatch: got %08X want %08X", ErrProtocol, got, wantCRC)
	}
	return Response{Status: frame[1], Data: append([]byte(nil), frame[12:12+dataLen]...)}, nil
}

// ATTX wraps a command frame in the "AT+TX=<len>,<HEX>\r\n" transport line.
func ATTX(packet []byte) string {
	return fmt.Sprintf("AT+TX=%d,%s\r\n", len(packet), strings.ToUpper(hex.EncodeToString(packet)))
}

// ParseATData extracts and decodes the response frame from a device notify line
// containing "AT+DATA=<HEX>".
func ParseATData(line string) (Response, error) {
	const marker = "AT+DATA="
	i := strings.Index(line, marker)
	if i < 0 {
		return Response{}, fmt.Errorf("%w: not an AT+DATA line", ErrProtocol)
	}
	hexText := strings.Map(func(r rune) rune {
		switch {
		case r >= '0' && r <= '9', r >= 'A' && r <= 'F', r >= 'a' && r <= 'f':
			return r
		default:
			return -1
		}
	}, line[i+len(marker):])
	if hexText == "" || len(hexText)%2 != 0 {
		return Response{}, fmt.Errorf("%w: bad AT+DATA hex", ErrProtocol)
	}
	raw, err := hex.DecodeString(hexText)
	if err != nil {
		return Response{}, fmt.Errorf("%w: %v", ErrProtocol, err)
	}
	return parseFrame(raw)
}

// alignUp rounds n up to the next multiple of a (a>0).
func alignUp(n, a int) int { return (n + a - 1) / a * a }

// Firmware is a validated application image ready to flash.
type Firmware struct {
	Image []byte // 8-byte aligned application bytes
	Addr  uint32 // always AppStart
}

// CRC32 is the checksum the VERIFY step compares against.
func (f Firmware) CRC32() uint32 { return crc(f.Image) }

// LoadFirmware validates a raw .bin application image and pads it to an 8-byte
// boundary with the erased-flash value. It rejects an empty image and one that
// does not fit the application region (a strong sign the .bin still carries the
// bootloader).
func LoadFirmware(image []byte) (Firmware, error) {
	if len(image) == 0 {
		return Firmware{}, fmt.Errorf("%w: empty firmware image", ErrProtocol)
	}
	if len(image) > appMax {
		return Firmware{}, fmt.Errorf("%w: image %d bytes exceeds application region %d — bootloader still in this .bin?", ErrProtocol, len(image), appMax)
	}
	img := image
	if rem := len(img) % 8; rem != 0 {
		pad := make([]byte, 8-rem)
		for i := range pad {
			pad[i] = 0xFF
		}
		img = append(append([]byte(nil), img...), pad...)
	}
	return Firmware{Image: img, Addr: AppStart}, nil
}

// blocks splits the image into chunkSize-byte wire blocks; the final block may
// be shorter.
func (f Firmware) blocks(chunkSize int) [][]byte {
	var out [][]byte
	for i := 0; i < len(f.Image); i += chunkSize {
		end := i + chunkSize
		if end > len(f.Image) {
			end = len(f.Image)
		}
		out = append(out, f.Image[i:end])
	}
	return out
}

// Plan is a fully-derived, deterministic upgrade specification.
type Plan struct {
	Firmware    Firmware
	Password    []byte
	ChunkSize   int
	EraseFirst  bool
	RebootAfter bool
}

// PlanUpgrade builds an upgrade Plan with sane defaults (erase, then flash at the
// default chunk size, verify, reboot). password must be 8 bytes; chunkSize must
// be a positive multiple of 8 no larger than MaxChunk.
func PlanUpgrade(fw Firmware, password []byte, chunkSize int) (Plan, error) {
	if len(password) != 8 {
		return Plan{}, fmt.Errorf("%w: password must be 8 bytes", ErrProtocol)
	}
	if chunkSize <= 0 {
		chunkSize = DefaultChunk
	}
	if chunkSize > MaxChunk || chunkSize%8 != 0 {
		return Plan{}, fmt.Errorf("%w: chunk size %d must be a multiple of 8 in 1..%d", ErrProtocol, chunkSize, MaxChunk)
	}
	return Plan{
		Firmware:    fw,
		Password:    append([]byte(nil), password...),
		ChunkSize:   chunkSize,
		EraseFirst:  true,
		RebootAfter: true,
	}, nil
}

// Transport is the serial-over-BLE round-trip surface the upgrade needs. The
// ble/UI layer implements it over the existing write/notify characteristics;
// keeping it an interface lets the whole flash sequence be tested against a
// frame-accurate fake device.
type Transport interface {
	// Roundtrip writes one transport line (from ATTX) to the device and returns
	// the next reply line (which ParseATData decodes), or an error on timeout.
	Roundtrip(line string, timeout time.Duration) (string, error)
}

// step-level timeouts, matching the reference tool's magnitudes.
const (
	eraseTimeout  = 12 * time.Second
	flashTimeout  = 4 * time.Second
	verifyTimeout = 8 * time.Second
	rebootTimeout = 2 * time.Second
)

// Progress reports flash advancement; sent is bytes written of total.
type Progress func(sent, total int)

// Upgrade runs the full flash sequence for plan over t: optional ERASE, then
// FLASH of every block, then VERIFY against the image CRC32, then optional
// REBOOT. onProgress may be nil. It aborts on the first non-zero status or
// transport error, leaving the node in the bootloader (recoverable by retrying;
// the bootloader region is never written).
func Upgrade(t Transport, plan Plan, onProgress Progress) error {
	fw := plan.Firmware
	total := len(fw.Image)

	if plan.EraseFirst {
		data := binary.LittleEndian.AppendUint32(nil, fw.Addr)
		data = binary.LittleEndian.AppendUint32(data, uint32(alignUp(total, flashPage)))
		if err := roundtripOK(t, cmdErase, data, plan.Password, eraseTimeout, "erase"); err != nil {
			return err
		}
	}

	sent := 0
	for _, block := range fw.blocks(plan.ChunkSize) {
		addr := fw.Addr + uint32(sent)
		data := binary.LittleEndian.AppendUint32(nil, addr)
		data = binary.LittleEndian.AppendUint32(data, uint32(len(block)))
		data = append(data, block...)
		if err := roundtripOK(t, cmdFlash, data, plan.Password, flashTimeout, fmt.Sprintf("flash@0x%08X", addr)); err != nil {
			return err
		}
		sent += len(block)
		if onProgress != nil {
			onProgress(sent, total)
		}
	}

	vdata := binary.LittleEndian.AppendUint32(nil, fw.Addr)
	vdata = binary.LittleEndian.AppendUint32(vdata, uint32(total))
	vdata = binary.LittleEndian.AppendUint32(vdata, fw.CRC32())
	if err := roundtripOK(t, cmdVerify, vdata, plan.Password, verifyTimeout, "verify"); err != nil {
		return err
	}

	if plan.RebootAfter {
		if err := roundtripOK(t, cmdReboot, []byte{0}, plan.Password, rebootTimeout, "reboot"); err != nil {
			return err
		}
	}
	return nil
}

// roundtripOK builds a frame, sends it, and requires a StatusOK reply.
func roundtripOK(t Transport, cmd byte, data, password []byte, timeout time.Duration, what string) error {
	frame, err := buildFrame(cmd, data, password)
	if err != nil {
		return err
	}
	reply, err := t.Roundtrip(ATTX(frame), timeout)
	if err != nil {
		return fmt.Errorf("otanb: %s send failed: %w", what, err)
	}
	resp, err := ParseATData(reply)
	if err != nil {
		return fmt.Errorf("otanb: %s reply unparseable: %w", what, err)
	}
	if resp.Status != StatusOK {
		return fmt.Errorf("otanb: %s rejected with status %d", what, resp.Status)
	}
	return nil
}
