//! The application state and the frame loop.
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime};

use egui::{Pos2, Rect, pos2};
use pdfcraft_render::{Pixels, RenderRequest, RequestKind};

use crate::cache::{TileCache, TileKey};
use crate::config::Config;
use crate::doc::{Doc, OpenFailure, placeholder_pdf};
use crate::field::Field;
use crate::form::{FormChange, FormState, HideChange};
use crate::hint::HintState;
use crate::keymap::{Keymap, Pending};
use crate::overview::Overview;
use crate::protect::Job;
use crate::search::Search;
use crate::smart::Reference;
use crate::store::{self, Annotation, Sidecar, Store, Strokes};
use crate::view::{Anchor, View};
use pdfcraft_render::PageText;

/// How long zooming has to stop for before it counts as over: a slow page waits this long to
/// re-render, and prefetching to resume.
pub const ZOOM_SETTLE: Duration = Duration::from_millis(120);
/// How long other pages wait: a little over the time between repeats of a held key, so a held key
/// stretches what is showing, and doesn't upload a raster of every step along the way.
const ZOOM_FOLLOW: Duration = Duration::from_millis(60);
/// A page that took this long to render is slow: restarting it at every step of a zoom would
/// only keep it from finishing.
const SLOW_MILLIS: u32 = 500;
/// How long saved state may lag behind.
const SAVE_EVERY: Duration = Duration::from_secs(2);
/// How long a message stays in the status bar.
const MESSAGE_FOR: Duration = Duration::from_secs(6);
/// Time per frame for searching pages whose text has arrived.
const SEARCH_BUDGET: Duration = Duration::from_millis(6);
/// Rendered pixels turned into textures per frame, so a burst of results can't stall a frame.
/// One result always goes through, however large.
const UPLOAD_BYTES_PER_FRAME: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Width,
    Height,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Highlight,
    Draw,
    Erase,
    Note,
    Fill,
}

/// What the fill & sign tool puts on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillItem {
    Text,
    Date,
    Check,
    Cross,
    Dot,
    Line,
    Sign,
}

impl FillItem {
    pub const ALL: [Self; 7] = [Self::Text, Self::Date, Self::Check, Self::Cross, Self::Dot, Self::Line, Self::Sign];

    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Date => "date",
            Self::Check => "check",
            Self::Cross => "cross",
            Self::Dot => "dot",
            Self::Line => "line",
            Self::Sign => "sign",
        }
    }
}

/// What a text entry in the bar makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Note,
    Text,
}

/// Text being typed for a place on a page, or for a note that is there already.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub kind: EntryKind,
    pub page: usize,
    pub at: [f32; 2],
    pub editing: Option<u64>,
}

/// The pad a signature is drawn on. Strokes are in fractions of its width.
#[derive(Clone, Debug, Default)]
pub struct SignPad {
    pub strokes: Strokes,
    pub drawing: bool,
    /// Set by an undo mid-stroke: the pointer is ignored until the button is released.
    pub wait_for_release: bool,
    /// Where to put the signature once it is drawn.
    pub place: Option<(usize, [f32; 2])>,
}

impl SignPad {
    /// Takes off the last stroke, ending it if it is still being drawn.
    pub fn undo_stroke(&mut self) {
        self.strokes.pop();
        if self.drawing {
            self.wait_for_release = true;
        }
        self.drawing = false;
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BarKind {
    Search {
        forward: bool,
    },
    Command,
    Entry(Entry),
    /// A field of the form: what it is editing is `App::form.edit`.
    Form,
}

/// The bar at the top.
#[derive(Clone, Debug)]
pub struct Bar {
    pub kind: BarKind,
    pub field: Field,
    /// Where the view was when the bar opened, to go back to on escape.
    pub origin: Anchor,
    /// The search the bar's replaces, to go back to on escape.
    pub previous: Option<Search>,
    /// The menu of the `:` bar, made again whenever its text changes. Boxed to keep `Mode` small.
    pub menu: Option<Box<crate::menu::Menu>>,
}

#[derive(Clone, Debug, Default)]
pub struct TocState {
    pub selected: usize,
    pub filter: Field,
    pub filtering: bool,
}

/// The panel for choosing in a combo or list box, laid out like the table of contents.
#[derive(Clone, Debug)]
pub struct ChoiceState {
    /// The field's fully qualified name.
    pub name: String,
    /// What the panel's header says: the field's tooltip, else its name.
    pub title: String,
    /// The options as (export value, display text).
    pub options: Vec<(String, String)>,
    pub multi: bool,
    pub editable: bool,
    /// The export values chosen now. In a multi-select list, the marks Space toggles.
    pub marked: Vec<String>,
    /// The values the field had on opening, to tell whether Enter changes anything.
    pub initial: Vec<String>,
    /// The current values that are no option's export value, each shown as a ticked row.
    pub strangers: Vec<String>,
    /// The highlighted row, as a place in the rows the filter lets through.
    pub selected: usize,
    /// The first row shown. Moving the highlight keeps it in sight; the wheel moves it.
    pub first: usize,
    /// The part of a row the wheel has moved the list by and not yet scrolled.
    pub wheel_rest: f32,
    pub filter: Field,
    pub filtering: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkOp {
    Set,
    Goto,
}

#[derive(Clone, Debug)]
pub enum Mode {
    Normal,
    Bar(Bar),
    Hint(HintState),
    Toc(TocState),
    Bookmarks(crate::bookmarks::BookmarkState),
    Recent(crate::recent::RecentState),
    Choice(ChoiceState),
    AwaitMark(MarkOp),
    Sign(SignPad),
    Password(PasswordState),
}

/// What the password menu is asking for.
#[derive(Clone, PartialEq, Eq)]
pub enum Purpose {
    /// The password that opens the file.
    Open,
    /// The owner password of the open document, before `job` writes the copy at `target`.
    Owner { job: Job, target: PathBuf, force: bool },
    /// The new password of the copy at `target`. `owner` is the owner password of the document, when
    /// it had to be asked for.
    New { target: PathBuf, force: bool, owner: Option<String> },
    /// The new password again, `first` being the first time.
    Confirm { target: PathBuf, force: bool, owner: Option<String>, first: String },
}

/// What typing in a step of the new password leads to.
#[derive(PartialEq, Eq)]
pub enum Entered {
    /// Nothing moves: the notice says why.
    Notice(String),
    /// The menu goes on to the next question.
    Next(PasswordState),
    /// The copy is to be written.
    Write { target: PathBuf, force: bool, owner: Option<String>, password: String },
}

/// The password menu: the file it is about, and what was typed so far.
#[derive(Clone, PartialEq, Eq)]
pub struct PasswordState {
    /// The file whose name the header gives: the one that wants a password, the document asked the
    /// owner password of, or the copy being given a new one.
    pub path: PathBuf,
    pub purpose: Purpose,
    pub field: Field,
    /// What went wrong with the last try.
    pub notice: Option<String>,
}

impl PasswordState {
    pub fn new(path: PathBuf, notice: Option<&str>) -> Self {
        Self { path, purpose: Purpose::Open, field: Field::default(), notice: notice.map(str::to_owned) }
    }

    /// The question of the owner password of `document`, before `job` writes `target`.
    pub fn owner(document: PathBuf, job: Job, target: PathBuf, force: bool) -> Self {
        Self { path: document, purpose: Purpose::Owner { job, target, force }, field: Field::default(), notice: None }
    }

    /// The question of the new password of the copy at `target`.
    pub fn new_password(target: PathBuf, force: bool, owner: Option<String>, notice: Option<&str>) -> Self {
        Self { path: target.clone(), purpose: Purpose::New { target, force, owner }, field: Field::default(), notice: notice.map(str::to_owned) }
    }

    /// What the header of the menu says.
    pub fn header(&self) -> String {
        let name = self.path.file_name().map_or_else(|| self.path.display().to_string(), |n| n.to_string_lossy().into_owned());
        match self.purpose {
            Purpose::Open => format!("Password for {name}"),
            Purpose::Owner { .. } => format!("Owner password for {name}"),
            Purpose::New { .. } => format!("New password for {name}"),
            Purpose::Confirm { .. } => "Type it again".to_owned(),
        }
    }

    /// What typing a new password, or the same again, leads to.
    pub fn entered(&self) -> Entered {
        let typed = self.field.text.clone();
        match &self.purpose {
            Purpose::New { target, force, owner }
                if typed.is_empty() || pdfcraft_cos::effective_password(pdfcraft_cos::Algorithm::Aes256, &typed).is_empty() =>
            {
                Entered::Next(Self::new_password(target.clone(), *force, owner.clone(), Some("the password is empty")))
            }
            Purpose::New { target, force, owner } => Entered::Next(Self {
                path: self.path.clone(),
                purpose: Purpose::Confirm { target: target.clone(), force: *force, owner: owner.clone(), first: typed },
                field: Field::default(),
                notice: None,
            }),
            Purpose::Confirm { target, force, owner, first } if *first == typed => {
                Entered::Write { target: target.clone(), force: *force, owner: owner.clone(), password: typed }
            }
            Purpose::Confirm { target, force, owner, .. } => {
                Entered::Next(Self::new_password(target.clone(), *force, owner.clone(), Some("the passwords don't match")))
            }
            Purpose::Open | Purpose::Owner { .. } => Entered::Notice("not a new password".to_owned()),
        }
    }
}

/// Written by hand, for the password it holds.
impl std::fmt::Debug for Entered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Notice(notice) => f.debug_tuple("Notice").field(notice).finish(),
            Self::Next(state) => f.debug_tuple("Next").field(state).finish(),
            Self::Write { target, force, .. } => f.debug_struct("Write").field("target", target).field("force", force).finish_non_exhaustive(),
        }
    }
}

/// Written by hand so that what was typed, and the passwords asked for before, can never reach a
/// log through `{:?}`.
impl std::fmt::Debug for Purpose {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => f.write_str("Open"),
            Self::Owner { job, target, force } => f.debug_struct("Owner").field("job", job).field("target", target).field("force", force).finish(),
            Self::New { target, force, owner } => {
                f.debug_struct("New").field("target", target).field("force", force).field("owner", &owner.is_some()).finish()
            }
            Self::Confirm { target, force, owner, .. } => {
                f.debug_struct("Confirm").field("target", target).field("force", force).field("owner", &owner.is_some()).finish_non_exhaustive()
            }
        }
    }
}

/// Written by hand so that what was typed can never reach a log through `{:?}`.
impl std::fmt::Debug for PasswordState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PasswordState")
            .field("path", &self.path)
            .field("purpose", &self.purpose)
            .field("notice", &self.notice)
            .finish_non_exhaustive()
    }
}

/// Writes the sidecar, unless the document is locked: then what is shown is a stand-in for it, and
/// writing would replace what is saved for the real one.
fn save_sidecar(locked: bool, store: &Store, side: &Sidecar) -> Result<(), String> {
    if locked { Ok(()) } else { store.save(side) }
}

/// A text selection between two glyphs, in reading order and inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start: (usize, usize),
    pub end: (usize, usize),
}

impl Selection {
    pub fn new(a: (usize, usize), b: (usize, usize)) -> Self {
        if a <= b { Self { start: a, end: b } } else { Self { start: b, end: a } }
    }
}

/// Where following a reference ends up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Landing {
    /// At what it points at, with a jump back.
    Jump,
    /// In the same place, with what it points at shown in the overview.
    Overview,
}

/// One undoable change to the annotations.
#[derive(Clone, Debug)]
pub enum Edit {
    Added(Vec<Annotation>),
    Removed(Vec<Annotation>),
    /// A note's text changed.
    Replaced {
        before: Annotation,
        after: Annotation,
    },
    /// Values filled into the form changed.
    Form(Vec<FormChange>),
    /// Widgets were shown or hidden by a form's button.
    Hidden(Vec<HideChange>),
    /// Several changes made as one step.
    Group(Vec<Edit>),
}

/// A portal being made: from where, and whether it replaces an existing one's target.
#[derive(Clone, Copy, Debug)]
pub struct PendingPortal {
    pub source: Anchor,
    pub replacing: Option<u64>,
}

/// Watches the document and the config for changes, off the UI thread.
struct Watcher {
    doc_path: Arc<std::sync::Mutex<PathBuf>>,
    doc_changed: Arc<AtomicBool>,
    config_changed: Arc<AtomicBool>,
}

type Stamp = Option<(SystemTime, u64)>;

fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

impl Watcher {
    fn start(ctx: egui::Context, doc: &Path) -> Self {
        let doc_path = Arc::new(std::sync::Mutex::new(doc.to_owned()));
        let doc_changed = Arc::new(AtomicBool::new(false));
        let config_changed = Arc::new(AtomicBool::new(false));
        let (path, doc_flag, config_flag) = (doc_path.clone(), doc_changed.clone(), config_changed.clone());
        std::thread::spawn(move || {
            let config_path = Config::path();
            let current = || path.lock().map(|p| p.clone()).unwrap_or_default();
            let mut watched = current();
            let (mut doc_seen, mut config_seen) = (stamp(&watched), stamp(&config_path));
            let mut unsettled = None;
            loop {
                std::thread::sleep(Duration::from_millis(400));
                if current() != watched {
                    watched = current();
                    doc_seen = stamp(&watched);
                    unsettled = None;
                }
                let now = stamp(&watched);
                // A file being written changes between polls: wait until it holds still.
                if now != doc_seen && now.is_some() {
                    if unsettled == Some(now) {
                        doc_seen = now;
                        unsettled = None;
                        doc_flag.store(true, Ordering::Release);
                        ctx.request_repaint();
                    } else {
                        unsettled = Some(now);
                    }
                }
                let config_now = stamp(&config_path);
                if config_now != config_seen {
                    config_seen = config_now;
                    config_flag.store(true, Ordering::Release);
                    ctx.request_repaint();
                }
            }
        });
        Self { doc_path, doc_changed, config_changed }
    }

    fn follow(&self, doc: &Path) {
        if let Ok(mut path) = self.doc_path.lock() {
            doc.clone_into(&mut path);
        }
    }
}

/// Whether the hotbar shows. `tt` always flips what is seen and records the
/// choice in both fields. `asked` is what shows with no tool in hand;
/// `hidden_with_tool` is the hotbar hidden while a tool is in hand, which keeps
/// the tool. Picking a tool shows the hotbar; dropping one leaves it shown only
/// if the last `tt` showed it.
#[derive(Debug, Default, Clone, Copy)]
pub struct HotbarState {
    asked: bool,
    hidden_with_tool: bool,
}

impl HotbarState {
    pub fn visible(&self, tool_in_hand: bool) -> bool {
        if tool_in_hand { !self.hidden_with_tool } else { self.asked }
    }

    pub fn toggle(&mut self, tool_in_hand: bool) {
        let show = !self.visible(tool_in_hand);
        self.asked = show;
        self.hidden_with_tool = !show;
    }

    pub fn tool_picked(&mut self) {
        self.hidden_with_tool = false;
    }

    pub fn tool_dropped(&mut self) {
        self.hidden_with_tool = false;
    }
}

pub struct App {
    pub config: Config,
    pub keymap: Keymap,
    pub pending: Pending,
    pub doc: Doc,
    pub store: Store,
    pub side: Sidecar,
    pub dirty: bool,
    /// The document is a blank stand-in behind the password menu: nothing may be saved.
    pub locked: bool,
    /// No document is open: `doc` is a blank page, and `locked` is set with it.
    pub blank: bool,
    last_save: Instant,

    pub view: View,
    pub render_zoom: f32,
    pub zoom_changed: Instant,
    pub fit: Option<Fit>,
    started: bool,
    start_page: Option<usize>,

    pub cache: TileCache,
    /// The tile requests of this frame, most urgent first.
    pub wants: Vec<RenderRequest>,
    queued: Vec<RenderRequest>,
    queued_text: Vec<usize>,
    /// Pages that took long to render, which wait for zooming to stop.
    slow_pages: HashSet<usize>,

    pub mode: Mode,
    pub search: Option<Search>,
    pub selection: Option<Selection>,
    pub drag_anchor: Option<(usize, usize)>,
    pub tool: Option<Tool>,
    pub hotbar: HotbarState,
    pub color: usize,
    pub stroke_width: f32,
    pub stroke: Option<(usize, Vec<[f32; 2]>)>,
    pub fill: FillItem,
    pub fill_size: f32,
    /// The signature kept for every document, at a width of 1.
    pub signature: Option<Strokes>,
    /// Where the pointer is over the view.
    pub hover: Option<Pos2>,
    /// When and where a click closed the choice panel, to swallow the second click of a double click.
    pub choice_closed: Option<(f64, Pos2)>,
    /// When and where a click opened the choice panel, to ignore the second click of a double click.
    pub choice_opened: Option<(f64, Pos2)>,
    /// Whether the file being opened was picked from the recent list, which a cancelled password returns to.
    pub recent_return: bool,
    pub links_shown: bool,
    pub fields_shown: bool,
    /// A reference waiting for the text of every page, to look up what it points at.
    pub smart: Option<(Reference, Landing)>,
    /// The table of contents was asked for while its printed pages were still being read.
    pub toc_waiting: bool,
    /// What the file chooser picks arrives here, while it is open.
    pub chooser: Option<Receiver<Option<PathBuf>>>,
    pub overview: Option<Overview>,
    /// The references the hints showing stand for.
    pub hint_references: Vec<Reference>,
    pub undo: Vec<Edit>,
    pub redo: Vec<Edit>,
    pub form: FormState,

    pub jumps: Vec<Anchor>,
    pub jump_index: usize,
    pub pending_portal: Option<PendingPortal>,
    pub portal_window: bool,

    pub statusbar_visible: bool,
    pub fullscreen: bool,
    /// Set while presenting: one page fit to the screen, and nothing else.
    pub presentation: Option<crate::present::Presentation>,
    /// Where the view goes back to after leaving the presentation, until the window has settled.
    pub return_to: Option<(Anchor, Instant)>,
    /// The theme the config file names, so a reload only changes it when the file did.
    file_theme: String,
    message: Option<(String, Instant)>,
    watcher: Watcher,
}

/// An opened document with its store and sidecar, prepared before the window exists.
pub struct Opened {
    pub doc: Doc,
    pub store: Store,
    pub side: Sidecar,
    pub load_warning: Option<String>,
    /// Set when the document wants a password: `doc` is then a blank stand-in, and this is the menu.
    pub locked: Option<PasswordState>,
    /// Set when there is no document to open: `doc` is then a blank page, and nothing of it is saved.
    pub blank: bool,
}

impl Opened {
    /// Opens `path`, and loads what is saved for its contents.
    pub fn load(path: &Path, password: Option<&str>, config: &Config) -> Result<Self, String> {
        Self::load_in(store::data_dir(), path, password, config)
    }

    /// `load`, with the sidecars kept in `data_dir`.
    fn load_in(data_dir: PathBuf, path: &Path, password: Option<&str>, config: &Config) -> Result<Self, String> {
        let bytes = Arc::new(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?);
        // The hash and the parsing don't depend on each other.
        let hashing = {
            let bytes = bytes.clone();
            std::thread::spawn(move || store::content_key(&bytes))
        };
        let doc = match Doc::from_bytes(path, bytes, password, config.active_theme(), config.view.page_gap) {
            Ok(doc) => doc,
            Err(failure @ (OpenFailure::NeedsPassword | OpenFailure::WrongPassword)) => {
                let notice = (failure == OpenFailure::WrongPassword).then_some("wrong password");
                return Self::locked(data_dir, path, notice, config);
            }
            // On a parse error the hashing thread is left to finish on its own.
            Err(OpenFailure::Other(e)) => return Err(e),
        };
        let key = hashing.join().map_err(|_| format!("{}: couldn't hash the document", path.display()))?;
        let store = Store::new(data_dir, key);
        let (side, load_warning) = store.load(&doc.path);
        Ok(Self { doc, store, side, load_warning, locked: None, blank: false })
    }

    /// A blank page named `path`, with the password menu over it. Nothing of it is ever saved.
    fn locked(data_dir: PathBuf, path: &Path, notice: Option<&str>, config: &Config) -> Result<Self, String> {
        let bytes = Arc::new(placeholder_pdf());
        let store = Store::new(data_dir, store::content_key(&bytes));
        let doc = Doc::from_bytes(path, bytes, None, config.active_theme(), config.view.page_gap).map_err(|e| e.to_string())?;
        let locked = Some(PasswordState::new(path.to_owned(), notice));
        Ok(Self { doc, store, side: Sidecar::default(), load_warning: None, locked, blank: false })
    }

    /// A blank page standing for no document at all. Nothing of it is ever saved.
    pub fn blank(data_dir: PathBuf, config: &Config) -> Result<Self, String> {
        let mut opened = Self::locked(data_dir, Path::new(""), None, config)?;
        (opened.locked, opened.blank) = (None, true);
        Ok(opened)
    }
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        config: Config,
        opened: Opened,
        font: Option<Vec<u8>>,
        start_page: Option<usize>,
        warning: Option<String>,
    ) -> Self {
        let Opened { doc, store, side, load_warning, locked, blank } = opened;
        if locked.is_none() && !blank {
            store::remember_document(&doc.path);
        }
        let (keymap, key_errors) = Keymap::new(&config.keys);
        let watcher = Watcher::start(cc.egui_ctx.clone(), &doc.path);
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        crate::chrome::apply_font_bytes(&cc.egui_ctx, font);
        let now = Instant::now();
        let (jumps, jump_index) = store::restored_jumps(&side.jumps, side.jump_index, doc.layout.len());
        let mut app = Self {
            statusbar_visible: config.statusbar.visible,
            stroke_width: config.hotbar.stroke_width,
            fill_size: config.hotbar.fill_size,
            links_shown: config.view.highlight_links,
            fields_shown: config.view.highlight_fields,
            file_theme: config.theme.clone(),
            keymap,
            pending: Pending::default(),
            doc,
            store,
            side,
            dirty: false,
            last_save: now,
            view: View::new(),
            render_zoom: 1.0,
            zoom_changed: now,
            fit: Some(Fit::Width),
            started: false,
            start_page: start_page.filter(|_| !blank),
            cache: TileCache::default(),
            wants: Vec::new(),
            queued: Vec::new(),
            queued_text: Vec::new(),
            slow_pages: HashSet::new(),
            locked: locked.is_some() || blank,
            blank,
            mode: locked.map_or(Mode::Normal, Mode::Password),
            search: None,
            selection: None,
            drag_anchor: None,
            tool: None,
            hotbar: HotbarState::default(),
            color: 0,
            stroke: None,
            fill: FillItem::Text,
            signature: store::load_signature(&store::data_dir()),
            hover: None,
            choice_closed: None,
            choice_opened: None,
            recent_return: false,
            smart: None,
            toc_waiting: false,
            chooser: None,
            overview: None,
            hint_references: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            form: FormState::fresh(),
            jumps,
            jump_index,
            pending_portal: None,
            portal_window: false,
            fullscreen: config.view.fullscreen,
            presentation: None,
            return_to: None,
            message: None,
            watcher,
            config,
        };
        if blank {
            app.set_title(&cc.egui_ctx);
            let recent = store::recent_documents();
            if !recent.is_empty() {
                app.mode = Mode::Recent(app.recent_state(recent));
            }
        }
        let problem = match (load_warning, warning) {
            (Some(load), Some(warning)) => Some(format!("{load} · {warning}")),
            (load, warning) => load.or(warning),
        }
        .or_else(|| key_errors.into_iter().next());
        if let Some(problem) = problem {
            app.say(problem);
        }
        app
    }

    /// Shows a message in the status bar for a while.
    pub fn say(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), Instant::now()));
    }

    pub fn message(&self) -> &str {
        self.message.as_ref().map_or("", |(text, _)| text)
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Writes what is kept for the document.
    pub fn save(&mut self) {
        self.side = self.live_side();
        if let Err(e) = save_sidecar(self.locked, &self.store, &self.side) {
            self.say(format!("couldn't save: {e}"));
        }
        self.dirty = false;
        self.last_save = Instant::now();
    }

    /// The sidecar as it is now: the saved one with the current place, zoom and jump history.
    pub fn live_side(&self) -> Sidecar {
        let mut side = self.side.clone();
        let (anchor, zoom) = match &self.presentation {
            Some(presentation) => presentation.kept_place(),
            None => (self.view.anchor(&self.doc.layout), self.view.zoom),
        };
        side.anchor = Some(anchor);
        side.zoom = Some(zoom);
        (side.jumps, side.jump_index) = store::capped_jumps(&self.jumps, self.jump_index);
        side
    }

    /// Sets up the view on the first frame, when the window's size is known.
    fn start(&mut self) {
        self.started = true;
        let restore = self.config.view.restore_position;
        match (restore.then_some(self.side.zoom).flatten(), restore.then_some(self.side.anchor).flatten()) {
            (Some(zoom), Some(anchor)) => {
                self.fit = None;
                self.view.zoom = zoom.clamp(self.config.view.min_zoom, self.config.view.max_zoom);
                self.view.go_to(&self.doc.layout, anchor);
            }
            _ => self.apply_fit(),
        }
        // Behind the password menu the page asked for isn't there yet.
        if !self.locked
            && let Some(page) = self.start_page.take()
        {
            self.view.go_to(&self.doc.layout, Anchor { page, y: 0.0, x: self.view.x });
        }
        self.render_zoom = self.view.zoom;
    }

    /// Fits the current page to the window, if a fit is on.
    pub fn apply_fit(&mut self) {
        let page = self.view.current_page(&self.doc.layout);
        let [w, h] = self.doc.layout.sizes[page];
        let zoom = match self.fit {
            Some(Fit::Width) => self.view.rect.width() * self.config.view.fit_width_ratio / w,
            Some(Fit::Height) => self.view.rect.height() / h,
            None => return,
        };
        self.view.set_zoom(zoom.clamp(self.config.view.min_zoom, self.config.view.max_zoom), &self.doc.layout);
        self.zoom_changed = Instant::now();
    }

    /// Replaces the document with `path`, keeping the view when it is the same file reloaded.
    pub fn open(&mut self, ctx: &egui::Context, path: &Path) -> Result<(), String> {
        let same_file = path == self.doc.path && !self.locked;
        let password = if same_file { self.doc.password().map(str::to_owned) } else { None };
        match Doc::open(path, password.as_deref(), self.config.active_theme(), self.config.view.page_gap) {
            Ok(doc) => self.install(ctx, doc, same_file),
            Err(OpenFailure::NeedsPassword) => self.ask_password(path, None),
            Err(OpenFailure::WrongPassword) => self.ask_password(path, Some("wrong password")),
            Err(OpenFailure::Other(e)) => return Err(e),
        }
        Ok(())
    }

    /// Opens the password menu for `path`, over what is open.
    fn ask_password(&mut self, path: &Path, notice: Option<&str>) {
        self.pending.clear();
        self.toc_waiting = false;
        self.mode = Mode::Password(PasswordState::new(path.to_owned(), notice));
    }

    /// Tries what was typed in the password menu: the document opens, or the menu says why not.
    pub fn password_submit(&mut self, ctx: &egui::Context) {
        let Mode::Password(state) = &self.mode else { return };
        if let Purpose::Owner { job, target, force } = state.purpose.clone() {
            return self.owner_submit(job, target, force, state.field.text.clone());
        }
        if state.purpose != Purpose::Open {
            return self.new_password_submit();
        }
        let (path, password) = (state.path.clone(), state.field.text.clone());
        let same_file = path == self.doc.path && !self.locked;
        let notice = match Doc::open(&path, Some(&password), self.config.active_theme(), self.config.view.page_gap) {
            Ok(doc) => return self.install(ctx, doc, same_file),
            Err(OpenFailure::Other(e)) => e,
            Err(_) => "wrong password".to_owned(),
        };
        self.password_notice(notice);
    }

    fn password_notice(&mut self, notice: String) {
        if let Mode::Password(state) = &mut self.mode {
            state.notice = Some(notice);
        }
    }

    /// The owner password of the document was typed: the copy is written, or the new password asked for.
    fn owner_submit(&mut self, job: Job, target: PathBuf, force: bool, owner: String) {
        if let Err(e) = crate::protect::check_owner(self.doc.bytes.clone(), self.doc.password(), &owner) {
            return self.password_notice(e);
        }
        match job {
            Job::Protect => self.mode = Mode::Password(PasswordState::new_password(target, force, Some(owner), None)),
            Job::Unprotect => self.write_copy(Job::Unprotect, &target, force, Some(&owner), ""),
        }
    }

    /// The new password was typed, or typed again.
    fn new_password_submit(&mut self) {
        let Mode::Password(state) = &self.mode else { return };
        match state.entered() {
            Entered::Notice(notice) => self.password_notice(notice),
            Entered::Next(next) => self.mode = Mode::Password(next),
            Entered::Write { target, force, owner, password } => self.write_copy(Job::Protect, &target, force, owner.as_deref(), &password),
        }
    }

    /// Closes the password menu: a file picked from the recent list returns to it; otherwise the open
    /// document stays, and with none the window closes, unless it shows the blank page.
    pub fn password_cancel(&mut self, ctx: &egui::Context) {
        let opening = matches!(&self.mode, Mode::Password(s) if s.purpose == Purpose::Open);
        if opening && std::mem::take(&mut self.recent_return) {
            self.mode = Mode::Normal;
            self.open_recent();
        } else if self.locked && !self.blank && opening {
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        } else {
            self.mode = Mode::Normal;
        }
    }

    /// Makes `doc`, opened from its own path, the document shown. `same_file` keeps the view, as
    /// for a reload.
    fn install(&mut self, ctx: &egui::Context, doc: Doc, same_file: bool) {
        let path = doc.path.clone();
        let path = path.as_path();
        // Another document starts with the window as it was, not as the presentation made it.
        if !same_file {
            self.end_presentation(ctx);
        }
        self.save();
        self.locked = false;
        self.blank = false;
        let key = store::content_key(&doc.bytes);
        if same_file {
            self.store = match self.store.clone().moved_to(key, &self.side) {
                Ok(moved) => moved,
                Err(e) => {
                    self.say(format!("couldn't save: {e}"));
                    self.store.clone()
                }
            };
            self.search = self.search.take().map(|s| s.restarted(doc.layout.len()));
            (self.jumps, self.jump_index) = store::restored_jumps(&self.jumps, self.jump_index, doc.layout.len());
        } else {
            self.store = self.store.rekeyed(key);
            let (side, warning) = self.store.load(path);
            self.side = side;
            self.search = None;
            (self.jumps, self.jump_index) = store::restored_jumps(&self.side.jumps, self.side.jump_index, doc.layout.len());
            self.undo.clear();
            self.redo.clear();
            self.started = false;
            self.fit = Some(Fit::Width);
            // A document with no saved place starts at the top, not where the last one was.
            self.view.y = 0.0;
            self.view.x = 0.0;
            store::remember_document(path);
            self.watcher.follow(path);
            // A change flagged for the old file says nothing about this one.
            self.watcher.doc_changed.store(false, Ordering::Release);
            if let Some(warning) = warning {
                self.say(warning);
            }
        }
        self.selection = None;
        self.smart = None;
        self.toc_waiting = false;
        self.overview = None;
        self.view.zoom_hold = None;
        self.mode = Mode::Normal;
        self.choice_closed = None;
        self.choice_opened = None;
        self.recent_return = false;
        self.pending.clear();
        self.doc = doc;
        self.form = FormState::fresh();
        self.reset_rendering();
        self.view.clamp(&self.doc.layout);
        self.set_title(ctx);
    }

    /// Forgets everything rendered, for a new document or new colours.
    pub fn reset_rendering(&mut self) {
        self.cache.clear();
        self.forget_queue();
    }

    /// Asks the render workers for everything again, whatever they were sent.
    pub fn forget_queue(&mut self) {
        self.queued.clear();
        self.queued_text.clear();
        self.slow_pages.clear();
    }

    pub fn set_title(&self, ctx: &egui::Context) {
        let title = if self.blank { "yavcpdf".to_owned() } else { format!("{} - yavcpdf", self.doc.file_name()) };
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Title(title));
    }

    /// Picks up a changed config file.
    fn reload_config(&mut self, ctx: &egui::Context) {
        let (mut config, warning) = Config::load();
        if warning.is_some() {
            self.say(warning.unwrap_or_default());
            return;
        }
        // A theme picked by key survives a reload that doesn't change the theme in the file.
        let file_theme = std::mem::replace(&mut self.file_theme, config.theme.clone());
        if config.theme == file_theme && config.themes.contains_key(&self.config.theme) {
            config.theme.clone_from(&self.config.theme);
        }
        let pages_changed = {
            let (old, new) = (self.config.active_theme(), config.active_theme());
            (old.page, old.page_text, old.recolor) != (new.page, new.page_text, new.recolor)
        };
        let font_changed = (&self.config.font, self.config.font_size) != (&config.font, config.font_size);
        let (keymap, errors) = Keymap::new(&config.keys);
        self.keymap = keymap;
        self.pending.clear();
        self.config = config;
        if font_changed {
            crate::chrome::apply_font(ctx, &self.config);
        }
        if pages_changed {
            self.doc.apply_theme(self.config.active_theme());
            self.reset_rendering();
        }
        self.color = self.color.min(self.config.hotbar.colors.len() - 1);
        self.say(errors.into_iter().next().unwrap_or_else(|| "config reloaded".to_owned()));
    }

    fn poll_watcher(&mut self, ctx: &egui::Context) {
        if self.watcher.config_changed.swap(false, Ordering::AcqRel) {
            self.reload_config(ctx);
        }
        // Behind the password menu there is nothing to reload; the change is looked at once it closes.
        if !matches!(self.mode, Mode::Password(_)) && self.watcher.doc_changed.swap(false, Ordering::AcqRel) {
            let path = self.doc.path.clone();
            // A broken file is usually a build in progress: keep showing what is open.
            if let Err(e) = self.open(ctx, &path) {
                self.say(format!("reload failed: {e}"));
            }
        }
    }

    /// Takes in rendered tiles and text.
    fn receive(&mut self, ctx: &egui::Context) {
        let mut uploaded = 0;
        while uploaded < UPLOAD_BYTES_PER_FRAME {
            let Some(done) = self.doc.pool.try_recv() else { break };
            let request = done.request;
            let key = TileKey::new(request.page, request.scale, request.tile);
            if done.error.is_some() || done.width == 0 || done.height == 0 {
                self.cache.mark_failed(key);
                continue;
            }
            if done.millis >= SLOW_MILLIS {
                self.slow_pages.insert(request.page);
            }
            let size = [done.width as usize, done.height as usize];
            uploaded += size[0].saturating_mul(size[1]).saturating_mul(4);
            let texture = ctx.load_texture("tile", texture_image(size, done.rgba), egui::TextureOptions::LINEAR);
            self.cache.insert(key, texture, done.width, done.height);
        }
        while let Some(done) = self.doc.text_pool.try_recv() {
            let page = done.request.page;
            if let Some(slot) = self.doc.text.get_mut(page) {
                // A page whose text can't be read counts as empty, so nothing waits for it.
                *slot = Some(done.text.unwrap_or_default());
            }
        }
    }

    /// Searches pages that have arrived, nearest first, and settles a waiting jump.
    fn pump_search(&mut self) {
        // The search waits behind the presentation, which would have it move the view.
        if self.presentation.is_some() {
            return;
        }
        let Some(search) = self.search.as_mut() else { return };
        let pages = self.doc.layout.len();
        let first = self.view.current_page(&self.doc.layout);
        let began = Instant::now();
        for step in 0..pages {
            let page = (first + step) % pages;
            if let Some(text) = self.doc.text[page].as_ref().filter(|_| !search.is_scanned(page)) {
                search.scan(page, text);
                if began.elapsed() > SEARCH_BUDGET {
                    break;
                }
            }
        }
        let target = search.resolve_jump().map(|m| (m.page, m.rects.first().copied()));
        if let Some((page, Some(rect))) = target {
            self.view.reveal(&self.doc.layout, page, rect);
        }
    }

    /// The pages whose text is wanted, most urgent first. Reading the rest of the document comes last,
    /// and is always on with `prefetch_text`, or else only while a search or smart lookup waits.
    fn wanted_text(&self) -> Vec<usize> {
        let layout = &self.doc.layout;
        let sweep = self.config.view.prefetch_text || self.search.as_ref().is_some_and(|s| !s.is_complete()) || self.smart.is_some();
        if sweep && self.doc.text.iter().all(Option::is_some) {
            return Vec::new();
        }
        let contents: Vec<usize> = self.doc.contents.iter().map(|r| r.page).collect();
        text_order(&self.doc.text, self.view.visible_pages(layout), &contents, self.view.current_page(layout), sweep)
    }

    /// Hands the render workers this frame's requests, when they changed.
    fn send_requests(&mut self) {
        if self.wants != self.queued {
            self.doc.pool.set_queue(self.wants.clone());
            self.queued = self.wants.clone();
        }
        let text = self.wanted_text();
        if text != self.queued_text {
            let request = |&page: &usize| RenderRequest { page, kind: RequestKind::Text, scale: 1.0, ..Default::default() };
            self.doc.text_pool.set_queue(text.iter().map(request).collect());
            self.queued_text = text;
        }
    }

    /// Follows the zoom with the render scale once a held key has let go. A slow page showing waits
    /// longer, as it would never finish if every step restarted it.
    fn settle_zoom(&mut self, ctx: &egui::Context) {
        let waited = self.zoom_changed.elapsed();
        if waited < ZOOM_SETTLE {
            // Prefetching resumes when zooming stops.
            ctx.request_repaint_after(ZOOM_FOLLOW.saturating_sub(waited).max(Duration::from_millis(8)));
        }
        if self.render_zoom == self.view.zoom {
            return;
        }
        let slow = self.view.visible_pages(&self.doc.layout).any(|page| self.slow_pages.contains(&page));
        if waited >= if slow { ZOOM_SETTLE } else { ZOOM_FOLLOW } {
            self.render_zoom = self.view.zoom;
        }
    }

    fn housekeeping(&mut self, ctx: &egui::Context) {
        if self.message.as_ref().is_some_and(|(_, at)| at.elapsed() > MESSAGE_FOR) {
            self.message = None;
        }
        if let Some((_, at)) = &self.message {
            ctx.request_repaint_after(MESSAGE_FOR.saturating_sub(at.elapsed()));
        }
        if self.dirty && self.last_save.elapsed() > SAVE_EVERY {
            self.save();
        } else if self.dirty {
            ctx.request_repaint_after(SAVE_EVERY);
        }
        let (fired, wait) = self.pending.tick(&self.keymap, Instant::now());
        if let Some((action, count)) = fired {
            self.run(ctx, action, count);
        }
        if let Some(wait) = wait {
            ctx.request_repaint_after(wait);
        }
        self.cache.evict(self.config.view.texture_budget_mb.saturating_mul(1024 * 1024));
        // Workers can't wake the UI, so keep looking while anything is on its way.
        let searching = self.search.as_ref().is_some_and(|s| !s.is_complete());
        // Reading the rest of the text ahead of time is not urgent, so it is looked at less often.
        let urgent = searching
            || self.smart.is_some()
            || self.toc_waiting
            || matches!(&self.mode, Mode::Hint(state) if !state.ready && state.purpose.needs_text())
            || self.view.visible_pages(&self.doc.layout).any(|page| self.doc.text[page].is_none());
        let prefetch = self.config.view.prefetch_text;
        if !self.queued.is_empty() || (!prefetch || urgent) && !self.queued_text.is_empty() || searching {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if !self.queued_text.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    /// Lays out the frame: the view, and the status bar under it.
    fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let area = ctx.content_rect();
        let bar = if self.statusbar_visible { crate::chrome::bar_height(&self.config) } else { 0.0 };
        let rect = Rect::from_min_max(area.min, pos2(area.max.x, area.max.y - bar));
        let resized = self.view.rect.size() != rect.size();
        self.view.rect = rect;
        if !self.started {
            self.start();
        } else if resized && self.presentation.is_none() {
            self.apply_fit();
            self.view.clamp(&self.doc.layout);
        }
        ui.painter().rect_filled(area, 0.0, self.config.active_theme().background.egui());
        if self.presentation.is_some() {
            return self.present_frame(ui);
        }
        self.settle_return(&ctx, resized);
        self.pointer(ui);
        self.draw_document(ui);
        self.draw_chrome(ui, Rect::from_min_max(pos2(area.min.x, rect.max.y), area.max));
    }
}

/// The pages without text, most urgent first: those showing and one beyond, then the contents, then
/// with `sweep` the rest from `current` on, wrapping around. Each page comes once.
fn text_order(text: &[Option<Arc<PageText>>], shown: std::ops::RangeInclusive<usize>, contents: &[usize], current: usize, sweep: bool) -> Vec<usize> {
    let count = text.len();
    let mut seen = vec![false; count];
    let mut wanted = Vec::new();
    let mut want = |page: usize| {
        if page < count && text[page].is_none() && !seen[page] {
            seen[page] = true;
            wanted.push(page);
        }
    };
    (shown.start().saturating_sub(1)..=(*shown.end() + 1).min(count.saturating_sub(1))).for_each(&mut want);
    contents.iter().for_each(|&page| want(page));
    if sweep && count > 0 {
        (0..count).for_each(|step| want((current + step) % count));
    }
    wanted
}

/// A rendered raster as texture data. Its words are premultiplied RGBA bytes, which is exactly what
/// `Color32`s are, so the buffer becomes the image without touching a pixel on the UI thread.
fn texture_image(size: [usize; 2], pixels: Pixels) -> egui::ColorImage {
    match bytemuck::allocation::try_cast_vec::<u32, egui::Color32>(pixels.into_words()) {
        Ok(colors) if colors.len() == size[0].saturating_mul(size[1]) => egui::ColorImage::new(size, colors),
        Ok(colors) => egui::ColorImage::from_rgba_premultiplied(size, bytemuck::cast_slice(&colors)),
        Err((_, words)) => egui::ColorImage::from_rgba_premultiplied(size, bytemuck::cast_slice(&words)),
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_watcher(&ctx);
        self.receive(&ctx);
        self.cache.next_frame();
        self.wants.clear();
        let events = ctx.input(|i| i.events.clone());
        self.keys(&ctx, &events);
        self.wheel(&ctx, &events);
        self.pump_search();
        self.pump_smart();
        self.pump_contents();
        self.pump_chooser(&ctx);
        self.pump_form(&ctx);
        self.prepare_hints();
        self.settle_zoom(&ctx);
        self.frame(ui);
        self.portal_viewport(&ctx);
        self.send_requests();
        self.housekeeping(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let [r, g, b] = self.config.active_theme().background.rgb();
        [f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0, 1.0]
    }
}

#[cfg(test)]
mod hotbar_tests {
    use super::HotbarState;

    #[test]
    fn pad_undo_removes_one_stroke_at_a_time() {
        let mut pad = super::SignPad { strokes: vec![vec![[0.0, 0.0]], vec![[0.1, 0.1]]], drawing: true, ..Default::default() };
        pad.undo_stroke();
        assert_eq!(pad.strokes.len(), 1);
        assert!(!pad.drawing);
        assert!(pad.wait_for_release);
        pad.undo_stroke();
        pad.undo_stroke();
        assert!(pad.strokes.is_empty());
    }

    #[test]
    fn pad_undo_waits_for_release_only_when_drawing() {
        let mut pad = super::SignPad { strokes: vec![vec![[0.0, 0.0]]], ..Default::default() };
        pad.undo_stroke();
        assert!(!pad.wait_for_release);
    }

    #[test]
    fn toggle_without_a_tool_shows_and_hides() {
        let mut h = HotbarState::default();
        assert!(!h.visible(false));
        h.toggle(false);
        assert!(h.visible(false));
        h.toggle(false);
        assert!(!h.visible(false));
    }

    #[test]
    fn picking_a_tool_shows_and_toggle_hides_it_keeping_the_tool() {
        let mut h = HotbarState::default();
        h.tool_picked();
        assert!(h.visible(true));
        h.toggle(true);
        assert!(!h.visible(true));
        h.toggle(true);
        assert!(h.visible(true));
    }

    #[test]
    fn picking_again_shows_a_hidden_hotbar() {
        let mut h = HotbarState::default();
        h.tool_picked();
        h.toggle(true);
        h.tool_picked();
        assert!(h.visible(true));
    }

    #[test]
    fn dropping_the_tool_returns_to_the_last_tt_choice() {
        let mut h = HotbarState::default();
        h.tool_picked();
        h.toggle(true);
        h.tool_dropped();
        assert!(!h.visible(false));
        h.toggle(false);
        h.tool_picked();
        h.toggle(true);
        h.tool_dropped();
        assert!(!h.visible(false));
    }

    #[test]
    fn show_pick_hide_drop_leaves_it_hidden() {
        let mut h = HotbarState::default();
        h.toggle(false);
        h.tool_picked();
        assert!(h.visible(true));
        h.toggle(true);
        assert!(!h.visible(true));
        h.tool_dropped();
        assert!(!h.visible(false));
    }

    #[test]
    fn pick_hide_show_drop_leaves_it_visible() {
        let mut h = HotbarState::default();
        h.tool_picked();
        h.toggle(true);
        h.toggle(true);
        assert!(h.visible(true));
        h.tool_dropped();
        assert!(h.visible(false));
    }
}

#[cfg(test)]
mod password_tests {
    use super::{Entered, PasswordState, Purpose, Sidecar, Store, save_sidecar};
    use crate::protect::Job;
    use std::path::PathBuf;

    fn typed(mut state: PasswordState, text: &str) -> PasswordState {
        state.field.insert(text);
        state
    }

    fn target() -> PathBuf {
        PathBuf::from("/a/b/copy.pdf")
    }

    #[test]
    fn each_purpose_has_its_header() {
        let owner = PasswordState::owner(PathBuf::from("/a/b/doc.pdf"), Job::Protect, target(), false);
        assert_eq!(owner.header(), "Owner password for doc.pdf");
        let new = PasswordState::new_password(target(), false, None, None);
        assert_eq!(new.header(), "New password for copy.pdf");
        let Entered::Next(confirm) = typed(new, "pw").entered() else { panic!("no confirmation") };
        assert_eq!(confirm.header(), "Type it again");
        assert_eq!(PasswordState::new(PathBuf::from("/a/x.pdf"), None).purpose, Purpose::Open);
    }

    #[test]
    fn a_new_password_is_typed_twice_and_not_empty() {
        let new = PasswordState::new_password(target(), false, Some("owner".to_owned()), None);
        let Entered::Next(again) = new.entered() else { panic!("empty goes on") };
        assert_eq!(again.notice.as_deref(), Some("the password is empty"));
        assert_eq!(again.header(), "New password for copy.pdf");
        let Entered::Next(confirm) = typed(new, "pw").entered() else { panic!("no confirmation") };
        assert_eq!(confirm.notice, None);
        let Entered::Next(back) = typed(confirm.clone(), "other").entered() else { panic!("mismatch goes back") };
        assert_eq!(back.notice.as_deref(), Some("the passwords don't match"));
        assert!(matches!(&back.purpose, Purpose::New { owner: Some(o), .. } if o == "owner"));
        assert!(back.field.text.is_empty());
        let Entered::Write { target: written, owner, password, .. } = typed(confirm, "pw").entered() else { panic!("matching writes") };
        assert_eq!((written, owner.as_deref(), password.as_str()), (target(), Some("owner"), "pw"));
    }

    #[test]
    fn a_password_that_prepares_to_nothing_is_empty() {
        // A soft hyphen: mapped to nothing by SASLprep.
        let new = typed(PasswordState::new_password(target(), false, None, None), "\u{00AD}");
        let Entered::Next(again) = new.entered() else { panic!("goes back") };
        assert_eq!(again.notice.as_deref(), Some("the password is empty"));
        assert!(matches!(again.purpose, Purpose::New { .. }));
    }

    #[test]
    fn the_force_of_the_command_reaches_the_write() {
        let new = typed(PasswordState::new_password(target(), true, None, None), "a");
        let Entered::Next(confirm) = new.entered() else { panic!("goes on") };
        let Entered::Write { force, .. } = typed(confirm, "a").entered() else { panic!("writes") };
        assert!(force);
    }

    #[test]
    fn no_purpose_prints_a_secret() {
        let secrets = ["s3cret-typed", "s3cret-first", "s3cret-owner"];
        let owner = Some(secrets[2].to_owned());
        let new = PasswordState::new_password(target(), false, owner.clone(), None);
        let confirm = PasswordState {
            purpose: Purpose::Confirm { target: target(), force: false, owner: owner.clone(), first: secrets[1].to_owned() },
            ..new.clone()
        };
        let ask = PasswordState::owner(PathBuf::from("/a/doc.pdf"), Job::Unprotect, target(), false);
        let write = typed(confirm.clone(), secrets[1]).entered();
        for state in [new, confirm, ask, PasswordState::new(target(), None)] {
            let state = typed(state, secrets[0]);
            for shown in [format!("{state:?}"), format!("{:?}", state.purpose), format!("{:?}", state.entered())] {
                assert!(secrets.iter().all(|s| !shown.contains(s)), "{shown}");
            }
        }
        let shown = format!("{write:?}");
        assert!(secrets.iter().all(|s| !shown.contains(s)), "{shown}");
    }

    #[test]
    fn the_protect_sequence_walks_from_new_to_write() {
        let owner = Some("s3cret-owner".to_owned());
        let new = PasswordState::new_password(target(), false, owner.clone(), None);
        assert_eq!(new.header(), "New password for copy.pdf");
        let Entered::Next(empty) = new.entered() else { panic!("empty gives a notice") };
        assert_eq!(empty.notice.as_deref(), Some("the password is empty"));
        assert_eq!(empty.header(), "New password for copy.pdf");
        let Entered::Next(confirm) = typed(new, "a").entered() else { panic!("a goes to confirm") };
        assert!(matches!(&confirm.purpose, Purpose::Confirm { first, .. } if first == "a"));
        assert_eq!(confirm.header(), "Type it again");
        assert!(confirm.field.text.is_empty());
        let Entered::Next(back) = typed(confirm.clone(), "b").entered() else { panic!("mismatch goes back") };
        assert!(matches!(&back.purpose, Purpose::New { .. }));
        assert_eq!(back.notice.as_deref(), Some("the passwords don't match"));
        assert!(back.field.text.is_empty());
        assert_eq!(back.header(), "New password for copy.pdf");
        let Entered::Write { target: t, owner: o, password, .. } = typed(confirm, "a").entered() else { panic!("match writes") };
        assert_eq!((t, o, password.as_str()), (target(), owner, "a"));
    }

    #[test]
    fn no_state_of_the_sequence_prints_what_was_typed_or_the_owner() {
        let secrets = ["zz-typed-1", "zz-owner-2"];
        let owner = Some(secrets[1].to_owned());
        let new = typed(PasswordState::new_password(target(), false, owner, None), secrets[0]);
        let Entered::Next(confirm) = new.entered() else { panic!("goes on") };
        let confirm = typed(confirm, secrets[0]);
        let write = confirm.entered();
        let Entered::Next(mismatch) = typed(PasswordState { field: Default::default(), ..confirm.clone() }, "other").entered() else {
            panic!("goes back")
        };
        for shown in [
            format!("{new:?}"),
            format!("{confirm:?}"),
            format!("{:?}", confirm.purpose),
            format!("{write:?}"),
            format!("{mismatch:?}"),
            format!("{:?}", mismatch.purpose),
        ] {
            assert!(secrets.iter().all(|s| !shown.contains(s)), "{shown}");
        }
    }

    #[test]
    fn a_locked_document_saves_nothing() {
        let dir = std::env::temp_dir().join(format!("yavcpdf-locked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(dir.clone(), "key".to_owned());
        let side = Sidecar::default();
        save_sidecar(true, &store, &side).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "locked must not write anything");
        save_sidecar(false, &store, &side).unwrap();
        assert!(std::fs::read_dir(&dir).unwrap().count() > 0, "unlocked saves");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_menu_names_the_file_and_never_prints_the_password() {
        let mut state = PasswordState::new(PathBuf::from("/a/b/secret.pdf"), Some("wrong password"));
        assert_eq!(state.header(), "Password for secret.pdf");
        state.field.insert("hunter2");
        let shown = format!("{state:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert_eq!(PasswordState::new(PathBuf::from("/a/b/x.pdf"), None).notice, None);
    }
}

#[cfg(test)]
mod locked_load_tests {
    use super::{Opened, Sidecar, Store, save_sidecar};
    use crate::config::Config;
    use crate::doc::{OpenFailure, placeholder_pdf};
    use crate::store;
    use pdfcraft_cos::{Algorithm, Document, NewEncryption, SaveOptions, write_full};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    fn encrypted_bytes(password: &str) -> Vec<u8> {
        let mut doc = Document::open(Arc::new(placeholder_pdf())).unwrap();
        let encryption = NewEncryption {
            algorithm: Algorithm::Aes128,
            user_password: password,
            owner_password: "o",
            permissions: -4,
            encrypt_metadata: true,
            seed: [5; 32],
        };
        doc.set_encryption(&encryption).unwrap();
        write_full(&doc, &SaveOptions::default()).unwrap()
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        names.sort();
        names
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yavcpdf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_blank_page_is_one_page_with_no_password_menu_and_writes_nothing() {
        let data = scratch("blank-data");
        let config = Config::load_str("").unwrap();
        let opened = Opened::blank(data.clone(), &config).unwrap();
        assert!(opened.blank && opened.locked.is_none());
        assert_eq!(opened.doc.layout.len(), 1);
        assert!(listing(&data).is_empty(), "the blank page wrote {:?}", listing(&data));
    }

    #[test]
    fn loading_an_encrypted_file_without_its_password_is_locked_and_writes_nothing() {
        let dir = scratch("files");
        let data = scratch("files-data");
        let file = dir.join("secret.pdf");
        std::fs::write(&file, encrypted_bytes("pw")).unwrap();
        let config = Config::load_str("").unwrap();

        let opened = Opened::load_in(data.clone(), &file, None, &config).unwrap();
        let state = opened.locked.as_ref().expect("locked");
        assert_eq!(state.path, file);
        assert_eq!(state.notice, None);
        assert_eq!(opened.doc.layout.len(), 1);
        assert_eq!(opened.doc.path, file);
        assert!(opened.doc.password().is_none());
        assert_eq!(opened.side, Sidecar::default());
        assert!(opened.load_warning.is_none());

        let wrong = Opened::load_in(data.clone(), &file, Some("nope"), &config).unwrap();
        assert_eq!(wrong.locked.as_ref().unwrap().notice.as_deref(), Some("wrong password"));
        assert!(listing(&data).is_empty(), "a locked load wrote {:?}", listing(&data));

        let right = Opened::load_in(data.clone(), &file, Some("pw"), &config).unwrap();
        assert!(right.locked.is_none());
        assert_eq!(right.doc.password(), Some("pw"));
        assert_eq!(right.side.path, file.display().to_string());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_locked_store_is_keyed_by_the_placeholder_and_the_unlocked_one_by_the_real_file() {
        let dir = scratch("keys");
        let data = scratch("keys-data");
        let file = dir.join("secret.pdf");
        let bytes = encrypted_bytes("pw");
        std::fs::write(&file, &bytes).unwrap();
        let config = Config::load_str("").unwrap();
        let locked = Opened::load_in(data.clone(), &file, None, &config).unwrap();
        let unlocked = Opened::load_in(data.clone(), &file, Some("pw"), &config).unwrap();
        let real_key = store::content_key(&bytes);
        let placeholder_key = store::content_key(&placeholder_pdf());
        assert_ne!(real_key, placeholder_key);
        let side = Sidecar { path: "x".into(), ..Default::default() };
        save_sidecar(true, &Store::new(data.clone(), placeholder_key), &side).unwrap();
        assert!(listing(&data).is_empty());
        save_sidecar(false, &Store::new(data.clone(), real_key.clone()), &side).unwrap();
        assert_eq!(listing(&data), vec![format!("{real_key}.json")]);
        assert!(locked.locked.is_some() && unlocked.locked.is_none());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_garbage_file_is_an_error_not_a_locked_document() {
        let dir = scratch("garbage");
        let data = scratch("garbage-data");
        let file = dir.join("g.pdf");
        std::fs::write(&file, b"nope").unwrap();
        let config = Config::load_str("").unwrap();
        let e = Opened::load_in(data.clone(), &file, None, &config).err().unwrap();
        assert!(e.starts_with(&format!("{}: ", file.display())), "{e}");
        let missing = Opened::load_in(data.clone(), &dir.join("none.pdf"), None, &config).err().unwrap();
        assert!(missing.starts_with(&dir.display().to_string()), "{missing}");
        assert_eq!(OpenFailure::NeedsPassword.to_string(), "the document needs a password");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&data);
    }
}

#[cfg(test)]
mod text_order_tests {
    use super::*;

    fn pages(count: usize, have: &[usize]) -> Vec<Option<Arc<PageText>>> {
        (0..count).map(|page| have.contains(&page).then(|| Arc::new(PageText::default()))).collect()
    }

    #[test]
    fn showing_pages_come_first_then_contents_then_the_sweep_from_the_current_page() {
        let text = pages(10, &[]);
        assert_eq!(text_order(&text, 4..=5, &[0, 9], 4, true), [3, 4, 5, 6, 0, 9, 7, 8, 1, 2]);
    }

    #[test]
    fn the_sweep_wraps_and_skips_pages_already_read() {
        let text = pages(6, &[0, 5]);
        assert_eq!(text_order(&text, 3..=3, &[], 3, true), [2, 3, 4, 1]);
    }

    #[test]
    fn no_page_comes_twice() {
        let text = pages(8, &[]);
        let mut order = text_order(&text, 1..=2, &[2, 3, 3, 7], 2, true);
        order.sort_unstable();
        assert_eq!(order, (0..8).collect::<Vec<_>>());
    }

    #[test]
    fn without_a_sweep_only_showing_and_contents_pages_are_wanted() {
        let text = pages(10, &[]);
        assert_eq!(text_order(&text, 4..=5, &[9], 4, false), [3, 4, 5, 6, 9]);
    }

    #[test]
    fn one_page_is_a_trivial_sweep() {
        assert_eq!(text_order(&pages(1, &[]), 0..=0, &[], 0, true), [0]);
    }

    #[test]
    fn an_empty_document_wants_nothing() {
        assert!(text_order(&pages(0, &[]), 0..=0, &[], 0, true).is_empty());
        assert!(text_order(&pages(0, &[]), 0..=0, &[3], 5, false).is_empty());
    }

    #[test]
    fn showing_the_last_page_clamps_the_neighbour() {
        let text = pages(5, &[]);
        assert_eq!(text_order(&text, 4..=4, &[], 4, false), [3, 4]);
        assert_eq!(text_order(&text, 4..=4, &[], 4, true), [3, 4, 0, 1, 2]);
    }

    #[test]
    fn contents_pages_out_of_range_are_ignored() {
        let text = pages(4, &[]);
        assert_eq!(text_order(&text, 0..=0, &[7, 2, 99], 0, false), [0, 1, 2]);
    }

    #[test]
    fn a_current_page_out_of_range_still_covers_every_page_once() {
        let text = pages(5, &[]);
        let mut order = text_order(&text, 0..=0, &[], 12, true);
        order.sort_unstable();
        assert_eq!(order, [0, 1, 2, 3, 4]);
        assert_eq!(text_order(&text, 0..=0, &[], 4, true), [0, 1, 4, 2, 3]);
    }

    #[test]
    fn when_every_page_is_read_nothing_is_wanted() {
        let text = pages(6, &[0, 1, 2, 3, 4, 5]);
        assert!(text_order(&text, 2..=3, &[0, 5], 2, true).is_empty());
        assert!(text_order(&text, 2..=3, &[0, 5], 2, false).is_empty());
    }

    /// The algorithm `wanted_text` had before `text_order`.
    fn old_order(text: &[Option<Arc<PageText>>], shown: std::ops::RangeInclusive<usize>, contents: &[usize], from: usize, sweep: bool) -> Vec<usize> {
        let n = text.len();
        let missing = |page: &usize| text[*page].is_none();
        let (first, last) = (shown.start().saturating_sub(1), (*shown.end() + 1).min(n - 1));
        let mut wanted: Vec<usize> = (first..=last).filter(missing).collect();
        for page in contents.iter().copied().filter(missing) {
            if !wanted.contains(&page) {
                wanted.push(page);
            }
        }
        if sweep {
            for page in (0..n).map(|s| (from + s) % n).filter(missing) {
                if !wanted.contains(&page) {
                    wanted.push(page);
                }
            }
        }
        wanted
    }

    #[test]
    fn the_order_matches_the_old_algorithm() {
        let mut checked = 0;
        for n in 1..=9usize {
            for mask in [0u32, 0b1, 0b10101, 0b110011010, 0b111111111, 0b100000001] {
                let have: Vec<usize> = (0..n).filter(|p| mask >> p & 1 == 1).collect();
                let text = pages(n, &have);
                for a in 0..n {
                    for b in a..n {
                        for from in 0..n {
                            for contents in [vec![], vec![0], vec![n - 1, 0], vec![n / 2, n / 2, 1 % n]] {
                                for sweep in [false, true] {
                                    assert_eq!(
                                        text_order(&text, a..=b, &contents, from, sweep),
                                        old_order(&text, a..=b, &contents, from, sweep),
                                        "n={n} have={have:?} shown={a}..={b} contents={contents:?} from={from} sweep={sweep}"
                                    );
                                    checked += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(checked > 1000);
    }

    #[test]
    fn a_five_thousand_page_document_is_ordered_quickly() {
        let text = pages(5000, &[]);
        let contents: Vec<usize> = (0..200).collect();
        let start = std::time::Instant::now();
        let mut total = 0;
        for _ in 0..100 {
            total += text_order(&text, 2500..=2501, &contents, 2500, true).len();
        }
        let each = start.elapsed() / 100;
        println!("text_order on 5000 pages: {each:?} per call");
        assert_eq!(total, 5000 * 100);
        assert!(each < std::time::Duration::from_millis(50));
    }
}
