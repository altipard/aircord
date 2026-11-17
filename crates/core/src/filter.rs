//! The list filter shared by the device list and the history picker.

use regex::RegexBuilder;

/// The items whose key matches `filter`: a case-insensitive regular expression, or, when it is not a valid one, a
/// case-insensitive substring. An empty or blank filter keeps everything.
pub fn matching<T: Clone>(items: &[T], filter: &str, key: impl Fn(&T) -> String) -> Vec<T> {
    let filter = filter.trim();
    if filter.is_empty() {
        return items.to_vec();
    }
    let re = RegexBuilder::new(filter).case_insensitive(true).size_limit(1 << 20).build().ok();
    let lower = filter.to_lowercase();
    items
        .iter()
        .filter(|it| {
            let hay = key(it);
            match &re {
                Some(re) => re.is_match(&hay),
                None => hay.to_lowercase().contains(&lower),
            }
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::ptr_arg)]
    fn id(s: &String) -> String {
        s.clone()
    }

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_keeps_all() {
        let all = v(&["a", "b"]);
        assert_eq!(matching(&all, "  ", id), all);
    }

    #[test]
    fn regex_is_case_insensitive() {
        let all = v(&["D20-NB", "d23", "WQS"]);
        assert_eq!(matching(&all, "^d2", id), v(&["D20-NB", "d23"]));
    }

    #[test]
    fn a_bad_regex_falls_back_to_a_substring() {
        let all = v(&["AT+X(1", "AT+Y"]);
        assert_eq!(matching(&all, "x(", id), v(&["AT+X(1"]));
    }

    #[test]
    fn the_key_decides() {
        let all = vec![("a", "8606"), ("b", "1234")];
        assert_eq!(matching(&all, "8606", |d| d.1.to_string()), vec![("a", "8606")]);
    }
}
