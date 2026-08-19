// Package playbook persists named command sequences ("playbooks") that replay a
// fixed series of AT commands with an optional reply-wait and pause per step.
// Playbooks are stored as JSON files under the app config dir, one file per
// playbook.
//
// The text format used by the editor is one command per line. A " ?<pattern>"
// suffix makes the runner wait until a device reply matches the pattern (a bare
// " ?" waits for "OK"); an "@<ms>" suffix adds a fixed pause after the step:
//
//	AT+CFG
//	AT+APN=iot.1nce.net ?OK
//	AT+TDC=3600 ? @1500
//	# lines starting with # are comments
package playbook

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/altipard/aircord/internal/config"
)

// Step is one command in a playbook, an optional reply pattern to wait for after
// sending it, and how long to additionally pause once the reply arrived.
type Step struct {
	Cmd     string `json:"cmd"`
	WaitFor string `json:"waitFor,omitempty"` // regexp; "" = don't wait for a reply
	WaitMs  int    `json:"waitMs"`
}

// Playbook is a named, ordered sequence of steps.
type Playbook struct {
	Name  string `json:"name"`
	Steps []Step `json:"steps"`
}

// Store reads and writes playbooks in a directory. Safe for concurrent use.
type Store struct {
	dir string
	mu  sync.Mutex
}

// NewStore returns a store rooted at config.Dir("playbooks").
func NewStore() *Store { return newStoreAt(config.Dir("playbooks")) }

// NewStoreAt returns a store rooted at an arbitrary directory. Tests use it to
// work against a temp dir instead of the user's real playbooks.
func NewStoreAt(dir string) *Store { return newStoreAt(dir) }

func newStoreAt(dir string) *Store { return &Store{dir: dir} }

var slugBad = regexp.MustCompile(`[^a-z0-9._-]+`)

// slug turns a name into a safe filename stem that cannot escape the dir.
func slug(name string) string {
	s := strings.ToLower(strings.TrimSpace(name))
	s = slugBad.ReplaceAllString(s, "-")
	s = strings.Trim(s, "-.")
	if s == "" {
		return "playbook"
	}
	if len(s) > 80 {
		s = s[:80]
	}
	return s
}

func (s *Store) path(name string) string {
	return filepath.Join(s.dir, slug(name)+".json")
}

// Save writes a playbook, overwriting any with the same name. Name is required.
func (s *Store) Save(pb Playbook) error {
	if strings.TrimSpace(pb.Name) == "" {
		return errors.New("playbook name is required")
	}
	b, err := json.MarshalIndent(pb, "", "  ")
	if err != nil {
		return err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return config.AtomicWriteFile(s.path(pb.Name), b, 0o644)
}

// Load reads the playbook with the given name.
func (s *Store) Load(name string) (Playbook, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	b, err := os.ReadFile(s.path(name))
	if err != nil {
		return Playbook{}, err
	}
	var pb Playbook
	if err := json.Unmarshal(b, &pb); err != nil {
		return Playbook{}, err
	}
	return pb, nil
}

// List returns the names of all saved playbooks, sorted.
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
		var pb Playbook
		if json.Unmarshal(b, &pb) == nil && pb.Name != "" {
			names = append(names, pb.Name)
		}
	}
	sort.Strings(names)
	return names
}

// Delete removes a playbook. Deleting a missing playbook is not an error.
func (s *Store) Delete(name string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	err := os.Remove(s.path(name))
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	return err
}

// ParseSteps turns the editor text format into steps. Blank lines and lines
// starting with '#' are ignored; a trailing "@<ms>" sets the pause after a step
// and a " ?<pattern>" suffix sets the reply pattern to wait for (bare " ?"
// means "OK").
func ParseSteps(text string) []Step {
	var steps []Step
	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		wait := 0
		if i := strings.LastIndex(line, "@"); i >= 0 {
			if ms, err := strconv.Atoi(strings.TrimSpace(line[i+1:])); err == nil && ms >= 0 {
				wait = ms
				line = strings.TrimSpace(line[:i])
			}
		}
		waitFor := ""
		if i := strings.LastIndex(line, " ?"); i >= 0 {
			waitFor = strings.TrimSpace(line[i+2:])
			if waitFor == "" {
				waitFor = "OK"
			}
			line = strings.TrimSpace(line[:i])
		}
		if line == "" {
			continue
		}
		steps = append(steps, Step{Cmd: line, WaitFor: waitFor, WaitMs: wait})
	}
	return steps
}

// FormatSteps renders steps back into the editor text format (inverse of
// ParseSteps for well-formed steps), so an existing playbook can be edited.
func FormatSteps(steps []Step) string {
	var b strings.Builder
	for _, st := range steps {
		b.WriteString(st.Cmd)
		if st.WaitFor != "" {
			b.WriteString(" ?")
			b.WriteString(st.WaitFor)
		}
		if st.WaitMs > 0 {
			b.WriteString(" @")
			b.WriteString(strconv.Itoa(st.WaitMs))
		}
		b.WriteByte('\n')
	}
	return b.String()
}

// CompileWait compiles a step's WaitFor pattern. An invalid regexp falls back to
// a literal substring match, so a typo can never make a playbook unrunnable.
func CompileWait(pattern string) *regexp.Regexp {
	re, err := regexp.Compile(pattern)
	if err != nil {
		return regexp.MustCompile(regexp.QuoteMeta(pattern))
	}
	return re
}

// AwaitMatch reads device reply lines until one matches re, the timeout elapses,
// or connected reports false. It returns true only on a match. The connected
// probe is polled so a dropped link aborts the wait early instead of running the
// timeout down.
func AwaitMatch(lines <-chan string, re *regexp.Regexp, timeout time.Duration, connected func() bool) bool {
	deadline := time.NewTimer(timeout)
	defer deadline.Stop()
	link := time.NewTicker(500 * time.Millisecond)
	defer link.Stop()
	for {
		select {
		case l := <-lines:
			if re.MatchString(l) {
				return true
			}
		case <-link.C:
			if connected != nil && !connected() {
				return false
			}
		case <-deadline.C:
			return false
		}
	}
}

// EnsureDefault saves pb only when no playbook with its name exists yet, so
// shipped defaults never overwrite a user's edits (or their deletion is
// respected only until the next start — deleting a default brings it back).
func (s *Store) EnsureDefault(pb Playbook) {
	s.mu.Lock()
	_, err := os.Stat(s.path(pb.Name))
	s.mu.Unlock()
	if err == nil {
		return
	}
	_ = s.Save(pb)
}
