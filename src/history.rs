//! The searches typed in the `/` and `?` bars, and the browsing of them with `edit_history_prev` and `edit_history_next`.

/// How many searches are kept.
pub const MAX: usize = 100;

/// Where a walk through the history started, and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Browse {
    /// What the field held before the first step; entries must start with it, and the walk ends back on it.
    typed: String,
    /// An index into the entries; one past the end is the typed text.
    at: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    /// Oldest first.
    entries: Vec<String>,
    browse: Option<Browse>,
}

impl History {
    /// The history written as `render` makes it: one search per line, oldest first.
    pub fn parse(text: &str) -> Self {
        let mut history = Self::default();
        for line in text.lines().filter(|l| !l.is_empty()) {
            history.push(line);
        }
        history
    }

    /// One search per line.
    pub fn render(&self) -> String {
        self.entries.iter().map(|e| format!("{e}\n")).collect()
    }

    /// Adds a search as the newest, dropping an earlier copy and the oldest beyond `MAX`, and ends browsing.
    pub fn push(&mut self, text: &str) {
        self.browse = None;
        if text.is_empty() || text.contains(['\n', '\r']) {
            return;
        }
        self.entries.retain(|e| e != text);
        self.entries.push(text.to_owned());
        let extra = self.entries.len().saturating_sub(MAX);
        self.entries.drain(..extra);
    }

    /// Ends browsing, as editing the field does.
    pub fn stop(&mut self) {
        self.browse = None;
    }

    /// The next older entry, given what the field holds now, if there is one.
    pub fn prev(&mut self, current: &str) -> Option<String> {
        let browse = self.browse.get_or_insert_with(|| Browse { typed: current.to_owned(), at: self.entries.len() });
        let found = (0..browse.at).rev().find(|&i| self.entries[i].starts_with(&browse.typed))?;
        browse.at = found;
        Some(self.entries[found].clone())
    }

    /// The next newer entry, or past the newest what was typed before browsing, if browsing.
    pub fn next(&mut self) -> Option<String> {
        let browse = self.browse.as_mut()?;
        if browse.at >= self.entries.len() {
            return None;
        }
        let found = (browse.at + 1..self.entries.len()).find(|&i| self.entries[i].starts_with(&browse.typed));
        browse.at = found.unwrap_or(self.entries.len());
        Some(found.map_or_else(|| browse.typed.clone(), |i| self.entries[i].clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(entries: &[&str]) -> History {
        let mut history = History::default();
        for e in entries {
            history.push(e);
        }
        history
    }

    #[test]
    fn a_repeated_search_moves_to_the_end() {
        let history = with(&["a", "b", "a"]);
        assert_eq!(history.entries, ["b", "a"]);
    }

    #[test]
    fn empty_searches_are_not_kept() {
        assert!(with(&[""]).entries.is_empty());
    }

    #[test]
    fn it_keeps_the_newest_hundred() {
        let all: Vec<String> = (0..150).map(|i| i.to_string()).collect();
        let mut history = History::default();
        for e in &all {
            history.push(e);
        }
        assert_eq!(history.entries.len(), MAX);
        assert_eq!(history.entries[0], "50");
        assert_eq!(history.entries[MAX - 1], "149");
    }

    #[test]
    fn it_round_trips_through_text() {
        let history = with(&["one", "two words", "three"]);
        assert_eq!(History::parse(&history.render()), history);
        assert_eq!(History::parse("").entries.len(), 0);
        assert_eq!(History::parse("a\n\nb\na\n").entries, ["b", "a"]);
    }

    #[test]
    fn it_walks_older_and_back_to_what_was_typed() {
        let mut history = with(&["a", "b", "c"]);
        assert_eq!(history.prev(""), Some("c".to_owned()));
        assert_eq!(history.prev("c"), Some("b".to_owned()));
        assert_eq!(history.next(), Some("c".to_owned()));
        assert_eq!(history.next(), Some(String::new()));
        assert_eq!(history.next(), None);
    }

    #[test]
    fn it_stays_on_the_oldest() {
        let mut history = with(&["a", "b"]);
        assert_eq!(history.prev(""), Some("b".to_owned()));
        assert_eq!(history.prev("b"), Some("a".to_owned()));
        assert_eq!(history.prev("a"), None);
        assert_eq!(history.next(), Some("b".to_owned()));
    }

    #[test]
    fn typed_text_filters_by_prefix() {
        let mut history = with(&["foo", "bar", "fab", "baz"]);
        assert_eq!(history.prev("f"), Some("fab".to_owned()));
        assert_eq!(history.prev("fab"), Some("foo".to_owned()));
        assert_eq!(history.prev("foo"), None);
        assert_eq!(history.next(), Some("fab".to_owned()));
        assert_eq!(history.next(), Some("f".to_owned()));
    }

    #[test]
    fn next_does_nothing_unless_browsing_and_editing_ends_it() {
        let mut history = with(&["a", "b"]);
        assert_eq!(history.next(), None);
        history.prev("");
        history.stop();
        assert_eq!(history.next(), None);
        assert_eq!(history.prev("x"), None, "a new walk filters by the new text");
    }

    #[test]
    fn no_match_leaves_the_field_alone() {
        let mut history = with(&["a"]);
        assert_eq!(history.prev("z"), None);
        assert_eq!(history.next(), None);
    }
}
