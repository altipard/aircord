package session

import (
	"reflect"
	"testing"
)

func tempStore(t *testing.T) *Store {
	t.Helper()
	return newStoreAt(t.TempDir())
}

func sample() Session {
	return Session{
		Name:     "Example Site",
		SelAddr:  "AABBCC",
		SelName:  "WQS-NB",
		PIN:      "000000",
		Template: "Dragino NB-IoT",
		Devices: []DeviceSnapshot{
			{Addr: "AABBCC", Name: "WQS-NB", RSSI: -61},
			{Addr: "DDEEFF", Name: "D23-NB", RSSI: -80},
		},
	}
}

func TestSaveLoadRoundTrip(t *testing.T) {
	s := tempStore(t)
	want := sample()
	if err := s.Save(want); err != nil {
		t.Fatalf("Save: %v", err)
	}
	got, err := s.Load(want.Name)
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("round trip mismatch:\n got %+v\nwant %+v", got, want)
	}
}

func TestSaveRejectsEmptyName(t *testing.T) {
	s := tempStore(t)
	if err := s.Save(Session{Name: "  "}); err == nil {
		t.Fatal("Save with blank name should error")
	}
}

func TestListSorted(t *testing.T) {
	s := tempStore(t)
	for _, n := range []string{"Zeta", "Alpha", "Mitte"} {
		if err := s.Save(Session{Name: n, Devices: []DeviceSnapshot{{Addr: "X"}}}); err != nil {
			t.Fatal(err)
		}
	}
	got := s.List()
	want := []string{"Alpha", "Mitte", "Zeta"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("List = %v, want %v", got, want)
	}
}

func TestSaveOverwrites(t *testing.T) {
	s := tempStore(t)
	_ = s.Save(Session{Name: "job", PIN: "1111"})
	_ = s.Save(Session{Name: "job", PIN: "2222"})
	got, err := s.Load("job")
	if err != nil {
		t.Fatal(err)
	}
	if got.PIN != "2222" {
		t.Fatalf("PIN = %q, want 2222 (overwrite)", got.PIN)
	}
	if names := s.List(); len(names) != 1 {
		t.Fatalf("overwrite created %d files, want 1", len(names))
	}
}

func TestDelete(t *testing.T) {
	s := tempStore(t)
	_ = s.Save(Session{Name: "gone"})
	if err := s.Delete("gone"); err != nil {
		t.Fatalf("Delete: %v", err)
	}
	if names := s.List(); len(names) != 0 {
		t.Fatalf("List after delete = %v, want empty", names)
	}
	// Deleting a missing session is a no-op, not an error.
	if err := s.Delete("never-existed"); err != nil {
		t.Fatalf("Delete(missing) = %v, want nil", err)
	}
}

func TestLoadMissing(t *testing.T) {
	s := tempStore(t)
	if _, err := s.Load("nope"); err == nil {
		t.Fatal("Load of missing session should error")
	}
}

// A name with path separators and spaces must resolve to a safe file inside the
// sessions dir and still round-trip by its original name.
func TestUnsafeNameStaysContained(t *testing.T) {
	s := tempStore(t)
	tricky := Session{Name: "../../etc/passwd hack", PIN: "x"}
	if err := s.Save(tricky); err != nil {
		t.Fatalf("Save: %v", err)
	}
	got, err := s.Load(tricky.Name)
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if got.PIN != "x" {
		t.Fatalf("PIN = %q, want x", got.PIN)
	}
	// Exactly one file, and it lives directly in the sessions dir.
	if names := s.List(); len(names) != 1 {
		t.Fatalf("List = %v, want single contained file", names)
	}
}
