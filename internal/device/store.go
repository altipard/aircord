package device

import (
	"embed"
	"fmt"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strings"

	"github.com/altipard/aircord/internal/config"
)

//go:embed presets/*.toml
var presetFS embed.FS

// Set is the merged collection of device templates: embedded presets with any
// user overrides layered on top. Templates are keyed by Name; a user template
// replaces the preset of the same Name.
type Set struct {
	byName   map[string]*Template
	order    []string // template names, sorted for stable display
	loadErrs []error
}

// Load builds the template set from the embedded presets and, layered on top,
// user TOML files in config.Dir("templates"). Per-file parse failures are
// collected in LoadErrs rather than being fatal, so one malformed file can't
// blank the whole panel.
func Load() *Set { return loadFrom(presetFS, "presets", config.Dir("templates")) }

// loadFrom is the testable core of Load: presets are read from presetsFS under
// presetsDir; user overrides from the OS directory userDir (which may be absent).
func loadFrom(presetsFS fs.FS, presetsDir, userDir string) *Set {
	s := &Set{byName: map[string]*Template{}}
	s.addEmbedded(presetsFS, presetsDir)
	s.addUserDir(userDir)
	s.rebuildOrder()
	return s
}

func (s *Set) addEmbedded(fsys fs.FS, dir string) {
	entries, err := fs.ReadDir(fsys, dir)
	if err != nil {
		return
	}
	for _, e := range entries {
		if e.IsDir() || !strings.HasSuffix(e.Name(), ".toml") {
			continue
		}
		b, err := fs.ReadFile(fsys, path.Join(dir, e.Name()))
		if err != nil {
			s.loadErrs = append(s.loadErrs, err)
			continue
		}
		s.add(e.Name(), b)
	}
}

func (s *Set) addUserDir(dir string) {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return // no user templates dir on first run is fine
	}
	for _, e := range entries {
		if e.IsDir() || !strings.HasSuffix(e.Name(), ".toml") {
			continue
		}
		b, err := os.ReadFile(filepath.Join(dir, e.Name()))
		if err != nil {
			s.loadErrs = append(s.loadErrs, err)
			continue
		}
		s.add(e.Name(), b)
	}
}

// add parses one file's bytes and inserts it. Called for presets first, then user
// files, so a user template of the same Name overrides the preset.
func (s *Set) add(file string, data []byte) {
	t, err := Parse(data)
	if err != nil {
		s.loadErrs = append(s.loadErrs, fmt.Errorf("%s: %w", file, err))
		return
	}
	s.byName[t.Name] = t
}

func (s *Set) rebuildOrder() {
	s.order = s.order[:0]
	for name := range s.byName {
		s.order = append(s.order, name)
	}
	sort.Strings(s.order)
}

// Suggest returns the first template (in stable name order) whose Match regexp
// matches deviceName, or nil when none match.
func (s *Set) Suggest(deviceName string) *Template {
	for _, name := range s.order {
		if t := s.byName[name]; t.Matches(deviceName) {
			return t
		}
	}
	return nil
}

// Get returns the template with the given name, or nil.
func (s *Set) Get(name string) *Template { return s.byName[name] }

// Names lists all template names in stable order for manual selection.
func (s *Set) Names() []string { return append([]string(nil), s.order...) }

// LoadErrs returns any per-file parse errors gathered while building the set.
func (s *Set) LoadErrs() []error { return s.loadErrs }
