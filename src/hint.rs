//! Hint mode: labels over what can be picked, chosen by typing.
use pdfcraft_render::PageText;

use crate::app::PlaceItem;
use crate::config::HintConfig;
use crate::view::unrotate_point;

/// The font size of every label at `zoom`: `font_size` scaled by the zoom, kept between
/// `min_font_size` and `font_size`, whichever of the two is smaller being the minimum. Without
/// `scale` it is `font_size`.
pub fn label_size(zoom: f32, config: &HintConfig) -> f32 {
    let (max, min) = (config.font_size, config.min_font_size.min(config.font_size));
    if config.scale { (max * zoom).clamp(min, max) } else { max }
}

/// The padding in points on each side of a key in its cell.
pub const PAD: f32 = 1.0;

/// Where the keys of a label sit: every key is centred in a cell of the same width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellLayout {
    pub cell_width: f32,
    pub box_width: f32,
}

impl CellLayout {
    /// The cells of a label of `keys` characters, each `glyph_width` wide.
    pub fn new(keys: usize, glyph_width: f32, pad: f32) -> Self {
        let cell_width = glyph_width + 2.0 * pad;
        CellLayout { cell_width, box_width: cell_width * keys as f32 }
    }

    /// The offset from the left of the box to the cell of key `i`.
    pub fn cell_x(&self, i: usize) -> f32 {
        self.cell_width * i as f32
    }

    /// The offset from the left of the box to the glyph of key `i`, centred in its cell.
    pub fn glyph_x(&self, i: usize, pad: f32) -> f32 {
        self.cell_x(i) + pad
    }
}

/// The size of the label font for each point of its size: how wide a character is and how tall a row.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub char_width: f32,
    pub row_height: f32,
}

/// The font size of every label, given the targets on screen (`[left, top, right, bottom]`, in reading order) and the longest
/// label. It starts from `label_size` and shrinks it where the words are too close for it, but not below `min_font_size` (or
/// `font_size` if that is smaller). The result is multiplied by `scale_factor`, then floored at `min_font_size`, which is the
/// smallest size ever shown. Before the factor, a label should be no taller than its word and no wider than the room to the
/// next word on the line. The few tightest words, like "a" or "I", would make every label tiny, so the fit is the size that
/// suits all but the tightest tenth.
pub fn fitted_size(zoom: f32, config: &HintConfig, rects: &[[f32; 4]], label_len: usize, metrics: Metrics) -> f32 {
    (fit_size(zoom, config, rects, label_len, metrics) * config.scale_factor).max(config.min_font_size)
}

fn fit_size(zoom: f32, config: &HintConfig, rects: &[[f32; 4]], label_len: usize, metrics: Metrics) -> f32 {
    let size = label_size(zoom, config);
    if !config.scale || rects.is_empty() || label_len == 0 {
        return size;
    }
    let min = config.min_font_size.min(config.font_size);
    // A label box is `label_len` cells, each a character plus 2 * PAD, and stays 1 clear of the next one.
    let widths = rects
        .windows(2)
        .filter(|w| same_line(w[0], w[1]) && w[1][0] > w[0][0])
        .map(|w| ((w[1][0] - w[0][0] - 1.0) / label_len as f32 - 2.0 * PAD) / metrics.char_width);
    let heights = rects.iter().map(|r| (r[3] - r[1]) / metrics.row_height);
    [low_tenth(widths.collect()), low_tenth(heights.collect())].into_iter().flatten().fold(size, f32::min).max(min)
}

fn same_line(a: [f32; 4], b: [f32; 4]) -> bool {
    let centre = (b[1] + b[3]) / 2.0;
    a[1] < centre && centre < a[3]
}

/// The value a tenth of the way up `values`, if there are any.
fn low_tenth(mut values: Vec<f32>) -> Option<f32> {
    values.retain(|v| v.is_finite());
    values.sort_by(f32::total_cmp);
    values.get((values.len().saturating_sub(1)) / 10).copied()
}

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

/// The numbers from 1 as labels. A short one can start a longer one, so a number is taken once
/// nothing longer could follow it, or with Enter.
pub fn numbers(n: usize) -> Vec<String> {
    (1..=n).map(|i| i.to_string()).collect()
}

/// What is left of a label to type, as the key to press next and the keys after it. They are
/// drawn in two colours.
pub fn next_and_later(rest: &str) -> (&str, &str) {
    rest.split_at(rest.chars().next().map_or(0, char::len_utf8))
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
    /// A cell of the grid that places an item.
    Cell,
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
    /// A cell of a grid over the page, to place this item at its centre.
    Grid(PlaceItem),
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

    /// Gives the targets their labels in order: numbers for links, letters of `alphabet` otherwise.
    pub fn assign(&mut self, mut targets: Vec<Target>, alphabet: &[char]) {
        let names = if self.purpose == Purpose::Link { numbers(targets.len()) } else { labels(targets.len(), alphabet) };
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

    /// The target whose whole label is typed, once no longer label could be meant.
    pub fn settled(&self) -> Option<&Target> {
        let longer = self.targets.iter().any(|t| t.label != self.typed && t.label.starts_with(&self.typed));
        if longer { None } else { self.chosen() }
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
    let blank = |g: &pdfcraft_render::TextGlyph| g.text.trim().is_empty();
    while i < text.glyphs.len() {
        // `word_at` on a blank glyph spans the words on both sides of it, so start on a letter.
        if blank(&text.glyphs[i]) {
            i += 1;
            continue;
        }
        let Some((first, last)) = text.word_at(i) else { break };
        i = last.max(i) + 1;
        // Separators such as "·" or "|" are not words.
        if !text.glyphs[first..=last].iter().any(|g| g.text.chars().any(char::is_alphanumeric)) {
            continue;
        }
        let rect = text.glyphs[first..=last].iter().map(|g| g.rect).reduce(union).unwrap_or(text.glyphs[first].rect);
        if intersects(rect, area) {
            out.push(Target { label: String::new(), page, rect, kind: TargetKind::Word(first, last) });
        }
    }
    out
}

/// The cells of side `size` that cut `area` (`[x0, y0, x1, y1]` on `page`) into rows, as targets without labels yet. Cells at
/// the right and bottom edges are clipped to the area.
pub fn grid_targets(page: usize, area: [f32; 4], size: f32) -> Vec<Target> {
    let mut out = Vec::new();
    if size <= 0.0 || area[2] <= area[0] || area[3] <= area[1] {
        return out;
    }
    let (cols, rows) = (((area[2] - area[0]) / size).ceil() as usize, ((area[3] - area[1]) / size).ceil() as usize);
    for row in 0..rows {
        for col in 0..cols {
            let (x, y) = (area[0] + col as f32 * size, area[1] + row as f32 * size);
            let rect = [x, y, (x + size).min(area[2]), (y + size).min(area[3])];
            out.push(Target { label: String::new(), page, rect, kind: TargetKind::Cell });
        }
    }
    out
}

/// The centre of a rectangle.
pub fn centre(rect: [f32; 4]) -> [f32; 2] {
    [(rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0]
}

/// `at` moved by `step` points in each direction of `dir` (-1, 0 or 1), kept inside a page of `size`.
pub fn moved(at: [f32; 2], dir: [f32; 2], step: f32, size: [f32; 2]) -> [f32; 2] {
    [(at[0] + dir[0] * step).clamp(0.0, size[0]), (at[1] + dir[1] * step).clamp(0.0, size[1])]
}

/// A step `dir` on screen (x right, y down) as a direction in the points of an unrotated page of `size`
/// shown turned by `rotation`.
pub fn page_direction(rotation: u16, size: [f32; 2], dir: [f32; 2]) -> [f32; 2] {
    let (to, from) = (unrotate_point(rotation, size, dir), unrotate_point(rotation, size, [0.0, 0.0]));
    [to[0] - from[0], to[1] - from[1]]
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

    #[test]
    fn place_keys_move_the_preview_the_way_they_say_on_screen_at_every_rotation() {
        use crate::view::rotate_point;
        let size = [600.0, 800.0];
        let start = [300.0, 400.0];
        for rotation in [0, 90, 180, 270] {
            let before = rotate_point(rotation, size, start);
            for (dir, name) in [([-1.0, 0.0], "left"), ([1.0, 0.0], "right"), ([0.0, -1.0], "up"), ([0.0, 1.0], "down")] {
                let at = moved(start, page_direction(rotation, size, dir), 2.0, size);
                let after = rotate_point(rotation, size, at);
                let shift = [after[0] - before[0], after[1] - before[1]];
                assert_eq!(shift, [dir[0] * 2.0, dir[1] * 2.0], "{name} at {rotation}");
            }
        }
    }

    fn hint_config(scale: bool) -> HintConfig {
        HintConfig { alphabet: "ab".to_owned(), font_size: 10.0, scale, min_font_size: 6.0, scale_factor: 1.0, grid_size: 50.0, place_step: 4.0 }
    }

    const METRICS: Metrics = Metrics { char_width: 0.5, row_height: 1.0 };

    /// `n` words on a line from `x`, each `step` apart and `height` tall.
    fn row(n: usize, x: f32, y: f32, step: f32, height: f32) -> Vec<[f32; 4]> {
        (0..n).map(|i| x + i as f32 * step).map(|l| [l, y, l + step - 2.0, y + height]).collect()
    }

    #[test]
    fn the_grid_covers_the_area_with_clipped_edge_cells() {
        let cells = grid_targets(3, [10.0, 20.0, 125.0, 90.0], 50.0);
        assert_eq!(cells.len(), 3 * 2);
        assert!(cells.iter().all(|c| c.page == 3 && c.kind == TargetKind::Cell));
        assert_eq!(cells[0].rect, [10.0, 20.0, 60.0, 70.0]);
        assert_eq!(cells[2].rect, [110.0, 20.0, 125.0, 70.0]);
        assert_eq!(cells[5].rect, [110.0, 70.0, 125.0, 90.0]);
        let covered: f32 = cells.iter().map(|c| (c.rect[2] - c.rect[0]) * (c.rect[3] - c.rect[1])).sum();
        assert_eq!(covered, 115.0 * 70.0);
        assert!(grid_targets(0, [0.0, 0.0, 0.0, 10.0], 50.0).is_empty());
        assert!(grid_targets(0, [0.0, 0.0, 10.0, 10.0], 0.0).is_empty());
        assert_eq!(grid_targets(0, [0.0, 0.0, 100.0, 100.0], 50.0).len(), 4);
    }

    #[test]
    fn a_moved_position_stays_on_the_page() {
        let size = [100.0, 50.0];
        assert_eq!(moved([10.0, 10.0], [1.0, 0.0], 2.0, size), [12.0, 10.0]);
        assert_eq!(moved([10.0, 10.0], [0.0, -1.0], 2.0, size), [10.0, 8.0]);
        assert_eq!(moved([1.0, 49.0], [-1.0, 1.0], 2.0, size), [0.0, 50.0]);
        assert_eq!(moved([99.0, 0.5], [1.0, -1.0], 2.0, size), [100.0, 0.0]);
        assert_eq!(centre([0.0, 0.0, 10.0, 20.0]), [5.0, 10.0]);
    }

    #[test]
    fn a_grid_area_smaller_than_a_cell_is_one_clipped_cell() {
        let cells = grid_targets(1, [5.0, 5.0, 15.0, 8.0], 50.0);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].rect, [5.0, 5.0, 15.0, 8.0]);
        assert_eq!(cells[0].page, 1);
    }

    #[test]
    fn an_empty_inverted_or_negative_grid_has_no_cells() {
        assert!(grid_targets(0, [10.0, 10.0, 10.0, 10.0], 5.0).is_empty());
        assert!(grid_targets(0, [10.0, 0.0, 5.0, 10.0], 5.0).is_empty());
        assert!(grid_targets(0, [0.0, 10.0, 10.0, 5.0], 5.0).is_empty());
        assert!(grid_targets(0, [0.0, 0.0, 10.0, 10.0], -5.0).is_empty());
        assert!(grid_targets(0, [-10.0, -10.0, -20.0, -20.0], 5.0).is_empty());
        // A negative origin is a fine area when it has extent.
        let cells = grid_targets(0, [-10.0, -10.0, 0.0, 0.0], 5.0);
        assert_eq!(cells.len(), 4);
        assert_eq!(cells[0].rect, [-10.0, -10.0, -5.0, -5.0]);
        assert_eq!(cells[3].rect, [-5.0, -5.0, 0.0, 0.0]);
    }

    #[test]
    fn grid_cells_run_along_rows_without_overlap_and_stay_in_the_area() {
        let area = [3.0, 4.0, 98.0, 77.0];
        let cells = grid_targets(2, area, 20.0);
        assert_eq!(cells.len(), 5 * 4);
        for c in &cells {
            assert!(c.rect[0] >= area[0] && c.rect[1] >= area[1] && c.rect[2] <= area[2] && c.rect[3] <= area[3]);
            assert!(c.rect[2] > c.rect[0] && c.rect[3] > c.rect[1]);
            assert_eq!(c.page, 2);
        }
        // Row-major: x grows within a row, y only between rows.
        assert!(cells[0].rect[0] < cells[1].rect[0] && cells[0].rect[1] == cells[1].rect[1]);
        assert!(cells[5].rect[1] > cells[0].rect[1] && cells[5].rect[0] == cells[0].rect[0]);
        let covered: f32 = cells.iter().map(|c| (c.rect[2] - c.rect[0]) * (c.rect[3] - c.rect[1])).sum();
        assert!((covered - 95.0 * 73.0).abs() < 0.01);
    }

    #[test]
    fn grid_cells_get_distinct_labels_when_assigned() {
        let cells = grid_targets(0, [0.0, 0.0, 300.0, 300.0], 20.0);
        let n = cells.len();
        let mut state = HintState::new(Purpose::Grid(PlaceItem::Note));
        state.assign(cells, &['a', 'b', 'c']);
        let mut names: Vec<_> = state.targets.iter().map(|t| t.label.clone()).collect();
        assert!(names.iter().all(|l| !l.is_empty()));
        names.sort();
        names.dedup();
        assert_eq!(names.len(), n);
    }

    #[test]
    fn moving_clamps_at_each_edge_and_goes_the_way_asked() {
        let size = [100.0, 50.0];
        assert_eq!(moved([1.0, 20.0], [-1.0, 0.0], 5.0, size), [0.0, 20.0]);
        assert_eq!(moved([98.0, 20.0], [1.0, 0.0], 5.0, size), [100.0, 20.0]);
        assert_eq!(moved([20.0, 1.0], [0.0, -1.0], 5.0, size), [20.0, 0.0]);
        assert_eq!(moved([20.0, 48.0], [0.0, 1.0], 5.0, size), [20.0, 50.0]);
        assert_eq!(moved([0.0, 0.0], [-1.0, -1.0], 5.0, size), [0.0, 0.0]);
        assert_eq!(moved([100.0, 50.0], [1.0, 1.0], 5.0, size), [100.0, 50.0]);
        assert_eq!(moved([20.0, 20.0], [0.0, 0.0], 5.0, size), [20.0, 20.0]);
        assert_eq!(moved([20.0, 20.0], [1.0, 1.0], 5.0, size), [25.0, 25.0]);
        assert_eq!(moved([20.0, 20.0], [-1.0, -1.0], 5.0, size), [15.0, 15.0]);
    }

    #[test]
    fn the_centre_of_a_clipped_cell_is_the_middle_of_what_is_left() {
        assert_eq!(centre([110.0, 70.0, 125.0, 90.0]), [117.5, 80.0]);
        assert_eq!(centre([-10.0, -10.0, 0.0, 0.0]), [-5.0, -5.0]);
        let cells = grid_targets(0, [0.0, 0.0, 30.0, 10.0], 20.0);
        assert_eq!(centre(cells[0].rect), [10.0, 5.0]);
        assert_eq!(centre(cells[1].rect), [25.0, 5.0]);
    }

    #[test]
    fn labels_shrink_to_fit_between_words() {
        // 2-letter labels are 1 point wide per point of size, plus 4 of padding and 1 of gap (a 12 step leaves 11 for 2 cells).
        let rects = row(20, 0.0, 0.0, 12.0, 100.0);
        assert_eq!(fitted_size(1.0, &hint_config(true), &rects, 2, METRICS), 7.0);
        // With room to spare the zoom decides.
        let rects = row(20, 0.0, 0.0, 100.0, 100.0);
        assert_eq!(fitted_size(0.8, &hint_config(true), &rects, 2, METRICS), 8.0);
    }

    #[test]
    fn the_scale_factor_multiplies_the_final_size() {
        let tight = row(20, 0.0, 0.0, 12.0, 100.0);
        for scale in [true, false] {
            for (factor, rects) in [(1.0, &tight[..]), (2.0, &tight[..]), (0.5, &tight[..]), (2.0, &[][..])] {
                let base = fitted_size(1.0, &hint_config(scale), rects, 2, METRICS);
                let config = HintConfig { scale_factor: factor, ..hint_config(scale) };
                assert_eq!(fitted_size(1.0, &config, rects, 2, METRICS), (base * factor).max(6.0));
            }
        }
        let config = HintConfig { scale_factor: 2.0, ..hint_config(true) };
        assert_eq!(fitted_size(1.0, &config, &tight, 2, METRICS), 14.0);
        let config = HintConfig { scale_factor: 0.5, ..hint_config(false) };
        assert_eq!(fitted_size(1.0, &config, &tight, 2, METRICS), 6.0);
    }

    #[test]
    fn labels_are_no_taller_than_their_words() {
        let rects: Vec<_> = row(5, 0.0, 0.0, 100.0, 7.5).into_iter().chain(row(5, 0.0, 10.0, 100.0, 7.5)).collect();
        assert_eq!(fitted_size(1.0, &hint_config(true), &rects, 2, METRICS), 7.5);
    }

    #[test]
    fn fitting_stops_at_the_minimum_and_is_off_without_scaling() {
        let rects = row(20, 0.0, 0.0, 4.0, 2.0);
        assert_eq!(fitted_size(1.0, &hint_config(true), &rects, 2, METRICS), 6.0);
        assert_eq!(fitted_size(1.0, &hint_config(false), &rects, 2, METRICS), 10.0);
        assert_eq!(fitted_size(1.0, &hint_config(true), &[], 2, METRICS), 10.0);
    }

    #[test]
    fn a_few_tight_words_do_not_shrink_every_label() {
        let mut rects = row(30, 0.0, 0.0, 100.0, 100.0);
        // One word right up against the next.
        rects[3][0] = rects[4][0] - 1.0;
        assert_eq!(fitted_size(1.0, &hint_config(true), &rects, 2, METRICS), 10.0);
    }

    #[test]
    fn the_gap_to_the_next_line_does_not_count_as_room() {
        // The last word of a line is followed by the first of the next, far below it.
        let rects: Vec<_> = row(3, 0.0, 0.0, 12.0, 100.0).into_iter().chain(row(3, 0.0, 200.0, 12.0, 100.0)).collect();
        assert_eq!(fitted_size(1.0, &hint_config(true), &rects, 2, METRICS), 7.0);
    }

    #[test]
    fn every_key_is_centred_in_a_cell_of_equal_width() {
        let l = CellLayout::new(3, 6.0, 1.0);
        assert_eq!(l, CellLayout { cell_width: 8.0, box_width: 24.0 });
        assert_eq!([0, 1, 2].map(|i| l.cell_x(i)), [0.0, 8.0, 16.0]);
        assert_eq!([0, 1, 2].map(|i| l.glyph_x(i, 1.0)), [1.0, 9.0, 17.0]);
        // Equal space left and right of each glyph.
        assert_eq!(l.cell_x(1) + l.cell_width - (l.glyph_x(1, 1.0) + 6.0), 1.0);
        assert_eq!(CellLayout::new(0, 6.0, 1.0).box_width, 0.0);
    }

    #[test]
    fn cell_layout_edge_cases() {
        // One key: the box is one cell and the glyph sits one pad in.
        let one = CellLayout::new(1, 6.0, 1.0);
        assert_eq!(one, CellLayout { cell_width: 8.0, box_width: 8.0 });
        assert_eq!((one.cell_x(0), one.glyph_x(0, 1.0)), (0.0, 1.0));
        // No padding: cells are exactly the glyph width and glyphs sit at the cell edge.
        let bare = CellLayout::new(3, 6.0, 0.0);
        assert_eq!(bare, CellLayout { cell_width: 6.0, box_width: 18.0 });
        assert_eq!([0, 1, 2].map(|i| bare.glyph_x(i, 0.0)), [0.0, 6.0, 12.0]);
        // No keys: an empty box, but the cell width is still defined.
        assert_eq!(CellLayout::new(0, 6.0, 1.0), CellLayout { cell_width: 8.0, box_width: 0.0 });
        // The last cell ends at the box edge, for any number of keys.
        for keys in 1..6 {
            let l = CellLayout::new(keys, 5.5, PAD);
            assert_eq!(l.cell_x(keys - 1) + l.cell_width, l.box_width);
        }
    }

    #[test]
    fn fitted_size_label_length_edge_cases() {
        // One key per label has plenty of room in a 12 step: the zoom decides.
        let tight = row(20, 0.0, 0.0, 12.0, 100.0);
        assert_eq!(fitted_size(1.0, &hint_config(true), &tight, 1, METRICS), 10.0);
        // Three keys: (18 / 3 - 2 * PAD) / 0.5 = 8 in a 19 step.
        let rects = row(20, 0.0, 0.0, 19.0, 100.0);
        assert_eq!(fitted_size(1.0, &hint_config(true), &rects, 3, METRICS), 8.0);
        // Longer labels never get a larger size than shorter ones.
        assert!(fitted_size(1.0, &hint_config(true), &tight, 3, METRICS) <= fitted_size(1.0, &hint_config(true), &tight, 2, METRICS));
        // Too tight for the minimum: clamped to it, not below.
        assert_eq!(fitted_size(1.0, &hint_config(true), &tight, 3, METRICS), 6.0);
        // No keys: no fitting, and no division by zero.
        assert_eq!(fitted_size(1.0, &hint_config(true), &tight, 0, METRICS), 10.0);
    }

    #[test]
    fn a_minimum_above_the_font_size_is_the_floor_with_or_without_fitting() {
        let roomy = row(20, 0.0, 0.0, 100.0, 100.0);
        for scale in [true, false] {
            for rects in [&roomy[..], &[][..]] {
                let config = HintConfig { min_font_size: 14.0, ..hint_config(scale) };
                assert_eq!(fitted_size(1.0, &config, rects, 2, METRICS), 14.0);
                let config = HintConfig { min_font_size: 14.0, scale_factor: 2.0, ..hint_config(scale) };
                assert_eq!(fitted_size(1.0, &config, rects, 2, METRICS), 20.0);
            }
        }
    }

    #[test]
    fn the_minimum_is_a_floor_after_the_scale_factor() {
        // Fitting clamps to the minimum of 6; the factor then enlarges, but never goes below 6.
        let rects = row(20, 0.0, 0.0, 4.0, 2.0);
        for (factor, want) in [(0.5, 6.0), (1.0, 6.0), (2.0, 12.0)] {
            let config = HintConfig { scale_factor: factor, ..hint_config(true) };
            assert_eq!(fitted_size(1.0, &config, &rects, 2, METRICS), want);
        }
        // Zoom below one, scaling on, with the factor on top.
        let config = HintConfig { scale_factor: 2.0, ..hint_config(true) };
        assert_eq!(fitted_size(0.8, &config, &[], 2, METRICS), 16.0);
        let config = HintConfig { scale_factor: 0.5, ..hint_config(true) };
        assert_eq!(fitted_size(0.8, &config, &[], 2, METRICS), 6.0);
    }

    #[test]
    fn labels_scale_with_the_zoom_between_the_minimum_and_the_font_size() {
        let config = hint_config(true);
        assert_eq!(label_size(3.0, &config), 10.0);
        assert_eq!(label_size(1.0, &config), 10.0);
        assert_eq!(label_size(0.8, &config), 8.0);
        assert_eq!(label_size(0.1, &config), 6.0);
        assert_eq!(label_size(0.1, &hint_config(false)), 10.0);
        assert_eq!(label_size(3.0, &hint_config(false)), 10.0);
        // A minimum above the font size gives way to it.
        let small = HintConfig { font_size: 4.0, ..hint_config(true) };
        assert_eq!(label_size(0.1, &small), 4.0);
        assert_eq!(label_size(1.0, &small), 4.0);
    }

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
    fn links_are_numbered_from_one_and_wait_while_a_longer_number_could_follow() {
        let target = |i| Target { label: String::new(), page: 0, rect: [0.0; 4], kind: TargetKind::Link(i) };
        let mut state = HintState::new(Purpose::Link);
        state.assign((0..12).map(target).collect(), &chars("ab"));
        assert_eq!(state.targets[0].label, "1");
        assert_eq!(state.targets[11].label, "12");
        state.typed = "1".to_owned();
        assert!(state.settled().is_none());
        assert_eq!(state.chosen().unwrap().kind, TargetKind::Link(0));
        state.typed = "12".to_owned();
        assert_eq!(state.settled().unwrap().kind, TargetKind::Link(11));
        state.typed = "3".to_owned();
        assert_eq!(state.settled().unwrap().kind, TargetKind::Link(2));
        state.typed = "0".to_owned();
        assert!(!state.matches());
    }

    #[test]
    fn the_next_key_of_a_label_is_split_from_the_later_ones() {
        assert_eq!(next_and_later("whabc"), ("w", "habc"));
        assert_eq!(next_and_later("h"), ("h", ""));
        assert_eq!(next_and_later(""), ("", ""));
        assert_eq!(next_and_later("éa"), ("é", "a"));
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

    fn line_page(s: &str) -> PageText {
        let glyphs: Vec<_> = s.chars().enumerate().map(|(i, c)| glyph(c, i as f32 * 5.0, 0.0)).collect();
        let n = glyphs.len();
        PageText { glyphs, line_of: vec![0; n], space_before: vec![false; n], truncated: false }
    }

    const ALL: [f32; 4] = [-100.0, -100.0, 1000.0, 1000.0];

    #[test]
    fn a_blank_glyph_does_not_join_the_words_around_it() {
        let targets = word_targets(&line_page("AB CD"), 0, ALL);
        let ranges: Vec<_> = targets.iter().map(|t| t.kind).collect();
        assert_eq!(ranges, vec![TargetKind::Word(0, 1), TargetKind::Word(3, 4)]);
        assert_eq!(targets[1].rect, [15.0, 0.0, 25.0, 10.0]);
    }

    #[test]
    fn separators_and_blanks_are_not_targets() {
        let targets = word_targets(&line_page("10 \u{b7} AB"), 0, ALL);
        let ranges: Vec<_> = targets.iter().map(|t| t.kind).collect();
        assert_eq!(ranges, vec![TargetKind::Word(0, 1), TargetKind::Word(5, 6)]);
        let targets = word_targets(&line_page("(a) | - b"), 0, ALL);
        assert_eq!(targets.len(), 2);
    }
}
