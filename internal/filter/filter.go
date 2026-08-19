// Package filter provides the shared list-filtering helpers used by both the
// device list and the command-history picker.
package filter

import (
	"regexp"
	"strings"
)

// Match keeps the items whose key matches f. The filter is a case-insensitive
// regular expression; when f is not a valid regex it falls back to a
// case-insensitive substring match. An empty (or whitespace-only) filter
// matches everything.
//
// It backs both the device-list filter and the command-history filter, which
// previously carried near-identical copies.
func Match[T any](in []T, f string, key func(T) string) []T {
	f = strings.TrimSpace(f)
	if f == "" {
		return in
	}
	re, reErr := regexp.Compile("(?i)" + f)
	lc := strings.ToLower(f)
	out := make([]T, 0, len(in))
	for _, it := range in {
		hay := key(it)
		if reErr == nil {
			if re.MatchString(hay) {
				out = append(out, it)
			}
		} else if strings.Contains(strings.ToLower(hay), lc) {
			out = append(out, it)
		}
	}
	return out
}

// Reversed returns a new slice holding the elements of in in reverse order.
func Reversed(in []string) []string {
	out := make([]string, len(in))
	for i, v := range in {
		out[len(in)-1-i] = v
	}
	return out
}
