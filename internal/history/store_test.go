package history

import (
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

func tempStore(t *testing.T) (*Store, string) {
	t.Helper()
	path := filepath.Join(t.TempDir(), "history.json")
	return newStoreAt(path), path
}

func TestFreshStoreHasNoLoadErr(t *testing.T) {
	s, _ := tempStore(t)
	if err := s.LoadErr(); err != nil {
		t.Fatalf("fresh store LoadErr = %v, want nil", err)
	}
	if got := s.Get("AA"); len(got) != 0 {
		t.Fatalf("unknown addr = %v, want empty", got)
	}
}

func TestAppendThenGet(t *testing.T) {
	s, _ := tempStore(t)
	if err := s.Append("AA", "AT+CFG"); err != nil {
		t.Fatalf("Append: %v", err)
	}
	if err := s.Append("AA", "AT+VER"); err != nil {
		t.Fatalf("Append: %v", err)
	}
	want := []string{"AT+CFG", "AT+VER"}
	if got := s.Get("AA"); !reflect.DeepEqual(got, want) {
		t.Fatalf("Get = %v, want %v", got, want)
	}
}

func TestGetReturnsCopy(t *testing.T) {
	s, _ := tempStore(t)
	_ = s.Append("AA", "one")
	got := s.Get("AA")
	got[0] = "mutated"
	if again := s.Get("AA"); again[0] != "one" {
		t.Fatalf("Get returned an aliased slice: %v", again)
	}
}

func TestAppendSkipsConsecutiveDuplicates(t *testing.T) {
	s, _ := tempStore(t)
	_ = s.Append("AA", "AT")
	_ = s.Append("AA", "AT")
	if got := s.Get("AA"); !reflect.DeepEqual(got, []string{"AT"}) {
		t.Fatalf("Get = %v, want single AT", got)
	}
}

func TestAppendKeepsNonConsecutiveDuplicate(t *testing.T) {
	s, _ := tempStore(t)
	_ = s.Append("AA", "a")
	_ = s.Append("AA", "b")
	_ = s.Append("AA", "a")
	want := []string{"a", "b", "a"}
	if got := s.Get("AA"); !reflect.DeepEqual(got, want) {
		t.Fatalf("Get = %v, want %v", got, want)
	}
}

func TestAppendCapsPerDevice(t *testing.T) {
	s, _ := tempStore(t)
	total := maxPerDevice + 5
	for i := 0; i < total; i++ {
		_ = s.Append("AA", fmt.Sprintf("cmd%d", i))
	}
	got := s.Get("AA")
	if len(got) != maxPerDevice {
		t.Fatalf("len = %d, want %d", len(got), maxPerDevice)
	}
	if got[0] != fmt.Sprintf("cmd%d", total-maxPerDevice) {
		t.Fatalf("oldest kept = %q, want cmd%d", got[0], total-maxPerDevice)
	}
	if got[len(got)-1] != fmt.Sprintf("cmd%d", total-1) {
		t.Fatalf("newest = %q, want cmd%d", got[len(got)-1], total-1)
	}
}

func TestPerDeviceIsolation(t *testing.T) {
	s, _ := tempStore(t)
	_ = s.Append("AA", "for-aa")
	_ = s.Append("BB", "for-bb")
	if got := s.Get("AA"); !reflect.DeepEqual(got, []string{"for-aa"}) {
		t.Fatalf("AA = %v", got)
	}
	if got := s.Get("BB"); !reflect.DeepEqual(got, []string{"for-bb"}) {
		t.Fatalf("BB = %v", got)
	}
}

func TestPersistsAcrossInstances(t *testing.T) {
	s1, path := tempStore(t)
	_ = s1.Append("AA", "persisted")

	s2 := newStoreAt(path)
	if err := s2.LoadErr(); err != nil {
		t.Fatalf("reload LoadErr = %v", err)
	}
	if got := s2.Get("AA"); !reflect.DeepEqual(got, []string{"persisted"}) {
		t.Fatalf("reloaded Get = %v, want [persisted]", got)
	}
}

func TestCorruptFileIsBackedUpAndSurfaced(t *testing.T) {
	path := filepath.Join(t.TempDir(), "history.json")
	if err := os.WriteFile(path, []byte("{ not json"), 0o644); err != nil {
		t.Fatalf("seed corrupt file: %v", err)
	}

	s := newStoreAt(path)
	if s.LoadErr() == nil {
		t.Fatal("LoadErr = nil, want a parse error")
	}
	if _, err := os.Stat(path + ".corrupt"); err != nil {
		t.Fatalf("corrupt backup missing: %v", err)
	}
	// A corrupt load must not wedge the store: it starts empty and still writes.
	if got := s.Get("AA"); len(got) != 0 {
		t.Fatalf("post-corrupt Get = %v, want empty", got)
	}
	if err := s.Append("AA", "recovered"); err != nil {
		t.Fatalf("Append after corrupt load: %v", err)
	}
	if got := newStoreAt(path).Get("AA"); !reflect.DeepEqual(got, []string{"recovered"}) {
		t.Fatalf("recovered store = %v, want [recovered]", got)
	}
}
