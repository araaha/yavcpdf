//! Smart jump: what a reference written as plain text, such as "Figure 3" or "[12]", points at.
use std::ops::Range;

use pdfcraft_render::PageText;

/// The kinds of thing a document numbers and refers to by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Figure,
    Table,
    Section,
    Chapter,
    Appendix,
    Equation,
    Algorithm,
    Listing,
    Theorem,
    Lemma,
    Proposition,
    Corollary,
    Definition,
    Remark,
    Example,
}

/// How each kind is written, lower case and without a trailing full stop.
const NAMES: &[(&str, Kind)] = &[
    ("figure", Kind::Figure),
    ("figures", Kind::Figure),
    ("fig", Kind::Figure),
    ("figs", Kind::Figure),
    ("table", Kind::Table),
    ("tables", Kind::Table),
    ("tab", Kind::Table),
    ("section", Kind::Section),
    ("sections", Kind::Section),
    ("sec", Kind::Section),
    ("secs", Kind::Section),
    ("chapter", Kind::Chapter),
    ("chapters", Kind::Chapter),
    ("ch", Kind::Chapter),
    ("appendix", Kind::Appendix),
    ("app", Kind::Appendix),
    ("equation", Kind::Equation),
    ("equations", Kind::Equation),
    ("eq", Kind::Equation),
    ("eqs", Kind::Equation),
    ("algorithm", Kind::Algorithm),
    ("algorithms", Kind::Algorithm),
    ("alg", Kind::Algorithm),
    ("listing", Kind::Listing),
    ("listings", Kind::Listing),
    ("theorem", Kind::Theorem),
    ("theorems", Kind::Theorem),
    ("thm", Kind::Theorem),
    ("lemma", Kind::Lemma),
    ("lemmas", Kind::Lemma),
    ("lem", Kind::Lemma),
    ("proposition", Kind::Proposition),
    ("propositions", Kind::Proposition),
    ("prop", Kind::Proposition),
    ("corollary", Kind::Corollary),
    ("corollaries", Kind::Corollary),
    ("cor", Kind::Corollary),
    ("definition", Kind::Definition),
    ("definitions", Kind::Definition),
    ("def", Kind::Definition),
    ("remark", Kind::Remark),
    ("remarks", Kind::Remark),
    ("example", Kind::Example),
    ("examples", Kind::Example),
    ("ex", Kind::Example),
];

/// A reference found in the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reference {
    /// `[12]` or `[Knu84]`: a bibliography entry by its key.
    Citation(String),
    /// `Smith et al., 2020`: a bibliography entry by its first author and year.
    AuthorYear { name: String, year: String },
    /// `Figure 3`, or a bare `(3)` for an equation.
    Labelled { kind: Kind, number: String },
}

impl Reference {
    /// As it is shown in messages.
    pub fn describe(&self) -> String {
        match self {
            Self::Citation(key) => format!("[{key}]"),
            Self::AuthorYear { name, year } => format!("{name} {year}"),
            Self::Labelled { kind, number } => format!("{kind:?} {number}").to_lowercase(),
        }
    }
}

/// A reference and the characters of the line it takes up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub range: Range<usize>,
    pub reference: Reference,
}

/// One line of a page.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    /// The top of the line, in page points.
    pub y: f32,
    /// The glyph each character of `text` came from.
    pub glyphs: Vec<usize>,
}

/// Whether a click at `at` was meant for the glyph in `rect`: on it, or within half its height. The
/// nearest glyph alone won't do, as it can be far across an empty part of the page.
pub fn touches(rect: [f32; 4], at: [f32; 2]) -> bool {
    let reach = (rect[3] - rect[1]).abs() / 2.0;
    let near = |a: f32, b: f32, value: f32| value >= a.min(b) - reach && value <= a.max(b) + reach;
    near(rect[0], rect[2], at[0]) && near(rect[1], rect[3], at[1])
}

/// The lines of a page, in reading order.
pub fn lines_of(text: &PageText) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut current = None;
    for (index, glyph) in text.glyphs.iter().enumerate() {
        let number = text.line_of.get(index).copied();
        if current != Some(number) || lines.is_empty() {
            current = Some(number);
            lines.push(Line { text: String::new(), y: glyph.rect[1], glyphs: Vec::new() });
        }
        let Some(line) = lines.last_mut() else { continue };
        if !line.text.is_empty() && text.space_before.get(index).copied().unwrap_or(false) {
            line.text.push(' ');
            line.glyphs.push(index);
        }
        for c in glyph.text.chars() {
            line.text.push(c);
            line.glyphs.push(index);
        }
    }
    lines
}

struct Word {
    text: String,
    start: usize,
    end: usize,
}

fn words_of(line: &str) -> Vec<Word> {
    let mut words = Vec::new();
    let mut current: Option<Word> = None;
    for (at, c) in line.chars().enumerate() {
        if c.is_whitespace() {
            words.extend(current.take());
        } else {
            let word = current.get_or_insert_with(|| Word { text: String::new(), start: at, end: at });
            word.text.push(c);
            word.end = at + 1;
        }
    }
    words.extend(current);
    words
}

/// A word without the punctuation around it.
fn bare(word: &str) -> &str {
    word.trim_matches(|c: char| "(),;:.[]".contains(c))
}

/// Digits with an optional letter after them, as in `12` or `12a`.
fn is_numeral(part: &str) -> bool {
    let digits = part.trim_end_matches(|c: char| c.is_ascii_lowercase());
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) && part.len() <= digits.len() + 1
}

/// `3`, `3.1`, `3b`, `A`, `A.1`, or a roman numeral such as `IV`.
fn is_number(text: &str) -> bool {
    if !text.is_empty() && text.chars().all(|c| "IVX".contains(c)) {
        return true;
    }
    let mut parts = text.split('.');
    let first = parts.next().unwrap_or("");
    let lettered = first.len() == 1 && first.chars().all(|c| c.is_ascii_uppercase());
    (lettered || is_numeral(first)) && parts.all(is_numeral)
}

fn kind_named(word: &str) -> Option<Kind> {
    let name = word.trim_start_matches(['(', '[']).trim_end_matches('.').to_lowercase();
    NAMES.iter().find(|(n, _)| *n == name).map(|(_, kind)| *kind)
}

/// `Figure 3` and `§3.1`, with the number of words they take.
fn labelled(words: &[Word], i: usize) -> Option<(Found, usize)> {
    let word = words.get(i)?;
    if let Some(rest) = word.text.trim_start_matches('(').strip_prefix('§').map(bare).filter(|r| is_number(r)) {
        let reference = Reference::Labelled { kind: Kind::Section, number: rest.to_owned() };
        return Some((Found { range: word.start..word.end, reference }, 1));
    }
    let kind = kind_named(&word.text)?;
    let next = words.get(i + 1)?;
    let number = bare(&next.text);
    is_number(number).then(|| {
        let reference = Reference::Labelled { kind, number: number.to_owned() };
        (Found { range: word.start..next.end, reference }, 2)
    })
}

/// A surname: a capital and then letters, without what hangs on it.
fn surname(word: &str) -> Option<&str> {
    let name = bare(word).trim_end_matches("'s").trim_end_matches("’s");
    let mut chars = name.chars();
    let capital = chars.next().is_some_and(char::is_uppercase);
    let letters = chars.clone().count() > 0 && chars.all(|c| c.is_alphabetic() || c == '-' || c == '\'' || c == '’');
    (capital && letters).then_some(name)
}

/// A year such as `2020` or `2020a`, without its letter.
fn year_of(word: &str) -> Option<&str> {
    let year = bare(word).trim_end_matches(|c: char| c.is_ascii_lowercase());
    let shaped = year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) && (year.starts_with("19") || year.starts_with("20"));
    shaped.then_some(year)
}

/// `Smith 2020`, `Smith et al., 2020`, `Smith and Jones (2020)`.
fn author_year(words: &[Word], i: usize) -> Option<(Found, usize)> {
    let first = words.get(i)?;
    let name = surname(&first.text)?;
    let mut at = i + 1;
    let next = words.get(at).map(|w| w.text.as_str());
    let after = words.get(at + 1);
    let et_al = next == Some("et") && after.is_some_and(|w| w.text.starts_with("al"));
    let and = matches!(next, Some("and" | "&")) && after.is_some_and(|w| surname(&w.text).is_some());
    if et_al || and {
        at += 2;
    }
    let last = words.get(at)?;
    let year = year_of(&last.text)?;
    let reference = Reference::AuthorYear { name: name.to_owned(), year: year.to_owned() };
    Some((Found { range: first.start..last.end, reference }, at + 1 - i))
}

/// A bare `(3)`, as equations are referred to. Years are left to `author_year`.
fn equation(words: &[Word], i: usize) -> Option<(Found, usize)> {
    let word = words.get(i)?;
    let inner = word.text.trim_end_matches([',', '.', ';', ':']).strip_prefix('(')?.strip_suffix(')')?;
    let numeric = inner.starts_with(|c: char| c.is_ascii_digit()) && inner.len() <= 6 && is_number(inner);
    (numeric && year_of(inner).is_none()).then(|| {
        let reference = Reference::Labelled { kind: Kind::Equation, number: inner.to_owned() };
        (Found { range: word.start..word.end, reference }, 1)
    })
}

/// The key of one item of a bracket: `12`, `Knu84`, or the start of a range such as `3-5`.
fn citation_key(item: &str) -> Option<String> {
    let allowed = |c: char| c.is_alphanumeric() || "+-–:._".contains(c);
    if !item.chars().any(|c| c.is_ascii_digit()) || !item.chars().all(allowed) {
        return None;
    }
    let (start, end) = item.split_once(['-', '–']).unwrap_or((item, item));
    let range = start.chars().all(|c| c.is_ascii_digit()) && end.chars().all(|c| c.is_ascii_digit());
    Some(if range { start.to_owned() } else { item.to_owned() })
}

/// The items of `[1, 2]` and `[Knu84]`, each a reference of its own.
fn brackets(line: &str) -> Vec<Found> {
    const LONGEST: usize = 48;
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(open) = chars.iter().skip(at).position(|c| *c == '[').map(|p| p + at) {
        at = open + 1;
        let close = chars.iter().skip(at).take(LONGEST).position(|c| *c == ']').map(|p| p + at);
        let Some(close) = close else { continue };
        let mut start = at;
        for end in (at..=close).filter(|i| *i == close || matches!(chars.get(*i), Some(',' | ';'))) {
            let item: String = chars.get(start..end).unwrap_or(&[]).iter().collect();
            let lead = item.chars().take_while(|c| c.is_whitespace()).count();
            if let Some(key) = citation_key(item.trim()) {
                // Alone in its bracket, the brackets count as part of it.
                let whole = start == at && end == close;
                let range = if whole { open..close + 1 } else { start + lead..start + lead + item.trim().chars().count() };
                out.push(Found { range, reference: Reference::Citation(key) });
            }
            start = end + 1;
        }
        at = close + 1;
    }
    out
}

/// The references on a line, left to right.
pub fn references_in(line: &str) -> Vec<Found> {
    let words = words_of(line);
    let mut out = brackets(line);
    let mut i = 0;
    while i < words.len() {
        match labelled(&words, i).or_else(|| author_year(&words, i)).or_else(|| equation(&words, i)) {
            Some((found, taken)) => {
                out.push(found);
                i += taken;
            }
            None => i += 1,
        }
    }
    out.sort_by_key(|f| f.range.start);
    out
}

/// Every line of the document with its page.
fn all_lines(pages: &[Vec<Line>]) -> Vec<(usize, &Line)> {
    pages.iter().enumerate().flat_map(|(page, lines)| lines.iter().map(move |line| (page, line))).collect()
}

/// Where the bibliography starts: after the last heading that names one.
fn bibliography(lines: &[(usize, &Line)]) -> usize {
    let heading = |(_, line): &(usize, &Line)| {
        let title = line.text.trim().trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ').trim_end_matches(':').to_lowercase();
        matches!(title.as_str(), "references" | "bibliography" | "works cited" | "literature cited")
    };
    lines.iter().rposition(heading).unwrap_or(0)
}

/// Whether a line starts the entry `key`: `[12]`, `12.`, `12)` or `(12)`.
fn opens_entry(line: &str, key: &str) -> bool {
    let line = line.trim_start();
    [format!("[{key}]"), format!("{key}."), format!("{key})"), format!("({key})")]
        .iter()
        .filter_map(|opening| line.strip_prefix(opening.as_str()))
        .any(|rest| !rest.starts_with(|c: char| c.is_ascii_digit()))
}

fn find_citations(key: &str, lines: &[(usize, &Line)]) -> Vec<(usize, f32)> {
    let place = |(page, line): &(usize, &Line)| (*page, line.y);
    let entry = |(_, line): &&(usize, &Line)| opens_entry(&line.text, key);
    let tail = lines.get(bibliography(lines)..).unwrap_or(&[]);
    let listed: Vec<(usize, f32)> = tail.iter().filter(entry).map(place).collect();
    if listed.is_empty() { lines.iter().rev().filter(entry).map(place).collect() } else { listed }
}

fn find_authors(name: &str, year: &str, lines: &[(usize, &Line)]) -> Vec<(usize, f32)> {
    /// An entry's first author comes this early in its line, and its year in the next few.
    const AUTHOR_WITHIN: usize = 40;
    const ENTRY_LINES: usize = 4;
    let name = name.to_lowercase();
    let tail = lines.get(bibliography(lines)..).unwrap_or(&[]);
    let names = |line: &Line| line.text.chars().take(AUTHOR_WITHIN).collect::<String>().to_lowercase().contains(&name);
    let found = (0..tail.len()).filter_map(|i| {
        let (page, line) = tail.get(i)?;
        // The first year after the name is the entry's: one further on belongs to the next entry.
        let has_year = |(_, l): &&(usize, &Line)| words_of(&l.text).iter().any(|w| year_of(&w.text).is_some());
        let dated = tail.iter().skip(i).take(ENTRY_LINES).find(has_year).is_some_and(|(_, l)| l.text.contains(year));
        (names(line) && dated).then_some((*page, line.y))
    });
    found.collect()
}

/// What follows a label, as evidence that this is the thing itself and not a mention of it.
fn caption_score(rest: &str) -> u8 {
    let mut chars = rest.chars();
    match chars.next() {
        None | Some('.' | ':' | '—' | '–' | '-' | '|' | '(') => 2,
        Some(' ') if chars.next().is_some_and(|c| !c.is_lowercase()) => 1,
        _ => 0,
    }
}

/// What follows `word` at the start of `text`, in either case. What follows keeps its own case,
/// which is the difference between a caption and a sentence.
fn after_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    text.get(..word.len()).filter(|head| head.eq_ignore_ascii_case(word))?;
    text.get(word.len()..)
}

/// A heading such as `3.1 Methods`, or `3.1 Methods .... 12` as a table of contents has it.
fn heading_score(text: &str, kind: Kind, number: &str) -> u8 {
    let named = NAMES.iter().filter(|(_, k)| *k == kind).find_map(|(name, _)| after_word(text, name)).map(str::trim_start);
    if let Some(rest) = named.and_then(|r| after_word(r, number)) {
        return caption_score(rest).max(1);
    }
    let Some(rest) = text.strip_prefix(number) else { return 0 };
    let rest = rest.strip_prefix('.').unwrap_or(rest);
    let titled = rest.starts_with(' ') && rest.trim_start().starts_with(|c: char| c.is_uppercase());
    match titled {
        true if rest.trim_end().ends_with(|c: char| c.is_ascii_digit()) => 1,
        true => 2,
        false => 0,
    }
}

/// How much a line looks like the thing labelled: 0 is not at all.
fn label_score(text: &str, kind: Kind, number: &str) -> u8 {
    let text = text.trim();
    match kind {
        Kind::Equation => u8::from(text.ends_with(&format!("({number})"))) * 2,
        Kind::Section | Kind::Chapter | Kind::Appendix => heading_score(text, kind, number),
        _ => {
            let names = NAMES.iter().filter(|(_, k)| *k == kind);
            let after = names.filter_map(|(name, _)| after_word(text, name)).map(|r| r.strip_prefix('.').unwrap_or(r).trim_start());
            after.filter_map(|rest| after_word(rest, number).map(caption_score)).max().unwrap_or(0)
        }
    }
}

fn find_labels(kind: Kind, number: &str, lines: &[(usize, &Line)]) -> Vec<(usize, f32)> {
    let mut found: Vec<(u8, usize, f32)> = lines.iter().map(|(page, line)| (label_score(&line.text, kind, number), *page, line.y)).collect();
    found.retain(|(score, ..)| *score > 0);
    // Stable: among equals, the first in the document comes first.
    found.sort_by_key(|(score, ..)| std::cmp::Reverse(*score));
    found.into_iter().map(|(_, page, y)| (page, y)).collect()
}

/// Everywhere a reference may point, most likely first: a page and the y on it.
pub fn find_definitions(reference: &Reference, pages: &[Vec<Line>]) -> Vec<(usize, f32)> {
    /// A common name can match a whole bibliography, and nobody steps through more than this.
    const MOST: usize = 32;
    let lines = all_lines(pages);
    let mut found = match reference {
        Reference::Citation(key) => find_citations(key, &lines),
        Reference::AuthorYear { name, year } => find_authors(name, year, &lines),
        Reference::Labelled { kind, number } => find_labels(*kind, number, &lines),
    };
    found.truncate(MOST);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_definition(reference: &Reference, pages: &[Vec<Line>]) -> Option<(usize, f32)> {
        find_definitions(reference, pages).first().copied()
    }

    fn refs(line: &str) -> Vec<Reference> {
        references_in(line).into_iter().map(|f| f.reference).collect()
    }

    fn label(kind: Kind, number: &str) -> Reference {
        Reference::Labelled { kind, number: number.to_owned() }
    }

    fn doc(pages: &[&[&str]]) -> Vec<Vec<Line>> {
        let line = |(i, text): (usize, &&str)| Line { text: (*text).to_owned(), y: i as f32 * 10.0, glyphs: Vec::new() };
        pages.iter().map(|lines| lines.iter().enumerate().map(line).collect()).collect()
    }

    #[test]
    fn names_and_numbers_make_a_label() {
        assert_eq!(refs("as shown in Figure 3, the"), [label(Kind::Figure, "3")]);
        assert_eq!(refs("see Fig. 12a."), [label(Kind::Figure, "12a")]);
        assert_eq!(refs("(Table 2)"), [label(Kind::Table, "2")]);
        assert_eq!(refs("in Section 3.1.2:"), [label(Kind::Section, "3.1.2")]);
        assert_eq!(refs("by §4.2 we"), [label(Kind::Section, "4.2")]);
        assert_eq!(refs("Appendix B holds"), [label(Kind::Appendix, "B")]);
        assert_eq!(refs("using Theorem A.1 and Lemma 2"), [label(Kind::Theorem, "A.1"), label(Kind::Lemma, "2")]);
        assert_eq!(refs("Eq. (4) gives"), [label(Kind::Equation, "4")]);
    }

    #[test]
    fn words_that_only_look_like_labels_are_left_alone() {
        assert!(refs("the table of contents shows a figure").is_empty());
        assert!(refs("an example would be nice").is_empty());
    }

    #[test]
    fn a_bare_number_in_brackets_is_an_equation_but_a_year_is_not() {
        assert_eq!(refs("from (3) and (4.2), we"), [label(Kind::Equation, "3"), label(Kind::Equation, "4.2")]);
        assert!(refs("published (2020) here").is_empty());
        assert!(refs("cases (a) and (b)").is_empty());
    }

    #[test]
    fn each_item_of_a_bracket_is_a_citation() {
        let citation = |k: &str| Reference::Citation(k.to_owned());
        assert_eq!(refs("as in [12]."), [citation("12")]);
        assert_eq!(refs("see [1, 4; 7]"), [citation("1"), citation("4"), citation("7")]);
        assert_eq!(refs("[Knu84] shows"), [citation("Knu84")]);
        assert_eq!(refs("[3-5] and [6–9]"), [citation("3"), citation("6")]);
        assert!(refs("the interval [a, b] and [see below]").is_empty());
    }

    #[test]
    fn a_citation_covers_its_brackets_alone_and_just_its_key_in_a_list() {
        let found = references_in("x [12] y");
        assert_eq!(found[0].range, 2..6);
        let found = references_in("[1, 22]");
        assert_eq!((found[0].range.clone(), found[1].range.clone()), (1..2, 4..6));
    }

    #[test]
    fn a_bracket_that_never_closes_is_no_citation() {
        assert!(refs("broken [12").is_empty());
        assert!(refs("[").is_empty());
        assert!(refs("]").is_empty());
    }

    #[test]
    fn authors_and_years_make_a_reference() {
        let by = |n: &str, y: &str| Reference::AuthorYear { name: n.to_owned(), year: y.to_owned() };
        assert_eq!(refs("(Smith et al., 2020)"), [by("Smith", "2020")]);
        assert_eq!(refs("Smith and Jones (2019a) found"), [by("Smith", "2019")]);
        assert_eq!(refs("(Smith, 2020; Müller, 1999)"), [by("Smith", "2020"), by("Müller", "1999")]);
        assert_eq!(refs("Lee's 2021"), [by("Lee", "2021")]);
    }

    #[test]
    fn a_caption_beats_a_mention_of_it() {
        let pages = doc(&[&["Figure 3 shows the result", "see Figure 3."], &["Figure 30: another", "Figure 3: the result", "Figure 3. a duplicate"]]);
        assert_eq!(find_definition(&label(Kind::Figure, "3"), &pages), Some((1, 10.0)));
    }

    #[test]
    fn every_candidate_is_kept_the_likeliest_first() {
        let pages = doc(&[&["Figure 3 Shows a mention", "Figure 3 lower case is no candidate"], &["Figure 3: one", "Figure 3. two"]]);
        let found = find_definitions(&label(Kind::Figure, "3"), &pages);
        assert_eq!(found, [(1, 0.0), (1, 10.0), (0, 0.0)]);
    }

    #[test]
    fn a_name_common_in_the_bibliography_gives_a_bounded_list() {
        let lines: Vec<&str> = std::iter::repeat_n("Smith, J. A work. 2020.", 100).collect();
        let reference = Reference::AuthorYear { name: "Smith".to_owned(), year: "2020".to_owned() };
        assert_eq!(find_definitions(&reference, &doc(&[&lines])).len(), 32);
    }

    #[test]
    fn a_click_touches_a_glyph_on_it_or_just_beside_it() {
        let rect = [100.0, 200.0, 106.0, 212.0];
        assert!(touches(rect, [103.0, 206.0]));
        assert!(touches(rect, [110.0, 206.0]));
        assert!(touches(rect, [103.0, 217.0]));
        assert!(touches([100.0, 212.0, 106.0, 200.0], [103.0, 206.0]));
    }

    #[test]
    fn a_click_across_empty_page_touches_nothing() {
        let rect = [100.0, 200.0, 106.0, 212.0];
        assert!(!touches(rect, [103.0, 300.0]));
        assert!(!touches(rect, [103.0, 219.0]));
        assert!(!touches(rect, [90.0, 206.0]));
        assert!(!touches(rect, [f32::NAN, 206.0]));
        assert!(!touches([100.0, f32::NAN, 106.0, 212.0], [103.0, 206.0]));
    }

    #[test]
    fn a_caption_without_punctuation_counts_whatever_the_case_of_its_label() {
        let pages = doc(&[&["FIGURE 3 The result"]]);
        assert_eq!(find_definition(&label(Kind::Figure, "3"), &pages), Some((0, 0.0)));
        let pages = doc(&[&["theorem a.1. Something"]]);
        assert_eq!(find_definition(&label(Kind::Theorem, "A.1"), &pages), Some((0, 0.0)));
    }

    #[test]
    fn text_that_is_not_ascii_does_not_trip_the_comparison() {
        let pages = doc(&[&["Fïgure 3: not the word", "§§§", "é"]]);
        assert_eq!(find_definition(&label(Kind::Figure, "3"), &pages), None);
    }

    #[test]
    fn a_mention_is_not_the_definition_when_nothing_else_is() {
        let pages = doc(&[&["Figure 3 shows the result"]]);
        assert_eq!(find_definition(&label(Kind::Figure, "3"), &pages), None);
    }

    #[test]
    fn a_heading_beats_its_line_in_the_contents() {
        let pages = doc(&[&["3.1 Methods ...... 12"], &["some text", "3.1 Methods"]]);
        assert_eq!(find_definition(&label(Kind::Section, "3.1"), &pages), Some((1, 10.0)));
        let pages = doc(&[&["Chapter 2"], &["Chapter 2 is about"]]);
        assert_eq!(find_definition(&label(Kind::Chapter, "2"), &pages), Some((0, 0.0)));
    }

    #[test]
    fn an_equation_is_the_line_ending_with_its_number() {
        let pages = doc(&[&["from (3) we get", "E = mc^2    (3)"]]);
        assert_eq!(find_definition(&label(Kind::Equation, "3"), &pages), Some((0, 10.0)));
    }

    #[test]
    fn a_citation_is_looked_up_in_the_bibliography() {
        let pages = doc(&[&["[1] is mentioned early", "[2] too"], &["References", "[1] A. Author. A title.", "[12] B. Author."]]);
        let citation = |k: &str| Reference::Citation(k.to_owned());
        assert_eq!(find_definition(&citation("1"), &pages), Some((1, 10.0)));
        assert_eq!(find_definition(&citation("12"), &pages), Some((1, 20.0)));
        assert_eq!(find_definition(&citation("99"), &pages), None);
    }

    #[test]
    fn numbered_entries_need_the_whole_number() {
        let pages = doc(&[&["Bibliography", "12. A. Author.", "1.5 Not an entry", "1. B. Author."]]);
        assert_eq!(find_definition(&Reference::Citation("1".to_owned()), &pages), Some((0, 30.0)));
    }

    #[test]
    fn an_author_is_found_with_the_right_year() {
        let pages =
            doc(&[&["as Smith (2020) says"], &["References", "Smith, J. (1999). Old work.", "Smith, J. and Lee, K. Newer", "work. Journal, 2020."]]);
        let reference = Reference::AuthorYear { name: "Smith".to_owned(), year: "2020".to_owned() };
        assert_eq!(find_definition(&reference, &pages), Some((1, 20.0)));
    }

    #[test]
    fn lines_keep_their_words_and_map_back_to_glyphs() {
        let glyph = |c: &str, y: f32| pdfcraft_render::TextGlyph {
            text: c.to_owned(),
            rect: [0.0, y, 1.0, y + 1.0],
            quad: [[0.0; 2]; 4],
            direction: [1.0, 0.0],
        };
        let text = PageText {
            glyphs: vec![glyph("a", 5.0), glyph("fi", 5.0), glyph("b", 5.0), glyph("c", 9.0)],
            line_of: vec![0, 0, 0, 1],
            space_before: vec![false, false, true, false],
        };
        let lines = lines_of(&text);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "afi b");
        assert_eq!(lines[0].glyphs, [0, 1, 1, 2, 2]);
        assert_eq!((lines[1].text.as_str(), lines[1].y), ("c", 9.0));
    }
}
