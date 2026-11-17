//! Safe file names from user-given names.

/// A file name stem for `name` that cannot escape its directory: lower-cased, every run of characters outside
/// `[a-z0-9._-]` turned into `-`, leading and trailing `-` and `.` stripped, at most 80 bytes; `fallback` when nothing is
/// left. The same rule as earlier releases, so files they saved are found again.
pub fn slug(name: &str, fallback: &str) -> String {
    let lower = name.trim().to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut in_bad = false;
    for c in lower.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-') {
            out.push(c);
            in_bad = false;
        } else if !in_bad {
            out.push('-');
            in_bad = true;
        }
    }
    let trimmed = out.trim_matches(|c| c == '-' || c == '.');
    if trimmed.is_empty() {
        return fallback.to_string();
    }
    // ASCII only by now, so any byte index is a character boundary.
    trimmed[..trimmed.len().min(80)].to_string()
}

#[cfg(test)]
mod tests {
    use super::slug;

    #[test]
    fn names_become_safe_stems() {
        assert_eq!(slug("Mein Playbook", "x"), "mein-playbook");
        assert_eq!(slug("../../etc/passwd", "x"), "etc-passwd");
        assert_eq!(slug("  ", "session"), "session");
        assert_eq!(slug("Grüße ÄÖ", "x"), "gr-e");
        assert_eq!(slug(&"a".repeat(200), "x").len(), 80);
    }
}
