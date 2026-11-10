//! The list of recent documents, shown over the blank page and by `:recent`.
use std::path::{Path, PathBuf};

use egui::{CursorIcon, Response};

use crate::app::{App, Mode};
use crate::hint;
use crate::keymap::{Action, Stroke};
use crate::store;

/// Keys the list uses for moving, so that no row's label is made of them.
const MOVE_KEYS: [char; 2] = ['j', 'k'];

/// Actions the list passes on to the keymap, whose single keys no label is made of either.
const PASSED_ON: [Action; 4] = [Action::Command, Action::Quit, Action::Open, Action::ToggleFullscreen];

#[derive(Clone, Debug)]
pub struct RecentState {
    pub files: Vec<PathBuf>,
    /// The hint label of each file.
    pub labels: Vec<String>,
    pub selected: usize,
    /// The start of a label, typed so far.
    pub typed: String,
}

impl RecentState {
    /// `bound` are the keys the keymap gives its own meaning here. Labels are made of the alphabet
    /// without them and the moving keys, or of the digits when fewer than two letters are left.
    pub fn new(files: Vec<PathBuf>, alphabet: &str, bound: &[char]) -> Self {
        let free = |chars: &mut dyn Iterator<Item = char>| {
            let mut letters: Vec<char> = Vec::new();
            for c in chars.filter(|c| !MOVE_KEYS.contains(c) && !bound.contains(c)) {
                if !letters.contains(&c) {
                    letters.push(c);
                }
            }
            letters
        };
        let mut letters = free(&mut alphabet.chars());
        if letters.len() < 2 {
            letters = free(&mut ('1'..='9'));
        }
        if letters.len() < 2 {
            // Every digit but one is bound too: the digits, bound or not, are all there is.
            letters = ('1'..='9').collect();
        }
        let labels = hint::labels(files.len(), &letters);
        Self { files, labels, selected: 0, typed: String::new() }
    }
}

/// `path` with `home` at its start shown as `~`.
pub fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => {
            let sep = std::path::MAIN_SEPARATOR.to_string();
            let parts: Vec<_> = rest.components().map(|c| c.as_os_str().to_string_lossy()).collect();
            format!("~{sep}{}", parts.join(&sep))
        }
        Err(_) => path.display().to_string(),
    }
}

impl App {
    /// Shows the recent files, over whatever is open.
    pub fn open_recent(&mut self) {
        let files = store::recent_documents();
        if files.is_empty() {
            self.say("no recent files");
        } else {
            self.mode = Mode::Recent(self.recent_state(files));
        }
    }

    /// The list of `files`, labelled from the hint alphabet.
    pub fn recent_state(&self, files: Vec<PathBuf>) -> RecentState {
        RecentState::new(files, &self.config.hints.alphabet, &self.keymap.typed_by(&PASSED_ON))
    }

    /// Opens a row. A file that won't open leaves the list as it was, with the reason in the status bar.
    fn recent_go(&mut self, ctx: &egui::Context, row: usize) {
        let Mode::Recent(state) = &self.mode else { return };
        let Some(path) = state.files.get(row).cloned() else { return };
        self.recent_return = true;
        if let Err(e) = self.open(ctx, &path) {
            self.recent_return = false;
            self.say(e);
            if let Mode::Recent(state) = &mut self.mode {
                state.typed.clear();
            }
        } else if !matches!(self.mode, Mode::Password(_)) {
            self.recent_return = false;
        }
    }

    pub fn recent_key(&mut self, ctx: &egui::Context, stroke: &Stroke) {
        let Mode::Recent(state) = &mut self.mode else { return };
        let last = state.files.len().saturating_sub(1);
        if self.keymap.cancels(stroke) {
            if state.typed.is_empty() {
                self.mode = Mode::Normal;
            } else {
                state.typed.clear();
            }
        } else if stroke.is("bs") {
            state.typed.pop();
        } else if stroke.is("cr") {
            let row = state.selected;
            self.recent_go(ctx, row);
        } else if stroke.is_ctrl("n") || stroke.is("down") || stroke.text() == Some('j') {
            state.selected = (state.selected + 1).min(last);
        } else if stroke.is_ctrl("p") || stroke.is("up") || stroke.text() == Some('k') {
            state.selected = state.selected.saturating_sub(1);
        } else if let Some(c) = stroke.text().filter(|c| state.labels.iter().any(|l| l.contains(*c))) {
            state.typed.push(c);
            if !state.labels.iter().any(|l| l.starts_with(&state.typed)) {
                state.typed.pop();
            } else if let Some(row) = state.labels.iter().position(|l| *l == state.typed) {
                self.recent_go(ctx, row);
            }
        } else if self.keymap.is(stroke, Action::Command) {
            self.mode = Mode::Normal;
            self.run(ctx, Action::Command, None);
        } else if let Some(action) = [Action::Quit, Action::ToggleFullscreen, Action::Open].into_iter().find(|a| self.keymap.is(stroke, *a)) {
            self.run(ctx, action, None);
        }
    }

    /// A click on a row opens it; one outside the list closes it. The page gets no clicks meanwhile.
    pub fn pointer_recent(&mut self, ctx: &egui::Context, response: &Response) {
        ctx.set_cursor_icon(CursorIcon::Default);
        let Some(at) = response.interact_pointer_pos().filter(|_| response.clicked()) else { return };
        let Mode::Recent(state) = &self.mode else { return };
        let layout = self.list_layout(true, false, state.files.len(), state.selected, None);
        if !layout.rect.contains(at) {
            self.mode = Mode::Normal;
            self.choice_closed = Some((ctx.input(|i| i.time), at));
            return;
        }
        let hit = (layout.first..state.files.len()).take(layout.fits).find(|&i| Self::list_row_rect(&layout, i).contains(at));
        if let Some(row) = hit {
            self.recent_go(ctx, row);
            if matches!(self.mode, Mode::Normal) {
                // Opened by a click: its double click must not land on the new page.
                self.choice_closed = Some((ctx.input(|i| i.time), at));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `text` with its slashes as this platform writes them.
    fn sep(text: &str) -> String {
        text.replace('/', std::path::MAIN_SEPARATOR_STR)
    }

    #[test]
    fn home_is_shown_as_a_tilde() {
        let home = Path::new("/home/me");
        assert_eq!(tilde(Path::new("/home/me/Documents"), home), sep("~/Documents"));
        assert_eq!(tilde(Path::new("/home/me"), home), "~");
        assert_eq!(tilde(Path::new("/home/ment/x"), home), "/home/ment/x");
        assert_eq!(tilde(Path::new("/srv/pdf"), home), "/srv/pdf");
    }

    #[test]
    fn the_labels_leave_out_the_moving_keys() {
        let files: Vec<PathBuf> = (0..10).map(|i| PathBuf::from(format!("/{i}.pdf"))).collect();
        let state = RecentState::new(files, "asdfghjklqwertyuiopzxcvbnm", &[]);
        assert_eq!(state.labels.len(), 10);
        assert!(state.labels.iter().all(|l| !l.contains(['j', 'k'])));
        assert_eq!(state.labels[0], "a");
    }

    fn files(n: usize) -> Vec<PathBuf> {
        (0..n).map(|i| PathBuf::from(format!("/{i}.pdf"))).collect()
    }

    fn prefix_free_and_unique(labels: &[String]) -> bool {
        labels.iter().enumerate().all(|(i, a)| labels.iter().enumerate().all(|(j, b)| i == j || (a != b && !b.starts_with(a.as_str()))))
    }

    #[test]
    fn labels_are_unique_and_prefix_free_for_every_count_with_the_default_alphabet() {
        for n in 0..=crate::store::RECENT_MAX {
            let state = RecentState::new(files(n), "asdfghjklqwertyuiopzxcvbnm", &[]);
            assert_eq!(state.labels.len(), n);
            assert!(prefix_free_and_unique(&state.labels), "{:?}", state.labels);
            assert!(state.labels.iter().all(|l| !l.contains(['j', 'k', ':'])));
        }
    }

    #[test]
    fn labels_stay_prefix_free_with_small_alphabets() {
        for alphabet in ["jkab", "ab", "abc", "jkabc", "a:b", "jkab:"] {
            for n in 1..=crate::store::RECENT_MAX {
                let state = RecentState::new(files(n), alphabet, &[':']);
                assert_eq!(state.labels.len(), n, "{alphabet}");
                assert!(prefix_free_and_unique(&state.labels), "{alphabet} {n}: {:?}", state.labels);
                assert!(state.labels.iter().all(|l| !l.contains(['j', 'k', ':'])), "{alphabet} {n}: {:?}", state.labels);
            }
        }
        // "ab" needs two letters for 10 rows.
        assert_eq!(RecentState::new(files(10), "ab", &[]).labels[0].len(), 4);
    }

    #[test]
    fn the_default_config_leaves_q_colon_j_and_k_out_of_the_labels() {
        let config = crate::config::Config::load_str("").unwrap();
        let (keymap, _) = crate::keymap::Keymap::new(&config.keys);
        let bound = keymap.typed_by(&PASSED_ON);
        for key in [':', 'q'] {
            assert!(bound.contains(&key), "{key}");
        }
        for n in 0..=crate::store::RECENT_MAX {
            let state = RecentState::new(files(n), &config.hints.alphabet, &bound);
            assert_eq!(state.labels.len(), n);
            assert!(prefix_free_and_unique(&state.labels), "{:?}", state.labels);
            assert!(state.labels.iter().all(|l| !l.contains(['q', ':', 'j', 'k'])), "{:?}", state.labels);
        }
    }

    #[test]
    fn an_alphabet_left_with_fewer_than_two_letters_falls_back_to_the_digits() {
        // "jka" leaves one letter once j and k are removed.
        let state = RecentState::new(files(3), "jka", &[]);
        assert_eq!(state.labels, ["1", "2", "3"]);
        // "jk" leaves none; bound keys are left out of the digits too.
        let state = RecentState::new(files(3), "jk", &['1', '3']);
        assert_eq!(state.labels, ["2", "4", "5"]);
        // Repeated letters count once.
        let state = RecentState::new(files(2), "aaa", &[]);
        assert_eq!(state.labels, ["1", "2"]);
        for n in 1..=crate::store::RECENT_MAX {
            let state = RecentState::new(files(n), "", &['5']);
            assert_eq!(state.labels.len(), n);
            assert!(prefix_free_and_unique(&state.labels), "{:?}", state.labels);
            assert!(state.labels.iter().all(|l| !l.contains('5')));
        }
    }

    #[test]
    fn labels_avoid_letters_a_custom_keymap_binds_to_quit_and_friends() {
        let config = crate::config::Config::load_str(
            "[keys]\nquit = [\"a\", \"s\", \"ZZ\"]\nopen = [\"d\"]\ncommand = [\"f\", \";\"]\ntoggle_fullscreen = [\"<C-f>\", \"g\"]",
        )
        .unwrap();
        let (keymap, errors) = crate::keymap::Keymap::new(&config.keys);
        assert!(errors.is_empty(), "{errors:?}");
        let bound = keymap.typed_by(&PASSED_ON);
        for key in ['a', 's', 'd', 'f', ';', 'g'] {
            assert!(bound.contains(&key), "{key}");
        }
        assert!(!bound.contains(&'Z'));
        for n in 0..=crate::store::RECENT_MAX {
            let state = RecentState::new(files(n), &config.hints.alphabet, &bound);
            assert_eq!(state.labels.len(), n);
            assert!(prefix_free_and_unique(&state.labels), "{:?}", state.labels);
            assert!(state.labels.iter().all(|l| !l.contains(['a', 's', 'd', 'f', 'g', 'j', 'k'])), "{:?}", state.labels);
        }
    }

    #[test]
    fn labels_fall_back_to_digits_when_quit_eats_the_alphabet() {
        let config = crate::config::Config::load_str("[hints]\nalphabet = \"abjk\"\n[keys]\nquit = [\"a\", \"b\"]").unwrap();
        let (keymap, _) = crate::keymap::Keymap::new(&config.keys);
        let bound = keymap.typed_by(&PASSED_ON);
        let state = RecentState::new(files(4), &config.hints.alphabet, &bound);
        assert!(state.labels.iter().all(|l| l.chars().all(|c| c.is_ascii_digit())), "{:?}", state.labels);
        assert!(prefix_free_and_unique(&state.labels));
    }

    #[test]
    fn labels_never_use_a_digit_bound_to_quit() {
        let config = crate::config::Config::load_str("[hints]\nalphabet = \"jk\"\n[keys]\nquit = [\"1\", \"2\"]").unwrap();
        let (keymap, _) = crate::keymap::Keymap::new(&config.keys);
        let bound = keymap.typed_by(&PASSED_ON);
        let state = RecentState::new(files(7), &config.hints.alphabet, &bound);
        assert!(state.labels.iter().all(|l| !l.contains(['1', '2'])), "{:?}", state.labels);
    }

    #[test]
    fn tilde_edge_cases() {
        assert_eq!(tilde(Path::new("/home/me/"), Path::new("/home/me")), "~");
        assert_eq!(tilde(Path::new("/home/me/a b/ü.pdf"), Path::new("/home/me/")), sep("~/a b/ü.pdf"));
        assert_eq!(tilde(Path::new("/x"), Path::new("/")), sep("~/x"));
        assert_eq!(tilde(Path::new("relative/x"), Path::new("/home/me")), "relative/x");
        let long = format!("/home/me/{}", "d/".repeat(2000));
        assert!(tilde(Path::new(&long), Path::new("/home/me")).starts_with(&sep("~/d/d/")));
    }
}
