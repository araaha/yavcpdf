//! An open document: its structure, render workers and the text received so far.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdfcraft_render::{
    DestView, DocInfo, LinkTarget, OpenError, OutlineItem, PageInfo, PageText, ReadingColors, ReadingParams, RenderConfig, RenderPool, inspect,
};

use crate::config::Theme;
use crate::contents::{Row, contents_rows, entries};
use crate::form::FormInfo;
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

/// Why a document didn't open. A password is asked for, so it is told apart from the rest.
#[derive(Debug, PartialEq, Eq)]
pub enum OpenFailure {
    NeedsPassword,
    WrongPassword,
    /// Anything else, as the message to show.
    Other(String),
}

impl OpenFailure {
    fn from_error(path: &Path, error: &OpenError) -> Self {
        match error {
            OpenError::NeedsPassword => Self::NeedsPassword,
            OpenError::WrongPassword => Self::WrongPassword,
            other => Self::Other(format!("{}: {other:?}", path.display())),
        }
    }
}

impl std::fmt::Display for OpenFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NeedsPassword => f.write_str("the document needs a password"),
            Self::WrongPassword => f.write_str("wrong password"),
            Self::Other(message) => f.write_str(message),
        }
    }
}

/// A blank one-page document, shown behind the password menu while the real one is locked.
pub fn placeholder_pdf() -> Vec<u8> {
    let objects =
        ["<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 >>", "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>"];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    out
}

pub struct Doc {
    pub path: PathBuf,
    pub bytes: Arc<Vec<u8>>,
    /// What the pages are rendered from: the document's own bytes, or a copy with the user's form
    /// values filled in. `bytes` stay the document's, as its key and its sidecar come from them.
    pub render_bytes: Arc<Vec<u8>>,
    pub form: FormInfo,
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
    pub fn open(path: &Path, password: Option<&str>, theme: &Theme, gap: f32) -> Result<Self, OpenFailure> {
        let bytes = Arc::new(std::fs::read(path).map_err(|e| OpenFailure::Other(format!("{}: {e}", path.display())))?);
        Self::from_bytes(path, bytes, password, theme, gap)
    }

    /// Opens the document `bytes`, which were read from `path`.
    pub fn from_bytes(path: &Path, bytes: Arc<Vec<u8>>, password: Option<&str>, theme: &Theme, gap: f32) -> Result<Self, OpenFailure> {
        let info = inspect(bytes.clone(), password).map_err(|e| OpenFailure::from_error(path, &e))?;
        if info.pages.is_empty() {
            return Err(OpenFailure::Other(format!("{}: the document has no pages", path.display())));
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
            render_bytes: bytes.clone(),
            form: FormInfo::default(),
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
        self.pool = RenderPool::new(self.render_bytes.clone(), render_threads(), config);
    }

    /// Starts rendering from `bytes`, a copy of the document with a form filled in. The caller
    /// drops what was rendered before.
    pub fn show(&mut self, bytes: Arc<Vec<u8>>, theme: &Theme) {
        self.render_bytes = bytes;
        self.apply_theme(theme);
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

    use std::path::Path;
    use std::sync::Arc;

    use pdfcraft_cos::{Algorithm, Document, NewEncryption, SaveOptions, write_full};
    use pdfcraft_render::OpenError;

    use super::{Doc, OpenFailure, TocEntry, dest_point, dest_y, flatten, link_rows, placeholder_pdf};
    use crate::config::Config;

    fn open_bytes(bytes: Vec<u8>, password: Option<&str>) -> Result<Doc, OpenFailure> {
        let config = Config::load_str("").unwrap();
        Doc::from_bytes(Path::new("/x/locked.pdf"), Arc::new(bytes), password, config.active_theme(), 8.0)
    }

    fn protected(user: &str) -> Vec<u8> {
        let mut doc = Document::open(Arc::new(placeholder_pdf())).unwrap();
        let encryption = NewEncryption {
            algorithm: Algorithm::Aes128,
            user_password: user,
            owner_password: "owner",
            permissions: -4,
            encrypt_metadata: true,
            seed: [9; 32],
        };
        doc.set_encryption(&encryption).unwrap();
        write_full(&doc, &SaveOptions::default()).unwrap()
    }

    #[test]
    fn the_placeholder_is_a_valid_one_page_document() {
        let doc = open_bytes(placeholder_pdf(), None).ok().unwrap();
        assert_eq!(doc.layout.len(), 1);
        assert_eq!(doc.file_name(), "locked.pdf");
    }

    #[test]
    fn open_errors_are_told_apart() {
        let path = Path::new("/x/a.pdf");
        assert_eq!(OpenFailure::from_error(path, &OpenError::NeedsPassword), OpenFailure::NeedsPassword);
        assert_eq!(OpenFailure::from_error(path, &OpenError::WrongPassword), OpenFailure::WrongPassword);
        let other = OpenFailure::from_error(path, &OpenError::Invalid("x".into()));
        assert_eq!(other, OpenFailure::Other("/x/a.pdf: Invalid(\"x\")".to_owned()));
        assert!(matches!(open_bytes(b"not a pdf".to_vec(), None), Err(OpenFailure::Other(_))));
    }

    #[test]
    fn an_encrypted_document_asks_for_its_password() {
        let bytes = protected("hunter2");
        assert_eq!(open_bytes(bytes.clone(), None).err(), Some(OpenFailure::NeedsPassword));
        assert_eq!(open_bytes(bytes.clone(), Some("nope")).err(), Some(OpenFailure::WrongPassword));
        let doc = open_bytes(bytes, Some("hunter2")).ok().unwrap();
        assert_eq!(doc.password(), Some("hunter2"));
    }

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

    fn encrypted(algorithm: Algorithm, user: &str, owner: &str) -> Vec<u8> {
        let mut doc = Document::open(Arc::new(placeholder_pdf())).unwrap();
        let encryption =
            NewEncryption { algorithm, user_password: user, owner_password: owner, permissions: -4, encrypt_metadata: true, seed: [7; 32] };
        doc.set_encryption(&encryption).unwrap();
        write_full(&doc, &SaveOptions::default()).unwrap()
    }

    const ALGORITHMS: [Algorithm; 4] = [Algorithm::Rc4_40, Algorithm::Rc4_128, Algorithm::Aes128, Algorithm::Aes256];

    #[test]
    fn every_algorithm_classifies_no_wrong_and_right_passwords() {
        for algorithm in ALGORITHMS {
            let bytes = encrypted(algorithm, "test", "owner");
            assert_eq!(open_bytes(bytes.clone(), None).err(), Some(OpenFailure::NeedsPassword), "{algorithm:?}");
            assert_eq!(open_bytes(bytes.clone(), Some("")).err(), Some(OpenFailure::WrongPassword), "{algorithm:?} empty");
            assert_eq!(open_bytes(bytes.clone(), Some("Test")).err(), Some(OpenFailure::WrongPassword), "{algorithm:?} case");
            assert_eq!(open_bytes(bytes.clone(), Some("nope")).err(), Some(OpenFailure::WrongPassword), "{algorithm:?}");
            let doc = open_bytes(bytes.clone(), Some("test")).ok().unwrap_or_else(|| panic!("{algorithm:?} right password"));
            assert_eq!(doc.layout.len(), 1);
            // The owner password opens it too.
            // (Only AES-256 opens with the owner password in pdfcraft's reader; the others say WrongPassword.)
            if algorithm == Algorithm::Aes256 {
                assert!(open_bytes(bytes, Some("owner")).is_ok(), "{algorithm:?} owner");
            } else {
                assert_eq!(open_bytes(bytes, Some("owner")).err(), Some(OpenFailure::WrongPassword), "{algorithm:?} owner");
            }
        }
    }

    #[test]
    fn an_empty_user_password_opens_without_asking() {
        for algorithm in ALGORITHMS {
            let bytes = encrypted(algorithm, "", "owner");
            assert!(open_bytes(bytes.clone(), None).is_ok(), "{algorithm:?} opens with no password");
            // A password given to a file that needs none is still fine.
            let _ = open_bytes(bytes, Some("whatever"));
        }
    }

    #[test]
    fn non_ascii_and_spaced_passwords_work() {
        for algorithm in ALGORITHMS {
            for password in ["p\u{e4}ssw\u{f6}rd", "  padded  ", " ", "\u{65e5}\u{672c}\u{8a9e}pw", "\u{1f423}x"] {
                let bytes = encrypted(algorithm, password, "owner");
                assert_eq!(open_bytes(bytes.clone(), None).err(), Some(OpenFailure::NeedsPassword), "{algorithm:?} {password:?}");
                // (R2-R4 files drop the characters PDFDocEncoding lacks when the password is set, and the reader does not
                // drop them again, so a password with such characters can never be typed in for them: pdfcraft's limit.)
                let lossy = algorithm != Algorithm::Aes256 && !password.is_ascii() && password.chars().any(|c| c as u32 > 0xff);
                if lossy {
                    let kept: String = password.chars().filter(|c| (*c as u32) <= 0xff).collect();
                    assert!(open_bytes(bytes.clone(), Some(&kept)).is_ok(), "{algorithm:?} {password:?} dropped form");
                }
                if !lossy {
                    assert!(open_bytes(bytes.clone(), Some(password)).is_ok(), "{algorithm:?} {password:?} right");
                }
                let trimmed = password.trim();
                if trimmed != password {
                    assert_eq!(open_bytes(bytes, Some(trimmed)).err(), Some(OpenFailure::WrongPassword), "{algorithm:?} {password:?} trimmed");
                }
            }
        }
    }

    #[test]
    fn a_very_long_password_works() {
        let long = "x".repeat(500);
        for algorithm in ALGORITHMS {
            let bytes = encrypted(algorithm, &long, "owner");
            assert_eq!(open_bytes(bytes.clone(), None).err(), Some(OpenFailure::NeedsPassword), "{algorithm:?}");
            assert!(open_bytes(bytes.clone(), Some(&long)).is_ok(), "{algorithm:?} long right");
            assert_eq!(open_bytes(bytes, Some("y")).err(), Some(OpenFailure::WrongPassword), "{algorithm:?}");
        }
    }

    #[test]
    fn garbage_and_missing_files_are_other_with_the_path_first() {
        for bytes in [b"not a pdf".to_vec(), Vec::new(), b"%PDF-1.4\ngarbage".to_vec()] {
            match open_bytes(bytes, None) {
                Err(OpenFailure::Other(m)) => assert!(m.starts_with("/x/locked.pdf: "), "{m}"),
                other => panic!("{:?}", other.err()),
            }
        }
        let config = Config::load_str("").unwrap();
        match Doc::open(Path::new("/nonexistent/zz.pdf"), None, config.active_theme(), 8.0) {
            Err(OpenFailure::Other(m)) => assert!(m.starts_with("/nonexistent/zz.pdf: "), "{m}"),
            other => panic!("{:?}", other.err()),
        }
        assert_eq!(OpenFailure::NeedsPassword.to_string(), "the document needs a password");
    }
}
