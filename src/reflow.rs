//! Copied text with its line breaks and hyphenation undone.

use std::ops::Range;

use pdfcraft_render::PageText;

/// One line of the selection, with the geometry that tells paragraphs apart.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub text: String,
    /// The box of the line, in page points, `top` above `bottom`. `top` and `bottom` are the median of the glyphs, so a tall inline
    /// formula does not move them.
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
    /// Whether this is the first line taken from its page. No spacing is measured across a page.
    pub new_page: bool,
    /// Whether most glyphs of the line run up or down the page, where the geometry means nothing.
    pub rotated: bool,
}

/// Paragraph breaks are guessed from the lines in the selection. Lines are grouped into blocks (a page, a column, a text size), and
/// the thresholds are:
/// - the leading is the `LEADING_QUANTILE` quantile of the positive gaps between line tops, in line heights (so text of two sizes
///   shares one leading), a low one so that paragraph gaps (short paragraphs, list items) do not take over. A gap over `GAP` times
///   the leading breaks: LaTeX paragraphs often have no extra skip and rely on the indent, while typst and Word add 0.3 to 1 line,
///   and the jitter of lines with sub- and superscripts stays well under 1.3. With a leading under `MEANINGFUL` line heights (equal
///   tops, rotated text) no gap is used;
/// - a line height more than `HEIGHT` off the line before (a heading) breaks before and after it;
/// - a line ending a sentence short of the block's right edge by `SHORT` of the block's width, if the block is justified (see
///   `JUSTIFIED`) or the break has a second signal (a gap over `HEADING_GAP` times the leading, or an indented next line); or one
///   that does not end in punctuation and is followed by a gap over `HEADING_GAP` times the leading (a heading of the body size);
/// - a line indented over `INDENT` line heights from the block's left edge, when the line before it ends a sentence or is short,
///   and it does not follow a list item or its hanging continuation, so the lines of an indented run are one paragraph;
/// - a left edge moving over `COLUMN` line heights, or a top going back up, starts a new block (another column);
/// - a smaller line inside the line before it or between two lines (the limits of a sum) is attached to the text around it.
const GAP: f32 = 1.3;
const LEADING_QUANTILE: f32 = 0.25;
const MEANINGFUL: f32 = 0.25;
const HEIGHT: f32 = 0.15;
const SHORT: f32 = 0.2;
const HEADING_GAP: f32 = 1.1;
const INDENT: f32 = 0.75;
const COLUMN: f32 = 3.0;
/// A block is justified when at least `JUSTIFIED_SHARE` of its lines but the last end within `JUSTIFIED` of its width of the right edge.
const JUSTIFIED: f32 = 0.03;
const JUSTIFIED_SHARE: f32 = 0.4;
/// A glyph direction this far from horizontal is rotated.
const ROTATED: f32 = 0.15;

/// Appends the lines of the glyphs in `range` of a page to `rows`. The lines are those of the text layer: columns that share a
/// baseline grid come as one row, which is a known limit of pdfcraft that this cannot undo.
pub fn rows_of(text: &PageText, range: Range<usize>, rows: &mut Vec<Row>) {
    let mut current = None;
    let mut first = true;
    let (mut tops, mut bottoms, mut rotated) = (Vec::new(), Vec::new(), 0);
    for index in range {
        let Some(glyph) = text.glyphs.get(index) else { break };
        if glyph.text.is_empty() {
            continue;
        }
        let number = text.line_of.get(index).copied();
        if current != Some(number) || first {
            if !first && let Some(row) = rows.last_mut() {
                settle(row, &mut tops, &mut bottoms, rotated);
            }
            rotated = 0;
            current = Some(number);
            rows.push(Row {
                text: String::new(),
                left: glyph.rect[0],
                right: glyph.rect[2],
                top: glyph.rect[1],
                bottom: glyph.rect[3],
                new_page: first,
                rotated: false,
            });
            first = false;
        }
        let Some(row) = rows.last_mut() else { continue };
        if !row.text.is_empty() && text.space_before.get(index).copied().unwrap_or(false) {
            row.text.push(' ');
        }
        row.text.push_str(&glyph.text);
        row.left = row.left.min(glyph.rect[0]);
        row.right = row.right.max(glyph.rect[2]);
        tops.push(glyph.rect[1]);
        bottoms.push(glyph.rect[3]);
        if glyph.direction[1].abs() > ROTATED {
            rotated += 1;
        }
    }
    if !first && let Some(row) = rows.last_mut() {
        settle(row, &mut tops, &mut bottoms, rotated);
    }
}

/// Sets the vertical extent and the rotation of a finished row from its glyphs.
fn settle(row: &mut Row, tops: &mut Vec<f32>, bottoms: &mut Vec<f32>, rotated: usize) {
    row.rotated = rotated * 2 > tops.len();
    row.top = median(std::mem::take(tops));
    row.bottom = median(std::mem::take(bottoms));
}

/// Whether `c` is written without spaces between words. Hangul is not: Korean has them.
fn is_cjk(c: char) -> bool {
    let n = c as u32;
    let hangul =
        matches!(n, 0x1100..=0x11FF | 0x3130..=0x318F | 0x3200..=0x321E | 0x3260..=0x327E | 0xA960..=0xA97F | 0xAC00..=0xD7FF | 0xFFA0..=0xFFDC);
    !hangul && matches!(n, 0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF)
}

fn is_combining(c: char) -> bool {
    matches!(c as u32, 0x300..=0x36F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F)
}

/// Whether a line starts a list item: a bullet, or, when `after_break` (the line before ended a sentence or started an item), `(a)`,
/// `a)`, `[1]` or a number of up to 3 digits with `)` or `.`, then a space. So "the year" / "1999. Then" and "authors in the" /
/// "[12] who" are no items.
fn starts_item(line: &str, after_break: bool) -> bool {
    let mut chars = line.chars();
    let Some(first) = chars.next() else { return false };
    let rest = chars.as_str();
    let boundary = |s: &str| s.is_empty() || s.starts_with(' ');
    if matches!(first, '•' | '-' | '–' | '*' | '●' | '▪' | '◦' | '‣' | '∙' | '\u{f0b7}' | '\u{f0a7}') {
        return boundary(rest);
    }
    if !after_break {
        return false;
    }
    if let Some(close) = match first {
        '(' => Some(')'),
        '[' => Some(']'),
        _ => None,
    } {
        let Some(end) = rest.find(close) else { return false };
        let label = &rest[..end];
        let count = label.chars().count();
        let label_ok =
            (count <= 3 && label.chars().all(|c| c.is_ascii_digit()) && count > 0) || (count == 1 && label.chars().all(char::is_alphabetic));
        return label_ok && boundary(&rest[end + 1..]);
    }
    if first.is_alphabetic() {
        return rest.strip_prefix(')').is_some_and(boundary);
    }
    let digits = line.trim_start_matches(|c: char| c.is_ascii_digit());
    let count = line.len() - digits.len();
    if count == 0 || count > 3 {
        return false;
    }
    if let Some(after) = digits.strip_prefix(')') {
        return boundary(after);
    }
    digits.strip_prefix('.').is_some_and(boundary)
}

fn ends_sentence(line: &str) -> bool {
    line.ends_with(['.', '!', '?', '。', '！', '？', ':', '"', '”', '’', '…', '」', '』', '）', '؟', '।'])
}

/// Whether the word after a hyphen is a conjunction that makes the hyphen a suspended one: "pre-" / "and post-processing".
fn suspended(text: &str) -> bool {
    let word = text.split(' ').next().unwrap_or("").to_lowercase();
    matches!(word.as_str(), "and" | "or" | "und" | "et" | "y" | "ou" | "oder")
}

fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(f32::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

fn height_differs(a: f32, b: f32) -> bool {
    a > 0.0 && b > 0.0 && (a - b).abs() > HEIGHT * a.max(b)
}

/// A non-blank line, with the block it is in.
struct Line<'a> {
    row: &'a Row,
    text: String,
    new_page: bool,
    height: f32,
    block: usize,
    /// The top of the blank row just before it, which splits the gap to the line above in two for the leading.
    blank_top: Option<f32>,
    /// Whether this is a smaller row inside the text around it (the limits of a sum), joined with a space and left out of every
    /// measure.
    attached: bool,
}

/// The extent of a block of lines, and whether its lines are justified.
struct Block {
    left: f32,
    right: f32,
    /// The index of its last line.
    last: usize,
    justified: bool,
}

/// What the paragraph decision measures: the blocks and the leading, in line heights, or 0 when there is none to use.
struct Layout<'a> {
    blocks: &'a [Block],
    usual: f32,
}

impl Layout<'_> {
    fn indented(&self, line: &Line) -> bool {
        line.row.left - self.blocks[line.block].left > INDENT * line.height
    }
}

/// Marks the smaller rows that overlap the line before them (the limits of a sum), or sit between two lines closer than the leading
/// `usual` (in line heights, 0 for none), inside the line's extent.
fn attach(lines: &mut [Line], usual: f32) {
    let mut anchor: Option<usize> = None;
    for i in 0..lines.len() {
        let attached = anchor.is_some_and(|a| {
            let (p, l) = (&lines[a], &lines[i]);
            let next = lines.get(i + 1).filter(|n| !n.new_page);
            let between = next.is_some_and(|n| usual > 0.0 && l.row.top < n.row.top && n.row.top - p.row.top < usual * p.height);
            !l.new_page
                && l.height < p.height * (1.0 - HEIGHT)
                && l.row.left >= p.row.left
                && l.row.right <= p.row.right
                && l.row.top > p.row.top
                && (l.row.top < p.row.bottom || between)
        });
        lines[i].attached = attached;
        if !attached {
            anchor = Some(i);
        }
    }
}

/// The leading in line heights: the `LEADING_QUANTILE` quantile of the gaps between the tops of lines, on a page, over the height of
/// the line above.
fn leading(lines: &[Line]) -> f32 {
    let mut ratios = Vec::new();
    let mut before: Option<&Line> = None;
    for line in lines.iter().filter(|l| !l.attached) {
        if let Some(p) = before
            && !line.new_page
            && p.height > 0.0
            && line.row.top > p.row.top
        {
            let middle = line.blank_top.unwrap_or(line.row.top);
            ratios.extend([middle - p.row.top, line.row.top - middle].into_iter().filter(|g| *g > 0.0).map(|g| g / p.height));
        }
        before = Some(line);
    }
    ratios.sort_by(f32::total_cmp);
    ratios.get((ratios.len().saturating_sub(1) as f32 * LEADING_QUANTILE) as usize).copied().unwrap_or(0.0)
}

/// Whether a paragraph ends between `prev` and `line` (`next` is the line after `line`), where `before` is the text so far, `item`
/// whether `line` starts a list item and `hanging` whether `prev` is a list item or its hanging continuation.
fn paragraph_break(layout: &Layout, line: &Line, prev: &Line, next: Option<&Line>, item: bool, hanging: bool, before: &str) -> bool {
    let block = &layout.blocks[prev.block];
    let short = prev.row.right < block.right - SHORT * (block.right - block.left);
    let ends = ends_sentence(before);
    let gap = line.row.top - prev.row.top;
    // A gap over `limit` leads, on a page.
    let spaced = |limit: f32| layout.usual > 0.0 && !line.new_page && gap > layout.usual * limit * prev.height;
    // A short line ending a sentence in ragged text needs a gap or an indented next line to count.
    let signal = block.justified || spaced(HEADING_GAP) || next.is_some_and(|n| n.block == line.block && layout.indented(n));
    spaced(GAP)
        || height_differs(prev.height, line.height)
        || (layout.indented(line) && !item && !hanging && (ends || (short && block.justified)))
        || (!item && short && ends && signal)
        || (short && before.chars().last().is_some_and(char::is_alphanumeric) && spaced(HEADING_GAP))
}

/// The lines joined into paragraphs: a space between lines of one, a hyphenated word made whole, `\n` before a list item and `\n\n`
/// between paragraphs.
pub fn reflow(rows: &[Row]) -> String {
    let rotated = rows.iter().filter(|r| r.rotated).count() * 2 > rows.len();

    let mut lines: Vec<Line> = Vec::new();
    let mut soft_after = Vec::new();
    let mut soft = false;
    let mut carry = false;
    let mut blank_top = None;
    for row in rows {
        let trimmed = row.text.trim_end();
        let ends_soft = trimmed.ends_with('\u{ad}');
        let text = trimmed.replace('\u{ad}', "");
        let text = text.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
        let new_page = row.new_page || carry;
        if text.is_empty() {
            carry = new_page;
            blank_top = Some(row.top);
            continue;
        }
        carry = false;
        soft_after.push(soft);
        soft = ends_soft;
        lines.push(Line { row, text, new_page, height: (row.bottom - row.top).abs(), block: 0, blank_top: blank_top.take(), attached: false });
    }

    attach(&mut lines, 0.0);
    let ratio = leading(&lines);
    attach(&mut lines, ratio);
    let usual = if rotated { 0.0 } else { leading(&lines) };
    let usual = if usual > MEANINGFUL { usual } else { 0.0 };

    let mut blocks: Vec<Block> = Vec::new();
    let mut anchor: Option<usize> = None;
    for i in 0..lines.len() {
        if let Some(a) = anchor.filter(|_| lines[i].attached) {
            lines[i].block = lines[a].block;
            continue;
        }
        let new = anchor.is_none_or(|a| {
            let (p, l) = (&lines[a], &lines[i]);
            l.new_page
                || height_differs(p.height, l.height)
                || (l.row.left - p.row.left).abs() > COLUMN * p.height
                || l.row.top < p.row.top - 0.5 * p.height
        });
        let block = if new {
            blocks.push(Block { left: f32::INFINITY, right: f32::NEG_INFINITY, last: i, justified: true });
            blocks.len() - 1
        } else {
            anchor.map_or(0, |a| lines[a].block)
        };
        lines[i].block = block;
        let edges = &mut blocks[block];
        edges.left = edges.left.min(lines[i].row.left);
        edges.right = edges.right.max(lines[i].row.right);
        edges.last = i;
        anchor = Some(i);
    }
    let mut counts = vec![(0usize, 0usize); blocks.len()];
    for (i, line) in lines.iter().enumerate().filter(|(_, l)| !l.attached) {
        let block = &blocks[line.block];
        if i != block.last {
            counts[line.block].0 += 1;
            counts[line.block].1 += usize::from(line.row.right >= block.right - JUSTIFIED * (block.right - block.left));
        }
    }
    for (block, (all, near)) in blocks.iter_mut().zip(counts) {
        block.justified = near as f32 >= JUSTIFIED_SHARE * all as f32;
    }
    let layout = Layout { blocks: &blocks, usual };

    let mut out = String::new();
    let mut item_before = false;
    let mut hanging = false;
    let mut prev: Option<&Line> = None;
    for (i, line) in lines.iter().enumerate() {
        let text = &line.text;
        if line.attached {
            out.push(' ');
            out.push_str(text);
            continue;
        }
        let item = starts_item(text, i == 0 || item_before || ends_sentence(&out));
        let Some(before) = prev.replace(line) else {
            out.push_str(text);
            item_before = item;
            hanging = item;
            continue;
        };
        let next = lines[i + 1..].iter().find(|l| !l.attached);
        let paragraph = !rotated && paragraph_break(&layout, line, before, next, item, hanging, &out);
        hanging = item || (!paragraph && hanging && layout.indented(line));
        let last = out.chars().last();
        let first = text.chars().next();
        let hyphen = last.is_some_and(|l| matches!(l, '-' | '\u{2010}'));
        let dash = hyphen || last == Some('\u{2013}');
        let back = || out.chars().rev().skip(1).find(|c| !is_combining(*c));
        if paragraph {
            out.push_str("\n\n");
        } else if item {
            out.push('\n');
        } else if soft_after[i] || (dash && first.is_some_and(|f| f.is_ascii_digit()) && back().is_some_and(|c| c.is_ascii_digit())) {
        } else if hyphen && first.is_some_and(char::is_lowercase) && back().is_some_and(char::is_alphabetic) && !suspended(text) {
            out.pop();
        } else if !last.zip(first).is_some_and(|(l, f)| is_cjk(l) && is_cjk(f)) {
            out.push(' ');
        }
        out.push_str(text);
        item_before = item;
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10-point line at line `n`, 12 points apart, as wide as `width` from `left`.
    fn row(text: &str, n: f32, left: f32, width: f32) -> Row {
        Row { text: text.to_owned(), left, right: left + width, top: n * 12.0, bottom: n * 12.0 + 10.0, new_page: false, rotated: false }
    }

    fn full(text: &str, n: f32) -> Row {
        row(text, n, 0.0, 100.0)
    }

    #[test]
    fn a_paragraph_is_joined_with_spaces() {
        let rows = [full("one two", 0.0), full("three four", 1.0), full("five", 2.0)];
        assert_eq!(reflow(&rows), "one two three four five");
    }

    #[test]
    fn a_hyphenated_word_is_made_whole() {
        assert_eq!(reflow(&[full("an exam-", 0.0), full("ple of it", 1.0)]), "an example of it");
        assert_eq!(reflow(&[full("an exam\u{2010}", 0.0), full("ple of it", 1.0)]), "an example of it");
    }

    #[test]
    fn a_hyphen_before_a_capital_or_after_a_space_is_kept() {
        assert_eq!(reflow(&[full("a well-", 0.0), full("Known fact", 1.0)]), "a well- Known fact");
        assert_eq!(reflow(&[full("this -", 0.0), full("that", 1.0)]), "this - that");
        assert_eq!(reflow(&[full("this \u{2014}", 0.0), full("that", 1.0)]), "this \u{2014} that");
    }

    #[test]
    fn a_soft_hyphen_is_dropped() {
        assert_eq!(reflow(&[full("exam\u{ad}", 0.0), full("Ple", 1.0)]), "examPle");
        assert_eq!(reflow(&[full("co\u{ad}operate fully", 0.0)]), "cooperate fully");
    }

    #[test]
    fn a_larger_gap_starts_a_paragraph() {
        let rows = [full("one", 0.0), full("two", 1.0), full("three", 2.0), full("four", 4.0), full("five", 5.0)];
        assert_eq!(reflow(&rows), "one two three\n\nfour five");
    }

    #[test]
    fn a_short_line_ending_a_sentence_starts_a_paragraph() {
        let rows = [full("one two", 0.0), row("three.", 1.0, 0.0, 30.0), full("Four five", 2.0)];
        assert_eq!(reflow(&rows), "one two three.\n\nFour five");
        let rows = [full("one two", 0.0), row("three", 1.0, 0.0, 30.0), full("four", 2.0)];
        assert_eq!(reflow(&rows), "one two three four");
    }

    #[test]
    fn an_indented_line_starts_a_paragraph() {
        let rows = [row("one two.", 0.0, 0.0, 100.0), row("three four", 1.0, 20.0, 80.0), row("five", 2.0, 0.0, 100.0)];
        assert_eq!(reflow(&rows), "one two.\n\nthree four five");
    }

    #[test]
    fn list_items_keep_their_own_lines() {
        let rows = [full("Things:", 0.0), full("• one", 1.0), full("- two", 2.0), full("3. three", 3.0), full("4) four", 4.0), full("2024 was", 5.0)];
        assert_eq!(reflow(&rows), "Things:\n• one\n- two\n3. three\n4) four 2024 was");
    }

    #[test]
    fn a_paragraph_continues_across_a_page() {
        let mut second = full("ple of it", 0.0);
        second.new_page = true;
        assert_eq!(reflow(&[full("one exam-", 5.0), second]), "one example of it");
        let mut third = full("next", 0.0);
        third.new_page = true;
        assert_eq!(reflow(&[full("one", 5.0), full("two", 6.0), third]), "one two next");
    }

    #[test]
    fn one_line_and_no_lines() {
        assert_eq!(reflow(&[full("  just   one  line ", 0.0)]), "just one line");
        assert_eq!(reflow(&[]), "");
    }

    #[test]
    fn cjk_lines_are_joined_without_a_space() {
        assert_eq!(reflow(&[full("日本語の", 0.0), full("文章です", 1.0)]), "日本語の文章です");
        assert_eq!(reflow(&[full("日本語", 0.0), full("text", 1.0)]), "日本語 text");
    }

    #[test]
    fn two_lines_have_one_gap_as_the_median() {
        assert_eq!(reflow(&[full("one", 0.0), full("two", 1.0)]), "one two");
        assert_eq!(reflow(&[full("one", 0.0), full("two", 5.0)]), "one two");
    }

    #[test]
    fn lines_with_identical_tops_are_joined_without_paragraphs() {
        let rows = [full("one", 0.0), full("two", 0.0), full("three", 0.0)];
        assert_eq!(reflow(&rows), "one two three");
    }

    #[test]
    fn a_hyphen_after_a_digit_is_kept() {
        assert_eq!(reflow(&[full("pages 10-", 0.0), full("20 of it", 1.0)]), "pages 10-20 of it");
        assert_eq!(reflow(&[full("B1-", 0.0), full("x", 1.0)]), "B1- x");
    }

    #[test]
    fn an_em_dash_at_the_line_end_is_kept_with_a_space() {
        assert_eq!(reflow(&[full("wait\u{2014}", 0.0), full("what", 1.0)]), "wait\u{2014} what");
        assert_eq!(reflow(&[full("wait\u{2013}", 0.0), full("what", 1.0)]), "wait\u{2013} what");
    }

    #[test]
    fn non_latin_lowercase_after_a_hyphen_is_joined() {
        assert_eq!(
            reflow(&[full("\u{3b5}\u{3be}\u{3ae}-", 0.0), full("\u{3b3}\u{3b7}\u{3c3}\u{3b7}", 1.0)]),
            "\u{3b5}\u{3be}\u{3ae}\u{3b3}\u{3b7}\u{3c3}\u{3b7}"
        );
        assert_eq!(
            reflow(&[full("\u{43f}\u{440}\u{438}\u{43c}\u{435}-", 0.0), full("\u{440} text", 1.0)]),
            "\u{43f}\u{440}\u{438}\u{43c}\u{435}\u{440} text"
        );
        assert_eq!(
            reflow(&[full("\u{43f}\u{440}\u{438}-", 0.0), full("\u{41c}\u{435}\u{440}", 1.0)]),
            "\u{43f}\u{440}\u{438}- \u{41c}\u{435}\u{440}"
        );
    }

    #[test]
    fn a_whitespace_only_line_is_skipped() {
        assert_eq!(reflow(&[full("   ", 0.0)]), "");
        assert_eq!(reflow(&[full("a", 0.0), full("  \t ", 1.0), full("b", 1.0)]), "a b");
        // A blank line between two lines widens the measured gap past the paragraph threshold: current behaviour.
        assert_eq!(reflow(&[full("a", 0.0), full("b", 1.0), full("c", 2.0), full(" ", 3.0), full("d", 4.0)]), "a b c\n\nd");
    }

    #[test]
    fn a_whitespace_only_line_after_a_soft_hyphen_breaks_the_paragraph() {
        assert_eq!(reflow(&[full("exam\u{ad}", 0.0), full(" ", 1.0), full("ple", 2.0)]), "exam\n\nple");
    }

    #[test]
    fn a_very_long_selection_is_fast() {
        let rows: Vec<Row> = (0..5000).map(|n| full(&format!("word{n} exam-"), n as f32)).collect();
        let start = std::time::Instant::now();
        let out = reflow(&rows);
        assert!(start.elapsed().as_secs_f32() < 2.0, "{:?}", start.elapsed());
        assert!(out.starts_with("word0 examword1 exam"), "{}", &out[..40]);
        assert!(!out.contains('\n'));
        let plain: Vec<Row> = (0..5000).map(|n| full("some text here", n as f32)).collect();
        assert_eq!(reflow(&plain).matches("some").count(), 5000);
    }

    #[test]
    fn a_wrapped_line_starting_with_a_number_is_taken_for_an_item() {
        // A "3. " counts only after a sentence end or an item.
        assert_eq!(reflow(&[full("as shown in section", 0.0), full("3. the next", 1.0)]), "as shown in section 3. the next");
        assert_eq!(reflow(&[full("as shown.", 0.0), full("3. the next", 1.0)]), "as shown.\n3. the next");
        // A number without the dot and a space is not an item.
        assert_eq!(reflow(&[full("as shown in", 0.0), full("3.5 percent", 1.0)]), "as shown in 3.5 percent");
    }

    #[test]
    fn cjk_and_latin_at_a_line_boundary_get_a_space() {
        assert_eq!(reflow(&[full("text", 0.0), full("\u{65e5}\u{672c}", 1.0)]), "text \u{65e5}\u{672c}");
        assert_eq!(reflow(&[full("\u{65e5}\u{672c}", 0.0), full("text", 1.0)]), "\u{65e5}\u{672c} text");
        assert_eq!(reflow(&[full("\u{65e5}\u{672c}\u{ff0c}", 0.0), full("\u{8a9e}", 1.0)]), "\u{65e5}\u{672c}\u{ff0c}\u{8a9e}");
    }

    #[test]
    fn a_soft_hyphen_in_the_middle_of_a_line_is_dropped() {
        assert_eq!(reflow(&[full("a co\u{ad}op\u{ad}er\u{ad}ate b", 0.0), full("next", 1.0)]), "a cooperate b next");
    }

    fn glyph(c: char, x: f32, y: f32, w: f32) -> pdfcraft_render::TextGlyph {
        let rect = [x, y, x + w, y + 10.0];
        let quad = [[x, y + 10.0], [x + w, y + 10.0], [x + w, y], [x, y]];
        pdfcraft_render::TextGlyph { text: c.to_string(), rect, quad, direction: [1.0, 0.0] }
    }

    /// Lays `lines` out as justified lines 12 points apart from `top`, `indent` points in for the first, 200 points wide.
    fn page_of(lines: &[(&str, f32)], top: f32) -> PageText {
        let mut text = PageText { glyphs: vec![], line_of: vec![], space_before: vec![], truncated: false };
        for (n, (line, indent)) in lines.iter().enumerate() {
            let count = line.chars().count() as f32;
            let w = (200.0 - indent) / count;
            let mut space = false;
            for (i, c) in line.chars().enumerate() {
                if c == ' ' {
                    space = true;
                    continue;
                }
                text.glyphs.push(glyph(c, indent + i as f32 * w, top + n as f32 * 12.0, w));
                text.line_of.push(n as u32);
                text.space_before.push(std::mem::take(&mut space));
            }
        }
        text
    }

    #[test]
    fn real_shaped_text_reflows_into_paragraphs() {
        let page = page_of(
            &[
                ("Reflowing text is hard because a para-", 0.0),
                ("graph is only lines of glyphs on a page", 0.0),
                ("with no markup. It ends here.", 0.0),
                ("", 0.0),
                ("The second paragraph starts after a gap", 0.0),
                ("and carries on.", 0.0),
            ],
            50.0,
        );
        let mut rows = Vec::new();
        rows_of(&page, 0..page.glyphs.len(), &mut rows);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].text, "Reflowing text is hard because a para-");
        assert_eq!(
            reflow(&rows),
            "Reflowing text is hard because a paragraph is only lines of glyphs on a page with no markup. It ends here.\n\nThe second paragraph starts after a gap and carries on."
        );
        // A range starting mid-line and ending mid-line.
        let mut part = Vec::new();
        rows_of(&page, 3..page.glyphs.len() - 20, &mut part);
        assert!(!part.is_empty() && part[0].new_page && !part[1].new_page);
        // A range past the end is clipped.
        let mut clipped = Vec::new();
        rows_of(&page, 0..page.glyphs.len() + 50, &mut clipped);
        assert_eq!(clipped.len(), 5);
    }

    #[test]
    fn rows_of_on_two_pages_marks_the_first_row_of_each_page() {
        let page = page_of(&[("an exam-", 0.0), ("ple", 0.0)], 700.0);
        let mut rows = Vec::new();
        rows_of(&page, 0..page.glyphs.len(), &mut rows);
        rows_of(&page, 0..page.glyphs.len(), &mut rows);
        assert_eq!(rows.iter().map(|r| r.new_page).collect::<Vec<_>>(), [true, false, true, false]);
        assert_eq!(reflow(&rows), "an example an example");
    }

    /// A 10-point line from `left` to `right` with its top at `top`.
    fn at(text: &str, top: f32, left: f32, right: f32) -> Row {
        Row { text: text.to_owned(), left, right, top, bottom: top + 10.0, new_page: false, rotated: false }
    }

    #[test]
    fn a_selection_across_two_columns_is_one_paragraph() {
        let rows = [
            at("one two", 0.0, 0.0, 100.0),
            at("three four", 12.0, 0.0, 100.0),
            at("five six", 24.0, 0.0, 100.0),
            at("seven eight", 0.0, 150.0, 250.0),
            at("nine ten", 12.0, 150.0, 250.0),
        ];
        assert_eq!(reflow(&rows), "one two three four five six seven eight nine ten");
    }

    #[test]
    fn two_pages_with_different_left_margins_join() {
        let mut third = at("five six", 0.0, 30.0, 130.0);
        third.new_page = true;
        let rows = [at("one two", 0.0, 0.0, 100.0), at("three four", 12.0, 0.0, 100.0), third, at("seven", 12.0, 30.0, 130.0)];
        assert_eq!(reflow(&rows), "one two three four five six seven");
    }

    #[test]
    fn a_full_width_title_above_two_columns() {
        let mut title = at("A Big Title", 0.0, 0.0, 400.0);
        title.bottom = 20.0;
        let rows = [
            title,
            at("alpha beta", 36.0, 0.0, 190.0),
            at("gamma delta", 48.0, 0.0, 190.0),
            at("epsilon zeta", 36.0, 210.0, 400.0),
            at("eta theta", 48.0, 210.0, 400.0),
        ];
        assert_eq!(reflow(&rows), "A Big Title\n\nalpha beta gamma delta epsilon zeta eta theta");
    }

    #[test]
    fn a_hanging_indent_item_keeps_its_continuation() {
        let rows = [
            at("Things:", 0.0, 0.0, 100.0),
            at("\u{2022} first item that runs", 12.0, 20.0, 100.0),
            at("onto another", 24.0, 29.0, 100.0),
            at("\u{2022} second item", 36.0, 20.0, 60.0),
            at("Next paragraph here.", 52.8, 0.0, 100.0),
        ];
        assert_eq!(reflow(&rows), "Things:\n\u{2022} first item that runs onto another\n\u{2022} second item\n\nNext paragraph here.");
    }

    #[test]
    fn a_bibliography_entry_keeps_its_continuation() {
        let rows = [
            at("[1] A. Author. Title that", 0.0, 0.0, 100.0),
            at("wraps here.", 12.0, 20.0, 60.0),
            at("[2] B. Other. A longer", 24.0, 0.0, 100.0),
            at("title ends.", 36.0, 20.0, 60.0),
        ];
        assert_eq!(reflow(&rows), "[1] A. Author. Title that wraps here.\n[2] B. Other. A longer title ends.");
    }

    #[test]
    fn a_selection_starting_mid_line_on_a_sentence_end_is_not_a_break() {
        let mut first = at("end of it.", 0.0, 70.0, 100.0);
        first.new_page = true;
        let rows = [first, at("Next line", 12.0, 0.0, 100.0), at("and more", 24.0, 0.0, 100.0)];
        assert_eq!(reflow(&rows), "end of it. Next line and more");
        // The same line ending short of the right edge is a real short last line: a break.
        let mut first = at("end of it.", 0.0, 25.0, 60.0);
        first.new_page = true;
        let rows = [first, at("Next line", 12.0, 0.0, 100.0), at("and more", 24.0, 0.0, 100.0)];
        assert_eq!(reflow(&rows), "end of it.\n\nNext line and more");
    }

    #[test]
    fn a_hanging_indent_without_a_marker_is_one_paragraph_per_entry() {
        let rows = [
            at("Smith, A. (2001). A long title that", 0.0, 0.0, 100.0),
            at("wraps around once more and", 12.0, 20.0, 100.0),
            at("ends here.", 24.0, 20.0, 60.0),
            at("Jones, B. (2003). Another title that", 36.0, 0.0, 100.0),
            at("wraps around.", 48.0, 20.0, 70.0),
        ];
        assert_eq!(
            reflow(&rows),
            "Smith, A. (2001). A long title that wraps around once more and ends here.\n\nJones, B. (2003). Another title that wraps around."
        );
    }

    #[test]
    fn latex_one_line_paragraphs_break_before_each_indent() {
        let rows = [
            at("A first line of the paragraph that", 0.0, 0.0, 100.0),
            at("runs on and ends here.", 12.0, 0.0, 100.0),
            at("One-line para B is short.", 24.0, 15.0, 80.0),
            at("C starts with an indent and", 36.0, 15.0, 100.0),
            at("wraps once.", 48.0, 0.0, 50.0),
        ];
        assert_eq!(
            reflow(&rows),
            "A first line of the paragraph that runs on and ends here.\n\nOne-line para B is short.\n\nC starts with an indent and wraps once."
        );
    }

    #[test]
    fn ragged_right_lines_ending_a_sentence_early_do_not_break() {
        let rows = [
            at("Ragged text can end a", 0.0, 0.0, 90.0),
            at("sentence early.", 12.0, 0.0, 55.0),
            at("and then it goes on for a", 24.0, 0.0, 100.0),
            at("while longer.", 36.0, 0.0, 60.0),
        ];
        assert_eq!(reflow(&rows), "Ragged text can end a sentence early. and then it goes on for a while longer.");
        // A gap above the leading is a second signal.
        let rows = [
            at("Ragged text can end a", 0.0, 0.0, 90.0),
            at("sentence early.", 12.0, 0.0, 55.0),
            at("Then more", 26.0, 0.0, 100.0),
            at("text here", 38.0, 0.0, 70.0),
        ];
        assert_eq!(reflow(&rows), "Ragged text can end a sentence early.\n\nThen more text here");
    }

    #[test]
    fn body_text_with_footnotes_of_another_size_is_not_split_by_row() {
        let sized = |text: &str, top: f32, height: f32| Row {
            text: text.to_owned(),
            left: 0.0,
            right: 100.0,
            top,
            bottom: top + height,
            new_page: false,
            rotated: false,
        };
        let rows = [
            sized("body one", 0.0, 12.0),
            sized("body two", 14.4, 12.0),
            sized("body three", 28.8, 12.0),
            sized("body four", 43.2, 12.0),
            sized("note one", 60.0, 8.0),
            sized("note two", 69.6, 8.0),
            sized("note three", 79.2, 8.0),
        ];
        assert_eq!(reflow(&rows), "body one body two body three body four\n\nnote one note two note three");
    }

    #[test]
    fn a_citation_or_an_equation_reference_at_a_wrap_is_not_an_item() {
        assert_eq!(
            reflow(&[full("shown by many authors in the", 0.0), full("[12] who showed", 1.0)]),
            "shown by many authors in the [12] who showed"
        );
        assert_eq!(reflow(&[full("by equation", 0.0), full("(2) we obtain", 1.0)]), "by equation (2) we obtain");
        assert_eq!(reflow(&[full("as in item", 0.0), full("a) the first", 1.0)]), "as in item a) the first");
        assert_eq!(reflow(&[full("shown.", 0.0), full("[12] who", 1.0)]), "shown.\n[12] who");
        assert_eq!(reflow(&[full("shown.", 0.0), full("(2) we", 1.0)]), "shown.\n(2) we");
    }

    #[test]
    fn an_en_dash_range_at_a_wrap_joins_without_a_space() {
        assert_eq!(reflow(&[full("pages 10\u{2013}", 0.0), full("20 of it", 1.0)]), "pages 10\u{2013}20 of it");
    }

    #[test]
    fn short_paragraphs_are_told_apart_by_the_low_quantile_of_the_gaps() {
        let rows = [
            at("Alpha one.", 0.0, 0.0, 100.0),
            at("Beta two", 17.0, 0.0, 100.0),
            at("continues here.", 29.0, 0.0, 100.0),
            at("Gamma three.", 46.0, 0.0, 100.0),
            at("Delta four", 63.0, 0.0, 100.0),
            at("ends here.", 75.0, 0.0, 100.0),
            at("Epsilon six.", 92.0, 0.0, 100.0),
        ];
        assert_eq!(reflow(&rows), "Alpha one.\n\nBeta two continues here.\n\nGamma three.\n\nDelta four ends here.\n\nEpsilon six.");
    }

    #[test]
    fn a_heading_with_a_small_gap_still_breaks() {
        let rows = [
            at("Introduction", 0.0, 0.0, 40.0),
            at("one two", 15.6, 0.0, 100.0),
            at("three four", 27.6, 0.0, 100.0),
            at("five six", 39.6, 0.0, 100.0),
            at("seven", 51.6, 0.0, 100.0),
        ];
        assert_eq!(reflow(&rows), "Introduction\n\none two three four five six seven");
    }

    #[test]
    fn a_larger_heading_breaks_before_and_after() {
        let mut heading = at("Methods", 12.0, 0.0, 100.0);
        heading.bottom = 32.0;
        let rows = [at("one two", 0.0, 0.0, 100.0), heading, at("three four", 36.0, 0.0, 100.0), at("five", 48.0, 0.0, 100.0)];
        assert_eq!(reflow(&rows), "one two\n\nMethods\n\nthree four five");
    }

    #[test]
    fn a_tall_inline_formula_does_not_move_the_line() {
        let mut page = page_of(&[("aaaaa", 0.0), ("bbbbb", 0.0), ("ccccc", 0.0), ("ddddd", 0.0)], 50.0);
        page.glyphs[5].rect[1] -= 20.0;
        page.glyphs[5].rect[3] += 20.0;
        let mut rows = Vec::new();
        rows_of(&page, 0..page.glyphs.len(), &mut rows);
        assert_eq!((rows[1].top, rows[1].bottom), (62.0, 72.0));
        assert_eq!(reflow(&rows), "aaaaa bbbbb ccccc ddddd");
    }

    #[test]
    fn rotated_text_only_joins_lines() {
        let mut page = page_of(&[("one", 0.0), ("two", 100.0), ("three", 0.0)], 50.0);
        for glyph in &mut page.glyphs {
            glyph.direction = [0.0, 1.0];
        }
        let mut rows = Vec::new();
        rows_of(&page, 0..page.glyphs.len(), &mut rows);
        assert!(rows.iter().all(|r| r.rotated));
        assert_eq!(reflow(&rows), "one two three");
    }

    #[test]
    fn korean_lines_are_joined_with_a_space() {
        assert_eq!(
            reflow(&[full("\u{d55c}\u{ad6d}\u{c5b4} \u{bb38}\u{c7a5}", 0.0), full("\u{c774}\u{c5b4}\u{c9d1}\u{b2c8}\u{b2e4}", 1.0)]),
            "\u{d55c}\u{ad6d}\u{c5b4} \u{bb38}\u{c7a5} \u{c774}\u{c5b4}\u{c9d1}\u{b2c8}\u{b2e4}"
        );
    }

    #[test]
    fn a_wrapped_year_is_not_an_item() {
        assert_eq!(reflow(&[full("It was in the year", 0.0), full("1999. Then it ended", 1.0)]), "It was in the year 1999. Then it ended");
        assert_eq!(reflow(&[full("It was in the year", 0.0), full("199. Then it ended", 1.0)]), "It was in the year 199. Then it ended");
    }

    #[test]
    fn a_suspended_hyphen_is_kept() {
        assert_eq!(reflow(&[full("pre-", 0.0), full("and post-processing", 1.0)]), "pre- and post-processing");
        assert_eq!(reflow(&[full("vor-", 0.0), full("oder nach", 1.0)]), "vor- oder nach");
    }

    #[test]
    fn a_combining_mark_before_the_hyphen_still_joins() {
        assert_eq!(reflow(&[full("ide\u{301}-", 0.0), full("es", 1.0)]), "ide\u{301}es");
    }

    #[test]
    fn more_item_markers_are_recognised() {
        let rows = [
            full("Things:", 0.0),
            full("(a) one", 1.0),
            full("b) two", 2.0),
            full("[1] three", 3.0),
            full("\u{25cf} four", 4.0),
            full("\u{25aa} five", 5.0),
            full("\u{25e6} six", 6.0),
            full("\u{2023} seven", 7.0),
            full("\u{2219} eight", 8.0),
            full("\u{f0b7} nine", 9.0),
            full("\u{f0a7} ten", 10.0),
        ];
        assert_eq!(
            reflow(&rows),
            "Things:\n(a) one\nb) two\n[1] three\n\u{25cf} four\n\u{25aa} five\n\u{25e6} six\n\u{2023} seven\n\u{2219} eight\n\u{f0b7} nine\n\u{f0a7} ten"
        );
    }

    #[test]
    fn a_blank_first_row_of_a_page_carries_its_page_mark() {
        let mut blank = full(" ", 0.0);
        blank.new_page = true;
        let rows = [full("one", 5.0), full("two", 6.0), blank, full("three", 40.0)];
        assert_eq!(reflow(&rows), "one two three");
    }

    // Geometry below is from a typst PDF (12 x 14 cm pages, 10 pt text, 13.1 pt leading) read through `PageText`.

    /// A 10-point line with its box and top as measured, on a new page when `page`.
    fn real(text: &str, top: f32, left: f32, right: f32, page: bool) -> Row {
        Row { text: text.to_owned(), left, right, top, bottom: top + 10.0, new_page: page, rotated: false }
    }

    #[test]
    fn typst_heading_hyphenated_paragraph_and_next_paragraph() {
        let mut heading = real("Introduction to Reflow", 40.3, 42.5, 188.1, true);
        heading.bottom = 54.3;
        let rows = [
            heading,
            real("Reflowing extraordinarily complicated documentation requires ", 61.3, 42.5, 300.1, false),
            real("understanding internationalization, characterization, and mis\u{ad}", 74.4, 42.5, 299.5, false),
            real("understandings of typographical conventions thoroughly ", 87.5, 42.5, 300.1, false),
            real("everywhere.", 100.6, 42.5, 93.0, false),
            real("The second paragraph is short and plain, but it still wraps onto ", 119.1, 42.5, 300.1, false),
            real("a second line so that we can see it.", 132.2, 42.5, 182.9, false),
        ];
        assert_eq!(
            reflow(&rows),
            "Introduction to Reflow\n\nReflowing extraordinarily complicated documentation requires understanding internationalization, characterization, \
             and misunderstandings of typographical conventions thoroughly everywhere.\n\nThe second paragraph is short and plain, but it still wraps onto \
             a second line so that we can see it."
        );
    }

    #[test]
    fn typst_bullets_with_hanging_indent_then_a_paragraph_then_numbered_items() {
        let rows = [
            real("Here is a list:", 150.8, 42.5, 95.8, true),
            real("\u{2022} The first bullet item has a lot of text that wraps over several ", 163.9, 42.5, 300.1, false),
            real("lines so that the hanging indent shows up properly.", 177.0, 51.0, 261.0, false),
            real("\u{2022} Second item.", 190.0, 42.5, 103.2, false),
            real("\u{2022} Third item that is also rather long and wraps around onto ", 203.1, 42.5, 300.1, false),
            real("another line of the page.", 216.2, 51.0, 150.5, false),
            real("A normal paragraph after the list that wraps onto a second line, ", 234.8, 42.5, 301.9, false),
            real("to see whether it is glued to the item.", 247.9, 42.5, 194.4, false),
            real("1. First numbered item with enough words to wrap onto a ", 266.4, 42.5, 300.1, false),
            real("second line of the page here.", 279.5, 54.4, 170.8, false),
            real("2. Second numbered.", 292.6, 42.5, 129.6, false),
            real("3. Third numbered item that again is long enough to wrap onto ", 305.7, 42.5, 300.1, false),
            real("the next line of the page.", 318.8, 54.4, 155.5, false),
        ];
        assert_eq!(
            reflow(&rows),
            "Here is a list:\n\u{2022} The first bullet item has a lot of text that wraps over several lines so that the hanging indent shows up properly.\n\
             \u{2022} Second item.\n\u{2022} Third item that is also rather long and wraps around onto another line of the page.\n\n\
             A normal paragraph after the list that wraps onto a second line, to see whether it is glued to the item.\n\n\
             1. First numbered item with enough words to wrap onto a second line of the page here.\n2. Second numbered.\n\
             3. Third numbered item that again is long enough to wrap onto the next line of the page."
        );
    }

    #[test]
    fn typst_columns_with_offset_baselines_selected_from_the_end_of_column_one() {
        let rows = [
            real("aeque doleamus animo, cum ", 140.0, 42.5, 166.9, true),
            real("corpore dolemus, fieri tamen ", 153.1, 42.5, 166.9, false),
            real("permagna accessio potest, si ", 166.2, 42.5, 166.9, false),
            real("aliquod aeternum et infini\u{ad}", 55.5, 175.2, 299.0, false),
            real("tum impendere malum nobis ", 68.6, 175.2, 299.6, false),
            real("opinemur. Quod idem licet ", 81.7, 175.2, 299.6, false),
            real("transferre in voluptatem", 94.8, 175.2, 280.9, false),
        ];
        assert_eq!(
            reflow(&rows),
            "aeque doleamus animo, cum corpore dolemus, fieri tamen permagna accessio potest, si aliquod aeternum et infinitum impendere malum nobis \
             opinemur. Quod idem licet transferre in voluptatem"
        );
    }

    #[test]
    fn typst_latex_indents_and_a_paragraph_across_a_page_break() {
        let rows = [
            real("Latex style paragraph two with an indent Lorem ipsum dolor ", 308.2, 52.5, 300.1, false),
            real("sit amet, consectetur adipiscing elit, sed do eiusmod tempor ", 321.3, 42.5, 300.1, false),
            real("incididunt ut labore et dolore magnam aliquam quaerat volup\u{ad}", 41.1, 42.5, 299.5, true),
            real("tatem. Ut enim aeque doleamus. end of two.", 54.2, 42.5, 221.5, false),
            real("Latex style paragraph three Lorem ipsum dolor sit amet, ", 67.3, 52.5, 301.9, false),
            real("consectetur adipiscing elit, sed do eiusmod tempor. end of three.", 80.3, 42.5, 299.4, false),
        ];
        assert_eq!(
            reflow(&rows),
            "Latex style paragraph two with an indent Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
             incididunt ut labore et dolore magnam aliquam quaerat voluptatem. Ut enim aeque doleamus. end of two.\n\n\
             Latex style paragraph three Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor. end of three."
        );
    }

    #[test]
    fn typst_japanese_lines_join_without_spaces() {
        let rows = [
            real("日本語の文章はスペースがありません。この文章はとても", 41.8, 42.5, 297.6, true),
            real("長いので次の行に折り返されなければなりませんから続", 55.7, 42.5, 297.6, false),
            real("けて書きます。終わり。", 69.5, 42.5, 151.3, false),
        ];
        assert_eq!(
            reflow(&rows),
            "日本語の文章はスペースがありません。この文章はとても長いので次の行に折り返されなければなりませんから続けて書きます。終わり。"
        );
    }

    #[test]
    fn typst_block_quote_stays_one_paragraph() {
        // The quote is inset 15 points; its lines are 57.5 wide of the 42.5 margin. A line inside an indented run is no new paragraph.
        let rows = [
            real("Quote: To be or not to be, that is the question, whether ", 261.4, 57.5, 284.1, true),
            real("it is nobler in the mind to suffer the slings and arrows ", 274.5, 57.5, 284.1, false),
            real("of outrageous fortune.", 287.5, 57.5, 148.6, false),
            real("After the quote, a normal line.", 306.1, 42.5, 165.4, false),
        ];
        assert_eq!(
            reflow(&rows),
            "Quote: To be or not to be, that is the question, whether it is nobler in the mind to suffer the slings and arrows of outrageous fortune.\n\n\
             After the quote, a normal line."
        );
    }

    #[test]
    fn typst_tall_inline_formula_with_a_limit_row_does_not_split_the_paragraph() {
        // `$sum_(i=1)^n x_i / y_i$`: the limits `i=1` are their own row (7 point high, 6.9 below the line top), attached to the text.
        let rows = [
            real(
                "A line with a tall formula \u{2211}\u{1d45b} \u{1d465}\u{1d456} \u{1d466}\u{1d456} inside the paragraph text ",
                88.3,
                42.5,
                300.1,
                true,
            ),
            Row { text: "\u{1d456}=1".to_owned(), left: 164.8, right: 177.0, top: 95.2, bottom: 102.2, new_page: false, rotated: false },
            real("that wraps to a second line and carries on", 102.1, 42.5, 212.8, false),
        ];
        assert_eq!(
            reflow(&rows),
            "A line with a tall formula \u{2211}\u{1d45b} \u{1d465}\u{1d456} \u{1d466}\u{1d456} inside the paragraph text \u{1d456}=1 that wraps to a second line and carries on"
        );
    }

    #[test]
    #[ignore = "known: pdfcraft merges both columns' lines into one row when the columns share a baseline grid (render/src/text/layout.rs, \"Known limit\")"]
    fn typst_columns_on_one_baseline_grid_are_read_column_by_column() {
        // From `#columns(2)` of typst: the first six lines of column two sit on the baselines of column one, so `line_of` is
        // shared and the glyph order alternates column one, column two, line by line. Each word is one glyph here.
        let left = ["Lorem ipsum dolor sit amet,", "consectetur adipiscing elit,", "sed do eiusmod tempor"];
        let right = ["doleamus animo, cum", "pore dolemus, fieri", "permagna accessio potest,"];
        let mut text = PageText { glyphs: vec![], line_of: vec![], space_before: vec![], truncated: false };
        for n in 0..3 {
            for (line, x) in [(left[n], 42.5), (right[n], 175.2)] {
                for (k, word) in line.split(' ').enumerate() {
                    let mut g = glyph('x', x + k as f32 * 20.0, 58.4 + n as f32 * 13.1, 18.0);
                    g.text = word.to_owned();
                    text.glyphs.push(g);
                    text.line_of.push(n as u32);
                    text.space_before.push(k > 0);
                }
            }
        }
        let mut rows = Vec::new();
        rows_of(&text, 0..text.glyphs.len(), &mut rows);
        assert_eq!(
            reflow(&rows),
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor doleamus animo, cum pore dolemus, fieri permagna accessio potest,"
        );
    }
}
