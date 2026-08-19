// Package session persists named working sessions so a scan can be put down and
// picked up later. A session records the selected device and its PIN, a snapshot
// of the discovered device list, and the active control-panel template. Sessions
// are stored as JSON files under the app config dir, one file per session.
package session

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"sync"

	"github.com/altipard/aircord/internal/config"
)

// DeviceSnapshot is one discovered device as recorded at save time. The address
// is a stable key string; it is display/record data — reconnecting still needs a
// live rediscovery to obtain a usable BLE handle.
type DeviceSnapshot struct {
	Addr string `json:"addr"`
	Name string `json:"name"`
	RSSI int16  `json:"rssi"`
}

// Session is a saved working context.
type Session struct {
	Name     string           `json:"name"`
	SelAddr  string           `json:"selectedAddr"`
	SelName  string           `json:"selectedName"`
	PIN      string           `json:"pin"`
	Template string           `json:"template"`
	Devices  []DeviceSnapshot `json:"devices"`
}

// Store reads and writes sessions in a directory. Safe for concurrent use.
type Store struct {
	dir string
	mu  sync.Mutex
}

// NewStore returns a store rooted at config.Dir("sessions").
func NewStore() *Store { return newStoreAt(config.Dir("sessions")) }

func newStoreAt(dir string) *Store { return &Store{dir: dir} }

var slugBad = regexp.MustCompile(`[^a-z0-9._-]+`)

// slug turns a session name into a safe, collision-resistant filename stem. It
// lowercases, replaces runs of unsafe characters with '-', and strips leading or
// trailing separators so a name can never escape the sessions dir (e.g. "../x").
func slug(name string) string {
	s := strings.ToLower(strings.TrimSpace(name))
	s = slugBad.ReplaceAllString(s, "-")
	s = strings.Trim(s, "-.")
	if s == "" {
		return "session"
	}
	if len(s) > 80 {
		s = s[:80]
	}
	return s
}

func (s *Store) path(name string) string {
	return filepath.Join(s.dir, slug(name)+".json")
}

// Save writes a session, overwriting any existing one with the same name. The
// name must be non-empty.
func (s *Store) Save(sess Session) error {
	if strings.TrimSpace(sess.Name) == "" {
		return errors.New("session name is required")
	}
	b, err := json.MarshalIndent(sess, "", "  ")
	if err != nil {
		return err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	// 0600: a session carries the device PIN, so it must not be world-readable.
	return config.AtomicWriteFile(s.path(sess.Name), b, 0o600)
}

// Load reads the session with the given name.
func (s *Store) Load(name string) (Session, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	b, err := os.ReadFile(s.path(name))
	if err != nil {
		return Session{}, err
	}
	var sess Session
	if err := json.Unmarshal(b, &sess); err != nil {
		return Session{}, err
	}
	return sess, nil
}

// List returns the names of all saved sessions, sorted. Unreadable or malformed
// files are skipped rather than failing the whole listing.
func (s *Store) List() []string {
	s.mu.Lock()
	defer s.mu.Unlock()
	entries, err := os.ReadDir(s.dir)
	if err != nil {
		return nil
	}
	var names []string
	for _, e := range entries {
		if e.IsDir() || !strings.HasSuffix(e.Name(), ".json") {
			continue
		}
		b, err := os.ReadFile(filepath.Join(s.dir, e.Name()))
		if err != nil {
			continue
		}
		var sess Session
		if json.Unmarshal(b, &sess) == nil && sess.Name != "" {
			names = append(names, sess.Name)
		}
	}
	sort.Strings(names)
	return names
}

// Delete removes a saved session. Deleting a missing session is not an error.
func (s *Store) Delete(name string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	err := os.Remove(s.path(name))
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	return err
}
