//! What is kept per document, outside the PDF: position, marks, portals and annotations.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::user_dir;
use crate::form::{FormValue, Hidden};
use crate::view::Anchor;

/// A link from one place in the document to another, shown beside the source.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Portal {
    pub id: u64,
    pub source: Anchor,
    pub target: Anchor,
}

/// Side of a sticky note's icon, in page points.
pub const NOTE_SIDE: f32 = 20.0;

/// Pen strokes.
pub type Strokes = Vec<Vec<[f32; 2]>>;

/// A fill & sign mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkKind {
    Check,
    Cross,
    Dot,
    Line,
}

/// The box of a mark `size` points tall, centred on `at`. A line is long and thin.
pub fn mark_rect(mark: MarkKind, at: [f32; 2], size: f32) -> [f32; 4] {
    let (w, h) = if mark == MarkKind::Line { (size * 3.0, size / 3.0) } else { (size, size) };
    [at[0] - w / 2.0, at[1] - h / 2.0, at[0] + w / 2.0, at[1] + h / 2.0]
}

/// A mark made on a page, in page points. What fill & sign adds has no colour of its own: it is
/// shown in the page's text colour and exported black, as it would be written on paper.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Annotation {
    Highlight {
        id: u64,
        page: usize,
        rects: Vec<[f32; 4]>,
        color: [u8; 3],
        text: String,
    },
    Stroke {
        id: u64,
        page: usize,
        points: Vec<[f32; 2]>,
        color: [u8; 3],
        width: f32,
    },
    /// A sticky note, `at` being the top-left corner of its icon.
    Note {
        id: u64,
        page: usize,
        at: [f32; 2],
        color: [u8; 3],
        text: String,
    },
    /// Text typed onto the page, `at` being its top-left corner.
    Text {
        id: u64,
        page: usize,
        at: [f32; 2],
        size: f32,
        text: String,
    },
    /// A mark centred on `at`.
    Mark {
        id: u64,
        page: usize,
        at: [f32; 2],
        size: f32,
        mark: MarkKind,
    },
    Signature {
        id: u64,
        page: usize,
        strokes: Strokes,
        width: f32,
    },
}

impl Annotation {
    pub fn id(&self) -> u64 {
        match self {
            Self::Highlight { id, .. }
            | Self::Stroke { id, .. }
            | Self::Note { id, .. }
            | Self::Text { id, .. }
            | Self::Mark { id, .. }
            | Self::Signature { id, .. } => *id,
        }
    }

    pub fn page(&self) -> usize {
        match self {
            Self::Highlight { page, .. }
            | Self::Stroke { page, .. }
            | Self::Note { page, .. }
            | Self::Text { page, .. }
            | Self::Mark { page, .. }
            | Self::Signature { page, .. } => *page,
        }
    }

    /// The box around it, in page points.
    pub fn bounds(&self) -> [f32; 4] {
        let start = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
        match self {
            Self::Highlight { rects, .. } => rects.iter().copied().fold(start, crate::hint::union),
            Self::Stroke { points, .. } => points.iter().map(|p| [p[0], p[1], p[0], p[1]]).fold(start, crate::hint::union),
            Self::Note { at, .. } => [at[0], at[1], at[0] + NOTE_SIDE, at[1] + NOTE_SIDE],
            // Near enough for picking it: what it is drawn with is up to the interface font.
            Self::Text { at, size, text, .. } => [at[0], at[1], at[0] + text.chars().count() as f32 * size * 0.55, at[1] + size * 1.2],
            Self::Mark { at, size, mark, .. } => mark_rect(*mark, *at, *size),
            Self::Signature { strokes, .. } => strokes.iter().flatten().map(|p| [p[0], p[1], p[0], p[1]]).fold(start, crate::hint::union),
        }
    }

    /// Whether the point (page points) touches it, within `reach`.
    pub fn hit(&self, at: [f32; 2], reach: f32) -> bool {
        match self {
            Self::Highlight { rects, .. } => {
                rects.iter().any(|r| (r[0] - reach..=r[2] + reach).contains(&at[0]) && (r[1] - reach..=r[3] + reach).contains(&at[1]))
            }
            Self::Stroke { points, width, .. } => {
                let reach = reach + width / 2.0;
                points.windows(2).any(|s| distance_to_segment(at, s[0], s[1]) <= reach)
                    || points.first().is_some_and(|p| distance_to_segment(at, *p, *p) <= reach)
            }
            Self::Note { .. } | Self::Text { .. } | Self::Mark { .. } | Self::Signature { .. } => {
                let b = self.bounds();
                (b[0] - reach..=b[2] + reach).contains(&at[0]) && (b[1] - reach..=b[3] + reach).contains(&at[1])
            }
        }
    }
}

fn distance_to_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length).clamp(0.0, 1.0) } else { 0.0 };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
}

/// The sidecar file's contents.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sidecar {
    pub path: String,
    pub anchor: Option<Anchor>,
    pub zoom: Option<f32>,
    pub marks: BTreeMap<char, Anchor>,
    pub portals: Vec<Portal>,
    pub annotations: Vec<Annotation>,
    /// What was filled into the form, by the fully qualified name of the field. Only the fields
    /// that were changed are here.
    pub form: BTreeMap<String, FormValue>,
    /// Widgets the form's show and hide buttons changed, by the fully qualified name of their
    /// field, then their place in it: `true` is hidden. Only the widgets that differ from the
    /// document are here.
    pub hidden: Hidden,
    pub next_id: u64,
}

impl Sidecar {
    pub fn take_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}

/// Scales strokes to a width of 1 with their corner at the origin, for placing at any size.
pub fn normalize_signature(strokes: &[Vec<[f32; 2]>]) -> Option<Strokes> {
    let points = || strokes.iter().flatten();
    let (left, right) = points().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), p| (a.min(p[0]), b.max(p[0])));
    let top = points().fold(f32::INFINITY, |a, p| a.min(p[1]));
    if !left.is_finite() {
        return None;
    }
    let width = (right - left).max(1e-3);
    let scaled = |p: &[f32; 2]| [(p[0] - left) / width, (p[1] - top) / width];
    Some(strokes.iter().filter(|s| !s.is_empty()).map(|s| s.iter().map(scaled).collect()).collect())
}

/// A saved signature, `width` points wide, with its left edge at `at` and its middle level with it.
pub fn place_signature(strokes: &[Vec<[f32; 2]>], at: [f32; 2], width: f32) -> Strokes {
    let (top, bottom) = strokes.iter().flatten().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), p| (a.min(p[1]), b.max(p[1])));
    let middle = (top + bottom) / 2.0;
    strokes.iter().map(|s| s.iter().map(|p| [at[0] + p[0] * width, at[1] + (p[1] - middle) * width]).collect()).collect()
}

fn signature_file(dir: &Path) -> PathBuf {
    dir.join("signature.json")
}

pub fn load_signature(dir: &Path) -> Option<Strokes> {
    serde_json::from_slice(&std::fs::read(signature_file(dir)).ok()?).ok()
}

/// Keeps the signature for every document, readable by this user alone.
pub fn save_signature(dir: &Path, strokes: &[Vec<[f32; 2]>]) -> Result<(), String> {
    use std::io::Write;
    let fail = |e: std::io::Error| e.to_string();
    std::fs::create_dir_all(dir).map_err(fail)?;
    let bytes = serde_json::to_vec(strokes).map_err(|e| e.to_string())?;
    let file = signature_file(dir);
    let temporary = file.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(&temporary).and_then(|mut f| f.write_all(&bytes)).map_err(fail)?;
    std::fs::rename(&temporary, &file).map_err(fail)
}

/// `$XDG_DATA_HOME/yavcpdf`, or the platform's own place without it.
pub fn data_dir() -> PathBuf {
    user_dir("XDG_DATA_HOME", ".local/share")
}

pub fn content_key(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// A document's sidecar on disk.
#[derive(Clone, Debug)]
pub struct Store {
    dir: PathBuf,
    key: String,
}

impl Store {
    pub fn new(dir: PathBuf, key: String) -> Self {
        Self { dir, key }
    }

    /// Whether a sidecar is saved under this key.
    pub fn exists(&self) -> bool {
        self.file().exists()
    }

    fn file(&self) -> PathBuf {
        self.dir.join(format!("{}.json", self.key))
    }

    /// Reads the document's sidecar. A file changed since it was last opened has a new key, so
    /// the sidecar that names its path is taken over; a moved file keeps its key. A sidecar that
    /// can't be read is set aside, never overwritten.
    pub fn load(&self, path: &Path) -> (Sidecar, Option<String>) {
        let mut warning = None;
        match read_sidecar(&self.file()) {
            Ok(Some(sidecar)) => return (Sidecar { path: path.display().to_string(), ..sidecar }, None),
            Ok(None) => {}
            Err(e) => {
                let aside = self.file().with_extension("json.bad");
                let _ = std::fs::rename(self.file(), &aside);
                warning = Some(format!("couldn't read saved data ({e}); kept it as {}", aside.display()));
            }
        }
        let sidecar = self.adopt(path).unwrap_or_else(|| Sidecar { path: path.display().to_string(), ..Default::default() });
        (sidecar, warning)
    }

    /// Takes over the sidecar of an earlier version of the file at `path`.
    fn adopt(&self, path: &Path) -> Option<Sidecar> {
        let wanted = path.display().to_string();
        let entries = std::fs::read_dir(&self.dir).ok()?;
        for entry in entries.flatten() {
            let file = entry.path();
            if file.extension().is_none_or(|e| e != "json") || file == self.file() {
                continue;
            }
            if let Ok(Some(sidecar)) = read_sidecar(&file).map(|s| s.filter(|s| s.path == wanted)) {
                let _ = std::fs::remove_file(&file);
                return Some(sidecar);
            }
        }
        None
    }

    /// Writes the sidecar through a temporary file, so a crash can't leave half of one.
    pub fn save(&self, sidecar: &Sidecar) -> Result<(), String> {
        // What was typed into a form is personal: for the user alone.
        let mut dir = std::fs::DirBuilder::new();
        dir.recursive(true);
        #[cfg(unix)]
        if !self.dir.exists() {
            std::os::unix::fs::DirBuilderExt::mode(&mut dir, 0o700);
        }
        dir.create(&self.dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_string_pretty(sidecar).map_err(|e| e.to_string())?;
        let temporary = self.file().with_extension("json.tmp");
        // One left by a crash keeps its mode if it is written in place: start from none.
        let _ = std::fs::remove_file(&temporary);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options
            .open(&temporary)
            .and_then(|mut f| {
                std::io::Write::write_all(&mut f, text.as_bytes())?;
                f.sync_all()
            })
            .map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, self.file()).map_err(|e| e.to_string())
    }

    /// The same store under another key.
    pub fn rekeyed(&self, key: String) -> Self {
        Self { dir: self.dir.clone(), key }
    }

    /// Moves the sidecar to the key of the file's new contents. Contents that came back the same
    /// keep their key, and so must keep their file.
    pub fn moved_to(self, key: String, sidecar: &Sidecar) -> Result<Self, String> {
        if key == self.key {
            return Ok(self);
        }
        let moved = self.rekeyed(key);
        moved.save(sidecar)?;
        let _ = std::fs::remove_file(self.file());
        Ok(moved)
    }
}

fn read_sidecar(file: &Path) -> Result<Option<Sidecar>, String> {
    match std::fs::read_to_string(file) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// The document opened last, for starting with no file named.
pub fn last_document() -> Option<PathBuf> {
    let text = std::fs::read_to_string(data_dir().join("last")).ok()?;
    let path = PathBuf::from(text.trim());
    path.exists().then_some(path)
}

pub fn remember_document(path: &Path) {
    let dir = data_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join("last"), path.display().to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory removed at the end of the test.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("yavcpdf-test-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn highlight(id: u64) -> Annotation {
        Annotation::Highlight { id, page: 2, rects: vec![[10.0, 10.0, 50.0, 20.0]], color: [1, 2, 3], text: "words".to_owned() }
    }

    #[test]
    fn a_sidecar_round_trips() {
        let scratch = Scratch::new("round-trip");
        let store = Store::new(scratch.0.clone(), "key".to_owned());
        let anchor = Anchor { page: 1, y: 2.0, x: 3.0 };
        let mut sidecar = Sidecar { anchor: Some(anchor), zoom: Some(1.5), ..Default::default() };
        sidecar.marks.insert('a', anchor);
        sidecar.portals.push(Portal { id: 1, source: anchor, target: anchor });
        sidecar.annotations.push(highlight(2));
        sidecar.annotations.push(Annotation::Stroke { id: 3, page: 0, points: vec![[1.0, 1.0]], color: [0, 0, 0], width: 2.0 });
        store.save(&sidecar).unwrap();
        let (loaded, warning) = store.load(Path::new("/doc.pdf"));
        assert!(warning.is_none());
        assert_eq!(loaded, Sidecar { path: "/doc.pdf".to_owned(), ..sidecar });
    }

    #[test]
    fn a_new_document_starts_empty() {
        let scratch = Scratch::new("new");
        let (sidecar, warning) = Store::new(scratch.0.clone(), "key".to_owned()).load(Path::new("/doc.pdf"));
        assert!(warning.is_none());
        assert_eq!(sidecar.path, "/doc.pdf");
        assert!(sidecar.annotations.is_empty());
    }

    #[test]
    fn an_edited_file_takes_over_the_sidecar_of_its_path() {
        let scratch = Scratch::new("adopt");
        let old = Store::new(scratch.0.clone(), "old".to_owned());
        old.save(&Sidecar { path: "/doc.pdf".to_owned(), annotations: vec![highlight(1)], ..Default::default() }).unwrap();
        let new = Store::new(scratch.0.clone(), "new".to_owned());
        let (sidecar, _) = new.load(Path::new("/doc.pdf"));
        assert_eq!(sidecar.annotations.len(), 1);
        assert!(!scratch.0.join("old.json").exists());
    }

    #[test]
    fn a_reload_with_new_contents_moves_the_sidecar() {
        let scratch = Scratch::new("moved");
        let store = Store::new(scratch.0.clone(), "old".to_owned());
        let sidecar = Sidecar { annotations: vec![highlight(1)], ..Default::default() };
        store.save(&sidecar).unwrap();
        let moved = store.moved_to("new".to_owned(), &sidecar).unwrap();
        assert!(!scratch.0.join("old.json").exists());
        assert_eq!(moved.load(Path::new("/doc.pdf")).0.annotations.len(), 1);
    }

    #[test]
    fn a_reload_with_the_same_contents_keeps_the_sidecar() {
        let scratch = Scratch::new("same");
        let store = Store::new(scratch.0.clone(), "key".to_owned());
        let sidecar = Sidecar { annotations: vec![highlight(1)], ..Default::default() };
        store.save(&sidecar).unwrap();
        let kept = store.moved_to("key".to_owned(), &sidecar).unwrap();
        assert_eq!(kept.load(Path::new("/doc.pdf")).0.annotations.len(), 1);
    }

    #[test]
    fn another_files_sidecar_is_left_alone() {
        let scratch = Scratch::new("other");
        let other = Store::new(scratch.0.clone(), "other".to_owned());
        other.save(&Sidecar { path: "/other.pdf".to_owned(), annotations: vec![highlight(1)], ..Default::default() }).unwrap();
        let (sidecar, _) = Store::new(scratch.0.clone(), "new".to_owned()).load(Path::new("/doc.pdf"));
        assert!(sidecar.annotations.is_empty());
        assert!(scratch.0.join("other.json").exists());
    }

    #[test]
    fn a_damaged_sidecar_is_set_aside_with_a_warning() {
        let scratch = Scratch::new("damaged");
        let store = Store::new(scratch.0.clone(), "key".to_owned());
        std::fs::write(scratch.0.join("key.json"), "{ not json").unwrap();
        let (sidecar, warning) = store.load(Path::new("/doc.pdf"));
        assert!(warning.is_some());
        assert!(sidecar.annotations.is_empty());
        assert_eq!(std::fs::read_to_string(scratch.0.join("key.json.bad")).unwrap(), "{ not json");
    }

    #[test]
    fn every_kind_of_annotation_round_trips() {
        let all = [
            Annotation::Note { id: 1, page: 0, at: [1.0, 2.0], color: [1, 2, 3], text: "n".to_owned() },
            Annotation::Text { id: 2, page: 0, at: [1.0, 2.0], size: 12.0, text: "t".to_owned() },
            Annotation::Mark { id: 3, page: 0, at: [1.0, 2.0], size: 12.0, mark: MarkKind::Dot },
            Annotation::Signature { id: 4, page: 0, strokes: vec![vec![[1.0, 2.0]]], width: 1.0 },
        ];
        for annotation in all {
            let text = serde_json::to_string(&annotation).unwrap();
            assert_eq!(serde_json::from_str::<Annotation>(&text).unwrap(), annotation);
        }
    }

    #[test]
    fn notes_and_marks_are_hit_inside_their_box() {
        let note = Annotation::Note { id: 1, page: 0, at: [10.0, 10.0], color: [0; 3], text: String::new() };
        assert!(note.hit([20.0, 20.0], 0.0) && !note.hit([40.0, 20.0], 0.0));
        let line = Annotation::Mark { id: 1, page: 0, at: [50.0, 50.0], size: 12.0, mark: MarkKind::Line };
        assert_eq!(line.bounds(), [32.0, 48.0, 68.0, 52.0]);
    }

    #[test]
    fn a_signature_is_scaled_and_placed() {
        let drawn = vec![vec![[10.0, 20.0], [30.0, 30.0]], vec![], vec![[20.0, 25.0]]];
        let normal = normalize_signature(&drawn).unwrap();
        assert_eq!(normal, [vec![[0.0, 0.0], [1.0, 0.5]], vec![[0.5, 0.25]]]);
        let placed = place_signature(&normal, [100.0, 200.0], 150.0);
        assert_eq!(placed[0], [[100.0, 162.5], [250.0, 237.5]]);
    }

    #[test]
    fn nothing_drawn_is_no_signature() {
        assert_eq!(normalize_signature(&[]), None);
        assert_eq!(normalize_signature(&[vec![]]), None);
    }

    #[test]
    fn a_dot_alone_can_be_a_signature_without_dividing_by_nothing() {
        let normal = normalize_signature(&[vec![[5.0, 5.0]]]).unwrap();
        assert_eq!(normal, [vec![[0.0, 0.0]]]);
    }

    #[test]
    fn the_signature_is_kept_for_this_user_alone() {
        let dir = Scratch::new("signature");
        assert_eq!(load_signature(&dir.0), None);
        let strokes = vec![vec![[0.0, 0.0], [1.0, 0.5]]];
        save_signature(&dir.0, &strokes).unwrap();
        assert_eq!(load_signature(&dir.0), Some(strokes));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(signature_file(&dir.0)).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0);
        }
    }

    #[test]
    fn hits_follow_the_shape() {
        assert!(highlight(1).hit([30.0, 15.0], 0.0));
        assert!(!highlight(1).hit([30.0, 25.0], 0.0));
        assert!(highlight(1).hit([30.0, 25.0], 6.0));
        let line = Annotation::Stroke { id: 1, page: 0, points: vec![[0.0, 0.0], [100.0, 0.0]], color: [0, 0, 0], width: 2.0 };
        assert!(line.hit([50.0, 2.0], 2.0));
        assert!(!line.hit([50.0, 9.0], 2.0));
        let dot = Annotation::Stroke { id: 1, page: 0, points: vec![[5.0, 5.0]], color: [0, 0, 0], width: 2.0 };
        assert!(dot.hit([5.0, 6.0], 0.0));
    }

    #[test]
    fn bounds_cover_the_shape() {
        let line = Annotation::Stroke { id: 1, page: 0, points: vec![[1.0, 9.0], [5.0, 3.0]], color: [0, 0, 0], width: 2.0 };
        assert_eq!(line.bounds(), [1.0, 3.0, 5.0, 9.0]);
    }

    #[test]
    fn the_key_depends_on_the_contents() {
        assert_eq!(content_key(b"a"), content_key(b"a"));
        assert_ne!(content_key(b"a"), content_key(b"b"));
        assert_eq!(content_key(b"a").len(), 32);
    }

    #[test]
    fn the_key_is_the_truncated_sha256() {
        assert_eq!(content_key(b"abc"), "ba7816bf8f01cfea414140de5dae2223");
        assert_eq!(content_key(b""), "e3b0c44298fc1c149afbf4c8996fb924");
    }

    #[test]
    fn an_old_sidecar_without_form_loads() {
        let old = r#"{"path":"/a.pdf","anchor":null,"zoom":1.5,"marks":{},"portals":[],"annotations":[],"next_id":3}"#;
        let side: Sidecar = serde_json::from_str(old).unwrap();
        assert!(side.form.is_empty());
        assert_eq!(side.next_id, 3);
        let minimal: Sidecar = serde_json::from_str("{}").unwrap();
        assert!(minimal.form.is_empty());
    }

    #[test]
    fn form_values_survive_save_and_load() {
        let dir = Scratch::new("vform");
        let store = Store::new(dir.0.clone(), "k".to_owned());
        let mut side = Sidecar { path: "/a.pdf".into(), ..Default::default() };
        side.form.insert("a.b".into(), FormValue::Text("é\"x".into()));
        side.form.insert("c".into(), FormValue::Check(true));
        side.form.insert("r".into(), FormValue::Radio(Some("On".into())));
        side.form.insert("l".into(), FormValue::Choice(vec!["1".into(), "2".into()]));
        store.save(&side).unwrap();
        let (back, warning) = store.load(Path::new("/a.pdf"));
        assert!(warning.is_none());
        assert_eq!(back.form, side.form);
    }

    #[test]
    fn an_unknown_form_value_variant_is_set_aside_not_lost() {
        let dir = Scratch::new("vbad");
        let store = Store::new(dir.0.clone(), "k".to_owned());
        std::fs::write(dir.0.join("k.json"), r#"{"path":"/a.pdf","form":{"x":{"Future":1}}}"#).unwrap();
        let (side, warning) = store.load(Path::new("/a.pdf"));
        assert!(warning.is_some());
        assert!(side.form.is_empty());
        assert!(dir.0.join("k.json.bad").exists(), "the unreadable file is kept");
    }

    #[cfg(unix)]
    #[test]
    fn a_sidecar_is_for_its_user_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = Scratch::new("mode");
        let store = Store::new(dir.0.clone(), "k".to_owned());
        let side = Sidecar { path: "/a.pdf".into(), ..Default::default() };
        store.save(&side).unwrap();
        store.save(&side).unwrap();
        assert_eq!(std::fs::metadata(dir.0.join("k.json")).unwrap().permissions().mode() & 0o077, 0);
    }

    #[cfg(unix)]
    #[test]
    fn save_is_0600_atomic_and_tightens_an_old_sidecar() {
        use std::os::unix::fs::PermissionsExt;
        let dir = Scratch::new("mode2");
        let store = Store::new(dir.0.join("sub"), "k".to_owned());
        let side = Sidecar { path: "/a.pdf".into(), ..Default::default() };
        store.save(&side).unwrap();
        let file = dir.0.join("sub").join("k.json");
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        let names: Vec<_> = std::fs::read_dir(dir.0.join("sub")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        assert_eq!(names, ["k.json"], "no temporary left");
        // An older, world-readable sidecar.
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o644);
        store.save(&side).unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::read_dir(dir.0.join("sub")).unwrap().count(), 1);
        assert!(store.load(Path::new("/a.pdf")).0.path == "/a.pdf");
    }

    #[cfg(unix)]
    #[test]
    fn save_over_a_stale_world_readable_temporary_still_ends_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = Scratch::new("mode3");
        let store = Store::new(dir.0.clone(), "k".to_owned());
        let side = Sidecar { path: "/a.pdf".into(), ..Default::default() };
        // A crash earlier left the temporary behind with a loose mode.
        let tmp = dir.0.join("k.json.tmp");
        std::fs::write(&tmp, "junk").unwrap();
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644)).unwrap();
        store.save(&side).unwrap();
        assert_eq!(std::fs::metadata(dir.0.join("k.json")).unwrap().permissions().mode() & 0o777, 0o600);
        assert!(!tmp.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_save_leaves_the_old_sidecar_whole() {
        let dir = Scratch::new("mode4");
        let store = Store::new(dir.0.clone(), "k".to_owned());
        let side = Sidecar { path: "/a.pdf".into(), ..Default::default() };
        store.save(&side).unwrap();
        let before = std::fs::read(dir.0.join("k.json")).unwrap();
        // The temporary's name is taken by a directory: the write fails, the sidecar stays.
        std::fs::create_dir(dir.0.join("k.json.tmp")).unwrap();
        assert!(store.save(&side).is_err());
        assert_eq!(std::fs::read(dir.0.join("k.json")).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_at_the_temporary_path_keeps_the_old_sidecar_and_errors() {
        let dir = Scratch::new("dirtmp");
        let store = Store::new(dir.0.clone(), "k".to_owned());
        let old = Sidecar { path: "/old.pdf".into(), ..Default::default() };
        store.save(&old).unwrap();
        let before = std::fs::read(dir.0.join("k.json")).unwrap();
        let tmp = dir.0.join("k.json.tmp");
        std::fs::create_dir(&tmp).unwrap();
        std::fs::write(tmp.join("inner"), "x").unwrap();
        let new = Sidecar { path: "/new.pdf".into(), ..Default::default() };
        let e = store.save(&new).unwrap_err();
        assert!(!e.is_empty());
        assert_eq!(std::fs::read(dir.0.join("k.json")).unwrap(), before);
        assert!(tmp.join("inner").exists(), "the directory is not touched");
        assert_eq!(store.load(Path::new("/old.pdf")).0.path, "/old.pdf");
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_data_dir_is_made_0700_and_an_existing_one_is_left_alone() {
        use std::os::unix::fs::PermissionsExt;
        let base = Scratch::new("dirs");
        let fresh = base.0.join("new").join("deeper");
        let store = Store::new(fresh.clone(), "k".to_owned());
        store.save(&Sidecar::default()).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&fresh), 0o700, "the data dir");
        assert_eq!(mode(&fresh.join("k.json")), 0o600);
        eprintln!("parent made by recursive create has mode {:o}", mode(&base.0.join("new")));
        assert_eq!(mode(&base.0.join("new")), 0o700, "intermediate directories too");
        // An existing directory with other modes stays as it is.
        for m in [0o755, 0o750, 0o777] {
            let existing = base.0.join(format!("exists{m:o}"));
            std::fs::create_dir(&existing).unwrap();
            std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(m)).unwrap();
            Store::new(existing.clone(), "k".to_owned()).save(&Sidecar::default()).unwrap();
            assert_eq!(mode(&existing), m);
        }
    }
}
