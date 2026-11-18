//! The named bookmarks: the `:bookmark` command and the picker, a panel like the contents.
use crate::app::App;
use crate::panel::{Picker, PickerState};
use crate::store;

impl App {
    /// Adds a bookmark at the current place, or moves the one of that name.
    pub fn command_bookmark(&mut self, name: &str) {
        let name = store::normalise_bookmark_name(name);
        let name = name.as_str();
        if self.blank {
            self.say("no document open");
        } else if name.is_empty() {
            self.say("usage: bookmark {name}");
        } else {
            let here = self.view.anchor(&self.doc.layout);
            let moved = store::add_bookmark(&mut self.side.bookmarks, name, here);
            self.mark_dirty();
            self.say(if moved { format!("bookmark {name} moved") } else { format!("bookmark {name} added") });
        }
    }

    /// Opens the picker, with the highlight on the last bookmark at or before the view.
    pub fn open_bookmarks(&mut self) {
        self.open_picker(Picker::Bookmarks);
    }

    /// The bookmarks the filter lets through, as indices.
    pub fn bookmark_rows(&self, state: &PickerState) -> Vec<usize> {
        store::bookmark_rows(&self.side.bookmarks, &state.filter.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::field::Field;
    use crate::keymap::{Keymap, Stroke};
    use crate::panel::{Outcome, key_step, settle};
    use crate::store::Bookmark;
    use crate::view::Anchor;

    fn keymap() -> Keymap {
        Keymap::new(&Config::load_str("").unwrap().keys, &Default::default()).0
    }

    fn list(names: &[&str]) -> Vec<Bookmark> {
        names.iter().enumerate().map(|(i, n)| Bookmark { name: (*n).to_owned(), anchor: Anchor { page: i, y: 0.0, x: 0.0 } }).collect()
    }

    fn key(name: &str) -> Stroke {
        crate::keymap::parse_sequence(name).unwrap().remove(0)
    }

    fn press(list: &[Bookmark], state: &mut PickerState, name: &str) -> Outcome {
        key_step(&|filter| store::bookmark_rows(list, filter), state, &key(name), &keymap())
    }

    /// Presses a key and carries out a deletion, as `App::picker_key` does.
    fn press_applying(list: &mut Vec<Bookmark>, state: &mut PickerState, name: &str) -> Outcome {
        let outcome = press(list, state, name);
        if let Outcome::Delete(row) = outcome {
            store::remove_bookmark(list, row);
            settle(&store::bookmark_rows(list, &state.filter.text), state);
        }
        outcome
    }

    fn filtering(text: &str) -> PickerState {
        let mut filter = Field::default();
        filter.insert(text);
        PickerState { filtering: true, filter, ..Default::default() }
    }

    #[test]
    fn delete_and_d_edit_the_filter_while_filtering() {
        let l = list(&["dad", "mum"]);
        let mut state = filtering("dd");
        assert_eq!(press(&l, &mut state, "d"), Outcome::None);
        assert_eq!(state.filter.text, "ddd");
        // The cursor is at the end, so Delete has nothing to remove there.
        assert_eq!(press(&l, &mut state, "<del>"), Outcome::None);
        assert_eq!(state.filter.text, "ddd");
        press(&l, &mut state, "<left>");
        assert_eq!(press(&l, &mut state, "<del>"), Outcome::None);
        assert_eq!(state.filter.text, "dd");
        assert_eq!(l.len(), 2);
    }

    #[test]
    fn delete_and_d_delete_when_not_filtering() {
        let l = list(&["a", "b", "c"]);
        let mut state = PickerState { selected: 1, ..Default::default() };
        assert_eq!(press(&l, &mut state, "d"), Outcome::Delete(1));
        assert_eq!(press(&l, &mut state, "<del>"), Outcome::Delete(1));
    }

    #[test]
    fn deleting_the_last_row_moves_the_highlight_up() {
        let mut l = list(&["a", "b", "c"]);
        let mut state = PickerState { selected: 2, ..Default::default() };
        assert_eq!(press_applying(&mut l, &mut state, "d"), Outcome::Delete(2));
        assert_eq!((l.len(), state.selected), (2, 1));
        press_applying(&mut l, &mut state, "<del>");
        press_applying(&mut l, &mut state, "d");
        assert_eq!((l.len(), state.selected), (0, 0));
    }

    #[test]
    fn enter_l_and_d_on_an_empty_filtered_list_do_nothing() {
        let l = list(&["a", "b"]);
        let mut state = filtering("zzz");
        assert_eq!(press(&l, &mut state, "<cr>"), Outcome::None);
        state.filtering = false;
        assert_eq!(press(&l, &mut state, "l"), Outcome::None);
        assert_eq!(press(&l, &mut state, "d"), Outcome::None);
        assert_eq!(press(&l, &mut state, "<del>"), Outcome::None);
        assert_eq!(l.len(), 2);
    }

    #[test]
    fn the_selection_stays_in_range_after_a_delete_and_a_filter_change() {
        let mut l = list(&["alpha", "beta", "gamma", "alpine"]);
        let mut state = PickerState { selected: 3, ..Default::default() };
        press_applying(&mut l, &mut state, "d");
        assert!(state.selected < l.len());
        state.selected = 2;
        press(&l, &mut state, "/");
        press(&l, &mut state, "b");
        assert_eq!(store::bookmark_rows(&l, &state.filter.text), [1]);
        assert_eq!(state.selected, 1);
        press(&l, &mut state, "z");
        assert_eq!(state.selected, 1, "no rows: the highlight stays put");
        press(&l, &mut state, "<bs>");
        press(&l, &mut state, "<bs>");
        assert!(state.selected < l.len());
    }

    #[test]
    fn escape_clears_the_filter_first_then_closes() {
        let l = list(&["a", "b"]);
        let mut state = filtering("b");
        assert_eq!(press(&l, &mut state, "<esc>"), Outcome::None);
        assert!(!state.filtering && state.filter.is_empty());
        assert_eq!(press(&l, &mut state, "<esc>"), Outcome::Close);
    }

    #[test]
    fn enter_and_l_jump_and_q_closes() {
        let l = list(&["a", "b"]);
        let mut state = PickerState { selected: 1, ..Default::default() };
        assert_eq!(press(&l, &mut state, "<cr>"), Outcome::Jump(1));
        assert_eq!(press(&l, &mut state, "l"), Outcome::Jump(1));
        assert_eq!(press(&l, &mut state, "q"), Outcome::Close);
    }
}
