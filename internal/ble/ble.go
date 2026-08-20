// Package ble owns the Bluetooth Low Energy adapter, device discovery, and the
// serial-over-BLE (Nordic UART / HM-10) connection plus AT I/O.
//
// It is UI-agnostic: it never touches widgets and reports activity through the
// Events sink. Events methods are invoked on internal BLE goroutines, so a UI
// adapter is responsible for marshalling them onto its own thread.
package ble

import (
	"bytes"
	"strings"
	"sync"
	"sync/atomic"

	"tinygo.org/x/bluetooth"
)

const mtuChunk = 20 // BLE write chunk size (matches nbtool.py)

// Serial-over-BLE characteristic UUIDs.
var (
	nusRXUUID  = bluetooth.CharacteristicUUIDUARTRX // Nordic: central -> device (write)
	nusTXUUID  = bluetooth.CharacteristicUUIDUARTTX // Nordic: device -> central (notify)
	hmUARTData = bluetooth.New16BitUUID(0xffe1)     // HM-10 / TI CC254x transparent UART
)

// uartProfiles maps serial-over-BLE conventions to their write/notify
// characteristics, tried in order. Dragino D20S-NB uses the HM-10 (0xFFE1)
// profile, not Nordic. Add a new entry from the GATT dump if a device differs.
var uartProfiles = []struct {
	name          string
	write, notify bluetooth.UUID
}{
	{"Nordic", nusRXUUID, nusTXUUID},
	{"HM-10", hmUARTData, hmUARTData}, // 0xFFE1 handles both write and notify
}

// Device is one discovered peripheral.
type Device struct {
	Address bluetooth.Address
	Addr    string
	Name    string
	RSSI    int16
}

// Events receives BLE activity. All methods may be called from internal BLE
// goroutines.
type Events interface {
	// Log reports a protocol/IO line: the GATT dump, ">> "/"<< " traffic, or a
	// "!! " error.
	Log(line string)
	// LinkDropped reports an asynchronous disconnect the app did not initiate
	// (device reset or out of range).
	LinkDropped()
}

// Client owns the adapter, the discovery store, and the active connection.
type Client struct {
	adapter *bluetooth.Adapter
	events  Events

	mu    sync.Mutex
	store map[string]Device // guarded: written by the BLE scan callback

	// scanning is toggled by the caller and read by its repaint loop, so it is
	// atomic rather than a plain bool.
	scanning atomic.Bool

	// enableMu guards the one-time adapter enable, reachable from both the scan
	// and connect goroutines.
	enableMu sync.Mutex
	enabled  bool

	// connection state, guarded by connMu: published on the connect goroutine and
	// read/cleared on the caller thread and the BLE link-drop callback.
	connMu    sync.Mutex
	connected bool
	device    bluetooth.Device
	rxChar    bluetooth.DeviceCharacteristic
	curAddr   string

	// rxBuf is the inbound notify reassembly buffer, touched only on the
	// (serialized) notify goroutine; Connect resets it before enabling
	// notifications so there is no cross-goroutine access.
	rxBuf []byte

	// tap, when non-nil, receives every reassembled notify line in addition to
	// the Events log. An OTA session installs it to await bootloader replies. See
	// ota.go. Guarded by tapMu because it is set on the caller thread and read on
	// the notify goroutine.
	tapMu sync.Mutex
	tap   chan string
}

// New returns a Client bound to the default adapter, reporting to events.
func New(events Events) *Client {
	return &Client{
		adapter: bluetooth.DefaultAdapter,
		events:  events,
		store:   map[string]Device{},
	}
}

// ensureEnabled powers on the adapter once. On macOS this triggers the Bluetooth
// permission prompt on first use. Guarded because both the scan and connect
// goroutines can reach it, and a double enable would register the connect handler
// twice.
func (c *Client) ensureEnabled() error {
	c.enableMu.Lock()
	defer c.enableMu.Unlock()
	if c.enabled {
		return nil
	}
	if err := c.adapter.Enable(); err != nil {
		return err
	}
	c.adapter.SetConnectHandler(c.onConnectChange)
	c.enabled = true
	return nil
}

// onDevice is the BLE scan callback. It fires once per advertisement — many times
// per second per device — so it stays cheap and lock-guarded.
func (c *Client) onDevice(_ *bluetooth.Adapter, r bluetooth.ScanResult) {
	c.record(r.Address, r.Address.String(), r.LocalName(), r.RSSI)
}

// record upserts a discovered device. An empty name never overwrites a known one,
// since adverts sometimes omit the name.
func (c *Client) record(address bluetooth.Address, addr, name string, rssi int16) {
	c.mu.Lock()
	prev, ok := c.store[addr]
	if !ok {
		prev = Device{Addr: addr, Address: address}
	}
	prev.Address = address
	prev.RSSI = rssi
	if name != "" {
		prev.Name = name
	}
	c.store[addr] = prev
	c.mu.Unlock()
}

// Snapshot returns a copy of the discovered devices, unsorted and unfiltered.
func (c *Client) Snapshot() []Device {
	c.mu.Lock()
	defer c.mu.Unlock()
	out := make([]Device, 0, len(c.store))
	for _, d := range c.store {
		out = append(out, d)
	}
	return out
}

// ClearDevices empties the discovery store.
func (c *Client) ClearDevices() {
	c.mu.Lock()
	c.store = map[string]Device{}
	c.mu.Unlock()
}

// IsScanning reports whether discovery is active.
func (c *Client) IsScanning() bool { return c.scanning.Load() }

// Scan powers on the adapter if needed and starts discovery on a background
// goroutine. onError is invoked (on that goroutine) if enabling or scanning
// fails; the scanning flag is cleared before it runs.
func (c *Client) Scan(onError func(error)) {
	if c.scanning.Load() {
		return
	}
	c.scanning.Store(true)
	go func() {
		if err := c.ensureEnabled(); err != nil {
			c.scanning.Store(false)
			if onError != nil {
				onError(err)
			}
			return
		}
		if err := c.adapter.Scan(c.onDevice); err != nil {
			c.scanning.Store(false)
			if onError != nil {
				onError(err)
			}
		}
	}()
}

// StopScan halts discovery. Safe to call when not scanning.
func (c *Client) StopScan() {
	if !c.scanning.Load() {
		return
	}
	c.scanning.Store(false)
	_ = c.adapter.StopScan()
}

// IsConnected reports the current link state under the connection lock.
func (c *Client) IsConnected() bool {
	c.connMu.Lock()
	defer c.connMu.Unlock()
	return c.connected
}

// CurrentAddr returns the connected device's address key under the lock.
func (c *Client) CurrentAddr() string {
	c.connMu.Lock()
	defer c.connMu.Unlock()
	return c.curAddr
}

// Connect powers on the adapter, connects to addr, discovers the UART profile,
// enables notifications, and publishes the live connection. It logs progress and
// errors via Events and returns the matched profile name. ok is false — with a
// "!! …" already logged — on any failure. The caller must stop scanning first.
func (c *Client) Connect(addr bluetooth.Address) (profile string, ok bool) {
	if err := c.ensureEnabled(); err != nil {
		c.events.Log("!! enable failed: " + err.Error())
		return "", false
	}
	device, err := c.adapter.Connect(addr, bluetooth.ConnectionParams{})
	if err != nil {
		c.events.Log("!! connect failed: " + err.Error())
		return "", false
	}
	rx, tx, prof, found := c.discoverUART(device)
	if !found {
		_ = device.Disconnect()
		return "", false
	}
	// Initialise the reassembly buffer before notifications can fire, so the
	// notify goroutine never races this reset.
	c.rxBuf = nil
	if err := tx.EnableNotifications(c.onNotify); err != nil {
		c.events.Log("!! enable notifications failed: " + err.Error())
		_ = device.Disconnect()
		return "", false
	}
	c.setConnected(device, rx, addr.String())
	return prof, true
}

// setConnected publishes the live connection under the lock.
func (c *Client) setConnected(device bluetooth.Device, rx bluetooth.DeviceCharacteristic, addr string) {
	c.connMu.Lock()
	c.connected = true
	c.device = device
	c.rxChar = rx
	c.curAddr = addr
	c.connMu.Unlock()
}

// takeDown clears the connection under the lock and returns the device to close
// plus whether we were connected. Idempotent across a caller-initiated Disconnect
// and the CoreBluetooth link-drop callback.
func (c *Client) takeDown() (bluetooth.Device, bool) {
	c.connMu.Lock()
	defer c.connMu.Unlock()
	if !c.connected {
		return bluetooth.Device{}, false
	}
	c.connected = false
	return c.device, true
}

// writeTarget snapshots the write characteristic and connection state together,
// closing the TOCTOU between the connection check and the write.
func (c *Client) writeTarget() (bluetooth.DeviceCharacteristic, bool) {
	c.connMu.Lock()
	defer c.connMu.Unlock()
	return c.rxChar, c.connected
}

// discoverUART enumerates all GATT characteristics (macOS CoreBluetooth returns
// nothing when DiscoverServices is filtered by UUID), dumps them via Events, and
// matches the first known UART profile.
func (c *Client) discoverUART(device bluetooth.Device) (rx, tx bluetooth.DeviceCharacteristic, profile string, ok bool) {
	svcs, err := device.DiscoverServices(nil)
	if err != nil {
		c.events.Log("!! discover services failed: " + err.Error())
		return
	}

	found := map[bluetooth.UUID]bluetooth.DeviceCharacteristic{}
	c.events.Log("-- GATT --")
	for _, svc := range svcs {
		c.events.Log("  svc " + svc.UUID().String())
		chars, cerr := svc.DiscoverCharacteristics(nil)
		if cerr != nil {
			c.events.Log("    (char discovery error: " + cerr.Error() + ")")
			continue
		}
		for _, ch := range chars {
			c.events.Log("    chr " + ch.UUID().String())
			found[ch.UUID()] = ch
		}
	}

	w, n, name, sel := selectProfile(func(u bluetooth.UUID) bool {
		_, has := found[u]
		return has
	})
	if !sel {
		c.events.Log("!! no known UART profile (Nordic/HM-10) in GATT above")
		return
	}
	return found[w], found[n], name, true
}

// selectProfile returns the write/notify UUIDs and name of the first UART profile
// whose characteristics are all present (per has). Pure — the presence test is
// injected so it can be unit-tested without a live device.
func selectProfile(has func(bluetooth.UUID) bool) (write, notify bluetooth.UUID, name string, ok bool) {
	for _, p := range uartProfiles {
		if has(p.write) && has(p.notify) {
			return p.write, p.notify, p.name, true
		}
	}
	return
}

// Disconnect tears down the active connection. Returns whether a connection was
// actually torn down (false if already disconnected).
func (c *Client) Disconnect() bool {
	device, was := c.takeDown()
	if !was {
		return false
	}
	_ = device.Disconnect()
	return true
}

// onConnectChange fires from CoreBluetooth on every link state change. We only
// care about drops: if the device resets or goes out of range, surface it so the
// UI doesn't look frozen.
func (c *Client) onConnectChange(_ bluetooth.Device, connected bool) {
	if connected {
		return
	}
	if _, was := c.takeDown(); !was {
		return // our own Disconnect(), or a stale event — already handled
	}
	c.events.LinkDropped()
}

// onNotify runs on the BLE goroutine. It reassembles \n-delimited lines and logs
// each one via Events.
func (c *Client) onNotify(data []byte) {
	var lines []string
	c.rxBuf, lines = appendLines(c.rxBuf, data)
	c.tapMu.Lock()
	tap := c.tap
	c.tapMu.Unlock()
	for _, line := range lines {
		c.events.Log("<< " + line)
		if tap != nil {
			// Non-blocking: never stall the BLE goroutine on a slow consumer. The
			// buffer is sized for a lock-step request/response protocol.
			select {
			case tap <- line:
			default:
			}
		}
	}
}

// appendLines appends data to buf and extracts complete '\n'-delimited lines,
// trimming a trailing '\r'. It returns the leftover buffer and the completed
// lines. Pure, so the reassembly logic is unit-tested directly.
func appendLines(buf, data []byte) (rest []byte, lines []string) {
	buf = append(buf, data...)
	for {
		i := bytes.IndexByte(buf, '\n')
		if i < 0 {
			break
		}
		lines = append(lines, strings.TrimRight(string(buf[:i]), "\r"))
		buf = buf[i+1:]
	}
	return buf, lines
}

// Send writes one AT line to the device, chunked to the BLE MTU. It logs the
// outgoing line and any write error via Events. Safe to call from a goroutine: it
// snapshots the connection under the lock first.
func (c *Client) Send(cmd string) { c.send(cmd, cmd) }

// SendSecret is Send for a value that must not appear in the log — a device PIN,
// above all, which would otherwise travel into saved log files and from there
// into shared bug reports. The device receives the command unchanged; only the
// echo is masked.
func (c *Client) SendSecret(cmd string) {
	c.send(cmd, strings.Repeat("*", len([]rune(cmd))))
}

// send writes cmd but logs echo, so a secret can be transmitted without being
// recorded.
func (c *Client) send(cmd, echo string) {
	rx, ok := c.writeTarget()
	if !ok {
		return
	}
	c.events.Log(">> " + echo)
	data := []byte(cmd + "\r\n")
	for i := 0; i < len(data); i += mtuChunk {
		end := i + mtuChunk
		if end > len(data) {
			end = len(data)
		}
		if _, err := rx.WriteWithoutResponse(data[i:end]); err != nil {
			c.events.Log("!! write error: " + err.Error())
			return
		}
	}
}
