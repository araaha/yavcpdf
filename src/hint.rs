//! Hint mode: labels over what can be picked, chosen by typing.
use pdfcraft_render::PageText;

/// Equal-length labels from `alphabet`, so no label is the start of another. The first labels use
/// the first letters, which the default alphabet puts on the home row.
pub fn labels(n: usize, alphabet: &[char]) -> Vec<String> {
    let base = alphabet.len();
    let mut length = 1;
    let mut room = base;
    while room < n {
        length += 1;
        room = room.saturating_mul(base);
    }
    (0..n)
        .map(|mut i| {
            let mut label = vec![alphabet[0]; length];
            for slot in label.iter_mut().rev() {
                *slot = alphabet[i % base];
                i /= base;
            }
            label.into_iter().collect()
        })
        .collect()
}

/// What a label stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    /// A word: its first and last glyph.
    Word(usize, usize),
    /// A link, by its index in the document's list.
    Link(usize),
    /// An annotation, by id.
    Annotation(u64),
    /// A reference written in the text, by its index in the app's list for these hints.
    Reference(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub label: String,
    pub page: usize,
    pub rect: [f32; 4],
    pub kind: TargetKind,
}

/// What the hints are for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Select,
    Link,
    DeleteAnnotation,
    /// A reference to jump to its definition.
    Smart,
    /// A reference to show its definition in the overview.
    Overview,
    /// Where the tool in hand puts what it makes.
    Place,
}

#[derive(Clone, Debug)]
pub struct HintState {
    pub purpose: Purpose,
    pub targets: Vec<Target>,
    /// Labels are assigned once the text of every page showing has arrived.
    pub ready: bool,
    pub typed: String,
    /// The first word of a selection, once it is picked: (page, first glyph, last glyph).
    pub start: Option<(usize, usize, usize)>,
}

impl HintState {
    pub fn new(purpose: Purpose) -> Self {
        Self { purpose, targets: Vec::new(), ready: false, typed: String::new(), start: None }
    }

    /// Gives the targets their labels, top to bottom.
    pub fn assign(&mut self, mut targets: Vec<Target>, alphabet: &[char]) {
        let names = labels(targets.len(), alphabet);
        for (target, label) in targets.iter_mut().zip(names) {
            target.label = label;
        }
        self.targets = targets;
        self.ready = true;
    }

    /// The target whose whole label is typed.
    pub fn chosen(&self) -> Option<&Target> {
        self.targets.iter().find(|t| t.label == self.typed)
    }

    /// Whether any label still starts with what is typed.
    pub fn matches(&self) -> bool {
        self.targets.iter().any(|t| t.label.starts_with(&self.typed))
    }
}

/// The words of `text` crossing `area` (page points), as targets without labels yet.
pub fn word_targets(text: &PageText, page: usize, area: [f32; 4]) -> Vec<Target> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.glyphs.len() {
        let Some((first, last)) = text.word_at(i) else { break };
        i = last.max(i) + 1;
        if text.glyphs[first..=last].iter().all(|g| g.text.trim().is_empty()) {
            continue;
        }
        let rect = text.glyphs[first..=last].iter().map(|g| g.rect).reduce(union).unwrap_or(text.glyphs[first].rect);
        if intersects(rect, area) {
            out.push(Target { label: String::new(), page, rect, kind: TargetKind::Word(first, last) });
        }
    }
    out
}

pub fn union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
}

pub fn intersects(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcraft_render::TextGlyph;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn glyph(c: char, x: f32, y: f32) -> TextGlyph {
        let rect = [x, y, x + 5.0, y + 10.0];
        let quad = [[x, y + 10.0], [x + 5.0, y + 10.0], [x + 5.0, y], [x, y]];
        TextGlyph { text: c.to_string(), rect, quad, direction: [1.0, 0.0] }
    }

    /// "ab cd" on one line, then "ef" on the next.
    fn page() -> PageText {
        let line = |s: &str, y: f32| s.chars().enumerate().map(|(i, c)| glyph(c, i as f32 * 5.0, y)).collect::<Vec<_>>();
        let glyphs: Vec<_> = line("abcd", 0.0).into_iter().chain(line("ef", 20.0)).collect();
        PageText { glyphs, line_of: vec![0, 0, 0, 0, 1, 1], space_before: vec![false, false, true, false, false, false] }
    }

    #[test]
    fn labels_are_unique_and_prefix_free() {
        for n in [1, 2, 3, 9, 10, 100, 1000] {
            let all = labels(n, &chars("abc"));
            assert_eq!(all.len(), n);
            for (i, a) in all.iter().enumerate() {
                for (j, b) in all.iter().enumerate() {
                    assert!(i == j || !b.starts_with(a.as_str()), "{a} and {b}");
                }
            }
        }
    }

    #[test]
    fn a_few_targets_get_single_letters_from_the_front_of_the_alphabet() {
        assert_eq!(labels(3, &chars("asdf")), ["a", "s", "d"]);
        assert_eq!(labels(5, &chars("asdf"))[0], "aa");
        assert!(labels(0, &chars("ab")).is_empty());
    }

    #[test]
    fn words_split_at_gaps_and_lines() {
        let targets = word_targets(&page(), 3, [0.0, 0.0, 100.0, 100.0]);
        let kinds: Vec<_> = targets.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, [TargetKind::Word(0, 1), TargetKind::Word(2, 3), TargetKind::Word(4, 5)]);
        assert_eq!(targets[1].rect, [10.0, 0.0, 20.0, 10.0]);
        assert_eq!(targets[0].page, 3);
    }

    #[test]
    fn only_words_in_the_area_get_hints() {
        let targets = word_targets(&page(), 0, [0.0, 15.0, 100.0, 100.0]);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].kind, TargetKind::Word(4, 5));
    }

    #[test]
    fn blank_glyphs_are_not_words() {
        let mut text = page();
        text.glyphs[2].text = " ".to_owned();
        text.glyphs[3].text = " ".to_owned();
        let targets = word_targets(&text, 0, [0.0, 0.0, 100.0, 100.0]);
        assert_eq!(targets.len(), 2);
    }

    #[test]
    fn typing_narrows_to_a_target() {
        let mut state = HintState::new(Purpose::Select);
        state.assign(word_targets(&page(), 0, [0.0, 0.0, 100.0, 100.0]), &chars("ab"));
        assert_eq!(state.targets[2].label, "ba");
        state.typed = "b".to_owned();
        assert!(state.chosen().is_none() && state.matches());
        state.typed = "ba".to_owned();
        assert_eq!(state.chosen().unwrap().kind, TargetKind::Word(4, 5));
        state.typed = "bb".to_owned();
        assert!(state.chosen().is_none());
        assert!(!state.matches());
    }
}
