//! Writing annotations into a new copy of the PDF. The document open in the viewer is never
//! touched: this is how filled-in forms and signatures leave the viewer.
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdfcraft_annot::{FillMark, Markup, Meta, NewAnnotation, NoteIcon, Shape, Style, add_annotation};
use pdfcraft_cos::{Document, SaveOptions, write_incremental};
use pdfcraft_render::PageInfo;

use crate::store::{Annotation, MarkKind, NOTE_SIDE, mark_rect};

const AUTHOR: &str = "yavcpdf";
const BLACK: [f64; 3] = [0.0; 3];

impl MarkKind {
    pub fn fill_mark(self) -> FillMark {
        match self {
            Self::Check => FillMark::Check,
            Self::Cross => FillMark::Cross,
            Self::Dot => FillMark::Dot,
            Self::Line => FillMark::Line,
        }
    }
}

fn user_point(page: &PageInfo, p: [f32; 2]) -> [f64; 2] {
    let [x, y] = page.view_to_user(p[0], p[1]);
    [f64::from(x), f64::from(y)]
}

fn user_rect(page: &PageInfo, r: [f32; 4]) -> [f64; 4] {
    let (a, b) = (user_point(page, [r[0], r[1]]), user_point(page, [r[2], r[3]]));
    [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])]
}

/// The corners of `r`, as displayed: top-left, top-right, bottom-left, bottom-right.
fn user_quad(page: &PageInfo, r: [f32; 4]) -> [f64; 8] {
    let corners = [[r[0], r[1]], [r[2], r[1]], [r[0], r[3]], [r[2], r[3]]].map(|p| user_point(page, p));
    let [a, b, c, d] = corners;
    [a[0], a[1], b[0], b[1], c[0], c[1], d[0], d[1]]
}

/// Strokes in user space. A dot needs two points to be a stroke at all.
fn user_strokes(page: &PageInfo, strokes: &[&[[f32; 2]]]) -> Vec<Vec<[f64; 2]>> {
    let stroke = |s: &&[[f32; 2]]| {
        let mut points: Vec<[f64; 2]> = s.iter().map(|p| user_point(page, *p)).collect();
        if let [only] = points[..] {
            points.push(only);
        }
        points
    };
    strokes.iter().filter(|s| !s.is_empty()).map(stroke).collect()
}

fn rgb(color: [u8; 3]) -> [f64; 3] {
    color.map(|c| f64::from(c) / 255.0)
}

/// The box that holds typed text: wider than any font needs, as text wraps at its edge.
fn text_box(at: [f32; 2], size: f32, text: &str) -> [f32; 4] {
    let (pad, width) = (2.0, text.chars().count() as f32 * size * 0.75 + 8.0);
    [at[0] - pad, at[1] - pad, at[0] - pad + width, at[1] + size * 1.2 + 2.0 * pad]
}

/// What an annotation is in the PDF: its shape, colour, line width and text.
fn pdf_form(annotation: &Annotation, page: &PageInfo) -> (Shape, Option<[f64; 3]>, Option<f64>, String) {
    match annotation {
        Annotation::Highlight { rects, color, text, .. } => {
            let quads = rects.iter().map(|r| user_quad(page, *r)).collect();
            (Shape::TextMarkup { kind: Markup::Highlight, quads }, Some(rgb(*color)), None, text.clone())
        }
        Annotation::Stroke { points, color, width, .. } => {
            let strokes = user_strokes(page, &[points.as_slice()]);
            (Shape::Ink { strokes }, Some(rgb(*color)), Some(f64::from(*width)), String::new())
        }
        Annotation::Note { at, color, text, .. } => {
            // The icon's top-left corner as displayed, wherever a turned page puts it.
            let r = user_rect(page, [at[0], at[1], at[0] + NOTE_SIDE, at[1] + NOTE_SIDE]);
            (Shape::Note { at: [r[0], r[3]], icon: NoteIcon::Comment }, Some(rgb(*color)), None, text.clone())
        }
        Annotation::Text { at, size, text, .. } => {
            let rect = user_rect(page, text_box(*at, *size, text));
            (Shape::Typewriter { rect, font_size: f64::from(*size) }, Some(BLACK), Some(0.0), text.clone())
        }
        Annotation::Mark { at, size, mark, .. } => {
            let rect = user_rect(page, mark_rect(*mark, *at, *size));
            (Shape::Mark { rect, mark: mark.fill_mark() }, Some(BLACK), None, String::new())
        }
        Annotation::Signature { strokes, width, .. } => {
            let parts: Vec<&[[f32; 2]]> = strokes.iter().map(Vec::as_slice).collect();
            (Shape::Signature { strokes: user_strokes(page, &parts) }, Some(BLACK), Some(f64::from(*width)), String::new())
        }
    }
}

fn add_all(doc: &mut Document, pages: &[PageInfo], annotations: &[Annotation]) -> Result<(), String> {
    let date = chrono::Local::now().format("D:%Y%m%d%H%M%S").to_string();
    for annotation in annotations {
        let Some(page) = pages.get(annotation.page()) else { continue };
        let (shape, color, width, contents) = pdf_form(annotation, page);
        let mut style = Style::default_for(&shape);
        style.color = color.unwrap_or(style.color);
        style.width = width.unwrap_or(style.width);
        let new = NewAnnotation { page: annotation.page(), shape, style, contents, author: AUTHOR.to_owned() };
        let meta = Meta { date: Some(date.clone()), id: format!("yavcpdf-{}", annotation.id()) };
        add_annotation(doc, &new, &meta).map_err(|e| format!("page {}: {e:?}", annotation.page() + 1))?;
    }
    Ok(())
}

/// The bytes of the document with the annotations in it.
fn with_annotations(bytes: Arc<Vec<u8>>, password: Option<&str>, pages: &[PageInfo], annotations: &[Annotation]) -> Result<Vec<u8>, String> {
    let mut doc = Document::open_with_password(bytes, password).map_err(|e| format!("{e:?}"))?;
    add_all(&mut doc, pages, annotations)?;
    write_incremental(&doc, &SaveOptions::default()).map_err(|e| format!("{e:?}"))
}

/// The copy's name when none is given: beside the document, with `-annotated` added.
pub fn default_path(document: &Path) -> PathBuf {
    let stem = document.file_stem().map_or_else(|| "document".to_owned(), |s| s.to_string_lossy().into_owned());
    document.with_file_name(format!("{stem}-annotated.pdf"))
}

/// Why `path` can't be written, if it can't.
pub fn refusal(document: &Path, path: &Path, force: bool) -> Option<String> {
    let same = |a: &Path, b: &Path| matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b);
    if same(document, path) {
        return Some("export writes a copy, not over the document itself".to_owned());
    }
    (path.exists() && !force).then(|| format!("{} exists: use :export! to replace it", path.display()))
}

/// Writes the document with `annotations` to `path`, whole or not at all.
pub fn export(bytes: Arc<Vec<u8>>, password: Option<&str>, pages: &[PageInfo], annotations: &[Annotation], path: &Path) -> Result<(), String> {
    // The document came from a stranger, and a panic here must not cost the user their work.
    let written = catch_unwind(AssertUnwindSafe(|| with_annotations(bytes, password, pages, annotations)));
    let written = written.map_err(|_| "the document could not be written".to_owned())??;
    let temporary = path.with_extension("pdf.tmp");
    let fail = |e: std::io::Error| format!("{}: {e}", path.display());
    std::fs::write(&temporary, written).map_err(fail)?;
    std::fs::rename(&temporary, path).map_err(fail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn page(rotation: u16) -> PageInfo {
        let (width, height) = if rotation % 180 == 90 { (800.0, 600.0) } else { (600.0, 800.0) };
        PageInfo { width, height, label: "1".to_owned(), crop: [0.0, 0.0, 600.0, 800.0], rotation }
    }

    fn sample() -> Vec<Annotation> {
        vec![
            Annotation::Highlight { id: 1, page: 0, rects: vec![[72.0, 90.0, 200.0, 104.0]], color: [250, 189, 47], text: "x".to_owned() },
            Annotation::Stroke { id: 2, page: 0, points: vec![[72.0, 200.0], [150.0, 210.0]], color: [1, 2, 3], width: 2.0 },
            Annotation::Stroke { id: 3, page: 0, points: vec![[72.0, 220.0]], color: [1, 2, 3], width: 2.0 },
            Annotation::Note { id: 4, page: 0, at: [300.0, 300.0], color: [250, 189, 47], text: "note".to_owned() },
            Annotation::Text { id: 5, page: 0, at: [72.0, 400.0], size: 12.0, text: "Ada Lovelace".to_owned() },
            Annotation::Mark { id: 6, page: 0, at: [72.0, 440.0], size: 12.0, mark: MarkKind::Check },
            Annotation::Mark { id: 7, page: 0, at: [100.0, 440.0], size: 12.0, mark: MarkKind::Dot },
            Annotation::Signature { id: 8, page: 0, strokes: vec![vec![[72.0, 500.0], [200.0, 480.0]], vec![[90.0, 490.0]]], width: 1.2 },
        ]
    }

    /// Positions go through `f32`, which leaves noise far below anything visible.
    fn close(a: &[f64], b: &[f64]) {
        assert!(a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3), "{a:?} {b:?}");
    }

    /// The smallest PDF with one page, built by hand.
    fn tiny_pdf() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << >> >>",
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes());
        out
    }

    #[test]
    fn every_kind_of_annotation_goes_into_the_copy() {
        let bytes = Arc::new(tiny_pdf());
        let annotations = sample();
        let written = with_annotations(bytes.clone(), None, &[page(0)], &annotations).unwrap();
        assert!(written.starts_with(&bytes), "an incremental save keeps the original bytes");
        let copy = Document::open(Arc::new(written)).unwrap();
        assert_eq!(pdfcraft_annot::summaries(&copy).len(), annotations.len());
    }

    #[test]
    fn an_annotation_on_a_page_that_is_gone_is_left_out() {
        let lost = Annotation::Mark { id: 1, page: 7, at: [1.0, 1.0], size: 12.0, mark: MarkKind::Dot };
        assert!(with_annotations(Arc::new(tiny_pdf()), None, &[page(0)], &[lost]).is_ok());
    }

    #[test]
    fn a_damaged_document_is_an_error() {
        assert!(with_annotations(Arc::new(b"not a pdf".to_vec()), None, &[page(0)], &sample()).is_err());
    }

    #[test]
    fn view_positions_become_user_space() {
        assert_eq!(user_point(&page(0), [100.0, 0.0]), [100.0, 800.0]);
        close(&user_rect(&page(0), [10.0, 20.0, 30.0, 40.0]), &[10.0, 760.0, 30.0, 780.0]);
        let [x, y] = user_point(&page(90), [0.0, 0.0]);
        assert!(x.abs() < 1e-3 && y.abs() < 1e-3, "a page turned clockwise has its origin at the top left: {x} {y}");
    }

    #[test]
    fn quads_keep_the_order_of_the_corners_as_displayed() {
        close(&user_quad(&page(0), [10.0, 20.0, 30.0, 40.0]), &[10.0, 780.0, 30.0, 780.0, 10.0, 760.0, 30.0, 760.0]);
    }

    #[test]
    fn a_dot_becomes_a_stroke_of_two_points() {
        let strokes = user_strokes(&page(0), &[&[[1.0, 2.0]], &[]]);
        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].len(), 2);
    }

    #[test]
    fn the_copy_goes_beside_the_document_and_never_over_it() {
        assert_eq!(default_path(Path::new("/a/form.pdf")), PathBuf::from("/a/form-annotated.pdf"));
        let dir = std::env::temp_dir().join(format!("yavcpdf-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (original, copy, free) = (dir.join("a.pdf"), dir.join("b.pdf"), dir.join("c.pdf"));
        std::fs::write(&original, b"a").unwrap();
        std::fs::write(&copy, b"b").unwrap();
        assert!(refusal(&original, &original, true).is_some(), "not even when forced");
        assert!(refusal(&original, &copy, false).is_some());
        assert!(refusal(&original, &copy, true).is_none());
        assert!(refusal(&original, &free, false).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
