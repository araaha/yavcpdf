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
    /// A text annotation of the PDF, by its index in the document's list.
    PdfNote(usize),
    /// A form field's widget: the field's index in the form, and the widget's in the field.
    Widget(usize, usize),
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
    /// A form field to fill, or a button of the form to press.
    Form,
}

impl Purpose {
    /// Whether the labels are made from the text of the pages, so they wait for it.
    pub fn needs_text(self) -> bool {
        matches!(self, Purpose::Select | Purpose::Place | Purpose::Smart | Purpose::Overview)
    }
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

/// The smallest label text, in points: below it a label can't be read.
pub const MIN_FONT: f32 = 7.0;

/// The size of a label's text over a target `height` tall on screen: `scale` times the height, but
/// no larger than `max` (the `hints.font_size`) and no smaller than `MIN_FONT`.
pub fn font_size(height: f32, scale: f32, max: f32) -> f32 {
    (height * scale).clamp(MIN_FONT.min(max), max)
}

fn overlap(a: [f32; 4], b: [f32; 4]) -> f32 {
    let (w, h) = (a[2].min(b[2]) - a[0].max(b[0]), a[3].min(b[3]) - a[1].max(b[1]));
    if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
}

/// Where each label goes on screen, as `[left, top, right, bottom]`: `targets` are the screen
/// rectangles the labels stand for and `sizes` the labels' `[width, height]`. A label is centred on
/// its target. If that overlaps a label placed before it, it tries the places just above and below
/// the target, so it is never more than a line away; with no free place it stays where it overlaps
/// least.
pub fn place_labels(targets: &[[f32; 4]], sizes: &[[f32; 2]]) -> Vec<[f32; 4]> {
    let mut placed: Vec<[f32; 4]> = Vec::with_capacity(targets.len());
    for (t, &[w, h]) in targets.iter().zip(sizes) {
        let (cx, cy) = ((t[0] + t[2]) / 2.0, (t[1] + t[3]) / 2.0);
        let at = |x: f32, y: f32| [x, y, x + w, y + h];
        let candidates = [at(cx - w / 2.0, cy - h / 2.0), at(cx - w / 2.0, t[1] - h), at(cx - w / 2.0, t[3]), at(t[0], t[1] - h), at(t[0], t[3])];
        let cost = |c: &[f32; 4]| placed.iter().map(|p| overlap(*p, *c)).sum::<f32>();
        let mut best = candidates[0];
        let mut least = cost(&best);
        for c in &candidates[1..] {
            if least == 0.0 {
                break;
            }
            let this = cost(c);
            if this < least {
                (best, least) = (*c, this);
            }
        }
        placed.push(best);
    }
    placed
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
        PageText { glyphs, line_of: vec![0, 0, 0, 0, 1, 1], space_before: vec![false, false, true, false, false, false], truncated: false }
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
    fn label_text_follows_the_target_between_the_limits() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(near(font_size(10.0, 0.7, 12.0), 7.0));
        assert_eq!(font_size(20.0, 0.7, 12.0), 12.0);
        assert!(near(font_size(13.0, 0.7, 12.0), 9.1));
        assert_eq!(font_size(1.0, 0.7, 12.0), MIN_FONT);
        assert_eq!(font_size(1.0, 0.7, 5.0), 5.0);
    }

    #[test]
    fn a_label_is_centred_on_its_word() {
        let placed = place_labels(&[[100.0, 50.0, 140.0, 60.0]], &[[10.0, 8.0]]);
        assert_eq!(placed, [[115.0, 51.0, 125.0, 59.0]]);
    }

    #[test]
    fn a_label_in_the_way_moves_a_line_up_or_down_not_along() {
        // Two short words side by side: centred, their wide labels would overlap.
        let words = [[100.0, 50.0, 106.0, 60.0], [110.0, 50.0, 116.0, 60.0]];
        let placed = place_labels(&words, &[[14.0, 8.0], [14.0, 8.0]]);
        assert_eq!(placed[0], [96.0, 51.0, 110.0, 59.0]);
        assert_eq!(overlap(placed[0], placed[1]), 0.0);
        assert!((placed[1][0] - 106.0).abs() < 1e-4, "{:?}", placed[1]);
        assert!(placed[1][1] >= 42.0 && placed[1][3] <= 68.0, "{:?}", placed[1]);
    }

    #[test]
    fn with_no_free_place_a_label_still_gets_one() {
        let word = [0.0, 0.0, 4.0, 4.0];
        let placed = place_labels(&[word; 8], &[[6.0, 6.0]; 8]);
        assert_eq!(placed.len(), 8);
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
