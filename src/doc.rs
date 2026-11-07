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
    let top = match view {
        DestView::Xyz { top, .. } | DestView::FitH { top } => top,
        DestView::FitR { rect } => Some(rect[3]),
        DestView::Top | DestView::Fit | DestView::FitV { .. } => None,
    };
    page.dest_fraction(None, top, 0)[1].unwrap_or(0.0) * page.height
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
        Some(Row { page: link.page, rect, target: (*target, dest_y(to, *dest)) })
    };
    info.links.iter().filter_map(row).collect()
}

fn flatten(items: &[OutlineItem], pages: &[PageInfo], depth: usize, out: &mut Vec<TocEntry>) {
    for item in items {
        if let Some(info) = item.page.and_then(|p| pages.get(p).map(|info| (p, info))) {
            out.push(TocEntry { title: item.title.clone(), depth, page: info.0, y: dest_y(info.1, item.view) });
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
