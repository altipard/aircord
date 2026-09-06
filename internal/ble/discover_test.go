package ble

import (
	"testing"
	"time"

	"tinygo.org/x/bluetooth"
)

func named(name string) func(Device) bool {
	return func(d Device) bool { return d.Name == name }
}

func TestFindDeviceImmediate(t *testing.T) {
	snap := func() []Device { return []Device{{Addr: "AA", Name: "other"}, {Addr: "BB", Name: "want"}} }
	d, ok := findDevice(snap, named("want"), nil, nil, nil)
	if !ok || d.Addr != "BB" {
		t.Fatalf("got (%+v, %v), want BB", d, ok)
	}
}

func TestFindDeviceAppearsLater(t *testing.T) {
	calls := 0
	snap := func() []Device {
		calls++
		if calls < 3 {
			return nil
		}
		return []Device{{Addr: "BB", Name: "want"}}
	}
	tick := make(chan time.Time, 8)
	for i := 0; i < 8; i++ {
		tick <- time.Now()
	}
	d, ok := findDevice(snap, named("want"), nil, tick, nil)
	if !ok || d.Addr != "BB" {
		t.Fatalf("got (%+v, %v), want BB after ticks", d, ok)
	}
	if calls != 3 {
		t.Errorf("snapshot called %d times, want 3", calls)
	}
}

func TestFindDeviceTimeout(t *testing.T) {
	fired := make(chan time.Time, 1)
	fired <- time.Now()
	if _, ok := findDevice(func() []Device { return nil }, named("want"), fired, nil, nil); ok {
		t.Fatal("found a device on an empty store")
	}
}

func TestFindDeviceStop(t *testing.T) {
	stop := make(chan struct{})
	close(stop)
	if _, ok := findDevice(func() []Device { return nil }, named("want"), nil, nil, stop); ok {
		t.Fatal("ok = true after stop")
	}
}

// A device recorded before `since` must not satisfy AwaitDevice, one recorded
// after it must — that is the whole point of the bound.
func TestAwaitDeviceIgnoresStaleEntry(t *testing.T) {
	c := New(nil)
	c.record(bluetooth.Address{}, "AA", "860600000000001", -50)
	since := time.Now().Add(time.Millisecond)

	if _, ok := c.AwaitDevice(named("860600000000001"), since, 10*time.Millisecond, nil); ok {
		t.Fatal("stale entry satisfied AwaitDevice")
	}

	done := make(chan struct{})
	go func() {
		time.Sleep(5 * time.Millisecond)
		c.record(bluetooth.Address{}, "AA", "860600000000001", -45) // re-advertised
		close(done)
	}()
	d, ok := c.AwaitDevice(named("860600000000001"), since, 2*time.Second, nil)
	<-done
	if !ok || d.Addr != "AA" {
		t.Fatalf("fresh advert not found: (%+v, %v)", d, ok)
	}
}
