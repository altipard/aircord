package ble

import "time"

// devicePoll is how often AwaitDevice re-reads the discovery store. Adverts
// arrive several times a second, so this adds at most a fraction of a second
// to the reaction time.
const devicePoll = 200 * time.Millisecond

// AwaitDevice waits for a device that has advertised at or after since and
// satisfies match, giving up after timeout or when stop is closed (a nil stop
// never fires). The caller must have started a scan first; this only reads
// what the scan records. ok is false when it gave up.
//
// The since bound matters: the discovery store keeps every device it has ever
// seen, so without it a node that advertised under the wanted name earlier —
// a previous, failed attempt — would be "found" instantly and connected to
// while it is not advertising at all.
func (c *Client) AwaitDevice(match func(Device) bool, since time.Time, timeout time.Duration, stop <-chan struct{}) (Device, bool) {
	tick := time.NewTicker(devicePoll)
	defer tick.Stop()
	fresh := func(d Device) bool { return !d.Seen.Before(since) && match(d) }
	return findDevice(c.Snapshot, fresh, time.After(timeout), tick.C, stop)
}

// findDevice is the control flow behind AwaitDevice: check the snapshot now
// and on every tick, until a match, the timeout, or stop. Pure over its inputs
// so it is unit-tested without a radio.
func findDevice(snapshot func() []Device, match func(Device) bool, timeout, tick <-chan time.Time, stop <-chan struct{}) (Device, bool) {
	for {
		for _, d := range snapshot() {
			if match(d) {
				return d, true
			}
		}
		select {
		case <-tick:
		case <-timeout:
			return Device{}, false
		case <-stop:
			return Device{}, false
		}
	}
}
