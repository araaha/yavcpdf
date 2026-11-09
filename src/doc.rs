//! An open document: its structure, render workers and the text received so far.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdfcraft_render::{
    DestView, DocInfo, LinkTarget, OutlineItem, PageInfo, PageText, ReadingColors, ReadingParams, RenderConfig, RenderPool, inspect,
};

use crate::config::Theme;
use crate::contents::{Row, contents_rows, entries};
use crate::view::Layout;

/// An outline entry, flattened for the table of contents.
#[derive(Clone, Debug, PartialEq)]
pub struct TocEntry {
    pub title: String,
    pub depth: usize,
    pub page: usize,
    pub y: f32,
    /// Whether `y` is a spot the entry names, rather than the top of its page.
    pub point: bool,
}

pub struct Doc {
    pub path: PathBuf,
    pub bytes: Arc<Vec<u8>>,
    pub info: DocInfo,
    pub layout: Layout,
    pub toc: Vec<TocEntry>,
    /// The printed contents to read `toc` from once their text is here, for a document with no
    /// outline. Empty when there are none, or they have been read.
    pub contents: Vec<Row>,
    /// Each page's links to places in the document, in page points.
    pub internal_links: Vec<Vec<[f32; 4]>>,
    pub text: Vec<Option<Arc<PageText>>>,
    pub pool: RenderPool,
    pub text_pool: RenderPool,
    password: Option<Arc<str>>,
}

fn render_threads() -> usize {
    std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(2, 8))
}

/// How the theme recolours pages.
fn reading_colors(theme: &Theme) -> ReadingColors {
    let params =
        theme.recolor.then(|| ReadingParams { text: theme.page_text.rgb(), background: theme.page.rgb(), only_black: false, line_art: true });
    ReadingColors { params }
}

fn render_config(password: Option<&Arc<str>>, theme: &Theme) -> RenderConfig {
    RenderConfig { password: password.cloned(), reading_colors: reading_colors(theme), ..Default::default() }.on_screen()
}

/// The y on a page that a destination points at, in page points.
pub fn dest_y(page: &PageInfo, view: DestView) -> f32 {
    dest_point(page, view).unwrap_or(0.0)
}

/// The y a destination points at, when it names one; `None` for a whole page or view.
pub fn dest_point(page: &PageInfo, view: DestView) -> Option<f32> {
    let fraction = |x: Option<f32>, y: f32| page.dest_fraction(x, Some(y), 0)[1];
    let f = match view {
        DestView::Xyz { left, top: Some(top), .. } => fraction(left, top),
        DestView::FitH { top: Some(top) } => fraction(None, top),
        DestView::FitR { rect } => [rect[0], rect[2]]
            .into_iter()
            .flat_map(|x| [rect[1], rect[3]].into_iter().map(move |y| (x, y)))
            .filter_map(|(x, y)| fraction(Some(x), y))
            .min_by(f32::total_cmp),
        _ => None,
    };
    f.map(|f| f * page.height)
}

/// The rectangles of links to places in the document, by page.
fn internal_links(info: &DocInfo) -> Vec<Vec<[f32; 4]>> {
    let mut out = vec![Vec::new(); info.pages.len()];
    for link in info.links.iter().filter(|l| matches!(l.target, LinkTarget::Page(..))) {
        let (Some(page), Some(rects)) = (info.pages.get(link.page), out.get_mut(link.page)) else { continue };
        let (a, b) = (page.user_to_view(link.rect[0], link.rect[1]), page.user_to_view(link.rect[2], link.rect[3]));
        rects.push([a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])]);
    }
    out
}

/// Links to places in the document, as rows for finding its printed contents.
fn link_rows(info: &DocInfo) -> Vec<Row> {
    let row = |link: &pdfcraft_render::Link| {
        let LinkTarget::Page(target, dest) = &link.target else { return None };
        let (from, to) = (info.pages.get(link.page)?, info.pages.get(*target)?);
        let (a, b) = (from.user_to_view(link.rect[0], link.rect[1]), from.user_to_view(link.rect[2], link.rect[3]));
        let rect = [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])];
        Some(Row { page: link.page, rect, target: (*target, dest_y(to, *dest)), point: dest_point(to, *dest).is_some() })
    };
    info.links.iter().filter_map(row).collect()
}

fn flatten(items: &[OutlineItem], pages: &[PageInfo], depth: usize, out: &mut Vec<TocEntry>) {
    for item in items {
        if let Some(info) = item.page.and_then(|p| pages.get(p).map(|info| (p, info))) {
            out.push(TocEntry {
                title: item.title.clone(),
                depth,
                page: info.0,
                y: dest_y(info.1, item.view),
                point: dest_point(info.1, item.view).is_some(),
            });
        }
        flatten(&item.children, pages, depth + 1, out);
    }
}

impl Doc {
    pub fn open(path: &Path, password: Option<&str>, theme: &Theme, gap: f32) -> Result<Self, String> {
        let bytes = Arc::new(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?);
        let info = inspect(bytes.clone(), password).map_err(|e| format!("{}: {e:?}", path.display()))?;
        if info.pages.is_empty() {
            return Err(format!("{}: the document has no pages", path.display()));
        }
        let password: Option<Arc<str>> = password.map(Arc::from);
        let layout = Layout::new(info.pages.iter().map(|p| [p.width, p.height]).collect(), gap);
        let mut toc = Vec::new();
        flatten(&info.outline, &info.pages, 0, &mut toc);
        let contents = if toc.is_empty() { contents_rows(link_rows(&info)) } else { Vec::new() };
        let config = render_config(password.as_ref(), theme);
        Ok(Self {
            path: path.to_owned(),
            pool: RenderPool::new(bytes.clone(), render_threads(), config.clone()),
            text_pool: RenderPool::new(bytes.clone(), 2, config),
            text: vec![None; info.pages.len()],
            internal_links: internal_links(&info),
            bytes,
            info,
            layout,
            toc,
            contents,
            password,
        })
    }

    /// Starts rendering with the theme's colours. The caller drops what was rendered before.
    pub fn apply_theme(&mut self, theme: &Theme) {
        let config = render_config(self.password.as_ref(), theme);
        self.pool = RenderPool::new(self.bytes.clone(), render_threads(), config);
    }

    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    pub fn file_name(&self) -> String {
        self.path.file_name().map_or_else(|| self.path.display().to_string(), |n| n.to_string_lossy().into_owned())
    }

    /// Whether the text of the printed contents is here.
    pub fn contents_ready(&self) -> bool {
        !self.contents.is_empty() && self.contents.iter().all(|r| self.text.get(r.page).is_some_and(Option::is_some))
    }

    /// Turns the printed contents into the table of contents.
    pub fn read_contents(&mut self) {
        let rows = std::mem::take(&mut self.contents);
        self.toc = entries(&rows, |page| self.text.get(page).cloned().flatten());
    }

    /// The last table-of-contents entry at or before the place.
    pub fn chapter_at(&self, page: usize, y: f32) -> Option<&TocEntry> {
        self.toc.iter().rfind(|e| (e.page, e.y) <= (page, y))
    }
}

#[cfg(test)]
mod tests {
    use pdfcraft_render::{DestView, OutlineItem, PageInfo};

    use super::{TocEntry, dest_point, dest_y, flatten, link_rows};

    fn page(rotation: u16, crop: [f32; 4]) -> PageInfo {
        let (w, h) = (crop[2] - crop[0], crop[3] - crop[1]);
        let (width, height) = if rotation % 180 == 90 { (h, w) } else { (w, h) };
        PageInfo { width, height, label: "1".into(), crop, rotation }
    }

    const CROPS: [[f32; 4]; 2] = [[0.0, 0.0, 600.0, 800.0], [50.0, 100.0, 650.0, 900.0]];

    #[test]
    fn views_without_a_point_give_none() {
        for rotation in [0, 90, 180, 270] {
            for crop in CROPS {
                let p = page(rotation, crop);
                for view in [
                    DestView::Top,
                    DestView::Fit,
                    DestView::FitV { left: Some(10.0) },
                    DestView::FitV { left: None },
                    DestView::FitH { top: None },
                    DestView::Xyz { left: Some(1.0), top: None, zoom: Some(2.0) },
                    DestView::FitH { top: Some(f32::NAN) },
                    DestView::Xyz { left: None, top: Some(f32::INFINITY), zoom: None },
                ] {
                    assert_eq!(dest_point(&p, view), None, "rot {rotation} {view:?}");
                    assert_eq!(dest_y(&p, view), 0.0, "rot {rotation} {view:?}");
                }
            }
        }
    }

    #[test]
    fn a_top_is_a_displayed_y_for_every_rotation_and_crop_offset() {
        // The user y 600 on crop [50,100,650,900] is 300 below the crop's top (900); on the
        // origin crop [0,0,600,800] it is 200 below the top.
        // Displayed fractions: rot 0 -> v, 90 -> u (= x-based, so None x gives None for y? see below).
        for crop in CROPS {
            let (top_user, below) = (crop[3] - 200.0, 200.0);
            for (rotation, expect) in [(0u16, Some(below)), (180, Some(800.0 - below)), (90, None), (270, None)] {
                let p = page(rotation, crop);
                let view = DestView::FitH { top: Some(top_user) };
                let got = dest_point(&p, view);
                match expect {
                    Some(e) => assert!((got.unwrap() - e).abs() < 1e-3, "rot {rotation} crop {crop:?}: {got:?}"),
                    // Documented behaviour of dest_point: on a quarter turn the top depends on x,
                    // which a top-only destination does not give.
                    None => assert_eq!(got, None, "rot {rotation} crop {crop:?}"),
                }
            }
        }
    }

    #[test]
    fn xyz_and_fith_and_fitr_agree_and_clamp_to_the_page() {
        let p = page(0, CROPS[1]);
        let a = dest_point(&p, DestView::Xyz { left: Some(0.0), top: Some(800.0), zoom: None }).unwrap();
        let b = dest_point(&p, DestView::FitH { top: Some(800.0) }).unwrap();
        assert!((a - 100.0).abs() < 1e-3 && a == b, "{a} {b}");
        // FitR uses rect[3], the top of the normalized rectangle.
        let r = dest_point(&p, DestView::FitR { rect: [60.0, 200.0, 300.0, 800.0] }).unwrap();
        assert!((r - 100.0).abs() < 1e-3, "{r}");
        // Off the page is clamped to its edges.
        assert_eq!(dest_point(&p, DestView::FitH { top: Some(5000.0) }), Some(0.0));
        assert_eq!(dest_point(&p, DestView::FitH { top: Some(-5000.0) }), Some(800.0));
    }

    #[test]
    fn flatten_sets_point_by_whether_the_outline_item_has_a_top() {
        let pages = vec![page(0, CROPS[0]), page(0, CROPS[1])];
        let item = |title: &str, page: Option<usize>, view, children| OutlineItem {
            title: title.into(),
            page,
            view,
            children,
            open: true,
            color: None,
            italic: false,
            bold: false,
        };
        let items = vec![
            item(
                "a",
                Some(0),
                DestView::FitH { top: Some(600.0) },
                vec![
                    item("a1", Some(1), DestView::Fit, vec![]),
                    item("a2", Some(1), DestView::Xyz { left: None, top: None, zoom: None }, vec![]),
                    item("nopage", None, DestView::Top, vec![item("deep", Some(0), DestView::Top, vec![])]),
                    item("badpage", Some(9), DestView::FitH { top: Some(1.0) }, vec![]),
                    item("a3", Some(1), DestView::FitR { rect: [0.0, 0.0, 1.0, 700.0] }, vec![]),
                ],
            ),
            item("b", Some(1), DestView::Top, vec![]),
        ];
        let mut out: Vec<TocEntry> = Vec::new();
        flatten(&items, &pages, 0, &mut out);
        let got: Vec<_> = out.iter().map(|e| (e.title.as_str(), e.depth, e.page, e.y, e.point)).collect();
        assert_eq!(
            got,
            vec![
                ("a", 0, 0, 200.0, true),
                ("a1", 1, 1, 0.0, false),
                ("a2", 1, 1, 0.0, false),
                ("deep", 2, 0, 0.0, false),
                ("a3", 1, 1, 200.0, true),
                ("b", 0, 1, 0.0, false),
            ]
        );
    }

    /// On a quarter-turned page the displayed y comes from the user x, which `dest_point` does not
    /// pass, so even a full Xyz point is lost there (it falls back to the page top).
    #[test]
    fn a_full_point_on_a_quarter_turned_page_is_kept() {
        for rotation in [90, 270] {
            let p = page(rotation, CROPS[0]);
            let view = DestView::Xyz { left: Some(300.0), top: Some(600.0), zoom: None };
            assert!(dest_point(&p, view).is_some(), "rot {rotation}: a point with both coordinates was dropped");
        }
    }

    #[test]
    fn a_row_from_a_fit_link_is_not_a_point() {
        use pdfcraft_render::{DocInfo, Link, LinkTarget};
        let info = DocInfo {
            pages: vec![page(0, CROPS[0])],
            links: vec![Link { page: 0, index: 0, rect: [0.0, 0.0, 10.0, 10.0], target: LinkTarget::Page(0, DestView::Fit) }],
            ..Default::default()
        };
        let rows = link_rows(&info);
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].point);
        assert_eq!(rows[0].target, (0, 0.0));
    }
}
