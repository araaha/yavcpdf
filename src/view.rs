//! Where pages sit in the continuous column, and how a view looks at them.
use egui::{Pos2, Rect, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};

/// A rotation in degrees, clockwise, as a multiple of 90 in `0..360`: anything else is taken down
/// to the multiple of 90 below it, after wrapping.
pub fn normalize_rotation(degrees: i64) -> u16 {
    (degrees.rem_euclid(360) / 90 * 90) as u16
}

/// Where `p`, a point of an unrotated page of `size`, lands on the page turned clockwise by
/// `rotation`, measured from the turned page's top-left corner.
pub fn rotate_point(rotation: u16, size: [f32; 2], p: [f32; 2]) -> [f32; 2] {
    let [w, h] = size;
    match rotation {
        90 => [h - p[1], p[0]],
        180 => [w - p[0], h - p[1]],
        270 => [p[1], w - p[0]],
        _ => p,
    }
}

/// The inverse of `rotate_point`.
pub fn unrotate_point(rotation: u16, size: [f32; 2], d: [f32; 2]) -> [f32; 2] {
    let [w, h] = size;
    match rotation {
        90 => [d[1], h - d[0]],
        180 => [w - d[0], h - d[1]],
        270 => [w - d[1], d[0]],
        _ => d,
    }
}

/// The texture corners that the screen corners of a rotated tile show, in the order top-left,
/// top-right, bottom-right, bottom-left. The texture is of the unrotated page.
pub fn tile_uv(rotation: u16) -> [Pos2; 4] {
    let (a, b, c, d) = (pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0));
    match rotation {
        90 => [d, a, b, c],
        180 => [c, d, a, b],
        270 => [b, c, d, a],
        _ => [a, b, c, d],
    }
}

/// The pixel area of an unrotated page of `pixels` that shows through `clip` on screen, as
/// `[x0, y0, x1, y1]`, when the turned page's top-left is at `origin` and a pixel is `unit` points.
pub fn visible_pixels(rotation: u16, pixels: [f32; 2], origin: Pos2, unit: f32, clip: Rect) -> [f32; 4] {
    let at = |p: Pos2| unrotate_point(rotation, pixels, [(p.x - origin.x) / unit, (p.y - origin.y) / unit]);
    let (a, b) = (at(clip.min), at(clip.max));
    [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])]
}

/// The pages stacked top to bottom, in points, as they are shown: turned by `rotation`.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    /// The size of each page as shown: width and height swap at 90 and 270 degrees.
    pub sizes: Vec<[f32; 2]>,
    /// The size of each page itself, whatever the rotation: the space of rasters and annotations.
    pub page_sizes: Vec<[f32; 2]>,
    /// The y of the top of each page.
    pub tops: Vec<f32>,
    /// The x of the left edge of each page, from the column's centre: `-width / 2` when pages are
    /// shown one at a time.
    pub lefts: Vec<f32>,
    /// The row each page is in.
    pub row_of: Vec<usize>,
    /// The rows, top to bottom: a page of its own, or with two pages side by side a pair.
    pub rows: Vec<Row>,
    pub total: f32,
    /// Twice the largest distance of a page edge from the column's centre.
    pub max_width: f32,
    /// Degrees clockwise, a multiple of 90 in `0..360`.
    pub rotation: u16,
    /// Whether pages sit side by side in pairs.
    pub two_page: bool,
    /// With `two_page`, whether the first page sits alone on the right.
    pub cover_alone: bool,
    gap: f32,
}

/// A row of one or two pages.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Row {
    /// The first and last page of the row.
    pub first: usize,
    pub last: usize,
    pub top: f32,
    /// The height of the taller page.
    pub height: f32,
}

impl Layout {
    pub fn new(sizes: Vec<[f32; 2]>, gap: f32) -> Self {
        let mut layout = Self { page_sizes: sizes, gap, ..Self::default() };
        layout.arrange();
        layout
    }

    /// Turns every page to `degrees` clockwise, as given to `normalize_rotation`.
    pub fn set_rotation(&mut self, degrees: i64) {
        self.rotation = normalize_rotation(degrees);
        self.arrange();
    }

    /// Shows pages in pairs, or one at a time, with the first page alone if `cover_alone`.
    pub fn set_two_page(&mut self, on: bool, cover_alone: bool) {
        self.two_page = on;
        self.cover_alone = cover_alone;
        self.arrange();
    }

    fn arrange(&mut self) {
        let turned = !self.rotation.is_multiple_of(180);
        self.sizes = self.page_sizes.iter().map(|&[w, h]| if turned { [h, w] } else { [w, h] }).collect();
        let n = self.sizes.len();
        let mut spans = Vec::new();
        let mut first = 0;
        if self.two_page && self.cover_alone && n > 0 {
            spans.push((0, 0));
            first = 1;
        }
        while first < n {
            let last = if self.two_page { (first + 1).min(n - 1) } else { first };
            spans.push((first, last));
            first = last + 1;
        }
        self.tops = vec![0.0; n];
        self.lefts = vec![0.0; n];
        self.row_of = vec![0; n];
        self.rows = Vec::with_capacity(spans.len());
        let mut y = 0.0;
        let mut reach: f32 = 0.0;
        for (index, &(first, last)) in spans.iter().enumerate() {
            let height = self.sizes[first..=last].iter().map(|s| s[1]).fold(0.0, f32::max);
            for page in first..=last {
                let [w, h] = self.sizes[page];
                self.row_of[page] = index;
                self.tops[page] = y + (height - h) / 2.0;
                self.lefts[page] = self.left_for(page, first, last, w);
                reach = reach.max(self.lefts[page].abs()).max((self.lefts[page] + w).abs());
            }
            self.rows.push(Row { first, last, top: y, height });
            y += height + self.gap;
        }
        self.total = (y - self.gap).max(0.0);
        self.max_width = reach * 2.0;
    }

    /// The left edge, from the column's centre, of `page` of width `w` in the row `first..=last`.
    fn left_for(&self, page: usize, first: usize, last: usize, w: f32) -> f32 {
        if !self.two_page {
            -w / 2.0
        } else if first != last {
            if page == first { -self.gap / 2.0 - w } else { self.gap / 2.0 }
        } else if self.cover_alone && page == 0 {
            self.gap / 2.0
        } else {
            -self.gap / 2.0 - w
        }
    }

    /// The x of the centre of `page` from the column's centre, as it would be with the pages turned
    /// to `rotation`.
    fn centre_at(&self, page: usize, rotation: u16) -> f32 {
        let size = self.page_sizes[page];
        let w = if rotation.is_multiple_of(180) { size[0] } else { size[1] };
        let row = self.rows[self.row_of[page]];
        self.left_for(page, row.first, row.last, w) + w / 2.0
    }

    /// The x of the centre of `page` from the column's centre, as arranged now.
    pub fn centre_of(&self, page: usize) -> f32 {
        self.lefts[page] + self.sizes[page][0] / 2.0
    }

    /// The page of the row at `y` that the horizontal position `x` is over, or nearest to: the first
    /// where it is as near to both. Where every page of the row is fully in a view `span` points
    /// wide, centred at `x`, it is the first, so that panning doesn't change it.
    pub fn page_near(&self, y: f32, x: f32, span: f32) -> usize {
        let Some(row) = self.rows.get(self.row_at(y)) else { return 0 };
        let half = span / 2.0;
        if (row.first..=row.last).all(|p| self.lefts[p] >= x - half && self.lefts[p] + self.sizes[p][0] <= x + half) {
            return row.first;
        }
        let away = |p: usize| (self.lefts[p] - x).max(x - (self.lefts[p] + self.sizes[p][0])).max(0.0);
        (row.first..=row.last).min_by(|&a, &b| away(a).total_cmp(&away(b))).unwrap_or(row.first)
    }

    /// The anchor that shows the row of `page` from its top, with the view centred at `x`.
    pub fn row_anchor(&self, page: usize, x: f32) -> Anchor {
        let first = self.row_first(page);
        let y = self.row_of.get(first).map_or(0.0, |&row| self.rows[row].top - self.tops[first]);
        Anchor { page: first, y, x }
    }

    /// The horizontal position of a view `span` points wide, centred at `x`, that shows `page`: `x`
    /// if the page is shown (or, being wider than the view, is under its centre), else the page's
    /// centre. Pages shown one at a time share the column's centre, so `x` is kept.
    pub fn x_for(&self, page: usize, x: f32, span: f32) -> f32 {
        if !self.two_page || page >= self.len() {
            return x;
        }
        let (left, w) = (self.lefts[page], self.sizes[page][0]);
        let (right, half) = (left + w, span / 2.0);
        let shown = if w > span { (left..=right).contains(&x) } else { left >= x - half && right <= x + half };
        if shown {
            return x;
        }
        let overflow = (self.max_width - span).max(0.0) / 2.0;
        (left + w / 2.0).clamp(-overflow, overflow)
    }

    /// The row starting at or above `y`, the last such, allowing for rounding in `y`.
    pub fn row_at(&self, y: f32) -> usize {
        self.rows.partition_point(|row| row.top <= y + ROW_EPS).saturating_sub(1)
    }

    /// The last page of the row at `y`.
    pub fn last_page_at(&self, y: f32) -> usize {
        self.rows.get(self.row_at(y)).map_or(0, |row| row.last)
    }

    /// The first page of the row that is `rows` rows after (or before, if negative) the row of `page`.
    pub fn page_by_rows(&self, page: usize, rows: isize) -> usize {
        let Some(&row) = self.row_of.get(page) else { return 0 };
        let target = row.saturating_add_signed(rows).min(self.rows.len() - 1);
        self.rows[target].first
    }

    /// The size of the row of `page` that a fit looks at: the height of the row, and the width it
    /// takes about the column's centre, both pages and the gap when there are two.
    pub fn row_size(&self, page: usize) -> [f32; 2] {
        let Some(&row) = self.row_of.get(page) else { return self.sizes[page] };
        let row = self.rows[row];
        let reach = (row.first..=row.last).map(|p| self.lefts[p].abs().max((self.lefts[p] + self.sizes[p][0]).abs())).fold(0.0, f32::max);
        [reach * 2.0, row.height]
    }

    /// The first page of the row containing `page`.
    pub fn row_first(&self, page: usize) -> usize {
        self.row_of.get(page).map_or(page, |&row| self.rows[row].first)
    }

    /// A page rectangle `[x0, y0, x1, y1]` in the space of the page as shown.
    pub fn shown_rect(&self, page: usize, r: [f32; 4]) -> [f32; 4] {
        let size = self.page_sizes[page];
        let (a, b) = (rotate_point(self.rotation, size, [r[0], r[1]]), rotate_point(self.rotation, size, [r[2], r[3]]));
        [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])]
    }

    /// A point `p` of an unrotated page, in the space of the page as shown.
    #[cfg(test)]
    pub fn shown_point(&self, page: usize, p: [f32; 2]) -> [f32; 2] {
        rotate_point(self.rotation, self.page_sizes[page], p)
    }

    /// The y as shown of a destination on `page` given by the page point `x`, `y` where known. A
    /// destination with no `y` names no point and is at the top, 0, however the pages are turned.
    /// With a `y`, upright and upside down only `y` matters, and turned a quarter only `x` does,
    /// the top being 0 where `x` is missing.
    pub fn shown_dest_y(&self, page: usize, x: Option<f32>, y: Option<f32>) -> f32 {
        let [w, h] = self.page_sizes[page];
        let Some(y) = y else { return 0.0 };
        match self.rotation {
            0 => y,
            180 => h - y,
            90 => x.unwrap_or(0.0),
            _ => x.map_or(0.0, |x| w - x),
        }
    }

    /// `shown_dest_y` for a destination that may be a rectangle `[x0, y0, x1, y1]` of the page, whose
    /// corner topmost as shown is taken.
    pub fn shown_spot_y(&self, page: usize, x: Option<f32>, y: Option<f32>, rect: Option<[f32; 4]>) -> f32 {
        let Some([x0, y0, x1, y1]) = rect else { return self.shown_dest_y(page, x, y) };
        let (cx, cy) = match self.rotation {
            0 | 90 => (x0, y0),
            180 => (x0, y1),
            _ => (x1, y0),
        };
        self.shown_dest_y(page, Some(cx), Some(cy))
    }

    /// An anchor made while the pages were turned to `from`, as it is for `to`. The anchor is taken
    /// as the page point at the top of the view, at its centre across: that point stays put. An
    /// anchor at or above the top of its page (`y <= 0`) stays at the top, y 0, and keeps its x
    /// only if it is turned by a half turn or none: across a quarter turn the page's width becomes
    /// its height, so x is 0 (the column's centre). Such an anchor is not lossless; one below the top is.
    pub fn turn_anchor(&self, a: Anchor, from: u16, to: u16) -> Anchor {
        let Some(&size) = self.page_sizes.get(a.page) else { return a };
        // The anchor's x is from the column's centre, the spine in two-page mode: work from the
        // page's own centre, which is the column's centre when pages are shown one at a time.
        let (centre_from, centre_to) = (self.centre_at(a.page, from), self.centre_at(a.page, to));
        if a.y <= 0.0 {
            let quarter = from.is_multiple_of(180) != to.is_multiple_of(180);
            return Anchor { page: a.page, y: 0.0, x: if quarter { centre_to } else { a.x - centre_from + centre_to } };
        }
        let width = |rotation: u16| if rotation.is_multiple_of(180) { size[0] } else { size[1] };
        let p = unrotate_point(from, size, [width(from) / 2.0 + a.x - centre_from, a.y]);
        let [dx, dy] = rotate_point(to, size, p);
        Anchor { page: a.page, y: dy, x: dx - width(to) / 2.0 + centre_to }
    }

    /// The first page of the last row starting at or above `y`.
    pub fn page_at(&self, y: f32) -> usize {
        self.rows.get(self.row_at(y)).map_or(0, |row| row.first)
    }

    pub fn len(&self) -> usize {
        self.sizes.len()
    }
}

/// A place in the document that doesn't depend on the zoom.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub page: usize,
    /// Points below the top of the page.
    pub y: f32,
    /// Horizontal offset of the view's centre from the column's centre, in points.
    pub x: f32,
}

/// Maps a page's points to the screen, through the rotation the page is shown at.
#[derive(Clone, Copy, Debug)]
pub struct PageTransform {
    /// The top-left corner on screen of the page as shown, turned.
    pub origin: Pos2,
    /// Screen points per page point.
    pub scale: f32,
    /// Degrees clockwise, a multiple of 90.
    pub rotation: u16,
    /// The page's own size, unrotated.
    pub size: [f32; 2],
}

impl PageTransform {
    pub fn point(&self, p: [f32; 2]) -> Pos2 {
        let d = rotate_point(self.rotation, self.size, p);
        self.origin + vec2(d[0], d[1]) * self.scale
    }

    pub fn rect(&self, r: [f32; 4]) -> Rect {
        Rect::from_two_pos(self.point([r[0], r[1]]), self.point([r[2], r[3]]))
    }

    pub fn to_page(self, p: Pos2) -> [f32; 2] {
        let d = (p - self.origin) / self.scale;
        unrotate_point(self.rotation, self.size, [d.x, d.y])
    }
}

/// What a run of zooming holds still: a point of the document at a place in the view. Kept apart
/// from the view, which `clamp` moves whenever the document stops filling the window: zooming far
/// out and back would otherwise come back to wherever that left the view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomHold {
    /// The point: `x` from the column's centre and `y` from the top of the document, in points.
    point: [f32; 2],
    /// Where it stays, from the middle of the view.
    offset: Vec2,
    /// `x`, `y` and `zoom` as the last zoom left them. A view that has moved since starts anew.
    left: [f32; 3],
}

/// A window onto the column.
#[derive(Clone, Copy, Debug)]
pub struct View {
    /// Horizontal offset of the centre, in points from the column's centre.
    pub x: f32,
    /// The document's y at the top of the view, in points.
    pub y: f32,
    /// Screen points per page point.
    pub zoom: f32,
    /// The view's area on screen.
    pub rect: Rect,
    /// What zooming is holding still, if the view is where the last zoom left it.
    pub zoom_hold: Option<ZoomHold>,
}

impl View {
    pub fn new() -> Self {
        Self { x: 0.0, y: 0.0, zoom: 1.0, rect: Rect::from_min_size(Pos2::ZERO, vec2(1.0, 1.0)), zoom_hold: None }
    }

    /// The view's size in points of the page.
    fn span(&self) -> [f32; 2] {
        [self.rect.width() / self.zoom, self.rect.height() / self.zoom]
    }

    /// Keeps the document in view: a document shorter than the window is centred.
    pub fn clamp(&mut self, layout: &Layout) {
        let [w, h] = self.span();
        let overflow = (layout.max_width - w).max(0.0) / 2.0;
        self.x = self.x.clamp(-overflow, overflow);
        self.y = if layout.total <= h { (layout.total - h) / 2.0 } else { self.y.clamp(0.0, layout.total - h) };
    }

    /// The pages showing, first to last.
    pub fn visible_pages(&self, layout: &Layout) -> std::ops::RangeInclusive<usize> {
        let top = layout.page_at(self.y.max(0.0));
        let bottom = layout.last_page_at((self.y + self.span()[1]).max(0.0));
        top..=bottom
    }

    /// The page at the middle of the view: the one the status bar calls current.
    pub fn current_page(&self, layout: &Layout) -> usize {
        layout.page_near((self.y + self.span()[1] / 2.0).max(0.0), self.x, self.span()[0])
    }

    pub fn transform(&self, layout: &Layout, page: usize) -> PageTransform {
        let left = self.rect.center().x + (layout.lefts[page] - self.x) * self.zoom;
        let top = self.rect.top() + (layout.tops[page] - self.y) * self.zoom;
        PageTransform { origin: pos2(left, top), scale: self.zoom, rotation: layout.rotation, size: layout.page_sizes[page] }
    }

    /// The current page and the part of it on screen, in page points as `[x0, y0, x1, y1]`.
    pub fn current_area(&self, layout: &Layout) -> (usize, [f32; 4]) {
        let page = self.current_page(layout);
        let t = self.transform(layout, page);
        let (a, b) = (t.to_page(self.rect.min), t.to_page(self.rect.max));
        let [w, h] = layout.page_sizes[page];
        let area = [a[0].min(b[0]).max(0.0), a[1].min(b[1]).max(0.0), a[0].max(b[0]).min(w), a[1].max(b[1]).min(h)];
        (page, area)
    }

    /// The page under a screen position, with the position in its points.
    pub fn page_under(&self, layout: &Layout, p: Pos2) -> Option<(usize, [f32; 2])> {
        self.visible_pages(layout).find_map(|page| {
            let at = self.transform(layout, page).to_page(p);
            let [w, h] = layout.page_sizes[page];
            ((0.0..=w).contains(&at[0]) && (0.0..=h).contains(&at[1])).then_some((page, at))
        })
    }

    /// Changes the zoom around the middle of the view.
    pub fn set_zoom(&mut self, zoom: f32, layout: &Layout) {
        self.zoom_around(zoom, None, layout);
    }

    /// Changes the zoom holding still what is at `at`, or what the last zoom held if the view
    /// hasn't moved since, or else the middle of the view.
    pub fn zoom_around(&mut self, zoom: f32, at: Option<Pos2>, layout: &Layout) {
        let asked = at.map_or(Vec2::ZERO, |p| p - self.rect.center());
        let here = [self.x, self.y, self.zoom];
        let held = self.zoom_hold.filter(|h| h.left == here && (at.is_none() || h.offset == asked));
        let (point, offset) = held.map_or_else(|| (self.point_at(asked), asked), |h| (h.point, h.offset));
        self.zoom = zoom;
        self.x = point[0] - offset.x / zoom;
        self.y = point[1] - (self.rect.height() / 2.0 + offset.y) / zoom;
        self.clamp(layout);
        self.zoom_hold = Some(ZoomHold { point, offset, left: [self.x, self.y, self.zoom] });
    }

    /// The document point `offset` from the middle of the view.
    fn point_at(&self, offset: Vec2) -> [f32; 2] {
        [self.x + offset.x / self.zoom, self.y + (self.rect.height() / 2.0 + offset.y) / self.zoom]
    }

    pub fn anchor(&self, layout: &Layout) -> Anchor {
        let page = layout.page_near(self.y.max(0.0), self.x, self.span()[0]);
        Anchor { page, y: self.y - layout.tops[page], x: self.x }
    }

    pub fn go_to(&mut self, layout: &Layout, anchor: Anchor) {
        let Some(top) = layout.tops.get(anchor.page.min(layout.len().saturating_sub(1))) else { return };
        self.y = top + anchor.y;
        self.x = anchor.x;
        self.clamp(layout);
    }

    /// Scrolls so `r` (page points) shows, centring it when it is out of view.
    pub fn reveal(&mut self, layout: &Layout, page: usize, r: [f32; 4]) {
        let r = layout.shown_rect(page, r);
        let [w, h] = self.span();
        let (top, bottom) = (layout.tops[page] + r[1], layout.tops[page] + r[3]);
        if top < self.y || bottom > self.y + h {
            self.y = (top + bottom) / 2.0 - h / 2.0;
        }
        let page_left = layout.lefts[page];
        let (left, right) = (page_left + r[0], page_left + r[2]);
        if left < self.x - w / 2.0 || right > self.x + w / 2.0 {
            self.x = (left + right) / 2.0;
        }
        self.clamp(layout);
    }
}

/// Slack, in points, for a y that is a row's top up to rounding.
const ROW_EPS: f32 = 1e-3;

/// Smallest side of a render tile in device pixels.
pub const TILE: u32 = 1024;
/// Largest side of a render tile: what the render backend accepts.
const MAX_TILE: u32 = 4096;

/// The side of the tiles of a page `longest` device pixels long at `scale`. Every tile costs an
/// interpretation of the whole page, so tiles grow with the zoom and the page.
pub fn tile_side(scale: f32, longest: u32) -> u32 {
    let by_zoom = if scale.is_finite() && scale > 0.0 { (TILE as f32 * (scale / 6.0)).clamp(TILE as f32, MAX_TILE as f32) as u32 } else { TILE };
    by_zoom.max(longest.div_ceil(4).min(MAX_TILE))
}

/// The grid tiles of side `side` of a `width` × `height` pixel page that cross the pixel area
/// `visible` (`[x0, y0, x1, y1]`).
pub fn tiles_crossing(width: u32, height: u32, visible: [f32; 4], side: u32) -> Vec<pdfcraft_render::Tile> {
    let index = |v: f32, limit: u32| ((v.max(0.0) as u32) / side).min(limit.saturating_sub(1) / side);
    let (x0, y0) = (index(visible[0], width), index(visible[1], height));
    let (x1, y1) = (index(visible[2], width), index(visible[3], height));
    let mut out = Vec::new();
    for ty in y0..=y1 {
        for tx in x0..=x1 {
            let (x, y) = (tx * side, ty * side);
            out.push(pdfcraft_render::Tile { x, y, w: side.min(width - x), h: side.min(height - y) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout::new(vec![[600.0, 800.0], [600.0, 800.0], [300.0, 400.0]], 10.0)
    }

    fn view(w: f32, h: f32) -> View {
        View { rect: Rect::from_min_size(Pos2::ZERO, vec2(w, h)), ..View::new() }
    }

    #[test]
    fn current_area_is_the_whole_current_page_when_zoomed_out() {
        let l = layout();
        let mut v = view(1000.0, 1000.0);
        v.zoom = 0.2;
        assert!(v.visible_pages(&l).count() > 1);
        let (page, area) = v.current_area(&l);
        assert_eq!(page, v.current_page(&l));
        let [w, h] = l.page_sizes[page];
        assert_eq!(area, [0.0, 0.0, w, h]);
    }

    #[test]
    fn current_area_is_the_part_on_screen_when_zoomed_in() {
        let l = layout();
        let mut v = view(300.0, 200.0);
        v.zoom = 2.0;
        let (page, area) = v.current_area(&l);
        assert_eq!(page, v.current_page(&l));
        let [w, h] = l.page_sizes[page];
        assert!(area[2] - area[0] < w && area[3] - area[1] < h);
        assert!(area[0] >= 0.0 && area[1] >= 0.0 && area[2] <= w && area[3] <= h);
        assert!((area[3] - area[1] - 100.0).abs() < 1e-3);
        assert!((area[2] - area[0] - 150.0).abs() < 1e-3);
    }

    #[test]
    fn pages_stack_with_gaps() {
        let l = layout();
        assert_eq!(l.tops, [0.0, 810.0, 1620.0]);
        assert_eq!(l.total, 2020.0);
        assert_eq!(l.page_at(0.0), 0);
        assert_eq!(l.page_at(809.0), 0);
        assert_eq!(l.page_at(810.0), 1);
        assert_eq!(l.page_at(99999.0), 2);
    }

    #[test]
    fn the_view_stops_at_both_ends() {
        let l = layout();
        let mut v = view(600.0, 500.0);
        v.y = -50.0;
        v.clamp(&l);
        assert_eq!(v.y, 0.0);
        v.y = 99999.0;
        v.clamp(&l);
        assert_eq!(v.y, l.total - 500.0);
    }

    #[test]
    fn a_short_document_is_centred_and_a_narrow_one_cannot_pan() {
        let l = Layout::new(vec![[100.0, 100.0]], 10.0);
        let mut v = view(500.0, 500.0);
        v.x = 40.0;
        v.clamp(&l);
        assert_eq!(v.x, 0.0);
        assert_eq!(v.y, -200.0);
        assert_eq!(v.transform(&l, 0).origin, pos2(200.0, 200.0));
    }

    #[test]
    fn zoom_keeps_the_middle_of_the_view() {
        let l = layout();
        let mut v = view(600.0, 400.0);
        v.y = 500.0;
        let centre = v.y + 200.0;
        v.set_zoom(2.0, &l);
        assert_eq!(v.y + 100.0, centre);
    }

    /// Zooms out in steps until the document is small, and back by the same steps.
    fn out_and_back(v: &mut View, l: &Layout, at: Option<Pos2>) {
        let start = v.zoom;
        let steps = 40;
        for i in 1..=steps {
            v.zoom_around((start * 1.2_f32.powi(-i)).max(0.02), at, l);
            v.clamp(l);
        }
        for i in (0..steps).rev() {
            v.zoom_around((start * 1.2_f32.powi(-i)).max(0.02), at, l);
            v.clamp(l);
        }
    }

    #[test]
    fn zooming_far_out_and_back_returns_to_the_same_place() {
        let l = Layout::new(vec![[600.0, 800.0]; 15], 6.0);
        for y in [0.0, 50.0, 3000.0, l.total - 900.0, l.total - 400.0] {
            let mut v = view(1000.0, 400.0);
            v.y = y;
            v.clamp(&l);
            let (x, y) = (v.x, v.y);
            out_and_back(&mut v, &l, None);
            assert!((v.y - y).abs() < 0.01 && (v.x - x).abs() < 0.01, "from {y}: at {} {}", v.x, v.y);
        }
    }

    #[test]
    fn a_panned_view_comes_back_sideways_too() {
        let l = Layout::new(vec![[600.0, 800.0]; 3], 6.0);
        let mut v = view(300.0, 400.0);
        v.zoom = 2.0;
        v.x = 120.0;
        v.y = 500.0;
        v.clamp(&l);
        let (x, y) = (v.x, v.y);
        out_and_back(&mut v, &l, None);
        assert!((v.x - x).abs() < 0.01 && (v.y - y).abs() < 0.01, "at {} {}", v.x, v.y);
    }

    #[test]
    fn zooming_at_the_pointer_keeps_what_is_under_it() {
        let l = Layout::new(vec![[600.0, 800.0]; 15], 6.0);
        let mut v = view(1000.0, 800.0);
        v.y = 3000.0;
        let at = pos2(700.0, 200.0);
        let before = v.point_at(at - v.rect.center());
        v.zoom_around(3.0, Some(at), &l);
        let after = v.point_at(at - v.rect.center());
        assert!((before[1] - after[1]).abs() < 0.01, "{before:?} {after:?}");
        out_and_back(&mut v, &l, Some(at));
        let back = v.point_at(at - v.rect.center());
        assert!((before[1] - back[1]).abs() < 0.01, "{before:?} {back:?}");
    }

    #[test]
    fn moving_the_view_starts_a_new_zoom() {
        let l = Layout::new(vec![[600.0, 800.0]; 15], 6.0);
        let mut v = view(600.0, 400.0);
        v.y = 3000.0;
        v.set_zoom(0.5, &l);
        v.y += 1000.0;
        let middle = v.point_at(Vec2::ZERO);
        v.set_zoom(2.0, &l);
        assert!((v.point_at(Vec2::ZERO)[1] - middle[1]).abs() < 0.01);
    }

    #[test]
    fn a_new_pointer_position_is_held_at_once_and_keys_keep_what_was_held() {
        let l = Layout::new(vec![[600.0, 800.0]; 15], 6.0);
        let mut v = view(1000.0, 800.0);
        v.y = 3000.0;
        v.zoom_around(2.0, Some(pos2(100.0, 100.0)), &l);
        let other = pos2(800.0, 700.0);
        let before = v.point_at(other - v.rect.center());
        v.zoom_around(3.0, Some(other), &l);
        let after = v.point_at(other - v.rect.center());
        assert!((before[1] - after[1]).abs() < 0.01);
        let held = v.zoom_hold;
        v.set_zoom(4.0, &l);
        assert_eq!(v.zoom_hold.map(|h| h.point), held.map(|h| h.point));
    }

    #[test]
    fn tiles_grow_with_the_zoom_and_the_page_within_what_can_be_rendered() {
        assert_eq!(tile_side(2.0, 3000), TILE);
        assert_eq!(tile_side(12.0, 3000), 2048);
        assert_eq!(tile_side(100.0, 3000), MAX_TILE);
        assert_eq!(tile_side(2.0, 12000), 3000);
        assert_eq!(tile_side(f32::NAN, 100), TILE);
    }

    #[test]
    fn screen_and_page_positions_round_trip() {
        let l = layout();
        let mut v = view(800.0, 600.0);
        v.zoom = 2.0;
        v.y = 810.0;
        let t = v.transform(&l, 1);
        let screen = t.point([100.0, 50.0]);
        let (page, at) = v.page_under(&l, screen).unwrap();
        assert_eq!(page, 1);
        assert!((at[0] - 100.0).abs() < 1e-3 && (at[1] - 50.0).abs() < 1e-3);
    }

    #[test]
    fn the_gap_between_pages_is_no_page() {
        let l = layout();
        let mut v = view(600.0, 400.0);
        v.y = 805.0;
        assert!(v.page_under(&l, pos2(300.0, 0.0)).is_none());
    }

    #[test]
    fn reveal_moves_only_when_it_must() {
        let l = layout();
        let mut v = view(600.0, 400.0);
        v.reveal(&l, 0, [10.0, 100.0, 50.0, 110.0]);
        assert_eq!(v.y, 0.0);
        v.reveal(&l, 1, [10.0, 100.0, 50.0, 120.0]);
        assert_eq!(v.y, 810.0 + 110.0 - 200.0);
    }

    #[test]
    fn anchors_survive_a_zoom() {
        let l = layout();
        let mut v = view(600.0, 400.0);
        v.y = 900.0;
        let a = v.anchor(&l);
        assert_eq!((a.page, a.y), (1, 90.0));
        v.zoom = 3.0;
        v.y = 0.0;
        v.go_to(&l, a);
        assert_eq!(v.y, 900.0);
    }

    #[test]
    fn tiles_cover_the_visible_area_and_stop_at_the_page_edge() {
        let tiles = tiles_crossing(1500, 2100, [0.0, 0.0, 1500.0, 1100.0], TILE);
        assert_eq!(tiles.len(), 4);
        assert_eq!((tiles[1].x, tiles[1].w), (1024, 476));
        let last = tiles_crossing(1500, 2100, [1400.0, 2090.0, 9000.0, 9000.0], TILE);
        assert_eq!(last.len(), 1);
        assert_eq!((last[0].x, last[0].y, last[0].w, last[0].h), (1024, 2048, 476, 52));
    }

    #[test]
    fn an_exact_multiple_of_the_tile_has_no_empty_tile() {
        let tiles = tiles_crossing(2048, 1024, [0.0, 0.0, 2048.0, 1024.0], TILE);
        assert_eq!(tiles.len(), 2);
        assert!(tiles.iter().all(|t| t.w > 0 && t.h > 0));
    }
    const ROTATIONS: [u16; 4] = [0, 90, 180, 270];

    fn transform(rotation: u16) -> PageTransform {
        PageTransform { origin: pos2(10.0, 20.0), scale: 2.0, rotation, size: [600.0, 800.0] }
    }

    #[test]
    fn page_points_round_trip_through_every_rotation() {
        for rotation in ROTATIONS {
            let t = transform(rotation);
            for p in [[0.0, 0.0], [600.0, 800.0], [123.0, 456.0], [599.0, 1.0]] {
                let back = t.to_page(t.point(p));
                assert!((back[0] - p[0]).abs() < 1e-3 && (back[1] - p[1]).abs() < 1e-3, "{rotation}: {p:?} -> {back:?}");
            }
        }
    }

    #[test]
    fn a_page_rect_maps_to_the_screen_rect_of_its_turned_corners() {
        let r = [100.0, 50.0, 300.0, 90.0];
        // Width 600, height 800; scale 2 from (10, 20). Turned by 90 the page is 800 wide, 600 tall.
        let expect = [
            (0, [210.0, 120.0, 610.0, 200.0]),
            (90, [10.0 + 2.0 * (800.0 - 90.0), 20.0 + 200.0, 10.0 + 2.0 * (800.0 - 50.0), 20.0 + 600.0]),
            (180, [10.0 + 2.0 * 300.0, 20.0 + 2.0 * 710.0, 10.0 + 2.0 * 500.0, 20.0 + 2.0 * 750.0]),
            (270, [10.0 + 2.0 * 50.0, 20.0 + 2.0 * 300.0, 10.0 + 2.0 * 90.0, 20.0 + 2.0 * 500.0]),
        ];
        for (rotation, [x0, y0, x1, y1]) in expect {
            let got = transform(rotation).rect(r);
            assert_eq!([got.min.x, got.min.y, got.max.x, got.max.y], [x0, y0, x1, y1], "{rotation}");
        }
    }

    #[test]
    fn the_whole_page_fills_the_turned_page_area() {
        for rotation in ROTATIONS {
            let t = transform(rotation);
            let got = t.rect([0.0, 0.0, 600.0, 800.0]);
            let (w, h) = if rotation % 180 == 0 { (1200.0, 1600.0) } else { (1600.0, 1200.0) };
            assert_eq!([got.min.x, got.min.y, got.width(), got.height()], [10.0, 20.0, w, h], "{rotation}");
        }
    }

    #[test]
    fn a_turned_layout_swaps_the_sizes_and_restacks_the_pages() {
        let mut l = layout();
        for (rotation, sizes, tops) in [
            (0, [[600.0, 800.0], [600.0, 800.0], [300.0, 400.0]], [0.0, 810.0, 1620.0]),
            (90, [[800.0, 600.0], [800.0, 600.0], [400.0, 300.0]], [0.0, 610.0, 1220.0]),
            (180, [[600.0, 800.0], [600.0, 800.0], [300.0, 400.0]], [0.0, 810.0, 1620.0]),
            (270, [[800.0, 600.0], [800.0, 600.0], [400.0, 300.0]], [0.0, 610.0, 1220.0]),
        ] {
            l.set_rotation(rotation);
            assert_eq!(l.sizes, sizes, "{rotation}");
            assert_eq!(l.tops, tops, "{rotation}");
            assert_eq!(l.page_sizes, [[600.0, 800.0], [600.0, 800.0], [300.0, 400.0]]);
        }
        l.set_rotation(90);
        assert_eq!((l.total, l.max_width), (1520.0, 800.0));
    }

    #[test]
    fn rotations_are_normalised() {
        assert_eq!([normalize_rotation(0), normalize_rotation(90), normalize_rotation(450), normalize_rotation(-90)], [0, 90, 90, 270]);
        assert_eq!([normalize_rotation(360), normalize_rotation(100), normalize_rotation(-1), normalize_rotation(-360)], [0, 90, 270, 0]);
    }

    #[test]
    fn the_page_is_under_the_pointer_in_its_own_space_when_turned() {
        let mut l = layout();
        l.set_rotation(90);
        let v = view(800.0, 600.0);
        let t = v.transform(&l, 0);
        let p = t.point([590.0, 10.0]);
        let (page, at) = v.page_under(&l, p).unwrap();
        assert_eq!(page, 0);
        assert!((at[0] - 590.0).abs() < 1e-3 && (at[1] - 10.0).abs() < 1e-3);
        // Beyond the turned page's height of 600 is the gap, though the page is 800 tall unturned.
        assert!(v.page_under(&l, pos2(400.0, 605.0)).is_none());
    }

    #[test]
    fn the_tile_texture_corners_follow_the_rotation() {
        let screen = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        for rotation in ROTATIONS {
            // Each screen corner must show the texture corner that the rotation takes there.
            for (uv, want) in tile_uv(rotation).into_iter().zip(screen) {
                let at = rotate_point(rotation, [1.0, 1.0], [uv.x, uv.y]);
                assert_eq!(at, want, "{rotation}");
            }
        }
        assert_eq!(tile_uv(0), [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)]);
        assert_eq!(tile_uv(90)[0], pos2(0.0, 1.0));
    }

    #[test]
    fn the_needed_pixels_are_found_in_unturned_space() {
        // A 1000 x 2000 pixel raster; turned by 90 it shows 2000 wide and 1000 tall at (100, 100).
        let origin = pos2(100.0, 100.0);
        let clip = Rect::from_min_max(pos2(100.0, 100.0), pos2(600.0, 300.0));
        assert_eq!(visible_pixels(0, [1000.0, 2000.0], origin, 1.0, clip), [0.0, 0.0, 500.0, 200.0]);
        // The left of the turned page is the bottom of the raster, and its top is the left.
        assert_eq!(visible_pixels(90, [1000.0, 2000.0], origin, 1.0, clip), [0.0, 1500.0, 200.0, 2000.0]);
        assert_eq!(visible_pixels(180, [1000.0, 2000.0], origin, 1.0, clip), [500.0, 1800.0, 1000.0, 2000.0]);
        assert_eq!(visible_pixels(270, [1000.0, 2000.0], origin, 1.0, clip), [800.0, 0.0, 1000.0, 500.0]);
    }

    #[test]
    fn reveal_looks_at_the_turned_page() {
        let mut l = layout();
        l.set_rotation(90);
        let mut v = view(600.0, 400.0);
        // Page 1 is 600 tall turned, from 610. Its x of 590 is at y 10 turned.
        v.reveal(&l, 1, [590.0, 700.0, 595.0, 710.0]);
        assert_eq!(v.y, 610.0 + 592.5 - 200.0);
    }

    const SIZES: [[f32; 2]; 5] = [[600.0, 800.0], [800.0, 600.0], [1.0, 1000.0], [333.5, 71.25], [500.0, 500.0]];

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-2 && (a[1] - b[1]).abs() < 1e-2
    }

    #[test]
    fn prop_to_page_inverts_point_everywhere() {
        for rotation in ROTATIONS {
            for size in SIZES {
                for (origin, scale) in [(pos2(0.0, 0.0), 1.0), (pos2(-37.5, 12.25), 0.37), (pos2(1000.0, -500.0), 3.0)] {
                    let t = PageTransform { origin, scale, rotation, size };
                    let mut points =
                        vec![[0.0, 0.0], [size[0], 0.0], [0.0, size[1]], size, [size[0] / 2.0, 0.0], [0.0, size[1] / 2.0], [size[0], size[1] / 2.0]];
                    for i in 0..=20 {
                        for j in 0..=20 {
                            points.push([size[0] * i as f32 / 20.0, size[1] * j as f32 / 20.0]);
                        }
                    }
                    for p in points {
                        let back = t.to_page(t.point(p));
                        assert!(close(back, p), "{rotation} {size:?} {scale}: {p:?} -> {back:?}");
                        let again = unrotate_point(rotation, size, rotate_point(rotation, size, p));
                        assert!(close(again, p));
                    }
                }
            }
        }
    }

    #[test]
    fn prop_corners_go_to_the_expected_corners_clockwise() {
        // Page corners in order: top-left, top-right, bottom-right, bottom-left. Turned clockwise
        // by 90, the top-left goes to the top-right, so the list rotates by one corner per quarter.
        for size in SIZES {
            let [w, h] = size;
            let corners = [[0.0, 0.0], [w, 0.0], [w, h], [0.0, h]];
            for (rotation, shift) in [(0u16, 0usize), (90, 1), (180, 2), (270, 3)] {
                let (tw, th) = if rotation % 180 == 0 { (w, h) } else { (h, w) };
                let shown = [[0.0, 0.0], [tw, 0.0], [tw, th], [0.0, th]];
                for (i, c) in corners.iter().enumerate() {
                    let got = rotate_point(rotation, size, *c);
                    assert!(close(got, shown[(i + shift) % 4]), "{rotation} {size:?} corner {i}: {got:?}");
                }
            }
        }
    }

    #[test]
    fn prop_tile_uv_agrees_with_rotate_point() {
        // Screen corners of a tile at pixel box (x, y, tw, th) of a raster; the uv at each screen
        // corner is the tile's page corner that rotate_point puts there.
        let raster = [1000.0_f32, 1700.0];
        for rotation in ROTATIONS {
            for (x, y, tw, th) in [(0.0, 0.0, 1000.0, 1700.0), (0.0, 0.0, 100.0, 50.0), (900.0, 1650.0, 100.0, 50.0), (24.0, 512.0, 300.0, 77.0)] {
                let uv = tile_uv(rotation);
                let page_corner = |u: Pos2| [x + u.x * tw, y + u.y * th];
                let shown: Vec<[f32; 2]> = uv.iter().map(|u| rotate_point(rotation, raster, page_corner(*u))).collect();
                let (x0, x1) = (shown.iter().map(|p| p[0]).fold(f32::MAX, f32::min), shown.iter().map(|p| p[0]).fold(f32::MIN, f32::max));
                let (y0, y1) = (shown.iter().map(|p| p[1]).fold(f32::MAX, f32::min), shown.iter().map(|p| p[1]).fold(f32::MIN, f32::max));
                let want = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
                for (got, want) in shown.iter().zip(want) {
                    assert!(close(*got, want), "{rotation} ({x},{y}): {shown:?} vs {want:?}");
                }
                // And the tile's top-left page point is where uv (0,0) is shown.
                let corner = rotate_point(rotation, raster, [x, y]);
                let at00 = uv.iter().position(|u| *u == pos2(0.0, 0.0)).unwrap();
                assert!(close(shown[at00], corner), "{rotation}");
            }
        }
    }

    #[test]
    fn prop_visible_pixels_with_tiles_matches_brute_force() {
        let (pw, ph) = (1300u32, 2100u32);
        let side = 512u32;
        let origin = pos2(100.0, 50.0);
        let unit = 0.5;
        for rotation in ROTATIONS {
            let pixels = [pw as f32, ph as f32];
            let turned = if rotation % 180 == 0 { [pixels[0], pixels[1]] } else { [pixels[1], pixels[0]] };
            let shown = Rect::from_min_size(origin, vec2(turned[0], turned[1]) * unit);
            // Clips inside the page, with a positive area, and some that touch tile edges
            // from inside only.
            let mut clips = vec![];
            for i in 0..12 {
                for j in 0..12 {
                    let a = pos2(shown.left() + shown.width() * i as f32 / 12.0, shown.top() + shown.height() * j as f32 / 12.0);
                    for (dw, dh) in [(1.0, 1.0), (37.3, 80.1), (300.0, 20.0), (shown.width(), shown.height())] {
                        clips.push(Rect::from_min_max(a, pos2((a.x + dw).min(shown.right()), (a.y + dh).min(shown.bottom()))));
                    }
                }
            }
            for clip in clips {
                if !(clip.width() > 0.0 && clip.height() > 0.0) {
                    continue;
                }
                let got = tiles_crossing(pw, ph, visible_pixels(rotation, pixels, origin, unit, clip), side);
                // Brute force: a tile is needed when its screen rect overlaps the clip with area.
                let mut want = vec![];
                for ty in 0..ph.div_ceil(side) {
                    for tx in 0..pw.div_ceil(side) {
                        let (x, y) = (tx * side, ty * side);
                        let (w, h) = (side.min(pw - x), side.min(ph - y));
                        let a = rotate_point(rotation, pixels, [x as f32, y as f32]);
                        let b = rotate_point(rotation, pixels, [(x + w) as f32, (y + h) as f32]);
                        let r = Rect::from_two_pos(origin + vec2(a[0], a[1]) * unit, origin + vec2(b[0], b[1]) * unit);
                        let inter = r.intersect(clip);
                        if inter.width() > 1e-3 && inter.height() > 1e-3 {
                            want.push((x, y, w, h));
                        }
                    }
                }
                let mut got: Vec<_> = got.iter().map(|t| (t.x, t.y, t.w, t.h)).collect();
                got.sort_unstable();
                want.sort_unstable();
                // Every tile that truly intersects must be present; extras may only touch an edge.
                for t in &want {
                    assert!(got.contains(t), "{rotation} {clip:?}: missing {t:?}");
                }
                for t in got.iter().filter(|t| !want.contains(t)) {
                    let a = rotate_point(rotation, pixels, [t.0 as f32, t.1 as f32]);
                    let b = rotate_point(rotation, pixels, [(t.0 + t.2) as f32, (t.1 + t.3) as f32]);
                    let r = Rect::from_two_pos(origin + vec2(a[0], a[1]) * unit, origin + vec2(b[0], b[1]) * unit);
                    let gap_x = (r.left() - clip.right()).max(clip.left() - r.right());
                    let gap_y = (r.top() - clip.bottom()).max(clip.top() - r.bottom());
                    assert!(gap_x.max(gap_y) < 1e-3, "{rotation} {clip:?}: extra tile {t:?} is not even touching");
                }
            }
        }
    }

    #[test]
    fn visible_pixels_edge_touching_clip_reports_the_neighbour() {
        // A clip that ends exactly on a tile boundary: record how many tiles are asked for.
        // Rotation 0: clip right edge at pixel 512 exactly.
        let origin = pos2(0.0, 0.0);
        let clip = Rect::from_min_max(pos2(0.0, 0.0), pos2(512.0, 100.0));
        let n = tiles_crossing(1300, 2100, visible_pixels(0, [1300.0, 2100.0], origin, 1.0, clip), 512).len();
        assert!(n <= 2, "{n}");
    }

    #[test]
    fn set_rotation_round_trips_and_normalises() {
        let mut l = layout();
        let (tops, total, mw) = (l.tops.clone(), l.total, l.max_width);
        // set_rotation is absolute: 90 then 270 is 270, and 90 + 270 is the original.
        l.set_rotation(90);
        l.set_rotation(270);
        assert_eq!(l.rotation, 270);
        l.set_rotation(90 + 270);
        assert_eq!((l.tops.clone(), l.total, l.max_width, l.rotation), (tops.clone(), total, mw, 0));
        l.set_rotation(0);
        assert_eq!((l.tops.clone(), l.total), (tops, total));
        for (input, want) in [
            (-90, 270u16),
            (-450, 270),
            (450, 90),
            (i64::MAX, normalize_rotation(i64::MAX)),
            (i64::MIN, normalize_rotation(i64::MIN)),
            (1_000_000_000_000, normalize_rotation(1_000_000_000_000)),
        ] {
            l.set_rotation(input);
            assert_eq!(l.rotation, want, "{input}");
            assert!(l.rotation < 360 && l.rotation.is_multiple_of(90));
        }
        let mut l = Layout::new(vec![], 8.0);
        l.set_rotation(90);
        assert_eq!((l.total, l.max_width), (0.0, 0.0));
    }

    #[test]
    fn normalize_rotation_edge_values() {
        assert_eq!(normalize_rotation(-90), 270);
        assert_eq!(normalize_rotation(450), 90);
        assert_eq!(normalize_rotation(45), 0);
        assert_eq!(normalize_rotation(-45), 270);
        assert_eq!(normalize_rotation(89), 0);
        assert_eq!(normalize_rotation(359), 270);
        for v in [i64::MIN, i64::MAX, 1_000_000_000_007, -1_000_000_000_007, 70000] {
            let r = normalize_rotation(v);
            assert!(r < 360 && r.is_multiple_of(90), "{v} -> {r}");
        }
    }

    #[test]
    fn shown_rect_agrees_with_the_transform_rect() {
        let mut l = layout();
        for rotation in ROTATIONS {
            l.set_rotation(rotation.into());
            let size = l.page_sizes[2];
            let t = PageTransform { origin: pos2(0.0, 0.0), scale: 1.0, rotation, size };
            for r in [[10.0, 20.0, 100.0, 300.0], [0.0, 0.0, 300.0, 400.0], [299.0, 399.0, 300.0, 400.0]] {
                let s = l.shown_rect(2, r);
                let q = t.rect(r);
                assert!(close([s[0], s[1]], [q.min.x, q.min.y]) && close([s[2], s[3]], [q.max.x, q.max.y]), "{rotation} {r:?}");
            }
        }
    }

    #[test]
    fn shown_points_and_turned_anchors_are_lossless_on_a_non_square_page() {
        let mut l = Layout::new(vec![[600.0, 800.0]], 8.0);
        for rotation in ROTATIONS {
            l.set_rotation(rotation.into());
            let p = [123.0, 456.0];
            let d = l.shown_point(0, p);
            assert_eq!(unrotate_point(rotation, [600.0, 800.0], d), p, "{rotation}");
        }
        for from in ROTATIONS {
            for to in ROTATIONS {
                for a in [Anchor { page: 0, y: 456.0, x: 0.0 }, Anchor { page: 0, y: 10.0, x: -40.0 }] {
                    let there = l.turn_anchor(a, from, to);
                    let back = l.turn_anchor(there, to, from);
                    assert!((back.y - a.y).abs() < 1e-3 && (back.x - a.x).abs() < 1e-3, "{from}->{to}: {a:?} {back:?}");
                }
            }
        }
    }

    #[test]
    fn a_quarter_turn_of_an_anchor_by_hand() {
        let l = Layout::new(vec![[600.0, 800.0]], 8.0);
        // Unturned, y 100 and x 50 right of the centre is the page point [350, 100]. Turned 90 degrees
        // clockwise (the page is then 800 wide) that point is at [800 - 100, 350] = [700, 350], which is
        // 700 - 400 = 300 right of the centre, 350 down.
        let got = l.turn_anchor(Anchor { page: 0, y: 100.0, x: 50.0 }, 0, 90);
        assert_eq!((got.page, got.y, got.x), (0, 350.0, 300.0));
        // A half turn keeps the width: [350, 100] -> [250, 700], x -50.
        let got = l.turn_anchor(Anchor { page: 0, y: 100.0, x: 50.0 }, 0, 180);
        assert_eq!((got.y, got.x), (700.0, -50.0));
        // A page that isn't there is left alone.
        let gone = Anchor { page: 9, y: 1.0, x: 2.0 };
        assert_eq!(l.turn_anchor(gone, 0, 90), gone);
    }

    #[test]
    fn a_page_point_is_shown_where_the_rotation_puts_it() {
        let mut l = Layout::new(vec![[600.0, 800.0]], 8.0);
        assert_eq!(l.shown_point(0, [100.0, 200.0]), [100.0, 200.0]);
        l.set_rotation(90);
        assert_eq!(l.shown_point(0, [100.0, 200.0]), [600.0, 100.0]);
        l.set_rotation(180);
        assert_eq!(l.shown_point(0, [100.0, 200.0]), [500.0, 600.0]);
    }
}

#[cfg(test)]
mod turn_fuzz_tests {
    use super::*;

    /// A small deterministic xorshift generator, as the crate has no property-testing dependency.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        /// A float in `lo..hi`, on a coarse grid so that the arithmetic is exact in f32.
        fn float(&mut self, lo: f32, hi: f32) -> f32 {
            lo + (hi - lo) * (self.next() % 4096) as f32 / 4096.0
        }
    }

    const ROTATIONS: [u16; 4] = [0, 90, 180, 270];

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-2 + 1e-5 * a.abs().max(b.abs())
    }

    fn same(a: Anchor, b: Anchor) -> bool {
        a.page == b.page && close(a.y, b.y) && close(a.x, b.x)
    }

    fn cases(count: usize) -> Vec<(Layout, Anchor)> {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        (0..count)
            .map(|_| {
                let pages = 1 + (rng.next() % 4) as usize;
                let sizes = (0..pages).map(|_| [rng.float(10.0, 2000.0), rng.float(10.0, 2000.0)]).collect();
                let layout = Layout::new(sizes, 8.0);
                let page = (rng.next() % pages as u64) as usize;
                let [w, h] = layout.page_sizes[page];
                // Any in-page point, whatever the rotation: `x` within half the narrower side.
                let a = Anchor { page, y: rng.float(0.5, w.min(h)), x: rng.float(-w.min(h) / 2.0, w.min(h) / 2.0) };
                (layout, a)
            })
            .collect()
    }

    #[test]
    fn turn_anchor_round_trips_for_every_pair() {
        for (layout, a) in cases(2000) {
            for from in ROTATIONS {
                for to in ROTATIONS {
                    let there = layout.turn_anchor(a, from, to);
                    let back = layout.turn_anchor(there, to, from);
                    assert!(same(a, back), "{:?} {a:?} {from}->{to}: {back:?}", layout.page_sizes[a.page]);
                }
            }
        }
    }

    #[test]
    fn turn_anchor_to_the_same_rotation_is_the_identity() {
        for (layout, a) in cases(500) {
            for r in ROTATIONS {
                assert!(same(a, layout.turn_anchor(a, r, r)), "{:?} {a:?} {r}", layout.page_sizes[a.page]);
            }
        }
    }

    #[test]
    fn four_quarter_turns_return_the_original() {
        for (layout, a) in cases(2000) {
            for start in ROTATIONS {
                let mut at = a;
                let mut r = start;
                for _ in 0..4 {
                    let next = (r + 90) % 360;
                    at = layout.turn_anchor(at, r, next);
                    r = next;
                }
                assert!(same(a, at), "{:?} {a:?} from {start}: {at:?}", layout.page_sizes[a.page]);
            }
        }
    }

    #[test]
    fn an_anchor_at_or_above_the_page_top_stays_at_the_top() {
        let layout = Layout::new(vec![[600.0, 800.0]], 8.0);
        for y in [0.0, -5.0, -0.5] {
            for from in ROTATIONS {
                for to in ROTATIONS {
                    let there = layout.turn_anchor(Anchor { page: 0, y, x: 40.0 }, from, to);
                    assert_eq!(there.y, 0.0, "{y} {from}->{to}");
                    // The x survives a half turn or none, and is the column's centre across a quarter turn.
                    let quarter = from % 180 != to % 180;
                    assert_eq!(there.x, if quarter { 0.0 } else { 40.0 }, "{y} {from}->{to}");
                }
            }
        }
    }

    #[test]
    fn turn_anchor_keeps_the_page_point() {
        for (mut layout, a) in cases(2000) {
            for from in ROTATIONS {
                for to in ROTATIONS {
                    let size = layout.page_sizes[a.page];
                    let width = |r: u16| if r.is_multiple_of(180) { size[0] } else { size[1] };
                    layout.set_rotation(i64::from(from));
                    let point = unrotate_point(from, size, [width(from) / 2.0 + a.x, a.y]);
                    let turned = layout.turn_anchor(a, from, to);
                    layout.set_rotation(i64::from(to));
                    let shown = layout.shown_point(a.page, point);
                    assert!(close(shown[0], width(to) / 2.0 + turned.x) && close(shown[1], turned.y), "{size:?} {a:?} {from}->{to}");
                    // And the page point is the same one the turned anchor unrotates to.
                    let back = unrotate_point(to, size, shown);
                    assert!(close(back[0], point[0]) && close(back[1], point[1]));
                }
            }
        }
    }

    #[test]
    fn shown_point_at_rotation_zero_is_the_identity() {
        let layout = Layout::new(vec![[300.0, 500.0], [612.0, 792.0]], 8.0);
        let mut rng = Rng(7);
        for _ in 0..500 {
            let p = [rng.float(-100.0, 900.0), rng.float(-100.0, 900.0)];
            for page in 0..2 {
                assert_eq!(layout.shown_point(page, p), p);
            }
        }
    }

    #[test]
    fn shown_point_stays_inside_the_shown_page() {
        let mut rng = Rng(11);
        for r in ROTATIONS {
            let mut layout = Layout::new(vec![[300.0, 500.0]], 8.0);
            layout.set_rotation(i64::from(r));
            let [w, h] = layout.sizes[0];
            for _ in 0..200 {
                let p = layout.shown_point(0, [rng.float(0.0, 300.0), rng.float(0.0, 500.0)]);
                assert!((0.0..=w).contains(&p[0]) && (0.0..=h).contains(&p[1]), "{r} {p:?}");
            }
        }
    }

    #[test]
    fn turn_anchor_on_a_page_past_the_end_is_unchanged() {
        let layout = Layout::new(vec![[300.0, 500.0]], 8.0);
        for page in [1, 7, usize::MAX] {
            let a = Anchor { page, y: 12.0, x: -3.0 };
            assert_eq!(layout.turn_anchor(a, 0, 90), a);
        }
        assert_eq!(Layout::default().turn_anchor(Anchor { page: 0, y: 1.0, x: 2.0 }, 90, 270), Anchor { page: 0, y: 1.0, x: 2.0 });
    }

    #[test]
    fn turn_anchor_hand_computed_on_a_non_square_page() {
        // 600 wide, 800 high. The page point at the top of the view, centred: [300, 100].
        let layout = Layout::new(vec![[600.0, 800.0]], 8.0);
        let a = Anchor { page: 0, y: 100.0, x: 0.0 };
        // At 90 the point [300, 100] is shown at [800 - 100, 300] on a page 800 wide: x = 700 - 400.
        assert_eq!(layout.turn_anchor(a, 0, 90), Anchor { page: 0, y: 300.0, x: 300.0 });
        // At 180 it is at [300, 700].
        assert_eq!(layout.turn_anchor(a, 0, 180), Anchor { page: 0, y: 700.0, x: 0.0 });
        // At 270 it is at [100, 300] on a page 800 wide: x = 100 - 400.
        assert_eq!(layout.turn_anchor(a, 0, 270), Anchor { page: 0, y: 300.0, x: -300.0 });
    }
}

#[cfg(test)]
mod rotation_round_two_tests {
    use super::*;

    const ROTATIONS: [i64; 4] = [0, 90, 180, 270];
    const W: f32 = 300.0;
    const H: f32 = 500.0;

    fn layout(rotation: i64) -> Layout {
        let mut layout = Layout::new(vec![[W, H]], 8.0);
        layout.set_rotation(rotation);
        layout
    }

    fn grid(max: f32) -> Vec<f32> {
        (0..=8).map(|i| max * i as f32 / 8.0).collect()
    }

    #[test]
    fn shown_dest_y_is_within_the_displayed_height_for_every_presence_combination() {
        for r in ROTATIONS {
            let l = layout(r);
            let shown_h = l.sizes[0][1];
            let xs: Vec<Option<f32>> = std::iter::once(None).chain(grid(W).into_iter().map(Some)).collect();
            let ys: Vec<Option<f32>> = std::iter::once(None).chain(grid(H).into_iter().map(Some)).collect();
            for &x in &xs {
                for &y in &ys {
                    let v = l.shown_dest_y(0, x, y);
                    assert!((0.0..=shown_h).contains(&v), "r {r} x {x:?} y {y:?} -> {v} not in 0..={shown_h}");
                    if y.is_none() {
                        assert_eq!(v, 0.0, "no y is the top at {r}");
                    }
                    if x.is_none() && r % 180 != 0 {
                        assert_eq!(v, 0.0, "no x is the top at {r}");
                    }
                }
            }
        }
    }

    #[test]
    fn shown_dest_y_has_the_documented_value_at_each_rotation() {
        let (x, y) = (Some(70.0), Some(120.0));
        assert_eq!(layout(0).shown_dest_y(0, x, y), 120.0);
        assert_eq!(layout(180).shown_dest_y(0, x, y), H - 120.0);
        assert_eq!(layout(90).shown_dest_y(0, x, y), 70.0);
        assert_eq!(layout(270).shown_dest_y(0, x, y), W - 70.0);
        assert_eq!(layout(270).shown_dest_y(0, None, y), 0.0);
        assert_eq!(layout(90).shown_dest_y(0, None, y), 0.0);
    }

    #[test]
    fn at_rotation_zero_shown_dest_y_is_the_old_behaviour() {
        let l = layout(0);
        for x in [None, Some(0.0), Some(123.0), Some(-5.0)] {
            for y in [None, Some(0.0), Some(1.5), Some(499.0), Some(-3.0), Some(900.0)] {
                assert_eq!(l.shown_dest_y(0, x, y), y.unwrap_or(0.0), "{x:?} {y:?}");
                assert_eq!(l.shown_spot_y(0, x, y, None), y.unwrap_or(0.0));
            }
        }
    }

    #[test]
    fn shown_spot_y_agrees_with_shown_dest_y_without_a_rect_and_is_within_the_page_with_one() {
        let rects = [[10.0, 20.0, 200.0, 400.0], [0.0, 0.0, W, H], [50.0, 60.0, 50.0, 60.0], [299.0, 499.0, 300.0, 500.0]];
        for r in ROTATIONS {
            let l = layout(r);
            let shown_h = l.sizes[0][1];
            for x in [None, Some(30.0)] {
                for y in [None, Some(40.0)] {
                    assert_eq!(l.shown_spot_y(0, x, y, None), l.shown_dest_y(0, x, y));
                    for rect in rects {
                        let v = l.shown_spot_y(0, x, y, Some(rect));
                        assert!((0.0..=shown_h).contains(&v), "r {r} {rect:?} -> {v}");
                        // The topmost corner: no corner is shown higher.
                        let [x0, y0, x1, y1] = rect;
                        let lowest_corner_y =
                            [[x0, y0], [x1, y0], [x0, y1], [x1, y1]].iter().map(|c| l.shown_point(0, *c)[1]).fold(f32::INFINITY, f32::min);
                        assert!((v - lowest_corner_y).abs() < 1e-3, "r {r} {rect:?}: {v} vs topmost {lowest_corner_y}");
                    }
                }
            }
        }
    }

    #[test]
    fn turn_anchor_at_or_above_the_top_stays_at_the_top_for_every_pair() {
        let l = layout(0);
        for from in [0u16, 90, 180, 270] {
            for to in [0u16, 90, 180, 270] {
                let quarter = from % 180 != to % 180;
                for y in [0.0, -0.0, -1.0, -250.0] {
                    for x in [-120.0, 0.0, 33.0] {
                        let got = l.turn_anchor(Anchor { page: 0, y, x }, from, to);
                        assert_eq!(got.y, 0.0, "{from}->{to} y {y}");
                        assert_eq!(got.x, if quarter { 0.0 } else { x }, "{from}->{to} y {y} x {x}");
                    }
                }
            }
        }
    }

    #[test]
    fn turn_anchor_slightly_below_the_top_is_lossless_and_stays_near_the_top_of_its_page() {
        let l = layout(0);
        for from in [0u16, 90, 180, 270] {
            for to in [0u16, 90, 180, 270] {
                for y in [0.001, 0.01, 0.5] {
                    let a = Anchor { page: 0, y, x: 12.0 };
                    let there = l.turn_anchor(a, from, to);
                    let back = l.turn_anchor(there, to, from);
                    assert!((back.y - y).abs() < 1e-2 && (back.x - 12.0).abs() < 1e-2, "{from}->{to} y {y}: {back:?}");
                    assert!(there.y.is_finite() && there.x.is_finite());
                }
            }
        }
    }
}

#[cfg(test)]
mod two_page_tests {
    use super::*;

    fn layout() -> Layout {
        Layout::new(vec![[600.0, 800.0], [600.0, 800.0], [300.0, 400.0]], 10.0)
    }

    fn view(w: f32, h: f32) -> View {
        View { rect: Rect::from_min_size(Pos2::ZERO, vec2(w, h)), ..View::new() }
    }

    fn book(sizes: Vec<[f32; 2]>, cover: bool) -> Layout {
        let mut l = Layout::new(sizes, 10.0);
        l.set_two_page(true, cover);
        l
    }

    fn rows(l: &Layout) -> Vec<(usize, usize)> {
        l.rows.iter().map(|r| (r.first, r.last)).collect()
    }

    #[test]
    fn single_page_mode_is_a_column_of_centred_rows() {
        let l = layout();
        assert_eq!(l.lefts, [-300.0, -300.0, -150.0]);
        assert_eq!(rows(&l), [(0, 0), (1, 1), (2, 2)]);
        assert_eq!(l.max_width, 600.0);
        assert_eq!(l.row_size(2), [300.0, 400.0]);
        assert_eq!(l.page_at(900.0), 1);
        assert_eq!(l.last_page_at(900.0), 1);
    }

    #[test]
    fn rows_with_the_cover_alone_on_the_right() {
        let l = book(vec![[100.0, 200.0]; 5], true);
        assert_eq!(rows(&l), [(0, 0), (1, 2), (3, 4)]);
        assert_eq!(l.lefts, [5.0, -105.0, 5.0, -105.0, 5.0]);
        assert_eq!(l.tops, [0.0, 210.0, 210.0, 420.0, 420.0]);
        assert_eq!(l.total, 620.0);
        assert_eq!(l.max_width, 210.0);
        // An even count: no trailing lone page.
        assert_eq!(rows(&book(vec![[100.0, 200.0]; 4], true)), [(0, 0), (1, 2), (3, 3)]);
        assert_eq!(book(vec![[100.0, 200.0]; 4], true).lefts[3], -105.0);
    }

    #[test]
    fn rows_without_the_cover_alone_and_a_trailing_page_on_the_left() {
        let l = book(vec![[100.0, 200.0]; 5], false);
        assert_eq!(rows(&l), [(0, 1), (2, 3), (4, 4)]);
        assert_eq!(l.lefts, [-105.0, 5.0, -105.0, 5.0, -105.0]);
        assert_eq!(rows(&book(vec![[100.0, 200.0]; 4], false)), [(0, 1), (2, 3)]);
    }

    #[test]
    fn few_pages() {
        assert!(book(vec![], true).rows.is_empty());
        assert_eq!(book(vec![], true).page_at(5.0), 0);
        assert_eq!(book(vec![], true).total, 0.0);
        assert_eq!(book(vec![[100.0, 200.0]], true).lefts, [5.0]);
        assert_eq!(book(vec![[100.0, 200.0]], false).lefts, [-105.0]);
        assert_eq!(rows(&book(vec![[100.0, 200.0]; 2], true)), [(0, 0), (1, 1)]);
        assert_eq!(book(vec![[100.0, 200.0]; 2], true).lefts, [5.0, -105.0]);
        assert_eq!(rows(&book(vec![[100.0, 200.0]; 2], false)), [(0, 1)]);
    }

    #[test]
    fn pages_of_a_row_are_centred_vertically_about_the_spine() {
        let l = book(vec![[100.0, 200.0], [80.0, 300.0], [120.0, 100.0]], false);
        assert_eq!(rows(&l), [(0, 1), (2, 2)]);
        assert_eq!(l.tops, [50.0, 0.0, 310.0]);
        assert_eq!(l.rows[0].height, 300.0);
        assert_eq!(l.lefts[0] + l.sizes[0][0], -5.0);
        assert_eq!(l.lefts[1], 5.0);
        assert_eq!(l.max_width, 250.0);
        assert_eq!(l.total, 410.0);
        assert_eq!(l.row_size(0), [2.0 * 105.0, 300.0]);
    }

    #[test]
    fn a_turned_two_page_layout_is_built_from_the_shown_sizes() {
        let mut l = book(vec![[100.0, 200.0]; 3], false);
        l.set_rotation(90);
        assert_eq!(l.sizes[0], [200.0, 100.0]);
        assert_eq!(l.lefts, [-205.0, 5.0, -205.0]);
        assert_eq!(l.tops, [0.0, 0.0, 110.0]);
        assert_eq!(l.page_sizes[0], [100.0, 200.0]);
        assert_eq!(l.max_width, 410.0);
    }

    #[test]
    fn lookups_by_row() {
        let l = book(vec![[100.0, 200.0], [100.0, 300.0], [100.0, 200.0], [100.0, 200.0]], false);
        // Row 0 is 0..300, row 1 starts at 310; page 0 starts at 50, below the row's top.
        assert_eq!(l.page_at(0.0), 0);
        assert_eq!(l.page_at(20.0), 0);
        assert_eq!(l.page_at(309.0), 0);
        assert_eq!(l.page_at(310.0), 2);
        assert_eq!(l.last_page_at(20.0), 1);
        assert_eq!(l.last_page_at(400.0), 3);
        let v = view(400.0, 100.0);
        assert_eq!(v.visible_pages(&l), 0..=1);
        let mut v = v;
        v.y = 300.0;
        assert_eq!(v.visible_pages(&l), 0..=3);
        v.y = 400.0;
        assert_eq!(v.visible_pages(&l), 2..=3);
        assert_eq!(v.current_page(&l), 2);
        assert_eq!(v.anchor(&l), Anchor { page: 2, y: 90.0, x: 0.0 });
    }

    #[test]
    fn rows_are_stepped_through_by_page_by_rows() {
        let l = book(vec![[100.0, 200.0]; 6], true);
        assert_eq!(rows(&l), [(0, 0), (1, 2), (3, 4), (5, 5)]);
        assert_eq!(l.page_by_rows(0, 1), 1);
        assert_eq!(l.page_by_rows(2, 1), 3);
        assert_eq!(l.page_by_rows(1, 5), 5);
        assert_eq!(l.page_by_rows(4, -1), 1);
        assert_eq!(l.page_by_rows(1, -3), 0);
        assert_eq!(l.row_first(4), 3);
        let single = layout();
        assert_eq!(single.page_by_rows(0, 2), 2);
        assert_eq!(single.page_by_rows(1, 9), 2);
        assert_eq!(single.page_by_rows(1, -9), 0);
    }

    #[test]
    fn anchors_on_either_page_of_a_row_round_trip() {
        let l = book(vec![[100.0, 200.0], [100.0, 300.0], [100.0, 200.0], [100.0, 250.0]], false);
        let mut v = view(300.0, 100.0);
        for page in [0, 1, 2, 3] {
            v.go_to(&l, Anchor { page, y: 0.0, x: 0.0 });
            assert_eq!(v.y, l.tops[page], "page {page}");
            let a = v.anchor(&l);
            assert_eq!(a.page, l.row_first(page));
            let mut w = v;
            w.y = 0.0;
            w.go_to(&l, a);
            assert!((w.y - v.y).abs() < 1e-3);
        }
    }

    #[test]
    fn screen_and_page_positions_round_trip_on_the_right_hand_page() {
        let l = book(vec![[100.0, 200.0], [80.0, 300.0]], false);
        let mut v = view(600.0, 400.0);
        v.zoom = 2.0;
        v.y = 0.0;
        v.clamp(&l);
        let t = v.transform(&l, 1);
        assert_eq!(t.origin.x, 300.0 + 5.0 * 2.0 - v.x * 2.0);
        let screen = t.point([40.0, 100.0]);
        let (page, at) = v.page_under(&l, screen).unwrap();
        assert_eq!(page, 1);
        assert!((at[0] - 40.0).abs() < 1e-3 && (at[1] - 100.0).abs() < 1e-3);
        // Beside the shorter left page, below it, there is no page.
        let t0 = v.transform(&l, 0);
        assert!(v.page_under(&l, t0.point([50.0, 210.0])).is_none());
        assert_eq!(v.page_under(&l, t0.point([50.0, 100.0])).map(|p| p.0), Some(0));
    }

    #[test]
    fn the_view_clamps_to_the_width_about_the_spine() {
        let l = book(vec![[100.0, 200.0]; 3], true);
        assert_eq!(l.max_width, 210.0);
        let mut v = view(110.0, 100.0);
        v.x = 999.0;
        v.clamp(&l);
        assert_eq!(v.x, 50.0);
        v.x = -999.0;
        v.clamp(&l);
        assert_eq!(v.x, -50.0);
        let mut wide = view(500.0, 100.0);
        wide.x = 30.0;
        wide.clamp(&l);
        assert_eq!(wide.x, 0.0);
    }

    #[test]
    fn leaving_two_page_mode_restores_the_single_column() {
        let before = layout();
        let mut l = layout();
        l.set_two_page(true, true);
        l.set_two_page(false, true);
        assert_eq!((l.tops.clone(), l.lefts.clone(), l.total, l.max_width), (before.tops, before.lefts, before.total, before.max_width));
    }

    #[test]
    fn reveal_uses_the_page_position() {
        let l = book(vec![[100.0, 200.0], [100.0, 200.0]], false);
        let mut v = view(50.0, 200.0);
        v.reveal(&l, 1, [60.0, 10.0, 80.0, 20.0]);
        assert_eq!(v.x, 5.0 + 70.0);
    }

    #[test]
    fn stepping_by_rows_from_the_right_hand_page_goes_to_the_next_row() {
        let l = book(vec![[600.0, 800.0]; 5], true);
        assert_eq!(rows(&l), vec![(0, 0), (1, 2), (3, 4)]);
        assert_eq!(l.page_by_rows(2, 1), 3);
        assert_eq!(l.page_by_rows(1, 1), 3);
        assert_eq!(l.page_by_rows(2, -1), 0);
        assert_eq!(l.page_by_rows(4, 1), 3);
    }

    #[test]
    fn a_row_anchor_shows_the_top_of_the_taller_page() {
        let l = book(vec![[600.0, 800.0], [300.0, 400.0], [600.0, 800.0]], true);
        // Row (1, 2): the first page is the shorter, centred 200 below the row's top.
        let row = l.rows[1];
        let a = l.row_anchor(2, 7.0);
        assert_eq!((a.page, a.x), (1, 7.0));
        assert_eq!(a.y, row.top - l.tops[1]);
        assert!(a.y < 0.0);
        let mut v = view(1000.0, 500.0);
        v.go_to(&l, a);
        assert_eq!(v.y, row.top);
        // A row of equal pages is at the page's top, and one past the end is a plain anchor.
        assert_eq!(l.row_anchor(0, 0.0).y, 0.0);
        assert_eq!(l.row_anchor(9, 0.0), Anchor { page: 9, y: 0.0, x: 0.0 });
    }

    #[test]
    fn turning_an_anchor_in_two_page_mode_keeps_its_page_point() {
        let sizes = vec![[600.0, 800.0], [300.0, 400.0], [500.0, 700.0]];
        let l = book(sizes.clone(), true);
        let turned = |rotation: i64| {
            let mut t = l.clone();
            t.set_rotation(rotation);
            t
        };
        // The point of page 1 (right of the spine, x from the spine) under the view's centre.
        let a = Anchor { page: 1, y: 100.0, x: l.lefts[1] + 40.0 };
        let t = l.turn_anchor(a, 0, 90);
        let after = turned(90);
        let point =
            |layout: &Layout, a: Anchor, rotation: u16| unrotate_point(rotation, layout.page_sizes[a.page], [a.x - layout.lefts[a.page], a.y]);
        let (p0, p1) = (point(&l, a, 0), point(&after, t, 90));
        assert!((p0[0] - p1[0]).abs() < 1e-3 && (p0[1] - p1[1]).abs() < 1e-3, "{p0:?} {p1:?}");
        // Every turn comes back.
        for (from, to) in [(0, 90), (0, 180), (90, 270), (270, 0)] {
            let b = l.turn_anchor(l.turn_anchor(a, from, to), to, from);
            assert!((b.x - a.x).abs() < 1e-3 && (b.y - a.y).abs() < 1e-3, "{from} {to}: {b:?}");
        }
        // At the top, a half turn keeps the offset from the page's centre, a quarter centres the page.
        let top = Anchor { page: 1, y: 0.0, x: l.lefts[1] + 50.0 };
        let half = l.turn_anchor(top, 0, 180);
        assert!((half.x - (turned(180).lefts[1] + 50.0)).abs() < 1e-3);
        let quarter = l.turn_anchor(top, 0, 90);
        assert!((quarter.x - (after.lefts[1] + after.sizes[1][0] / 2.0)).abs() < 1e-3);
    }

    #[test]
    fn a_jump_shows_its_page_without_moving_a_view_that_does() {
        let l = book(vec![[600.0, 800.0]; 4], true);
        // Rows (0), (1, 2), (3). Page 1 is [-605, -5] and page 2 is [5, 605] about the spine.
        let span = 400.0;
        // Zoomed in on the left page: the right one's centre.
        assert_eq!(l.x_for(2, -300.0, span), 305.0);
        assert_eq!(l.x_for(1, 300.0, span), -305.0);
        // A view that shows a page whole, or sits over one wider than itself, stays.
        assert_eq!(l.x_for(2, 305.0, 700.0), 305.0);
        assert_eq!(l.x_for(2, 100.0, span), 100.0);
        assert_eq!(l.x_for(2, 0.0, 1300.0), 0.0);
        // Beyond the panning range it is clamped.
        assert_eq!(l.x_for(2, -300.0, 1000.0), 105.0);
        // One page at a time, every page shares the centre: x is kept.
        let single = layout();
        assert_eq!(single.x_for(1, 37.0, 10.0), 37.0);
        assert_eq!(single.x_for(9, 37.0, 10.0), 37.0);
    }

    #[test]
    fn the_anchor_and_current_page_are_the_page_in_the_row_that_is_looked_at() {
        let l = book(vec![[600.0, 800.0], [300.0, 400.0], [600.0, 800.0]], true);
        let mut v = view(300.0, 300.0);
        v.zoom = 1.0;
        let row = l.rows[1];
        v.y = row.top + 100.0;
        v.x = -300.0;
        assert_eq!(v.current_page(&l), 1);
        assert_eq!(v.anchor(&l).page, 1);
        v.x = 300.0;
        assert_eq!(v.current_page(&l), 2);
        let a = v.anchor(&l);
        assert_eq!((a.page, a.x), (2, 300.0));
        assert_eq!(a.y, v.y - l.tops[2]);
        // On the spine, it is the first.
        v.x = 0.0;
        assert_eq!(v.anchor(&l).page, 1);
        // The anchor goes back to the same view.
        v.x = 300.0;
        let mut w = View { x: 0.0, y: 0.0, ..v };
        w.go_to(&l, v.anchor(&l));
        assert_eq!((w.x, w.y), (v.x, v.y));
        // The shorter first page starts below the row's top: the anchor on it is negative.
        v.y = row.top;
        v.x = -300.0;
        assert!(v.anchor(&l).y < 0.0);
        // One page at a time, nothing changes.
        let single = layout();
        let mut s = view(300.0, 300.0);
        s.x = 200.0;
        s.y = 900.0;
        assert_eq!(s.anchor(&single).page, single.page_at(900.0));
    }
}

#[cfg(test)]
mod two_page_fuzz_tests {
    use super::*;

    pub(super) struct Rng(pub(super) u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        pub(super) fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }

        pub(super) fn float(&mut self, lo: f32, hi: f32) -> f32 {
            lo + (hi - lo) * (self.next() % 4096) as f32 / 4096.0
        }
    }

    const GAP: f32 = 8.0;
    const EPS: f32 = 1e-3;

    pub(super) fn sizes(rng: &mut Rng, n: usize) -> Vec<[f32; 2]> {
        (0..n).map(|_| [rng.float(10.0, 900.0).round(), rng.float(10.0, 900.0).round()]).collect()
    }

    /// Every combination of cover and rotation over random documents of 0..40 pages.
    pub(super) fn each_layout(seed: u64, rounds: usize, mut check: impl FnMut(&mut Rng, &Layout, bool)) {
        let mut rng = Rng(seed);
        for _ in 0..rounds {
            let n = rng.below(40);
            let page_sizes = sizes(&mut rng, n);
            for cover in [false, true] {
                for rotation in [0, 90, 180, 270] {
                    let mut l = Layout::new(page_sizes.clone(), GAP);
                    l.set_rotation(rotation);
                    l.set_two_page(true, cover);
                    check(&mut rng, &l, cover);
                }
            }
        }
    }

    #[test]
    fn rows_partition_the_pages_by_the_rules_and_the_geometry_is_exact() {
        each_layout(0x1234_5678_9ABC_DEF1, 150, |_, l, cover| {
            let n = l.len();
            assert_eq!((l.tops.len(), l.lefts.len(), l.row_of.len()), (n, n, n));
            // Expected rows.
            let mut expected = Vec::new();
            let mut p = 0;
            if cover && n > 0 {
                expected.push((0, 0));
                p = 1;
            }
            while p < n {
                let last = (p + 1).min(n - 1);
                expected.push((p, last));
                p = last + 1;
            }
            let got: Vec<_> = l.rows.iter().map(|r| (r.first, r.last)).collect();
            assert_eq!(got, expected, "n={n} cover={cover}");
            // Contiguous, ordered, each page in exactly one row.
            let mut next = 0;
            for (i, row) in l.rows.iter().enumerate() {
                assert_eq!(row.first, next);
                assert!(row.last >= row.first && row.last - row.first <= 1);
                for page in row.first..=row.last {
                    assert_eq!(l.row_of[page], i);
                }
                next = row.last + 1;
            }
            assert_eq!(next, n);
        });
    }

    #[test]
    fn rows_are_stacked_with_the_gap_and_pages_sit_about_the_spine() {
        each_layout(0x1234_5678_9ABC_DEF2, 150, |_, l, cover| {
            let n = l.len();
            let mut y = 0.0f32;
            for (i, row) in l.rows.iter().enumerate() {
                assert!((row.top - y).abs() < EPS, "row {i} top {} vs {y}", row.top);
                let tallest = (row.first..=row.last).map(|p| l.sizes[p][1]).fold(0.0, f32::max);
                assert!((row.height - tallest).abs() < EPS);
                for page in row.first..=row.last {
                    let h = l.sizes[page][1];
                    let top = l.tops[page];
                    assert!(top >= row.top - EPS && top + h <= row.top + row.height + EPS);
                    assert!((top - (row.top + (row.height - h) / 2.0)).abs() < EPS, "vertical centring");
                }
                if row.first != row.last {
                    let (a, b) = (row.first, row.last);
                    assert!((l.lefts[a] + l.sizes[a][0] + GAP / 2.0).abs() < EPS, "left page ends at -gap/2");
                    assert!((l.lefts[b] - GAP / 2.0).abs() < EPS, "right page starts at gap/2");
                } else {
                    let a = row.first;
                    let [w, _] = l.sizes[a];
                    if cover && a == 0 {
                        assert!((l.lefts[a] - GAP / 2.0).abs() < EPS, "cover on the right");
                    } else {
                        assert!((l.lefts[a] + w + GAP / 2.0).abs() < EPS, "lone page on the left");
                    }
                }
                if let Some(next) = l.rows.get(i + 1) {
                    assert!((next.top - (row.top + row.height + GAP)).abs() < EPS, "rows separated by the gap");
                }
                y = row.top + row.height + GAP;
            }
            let total = if n == 0 { 0.0 } else { y - GAP };
            assert!((l.total - total).abs() < EPS, "total {} vs {total}", l.total);
        });
    }

    #[test]
    fn the_width_covers_every_page_and_sizes_follow_the_rotation() {
        each_layout(0x1234_5678_9ABC_DEF3, 150, |_, l, _| {
            let n = l.len();
            let reach = (0..n).map(|p| l.lefts[p].abs().max((l.lefts[p] + l.sizes[p][0]).abs())).fold(0.0, f32::max);
            assert!((l.max_width - 2.0 * reach).abs() < EPS);
            for p in 0..n {
                assert!(l.lefts[p] >= -l.max_width / 2.0 - EPS && l.lefts[p] + l.sizes[p][0] <= l.max_width / 2.0 + EPS);
            }
            // The sizes follow the rotation.
            for p in 0..n {
                let [w, h] = l.page_sizes[p];
                assert_eq!(l.sizes[p], if l.rotation % 180 == 0 { [w, h] } else { [h, w] });
            }
        });
    }

    #[test]
    fn single_page_mode_is_the_old_cumulative_column_at_every_rotation() {
        let mut rng = Rng(77);
        for _ in 0..200 {
            let n = rng.below(40);
            let ps = sizes(&mut rng, n);
            for rotation in [0, 90, 180, 270] {
                let mut l = Layout::new(ps.clone(), GAP);
                l.set_rotation(rotation);
                let mut y = 0.0;
                let mut widest: f32 = 0.0;
                for p in 0..n {
                    let [w, h] = l.sizes[p];
                    assert_eq!(l.lefts[p], -w / 2.0);
                    assert!((l.tops[p] - y).abs() < EPS);
                    assert_eq!((l.rows[p].first, l.rows[p].last, l.row_of[p]), (p, p, p));
                    y += h + GAP;
                    widest = widest.max(w);
                }
                assert!((l.total - (y - GAP).max(0.0)).abs() < EPS);
                assert!((l.max_width - widest).abs() < EPS);
                assert_eq!(l.rows.len(), n);
            }
        }
    }

    #[test]
    fn toggling_two_page_on_and_off_returns_the_single_layout() {
        each_layout(99, 60, |_, l, _| {
            let mut back = l.clone();
            back.set_two_page(false, false);
            let mut fresh = Layout::new(l.page_sizes.clone(), GAP);
            fresh.set_rotation(l.rotation as i64);
            assert_eq!(back.sizes, fresh.sizes);
            assert_eq!(back.tops, fresh.tops);
            assert_eq!(back.lefts, fresh.lefts);
            assert_eq!(back.row_of, fresh.row_of);
            assert_eq!(back.rows, fresh.rows);
            assert_eq!((back.total, back.max_width, back.rotation), (fresh.total, fresh.max_width, fresh.rotation));
            assert!(!back.two_page);
            // Cover without two_page changes nothing either.
            let mut covered = fresh.clone();
            covered.set_two_page(false, true);
            assert_eq!(covered.tops, fresh.tops);
            assert_eq!(covered.lefts, fresh.lefts);
            assert_eq!(covered.rows, fresh.rows);
            // And back on again equals the layout we began with.
            back.set_two_page(true, l.cover_alone);
            assert_eq!(back.rows, l.rows);
            assert_eq!(back.tops, l.tops);
            assert_eq!(back.lefts, l.lefts);
        });
    }

    #[test]
    fn page_at_gives_the_first_page_of_the_row_and_is_monotone() {
        each_layout(4242, 40, |_, l, _| {
            if l.len() == 0 {
                assert_eq!((l.page_at(0.0), l.last_page_at(0.0), l.page_at(100.0)), (0, 0, 0));
                return;
            }
            let steps = 400;
            let mut prev = 0;
            let mut prev_last = 0;
            for i in 0..=steps + 20 {
                let y = -10.0 + (l.total + 20.0) * i as f32 / steps as f32;
                let row = l
                    .rows
                    .iter()
                    .position(|r| y >= r.top && y <= r.top + r.height)
                    // In a gap, or above the document: the row above, or the first.
                    .unwrap_or_else(|| l.rows.iter().rposition(|r| r.top <= y).unwrap_or(0));
                if let Some(r) = l.rows.iter().position(|r| y >= r.top && y < r.top + r.height) {
                    assert_eq!(r, row);
                }
                let page = l.page_at(y);
                assert_eq!(page, l.rows[row].first, "y={y}");
                assert_eq!(l.last_page_at(y), l.rows[row].last, "y={y}");
                assert_eq!(l.row_at(y), row);
                assert_eq!(l.row_first(page), page);
                assert!(page >= prev && l.last_page_at(y) >= prev_last, "monotone at y={y}");
                prev = page;
                prev_last = l.last_page_at(y);
            }
        });
    }

    fn rect_of(l: &Layout, p: usize) -> (f32, f32) {
        (l.tops[p], l.tops[p] + l.sizes[p][1])
    }

    #[test]
    fn visible_pages_contains_every_page_in_view_and_only_pages_of_rows_in_view() {
        each_layout(555, 40, |rng, l, _| {
            let n = l.len();
            if n == 0 {
                return;
            }
            for _ in 0..30 {
                let mut v = View::new();
                v.zoom = rng.float(0.2, 3.0);
                v.rect = Rect::from_min_size(Pos2::ZERO, vec2(rng.float(50.0, 900.0), rng.float(50.0, 900.0)));
                v.y = rng.float(-50.0, l.total + 50.0);
                let (y0, y1) = (v.y, v.y + v.rect.height() / v.zoom);
                let range = v.visible_pages(l);
                let brute: Vec<usize> = (0..n)
                    .filter(|&p| {
                        let (t, b) = rect_of(l, p);
                        t < y1 && b > y0
                    })
                    .collect();
                for &p in &brute {
                    assert!(range.contains(&p), "page {p} in view [{y0},{y1}] missing from {range:?}");
                }
                assert!(*range.end() < n);
                // Nothing wanted beyond the rows that touch the view, plus the row above its top.
                for p in range.clone() {
                    let r = l.rows[l.row_of[p]];
                    let touches = r.top <= y1 && r.top + r.height >= y0;
                    let above = l.row_of[p] == l.row_at(y0.max(0.0));
                    assert!(touches || above, "page {p} not near view [{y0},{y1}]");
                }
            }
        });
    }

    #[test]
    fn transform_to_page_and_page_under_round_trip_on_every_page() {
        each_layout(31337, 30, |rng, l, _| {
            let n = l.len();
            if n == 0 {
                return;
            }
            for _ in 0..40 {
                let page = rng.below(n);
                let [w, h] = l.page_sizes[page];
                let p = [rng.float(0.05, 0.95) * w, rng.float(0.05, 0.95) * h];
                let mut v = View::new();
                v.zoom = rng.float(0.25, 3.0);
                v.rect = Rect::from_min_size(pos2(13.0, 7.0), vec2(rng.float(100.0, 900.0), rng.float(100.0, 900.0)));
                v.x = rng.float(-300.0, 300.0);
                // Put the point somewhere on screen.
                let shown = rotate_point(l.rotation, l.page_sizes[page], p);
                let fy = rng.float(0.05, 0.95);
                v.y = l.tops[page] + shown[1] - fy * v.rect.height() / v.zoom;
                let tr = v.transform(l, page);
                let screen = tr.point(p);
                let back = tr.to_page(screen);
                assert!((back[0] - p[0]).abs() < 0.05 && (back[1] - p[1]).abs() < 0.05);
                // Where it lands on screen follows the layout.
                let want =
                    pos2(v.rect.center().x + (l.lefts[page] + shown[0] - v.x) * v.zoom, v.rect.top() + (l.tops[page] + shown[1] - v.y) * v.zoom);
                assert!((screen - want).length() < 0.05);
                if v.rect.contains(screen) {
                    let (under, at) = v.page_under(l, screen).unwrap_or_else(|| panic!("nothing under page {page} point {p:?}"));
                    assert_eq!(under, page, "rotation {} p={p:?} screen={screen:?}", l.rotation);
                    assert!((at[0] - p[0]).abs() < 0.05 && (at[1] - p[1]).abs() < 0.05);
                }
            }
        });
    }

    #[test]
    fn page_under_finds_nothing_in_the_gutter_or_beyond_the_pages() {
        each_layout(8, 20, |rng, l, _| {
            let Some(row) = l.rows.iter().find(|r| r.first != r.last) else { return };
            let mut v = View::new();
            v.rect = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 800.0));
            v.y = row.top - 10.0;
            let x = v.rect.center().x;
            let _ = rng;
            // The gutter is only GAP wide: sample its middle.
            let y = v.rect.top() + (row.top + row.height / 2.0 - v.y) * v.zoom;
            assert_eq!(v.page_under(l, pos2(x, y)), None);
        });
    }

    #[test]
    fn anchor_and_go_to_round_trip_in_two_page_mode() {
        each_layout(2024, 40, |rng, l, _| {
            if l.len() == 0 {
                return;
            }
            for _ in 0..30 {
                let mut v = View::new();
                v.zoom = rng.float(0.2, 3.0);
                v.rect = Rect::from_min_size(Pos2::ZERO, vec2(rng.float(50.0, 900.0), rng.float(50.0, 900.0)));
                v.x = rng.float(-400.0, 400.0);
                v.y = rng.float(-20.0, l.total + 20.0);
                v.clamp(l);
                let a = v.anchor(l);
                assert!(a.page < l.len());
                let mut w = View { x: 12345.0, y: 12345.0, ..v };
                w.go_to(l, a);
                assert!((w.y - v.y).abs() < 0.01 && (w.x - v.x).abs() < 0.01, "{:?} vs {:?} anchor {a:?}", (w.x, w.y), (v.x, v.y));
                // An anchor on any page, at any depth, goes to that page's own top.
                let page = rng.below(l.len());
                let mut u = View { rect: v.rect, zoom: v.zoom, ..View::new() };
                u.go_to(l, Anchor { page, y: 0.0, x: 0.0 });
                let want = l.tops[page];
                let mut c = u;
                c.clamp(l);
                assert!((u.y - c.y).abs() < 1e-3);
                if l.total > v.rect.height() / v.zoom {
                    assert!((u.y - want.min(l.total - v.rect.height() / v.zoom)).abs() < 0.01);
                }
            }
        });
    }

    #[test]
    fn reveal_brings_a_rect_on_either_page_into_view() {
        each_layout(6, 20, |rng, l, _| {
            let n = l.len();
            if n == 0 {
                return;
            }
            for _ in 0..20 {
                let page = rng.below(n);
                let [w, h] = l.page_sizes[page];
                let r = [w * 0.4, h * 0.4, w * 0.5, h * 0.5];
                let mut v = View::new();
                v.zoom = 1.0;
                v.rect = Rect::from_min_size(Pos2::ZERO, vec2(2000.0, 2000.0));
                v.reveal(l, page, r);
                let s = l.shown_rect(page, r);
                let top = l.tops[page] + s[1];
                let left = l.lefts[page] + s[0];
                assert!(top >= v.y - 0.01 && top <= v.y + 2000.0 + 0.01, "y {top} view {}", v.y);
                assert!(left >= v.x - 1000.0 - 0.01 && left <= v.x + 1000.0 + 0.01, "x {left} view {}", v.x);
            }
        });
    }

    #[test]
    fn page_by_rows_and_row_first_at_both_ends() {
        each_layout(909, 40, |_, l, _| {
            let n = l.len();
            let rows = l.rows.len() as isize;
            for page in 0..n {
                let row = l.row_of[page] as isize;
                assert_eq!(l.row_first(page), l.rows[row as usize].first);
                assert!(l.row_first(page) <= page);
                for step in [-1000, -41, -5, -3, -2, -1, 0, 1, 2, 3, 5, 41, 1000, isize::MAX, isize::MIN] {
                    let target = (row.saturating_add(step)).clamp(0, rows - 1) as usize;
                    assert_eq!(l.page_by_rows(page, step), l.rows[target].first, "page {page} step {step}");
                }
                assert_eq!(l.page_by_rows(page, 0), l.row_first(page));
                // Forward n then back n returns to the first page of the row if it did not hit an end.
                for k in 0..4isize {
                    if row + k < rows {
                        assert_eq!(l.page_by_rows(l.page_by_rows(page, k), -k), l.row_first(page));
                    }
                }
            }
            assert_eq!(l.row_first(n + 3), n + 3);
            if n > 0 {
                assert_eq!(l.page_by_rows(n + 3, 1), 0);
                assert_eq!(l.page_by_rows(n - 1, 10), l.rows[l.rows.len() - 1].first);
                assert_eq!(l.page_by_rows(0, -10), 0);
            }
        });
    }

    #[test]
    fn row_size_is_the_reach_of_the_row_and_its_height() {
        each_layout(11, 40, |_, l, _| {
            for p in 0..l.len() {
                let r = l.rows[l.row_of[p]];
                let [rw, rh] = l.row_size(p);
                assert!((rh - r.height).abs() < EPS);
                assert!(rw <= l.max_width + EPS);
                for q in r.first..=r.last {
                    assert!(rw + EPS >= 2.0 * l.lefts[q].abs() && rw + EPS >= 2.0 * (l.lefts[q] + l.sizes[q][0]).abs());
                }
            }
        });
    }
}

#[cfg(test)]
mod two_page_round_two_tests {
    use super::two_page_fuzz_tests::{Rng, each_layout, sizes};
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-2 + 1e-4 * a.abs().max(b.abs())
    }

    /// Random documents laid out one page at a time, in every rotation.
    fn each_single(seed: u64, rounds: usize, mut check: impl FnMut(&mut Rng, &Layout)) {
        let mut rng = Rng(seed);
        for _ in 0..rounds {
            let n = rng.below(30);
            let page_sizes = sizes(&mut rng, n);
            for rotation in [0, 90, 180, 270] {
                let mut l = Layout::new(page_sizes.clone(), 8.0);
                l.set_rotation(rotation);
                check(&mut rng, &l);
            }
        }
    }

    fn laid_two_page(sizes: &[[f32; 2]]) -> Layout {
        let mut l = Layout::new(sizes.to_vec(), 10.0);
        l.set_two_page(true, false);
        l
    }

    fn small_view(rng: &mut Rng, width: f32) -> View {
        let mut v = View::new();
        v.zoom = rng.float(0.5, 2.0);
        v.rect = Rect::from_min_size(Pos2::ZERO, vec2(width, 2.0));
        v
    }

    #[test]
    fn row_anchor_go_to_and_row_at_round_trip_over_unequal_rows() {
        each_layout(0xB0_0C, 80, |rng, l, _| {
            for page in 0..l.len() {
                let mut v = small_view(rng, 100.0);
                v.go_to(l, l.row_anchor(page, 0.0));
                if l.total > v.span()[1] {
                    assert_eq!(l.row_at(v.y), l.row_of[page], "page {page}: y {}", v.y);
                }
            }
        });
    }

    #[test]
    fn row_at_tolerates_a_y_one_ulp_below_a_row_top() {
        let l = laid_two_page(&[[100.0, 300.0], [100.0, 200.0], [100.0, 50.0]]);
        let top = l.rows[1].top;
        assert_eq!(l.row_at(f32::from_bits(top.to_bits() - 1)), 1);
        assert_eq!(l.row_at(top - 0.5), 0);
    }

    #[test]
    fn x_converts_between_arrangements_by_keeping_the_offset_from_the_page_centre() {
        let mut l = Layout::new(vec![[100.0, 100.0]; 4], 10.0);
        l.set_two_page(true, false);
        // The right page of the first row is centred 55 from the spine.
        let (page, x) = (1, 55.0 + 20.0);
        let offset = x - l.centre_of(page);
        l.set_two_page(false, false);
        assert_eq!(l.centre_of(page) + offset, 20.0);
        l.set_two_page(true, false);
        assert_eq!(l.centre_of(page) + offset, x);
    }

    #[test]
    fn page_near_gives_the_first_page_when_both_show_and_the_shown_one_otherwise() {
        let l = laid_two_page(&[[100.0, 100.0], [100.0, 100.0]]);
        // Both pages (-110..110 about the spine) fit in 300 across, wherever the pan is.
        for x in [-30.0, 0.0, 30.0] {
            assert_eq!(l.page_near(0.0, x, 300.0), 0);
        }
        // Only the left page shows in a view 150 wide centred at -80, only the right at 80.
        assert_eq!(l.page_near(0.0, -80.0, 150.0), 0);
        assert_eq!(l.page_near(0.0, 80.0, 150.0), 1);
    }

    #[test]
    fn row_anchor_with_go_to_puts_the_row_top_at_the_view_top() {
        each_layout(0xA11CE, 60, |rng, l, _| {
            for page in 0..l.len() {
                let row = l.rows[l.row_of[page]];
                let x = rng.float(-500.0, 500.0);
                let a = l.row_anchor(page, x);
                assert_eq!(a.page, row.first);
                assert_eq!(a.x, x);
                // The unclamped arithmetic.
                assert!(close(l.tops[a.page] + a.y, row.top), "page {page}");
                // A short view in a taller document is not moved by the clamp.
                let mut v = small_view(rng, 100.0);
                v.go_to(l, a);
                assert!(close(v.y, row.top), "page {page}: y {} row top {}", v.y, row.top);
            }
        });
    }

    #[test]
    fn x_for_always_shows_the_page_and_keeps_x_when_it_is_shown() {
        each_layout(0xB0B, 60, |rng, l, _| {
            for page in 0..l.len() {
                let (left, w) = (l.lefts[page], l.sizes[page][0]);
                let right = left + w;
                for _ in 0..12 {
                    let span = rng.float(5.0, 2500.0);
                    let x = match rng.below(3) {
                        0 => rng.float(-1500.0, 1500.0),
                        1 => left + rng.float(0.0, 1.0) * w,
                        _ => rng.float(-1.0, 1.0) * (l.max_width / 2.0),
                    };
                    let half = span / 2.0;
                    let was_shown = if w > span { left <= x && x <= right } else { left >= x - half && right <= x + half };
                    let got = l.x_for(page, x, span);
                    if was_shown {
                        assert_eq!(got, x, "page {page} x {x} span {span}");
                    }
                    let overflow = (l.max_width - span).max(0.0) / 2.0;
                    if !was_shown {
                        assert!(got >= -overflow - 1e-3 && got <= overflow + 1e-3, "{got} outside +-{overflow}");
                    }
                    let tol = 1e-2;
                    // A wider page with x on it is already shown: x is kept, and no more is asked.
                    if w > span && was_shown {
                        continue;
                    }
                    if w <= span {
                        assert!(
                            left >= got - half - tol && right <= got + half + tol,
                            "page [{left},{right}] not in span {got}+-{half} (was_shown {was_shown})"
                        );
                    } else {
                        assert!(
                            got - half >= left - tol && got + half <= right + tol,
                            "span {got}+-{half} not in page [{left},{right}] (was_shown {was_shown})"
                        );
                    }
                }
            }
        });
    }

    #[test]
    fn x_for_in_single_page_mode_is_always_x_and_so_is_a_page_past_the_end() {
        each_single(0xC0FFEE, 40, |rng, l| {
            for page in 0..l.len() + 2 {
                for _ in 0..8 {
                    let (x, span) = (rng.float(-2000.0, 2000.0), rng.float(5.0, 2500.0));
                    assert_eq!(l.x_for(page, x, span), x);
                }
            }
        });
        each_layout(77, 10, |_, l, _| {
            assert_eq!(l.x_for(l.len() + 5, 123.0, 50.0), 123.0);
        });
    }

    #[test]
    fn page_near_is_in_the_row_over_the_page_and_monotone_in_x() {
        each_layout(0xD00D, 50, |rng, l, _| {
            if l.len() == 0 {
                return;
            }
            for _ in 0..30 {
                let y = rng.float(-5.0, l.total + 5.0);
                let row = l.rows[l.row_at(y)];
                let x = rng.float(-1500.0, 1500.0);
                let p = l.page_near(y, x, 0.0);
                assert!(p >= row.first && p <= row.last, "y {y} gave page {p} outside {row:?}");
                for q in row.first..=row.last {
                    let frac = rng.float(0.01, 0.99) / 1.0;
                    let inside = l.lefts[q] + frac * l.sizes[q][0];
                    assert_eq!(l.page_near(y, inside, 0.0), q, "x {inside} inside page {q}");
                }
                // Monotone: sweeping x left to right never goes back a page.
                let mut last = row.first;
                let mut x = -1500.0;
                while x <= 1500.0 {
                    let p = l.page_near(y, x, 0.0);
                    assert!(p >= last, "y {y} x {x}: page {p} after {last}");
                    last = p;
                    x += 7.3;
                }
                assert_eq!(l.page_near(y, -1e6, 0.0), row.first);
                assert_eq!(l.page_near(y, 1e6, 0.0), row.last);
            }
        });
    }

    #[test]
    fn anchor_round_trips_over_the_left_page_the_right_page_and_the_spine() {
        each_layout(0xE66, 40, |rng, l, _| {
            if l.len() == 0 {
                return;
            }
            for page in 0..l.len() {
                let row = l.rows[l.row_of[page]];
                let centre = l.lefts[page] + l.sizes[page][0] / 2.0;
                for x_want in [centre, 0.0, rng.float(-300.0, 300.0)] {
                    let width = rng.float(10.0, 700.0);
                    let mut v = small_view(rng, width);
                    v.x = x_want;
                    v.y = row.top + rng.float(0.0, 1.0) * row.height;
                    v.clamp(l);
                    let a = v.anchor(l);
                    assert!(a.page < l.len());
                    assert!(a.page >= row.first.min(l.rows[l.row_at(v.y.max(0.0))].first));
                    let mut w = View { x: 9999.0, y: 9999.0, ..v };
                    w.go_to(l, a);
                    assert!(close(w.x, v.x) && close(w.y, v.y), "view {:?} -> anchor {a:?} -> {:?}", (v.x, v.y), (w.x, w.y));
                    // current_page agrees where the top and the middle are in the same row.
                    let mid = (v.y + v.span()[1] / 2.0).max(0.0);
                    if l.row_at(mid) == l.row_at(v.y.max(0.0)) {
                        assert_eq!(v.current_page(l), a.page);
                    }
                    // Centred over a page of the row, and not pushed off by the clamp, it is that page.
                    if x_want == centre && close(v.x, centre) && v.y >= row.top - 1e-3 && v.y < row.top + row.height {
                        // Unless the whole row shows, when it is the first page.
                        let half = v.span()[0] / 2.0;
                        let whole = (row.first..=row.last).all(|p| l.lefts[p] >= v.x - half && l.lefts[p] + l.sizes[p][0] <= v.x + half);
                        let want = if whole { row.first } else { page };
                        assert_eq!(a.page, want, "centre {centre} row {row:?}");
                        assert_eq!(v.current_page(l), if whole { row.first } else { page.min(row.last) });
                    }
                }
            }
        });
    }

    #[test]
    fn two_page_turn_anchor_round_trips_and_keeps_the_page_point() {
        each_layout(0xF00D, 25, |rng, l, _| {
            if l.len() == 0 {
                return;
            }
            let mut l = l.clone();
            for _ in 0..12 {
                let page = rng.below(l.len());
                let size = l.page_sizes[page];
                let width = |r: u16| if r.is_multiple_of(180) { size[0] } else { size[1] };
                for from in [0u16, 90, 180, 270] {
                    for to in [0u16, 90, 180, 270] {
                        // A point on the page (lossless only while it stays below the top as turned).
                        l.set_rotation(i64::from(from));
                        let cf = l.lefts[page] + width(from) / 2.0;
                        let y = rng.float(0.5, size[0].min(size[1]) - 0.5);
                        let a = Anchor { page, y, x: cf + rng.float(-0.49, 0.49) * width(from) };
                        let turned = l.turn_anchor(a, from, to);
                        let back = l.turn_anchor(turned, to, from);
                        assert!(
                            back.page == page && close(back.y, a.y) && close(back.x, a.x),
                            "{size:?} {a:?} {from}->{to}: {turned:?} back {back:?}"
                        );
                        // The page point under the view's top centre stays the same.
                        l.set_rotation(i64::from(from));
                        let cf = l.lefts[page] + width(from) / 2.0;
                        let point = unrotate_point(from, size, [width(from) / 2.0 + a.x - cf, a.y]);
                        l.set_rotation(i64::from(to));
                        let ct = l.lefts[page] + width(to) / 2.0;
                        let shown = rotate_point(to, size, point);
                        assert!(
                            close(shown[0], width(to) / 2.0 + turned.x - ct) && close(shown[1], turned.y),
                            "{size:?} {a:?} {from}->{to}: {turned:?} point {point:?} shown {shown:?}"
                        );
                    }
                }
            }
        });
    }

    #[test]
    fn two_page_turn_anchor_at_the_top_goes_to_the_top_of_the_page() {
        each_layout(0x70B, 15, |rng, l, _| {
            if l.len() == 0 {
                return;
            }
            let page = rng.below(l.len());
            let a = Anchor { page, y: -rng.float(0.0, 50.0), x: rng.float(-300.0, 300.0) };
            for from in [0u16, 90, 180, 270] {
                for to in [0u16, 90, 180, 270] {
                    let t = l.turn_anchor(a, from, to);
                    assert_eq!(t.y, 0.0);
                    assert!(t.x.is_finite());
                }
            }
        });
    }

    #[test]
    fn single_page_turn_anchor_is_the_plain_formula() {
        each_single(0x51, 20, |rng, l| {
            if l.len() == 0 {
                return;
            }
            for _ in 0..15 {
                let page = rng.below(l.len());
                let size = l.page_sizes[page];
                let a = Anchor { page, y: rng.float(0.5, 1500.0), x: rng.float(-600.0, 600.0) };
                let width = |r: u16| if r.is_multiple_of(180) { size[0] } else { size[1] };
                for from in [0u16, 90, 180, 270] {
                    for to in [0u16, 90, 180, 270] {
                        let p = unrotate_point(from, size, [width(from) / 2.0 + a.x, a.y]);
                        let [dx, dy] = rotate_point(to, size, p);
                        let want = Anchor { page, y: dy, x: dx - width(to) / 2.0 };
                        let got = l.turn_anchor(a, from, to);
                        assert!(
                            got.page == page && close(got.y, want.y) && close(got.x, want.x),
                            "{size:?} {a:?} {from}->{to}: {got:?} want {want:?}"
                        );
                    }
                }
            }
        });
    }

    #[test]
    fn page_by_rows_from_either_page_of_a_row_steps_to_the_next_and_previous_row_first() {
        each_layout(0xBEEF, 40, |_, l, _| {
            let rows = l.rows.len();
            for page in 0..l.len() {
                let r = l.row_of[page];
                let next = l.rows[(r + 1).min(rows - 1)].first;
                let prev = l.rows[r.saturating_sub(1)].first;
                assert_eq!(l.page_by_rows(page, 1), next, "page {page} +1");
                assert_eq!(l.page_by_rows(page, -1), prev, "page {page} -1");
                if r + 1 < rows {
                    assert_eq!(next, l.rows[r].last + 1);
                }
                if r > 0 {
                    assert_eq!(l.rows[r - 1].last + 1, l.rows[r].first);
                }
            }
        });
    }
}
