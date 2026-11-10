//! What is kept per document, outside the PDF: position, marks, bookmarks, jump history, portals and annotations.
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

/// A named place in the document, kept in the sidecar next to the one-letter marks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bookmark {
    pub name: String,
    pub anchor: Anchor,
}

/// The longest bookmark name, in characters.
pub const MAX_BOOKMARK_NAME: usize = 200;

/// A bookmark name as stored: trimmed, tabs and newlines as spaces, runs of spaces collapsed, and
/// cut to `MAX_BOOKMARK_NAME` characters.
pub fn normalise_bookmark_name(name: &str) -> String {
    let joined = name.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.chars().take(MAX_BOOKMARK_NAME).collect::<String>().trim_end().to_owned()
}

/// Puts a bookmark at `anchor` under `name`, which moves one of that name, in any case, instead of adding a second.
/// The stored name is the new spelling.
/// The list stays in the order of the document, so the picker reads like the contents. Returns
/// whether it moved an existing one.
pub fn add_bookmark(list: &mut Vec<Bookmark>, name: &str, anchor: Anchor) -> bool {
    let same = |b: &Bookmark| b.name.to_lowercase() == name.to_lowercase();
    let moved = list.iter().any(same);
    list.retain(|b| !same(b));
    list.push(Bookmark { name: name.to_owned(), anchor });
    list.sort_by(|a, b| a.anchor.page.cmp(&b.anchor.page).then(a.anchor.y.total_cmp(&b.anchor.y)));
    moved
}

/// Removes the bookmark at `index` and returns it.
pub fn remove_bookmark(list: &mut Vec<Bookmark>, index: usize) -> Option<Bookmark> {
    (index < list.len()).then(|| list.remove(index))
}

/// The bookmarks whose names contain `filter`, ignoring case, as indices.
pub fn bookmark_rows(list: &[Bookmark], filter: &str) -> Vec<usize> {
    let filter = filter.to_lowercase();
    list.iter().enumerate().filter(|(_, b)| b.name.to_lowercase().contains(&filter)).map(|(i, _)| i).collect()
}

/// The sidecar file's contents.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sidecar {
    pub path: String,
    pub anchor: Option<Anchor>,
    pub zoom: Option<f32>,
    /// The jump history, oldest first, and where `jump_back` stands in it.
    pub jumps: Vec<Anchor>,
    pub jump_index: usize,
    pub marks: BTreeMap<char, Anchor>,
    /// The named bookmarks, in the order of the document.
    pub bookmarks: Vec<Bookmark>,
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

/// How many places the jump history keeps.
pub const MAX_JUMPS: usize = 100;

/// The newest `MAX_JUMPS` of a jump history, with the index moved to the same place in them.
pub fn capped_jumps(jumps: &[Anchor], index: usize) -> (Vec<Anchor>, usize) {
    let dropped = jumps.len().saturating_sub(MAX_JUMPS);
    (jumps[dropped..].to_vec(), index.min(jumps.len()).saturating_sub(dropped))
}

/// A saved jump history for a document of `pages` pages. The file may have changed since, so
/// places beyond the last page are dropped, not moved: a clamped one would point somewhere the
/// reader never was. The index keeps its place among the rest.
pub fn restored_jumps(jumps: &[Anchor], index: usize, pages: usize) -> (Vec<Anchor>, usize) {
    let index = index.min(jumps.len());
    let kept = |a: &&Anchor| a.page < pages;
    let before = jumps[..index].iter().filter(kept).count();
    let all: Vec<Anchor> = jumps.iter().filter(kept).copied().collect();
    let (all, before) = capped_jumps(&all, before);
    (all, before)
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

/// How many documents the recent list keeps.
pub const RECENT_MAX: usize = 10;

/// The recent documents that still exist, newest first. Missing ones are hidden, not forgotten:
/// a file on a drive that is not mounted comes back with it.
pub fn recent_documents() -> Vec<PathBuf> {
    recent_documents_in(&data_dir())
}

/// The document opened last, for starting with no file named: the newest entry, if it still
/// exists. When it doesn't, nothing is, and the blank page lists the older ones.
pub fn last_document() -> Option<PathBuf> {
    last_document_in(&data_dir())
}

fn last_document_in(dir: &Path) -> Option<PathBuf> {
    load_recent(dir).into_iter().next().filter(|p| p.exists())
}

/// Puts `path` at the top of the recent list, as the file system names it, so that one file is one entry.
pub fn remember_document(path: &Path) {
    let path = std::fs::canonicalize(path).map_or_else(|_| path.to_owned(), crate::strip_verbatim);
    remember_document_in(&data_dir(), &path);
}

fn recent_file(dir: &Path) -> PathBuf {
    dir.join("recent")
}

/// The whole list, one path per line. Without a `recent` file, or with an empty one, it starts
/// from the single path of the older `last` file.
fn load_recent(dir: &Path) -> Vec<PathBuf> {
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).map(|t| parse_recent(&t)).unwrap_or_default();
    let mut list = read("recent");
    if list.is_empty() {
        return read("last");
    }
    // An older build writes only `last`: when it names another document, and was written no
    // earlier than `recent`, it was opened since.
    if let Some(raw) = read("last").into_iter().next() {
        let last = std::fs::canonicalize(&raw).map_or(raw, crate::strip_verbatim);
        if list.first() != Some(&last) && !last_is_older(dir) {
            list = pushed(list, &last);
        }
    }
    list
}

/// Whether `last` is known to be older than `recent`.
fn last_is_older(dir: &Path) -> bool {
    let modified = |name: &str| std::fs::metadata(dir.join(name)).and_then(|m| m.modified()).ok();
    matches!((modified("last"), modified("recent")), (Some(last), Some(recent)) if last < recent)
}

fn recent_documents_in(dir: &Path) -> Vec<PathBuf> {
    load_recent(dir).into_iter().filter(|p| p.exists()).collect()
}

/// Writes `recent`, and `last` for the builds that only read that. Both are readable by this user
/// alone, as they name personal documents.
fn remember_document_in(dir: &Path, path: &Path) {
    let list = pushed(load_recent(dir), path);
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    if builder.create(dir).is_err() {
        return;
    }
    // `last` first: if the second write fails, `last` is the newer, which is read as such.
    let newest = render_recent(&list[..1]);
    if !newest.is_empty() {
        let _ = crate::export::write_atomic(&dir.join("last"), newest.as_bytes(), true, Some(0o600));
    }
    let _ = crate::export::write_atomic(&recent_file(dir), render_recent(&list).as_bytes(), true, Some(0o600));
}

fn parse_recent(text: &str) -> Vec<PathBuf> {
    let mut list: Vec<PathBuf> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let path = PathBuf::from(line);
        if !list.contains(&path) {
            list.push(path);
        }
    }
    list.truncate(RECENT_MAX);
    list
}

/// One path per line. A path that is not valid UTF-8, or has a line break in it, can't be written
/// as a line and is left out.
fn render_recent(list: &[PathBuf]) -> String {
    list.iter().filter_map(|p| p.to_str()).filter(|p| !p.contains(['\n', '\r'])).map(|p| format!("{p}\n")).collect()
}

/// `list` with `path` first, once, and no more than `RECENT_MAX` long.
fn pushed(mut list: Vec<PathBuf>, path: &Path) -> Vec<PathBuf> {
    list.retain(|p| p != path);
    list.insert(0, path.to_owned());
    list.truncate(RECENT_MAX);
    list
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

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn a_pushed_path_goes_first_without_a_duplicate() {
        let list = pushed(paths(&["/a", "/b", "/c"]), Path::new("/b"));
        assert_eq!(list, paths(&["/b", "/a", "/c"]));
        assert_eq!(pushed(Vec::new(), Path::new("/a")), paths(&["/a"]));
    }

    #[test]
    fn the_list_keeps_the_newest_few() {
        let mut list = Vec::new();
        for i in 0..RECENT_MAX + 3 {
            list = pushed(list, Path::new(&format!("/{i}")));
        }
        assert_eq!(list.len(), RECENT_MAX);
        assert_eq!(list[0], PathBuf::from(format!("/{}", RECENT_MAX + 2)));
        assert_eq!(list[RECENT_MAX - 1], PathBuf::from("/3"));
    }

    #[test]
    fn the_list_round_trips_through_its_file_newest_first() {
        let scratch = Scratch::new("recent-trip");
        let (a, b) = (scratch.0.join("a.pdf"), scratch.0.join("b.pdf"));
        std::fs::write(&a, "").unwrap();
        std::fs::write(&b, "").unwrap();
        remember_document_in(&scratch.0, &a);
        remember_document_in(&scratch.0, &b);
        remember_document_in(&scratch.0, &a);
        assert_eq!(recent_documents_in(&scratch.0), vec![a.clone(), b]);
        let leftovers =
            std::fs::read_dir(&scratch.0).unwrap().filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).count();
        assert_eq!(leftovers, 0);
        // `last` follows, for the builds that only read it.
        assert_eq!(std::fs::read_to_string(scratch.0.join("last")).unwrap().trim(), a.to_str().unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["recent", "last"] {
                assert_eq!(std::fs::metadata(scratch.0.join(name)).unwrap().permissions().mode() & 0o077, 0, "{name}");
            }
        }
    }

    #[test]
    fn a_list_starts_from_the_older_last_file() {
        let scratch = Scratch::new("recent-migrate");
        let old = scratch.0.join("old.pdf");
        std::fs::write(&old, "").unwrap();
        std::fs::write(scratch.0.join("last"), old.to_str().unwrap()).unwrap();
        assert_eq!(recent_documents_in(&scratch.0), vec![old.clone()]);
        let new = scratch.0.join("new.pdf");
        std::fs::write(&new, "").unwrap();
        remember_document_in(&scratch.0, &new);
        assert_eq!(recent_documents_in(&scratch.0), vec![new, old]);
    }

    #[test]
    fn the_last_document_is_the_newest_entry_even_when_it_is_gone() {
        let scratch = Scratch::new("recent-last-gone");
        let (here, gone) = (scratch.0.join("here.pdf"), scratch.0.join("gone.pdf"));
        std::fs::write(&here, "").unwrap();
        remember_document_in(&scratch.0, &here);
        assert_eq!(last_document_in(&scratch.0), Some(here.clone()));
        remember_document_in(&scratch.0, &gone);
        assert_eq!(last_document_in(&scratch.0), None);
        assert_eq!(recent_documents_in(&scratch.0), vec![here]);
    }

    #[test]
    fn missing_files_are_hidden_but_kept() {
        let scratch = Scratch::new("recent-missing");
        let (here, gone) = (scratch.0.join("here.pdf"), scratch.0.join("gone.pdf"));
        std::fs::write(&here, "").unwrap();
        remember_document_in(&scratch.0, &gone);
        remember_document_in(&scratch.0, &here);
        assert_eq!(recent_documents_in(&scratch.0), vec![here.clone()]);
        assert_eq!(load_recent(&scratch.0), vec![here, gone]);
        assert!(recent_documents_in(&scratch.0.join("nowhere")).is_empty());
    }

    #[test]
    fn a_path_that_cannot_be_a_line_is_left_out() {
        assert_eq!(render_recent(&paths(&["/a", "/b\nc", "/d"])), "/a\n/d\n");
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let odd = PathBuf::from(std::ffi::OsStr::from_bytes(b"/bad\xff"));
            assert_eq!(render_recent(&[odd, PathBuf::from("/ok")]), "/ok\n");
        }
        assert_eq!(parse_recent("/a\n\n /b \n/a\n"), paths(&["/a", " /b "]));
    }

    #[test]
    fn parse_recent_handles_crlf_and_a_trailing_newline() {
        assert_eq!(parse_recent("/a\r\n/b\r\n"), paths(&["/a", "/b"]));
        assert_eq!(parse_recent("/a\n/b\n"), parse_recent("/a\n/b"));
        assert_eq!(parse_recent("/a\r/b"), paths(&["/a\r/b"]).into_iter().map(|_| PathBuf::from("/a\r/b")).collect::<Vec<_>>());
        assert!(parse_recent("").is_empty());
        assert!(parse_recent("\n\r\n  \n").is_empty());
    }

    #[test]
    fn parse_recent_keeps_only_the_first_ten_and_the_first_of_each_path() {
        let text: String = (0..15).map(|i| format!("/{i}\n/0\n")).collect();
        let list = parse_recent(&text);
        assert_eq!(list.len(), RECENT_MAX);
        assert_eq!(list[0], PathBuf::from("/0"));
        assert_eq!(list[9], PathBuf::from("/9"));
    }

    #[test]
    fn render_then_parse_round_trips_and_ends_in_a_newline() {
        let list = paths(&["/a b/c.pdf", "/ünï/x.pdf", "/ spaced"]);
        let text = render_recent(&list);
        assert!(text.ends_with('\n'));
        assert_eq!(parse_recent(&text), list);
        assert_eq!(render_recent(&[]), "");
    }

    #[test]
    fn a_path_with_edge_spaces_survives_the_round_trip() {
        // Only the line ending is stripped on reading, so blanks at either end of a path are kept.
        let list = paths(&["/a/b.pdf ", " /c.pdf"]);
        assert_eq!(parse_recent(&render_recent(&list)), list);
    }

    #[test]
    fn pushing_an_existing_path_from_the_middle_or_the_end_moves_it_to_the_top() {
        let all: Vec<String> = (0..RECENT_MAX).map(|i| format!("/{i}")).collect();
        let list: Vec<PathBuf> = all.iter().map(PathBuf::from).collect();
        let moved = pushed(list.clone(), Path::new("/5"));
        assert_eq!(moved.len(), RECENT_MAX);
        assert_eq!(moved[0], PathBuf::from("/5"));
        assert_eq!(moved.iter().filter(|p| **p == Path::new("/5")).count(), 1);
        assert_eq!(&moved[1..6], &list[0..5]);
        assert_eq!(&moved[6..], &list[6..]);
        let last = pushed(list.clone(), Path::new("/9"));
        assert_eq!(last.len(), RECENT_MAX);
        assert_eq!(last[0], PathBuf::from("/9"));
        assert_eq!(&last[1..], &list[..9]);
        assert_eq!(pushed(list.clone(), Path::new("/0")), list);
    }

    #[test]
    fn hiding_missing_files_keeps_the_order_of_the_rest() {
        let scratch = Scratch::new("recent-order");
        let mut kept = Vec::new();
        for i in 0..6 {
            let p = scratch.0.join(format!("{i}.pdf"));
            if i % 2 == 0 {
                std::fs::write(&p, "").unwrap();
                kept.push(p.clone());
            }
            remember_document_in(&scratch.0, &p);
        }
        kept.reverse();
        assert_eq!(recent_documents_in(&scratch.0), kept);
        assert_eq!(load_recent(&scratch.0).len(), 6);
    }

    #[test]
    fn a_recent_file_with_crlf_and_a_trailing_newline_is_read() {
        let scratch = Scratch::new("recent-crlf");
        let (a, b) = (scratch.0.join("a.pdf"), scratch.0.join("b.pdf"));
        std::fs::write(&a, "").unwrap();
        std::fs::write(&b, "").unwrap();
        let text = format!("{}\r\n{}\r\n", a.display(), b.display());
        std::fs::write(scratch.0.join("recent"), text).unwrap();
        assert_eq!(recent_documents_in(&scratch.0), vec![a.clone(), b.clone()]);
        remember_document_in(&scratch.0, &b);
        assert_eq!(recent_documents_in(&scratch.0), vec![b, a]);
    }

    #[test]
    fn a_very_long_path_is_stored_and_read_back() {
        let scratch = Scratch::new("recent-long");
        let long = PathBuf::from(format!("/{}", "x".repeat(100_000)));
        remember_document_in(&scratch.0, &long);
        assert_eq!(load_recent(&scratch.0), vec![long.clone()]);
        // It doesn't exist, so it is hidden rather than an error.
        assert!(recent_documents_in(&scratch.0).is_empty());
        remember_document_in(&scratch.0, Path::new("/short"));
        assert_eq!(load_recent(&scratch.0), vec![PathBuf::from("/short"), long]);
    }

    #[test]
    fn an_empty_recent_file_falls_back_to_last_but_a_filled_one_wins() {
        let scratch = Scratch::new("recent-empty-wins");
        let old = scratch.0.join("old.pdf");
        std::fs::write(&old, "").unwrap();
        std::fs::write(scratch.0.join("last"), old.to_str().unwrap()).unwrap();
        std::fs::write(scratch.0.join("recent"), "").unwrap();
        assert_eq!(recent_documents_in(&scratch.0), vec![old.clone()]);
        let new = scratch.0.join("new.pdf");
        std::fs::write(&new, "").unwrap();
        std::fs::write(scratch.0.join("recent"), format!("{}\n", new.display())).unwrap();
        assert_eq!(recent_documents_in(&scratch.0), vec![new]);
    }

    fn touch(path: &Path) -> PathBuf {
        std::fs::write(path, "").unwrap();
        path.to_owned()
    }

    fn set_age(path: &Path, ago: u64) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(ago)).unwrap();
    }

    fn write_lists(dir: &Path, recent: &[PathBuf], last: &Path) {
        std::fs::write(dir.join("recent"), render_recent(recent)).unwrap();
        std::fs::write(dir.join("last"), last.to_str().unwrap()).unwrap();
    }

    #[test]
    fn a_different_last_is_promoted() {
        let scratch = Scratch::new("recent-promote");
        let (a, b) = (touch(&scratch.0.join("a.pdf")), touch(&scratch.0.join("b.pdf")));
        write_lists(&scratch.0, std::slice::from_ref(&a), &b);
        assert_eq!(load_recent(&scratch.0), vec![b, a]);
    }

    #[test]
    fn a_last_equal_to_the_newest_changes_nothing() {
        let scratch = Scratch::new("recent-same");
        let (a, b) = (touch(&scratch.0.join("a.pdf")), touch(&scratch.0.join("b.pdf")));
        write_lists(&scratch.0, &[a.clone(), b.clone()], &a);
        assert_eq!(load_recent(&scratch.0), vec![a, b]);
    }

    #[test]
    fn a_last_deeper_in_the_list_moves_up_once() {
        let scratch = Scratch::new("recent-deeper");
        let (a, b, c) = (touch(&scratch.0.join("a.pdf")), touch(&scratch.0.join("b.pdf")), touch(&scratch.0.join("c.pdf")));
        write_lists(&scratch.0, &[a.clone(), b.clone(), c.clone()], &c);
        assert_eq!(load_recent(&scratch.0), vec![c, a, b]);
    }

    #[test]
    fn an_older_last_is_not_promoted() {
        let scratch = Scratch::new("recent-stale");
        let (a, b) = (touch(&scratch.0.join("a.pdf")), touch(&scratch.0.join("b.pdf")));
        write_lists(&scratch.0, std::slice::from_ref(&a), &b);
        set_age(&scratch.0.join("last"), 100);
        assert_eq!(load_recent(&scratch.0), vec![a]);
    }

    #[test]
    fn a_promoted_last_keeps_the_cap() {
        let scratch = Scratch::new("recent-cap");
        let list: Vec<PathBuf> = (0..RECENT_MAX).map(|i| touch(&scratch.0.join(format!("{i}.pdf")))).collect();
        let extra = touch(&scratch.0.join("extra.pdf"));
        write_lists(&scratch.0, &list, &extra);
        let loaded = load_recent(&scratch.0);
        assert_eq!(loaded.len(), RECENT_MAX);
        assert_eq!(loaded[0], extra);
        assert_eq!(&loaded[1..], &list[..RECENT_MAX - 1]);
    }

    #[cfg(unix)]
    #[test]
    fn a_new_data_dir_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new("recent-mode");
        let dir = scratch.0.join("data");
        remember_document_in(&dir, &touch(&scratch.0.join("a.pdf")));
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    }

    #[test]
    fn a_last_file_with_a_trailing_newline_or_crlf_migrates() {
        let scratch = Scratch::new("recent-last-nl");
        let old = scratch.0.join("old.pdf");
        std::fs::write(&old, "").unwrap();
        std::fs::write(scratch.0.join("last"), format!("{}\r\n", old.display())).unwrap();
        assert_eq!(recent_documents_in(&scratch.0), vec![old]);
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

    fn spot(page: usize) -> Anchor {
        Anchor { page, y: 1.0, x: 0.0 }
    }

    #[test]
    fn bookmarks_survive_a_round_trip() {
        let bookmarks = vec![Bookmark { name: "intro".into(), anchor: spot(1) }, Bookmark { name: "proof".into(), anchor: spot(5) }];
        let side = Sidecar { bookmarks, ..Default::default() };
        let text = serde_json::to_string(&side).unwrap();
        assert_eq!(serde_json::from_str::<Sidecar>(&text).unwrap(), side);
    }

    #[test]
    fn an_old_sidecar_without_bookmarks_loads() {
        let old = r#"{"path":"/a.pdf","anchor":null,"zoom":1.5,"marks":{},"portals":[],"annotations":[],"next_id":3}"#;
        let side: Sidecar = serde_json::from_str(old).unwrap();
        assert!(side.bookmarks.is_empty());
    }

    #[test]
    fn adding_a_bookmark_keeps_document_order_and_a_name_moves() {
        let mut list = Vec::new();
        assert!(!add_bookmark(&mut list, "b", spot(5)));
        assert!(!add_bookmark(&mut list, "a", spot(2)));
        assert!(!add_bookmark(&mut list, "c", Anchor { page: 2, y: 0.5, x: 0.0 }));
        let names = |l: &[Bookmark]| l.iter().map(|b| b.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&list), ["c", "a", "b"]);
        assert!(add_bookmark(&mut list, "c", spot(9)));
        assert_eq!(names(&list), ["a", "b", "c"]);
        assert_eq!(list[2].anchor.page, 9);
    }

    #[test]
    fn removing_a_bookmark_and_filtering_them() {
        let mut list = Vec::new();
        for (name, page) in [("Intro", 1), ("Proof of A", 2), ("proof of B", 3)] {
            add_bookmark(&mut list, name, spot(page));
        }
        assert_eq!(bookmark_rows(&list, ""), [0, 1, 2]);
        assert_eq!(bookmark_rows(&list, "PROOF"), [1, 2]);
        assert!(bookmark_rows(&list, "zzz").is_empty());
        assert_eq!(remove_bookmark(&mut list, 1).map(|b| b.name), Some("Proof of A".to_owned()));
        assert_eq!(list.len(), 2);
        assert_eq!(remove_bookmark(&mut list, 5), None);
    }

    fn bm_names(l: &[Bookmark]) -> Vec<String> {
        l.iter().map(|b| b.name.clone()).collect()
    }

    #[test]
    fn bookmark_names_match_without_regard_to_case_and_are_unicode_safe() {
        let mut list = Vec::new();
        add_bookmark(&mut list, "Intro", spot(1));
        assert!(add_bookmark(&mut list, "intro", spot(2)), "a case variant moves the bookmark");
        assert_eq!(bm_names(&list), ["intro"], "the new spelling is stored");
        add_bookmark(&mut list, "héllo wörld", spot(3));
        add_bookmark(&mut list, "🔖 mark", spot(4));
        add_bookmark(&mut list, "日本語", spot(5));
        assert_eq!(list.len(), 4);
        assert!(add_bookmark(&mut list, "HÉLLO WÖRLD", spot(0)));
        assert_eq!(list[0].name, "HÉLLO WÖRLD");
        assert!(add_bookmark(&mut list, "🔖 mark", spot(0)));
        assert_eq!(list.len(), 4);
        assert_eq!(bookmark_rows(&list, "INTRO"), [list.iter().position(|b| b.name == "intro").unwrap()]);
        assert_eq!(bookmark_rows(&list, "日本"), [list.iter().position(|b| b.name == "日本語").unwrap()]);
        assert!(bookmark_rows(&list, "x🔖").is_empty());
        let text = serde_json::to_string(&Sidecar { bookmarks: list.clone(), ..Default::default() }).unwrap();
        assert_eq!(serde_json::from_str::<Sidecar>(&text).unwrap().bookmarks, list);
    }

    #[test]
    fn names_are_normalised_and_capped_at_a_char_boundary() {
        assert_eq!(normalise_bookmark_name("   "), "");
        assert_eq!(
            normalise_bookmark_name(
                "  a 	 b

c   d  "
            ),
            "a b c d"
        );
        let long = "é".repeat(300);
        assert_eq!(normalise_bookmark_name(&long).chars().count(), MAX_BOOKMARK_NAME);
        let spaced = format!("{} x", "a".repeat(MAX_BOOKMARK_NAME - 1));
        assert_eq!(normalise_bookmark_name(&spaced), "a".repeat(MAX_BOOKMARK_NAME - 1));
    }

    #[test]
    fn an_empty_name_is_stored_as_given_by_add_bookmark() {
        // `add_bookmark` does not normalise: `command_bookmark` does that, and refuses an empty name.
        let mut list = Vec::new();
        add_bookmark(&mut list, "   ", spot(1));
        add_bookmark(&mut list, "", spot(2));
        assert_eq!(list.len(), 2);
        assert_eq!(bookmark_rows(&list, " "), [0]);
        assert_eq!(bookmark_rows(&list, ""), [0, 1]);
    }

    #[test]
    fn moving_a_bookmark_earlier_or_later_keeps_the_order() {
        let mut list = Vec::new();
        for (n, p) in [("a", 1), ("b", 2), ("c", 3), ("d", 4)] {
            add_bookmark(&mut list, n, spot(p));
        }
        assert!(add_bookmark(&mut list, "d", spot(0)));
        assert_eq!(bm_names(&list), ["d", "a", "b", "c"]);
        assert!(add_bookmark(&mut list, "d", spot(99)));
        assert_eq!(bm_names(&list), ["a", "b", "c", "d"]);
        assert!(add_bookmark(&mut list, "b", Anchor { page: 2, y: 5.0, x: 3.0 }));
        assert_eq!(bm_names(&list), ["a", "b", "c", "d"]);
        assert_eq!(list[1].anchor.x, 3.0);
        assert_eq!(list.len(), 4);
    }

    #[test]
    fn equal_places_for_two_names_are_both_kept_and_the_sort_is_stable() {
        let mut list = Vec::new();
        add_bookmark(&mut list, "first", spot(3));
        add_bookmark(&mut list, "second", spot(3));
        add_bookmark(&mut list, "third", spot(3));
        assert_eq!(bm_names(&list), ["first", "second", "third"]);
        // Moving one to the same place puts it last among its equals.
        add_bookmark(&mut list, "first", spot(3));
        assert_eq!(bm_names(&list), ["second", "third", "first"]);
    }

    #[test]
    fn a_nan_or_infinite_y_does_not_panic_the_sort() {
        let mut list = Vec::new();
        for (n, y) in [("a", f32::NAN), ("b", 1.0), ("c", f32::INFINITY), ("d", f32::NEG_INFINITY), ("e", -f32::NAN)] {
            add_bookmark(&mut list, n, Anchor { page: 1, y, x: 0.0 });
        }
        assert_eq!(list.len(), 5);
        let finite: Vec<&str> = list.iter().filter(|b| !b.anchor.y.is_nan()).map(|b| b.name.as_str()).collect();
        assert_eq!(finite, ["d", "b", "c"]);
        // NaN cannot come from a sidecar: JSON has no NaN, and null is refused for a number.
        let bad = r#"{"bookmarks":[{"name":"x","anchor":{"page":0,"y":null,"x":0.0}}]}"#;
        assert!(serde_json::from_str::<Sidecar>(bad).is_err());
    }

    #[test]
    fn removing_a_missing_bookmark_changes_nothing() {
        let mut list = Vec::new();
        assert_eq!(remove_bookmark(&mut list, 0), None);
        add_bookmark(&mut list, "a", spot(1));
        assert_eq!(remove_bookmark(&mut list, 1), None);
        assert_eq!(remove_bookmark(&mut list, usize::MAX), None);
        assert_eq!(list.len(), 1);
        assert_eq!(remove_bookmark(&mut list, 0).map(|b| b.name), Some("a".to_owned()));
        assert!(list.is_empty());
    }

    #[test]
    fn a_thousand_bookmarks_sort_and_filter_quickly() {
        let mut list = Vec::new();
        let start = std::time::Instant::now();
        for i in 0..1000usize {
            // Pages in a scrambled order, so each insert re-sorts something out of order.
            add_bookmark(&mut list, &format!("Mark {i} é"), Anchor { page: (i * 7919) % 1000, y: (i % 13) as f32, x: 0.0 });
        }
        assert_eq!(list.len(), 1000);
        assert!(list.windows(2).all(|w| (w[0].anchor.page, w[0].anchor.y) <= (w[1].anchor.page, w[1].anchor.y)));
        assert_eq!(bookmark_rows(&list, "").len(), 1000);
        assert_eq!(bookmark_rows(&list, "MARK 99").len(), 11);
        assert_eq!(bookmark_rows(&list, "É").len(), 1000);
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "{:?}", start.elapsed());
    }

    #[test]
    fn jumps_survive_a_round_trip() {
        let side = Sidecar { jumps: vec![spot(1), spot(5)], jump_index: 1, ..Default::default() };
        let text = serde_json::to_string(&side).unwrap();
        assert_eq!(serde_json::from_str::<Sidecar>(&text).unwrap(), side);
    }

    #[test]
    fn an_old_sidecar_without_jumps_loads() {
        let old = r#"{"path":"/a.pdf","anchor":null,"zoom":1.5,"marks":{},"portals":[],"annotations":[],"next_id":3}"#;
        let side: Sidecar = serde_json::from_str(old).unwrap();
        assert!(side.jumps.is_empty());
        assert_eq!(side.jump_index, 0);
    }

    #[test]
    fn the_jump_list_keeps_the_newest_and_moves_the_index() {
        let many: Vec<Anchor> = (0..MAX_JUMPS + 10).map(spot).collect();
        let (kept, index) = capped_jumps(&many, MAX_JUMPS + 5);
        assert_eq!(kept.len(), MAX_JUMPS);
        assert_eq!(kept[0].page, 10);
        assert_eq!(index, MAX_JUMPS - 5);
        assert_eq!(capped_jumps(&many, 3).1, 0);
        assert_eq!(capped_jumps(&many[..2], 2), (many[..2].to_vec(), 2));
    }

    #[test]
    fn restoring_jumps_drops_pages_past_the_end() {
        let jumps = [spot(0), spot(9), spot(2), spot(7), spot(3)];
        let (kept, index) = restored_jumps(&jumps, 4, 5);
        assert_eq!(kept.iter().map(|a| a.page).collect::<Vec<_>>(), [0, 2, 3]);
        assert_eq!(index, 2);
        assert_eq!(restored_jumps(&jumps, 99, 100), (jumps.to_vec(), 5));
        assert_eq!(restored_jumps(&jumps, 2, 0), (Vec::new(), 0));
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

    mod jump_history_gaps {
        use super::*;

        fn pages(list: &[Anchor]) -> Vec<usize> {
            list.iter().map(|a| a.page).collect()
        }

        fn run(n: usize) -> Vec<Anchor> {
            (0..n).map(super::spot).collect()
        }

        #[test]
        fn capping_clamps_an_index_past_the_length() {
            let (kept, index) = capped_jumps(&run(3), 50);
            assert_eq!((kept.len(), index), (3, 3));
            let (kept, index) = capped_jumps(&run(150), 9999);
            assert_eq!((kept.len(), index), (MAX_JUMPS, MAX_JUMPS));
            assert_eq!(kept[0].page, 50);
            assert_eq!(capped_jumps(&run(3), usize::MAX).1, 3);
        }

        #[test]
        fn capping_keeps_an_index_at_the_length_at_the_newest_place() {
            assert_eq!(capped_jumps(&run(100), 100).1, 100);
            assert_eq!(capped_jumps(&run(101), 101).1, 100);
            assert_eq!(capped_jumps(&run(500), 500).1, 100);
        }

        #[test]
        fn capping_exactly_100_and_101() {
            let (kept, index) = capped_jumps(&run(100), 40);
            assert_eq!((pages(&kept), index), (pages(&run(100)), 40));
            let (kept, index) = capped_jumps(&run(101), 40);
            assert_eq!((kept.len(), kept[0].page, index), (100, 1, 39));
            // An index on the entry that is dropped saturates to the oldest kept one.
            assert_eq!(capped_jumps(&run(101), 0).1, 0);
            assert_eq!(capped_jumps(&run(101), 1).1, 0);
            assert_eq!(capped_jumps(&run(101), 2).1, 1);
        }

        #[test]
        fn capping_an_empty_list() {
            assert_eq!(capped_jumps(&[], 0), (Vec::new(), 0));
            assert_eq!(capped_jumps(&[], 7), (Vec::new(), 0));
        }

        #[test]
        fn restoring_with_an_index_past_the_length_puts_it_at_the_newest_place() {
            let jumps = run(4);
            assert_eq!(restored_jumps(&jumps, 10, 10), (jumps.clone(), 4));
            assert_eq!(restored_jumps(&jumps, usize::MAX, 10), (jumps.clone(), 4));
            // Past the length with some entries dropped: still at the (new) end.
            let (kept, index) = restored_jumps(&jumps, 10, 2);
            assert_eq!((pages(&kept), index), (vec![0, 1], 2));
        }

        #[test]
        fn restoring_with_an_index_at_the_length() {
            let jumps = run(4);
            assert_eq!(restored_jumps(&jumps, 4, 10), (jumps.clone(), 4));
            let (kept, index) = restored_jumps(&jumps, 4, 3);
            assert_eq!((pages(&kept), index), (vec![0, 1, 2], 3));
        }

        #[test]
        fn restoring_with_the_index_on_a_dropped_entry() {
            // Index 1 points at page 9, which is gone: it lands on what followed it.
            let jumps = [spot(0), spot(9), spot(2)];
            let (kept, index) = restored_jumps(&jumps, 1, 5);
            assert_eq!((pages(&kept), index), (vec![0, 2], 1));
            // Dropped entry last, index on it: ends at the newest place.
            let jumps = [spot(0), spot(1), spot(9)];
            let (kept, index) = restored_jumps(&jumps, 2, 5);
            assert_eq!((pages(&kept), index), (vec![0, 1], 2));
            // Dropped entries all before the index.
            let jumps = [spot(9), spot(8), spot(1)];
            let (kept, index) = restored_jumps(&jumps, 2, 5);
            assert_eq!((pages(&kept), index), (vec![1], 0));
        }

        #[test]
        fn restoring_with_every_entry_dropped() {
            let jumps = [spot(5), spot(6), spot(7)];
            assert_eq!(restored_jumps(&jumps, 0, 5), (Vec::new(), 0));
            assert_eq!(restored_jumps(&jumps, 2, 5), (Vec::new(), 0));
            assert_eq!(restored_jumps(&jumps, 3, 5), (Vec::new(), 0));
            assert_eq!(restored_jumps(&jumps, 99, 5), (Vec::new(), 0));
        }

        #[test]
        fn restoring_exactly_100_and_101_and_more() {
            let (kept, index) = restored_jumps(&run(100), 100, 1000);
            assert_eq!((kept.len(), index), (100, 100));
            let (kept, index) = restored_jumps(&run(101), 101, 1000);
            assert_eq!((kept.len(), kept[0].page, index), (100, 1, 100));
            let (kept, index) = restored_jumps(&run(101), 50, 1000);
            assert_eq!((kept.len(), index), (100, 49));
            // Dropping by page and capping together: pages 0..200, document of 150 pages.
            let (kept, index) = restored_jumps(&run(200), 200, 150);
            assert_eq!((kept.len(), kept[0].page, index), (100, 50, 100));
        }

        #[test]
        fn restoring_an_empty_list_with_a_nonzero_index() {
            assert_eq!(restored_jumps(&[], 5, 10), (Vec::new(), 0));
            assert_eq!(restored_jumps(&[], usize::MAX, 10), (Vec::new(), 0));
        }

        #[test]
        fn restoring_for_zero_and_one_page_documents() {
            let jumps = [spot(0), spot(1), spot(0)];
            assert_eq!(restored_jumps(&jumps, 3, 0), (Vec::new(), 0));
            let (kept, index) = restored_jumps(&jumps, 3, 1);
            assert_eq!((pages(&kept), index), (vec![0, 0], 2));
            let (kept, index) = restored_jumps(&jumps, 1, 1);
            assert_eq!((pages(&kept), index), (vec![0, 0], 1));
        }

        #[test]
        fn anchors_with_odd_numbers_in_json() {
            let parse = |y: &str| serde_json::from_str::<Sidecar>(&format!(r#"{{"jumps":[{{"page":0,"y":{y},"x":0.0}}],"jump_index":1}}"#));
            // Negative y is accepted as is, and passes through restore untouched.
            let side = parse("-12.5").unwrap();
            assert_eq!(side.jumps[0].y, -12.5);
            let (kept, index) = restored_jumps(&side.jumps, side.jump_index, 3);
            assert_eq!((kept.len(), index, kept[0].y), (1, 1, -12.5));
            // JSON has no NaN literal, and null is not a number.
            assert!(parse("NaN").is_err());
            assert!(parse("null").is_err());
            // A negative page or index is refused for a usize, so the sidecar counts as damaged.
            assert!(serde_json::from_str::<Sidecar>(r#"{"jumps":[{"page":-1,"y":0,"x":0}]}"#).is_err());
            assert!(serde_json::from_str::<Sidecar>(r#"{"jump_index":-1}"#).is_err());
        }

        #[test]
        fn a_nan_anchor_in_memory_survives_capping_and_restoring() {
            let nan = Anchor { page: 0, y: f32::NAN, x: f32::NAN };
            let (kept, index) = restored_jumps(&[nan], 1, 1);
            assert_eq!((kept.len(), index), (1, 1));
            assert!(kept[0].y.is_nan());
            // serde_json writes NaN as null, which then fails to load back: record what happens.
            let side = Sidecar { jumps: vec![nan], jump_index: 1, ..Default::default() };
            let text = serde_json::to_string(&side).unwrap();
            assert!(serde_json::from_str::<Sidecar>(&text).is_err(), "{text}");
        }

        #[test]
        fn a_new_sidecar_loads_in_a_build_without_the_new_fields() {
            #[derive(Debug, Default, serde::Deserialize)]
            #[serde(default)]
            struct OldSidecar {
                path: String,
                anchor: Option<Anchor>,
                zoom: Option<f32>,
                next_id: u64,
            }
            let side = Sidecar { path: "/a.pdf".into(), zoom: Some(2.0), jumps: vec![spot(1), spot(2)], jump_index: 1, ..Default::default() };
            let text = serde_json::to_string_pretty(&side).unwrap();
            assert!(text.contains("\"jumps\"") && text.contains("\"jump_index\""));
            let old: OldSidecar = serde_json::from_str(&text).unwrap();
            assert_eq!(old.path, "/a.pdf");
            assert_eq!(old.zoom, Some(2.0));
            assert!(old.anchor.is_none());
            let _ = old.next_id;
        }

        #[test]
        fn the_real_sidecar_tolerates_unknown_fields() {
            let text = r#"{"path":"/a.pdf","from_the_future":{"a":[1,2]},"jumps":[{"page":2,"y":1.0,"x":0.0}],"jump_index":1}"#;
            let side: Sidecar = serde_json::from_str(text).unwrap();
            assert_eq!(side.path, "/a.pdf");
            assert_eq!((side.jumps.len(), side.jump_index), (1, 1));
            // Anchors too ignore unknown fields.
            let text = r#"{"jumps":[{"page":2,"y":1.0,"x":0.0,"extra":true}]}"#;
            assert_eq!(serde_json::from_str::<Sidecar>(text).unwrap().jumps.len(), 1);
        }
    }
}
