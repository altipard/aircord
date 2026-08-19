package filter

import (
	"reflect"
	"testing"
)

func id(s string) string { return s }

func TestMatchEmptyReturnsAll(t *testing.T) {
	in := []string{"AT+CFG", "AT+VER"}
	for _, f := range []string{"", "   "} {
		got := Match(in, f, id)
		if !reflect.DeepEqual(got, in) {
			t.Fatalf("filter %q: got %v, want %v", f, got, in)
		}
	}
}

func TestMatchRegexCaseInsensitive(t *testing.T) {
	in := []string{"AT+CFG", "AT+VER", "reset"}
	got := Match(in, "^at", id)
	want := []string{"AT+CFG", "AT+VER"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got %v, want %v", got, want)
	}
}

func TestMatchSubstringFallbackOnBadRegex(t *testing.T) {
	// "A(" is not a valid regex → case-insensitive substring fallback.
	in := []string{"foo a( bar", "nope"}
	got := Match(in, "A(", id)
	want := []string{"foo a( bar"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got %v, want %v", got, want)
	}
}

func TestMatchKeyExtractor(t *testing.T) {
	type kv struct{ name, addr string }
	in := []kv{{"D20S-NB", "AA"}, {"other", "BB"}}
	got := Match(in, "d20", func(d kv) string { return d.name + " " + d.addr })
	if len(got) != 1 || got[0].name != "D20S-NB" {
		t.Fatalf("got %v", got)
	}
}

func TestReversed(t *testing.T) {
	got := Reversed([]string{"a", "b", "c"})
	want := []string{"c", "b", "a"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got %v, want %v", got, want)
	}
	if r := Reversed(nil); len(r) != 0 {
		t.Fatalf("Reversed(nil) = %v", r)
	}
}
