//! The one-line text field of the bars: its text, a cursor, and what the editing keys do to them.

/// What an editing key does. The `edit_*` actions under `[keys]` choose the keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    Left,
    Right,
    WordLeft,
    WordRight,
    Start,
    End,
    Backspace,
    Delete,
    EraseWord,
    DeleteWord,
    /// Back to the last space, so all of a path goes at once.
    EraseBlank,
    EraseToStart,
    EraseToEnd,
    /// Clears the password field. The other fields leave it alone: a typed line is edited, not
    /// undone, and `Ctrl+U` already clears it.
    Undo,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Field {
    pub text: String,
    /// A byte offset on a character boundary, kept there by every edit.
    cursor: usize,
}

/// Letters and digits make words, punctuation makes its own, and space separates them.
#[derive(PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Punctuation,
}

fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Punctuation
    }
}

impl Field {
    /// A field holding `text`, with the cursor at its end.
    pub fn new(text: String) -> Self {
        let cursor = text.len();
        Self { text, cursor }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// What is left of the cursor.
    pub fn before_cursor(&self) -> &str {
        self.text.get(..self.cursor).unwrap_or(&self.text)
    }

    /// Empties the field.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    /// The same field with every character shown as a bullet, the cursor where it was.
    pub fn masked(&self) -> Self {
        const BULLET: char = '\u{2022}';
        let count = |text: &str| text.chars().count();
        Self { text: BULLET.to_string().repeat(count(&self.text)), cursor: count(self.before_cursor()) * BULLET.len_utf8() }
    }

    /// Puts `text` in place of what is left of the cursor, which then sits after it.
    pub fn replace_before_cursor(&mut self, text: &str) {
        let end = if self.text.is_char_boundary(self.cursor) { self.cursor } else { self.text.len() };
        self.text.replace_range(..end, text);
        self.cursor = text.len();
    }

    /// Whether nothing is left of the field after the cursor.
    pub fn at_end(&self) -> bool {
        self.after_cursor().is_empty()
    }

    fn after_cursor(&self) -> &str {
        self.text.get(self.cursor..).unwrap_or("")
    }

    pub fn insert(&mut self, text: &str) {
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    /// Where the character before the cursor starts.
    fn char_left(&self) -> usize {
        self.before_cursor().char_indices().next_back().map_or(0, |(i, _)| i)
    }

    /// Where the character after the cursor ends.
    fn char_right(&self) -> usize {
        self.cursor + self.after_cursor().chars().next().map_or(0, char::len_utf8)
    }

    /// The start of the word before the cursor.
    fn word_left(&self) -> usize {
        let mut chars = self.before_cursor().char_indices().rev().skip_while(|(_, c)| class(*c) == Class::Space).peekable();
        let Some((mut start, first)) = chars.next() else { return 0 };
        let kind = class(first);
        for (i, c) in chars {
            if class(c) != kind {
                break;
            }
            start = i;
        }
        start
    }

    /// The end of the word after the cursor.
    fn word_right(&self) -> usize {
        let mut chars = self.after_cursor().char_indices().skip_while(|(_, c)| class(*c) == Class::Space);
        let Some((at, first)) = chars.next() else { return self.text.len() };
        let kind = class(first);
        let mut end = at + first.len_utf8();
        for (i, c) in chars {
            if class(c) != kind {
                break;
            }
            end = i + c.len_utf8();
        }
        self.cursor + end
    }

    /// The start of the run of non-space before the cursor.
    fn blank_left(&self) -> usize {
        let before = self.before_cursor().trim_end();
        before.rfind(char::is_whitespace).map_or(0, |i| i + before[i..].chars().next().map_or(1, char::len_utf8))
    }

    /// Removes from `from` to the cursor.
    fn erase_back(&mut self, from: usize) -> bool {
        let erased = from < self.cursor;
        self.text.replace_range(from..self.cursor, "");
        self.cursor = from;
        erased
    }

    /// Removes from the cursor to `to`.
    fn erase_ahead(&mut self, to: usize) -> bool {
        let erased = self.cursor < to;
        self.text.replace_range(self.cursor..to, "");
        erased
    }

    /// Does an edit, and says whether the text changed.
    pub fn apply(&mut self, edit: Edit) -> bool {
        match edit {
            Edit::Left => self.cursor = self.char_left(),
            Edit::Right => self.cursor = self.char_right(),
            Edit::WordLeft => self.cursor = self.word_left(),
            Edit::WordRight => self.cursor = self.word_right(),
            Edit::Start => self.cursor = 0,
            Edit::End => self.cursor = self.text.len(),
            Edit::Backspace => return self.erase_back(self.char_left()),
            Edit::Delete => return self.erase_ahead(self.char_right()),
            Edit::EraseWord => return self.erase_back(self.word_left()),
            Edit::DeleteWord => return self.erase_ahead(self.word_right()),
            Edit::EraseBlank => return self.erase_back(self.blank_left()),
            Edit::EraseToStart => return self.erase_back(0),
            Edit::EraseToEnd => return self.erase_ahead(self.text.len()),
            Edit::Undo => {}
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field written with `|` for the cursor.
    fn at(marked: &str) -> Field {
        let cursor = marked.find('|').unwrap();
        Field { text: marked.replace('|', ""), cursor }
    }

    fn marked(field: &Field) -> String {
        format!("{}|{}", field.before_cursor(), field.after_cursor())
    }

    fn after(start: &str, edit: Edit) -> String {
        let mut field = at(start);
        field.apply(edit);
        marked(&field)
    }

    #[test]
    fn a_new_field_has_its_cursor_at_the_end() {
        assert_eq!(marked(&Field::new("abc".to_owned())), "abc|");
    }

    #[test]
    fn what_is_left_of_the_cursor_can_be_replaced() {
        let mut field = at("th|日本");
        field.replace_before_cursor("theme ");
        assert_eq!(marked(&field), "theme |日本");
    }

    #[test]
    fn at_end_is_true_only_with_nothing_after_the_cursor() {
        assert!(at("abc|").at_end() && at("|").at_end());
        assert!(!at("ab|c").at_end() && !at("|abc").at_end());
    }

    #[test]
    fn text_goes_in_at_the_cursor() {
        let mut field = at("ab|cd");
        field.insert("xy");
        assert_eq!(marked(&field), "abxy|cd");
    }

    #[test]
    fn the_cursor_moves_by_character() {
        assert_eq!(after("ab|c", Edit::Left), "a|bc");
        assert_eq!(after("ab|c", Edit::Right), "abc|");
        assert_eq!(after("|abc", Edit::Left), "|abc");
        assert_eq!(after("abc|", Edit::Right), "abc|");
    }

    #[test]
    fn the_cursor_moves_by_word() {
        assert_eq!(after("one two three|", Edit::WordLeft), "one two |three");
        assert_eq!(after("one two |three", Edit::WordLeft), "one |two three");
        assert_eq!(after("|one two", Edit::WordRight), "one| two");
        assert_eq!(after("one| two", Edit::WordRight), "one two|");
        assert_eq!(after("|", Edit::WordLeft), "|");
        assert_eq!(after("|", Edit::WordRight), "|");
    }

    #[test]
    fn punctuation_stops_a_word() {
        assert_eq!(after("~/papers/draft.pdf|", Edit::WordLeft), "~/papers/draft.|pdf");
        assert_eq!(after("~/papers/draft.|pdf", Edit::WordLeft), "~/papers/draft|.pdf");
        assert_eq!(after("|snake_case word", Edit::WordRight), "snake_case| word");
    }

    #[test]
    fn the_ends_of_the_line() {
        assert_eq!(after("ab|c", Edit::Start), "|abc");
        assert_eq!(after("ab|c", Edit::End), "abc|");
    }

    #[test]
    fn single_characters_are_erased_either_side() {
        assert_eq!(after("ab|c", Edit::Backspace), "a|c");
        assert_eq!(after("ab|c", Edit::Delete), "ab|");
        assert_eq!(after("|abc", Edit::Backspace), "|abc");
        assert_eq!(after("abc|", Edit::Delete), "abc|");
    }

    #[test]
    fn words_are_erased_either_side() {
        assert_eq!(after("one two  |three", Edit::EraseWord), "one |three");
        assert_eq!(after("one|  two three", Edit::DeleteWord), "one| three");
        assert_eq!(after("  |", Edit::EraseWord), "|");
    }

    #[test]
    fn a_blank_erase_takes_all_of_a_path() {
        assert_eq!(after("e ~/a/b.pdf|", Edit::EraseBlank), "e |");
        assert_eq!(after("one two  |", Edit::EraseBlank), "one |");
        assert_eq!(after("word|", Edit::EraseBlank), "|");
        assert_eq!(after("a\u{3000}b|", Edit::EraseBlank), "a\u{3000}|");
    }

    #[test]
    fn erasing_to_either_end_keeps_the_other_side() {
        assert_eq!(after("ab|cd", Edit::EraseToStart), "|cd");
        assert_eq!(after("ab|cd", Edit::EraseToEnd), "ab|");
    }

    #[test]
    fn moving_does_not_count_as_a_change() {
        let mut field = at("ab|c");
        assert!(!field.apply(Edit::Left) && !field.apply(Edit::WordRight) && !field.apply(Edit::Start));
        assert!(!field.apply(Edit::Backspace), "nothing before the cursor");
        assert!(field.apply(Edit::Delete));
    }

    #[test]
    fn every_edit_stays_on_a_character_boundary() {
        let edits = [
            Edit::Left,
            Edit::Right,
            Edit::WordLeft,
            Edit::WordRight,
            Edit::Start,
            Edit::End,
            Edit::Backspace,
            Edit::Delete,
            Edit::EraseWord,
            Edit::DeleteWord,
            Edit::EraseBlank,
            Edit::EraseToStart,
            Edit::EraseToEnd,
            Edit::Undo,
        ];
        let text = "é 日本語 🐣 naïve-x";
        for start in text.char_indices().map(|(i, _)| i).chain([text.len()]) {
            for first in edits {
                for second in edits {
                    let mut field = Field { text: text.to_owned(), cursor: start };
                    field.apply(first);
                    field.apply(second);
                    assert!(field.text.is_char_boundary(field.cursor), "{first:?} then {second:?} from {start}");
                    field.insert("x");
                }
            }
        }
    }

    #[test]
    fn wide_characters_are_one_step() {
        assert_eq!(after("日本語|", Edit::Left), "日本|語");
        assert_eq!(after("日本語|", Edit::Backspace), "日本|");
        assert_eq!(after("|🐣a", Edit::Delete), "|a");
        assert_eq!(after("日本 語|", Edit::EraseWord), "日本 |");
    }

    #[test]
    fn undo_changes_nothing_in_a_field_and_clear_empties_it() {
        let mut field = at("ab|cd");
        assert!(!field.apply(Edit::Undo));
        assert_eq!(marked(&field), "ab|cd");
        field.clear();
        assert_eq!(marked(&field), "|");
        field.insert("x");
        assert_eq!(marked(&field), "x|");
    }

    #[test]
    fn a_masked_field_shows_bullets_and_keeps_the_cursor() {
        let masked = at("pä|ss日").masked();
        assert_eq!(marked(&masked), "\u{2022}\u{2022}|\u{2022}\u{2022}\u{2022}");
        assert!(!masked.text.contains(['p', 's', '日']));
        assert_eq!(marked(&at("|").masked()), "|");
    }

    #[test]
    fn undo_is_a_no_op_through_apply_for_every_text_and_cursor() {
        let text = "é 日本語 🐣 naïve-x";
        for start in text.char_indices().map(|(i, _)| i).chain([text.len()]) {
            let mut field = Field { text: text.to_owned(), cursor: start };
            let before = field.clone();
            assert!(!field.apply(Edit::Undo));
            assert_eq!(field, before);
        }
        let mut empty = Field::default();
        assert!(!empty.apply(Edit::Undo));
        assert_eq!(empty, Field::default());
    }

    #[test]
    fn masked_counts_characters_not_bytes_at_every_cursor() {
        let text = "pä日🐣s";
        for (n, (start, _)) in text.char_indices().chain([(text.len(), ' ')]).enumerate() {
            let masked = Field { text: text.to_owned(), cursor: start }.masked();
            assert_eq!(masked.text.chars().count(), 5);
            assert!(masked.text.chars().all(|c| c == '\u{2022}'));
            assert_eq!(masked.before_cursor().chars().count(), n, "cursor {start}");
            assert!(masked.text.is_char_boundary(masked.cursor));
        }
        // A cursor that is not on a boundary (it shouldn't happen) must not panic.
        let odd = Field { text: "日".to_owned(), cursor: 1 }.masked();
        assert_eq!(odd.text.chars().count(), 1);
    }

    #[test]
    fn typing_and_editing_a_password_like_field() {
        let mut field = Field::default();
        field.insert("p\u{e4}ss");
        field.insert("w\u{f6}rd");
        assert_eq!(field.text, "p\u{e4}ssw\u{f6}rd");
        assert!(field.apply(Edit::Backspace));
        assert_eq!(field.text, "p\u{e4}ssw\u{f6}r");
        field.apply(Edit::Left);
        field.insert(" ");
        assert_eq!(marked(&field), "p\u{e4}ssw\u{f6} |r");
        field.clear();
        assert_eq!((field.text.as_str(), field.cursor), ("", 0));
        assert!(!field.apply(Edit::Backspace));
        assert_eq!(marked(&field.masked()), "|");
    }
}
