// Package history persists per-device BLE command history to disk so prior
// commands reappear when reconnecting to the same address later.
package history

import (
	"encoding/json"
	"os"
	"sync"

	"gitlab.com/zilicon-it-services/petriheil/aircord/internal/config"
)

const maxPerDevice = 300 // cap stored commands per address

// Store persists per-device command history to a JSON file. Keyed by the device
// address string (a stable CoreBluetooth UUID on macOS).
type Store struct {
	path    string
	mu      sync.Mutex
	data    map[string][]string
	loadErr error // set when an existing history file could not be parsed
}

// NewStore loads the on-disk history (if any) and returns a ready store. A parse
// failure is surfaced via LoadErr rather than returned, so the app still starts.
func NewStore() *Store { return newStoreAt(config.Dir("history.json")) }

// newStoreAt builds a store backed by an explicit path. NewStore wraps it with
// the real config location; tests inject a temp path.
func newStoreAt(path string) *Store {
	hs := &Store{
		path: path,
		data: map[string][]string{},
	}
	hs.load()
	return hs
}

// LoadErr returns the error from parsing an existing history file at startup, or
// nil. On a parse failure the unparseable file is preserved as .corrupt.
func (h *Store) LoadErr() error { return h.loadErr }

func (h *Store) load() {
	h.mu.Lock()
	defer h.mu.Unlock()
	b, err := os.ReadFile(h.path)
	if err != nil {
		return // missing file on first run is fine
	}
	m := map[string][]string{}
	if err := json.Unmarshal(b, &m); err != nil {
		// Preserve the unparseable file under .corrupt so the next save doesn't
		// silently overwrite recoverable data.
		_ = os.Rename(h.path, h.path+".corrupt")
		h.loadErr = err
		return
	}
	h.data = m
}

// Get returns a copy of the stored history for an address.
func (h *Store) Get(addr string) []string {
	h.mu.Lock()
	defer h.mu.Unlock()
	src := h.data[addr]
	out := make([]string, len(src))
	copy(out, src)
	return out
}

// Append records a command for an address (skipping consecutive duplicates) and
// writes the store to disk. It returns any persistence error so the caller can
// surface it instead of failing silently.
func (h *Store) Append(addr, cmd string) error {
	h.mu.Lock()
	list := h.data[addr]
	if n := len(list); n == 0 || list[n-1] != cmd {
		list = append(list, cmd)
	}
	if len(list) > maxPerDevice {
		list = list[len(list)-maxPerDevice:]
	}
	h.data[addr] = list
	h.mu.Unlock()
	return h.save()
}

func (h *Store) save() error {
	h.mu.Lock()
	b, err := json.MarshalIndent(h.data, "", "  ")
	path := h.path
	h.mu.Unlock()
	if err != nil {
		return err
	}
	return config.AtomicWriteFile(path, b, 0o644)
}
