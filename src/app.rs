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
use crate::doc::Doc;
use crate::field::Field;
use crate::form::{FormChange, FormState};
use crate::hint::HintState;
use crate::keymap::{Keymap, Pending};
use crate::overview::Overview;
use crate::search::Search;
use crate::smart::Reference;
use crate::store::{self, Annotation, Sidecar, Store, Strokes};
use crate::view::{Anchor, View};

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
    Choice(ChoiceState),
    AwaitMark(MarkOp),
    Sign(SignPad),
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
    pub links_shown: bool,
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
}

impl Opened {
    /// Opens `path`, and loads what is saved for its contents.
    pub fn load(path: &Path, password: Option<&str>, config: &Config) -> Result<Self, String> {
        let bytes = Arc::new(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?);
        // The hash and the parsing don't depend on each other.
        let hashing = {
            let bytes = bytes.clone();
            std::thread::spawn(move || store::content_key(&bytes))
        };
        let doc = Doc::from_bytes(path, bytes, password, config.active_theme(), config.view.page_gap);
        // On a parse error the hashing thread is left to finish on its own.
        let doc = doc?;
        let key = hashing.join().map_err(|_| format!("{}: couldn't hash the document", path.display()))?;
        let store = Store::new(store::data_dir(), key);
        let (side, load_warning) = store.load(&doc.path);
        Ok(Self { doc, store, side, load_warning })
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
        let Opened { doc, store, side, load_warning } = opened;
        store::remember_document(&doc.path);
        let (keymap, key_errors) = Keymap::new(&config.keys);
        let watcher = Watcher::start(cc.egui_ctx.clone(), &doc.path);
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        crate::chrome::apply_font_bytes(&cc.egui_ctx, font);
        let now = Instant::now();
        let mut app = Self {
            statusbar_visible: config.statusbar.visible,
            stroke_width: config.hotbar.stroke_width,
            fill_size: config.hotbar.fill_size,
            links_shown: config.view.highlight_links,
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
            start_page,
            cache: TileCache::default(),
            wants: Vec::new(),
            queued: Vec::new(),
            queued_text: Vec::new(),
            slow_pages: HashSet::new(),
            mode: Mode::Normal,
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
            smart: None,
            toc_waiting: false,
            chooser: None,
            overview: None,
            hint_references: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            form: FormState::fresh(),
            jumps: Vec::new(),
            jump_index: 0,
            pending_portal: None,
            portal_window: false,
            fullscreen: config.view.fullscreen,
            message: None,
            watcher,
            config,
        };
        let problem = warning.or(load_warning).or_else(|| key_errors.into_iter().next());
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
        self.side.anchor = Some(self.view.anchor(&self.doc.layout));
        self.side.zoom = Some(self.view.zoom);
        if let Err(e) = self.store.save(&self.side) {
            self.say(format!("couldn't save: {e}"));
        }
        self.dirty = false;
        self.last_save = Instant::now();
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
        if let Some(page) = self.start_page.take() {
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
        let same_file = path == self.doc.path;
        let password = if same_file { self.doc.password().map(str::to_owned) } else { None };
        let doc = Doc::open(path, password.as_deref(), self.config.active_theme(), self.config.view.page_gap)?;
        self.save();
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
        } else {
            self.store = self.store.rekeyed(key);
            let (side, warning) = self.store.load(path);
            self.side = side;
            self.search = None;
            self.jumps.clear();
            self.jump_index = 0;
            self.undo.clear();
            self.redo.clear();
            self.started = false;
            self.fit = Some(Fit::Width);
            // A document with no saved place starts at the top, not where the last one was.
            self.view.y = 0.0;
            self.view.x = 0.0;
            store::remember_document(path);
            self.watcher.follow(path);
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
        self.pending.clear();
        self.doc = doc;
        self.form = FormState::fresh();
        self.reset_rendering();
        self.view.clamp(&self.doc.layout);
        self.set_title(ctx);
        Ok(())
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
        ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Title(format!("{} - yavcpdf", self.doc.file_name())));
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
        if self.watcher.doc_changed.swap(false, Ordering::AcqRel) {
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

    /// The pages whose text is wanted, most urgent first.
    fn wanted_text(&self) -> Vec<usize> {
        let layout = &self.doc.layout;
        let missing = |page: &usize| self.doc.text[*page].is_none();
        let shown = self.view.visible_pages(layout);
        let (first, last) = (shown.start().saturating_sub(1), (*shown.end() + 1).min(layout.len() - 1));
        let mut wanted: Vec<usize> = (first..=last).filter(missing).collect();
        for page in self.doc.contents.iter().map(|r| r.page).filter(missing) {
            if !wanted.contains(&page) {
                wanted.push(page);
            }
        }
        if self.search.as_ref().is_some_and(|s| !s.is_complete()) || self.smart.is_some() {
            let from = self.view.current_page(layout);
            let rest = (0..layout.len()).map(|step| (from + step) % layout.len()).filter(missing);
            for page in rest {
                if !wanted.contains(&page) {
                    wanted.push(page);
                }
            }
        }
        wanted
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
        if !self.queued.is_empty() || !self.queued_text.is_empty() || searching {
            ctx.request_repaint_after(Duration::from_millis(16));
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
        } else if resized {
            self.apply_fit();
            self.view.clamp(&self.doc.layout);
        }
        ui.painter().rect_filled(area, 0.0, self.config.active_theme().background.egui());
        self.pointer(ui);
        self.draw_document(ui);
        self.draw_chrome(ui, Rect::from_min_max(pos2(area.min.x, rect.max.y), area.max));
    }
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
