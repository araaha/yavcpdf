//! Interactive forms (AcroForm). The document open in the viewer is never touched: what the user
//! fills in is kept in the sidecar by field name, shown by rendering a filled copy made in
//! memory, and written into a real copy by `:export`.
//!
//! No JavaScript is run, ever. Values go in through `set_values`, which runs no scripts of its
//! own (only the library's native emulation of Acrobat's calculation functions), and a push
//! button only acts on a PDF action that is not a script.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use pdfcraft_cos::ObjRef;
use pdfcraft_cos::{Dict, Document, Object, SaveOptions, write_incremental};
use pdfcraft_forms::{Field, FieldKind, FieldValue, Widget, fields, flags, name_text, set_values, with_field_appearances};
use pdfcraft_render::PageInfo;
use serde::{Deserialize, Serialize};

use crate::app::{App, Bar, BarKind, ChoiceState, Edit, Mode};
use crate::field::Field as TextField;
use crate::hint::{self, Target, TargetKind};

/// What the user put in a field, as the sidecar keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FormValue {
    Text(String),
    Check(bool),
    /// The on-state of the radio button chosen, or none.
    Radio(Option<String>),
    /// Export values of a combo or list box.
    Choice(Vec<String>),
}

impl FormValue {
    /// The value a field has.
    pub fn of(field: &Field) -> Self {
        let first = field.value.first().filter(|v| !v.is_empty() && v.as_str() != "Off");
        match field.kind {
            FieldKind::CheckBox => Self::Check(first.is_some()),
            FieldKind::Radio => Self::Radio(first.cloned()),
            FieldKind::Combo | FieldKind::List => Self::Choice(field.value.iter().filter(|v| !v.is_empty()).cloned().collect()),
            FieldKind::Text | FieldKind::PushButton | FieldKind::Signature => Self::Text(field.value.first().cloned().unwrap_or_default()),
        }
    }

    fn to_field(&self) -> FieldValue {
        match self {
            Self::Text(text) => FieldValue::Text(text.clone()),
            Self::Check(on) => FieldValue::Check(*on),
            Self::Radio(state) => FieldValue::Radio(state.clone()),
            Self::Choice(values) => FieldValue::Choice(values.clone()),
        }
    }
}

/// What clicking a push button does. Read from the button's own PDF action, so that nothing a
/// script says is believed.
#[derive(Clone, Debug, PartialEq)]
pub enum ButtonCmd {
    /// `fields` is `None` when the action names none, which resets every field. Names are fully
    /// qualified; a name given but resolved to nothing stays an empty list.
    Reset {
        fields: Option<Vec<String>>,
        exclude: bool,
    },
    Named(String),
    /// A page of this document, 0-based.
    GoTo(usize),
    Uri(String),
    Submit,
    /// Shows or hides the widgets of `targets`. A target list too long to be read whole is
    /// `Unsupported` instead.
    Hide {
        targets: Vec<HideTarget>,
        hide: bool,
    },
    /// A JavaScript action.
    Script,
    /// Some other action, or one that can't be read.
    Unsupported,
    /// The button has no action.
    Nothing,
}

/// The action dictionary of a widget or field: `/A`, else the mouse-up action `/AA /U`.
fn action_of(doc: &Document, d: &Dict) -> Option<Arc<Object>> {
    let direct = d.get(b"A").cloned();
    let up = || d.get(b"AA").map(|a| doc.resolve(a)).and_then(|aa| aa.as_dict().and_then(|aa| aa.get(b"U").cloned()));
    direct.or_else(up).map(|o| doc.resolve(&o))
}

/// The fully qualified name of the field that `r` (a field or one of its widgets) belongs to,
/// from the partial names (`/T`) up its `/Parent` chain.
fn qualified_name(doc: &Document, r: ObjRef) -> Option<String> {
    let mut parts = Vec::new();
    let mut seen = HashSet::new();
    let mut cur = Some(r);
    while let Some(c) = cur {
        if seen.len() > 64 || !seen.insert(c) {
            break;
        }
        let obj = doc.get(c);
        let d = obj.as_dict()?;
        if let Some(t) = d.get(b"T").and_then(|o| text_of(&doc.resolve(o))) {
            parts.push(t);
        }
        cur = d.get(b"Parent").and_then(Object::as_ref);
    }
    parts.reverse();
    (!parts.is_empty()).then(|| parts.join("."))
}

fn text_of(o: &Object) -> Option<String> {
    match o {
        Object::String(s) => Some(s.to_text()),
        Object::Name(n) => Some(name_text(n)),
        _ => None,
    }
}

/// The entries a reset action's `/Fields` names, as fully qualified names: `Ok(None)` when it
/// names none (absent, or null, which PDF treats as absent), and `Err` when the list is too long
/// to be read whole (a part of it would reset the wrong fields). A string is a name already, and
/// alone it is a list of one; a reference is the field (or widget of one) it points at.
fn reset_targets(doc: &Document, action: &Dict) -> Result<Option<Vec<String>>, ()> {
    let list = match action.get(b"Fields").map(|f| doc.resolve(f)) {
        None => return Ok(None),
        Some(list) if matches!(&*list, Object::Null) => return Ok(None),
        Some(list) => list,
    };
    let name = |o: &Object| match o {
        Object::Ref(r) => match &*doc.get(*r) {
            Object::String(s) => Some(s.to_text()),
            _ => qualified_name(doc, *r),
        },
        other => text_of(other),
    };
    let items: &[Object] = match &*list {
        Object::Array(a) => a.as_slice(),
        single @ Object::String(_) => return Ok(Some(text_of(single).into_iter().collect())),
        _ => &[],
    };
    if items.len() > MAX_TARGETS {
        return Err(());
    }
    Ok(Some(items.iter().filter_map(name).collect()))
}

/// A reset or hide action naming more fields than this is not read.
const MAX_TARGETS: usize = 10_000;

/// What a hide action (`/T`, ISO 32000-2 §12.6.4.11) names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HideTarget {
    /// Every widget of the field with this fully qualified name, and of the fields under it.
    Name(String),
    /// One widget annotation (or a field merged with its only widget).
    Widget(ObjRef),
}

/// The targets of a hide action's `/T`: a field name, a reference to a widget or a field, or an
/// array of them. `Err` when the list is too long to be read whole. An annotation that is no
/// widget, and anything else, names nothing.
fn hide_targets(doc: &Document, action: &Dict) -> Result<Vec<HideTarget>, ()> {
    let Some(list) = action.get(b"T") else { return Ok(Vec::new()) };
    let target = |o: &Object| match o {
        Object::Ref(r) => match &*doc.get(*r) {
            Object::String(s) => Some(HideTarget::Name(s.to_text())),
            Object::Dict(d) if d.name(b"Subtype") == Some(b"Widget") => Some(HideTarget::Widget(*r)),
            Object::Dict(d) if d.get(b"Subtype").is_none() => qualified_name(doc, *r).map(HideTarget::Name),
            _ => None,
        },
        other => text_of(other).map(HideTarget::Name),
    };
    match &*doc.resolve(list) {
        Object::Array(items) if items.len() > MAX_TARGETS => Err(()),
        Object::Array(items) => Ok(items.iter().filter_map(target).collect()),
        _ => Ok(target(list).into_iter().collect()),
    }
}

/// The widgets that hide `targets` name, as their field's name and their place in it, in order
/// and each once. A name is a field's own or one of its ancestors'.
pub fn hide_widgets(all: &[Field], targets: &[HideTarget]) -> Vec<(String, usize)> {
    let names: Vec<String> = targets.iter().filter_map(|t| if let HideTarget::Name(n) = t { Some(n.clone()) } else { None }).collect();
    let named: HashSet<String> = if names.is_empty() { HashSet::new() } else { reset_names(all, Some(&names), false).into_iter().collect() };
    let refs: HashSet<ObjRef> = targets.iter().filter_map(|t| if let HideTarget::Widget(r) = t { Some(*r) } else { None }).collect();
    let mut out = Vec::new();
    for field in all {
        let whole = named.contains(&field.name);
        for (wi, widget) in field.widgets.iter().enumerate() {
            if whole || refs.contains(&widget.obj) {
                out.push((field.name.clone(), wi));
            }
        }
    }
    out
}

/// Widgets a form's show and hide buttons changed, by their field's fully qualified name, then
/// their place in the field: `true` is hidden.
pub type Hidden = BTreeMap<String, BTreeMap<usize, bool>>;

/// The annotation flags Hidden and NoView (ISO 32000-2 §12.5.3, Table 167): a widget with either
/// doesn't show.
const HIDDEN: i64 = 2;
const NO_VIEW: i64 = 32;

/// Shows or hides the widgets `hidden` names, in `doc`, whose fields are `all`. Hiding sets the
/// Hidden flag, and showing clears it and NoView. A widget that is no longer there is left out.
pub fn apply_hidden(doc: &mut Document, all: &[Field], hidden: &Hidden) {
    for field in all {
        let Some(widgets) = hidden.get(&field.name) else { continue };
        for (wi, hide) in widgets {
            let Some(widget) = field.widgets.get(*wi) else { continue };
            let flags = doc.get(widget.obj).as_dict().and_then(|d| d.get(b"F").map(|f| doc.resolve(f))).and_then(|f| f.as_int()).unwrap_or(0);
            let flags = if *hide { flags | HIDDEN } else { flags & !(HIDDEN | NO_VIEW) };
            // A widget that isn't a dictionary can't be changed, and isn't drawn anyway.
            let _ = doc.update_dict(widget.obj, |d| d.set(b"F", Object::Int(flags)));
        }
    }
}

/// The page, 0-based, that a destination (an explicit array, a dictionary with `/D`, or the name
/// of one) is on.
fn dest_page(doc: &Document, dest: &Object, depth: usize) -> Option<usize> {
    if depth > 8 {
        return None;
    }
    let target = match &*doc.resolve(dest) {
        Object::Array(a) => a.first()?.as_ref()?,
        Object::Dict(d) => return dest_page(doc, d.get(b"D")?, depth + 1),
        named @ (Object::String(_) | Object::Name(_)) => return dest_page(doc, &named_dest(doc, &text_of(named)?)?, depth + 1),
        _ => return None,
    };
    pdfcraft_annot::page_refs(doc).ok()?.iter().position(|p| *p == target)
}

/// The destination a name stands for: in the catalog's `/Dests`, else in the `/Names /Dests` tree.
fn named_dest(doc: &Document, name: &str) -> Option<Object> {
    if name.len() > MAX_NAME {
        return None;
    }
    let root = doc.get(doc.root()?);
    let root = root.as_dict()?;
    let old = root.get(b"Dests").map(|d| doc.resolve(d)).and_then(|d| d.as_dict().and_then(|d| d.get(name.as_bytes()).cloned()));
    old.or_else(|| {
        let names = doc.resolve(root.get(b"Names")?);
        let tree = names.as_dict()?.get(b"Dests")?.clone();
        let mut search = Search { budget: SEARCH_BUDGET, seen: HashSet::new() };
        search_tree(doc, &tree, name, 0, &mut search)
    })
}

/// What one name tree lookup may still spend, and the nodes it has read.
struct Search {
    budget: usize,
    seen: HashSet<ObjRef>,
}

impl Search {
    /// Pays for one step; false when the lookup has used up its share.
    fn spend(&mut self) -> bool {
        self.budget = self.budget.saturating_sub(1);
        self.budget > 0
    }
}

/// The longest destination or field name worth looking up or splitting into prefixes.
const MAX_NAME: usize = 4096;

/// What a lookup may examine in all: each node, each `/Kids` entry and each `/Names` pair.
const SEARCH_BUDGET: usize = 100_000;

/// Whether a name tree key is `name`, without making a string of a key that is much too long.
fn key_is(doc: &Document, key: &Object, name: &str) -> bool {
    // A text string takes up to two bytes (UTF-16) and a BOM for each byte of its UTF-8.
    let fits = |len: usize| len <= 2 * name.len() + 4;
    match &*doc.resolve(key) {
        Object::String(s) if fits(s.bytes.len()) => {
            if s.bytes.is_ascii() {
                s.bytes == name.as_bytes()
            } else {
                s.to_text() == name
            }
        }
        Object::Name(n) if fits(n.len()) => name_text(n) == name,
        _ => false,
    }
}

/// A name tree lookup, bounded in depth and in work done, reading each node once.
fn search_tree(doc: &Document, node: &Object, name: &str, depth: usize, search: &mut Search) -> Option<Object> {
    if depth > 16 || !search.spend() {
        return None;
    }
    if let Object::Ref(r) = node
        && !search.seen.insert(*r)
    {
        return None;
    }
    let node = doc.resolve(node);
    let node = node.as_dict()?;
    if let Some(pairs) = node.get(b"Names").map(|n| doc.resolve(n)) {
        for pair in pairs.as_array()?.chunks(2) {
            if !search.spend() {
                return None;
            }
            if pair.len() == 2 && key_is(doc, &pair[0], name) {
                return Some(pair[1].clone());
            }
        }
    }
    let kids = node.get(b"Kids").map(|k| doc.resolve(k))?;
    for kid in kids.as_array()? {
        if !search.spend() {
            return None;
        }
        if let Some(found) = search_tree(doc, kid, name, depth + 1, search) {
            return Some(found);
        }
    }
    None
}

/// What widget `widget` of the push button `field` does, read from the one action dictionary that
/// the widget (else the field) has. Nothing comes from the library's reading of the field, which
/// may be another action, and a script (`/S /JavaScript`) is never believed, however much it
/// looks like a page turn or a reset.
pub fn button_command(doc: &Document, field: &Field, widget: usize) -> ButtonCmd {
    let on_widget = field.widgets.get(widget).and_then(|w| doc.get(w.obj).as_dict().and_then(|d| action_of(doc, d)));
    let action = on_widget.or_else(|| doc.get(field.obj).as_dict().and_then(|d| action_of(doc, d)));
    let Some(action) = action else { return ButtonCmd::Nothing };
    let Some(a) = action.as_dict() else { return ButtonCmd::Unsupported };
    let get = |key: &[u8]| a.get(key).map(|o| doc.resolve(o));
    match a.name(b"S") {
        Some(b"JavaScript") => ButtonCmd::Script,
        Some(b"ResetForm") => {
            let exclude = get(b"Flags").and_then(|f| f.as_int()).unwrap_or(0) & 1 != 0;
            reset_targets(doc, a).map_or(ButtonCmd::Unsupported, |fields| ButtonCmd::Reset { fields, exclude })
        }
        Some(b"Named") => get(b"N").and_then(|n| text_of(&n)).map_or(ButtonCmd::Unsupported, ButtonCmd::Named),
        Some(b"URI") => match get(b"URI").as_deref() {
            Some(Object::String(s)) => ButtonCmd::Uri(s.to_text()),
            _ => ButtonCmd::Unsupported,
        },
        Some(b"GoTo") => a.get(b"D").and_then(|d| dest_page(doc, d, 0)).map_or(ButtonCmd::Unsupported, ButtonCmd::GoTo),
        Some(b"SubmitForm") => ButtonCmd::Submit,
        Some(b"Hide") => {
            // `/H` is true unless it is false (ISO 32000-2 Table 205).
            let hide = !matches!(get(b"H").as_deref(), Some(Object::Bool(false)));
            hide_targets(doc, a).map_or(ButtonCmd::Unsupported, |targets| ButtonCmd::Hide { targets, hide })
        }
        _ => ButtonCmd::Unsupported,
    }
}

/// How the form was last read: what the UI needs to pick and edit fields.
#[derive(Debug, Default)]
pub struct FormInfo {
    /// The fields as they show, with the user's values in.
    pub fields: Vec<Field>,
    /// The value each field has in the document itself.
    pub base: HashMap<String, FormValue>,
    /// The widgets the document itself hides, by field name and place in the field.
    pub base_hidden: HashSet<(String, usize)>,
    /// The view rectangles of the widgets that can be picked (not hidden or read-only), by page.
    pub widget_rects: Vec<Vec<[f32; 4]>>,
    /// The form has been read once.
    pub ready: bool,
}

impl FormInfo {
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// The result of filling a document: the form as it reads, and what to render from.
#[derive(Debug, Default)]
pub struct Built {
    /// The bytes to render, when they are not the document's own.
    pub render: Option<Vec<u8>>,
    pub fields: Vec<Field>,
    pub base: HashMap<String, FormValue>,
    pub base_hidden: HashSet<(String, usize)>,
    /// Values that couldn't be put in, with why.
    pub failed: Vec<(String, String)>,
    /// The values that were tried, to tell a failure of one from a newer value of the same field.
    pub tried: BTreeMap<String, FormValue>,
    /// Why recalculating the calculated fields failed, if it did.
    pub recalc: Option<String>,
}

/// What `fill` made of the values.
pub type Filled = (usize, Vec<(String, String)>, Option<String>);

/// Puts `values` into `doc`: how many went in, each that didn't with why, and why recalculating
/// failed if it did (which no one value is to blame for).
pub fn fill(doc: &mut Document, values: &BTreeMap<String, FormValue>) -> Filled {
    if values.is_empty() {
        return (0, Vec::new(), None);
    }
    let list: Vec<(String, FieldValue)> = values.iter().map(|(name, value)| (name.clone(), value.to_field())).collect();
    match set_values(doc, &list) {
        Ok(results) => {
            let failed: Vec<(String, String)> =
                list.iter().zip(results).filter_map(|((name, _), result)| result.err().map(|e| (name.clone(), e.to_string()))).collect();
            (list.len() - failed.len(), failed, None)
        }
        // The recalculation failed, so the library gives no result per value, and none of them
        // counts as filled (though each was written). Trying them one by one with `set_value`
        // would not tell the two apart (it folds the recalculation into each value's result) and
        // reads the whole form twice per value, which takes minutes on a large one.
        Err(e) => (0, Vec::new(), Some(e.to_string())),
    }
}

fn build_inner(bytes: Arc<Vec<u8>>, password: Option<&str>, values: &BTreeMap<String, FormValue>, hidden: &Hidden) -> Result<Built, String> {
    let mut doc = Document::open_with_password(bytes, password).map_err(|e| format!("{e:?}"))?;
    let before = fields(&doc);
    if before.is_empty() {
        return Ok(Built::default());
    }
    let base = before.iter().map(|f| (f.name.clone(), FormValue::of(f))).collect();
    let base_hidden = hidden_widgets(&before);
    let (_, failed, recalc) = fill(&mut doc, values);
    apply_hidden(&mut doc, &before, hidden);
    let untouched = values.is_empty() && hidden.is_empty();
    let now = if untouched { before } else { fields(&doc) };
    // Fields that came with no look of their own, or that the form leaves for a viewer to draw.
    let shown = with_field_appearances(&doc);
    let render = if untouched && shown.is_none() {
        None
    } else {
        Some(write_incremental(shown.as_ref().unwrap_or(&doc), &SaveOptions::default()).map_err(|e| format!("{e:?}"))?)
    };
    Ok(Built { render, fields: now, base, base_hidden, failed, tried: values.clone(), recalc })
}

/// The widgets of `all` that don't show, by field name and place in the field.
fn hidden_widgets(all: &[Field]) -> HashSet<(String, usize)> {
    let each = |f: &Field| f.widgets.iter().enumerate().filter(|(_, w)| w.hidden).map(|(wi, _)| (f.name.clone(), wi)).collect::<Vec<_>>();
    all.iter().flat_map(each).collect()
}

/// Reads the form of a document, fills it with `values` and shows or hides the widgets `hidden`
/// names. The document came from a stranger, and a panic here must not take the viewer with it.
pub fn build(bytes: Arc<Vec<u8>>, password: Option<&str>, values: &BTreeMap<String, FormValue>, hidden: &Hidden) -> Result<Built, String> {
    catch_unwind(AssertUnwindSafe(|| build_inner(bytes, password, values, hidden))).map_err(|_| "the form could not be read".to_owned())?
}

/// What an export did with the form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub filled: usize,
    pub skipped: usize,
    /// Why recalculating the calculated fields failed, if it did: the values were written, but
    /// none is counted as filled.
    pub recalc: Option<String>,
}

/// Whether the widget takes input.
pub fn usable(widget: &Widget) -> bool {
    widget.page.is_some() && !widget.hidden && !widget.read_only
}

/// A widget's rectangle in view points: top-left origin, as the rest of the viewer has it.
pub fn view_rect(page: &PageInfo, rect: [f64; 4]) -> [f32; 4] {
    let (a, b) = (page.user_to_view(rect[0] as f32, rect[1] as f32), page.user_to_view(rect[2] as f32, rect[3] as f32));
    [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])]
}

/// The view rectangles of the widgets that can be picked (not hidden or read-only), one list per page.
pub fn widget_rects_by_page(fields: &[Field], pages: &[PageInfo]) -> Vec<Vec<[f32; 4]>> {
    let mut out = vec![Vec::new(); pages.len()];
    for widget in fields.iter().flat_map(|f| &f.widgets).filter(|w| usable(w)) {
        let Some(page) = widget.page else { continue };
        if let (Some(info), Some(rects)) = (pages.get(page), out.get_mut(page)) {
            rects.push(view_rect(info, widget.rect));
        }
    }
    out
}

/// The text of a field as one line, for the bar.
pub fn one_line(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

/// The text of a field as the bar shows it: a multiline field keeps its line breaks, as the two
/// characters `\n`, and its backslashes are doubled (`\\`); any other is on one line.
pub fn bar_text(text: &str, multiline: bool) -> String {
    if multiline { text.replace("\r\n", "\n").replace('\r', "\n").replace('\\', "\\\\").replace('\n', "\\n") } else { one_line(text) }
}

/// What was typed in the bar, as the text of the field: for a multiline field, `\n` is a line
/// break and `\\` a backslash. A backslash before anything else stays as it is.
pub fn field_text(typed: &str, multiline: bool) -> String {
    if !multiline {
        return typed.to_owned();
    }
    let mut out = String::with_capacity(typed.len());
    let mut chars = typed.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('\\', Some('n')) => out.push('\n'),
            ('\\', Some('\\')) => out.push('\\'),
            _ => {
                out.push(c);
                continue;
            }
        }
        chars.next();
    }
    out
}

/// Whether `text` fits the field's `/MaxLen`.
pub fn check_length(field: &Field, text: &str) -> Result<(), String> {
    match field.max_len {
        Some(max) if text.chars().count() > max => Err(format!("this field takes {max} characters at most, not {}", text.chars().count())),
        _ => Ok(()),
    }
}

/// A row of the choice panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceRow {
    /// "(none)": clears the choice. Single-select only.
    Clear,
    /// The option at this index of the field's options.
    Option(usize),
    /// A current value that is no option's export value: the one at this index of `strangers`.
    Stranger(usize),
    /// What was typed in the filter, taken as the value (editable combo boxes).
    Free,
}

/// Moving the highlight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Next,
    Previous,
    PageDown,
    PageUp,
    First,
    Last,
}

/// Where the highlight goes from row `at` of `len` rows.
pub fn moved(at: usize, len: usize, by: Move) -> usize {
    let last = len.saturating_sub(1);
    match by {
        Move::Next => (at + 1).min(last),
        Move::Previous => at.saturating_sub(1),
        Move::PageDown => (at + 10).min(last),
        Move::PageUp => at.saturating_sub(10),
        Move::First => 0,
        Move::Last => last,
    }
}

/// The first row shown after the highlight moved to `at`, scrolling as little as it takes.
pub fn keep_visible(first: usize, at: usize, fits: usize, len: usize) -> usize {
    let first = if at < first {
        at
    } else if at >= first + fits {
        at + 1 - fits
    } else {
        first
    };
    first.min(len.saturating_sub(fits))
}

/// The first row shown after the wheel moved the list by `rows` (positive is down).
pub fn scrolled(first: usize, rows: isize, fits: usize, len: usize) -> usize {
    first.saturating_add_signed(rows).min(len.saturating_sub(fits))
}

/// The wheel's step: `pixels_y` (positive is up) added to the unspent `rest`, in rows. Returns
/// the first row shown and the rest, the fraction of a row not yet scrolled. A scroll that
/// reaches or is stopped at either end clears the rest, so none is kept in reserve.
pub fn wheel_step(first: usize, rest: f32, pixels_y: f32, row_height: f32, fits: usize, len: usize) -> (usize, f32) {
    let total = rest - pixels_y / row_height;
    if !total.is_finite() || !row_height.is_finite() || row_height <= 0.0 {
        return (first, 0.0);
    }
    let rows = total.trunc();
    let to = scrolled(first, rows as isize, fits, len);
    let last = len.saturating_sub(fits);
    if (to == 0 && total < 0.0) || (to == last && total > 0.0) { (to, 0.0) } else { (to, total - rows) }
}

/// The highlight `selected` moved into the rows shown, `first..first + fits` within `len` rows.
pub fn clamp_selected(selected: usize, first: usize, fits: usize, len: usize) -> usize {
    let last = (first + fits.max(1) - 1).min(len.saturating_sub(1));
    selected.clamp(first.min(last), last)
}

/// `text` cut to at most `max` characters, with an ellipsis when it was longer.
fn truncated(text: String, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text,
    }
}

impl ChoiceState {
    /// The panel for `field`, with `current` (export values) chosen and the highlight on the
    /// first of them. A current value that is no option's export value gets a row of its own.
    pub fn new(field: &Field, current: Vec<String>) -> Self {
        let mut current: Vec<String> = current.into_iter().filter(|v| !v.is_empty()).collect();
        let mut seen = HashSet::new();
        current.retain(|v| seen.insert(v.clone()));
        let mut strangers: Vec<String> = Vec::new();
        for value in &current {
            if !field.options.iter().any(|(e, _)| e == value) && !strangers.contains(value) {
                strangers.push(value.clone());
            }
        }
        let title = field.tooltip.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| field.name.clone());
        let title = title.lines().next().map(str::trim).filter(|t| !t.is_empty()).unwrap_or(&field.name).to_owned();
        let title = truncated(title, 200);
        let mut state = Self {
            name: field.name.clone(),
            title,
            options: field.options.clone(),
            multi: field.kind == FieldKind::List && field.has(flags::MULTI_SELECT),
            editable: field.kind == FieldKind::Combo && field.has(flags::EDIT),
            initial: current.clone(),
            marked: current,
            strangers,
            selected: 0,
            first: 0,
            wheel_rest: 0.0,
            filter: TextField::default(),
            filtering: false,
        };
        state.selected = state.rows().iter().position(|r| state.ticked(*r)).unwrap_or(0);
        state
    }

    /// The rows the filter lets through. A single-select panel starts with "(none)" while nothing
    /// is typed, then come the foreign values, then the options, and an editable combo box ends
    /// with the typed text when no row is exactly it.
    pub fn rows(&self) -> Vec<ChoiceRow> {
        let typed = self.filter.text.trim();
        let lower = typed.to_lowercase();
        let mut rows = Vec::new();
        if !self.multi && typed.is_empty() {
            rows.push(ChoiceRow::Clear);
        }
        rows.extend(self.strangers.iter().enumerate().filter(|(_, v)| v.to_lowercase().contains(&lower)).map(|(i, _)| ChoiceRow::Stranger(i)));
        rows.extend(self.options.iter().enumerate().filter(|(_, (_, d))| d.to_lowercase().contains(&lower)).map(|(i, _)| ChoiceRow::Option(i)));
        if self.editable && !typed.is_empty() && !self.options.iter().any(|(_, d)| d == typed) && !self.strangers.iter().any(|v| v == typed) {
            rows.push(ChoiceRow::Free);
        }
        rows
    }

    /// The row the highlight is on, if there is one.
    pub fn highlighted(&self) -> Option<ChoiceRow> {
        self.rows().get(self.selected).copied()
    }

    /// What a row says.
    pub fn label(&self, row: ChoiceRow) -> String {
        match row {
            ChoiceRow::Clear => "(none)".to_owned(),
            ChoiceRow::Option(i) => self.options.get(i).map_or_else(String::new, |(_, d)| d.clone()),
            ChoiceRow::Stranger(i) => self.strangers.get(i).cloned().unwrap_or_default(),
            ChoiceRow::Free => format!("use \"{}\"", self.filter.text.trim()),
        }
    }

    /// Whether a row is a chosen one, to be ticked.
    pub fn ticked(&self, row: ChoiceRow) -> bool {
        match row {
            ChoiceRow::Option(i) => self.options.get(i).is_some_and(|(e, _)| self.marked.contains(e)),
            ChoiceRow::Stranger(i) => self.strangers.get(i).is_some_and(|v| self.marked.contains(v)),
            ChoiceRow::Clear | ChoiceRow::Free => false,
        }
    }

    /// Moves the highlight.
    pub fn go(&mut self, by: Move) {
        self.selected = moved(self.selected, self.rows().len(), by);
    }

    /// Changes the filter with `edit`, keeping the highlight on its row if that stays shown.
    pub fn refilter(&mut self, edit: impl FnOnce(&mut Self)) {
        let kept = self.highlighted();
        edit(self);
        self.selected = kept.and_then(|k| self.rows().iter().position(|r| *r == k)).unwrap_or(0);
    }

    /// Whether a filter that lets no row through leaves nothing to choose from.
    pub fn nothing_to_choose(&self) -> bool {
        !self.filter.is_empty() && self.rows().is_empty()
    }

    /// Marks or unmarks an option or a foreign value; for a multi-select list only.
    pub fn toggle(&mut self, row: ChoiceRow) {
        let value = match row {
            ChoiceRow::Option(i) => self.options.get(i).map(|(e, _)| e.clone()),
            ChoiceRow::Stranger(i) => self.strangers.get(i).cloned(),
            ChoiceRow::Clear | ChoiceRow::Free => None,
        };
        let Some(value) = value.filter(|_| self.multi) else { return };
        match self.marked.iter().position(|m| *m == value) {
            Some(at) => {
                self.marked.remove(at);
            }
            None => self.marked.push(value),
        }
    }

    /// The value that choosing a row of a single-select panel stores.
    pub fn value_of(&self, row: ChoiceRow) -> FormValue {
        match row {
            ChoiceRow::Clear => FormValue::Choice(Vec::new()),
            ChoiceRow::Option(i) => FormValue::Choice(self.options.get(i).map(|(e, _)| e.clone()).into_iter().collect()),
            ChoiceRow::Stranger(i) => FormValue::Choice(self.strangers.get(i).cloned().into_iter().collect()),
            ChoiceRow::Free => FormValue::Choice(vec![self.filter.text.trim().to_owned()]),
        }
    }

    /// The value that committing the marked options of a multi-select list stores: in the order
    /// of the options, then any marked value that is none of them, each once.
    pub fn marked_value(&self) -> FormValue {
        let mut values: Vec<String> = Vec::new();
        let in_order = self.options.iter().map(|(e, _)| e).filter(|e| self.marked.contains(e));
        for value in in_order.chain(self.marked.iter().filter(|m| !self.options.iter().any(|(e, _)| e == *m))) {
            if !values.contains(value) {
                values.push(value.clone());
            }
        }
        FormValue::Choice(values)
    }

    /// Whether the marks are the set the field had on opening.
    fn marks_unchanged(&self) -> bool {
        let set = |v: &[String]| v.iter().cloned().collect::<HashSet<String>>();
        set(&self.marked) == set(&self.initial)
    }

    /// What Enter stores: the marked set of a multi-select list, unless it is the one the field
    /// had, else the highlighted row.
    pub fn commit(&self, row: Option<ChoiceRow>) -> Option<FormValue> {
        if self.multi { (!self.marks_unchanged()).then(|| self.marked_value()) } else { row.map(|r| self.value_of(r)) }
    }

    /// A click on row `at` of the rows shown: it becomes the highlight, and marks the row in a
    /// multi-select list or gives the value a single-select panel stores. The list stays where it is.
    pub fn click(&mut self, at: usize) -> Option<FormValue> {
        let row = *self.rows().get(at)?;
        self.selected = at;
        if self.multi {
            self.toggle(row);
            None
        } else {
            self.commit(Some(row))
        }
    }
}

/// The fields a reset action touches: the ones it names, or all but them, or all when it names
/// none at all. Names given that match nothing touch nothing (or everything, when excluding). A
/// name is a field's own or one of its ancestors' (the dotted prefixes of its name).
pub fn reset_names(all: &[Field], names: Option<&[String]>, exclude: bool) -> Vec<String> {
    let Some(names) = names else { return all.iter().map(|f| f.name.clone()).collect() };
    let wanted: HashSet<&str> = names.iter().map(String::as_str).collect();
    let longest = wanted.iter().map(|w| w.len()).max().unwrap_or(0);
    let named = |f: &Field| {
        wanted.contains(f.name.as_str())
            || (f.name.len() <= MAX_NAME && f.name.match_indices('.').take_while(|&(i, _)| i <= longest).any(|(i, _)| wanted.contains(&f.name[..i])))
    };
    all.iter().filter(|f| named(f) != exclude).map(|f| f.name.clone()).collect()
}

// What the user does with a form, in the app.

/// What the bar is editing, when it is editing a field.
#[derive(Clone, Debug)]
pub struct FormEdit {
    pub name: String,
    /// The text the bar opened with, to tell a change from none.
    pub shown: String,
    pub prompt: String,
    /// A multiline field: its line breaks are `\n` in the bar.
    pub multiline: bool,
}

/// How the app is getting on with the form.
#[derive(Default)]
pub struct FormState {
    job: Option<Receiver<Result<Built, String>>>,
    /// The values changed, or the document did, since the form was last read.
    dirty: bool,
    pub edit: Option<FormEdit>,
}

impl FormState {
    /// For a new document: the form has to be read.
    pub fn fresh() -> Self {
        Self { dirty: true, ..Self::default() }
    }
}

/// A change to one field's stored value, for undo.
#[derive(Clone, Debug)]
pub struct FormChange {
    pub name: String,
    pub before: Option<FormValue>,
    pub after: Option<FormValue>,
}

/// A change to whether one widget shows, for undo: `None` is as the document has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HideChange {
    pub name: String,
    pub widget: usize,
    pub before: Option<bool>,
    pub after: Option<bool>,
}

/// What showing (`hide` false) or hiding the widgets `keys` changes, given what the document
/// hides itself and what is stored. A widget already so is left out, and one put back as the
/// document has it is stored as no change at all.
pub fn hide_changes(keys: &[(String, usize)], hide: bool, base_hidden: &HashSet<(String, usize)>, stored: &Hidden) -> Vec<HideChange> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for key in keys {
        if !seen.insert(key) {
            continue;
        }
        let (name, widget) = (key.0.clone(), key.1);
        let own = base_hidden.contains(key);
        let before = stored.get(&name).and_then(|w| w.get(&widget)).copied();
        if before.unwrap_or(own) == hide {
            continue;
        }
        let after = (own != hide).then_some(hide);
        out.push(HideChange { name, widget, before, after });
    }
    out
}

/// Puts `change.after` into `stored`, dropping a field left with nothing.
fn store_hidden(stored: &mut Hidden, name: &str, widget: usize, value: Option<bool>) {
    match value {
        Some(hide) => {
            stored.entry(name.to_owned()).or_default().insert(widget, hide);
        }
        None => {
            if let Some(widgets) = stored.get_mut(name) {
                widgets.remove(&widget);
                if widgets.is_empty() {
                    stored.remove(name);
                }
            }
        }
    }
}

/// Drops the newest undo step's change of `name` to `value`: it was rejected, and undoing or
/// redoing it would only bring it back. Gives the value the change replaced (`Some(None)` when
/// there was none), or `None` when no step holds the change.
fn forget_change(undo: &mut Vec<Edit>, name: &str, value: Option<&FormValue>) -> Option<Option<FormValue>> {
    let rejected = |c: &FormChange| c.name == name && c.after.as_ref() == value;
    let at = undo.iter().rposition(|e| matches!(e, Edit::Form(c) if c.iter().any(rejected)))?;
    let Edit::Form(changes) = &mut undo[at] else { return None };
    let before = changes.remove(changes.iter().position(rejected)?).before;
    changes.retain(|c| !rejected(c));
    if changes.is_empty() {
        undo.remove(at);
    }
    Some(before)
}

impl App {
    /// Reads and fills the form in the background when it needs it, and takes in what it made.
    pub fn pump_form(&mut self, ctx: &egui::Context) {
        if let Some(job) = &self.form.job {
            match job.try_recv() {
                Ok(result) => {
                    self.form.job = None;
                    self.form_arrived(result);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.form.job = None;
                    self.doc.form.ready = true;
                    self.say("the form could not be read");
                }
            }
        }
        if self.form.dirty && self.form.job.is_none() {
            self.form.dirty = false;
            self.start_form_job(ctx);
        }
    }

    fn start_form_job(&mut self, ctx: &egui::Context) {
        let (bytes, password) = (self.doc.bytes.clone(), self.doc.password().map(str::to_owned));
        let (values, hidden) = (self.side.form.clone(), self.side.hidden.clone());
        let (send, receive) = std::sync::mpsc::channel();
        let repaint = ctx.clone();
        let work = move || {
            // The viewer may have moved on to another document by now, with nothing left to tell.
            let _ = send.send(build(bytes, password.as_deref(), &values, &hidden));
            repaint.request_repaint();
        };
        match std::thread::Builder::new().name("form".to_owned()).spawn(work) {
            Ok(_) => self.form.job = Some(receive),
            Err(e) => {
                // Not read, but not waited for either.
                self.doc.form.ready = true;
                self.say(format!("form: {e}"));
            }
        }
    }

    fn form_arrived(&mut self, result: Result<Built, String>) {
        let built = match result {
            Ok(built) => built,
            Err(e) => {
                self.doc.form.ready = true;
                self.say(format!("form: {e}"));
                return;
            }
        };
        if let Some((name, why)) = built.failed.first().cloned() {
            // A value that doesn't go in would fail every time: give it up, unless it has been
            // replaced since by one that wasn't tried. What it replaced comes back; if that fails
            // as well, it goes the next round, with no undo step left to find it in.
            let mut removed = false;
            for (name, _) in &built.failed {
                let tried = built.tried.get(name);
                if tried.is_some() && self.side.form.get(name) == tried {
                    // Put back what the value replaced, when that is known.
                    match self.forget_form_change(name, tried).flatten() {
                        Some(before) => self.side.form.insert(name.clone(), before),
                        None => self.side.form.remove(name),
                    };
                    removed = true;
                }
            }
            if removed {
                self.form_changed();
            }
            self.say(format!("{name}: {why}"));
        } else if let Some(why) = &built.recalc {
            // Not the fault of any one value, and the same again on a second try.
            self.say(format!("the calculated fields: {why}"));
        }
        let Built { render, fields, base, base_hidden, .. } = built;
        let widget_rects = widget_rects_by_page(&fields, &self.doc.info.pages);
        self.doc.form = FormInfo { fields, base, base_hidden, widget_rects, ready: true };
        let bytes = match render {
            Some(bytes) => Arc::new(bytes),
            None if Arc::ptr_eq(&self.doc.render_bytes, &self.doc.bytes) => return,
            None => self.doc.bytes.clone(),
        };
        self.doc.show(bytes, self.config.active_theme());
        // What shows stays until the new pages are in.
        self.cache.retire();
        self.forget_queue();
    }

    fn forget_form_change(&mut self, name: &str, value: Option<&FormValue>) -> Option<Option<FormValue>> {
        forget_change(&mut self.undo, name, value)
    }

    /// The value a field has now: what the user stored, which may not be in `doc.form` yet, else
    /// the document's own (`field` may be from a filled copy that is about to be read again).
    fn current_value(&self, field: &Field) -> FormValue {
        let stored = self.side.form.get(&field.name).or_else(|| self.doc.form.base.get(&field.name));
        stored.cloned().unwrap_or_else(|| FormValue::of(field))
    }

    /// Changes stored values as one undoable step, and shows them.
    fn change_form(&mut self, changes: Vec<(String, Option<FormValue>)>) {
        let mut done = Vec::new();
        for (name, new) in changes {
            // A value the document has already is not a change to keep.
            let new = new.filter(|v| self.doc.form.base.get(&name) != Some(v));
            let before = self.side.form.get(&name).cloned();
            if before == new {
                continue;
            }
            match &new {
                Some(value) => self.side.form.insert(name.clone(), value.clone()),
                None => self.side.form.remove(&name),
            };
            done.push(FormChange { name, before, after: new });
        }
        if done.is_empty() {
            return;
        }
        self.undo.push(Edit::Form(done));
        self.redo.clear();
        self.form_changed();
    }

    /// Stored values changed: fill and render again.
    pub fn form_changed(&mut self) {
        self.form.dirty = true;
        self.mark_dirty();
    }

    /// Puts back what `changes` say was before, returning what undoes that.
    pub fn apply_form_edit(&mut self, changes: Vec<FormChange>) -> Edit {
        for change in &changes {
            match &change.before {
                Some(value) => self.side.form.insert(change.name.clone(), value.clone()),
                None => self.side.form.remove(&change.name),
            };
        }
        self.form_changed();
        Edit::Form(changes.into_iter().map(|c| FormChange { name: c.name, before: c.after, after: c.before }).collect())
    }

    /// Puts back what `changes` say was before, returning what undoes that.
    pub fn apply_hidden_edit(&mut self, changes: Vec<HideChange>) -> Edit {
        for change in &changes {
            store_hidden(&mut self.side.hidden, &change.name, change.widget, change.before);
        }
        self.form_changed();
        Edit::Hidden(changes.into_iter().map(|c| HideChange { before: c.after, after: c.before, ..c }).collect())
    }

    /// Shows or hides widgets as one undoable step, and says what became of them.
    fn show_hide(&mut self, targets: &[HideTarget], hide: bool) {
        let keys = hide_widgets(&self.doc.form.fields, targets);
        if keys.is_empty() {
            self.say("this button shows or hides nothing");
            return;
        }
        let changes = hide_changes(&keys, hide, &self.doc.form.base_hidden, &self.side.hidden);
        let (done, already) = if hide { ("hid", "already hidden") } else { ("showed", "already showing") };
        if changes.is_empty() {
            self.say(already);
            return;
        }
        let fields: HashSet<&str> = changes.iter().map(|c| c.name.as_str()).collect();
        let what = if fields.len() == 1 { "1 field".to_owned() } else { format!("{} fields", fields.len()) };
        self.say(format!("{done} {what}"));
        for change in &changes {
            store_hidden(&mut self.side.hidden, &change.name, change.widget, change.after);
        }
        self.undo.push(Edit::Hidden(changes));
        self.redo.clear();
        self.form_changed();
    }

    /// Forgets every value filled in, and every widget shown or hidden, as one undoable step.
    pub fn clear_form(&mut self) {
        let names: Vec<String> = self.side.form.keys().cloned().collect();
        let hidden: Vec<HideChange> = self
            .side
            .hidden
            .iter()
            .flat_map(|(name, widgets)| {
                widgets.iter().map(|(wi, hide)| HideChange { name: name.clone(), widget: *wi, before: Some(*hide), after: None })
            })
            .collect();
        if names.is_empty() && hidden.is_empty() {
            self.say("no filled-in values");
            return;
        }
        let values: Vec<FormChange> =
            names.iter().map(|n| FormChange { name: n.clone(), before: self.side.form.get(n).cloned(), after: None }).collect();
        self.side.form.clear();
        self.side.hidden.clear();
        let mut steps = Vec::new();
        if !values.is_empty() {
            steps.push(Edit::Form(values));
        }
        if !hidden.is_empty() {
            steps.push(Edit::Hidden(hidden.clone()));
        }
        self.undo.push(if steps.len() == 1 { steps.remove(0) } else { Edit::Group(steps) });
        self.redo.clear();
        self.form_changed();
        let shown = if hidden.is_empty() { String::new() } else { format!(", and put back {} shown or hidden widgets", hidden.len()) };
        self.say(format!("cleared {} filled-in fields{shown}", names.len()));
    }

    /// The widgets that can be picked on the pages showing.
    pub fn form_targets(&self, areas: &[(usize, [f32; 4])]) -> Vec<Target> {
        let mut out = Vec::new();
        for (fi, field) in self.doc.form.fields.iter().enumerate() {
            for (wi, widget) in field.widgets.iter().enumerate().filter(|(_, w)| usable(w)) {
                let (Some(p), Some(_)) = (widget.page, field.widgets.get(wi)) else { continue };
                let (Some((_, area)), Some(page)) = (areas.iter().find(|(page, _)| *page == p), self.doc.info.pages.get(p)) else { continue };
                let rect = view_rect(page, widget.rect);
                if hint::intersects(rect, *area) {
                    out.push(Target { label: String::new(), page: p, rect, kind: TargetKind::Widget(fi, wi) });
                }
            }
        }
        out
    }

    /// The widget at a place on a page, as its field's index and its own.
    pub fn widget_at(&self, page: usize, at: [f32; 2]) -> Option<(usize, usize)> {
        let info = self.doc.info.pages.get(page)?;
        let inside = |r: [f32; 4]| (r[0]..=r[2]).contains(&at[0]) && (r[1]..=r[3]).contains(&at[1]);
        self.doc.form.fields.iter().enumerate().find_map(|(fi, field)| {
            let wi = field.widgets.iter().position(|w| usable(w) && w.page == Some(page) && inside(view_rect(info, w.rect)))?;
            Some((fi, wi))
        })
    }

    /// What choosing a widget does.
    pub fn form_activate(&mut self, fi: usize, wi: usize) {
        let Some(field) = self.doc.form.fields.get(fi).cloned() else { return };
        match field.kind {
            FieldKind::Text if field.has(flags::PASSWORD) => self.say("password fields aren't supported: what is typed would be kept as plain text"),
            FieldKind::Text => {
                let text = match self.current_value(&field) {
                    FormValue::Text(text) => text,
                    _ => String::new(),
                };
                self.open_form_text(&field, &text);
            }
            FieldKind::CheckBox => {
                let on = self.current_value(&field) == FormValue::Check(true);
                self.change_form(vec![(field.name, Some(FormValue::Check(!on)))]);
            }
            FieldKind::Radio => match field.widgets.get(wi).and_then(|w| w.on_state.clone()) {
                Some(state) => self.change_form(vec![(field.name, Some(FormValue::Radio(Some(state))))]),
                None => self.say("this radio button has no state to select"),
            },
            FieldKind::Combo | FieldKind::List => self.open_form_choice(&field),
            FieldKind::PushButton => self.form_button(fi, wi),
            FieldKind::Signature => self.say("signature fields aren't supported: sign with the fill & sign tool (tf, then tS)"),
        }
    }

    fn open_form_bar(&mut self, edit: FormEdit) {
        let origin = self.view.anchor(&self.doc.layout);
        let field = TextField::new(edit.shown.clone());
        self.form.edit = Some(edit);
        self.mode = Mode::Bar(Bar { kind: BarKind::Form, field, origin, previous: None, menu: None });
    }

    fn open_form_text(&mut self, field: &Field, text: &str) {
        let multiline = field.has(flags::MULTILINE);
        let shown = bar_text(text, multiline);
        let limit = field.max_len.map_or(String::new(), |m| format!(" (up to {m})"));
        self.open_form_bar(FormEdit { name: field.name.clone(), shown, prompt: format!("{}{limit}: ", field.name), multiline });
    }

    fn open_form_choice(&mut self, field: &Field) {
        let current = match self.current_value(field) {
            FormValue::Choice(values) => values,
            _ => Vec::new(),
        };
        let mut state = ChoiceState::new(field, current);
        // Opens centred on the highlight, as the contents do.
        let layout = self.list_layout(true, false, state.rows().len(), state.selected, None);
        state.first = layout.first;
        self.mode = Mode::Choice(state);
    }

    /// Stores what the choice panel picked, and closes it.
    pub fn choice_done(&mut self, value: Option<FormValue>) {
        let Mode::Choice(state) = std::mem::replace(&mut self.mode, Mode::Normal) else { return };
        if let Some(value) = value {
            self.change_form(vec![(state.name, Some(value))]);
        }
    }

    /// Takes what was typed in the bar for a field.
    pub fn form_entry_done(&mut self, text: &str) {
        let Some(edit) = self.form.edit.take() else { return };
        let Some(field) = self.doc.form.field(&edit.name).cloned() else { return };
        if text == edit.shown {
            return;
        }
        let typed = field_text(text, edit.multiline);
        match check_length(&field, &typed).map(|()| FormValue::Text(typed)) {
            Ok(value) => self.change_form(vec![(field.name, Some(value))]),
            Err(why) => {
                self.say(why);
                self.reopen_form_bar(edit, text);
            }
        }
    }

    /// Back to the bar with what was typed, to put it right.
    fn reopen_form_bar(&mut self, edit: FormEdit, text: &str) {
        let shown = edit.shown.clone();
        self.open_form_bar(edit.clone());
        if let Mode::Bar(bar) = &mut self.mode {
            bar.field = TextField::new(text.to_owned());
        }
        self.form.edit = Some(FormEdit { shown, ..edit });
    }

    /// What a push button does, read now from the document's own bytes: the actions are no part
    /// of what filling changes, and reading them only when a button is used keeps a hostile
    /// form's buttons from costing anything while it is opened.
    fn button_at_click(&self, fi: usize, wi: usize) -> ButtonCmd {
        let Some(field) = self.doc.form.fields.get(fi) else { return ButtonCmd::Nothing };
        let (bytes, password) = (self.doc.bytes.clone(), self.doc.password());
        let read = catch_unwind(AssertUnwindSafe(|| {
            let doc = Document::open_with_password(bytes, password).ok()?;
            Some(button_command(&doc, field, wi))
        }));
        read.ok().flatten().unwrap_or(ButtonCmd::Unsupported)
    }

    /// Does what a push button's action says, when that is not a script.
    fn form_button(&mut self, fi: usize, wi: usize) {
        let command = self.button_at_click(fi, wi);
        match command {
            ButtonCmd::Reset { fields, exclude } => {
                let touched = reset_names(&self.doc.form.fields, fields.as_deref(), exclude);
                let changes: Vec<_> = touched.into_iter().filter(|n| self.side.form.contains_key(n)).map(|n| (n, None)).collect();
                self.say(if changes.is_empty() { "nothing to reset".to_owned() } else { format!("reset {} fields", changes.len()) });
                self.change_form(changes);
            }
            ButtonCmd::Named(name) => self.named_action(&name),
            ButtonCmd::GoTo(page) => self.jump_to_page(page),
            ButtonCmd::Uri(uri) => self.open_uri(&uri),
            ButtonCmd::Submit => self.say("submitting a form isn't supported: yavcpdf never sends it anywhere"),
            ButtonCmd::Hide { targets, hide } => self.show_hide(&targets, hide),
            ButtonCmd::Script => self.say("this button needs JavaScript, which yavcpdf doesn't run"),
            ButtonCmd::Unsupported => self.say("this button's action isn't supported"),
            ButtonCmd::Nothing => self.say("this button does nothing"),
        }
    }

    fn named_action(&mut self, name: &str) {
        let page = self.view.current_page(&self.doc.layout);
        match name {
            "NextPage" => self.jump_to_page((page + 1).min(self.doc.layout.len() - 1)),
            "PrevPage" => self.jump_to_page(page.saturating_sub(1)),
            "FirstPage" => self.jump_to_page(0),
            "LastPage" => self.jump_to_page(self.doc.layout.len() - 1),
            other => self.say(format!("the {other} action isn't supported")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfcraft_forms::af::ButtonAction;
    use pdfcraft_forms::{NewField, add_field};

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

    /// A page with a text field, a check box, a combo box, a radio group of two, and a button.
    fn form_pdf() -> Vec<u8> {
        let mut doc = Document::open(Arc::new(tiny_pdf())).unwrap();
        let rect = |y: f64| [50.0, y, 200.0, y + 20.0];
        let options = || vec!["Red".to_owned(), "Green".to_owned(), "Blue".to_owned()];
        add_field(&mut doc, 0, rect(700.0), &NewField::Text { multiline: false }, Some("name")).unwrap();
        add_field(&mut doc, 0, rect(660.0), &NewField::CheckBox, Some("agree")).unwrap();
        add_field(&mut doc, 0, rect(620.0), &NewField::Combo { options: options(), editable: false }, Some("colour")).unwrap();
        add_field(&mut doc, 0, rect(580.0), &NewField::Radio { group: Some("size".into()), export: "S".into() }, None).unwrap();
        add_field(&mut doc, 0, rect(540.0), &NewField::Radio { group: Some("size".into()), export: "L".into() }, None).unwrap();
        add_field(&mut doc, 0, rect(500.0), &NewField::Button { caption: "Go".into() }, Some("go")).unwrap();
        write_incremental(&doc, &SaveOptions::default()).unwrap()
    }

    fn values(pairs: &[(&str, FormValue)]) -> BTreeMap<String, FormValue> {
        pairs.iter().map(|(n, v)| ((*n).to_owned(), v.clone())).collect()
    }

    fn open(bytes: Vec<u8>) -> Document {
        Document::open(Arc::new(bytes)).unwrap()
    }

    fn field(doc: &Document, name: &str) -> Field {
        fields(doc).into_iter().find(|f| f.name == name).unwrap()
    }

    #[test]
    fn the_fixture_has_the_fields_it_should() {
        let names: Vec<String> = fields(&open(form_pdf())).into_iter().map(|f| f.name).collect();
        assert_eq!(names, ["name", "agree", "colour", "size", "go"]);
    }

    #[test]
    fn values_are_read_back_from_a_filled_document() {
        let bytes = form_pdf();
        let size = field(&open(bytes.clone()), "size");
        let on = size.widgets[1].on_state.clone().unwrap();
        let set = values(&[
            ("name", FormValue::Text("Ada".into())),
            ("agree", FormValue::Check(true)),
            ("colour", FormValue::Choice(vec!["Green".into()])),
            ("size", FormValue::Radio(Some(on.clone()))),
        ]);
        let built = build(Arc::new(bytes), None, &set, &Hidden::new()).unwrap();
        assert!(built.failed.is_empty(), "{:?}", built.failed);
        let by = |n: &str| built.fields.iter().find(|f| f.name == n).unwrap();
        assert_eq!(FormValue::of(by("name")), FormValue::Text("Ada".into()));
        assert_eq!(FormValue::of(by("agree")), FormValue::Check(true));
        assert_eq!(FormValue::of(by("colour")), FormValue::Choice(vec!["Green".into()]));
        assert_eq!(FormValue::of(by("size")), FormValue::Radio(Some(on)));
        assert_eq!(built.base["name"], FormValue::Text(String::new()), "the document's own values are kept apart");
        let render = built.render.expect("filled values are rendered from a filled copy");
        assert_eq!(FormValue::of(&field(&open(render), "name")), FormValue::Text("Ada".into()));
    }

    #[test]
    fn a_form_with_no_values_and_looks_of_its_own_renders_as_it_is() {
        let built = build(Arc::new(form_pdf()), None, &BTreeMap::new(), &Hidden::new()).unwrap();
        assert!(built.render.is_none());
        assert_eq!(built.fields.len(), 5);
    }

    #[test]
    fn a_value_that_cannot_go_in_is_reported_and_the_rest_still_do() {
        let set = values(&[("name", FormValue::Text("Ada".into())), ("nobody", FormValue::Text("x".into()))]);
        let built = build(Arc::new(form_pdf()), None, &set, &Hidden::new()).unwrap();
        assert_eq!(built.failed.len(), 1);
        assert_eq!(built.failed[0].0, "nobody");
        assert_eq!(FormValue::of(built.fields.iter().find(|f| f.name == "name").unwrap()), FormValue::Text("Ada".into()));
    }

    #[test]
    fn a_document_with_no_form_has_no_fields() {
        let built = build(Arc::new(tiny_pdf()), None, &BTreeMap::new(), &Hidden::new()).unwrap();
        assert!(built.fields.is_empty() && built.render.is_none());
        assert!(build(Arc::new(b"not a pdf".to_vec()), None, &BTreeMap::new(), &Hidden::new()).is_err());
    }

    #[test]
    fn values_survive_the_sidecar() {
        let all = values(&[
            ("a", FormValue::Text("x".into())),
            ("b", FormValue::Check(true)),
            ("c", FormValue::Radio(None)),
            ("d", FormValue::Choice(vec!["1".into(), "2".into()])),
        ]);
        let json = serde_json::to_string(&all).unwrap();
        assert_eq!(serde_json::from_str::<BTreeMap<String, FormValue>>(&json).unwrap(), all);
    }

    fn choice(options: &[(&str, &str)], flag: u32) -> Field {
        let mut f = field(&open(form_pdf()), "colour");
        f.options = options.iter().map(|(e, d)| ((*e).to_owned(), (*d).to_owned())).collect();
        f.flags = flag;
        f
    }

    #[test]
    fn the_wheel_adds_up_small_steps() {
        use super::wheel_step;
        // Many small deltas scroll as far as one large one.
        let (mut first, mut rest) = (10, 0.0);
        for _ in 0..40 {
            (first, rest) = wheel_step(first, rest, -2.5, 20.0, 5, 100);
        }
        assert_eq!(wheel_step(10, 0.0, -100.0, 20.0, 5, 100).0, first);
        assert_eq!(first, 15);
        // One notch of a row height is one row, either way.
        assert_eq!(wheel_step(10, 0.0, -20.0, 20.0, 5, 100), (11, 0.0));
        assert_eq!(wheel_step(10, 0.0, 20.0, 20.0, 5, 100), (9, 0.0));
        // The rest clears at either end.
        assert_eq!(wheel_step(94, 0.5, -30.0, 20.0, 5, 100), (95, 0.0));
        assert_eq!(wheel_step(0, -0.5, 10.0, 20.0, 5, 100), (0, 0.0));
        assert_eq!(wheel_step(95, 0.0, -10.0, 20.0, 5, 100), (95, 0.0));
        assert_eq!(wheel_step(10, 0.3, f32::NAN, 20.0, 5, 100), (10, 0.0));
        assert_eq!(wheel_step(10, 0.3, 20.0, f32::NAN, 5, 100), (10, 0.0));
        assert_eq!(wheel_step(10, 0.0, -20.0, 20.0, 5, 100), (11, 0.0), "later events still scroll");
        assert_eq!(clamp_selected(2, 10, 5, 100), 10);
        assert_eq!(clamp_selected(50, 10, 5, 100), 14);
        assert_eq!(clamp_selected(12, 10, 5, 100), 12);
        assert_eq!(clamp_selected(9, 0, 5, 3), 2);
        assert_eq!(truncated("é".repeat(300), 200).chars().count(), 201);
        assert_eq!(truncated("short".into(), 200), "short");
    }

    fn panel(options: &[(&str, &str)], flag: u32, current: &[&str]) -> ChoiceState {
        let mut f = choice(options, flag);
        f.kind = if flag & flags::EDIT != 0 { FieldKind::Combo } else { FieldKind::List };
        ChoiceState::new(&f, current.iter().map(|c| (*c).to_owned()).collect())
    }

    fn typed(state: &mut ChoiceState, text: &str) {
        state.refilter(|s| s.filter = TextField::new(text.to_owned()));
    }

    #[test]
    fn a_single_choice_starts_with_none_and_opens_on_the_current_option() {
        let s = panel(&[("r", "Red"), ("g", "Green")], 0, &["g"]);
        assert_eq!(s.rows(), vec![ChoiceRow::Clear, ChoiceRow::Option(0), ChoiceRow::Option(1)]);
        assert_eq!(s.highlighted(), Some(ChoiceRow::Option(1)));
        assert!(s.ticked(ChoiceRow::Option(1)) && !s.ticked(ChoiceRow::Option(0)));
        assert_eq!(panel(&[("r", "Red")], 0, &[]).selected, 0, "no current choice: the first row");
        assert_eq!(s.label(ChoiceRow::Clear), "(none)");
    }

    #[test]
    fn filtering_is_by_case_insensitive_substring_and_keeps_the_row() {
        let mut s = panel(&[("r", "Red"), ("g", "Green"), ("gr", "Grey")], 0, &[]);
        s.go(Move::Last);
        typed(&mut s, "GR");
        assert_eq!(s.rows(), vec![ChoiceRow::Option(1), ChoiceRow::Option(2)], "no (none) row while filtering");
        assert_eq!(s.highlighted(), Some(ChoiceRow::Option(2)), "the row stays highlighted");
        typed(&mut s, "zzz");
        assert!(s.rows().is_empty() && s.highlighted().is_none());
        s.go(Move::Next);
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn moving_stays_within_the_rows() {
        assert_eq!(moved(0, 3, Move::Previous), 0);
        assert_eq!(moved(1, 3, Move::Next), 2);
        assert_eq!(moved(2, 3, Move::Next), 2);
        assert_eq!(moved(2, 30, Move::PageDown), 12);
        assert_eq!(moved(25, 30, Move::PageDown), 29);
        assert_eq!(moved(5, 30, Move::PageUp), 0);
        assert_eq!((moved(7, 30, Move::First), moved(7, 30, Move::Last), moved(0, 0, Move::Last)), (0, 29, 0));
    }

    #[test]
    fn choosing_stores_export_values() {
        let s = panel(&[("r", "Red"), ("g", "Green")], 0, &["r"]);
        assert_eq!(s.commit(Some(ChoiceRow::Option(1))), Some(FormValue::Choice(vec!["g".into()])));
        assert_eq!(s.commit(Some(ChoiceRow::Clear)), Some(FormValue::Choice(Vec::new())));
        assert_eq!(s.commit(None), None, "no row, no choice");
    }

    #[test]
    fn an_editable_combo_offers_the_typed_text_unless_an_option_is_exactly_it() {
        let mut s = panel(&[("r", "Red"), ("g", "Green")], flags::EDIT, &[]);
        typed(&mut s, "Mauve");
        assert_eq!(s.rows(), vec![ChoiceRow::Free]);
        assert_eq!(s.label(ChoiceRow::Free), "use \"Mauve\"");
        assert_eq!(s.commit(Some(ChoiceRow::Free)), Some(FormValue::Choice(vec!["Mauve".into()])));
        typed(&mut s, "Red");
        assert_eq!(s.rows(), vec![ChoiceRow::Option(0)], "exactly an option's text");
        typed(&mut s, "Re");
        assert_eq!(s.rows(), vec![ChoiceRow::Option(0), ChoiceRow::Option(1), ChoiceRow::Free]);
        let plain = panel(&[("r", "Red")], 0, &[]);
        assert!(!plain.rows().contains(&ChoiceRow::Free));
    }

    #[test]
    fn a_multi_select_list_marks_and_commits_the_set() {
        let mut s = panel(&[("a", "A"), ("b", "B"), ("c", "C")], flags::MULTI_SELECT, &["c", "x"]);
        assert_eq!(s.rows().len(), 4, "no (none) row: unmark instead; the stranger \"x\" has one");
        assert_eq!(s.highlighted(), Some(ChoiceRow::Stranger(0)));
        s.toggle(ChoiceRow::Option(0));
        s.toggle(ChoiceRow::Option(2));
        assert_eq!(s.commit(None), Some(FormValue::Choice(vec!["a".into(), "x".into()])), "option order, then the strangers");
        s.toggle(ChoiceRow::Option(0));
        s.marked.clear();
        assert_eq!(s.commit(None), Some(FormValue::Choice(Vec::new())), "an empty set clears");
        let mut single = panel(&[("a", "A")], 0, &[]);
        single.toggle(ChoiceRow::Option(0));
        assert!(single.marked.is_empty(), "a single choice has no marks to toggle");
    }

    fn many(n: usize) -> Vec<(String, String)> {
        (0..n).map(|i| (format!("e{i}"), format!("Option {i}"))).collect()
    }

    #[test]
    fn a_panel_without_options_is_harmless() {
        let mut s = panel(&[], 0, &[]);
        assert_eq!(s.rows(), vec![ChoiceRow::Clear]);
        assert_eq!(s.highlighted(), Some(ChoiceRow::Clear));
        s.go(Move::Last);
        s.go(Move::PageDown);
        assert_eq!(s.selected, 0);
        assert_eq!(s.commit(s.highlighted()), Some(FormValue::Choice(vec![])));
        let mut m = panel(&[], flags::MULTI_SELECT, &[]);
        assert!(m.rows().is_empty());
        m.go(Move::Next);
        m.go(Move::Last);
        assert_eq!(m.commit(m.highlighted()), None, "nothing marked, nothing changed");
        let mut e = panel(&[], flags::EDIT, &[]);
        typed(&mut e, "  hi ");
        assert_eq!(e.rows(), vec![ChoiceRow::Free]);
    }

    #[test]
    fn a_filter_matching_nothing_leaves_nothing_to_choose() {
        let mut s = panel(&[("r", "Red"), ("g", "Green")], 0, &["g"]);
        typed(&mut s, "zzz");
        assert!(s.rows().is_empty());
        assert_eq!(s.highlighted(), None);
        assert_eq!(s.commit(s.highlighted()), None, "Enter stores nothing");
        assert!(s.nothing_to_choose(), "and keeps the panel open");
        for by in [Move::Next, Move::Previous, Move::PageDown, Move::PageUp, Move::First, Move::Last] {
            s.go(by);
            assert_eq!(s.selected, 0);
        }
        s.toggle(ChoiceRow::Option(0));
        assert_eq!(s.marked, vec!["g".to_owned()], "nothing changed");
        let mut m = panel(&[("a", "A")], flags::MULTI_SELECT, &["a"]);
        typed(&mut m, "zzz");
        assert!(m.nothing_to_choose());
        assert!(!panel(&[], flags::MULTI_SELECT, &[]).nothing_to_choose(), "no filter, no options: Enter may close");
    }

    #[test]
    fn the_highlight_stops_at_both_ends() {
        let mut s = panel(&[], 0, &[]);
        s.options = many(25);
        // rows: Clear + 25 = 26
        assert_eq!(s.rows().len(), 26);
        s.go(Move::Last);
        assert_eq!(s.selected, 25);
        s.go(Move::Next);
        assert_eq!(s.selected, 25);
        s.go(Move::PageDown);
        assert_eq!(s.selected, 25);
        s.go(Move::First);
        assert_eq!(s.selected, 0);
        s.go(Move::Previous);
        s.go(Move::PageUp);
        assert_eq!(s.selected, 0);
        s.go(Move::PageDown);
        assert_eq!(s.selected, 10);
        s.go(Move::PageDown);
        s.go(Move::PageDown);
        assert_eq!(s.selected, 25);
        s.go(Move::PageUp);
        assert_eq!(s.selected, 15);
        assert_eq!(moved(usize::MAX - 1, usize::MAX, Move::Next), usize::MAX - 1);
        assert_eq!(moved(5, 0, Move::PageUp), 0);
    }

    #[test]
    fn the_highlight_survives_narrowing_and_widening_the_filter() {
        let mut s = panel(&[("r", "Red"), ("g", "Green"), ("b", "Blue"), ("gr", "Grey")], 0, &[]);
        s.go(Move::Last); // Grey, option 3
        assert_eq!(s.highlighted(), Some(ChoiceRow::Option(3)));
        typed(&mut s, "gr");
        assert_eq!(s.highlighted(), Some(ChoiceRow::Option(3)), "kept through narrowing");
        typed(&mut s, "gre");
        assert_eq!(s.highlighted(), Some(ChoiceRow::Option(3)));
        typed(&mut s, "gree");
        assert_eq!(s.highlighted(), Some(ChoiceRow::Option(1)), "dropped row: back to the top shown row");
        typed(&mut s, "");
        assert_eq!(s.rows().len(), 5);
        assert_eq!(s.highlighted(), Some(ChoiceRow::Option(1)), "Green kept on widening, behind (none) and Red");
        assert_eq!(s.selected, 2);
    }

    #[test]
    fn duplicate_display_texts_and_export_values_are_told_apart() {
        // duplicate display
        let s = panel(&[("a", "Same"), ("b", "Same")], 0, &["b"]);
        assert_eq!(s.highlighted(), Some(ChoiceRow::Option(1)));
        assert!(s.ticked(ChoiceRow::Option(1)) && !s.ticked(ChoiceRow::Option(0)));
        assert_eq!(s.commit(Some(ChoiceRow::Option(0))), Some(FormValue::Choice(vec!["a".into()])));
        // duplicate export values: both rows tick, opens on the first
        let d = panel(&[("x", "One"), ("y", "Two"), ("x", "Uno")], 0, &["x"]);
        assert!(d.ticked(ChoiceRow::Option(0)) && d.ticked(ChoiceRow::Option(2)));
        assert_eq!(d.highlighted(), Some(ChoiceRow::Option(0)));
        // multi: toggling one of two same-export rows toggles both; the commit has no duplicates
        let mut m = panel(&[("x", "One"), ("y", "Two"), ("x", "Uno")], flags::MULTI_SELECT, &[]);
        m.toggle(ChoiceRow::Option(2));
        assert!(m.ticked(ChoiceRow::Option(0)));
        assert_eq!(m.marked_value(), FormValue::Choice(vec!["x".into()]), "a duplicate export value is stored once");
    }

    #[test]
    fn filtering_matches_the_text_not_the_export_value() {
        let mut s = panel(&[("1", "January"), ("2", "February")], 0, &["2"]);
        assert_eq!(s.highlighted(), Some(ChoiceRow::Option(1)), "current is an export value");
        typed(&mut s, "2");
        assert!(s.rows().is_empty(), "filter matches display, not export value");
        typed(&mut s, "feb");
        assert_eq!(s.commit(s.highlighted()), Some(FormValue::Choice(vec!["2".into()])));
        // a current value that is a display text, not an export value, is a stranger
        let t = panel(&[("1", "January"), ("2", "February")], 0, &["February"]);
        assert!(!t.ticked(ChoiceRow::Option(1)));
        assert_eq!(t.highlighted(), Some(ChoiceRow::Stranger(0)));
    }

    #[test]
    fn a_current_value_that_is_no_option_gets_a_row() {
        let s = panel(&[("a", "A"), ("b", "B")], 0, &["zzz"]);
        assert_eq!(s.rows(), vec![ChoiceRow::Clear, ChoiceRow::Stranger(0), ChoiceRow::Option(0), ChoiceRow::Option(1)]);
        assert!(s.ticked(ChoiceRow::Stranger(0)) && !s.ticked(ChoiceRow::Option(0)));
        assert_eq!(s.label(ChoiceRow::Stranger(0)), "zzz");
        assert_eq!(s.highlighted(), Some(ChoiceRow::Stranger(0)), "opens on it");
        assert_eq!(s.commit(s.highlighted()), Some(FormValue::Choice(vec!["zzz".into()])), "choosing it keeps the value");
        let mut f = panel(&[("a", "A")], 0, &["zzz"]);
        typed(&mut f, "zz");
        assert_eq!(f.rows(), vec![ChoiceRow::Stranger(0)], "it is filtered like any row");
        let m = panel(&[("a", "A"), ("b", "B")], flags::MULTI_SELECT, &["zzz", "b", "zzz", ""]);
        assert_eq!(m.rows(), vec![ChoiceRow::Stranger(0), ChoiceRow::Option(0), ChoiceRow::Option(1)], "at the top, once");
        assert_eq!(m.highlighted(), Some(ChoiceRow::Stranger(0)));
        assert_eq!(m.marked_value(), FormValue::Choice(vec!["b".into(), "zzz".into()]));
        let mut m2 = m.clone();
        m2.toggle(ChoiceRow::Option(0));
        assert_eq!(m2.marked_value(), FormValue::Choice(vec!["a".into(), "b".into(), "zzz".into()]));
        m2.toggle(ChoiceRow::Stranger(0));
        assert!(!m2.ticked(ChoiceRow::Stranger(0)));
        assert_eq!(m2.marked_value(), FormValue::Choice(vec!["a".into(), "b".into()]), "toggling it unmarks it");
        let mut c = panel(&[("a", "A")], flags::EDIT, &["zzz"]);
        typed(&mut c, "zzz");
        assert_eq!(c.rows(), vec![ChoiceRow::Stranger(0)], "no free row for what a row already is");
    }

    #[test]
    fn enter_without_toggles_changes_nothing() {
        let same = panel(&[("a", "A"), ("b", "B")], flags::MULTI_SELECT, &["b", "a", "b"]);
        assert_eq!(same.commit(None), None, "the same set, in another order or with repeats");
        let mut m = panel(&[("a", "A"), ("b", "B")], flags::MULTI_SELECT, &["b"]);
        m.toggle(ChoiceRow::Option(0));
        m.toggle(ChoiceRow::Option(0));
        assert_eq!(m.commit(None), None, "marked and unmarked again");
        m.toggle(ChoiceRow::Option(0));
        assert_eq!(m.commit(None), Some(FormValue::Choice(vec!["a".into(), "b".into()])));
    }

    #[test]
    fn only_a_combo_box_can_be_typed_in() {
        let mut f = choice(&[("r", "Red")], flags::EDIT);
        f.kind = FieldKind::List;
        assert!(!ChoiceState::new(&f, Vec::new()).editable, "a list box has no such flag");
        f.kind = FieldKind::Combo;
        assert!(ChoiceState::new(&f, Vec::new()).editable);
        f.flags = 0;
        assert!(!ChoiceState::new(&f, Vec::new()).editable);
    }

    #[test]
    fn the_list_scrolls_only_as_far_as_needed() {
        assert_eq!(keep_visible(5, 7, 10, 50), 5, "in sight: stays");
        assert_eq!(keep_visible(5, 3, 10, 50), 3, "above: the row comes in at the top");
        assert_eq!(keep_visible(5, 15, 10, 50), 6, "below: the row comes in at the bottom");
        assert_eq!(keep_visible(40, 49, 10, 50), 40);
        assert_eq!(keep_visible(30, 2, 10, 5), 0, "a short list never scrolls");
        assert_eq!(scrolled(0, -3, 10, 50), 0, "not above the first row");
        assert_eq!(scrolled(5, 3, 10, 50), 8);
        assert_eq!(scrolled(35, 9, 10, 50), 40, "not past the last page");
        assert_eq!(scrolled(0, 4, 10, 5), 0, "a short list does not move");
    }

    #[test]
    fn a_click_does_not_move_the_list() {
        let mut m = panel(&[], flags::MULTI_SELECT, &[]);
        m.options = many(40);
        m.first = 12;
        assert_eq!(m.click(15), None, "a multi-select click only marks");
        assert_eq!((m.selected, m.first), (15, 12));
        assert_eq!(m.marked, vec!["e15".to_owned()]);
        m.click(15);
        assert!(m.marked.is_empty());
        let mut s = panel(&[("a", "A"), ("b", "B")], 0, &[]);
        s.first = 0;
        assert_eq!(s.click(2), Some(FormValue::Choice(vec!["b".into()])), "a single-select click chooses");
        assert_eq!(s.click(9), None, "no such row");
    }

    #[test]
    fn filtering_folds_case_and_keeps_non_ascii() {
        let mut s = panel(&[("1", "Écran"), ("2", "café"), ("3", "Straße"), ("4", "STRASSE")], 0, &[]);
        typed(&mut s, "é");
        assert_eq!(s.rows(), vec![ChoiceRow::Option(0), ChoiceRow::Option(1)], "é matches É and é");
        typed(&mut s, "É");
        assert_eq!(s.rows(), vec![ChoiceRow::Option(0), ChoiceRow::Option(1)]);
        typed(&mut s, "straße");
        assert_eq!(s.rows(), vec![ChoiceRow::Option(2)], "ß does not fold to ss");
        typed(&mut s, "strasse");
        assert_eq!(s.rows(), vec![ChoiceRow::Option(3)]);
        typed(&mut s, "ẞ");
        // capital sharp s lowercases to ß
        assert_eq!(s.rows(), vec![ChoiceRow::Option(2)]);
    }

    #[test]
    fn free_text_is_trimmed_and_offered_unless_exact() {
        let mut s = panel(&[("r", "Red"), ("g", "Green")], flags::EDIT, &[]);
        typed(&mut s, "  Mauve \t");
        assert_eq!(s.label(ChoiceRow::Free), "use \"Mauve\"");
        assert_eq!(s.commit(Some(ChoiceRow::Free)), Some(FormValue::Choice(vec!["Mauve".into()])));
        typed(&mut s, "   ");
        assert_eq!(s.rows(), vec![ChoiceRow::Clear, ChoiceRow::Option(0), ChoiceRow::Option(1)], "blank filter is empty");
        // exact text: no Free; different case: Free offered as well
        typed(&mut s, " Red ");
        assert_eq!(s.rows(), vec![ChoiceRow::Option(0)]);
        typed(&mut s, "red");
        assert_eq!(s.rows(), vec![ChoiceRow::Option(0), ChoiceRow::Free], "case-insensitive match is not exact");
        assert_eq!(s.commit(Some(ChoiceRow::Free)), Some(FormValue::Choice(vec!["red".into()])));
        // free text equal to an export value but not a display text
        let mut e = panel(&[("r", "Red")], flags::EDIT, &[]);
        typed(&mut e, "r");
        assert_eq!(e.rows(), vec![ChoiceRow::Option(0), ChoiceRow::Free]);
    }

    #[test]
    fn choosing_none_on_an_empty_field_is_no_change() {
        let s = panel(&[("r", "Red")], 0, &[]);
        assert_eq!(s.highlighted(), Some(ChoiceRow::Clear));
        assert_eq!(s.commit(s.highlighted()), Some(FormValue::Choice(vec![])));
        // an empty field's own value reads as Choice([]), so choosing (none) is no change
        let f = field(&open(form_pdf()), "colour");
        assert_eq!(FormValue::of(&f), FormValue::Choice(vec![]));
    }

    #[test]
    fn ten_thousand_options_filter_quickly() {
        let mut s = panel(&[], 0, &[]);
        s.options = many(10_000);
        let t = std::time::Instant::now();
        let n = (0..20).map(|_| s.rows().len()).sum::<usize>();
        typed(&mut s, "option 99");
        let m = s.rows().len();
        let el = t.elapsed();
        assert_eq!(n, 20 * 10_001);
        assert_eq!(m, 111, "Option 99, 990-999, 9900-9999 -> 1+10+100");
        eprintln!("10k rows x21: {el:?}");
        assert!(el.as_millis() < 2000, "{el:?}");
    }

    #[test]
    fn the_header_is_the_tooltip_or_the_name() {
        let mut f = choice(&[("r", "Red")], 0);
        f.tooltip = Some("Pick a colour".into());
        assert_eq!(ChoiceState::new(&f, Vec::new()).title, "Pick a colour");
        f.tooltip = None;
        assert_eq!(ChoiceState::new(&f, Vec::new()).title, f.name);
    }

    #[test]
    fn max_len_counts_characters() {
        let mut f = field(&open(form_pdf()), "name");
        f.max_len = Some(3);
        assert!(check_length(&f, "añb").is_ok());
        assert!(check_length(&f, "abcd").is_err());
        f.max_len = None;
        assert!(check_length(&f, "abcd").is_ok());
    }

    #[test]
    fn a_reset_touches_the_named_fields_or_all_but_them_or_all() {
        let all = fields(&open(form_pdf()));
        let names = |v: Vec<String>| v;
        let some = |v: &[&str]| Some(v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        assert_eq!(names(reset_names(&all, some(&["name"]).as_deref(), false)), ["name"]);
        assert_eq!(reset_names(&all, some(&["name", "go"]).as_deref(), true), ["agree", "colour", "size"]);
        assert_eq!(reset_names(&all, None, false).len(), 5);
    }

    #[test]
    fn widget_rects_are_grouped_by_page_and_only_usable_ones() {
        let page = PageInfo { width: 600.0, height: 800.0, label: "1".into(), crop: [0.0, 0.0, 600.0, 800.0], rotation: 0 };
        let pages = [page.clone(), page];
        let mut all = fields(&open(form_pdf()));
        let usable_count = all.iter().flat_map(|f| &f.widgets).filter(|w| usable(w)).count();
        assert!(usable_count >= 2);
        let grouped = widget_rects_by_page(&all, &pages);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].len(), usable_count);
        assert!(grouped[1].is_empty());
        let first = all[0].widgets[0].clone();
        assert_eq!(grouped[0][0], view_rect(&pages[0], first.rect));
        // Hidden, read-only and page-less widgets are skipped; a page out of range is ignored.
        all[0].widgets[0].hidden = true;
        all[1].widgets[0].read_only = true;
        all[2].widgets[0].page = None;
        all[3].widgets[0].page = Some(9);
        let grouped = widget_rects_by_page(&all, &pages);
        assert_eq!(grouped[0].len(), usable_count - 4);
        assert!(widget_rects_by_page(&all, &[]).is_empty());
    }

    #[test]
    fn lines_become_one() {
        assert_eq!(one_line("a\r\nb\nc\rd"), "a b c d");
    }

    #[test]
    fn view_rects_have_the_top_left_origin() {
        let page = PageInfo { width: 600.0, height: 800.0, label: "1".into(), crop: [0.0, 0.0, 600.0, 800.0], rotation: 0 };
        assert_eq!(view_rect(&page, [50.0, 700.0, 200.0, 720.0]), [50.0, 80.0, 200.0, 100.0]);
    }

    /// The button of the form with `action` as its `/A`, as an odd or malicious form would have it.
    fn button_with(action: Dict) -> Document {
        let mut doc = open(form_pdf());
        let widget = field(&doc, "go").widgets[0].obj;
        doc.update_dict(widget, |d| d.set(b"A", Object::Dict(action))).unwrap();
        doc
    }

    fn named(kind: &str) -> Dict {
        let mut action = Dict::new();
        action.set(b"S", Object::name(kind));
        action
    }

    #[test]
    fn a_javascript_action_is_never_believed() {
        let mut action = named("JavaScript");
        action.set(b"JS", Object::String(pdfcraft_cos::PdfString::text("this.pageNum++;")));
        let doc = button_with(action);
        let go = field(&doc, "go");
        assert_eq!(go.button, Some(ButtonAction::Named("NextPage".into())), "the library reads the script as a page turn");
        assert_eq!(button_command(&doc, &go, 0), ButtonCmd::Script);
    }

    #[test]
    fn a_pdf_action_is_read_from_the_action_itself() {
        let doc = button_with(named("SubmitForm"));
        assert_eq!(button_command(&doc, &field(&doc, "go"), 0), ButtonCmd::Submit);
        let doc = button_with(named("Hide"));
        assert_eq!(button_command(&doc, &field(&doc, "go"), 0), ButtonCmd::Hide { targets: Vec::new(), hide: true });
        let doc = button_with(named("Launch"));
        assert_eq!(button_command(&doc, &field(&doc, "go"), 0), ButtonCmd::Unsupported);
    }

    /// A one-page PDF from raw object bodies (object 1 is the catalog, 2 the page tree).
    fn raw_pdf(objects: &[&str]) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n".to_vec();
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

    /// A button whose field and (separate) widget carry the given entries.
    fn button_pdf(field_extra: &str, widget_extra: &str) -> Document {
        let field = format!("<< /FT /Btn /Ff 65536 /T (go) /Kids [5 0 R] {field_extra} >>");
        let widget = format!("<< /Type /Annot /Subtype /Widget /Parent 4 0 R /Rect [10 10 90 30] /P 3 0 R {widget_extra} >>");
        open(raw_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << >> /Annots [5 0 R] >>",
            &field,
            &widget,
        ]))
    }

    fn cmd(doc: &Document) -> ButtonCmd {
        button_command(doc, &field(doc, "go"), 0)
    }

    #[test]
    fn button_actions_of_each_kind() {
        let d = button_pdf("", "/A << /S /Named /N /NextPage >>");
        assert_eq!(cmd(&d), ButtonCmd::Named("NextPage".into()));
        let d = button_pdf("", "/A << /S /GoTo /D [3 0 R /Fit] >>");
        assert_eq!(cmd(&d), ButtonCmd::GoTo(0));
        let d = button_pdf("", "/A << /S /URI /URI (https://example.org/) >>");
        assert_eq!(cmd(&d), ButtonCmd::Uri("https://example.org/".into()));
        let d = button_pdf("", "/A << /S /ResetForm /Fields [(a) (b)] /Flags 1 >>");
        assert_eq!(cmd(&d), ButtonCmd::Reset { fields: Some(vec!["a".into(), "b".into()]), exclude: true });
        let d = button_pdf("", "/A << /S /ResetForm >>");
        assert_eq!(cmd(&d), ButtonCmd::Reset { fields: None, exclude: false });
        let d = button_pdf("", "");
        assert_eq!(cmd(&d), ButtonCmd::Nothing);
    }

    fn obj(num: u32) -> ObjRef {
        ObjRef { num, generation: 0 }
    }

    #[test]
    fn hide_actions_name_their_targets() {
        let hide = |t: &str| cmd(&button_pdf("", &format!("/A << /S /Hide {t} >>")));
        let name = |n: &str| HideTarget::Name(n.into());
        assert_eq!(hide("/T (a.b)"), ButtonCmd::Hide { targets: vec![name("a.b")], hide: true });
        assert_eq!(hide("/T (a) /H false"), ButtonCmd::Hide { targets: vec![name("a")], hide: false });
        assert_eq!(hide("/T (a) /H true"), ButtonCmd::Hide { targets: vec![name("a")], hide: true });
        // A widget by reference, and a field by reference, which goes by its name.
        assert_eq!(hide("/T [(a) 5 0 R 4 0 R]"), ButtonCmd::Hide { targets: vec![name("a"), HideTarget::Widget(obj(5)), name("go")], hide: true });
        assert_eq!(hide("/T 5 0 R"), ButtonCmd::Hide { targets: vec![HideTarget::Widget(obj(5))], hide: true });
        // The page is no field and no widget, and a number names nothing.
        assert_eq!(hide("/T [3 0 R 7]"), ButtonCmd::Hide { targets: Vec::new(), hide: true });
        assert_eq!(hide(""), ButtonCmd::Hide { targets: Vec::new(), hide: true });
        let many = format!("/T [{}]", "(x) ".repeat(MAX_TARGETS + 1));
        assert_eq!(hide(&many), ButtonCmd::Unsupported, "a list too long to read whole");
    }

    #[test]
    fn a_hide_target_reaches_every_widget_of_a_field_and_its_kids_or_just_one() {
        let all = fields(&open(form_pdf()));
        let name = |n: &str| HideTarget::Name(n.into());
        assert_eq!(hide_widgets(&all, &[name("size")]), [("size".to_owned(), 0), ("size".to_owned(), 1)]);
        let second = all.iter().find(|f| f.name == "size").unwrap().widgets[1].obj;
        assert_eq!(hide_widgets(&all, &[HideTarget::Widget(second)]), [("size".to_owned(), 1)]);
        assert_eq!(hide_widgets(&all, &[name("size"), HideTarget::Widget(second)]).len(), 2, "each widget once");
        assert!(hide_widgets(&all, &[name("nobody"), HideTarget::Widget(obj(9999))]).is_empty());
        let kids = named_fields(&["a", "a.b", "ab"]);
        let hit: Vec<String> = hide_widgets(&kids, &[name("a")]).into_iter().map(|(n, _)| n).collect();
        assert_eq!(hit, ["a", "a.b"], "a parent hides its kids, not 'ab'");
    }

    #[test]
    fn hiding_and_showing_store_only_what_differs_from_the_document() {
        let key = |n: &str| (n.to_owned(), 0);
        let base: HashSet<(String, usize)> = [key("own")].into();
        let change = |n: &str, before, after| HideChange { name: n.into(), widget: 0, before, after };
        let none = Hidden::new();
        assert_eq!(hide_changes(&[key("a")], true, &base, &none), [change("a", None, Some(true))]);
        assert_eq!(hide_changes(&[key("a")], false, &base, &none), [], "already showing");
        assert_eq!(hide_changes(&[key("own")], false, &base, &none), [change("own", None, Some(false))]);
        assert_eq!(hide_changes(&[key("own")], true, &base, &none), [], "already hidden by the document");
        let stored: Hidden = [("a".to_owned(), [(0, true)].into()), ("own".to_owned(), [(0, false)].into())].into();
        assert_eq!(hide_changes(&[key("a")], false, &base, &stored), [change("a", Some(true), None)], "back as the document has it");
        assert_eq!(hide_changes(&[key("own")], true, &base, &stored), [change("own", Some(false), None)]);
        assert_eq!(hide_changes(&[key("a"), key("a")], false, &base, &stored).len(), 1, "each widget once");
    }

    #[test]
    fn stored_visibility_drops_a_field_left_with_nothing() {
        let mut stored = Hidden::new();
        store_hidden(&mut stored, "a", 1, Some(true));
        store_hidden(&mut stored, "a", 2, Some(false));
        assert_eq!(stored["a"], [(1, true), (2, false)].into());
        store_hidden(&mut stored, "a", 1, None);
        store_hidden(&mut stored, "a", 2, None);
        store_hidden(&mut stored, "b", 0, None);
        assert!(stored.is_empty());
    }

    #[test]
    fn visibility_survives_the_sidecar() {
        let stored: Hidden = [("a.b".to_owned(), [(0, true), (3, false)].into())].into();
        let json = serde_json::to_string(&stored).unwrap();
        assert_eq!(serde_json::from_str::<Hidden>(&json).unwrap(), stored);
    }

    #[test]
    fn hidden_widgets_render_hidden_and_shown_ones_show() {
        let hidden: Hidden = [("go".to_owned(), [(0, true)].into())].into();
        let built = build(Arc::new(form_pdf()), None, &BTreeMap::new(), &hidden).unwrap();
        let go = |b: &Built| b.fields.iter().find(|f| f.name == "go").unwrap().widgets[0].clone();
        assert!(go(&built).hidden && !usable(&go(&built)), "a hidden button can't be pressed");
        assert!(built.base_hidden.is_empty(), "the document itself hides nothing");
        let render = built.render.expect("rendered from a copy");
        // Shown again, from a document that hides it.
        let shown: Hidden = [("go".to_owned(), [(0, false)].into())].into();
        let again = build(Arc::new(render), None, &BTreeMap::new(), &shown).unwrap();
        assert!(again.base_hidden.contains(&("go".to_owned(), 0)));
        assert!(!go(&again).hidden && usable(&go(&again)));
    }

    #[test]
    fn showing_clears_no_view_and_hiding_keeps_the_other_flags() {
        let flags = |widget_extra: &str, hide: bool| {
            let mut doc = button_pdf("", widget_extra);
            let all = fields(&doc);
            apply_hidden(&mut doc, &all, &[("go".to_owned(), [(0, hide)].into())].into());
            doc.get(obj(5)).as_dict().unwrap().get(b"F").and_then(Object::as_int)
        };
        assert_eq!(flags("/F 36", false), Some(4), "NoView and Hidden go, Print stays");
        assert_eq!(flags("/F 4", true), Some(6));
        assert_eq!(flags("", true), Some(2));
        // A widget index the field doesn't have is left alone.
        let mut doc = button_pdf("", "/F 4");
        let all = fields(&doc);
        apply_hidden(&mut doc, &all, &[("go".to_owned(), [(7, true)].into())].into());
        assert_eq!(doc.get(obj(5)).as_dict().unwrap().get(b"F").and_then(Object::as_int), Some(4));
    }

    #[test]
    fn action_in_aa_u_is_read() {
        let d = button_pdf("", "/AA << /U << /S /Named /N /LastPage >> >>");
        assert_eq!(cmd(&d), ButtonCmd::Named("LastPage".into()));
    }

    #[test]
    fn javascript_with_exact_calls_is_never_believed() {
        for js in ["this.pageNum++;", "this.resetForm();", "app.launchURL(\"https://evil.example/\");", "this.print();", "app.alert(\"x\");"] {
            let d = button_pdf("", &format!("/A << /S /JavaScript /JS ({js}) >>"));
            assert_eq!(cmd(&d), ButtonCmd::Script, "{js}");
            // And in /AA /U
            let d = button_pdf("", &format!("/AA << /U << /S /JavaScript /JS ({js}) >> >>"));
            assert_eq!(cmd(&d), ButtonCmd::Script, "AA/U {js}");
            // And on the field instead of the widget
            let d = button_pdf(&format!("/A << /S /JavaScript /JS ({js}) >>"), "");
            assert_eq!(cmd(&d), ButtonCmd::Script, "field {js}");
        }
    }

    /// The widget has a plain PDF action; the field above it carries a script that the library
    /// reads as a Named action. The library reads the field first; `button_command` reads the
    /// widget first, so the two sources can be mixed.
    #[test]
    fn widget_action_is_not_mixed_with_the_script_of_its_field() {
        let d = button_pdf("/A << /S /JavaScript /JS (this.pageNum--;) >>", "/A << /S /Named /N /NextPage >>");
        let f = field(&d, "go");
        let c = button_command(&d, &f, 0);
        assert_eq!(c, ButtonCmd::Named("NextPage".into()), "library says {:?}", f.button);
    }

    #[test]
    fn widget_reset_is_not_mixed_with_the_reset_of_its_field() {
        let d = button_pdf("/A << /S /ResetForm /Fields [(a)] >>", "/A << /S /ResetForm /Fields [(b)] >>");
        let f = field(&d, "go");
        assert_eq!(button_command(&d, &f, 0), ButtonCmd::Reset { fields: Some(vec!["b".into()]), exclude: false }, "library says {:?}", f.button);
    }

    #[test]
    fn goto_to_a_missing_page_is_unsupported_not_a_panic() {
        let d = button_pdf("", "/A << /S /GoTo /D [99 0 R /Fit] >>");
        assert_eq!(cmd(&d), ButtonCmd::Unsupported);
        let d = button_pdf("", "/A << /S /GoTo /D (named) >>");
        assert_eq!(cmd(&d), ButtonCmd::Unsupported);
    }

    #[test]
    fn action_that_is_not_a_dict_is_unsupported() {
        let d = button_pdf("", "/A 42");
        let c = cmd(&d);
        assert!(matches!(c, ButtonCmd::Unsupported | ButtonCmd::Nothing), "{c:?}");
    }

    fn named_fields(names: &[&str]) -> Vec<Field> {
        let template = field(&open(form_pdf()), "name");
        names.iter().map(|n| Field { name: (*n).to_owned(), ..template.clone() }).collect()
    }

    #[test]
    fn reset_name_matching() {
        let all = named_fields(&["a", "a.b", "a.c", "x.b", "ab", "b"]);
        let r = |n: &[&str], ex| reset_names(&all, Some(&n.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>()), ex);
        assert_eq!(r(&["a"], false), ["a", "a.b", "a.c"], "a parent resets its kids, not 'ab'");
        assert_eq!(r(&["b"], false), ["b"], "a name is fully qualified: not a.b or x.b");
        assert_eq!(r(&["a"], true), ["x.b", "ab", "b"]);
        assert_eq!(reset_names(&all, None, true).len(), 6, "no /Fields: everything, even when excluding");
        assert_eq!(r(&[], false), Vec::<String>::new(), "names that resolved to nothing reset nothing");
        assert_eq!(r(&[], true).len(), 6, "and exclude nothing");
        assert_eq!(r(&["zzz"], false), Vec::<String>::new());
        assert_eq!(r(&["a.b"], false), ["a.b"]);
    }

    #[test]
    fn max_len_in_characters_not_bytes() {
        let mut f = field(&open(form_pdf()), "name");
        f.max_len = Some(4);
        assert!(check_length(&f, "日本語字").is_ok(), "4 chars, 12 bytes");
        assert!(check_length(&f, "日本語字x").is_err());
        assert!(check_length(&f, "").is_ok());
        f.max_len = Some(0);
        assert!(check_length(&f, "").is_ok());
        assert!(check_length(&f, "a").is_err());
        // a combining sequence counts as two chars; documented behaviour here, just pin it
        f.max_len = Some(1);
        assert!(check_length(&f, "e\u{301}").is_err());
    }

    #[test]
    fn form_value_serde_round_trip() {
        for v in [
            FormValue::Text("é\n\"q\"".into()),
            FormValue::Text(String::new()),
            FormValue::Check(false),
            FormValue::Radio(None),
            FormValue::Radio(Some("On".into())),
            FormValue::Choice(vec![]),
            FormValue::Choice(vec!["a".into(), "b".into()]),
        ] {
            let json = serde_json::to_string(&v).unwrap();
            assert_eq!(serde_json::from_str::<FormValue>(&json).unwrap(), v, "{json}");
        }
    }

    #[test]
    fn build_survives_garbage() {
        let inputs: Vec<Vec<u8>> = vec![
            vec![],
            b"%PDF-1.7\n".to_vec(),
            b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /AcroForm 99 0 R >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF".to_vec(),
            vec![0xff; 4096],
            raw_pdf(&["<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [1 0 R 1 0 R 7 0 R] >> >>", "<< /Type /Pages /Kids [2 0 R] /Count 1 >>"]),
            // field that is its own kid
            raw_pdf(&[
                "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
                "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1 1] >>",
                "<< /FT /Tx /T (a) /Kids [4 0 R] >>",
            ]),
        ];
        let set = values(&[("a", FormValue::Text("x".into())), ("b", FormValue::Check(true))]);
        for bytes in inputs {
            let _ = build(Arc::new(bytes.clone()), None, &BTreeMap::new(), &Hidden::new());
            let _ = build(Arc::new(bytes), None, &set, &Hidden::new());
        }
    }

    #[test]
    fn non_form_pdf_with_values_has_no_fields_and_no_failures() {
        let set = values(&[("a", FormValue::Text("x".into()))]);
        let built = build(Arc::new(tiny_pdf()), None, &set, &Hidden::new()).unwrap();
        assert!(built.fields.is_empty());
        assert!(built.render.is_none());
    }

    #[test]
    fn values_for_read_only_field_fail_and_are_reported() {
        let mut doc = open(form_pdf());
        let name = field(&doc, "name").obj;
        doc.update_dict(name, |d| d.set(b"Ff", Object::Int(1))).unwrap();
        let bytes = write_incremental(&doc, &SaveOptions::default()).unwrap();
        let built = build(Arc::new(bytes), None, &values(&[("name", FormValue::Text("x".into()))]), &Hidden::new()).unwrap();
        assert_eq!(built.failed.len(), 1, "{:?}", built.failed);
    }

    #[test]
    fn text_longer_than_maxlen_is_still_filled_by_the_library_when_sidecar_has_it() {
        // The viewer checks MaxLen on entry; here: what does the library do with a longer sidecar value?
        let mut doc = open(form_pdf());
        let name = field(&doc, "name").obj;
        doc.update_dict(name, |d| d.set(b"MaxLen", Object::Int(2))).unwrap();
        let bytes = write_incremental(&doc, &SaveOptions::default()).unwrap();
        let built = build(Arc::new(bytes), None, &values(&[("name", FormValue::Text("abcdef".into()))]), &Hidden::new()).unwrap();
        eprintln!("failed={:?} value={:?}", built.failed, built.fields.iter().find(|f| f.name == "name").map(|f| f.value.clone()));
    }

    #[test]
    fn radio_with_unknown_state_and_choice_with_unknown_value() {
        let set = values(&[("size", FormValue::Radio(Some("XXL".into())))]);
        let built = build(Arc::new(form_pdf()), None, &set, &Hidden::new()).unwrap();
        eprintln!("radio unknown: failed={:?}", built.failed);
        let set = values(&[("colour", FormValue::Choice(vec!["Mauve".into()]))]);
        let built = build(Arc::new(form_pdf()), None, &set, &Hidden::new()).unwrap();
        eprintln!("combo unknown: failed={:?}", built.failed);
    }

    #[test]
    fn fill_with_nothing_changes_nothing() {
        let mut doc = open(form_pdf());
        assert_eq!(fill(&mut doc, &BTreeMap::new()), (0, vec![], None));
        assert!(!doc.is_modified());
    }

    #[test]
    fn unchecking_and_clearing_through_fill() {
        let bytes = form_pdf();
        let set = values(&[("agree", FormValue::Check(true)), ("name", FormValue::Text("x".into()))]);
        let on = build(Arc::new(bytes), None, &set, &Hidden::new()).unwrap().render.unwrap();
        let set = values(&[("agree", FormValue::Check(false)), ("name", FormValue::Text(String::new()))]);
        let off = build(Arc::new(on), None, &set, &Hidden::new()).unwrap();
        let by = |n: &str| FormValue::of(off.fields.iter().find(|f| f.name == n).unwrap());
        assert_eq!(by("agree"), FormValue::Check(false));
        assert_eq!(by("name"), FormValue::Text(String::new()));
        assert_eq!(off.base["agree"], FormValue::Check(true), "base is the document it was given");
    }

    #[test]
    fn a_reset_names_its_fields_by_string_or_by_reference() {
        let d = button_pdf("", "/A << /S /ResetForm /Fields [4 0 R (other.x)] >>");
        assert_eq!(cmd(&d), ButtonCmd::Reset { fields: Some(vec!["go".into(), "other.x".into()]), exclude: false });
        let d = button_pdf("", "/A << /S /ResetForm /Fields [5 0 R] >>");
        assert_eq!(cmd(&d), ButtonCmd::Reset { fields: Some(vec!["go".into()]), exclude: false }, "a widget stands for its field");
        let d = button_pdf("", "/A << /S /ResetForm /Fields [42 7 R] /Flags 1 >>");
        assert_eq!(cmd(&d), ButtonCmd::Reset { fields: Some(vec![]), exclude: true }, "given but unresolved is not 'all'");
    }

    #[test]
    fn a_goto_resolves_a_named_destination() {
        let d = button_pdf("", "/A << /S /GoTo /D /Top >>");
        assert_eq!(cmd(&d), ButtonCmd::Unsupported);
        let mut doc = button_pdf("", "/A << /S /GoTo /D (Top) >>");
        let root = doc.root().unwrap();
        let dests = Dict::new();
        let mut dests = dests;
        dests.set(b"Top", Object::Array(vec![Object::Ref(pdfcraft_cos::ObjRef::new(3, 0)), Object::name("Fit")]));
        doc.update_dict(root, |d| d.set(b"Dests", Object::Dict(dests))).unwrap();
        assert_eq!(cmd(&doc), ButtonCmd::GoTo(0));
    }

    #[test]
    fn a_failed_recalculation_is_told_apart_from_a_failed_value() {
        // Nothing to recalculate here, so no error: a value alone that fails is a failure of its own.
        let built = build(Arc::new(form_pdf()), None, &values(&[("nobody", FormValue::Text("x".into()))]), &Hidden::new()).unwrap();
        assert!(built.recalc.is_none());
        assert_eq!(built.failed.len(), 1);
        assert_eq!(built.tried["nobody"], FormValue::Text("x".into()), "the values tried come back");
    }

    #[test]
    fn multiline_text_keeps_its_breaks_through_the_bar() {
        assert_eq!(bar_text("a\r\nb\nc\rd", true), "a\\nb\\nc\\nd");
        assert_eq!(field_text("a\\nb", true), "a\nb");
        assert_eq!(field_text(&bar_text("one\ntwo", true), true), "one\ntwo");
        assert_eq!(bar_text("a\nb", false), "a b");
        assert_eq!(field_text("a\\nb", false), "a\\nb", "only a multiline field has breaks");
    }

    #[test]
    fn clearing_a_choice_stores_no_empty_value() {
        let mut f = choice(&[("r", "Red")], 0);
        f.value = vec![String::new()];
        assert_eq!(FormValue::of(&f), FormValue::Choice(Vec::new()));
        f.value = vec!["r".into()];
        assert_eq!(FormValue::of(&f), FormValue::Choice(vec!["r".into()]));
    }
    // ---- attack tests ----

    /// Runs `f` on another thread and fails if it panics or takes more than 20 seconds.
    fn guarded<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(catch_unwind(AssertUnwindSafe(f)));
        });
        match rx.recv_timeout(std::time::Duration::from_secs(20)) {
            Ok(Ok(v)) => v,
            Ok(Err(_)) => panic!("panicked"),
            Err(_) => panic!("hung or too slow"),
        }
    }

    /// Objects 1-3 as `raw_pdf` wants them, object 4 the button field, 5 its widget, then `more`.
    fn hostile_pdf(field_extra: &str, widget_extra: &str, catalog_extra: &str, more: &[String]) -> Vec<u8> {
        let field = format!("<< /FT /Btn /Ff 65536 /T (go) /Kids [5 0 R] {field_extra} >>");
        let widget = format!("<< /Type /Annot /Subtype /Widget /Parent 4 0 R /Rect [10 10 90 30] /P 3 0 R {widget_extra} >>");
        let catalog = format!("<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> {catalog_extra} >>");
        let mut objs: Vec<String> = vec![
            catalog,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << >> /Annots [5 0 R] >>".into(),
            field,
            widget,
        ];
        objs.extend(more.iter().cloned());
        let refs: Vec<&str> = objs.iter().map(String::as_str).collect();
        raw_pdf(&refs)
    }

    /// The command of the button of a hostile document, with `build` run over it too; must return.
    fn hostile(field_extra: &str, widget_extra: &str, catalog_extra: &str, more: Vec<String>) -> ButtonCmd {
        let bytes = hostile_pdf(field_extra, widget_extra, catalog_extra, &more);
        guarded(move || {
            let _ = build(Arc::new(bytes.clone()), None, &BTreeMap::new(), &Hidden::new()).expect("build");
            let doc = open(bytes);
            button_command(&doc, &field(&doc, "go"), 0)
        })
    }

    #[test]
    fn hostile_parent_cycle_in_reset_fields() {
        // 6 and 7 are fields that are each other's parent; 8 is its own parent.
        let more = vec!["<< /T (x) /Parent 7 0 R >>".to_owned(), "<< /T (y) /Parent 6 0 R >>".to_owned(), "<< /T (z) /Parent 8 0 R >>".to_owned()];
        let c = hostile("", "/A << /S /ResetForm /Fields [6 0 R 7 0 R 8 0 R] >>", "", more);
        match c {
            ButtonCmd::Reset { fields: Some(f), exclude: false } => assert_eq!(f.len(), 3, "{f:?}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn hostile_very_long_parent_chain() {
        let mut more = Vec::new();
        for i in 0..5000 {
            more.push(format!("<< /T (p{i}) /Parent {} 0 R >>", 6 + i + 1));
        }
        more.push("<< /T (end) >>".into());
        let c = hostile("", "/A << /S /ResetForm /Fields [6 0 R] >>", "", more);
        assert!(matches!(c, ButtonCmd::Reset { fields: Some(_), .. }), "{c:?}");
    }

    #[test]
    fn hostile_dests_tree_cyclic_kids() {
        let more = vec!["<< /Kids [6 0 R 7 0 R] >>".to_owned(), "<< /Kids [6 0 R 7 0 R] /Limits [(a) (z)] >>".to_owned()];
        let c = hostile("", "/A << /S /GoTo /D (nothere) >>", "/Names << /Dests 6 0 R >>", more);
        assert_eq!(c, ButtonCmd::Unsupported);
    }

    #[test]
    fn hostile_dests_tree_very_deep() {
        let mut more = Vec::new();
        for i in 0..20_000 {
            more.push(format!("<< /Kids [{} 0 R] >>", 6 + i + 1));
        }
        more.push("<< /Names [(deep) [3 0 R /Fit]] >>".into());
        let c = hostile("", "/A << /S /GoTo /D (deep) >>", "/Names << /Dests 6 0 R >>", more);
        // Too deep to be read: unsupported, not a stack overflow.
        assert_eq!(c, ButtonCmd::Unsupported);
    }

    #[test]
    fn hostile_dests_tree_very_wide() {
        let kids: String = (0..200_000).map(|_| "99 0 R ").collect();
        let more = vec![format!("<< /Kids [{kids}] >>")];
        let c = hostile("", "/A << /S /GoTo /D (w) >>", "/Names << /Dests 6 0 R >>", more);
        assert_eq!(c, ButtonCmd::Unsupported);
    }

    #[test]
    fn hostile_dests_tree_found_normally() {
        let more = vec!["<< /Kids [7 0 R] >>".to_owned(), "<< /Names [(a) [3 0 R /Fit] (b) [3 0 R /XYZ 0 0 0]] >>".to_owned()];
        let c = hostile("", "/A << /S /GoTo /D (b) >>", "/Names << /Dests 6 0 R >>", more);
        assert_eq!(c, ButtonCmd::GoTo(0));
    }

    #[test]
    fn hostile_dests_tree_odd_shapes() {
        for names in ["/Names 5", "/Names (str)", "/Names [(a)]", "/Names [1 2 3]", "/Kids 7", "/Kids [(x) 5 /n null]", "/Names << >>"] {
            let c = hostile("", "/A << /S /GoTo /D (a) >>", "/Names << /Dests 6 0 R >>", vec![format!("<< {names} >>")]);
            assert_eq!(c, ButtonCmd::Unsupported, "{names}");
        }
        for names in ["/Names 5", "/Names [ ]", "/Names null", "/Names (x)", "/Names [6 0 R]"] {
            let c = hostile("", "/A << /S /GoTo /D (a) >>", &format!("{names} /Dests 7"), vec!["null".into(), "5".into()]);
            assert_eq!(c, ButtonCmd::Unsupported, "{names}");
        }
    }

    #[test]
    fn hostile_goto_targets() {
        let cases = [
            ("/D [4 0 R /Fit]", "a field, not a page"),
            ("/D [2 0 R /Fit]", "the page tree"),
            ("/D [1 0 R /Fit]", "the catalog"),
            ("/D [99 0 R /Fit]", "missing"),
            ("/D [0 /Fit]", "an integer page index (valid only for remote)"),
            ("/D [7]", "an integer"),
            ("/D []", "empty"),
            ("/D 5", "integer"),
            ("/D null", "null"),
            ("/D 6 0 R", "a cycle through a dict"),
            ("/D /A", "a name with a cycle"),
            ("/D << /D << /D << >> >> >>", "nested dicts"),
            ("/D", "nothing"),
        ];
        for (d, why) in cases {
            let more = vec!["<< /D 6 0 R >>".to_owned()];
            let c = hostile("", &format!("/A << /S /GoTo {d} >>"), "/Dests << /A /A >>", more);
            assert_eq!(c, ButtonCmd::Unsupported, "{why}: {d}");
        }
        let c = hostile("", "/A << /S /GoTo >>", "", vec![]);
        assert_eq!(c, ButtonCmd::Unsupported);
        // Dict with /D chain to a page is fine.
        let c = hostile("", "/A << /S /GoTo /D << /D [3 0 R /Fit] >> >>", "", vec![]);
        assert_eq!(c, ButtonCmd::GoTo(0));
    }

    #[test]
    fn hostile_non_dict_and_non_array_everywhere() {
        let actions = [
            "/A 5",
            "/A (s)",
            "/A [1 2]",
            "/A null",
            "/A /Name",
            "/A 99 0 R",
            "/A 6 0 R",
            "/AA 5",
            "/AA (s)",
            "/AA << /U 5 >>",
            "/AA << /U null >>",
            "/AA << /U [1] >>",
            "/A << /S 5 >>",
            "/A << /S (JavaScript) >>",
            "/A << /S [/JavaScript] >>",
            "/A << >>",
            "/A << /S /ResetForm /Fields 5 >>",
            "/A << /S /ResetForm /Fields (a) >>",
            "/A << /S /ResetForm /Fields << >> >>",
            "/A << /S /ResetForm /Fields [5 [6] << >> null true] /Flags (x) >>",
            "/A << /S /ResetForm /Fields [[(a)]] >>",
            "/A << /S /ResetForm /Flags [1] >>",
            "/A << /S /ResetForm /Flags 1.5 >>",
            "/A << /S /ResetForm /Flags /One >>",
            "/A << /S /ResetForm /Flags null >>",
            "/A << /S /ResetForm /Flags 99 0 R >>",
            "/A << /S /ResetForm /Flags -1 >>",
            "/A << /S /ResetForm /Flags 99999999999999999999 >>",
            "/A << /S /Named /N 5 >>",
            "/A << /S /Named /N [/NextPage] >>",
            "/A << /S /Named >>",
            "/A << /S /Named /N << >> >>",
            "/A << /S /URI /URI 5 >>",
            "/A << /S /URI /URI /name >>",
            "/A << /S /URI /URI [(x)] >>",
            "/A << /S /URI >>",
            "/A << /S /URI /URI null >>",
            "/A << /S /GoTo /D 6 0 R >>",
            "/A << /S /Hide /H 5 >>",
            "/A << /S /SubmitForm /F 5 >>",
            "/A << /S /Hide /Next 6 0 R >>",
        ];
        for a in actions {
            // As widget action, as field action, and with the other having a decoy.
            let more = vec!["<< /S /GoTo /D 6 0 R /Next 6 0 R >>".to_owned()];
            let _ = hostile("", a, "", more.clone());
            let _ = hostile(a, "", "", more.clone());
            let _ = hostile(a, "/A << /S /Hide >>", "", more.clone());
            // Also on a doc where the catalog's pieces are of the wrong type.
            let _ = hostile("", a, "/Dests 5 /Names 6", more);
        }
    }

    #[test]
    fn hostile_odd_values_read_as_documented() {
        // A URI that is not a string.
        for u in ["5", "/name", "[(x)]", "null", "<< >>", "true"] {
            let c = hostile("", &format!("/A << /S /URI /URI {u} >>"), "", vec![]);
            assert_eq!(c, ButtonCmd::Unsupported, "URI {u}");
        }
        // Flags that are not an integer: not excluding.
        for f in ["(x)", "/One", "[1]", "null", "true", "<< >>"] {
            let c = hostile("", &format!("/A << /S /ResetForm /Flags {f} >>"), "", vec![]);
            assert_eq!(c, ButtonCmd::Reset { fields: None, exclude: false }, "Flags {f}");
        }
        let c = hostile("", "/A << /S /ResetForm /Flags 3 >>", "", vec![]);
        assert_eq!(c, ButtonCmd::Reset { fields: None, exclude: true });
        let c = hostile("", "/A << /S /ResetForm /Flags 2 >>", "", vec![]);
        assert_eq!(c, ButtonCmd::Reset { fields: None, exclude: false });
        // Flags through a reference.
        let c = hostile("", "/A << /S /ResetForm /Flags 6 0 R >>", "", vec!["1".into()]);
        assert_eq!(c, ButtonCmd::Reset { fields: None, exclude: true });
        // Named with a name and with a string.
        let c = hostile("", "/A << /S /Named /N (PrevPage) >>", "", vec![]);
        assert_eq!(c, ButtonCmd::Named("PrevPage".into()));
        for n in ["5", "[/X]", "null"] {
            let c = hostile("", &format!("/A << /S /Named /N {n} >>"), "", vec![]);
            assert_eq!(c, ButtonCmd::Unsupported, "N {n}");
        }
    }

    /// Every kind of action, written as a dictionary, with what it should read as.
    fn kinds() -> Vec<(&'static str, &'static str, ButtonCmd)> {
        vec![
            ("js", "<< /S /JavaScript /JS (this.pageNum++;) >>", ButtonCmd::Script),
            ("named", "<< /S /Named /N /NextPage >>", ButtonCmd::Named("NextPage".into())),
            ("goto", "<< /S /GoTo /D [3 0 R /Fit] >>", ButtonCmd::GoTo(0)),
            ("uri", "<< /S /URI /URI (https://u.example/) >>", ButtonCmd::Uri("https://u.example/".into())),
            ("reset", "<< /S /ResetForm /Fields [(q)] >>", ButtonCmd::Reset { fields: Some(vec!["q".into()]), exclude: false }),
        ]
    }

    #[test]
    fn widget_action_wins_over_field_action_for_every_pair() {
        for (wn, w, want) in kinds() {
            for (fname, f, _) in kinds() {
                let c = hostile(&format!("/A {f}"), &format!("/A {w}"), "", vec![]);
                assert_eq!(c, want, "widget {wn} over field {fname}");
                // The widget's mouse-up action wins over the field's /A as well.
                let c = hostile(&format!("/A {f}"), &format!("/AA << /U {w} >>"), "", vec![]);
                assert_eq!(c, want, "widget AA/U {wn} over field A {fname}");
            }
            // Only on the field: it is read from there.
            let c = hostile(&format!("/A {w}"), "", "", vec![]);
            assert_eq!(c, want, "field alone {wn}");
            let c = hostile(&format!("/AA << /U {w} >>"), "", "", vec![]);
            assert_eq!(c, want, "field AA/U alone {wn}");
            // Only on the widget.
            let c = hostile("", &format!("/A {w}"), "", vec![]);
            assert_eq!(c, want, "widget alone {wn}");
        }
        // JavaScript anywhere that wins always is a Script, and never anything else.
        for (n, w, _) in kinds() {
            let js = "<< /S /JavaScript /JS (app.launchURL('x');) >>";
            assert_eq!(hostile(&format!("/A {w}"), &format!("/A {js}"), "", vec![]), ButtonCmd::Script, "js over {n}");
        }
        // /A and /AA /U on the same dictionary: /A.
        let c = hostile("", "/A << /S /Named /N /A >> /AA << /U << /S /JavaScript >> >>", "", vec![]);
        assert_eq!(c, ButtonCmd::Named("A".into()));
        let c = hostile("", "/A << /S /JavaScript >> /AA << /U << /S /Named /N /A >> >>", "", vec![]);
        assert_eq!(c, ButtonCmd::Script);
    }

    #[test]
    fn javascript_behind_indirect_objects_is_never_a_page_turn() {
        // /S as an indirect name: whatever it reads as, never a Named or a GoTo.
        let c = hostile("", "/A 6 0 R", "", vec!["<< /S 7 0 R /N /NextPage >>".into(), "/JavaScript".into()]);
        assert!(matches!(c, ButtonCmd::Script | ButtonCmd::Unsupported), "{c:?}");
        // A JavaScript action chaining a plain action with /Next: the script alone counts.
        let c = hostile("", "/A << /S /JavaScript /JS () /Next << /S /Named /N /NextPage >> >>", "", vec![]);
        assert_eq!(c, ButtonCmd::Script);
        // A script that is a stream.
        let c = hostile("", "/A << /S /JavaScript /JS 6 0 R >>", "", vec!["<< /Length 4 >>\nstream\nhi()\nendstream".into()]);
        assert_eq!(c, ButtonCmd::Script);
    }

    #[test]
    fn second_widget_is_read_on_its_own() {
        // Two widgets with different actions; the second has none, so the field's is used.
        let field = "<< /FT /Btn /Ff 65536 /T (go) /Kids [5 0 R 6 0 R] /A << /S /Named /N /LastPage >> >>";
        let w = |extra: &str| format!("<< /Type /Annot /Subtype /Widget /Parent 4 0 R /Rect [10 10 90 30] /P 3 0 R {extra} >>");
        let (w1, w2) = (w("/A << /S /Named /N /FirstPage >>"), w(""));
        let bytes = raw_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << >> /Annots [5 0 R 6 0 R] >>",
            field,
            &w1,
            &w2,
        ]);
        let doc = open(bytes);
        let f = field_of(&doc, "go");
        assert_eq!(button_command(&doc, &f, 0), ButtonCmd::Named("FirstPage".into()));
        assert_eq!(button_command(&doc, &f, 1), ButtonCmd::Named("LastPage".into()));
        assert_eq!(button_command(&doc, &f, 2), ButtonCmd::Named("LastPage".into()), "past the end falls to the field");
    }

    fn field_of(doc: &Document, name: &str) -> Field {
        field(doc, name)
    }

    fn named_fields_of(names: &[&str]) -> Vec<Field> {
        let mut doc = open(tiny_pdf());
        let _ = &mut doc;
        names_to_fields(names)
    }

    fn names_to_fields(names: &[&str]) -> Vec<Field> {
        named_fields(names)
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn reset_semantics() {
        let all = named_fields_of(&["a", "a.b", "a.b.c", "ab", "b", "c.a"]);
        let names = |v: Vec<String>| v;
        // No /Fields: everything, whichever the flag.
        assert_eq!(reset_names(&all, None, false), s(&["a", "a.b", "a.b.c", "ab", "b", "c.a"]));
        assert_eq!(reset_names(&all, None, true), s(&["a", "a.b", "a.b.c", "ab", "b", "c.a"]));
        // Present but resolving to nothing: include touches nothing, exclude touches everything.
        let none: Vec<String> = vec![];
        assert_eq!(reset_names(&all, Some(&none), false), Vec::<String>::new());
        assert_eq!(reset_names(&all, Some(&none), true), s(&["a", "a.b", "a.b.c", "ab", "b", "c.a"]));
        // Exact names.
        assert_eq!(reset_names(&all, Some(&s(&["b"])), false), s(&["b"]));
        assert_eq!(reset_names(&all, Some(&s(&["b"])), true), s(&["a", "a.b", "a.b.c", "ab", "c.a"]));
        // Ancestors: "a" matches "a.b" and "a.b.c", "b" does not match "a.b"; "a" does not match "ab".
        assert_eq!(reset_names(&all, Some(&s(&["a"])), false), s(&["a", "a.b", "a.b.c"]));
        assert_eq!(reset_names(&all, Some(&s(&["a.b"])), false), s(&["a.b", "a.b.c"]));
        assert_eq!(reset_names(&all, Some(&s(&["a"])), true), s(&["ab", "b", "c.a"]));
        assert!(!reset_names(&all, Some(&s(&["b"])), false).contains(&"a.b".to_owned()));
        // A descendant name does not match its ancestor.
        assert_eq!(reset_names(&all, Some(&s(&["a.b.c"])), false), s(&["a.b.c"]));
        // Unknown names touch nothing / everything.
        assert_eq!(reset_names(&all, Some(&s(&["zzz"])), false), Vec::<String>::new());
        assert_eq!(reset_names(&all, Some(&s(&["zzz"])), true).len(), 6);
        // Empty string and odd names must not match everything.
        assert_eq!(reset_names(&all, Some(&s(&[""])), false), Vec::<String>::new());
        assert_eq!(reset_names(&all, Some(&s(&["a."])), false), Vec::<String>::new());
        let _ = names(vec![]);
        // Nothing at all.
        assert_eq!(reset_names(&[], None, false), Vec::<String>::new());
    }

    #[test]
    fn reset_fields_present_but_unreadable_through_button_command() {
        let none = ButtonCmd::Reset { fields: Some(vec![]), exclude: true };
        for f in ["[42 7 R]", "[]", "5", "<< >>", "[null 5]"] {
            let c = hostile("", &format!("/A << /S /ResetForm /Fields {f} /Flags 1 >>"), "", vec![]);
            assert_eq!(c, none, "Fields {f}");
        }
        // A bare string is a list of one.
        let c = hostile("", "/A << /S /ResetForm /Fields (a) /Flags 1 >>", "", vec![]);
        assert_eq!(c, ButtonCmd::Reset { fields: Some(vec!["a".into()]), exclude: true });
        // Null, direct or through a reference, is as good as absent: every field.
        for f in ["null", "6 0 R"] {
            let c = hostile("", &format!("/A << /S /ResetForm /Fields {f} /Flags 1 >>"), "", vec!["null".into()]);
            assert_eq!(c, ButtonCmd::Reset { fields: None, exclude: true }, "Fields {f}");
        }
    }

    #[test]
    fn fill_and_build_separate_recalc_from_failures() {
        // Failing values: each is reported, none blames the recalculation, tried is echoed.
        let vals = values(&[
            ("nobody", FormValue::Text("x".into())),
            ("name", FormValue::Text("Ada".into())),
            ("agree", FormValue::Radio(Some("x".into()))),
        ]);
        let built = build(Arc::new(form_pdf()), None, &vals, &Hidden::new()).unwrap();
        assert!(built.recalc.is_none(), "{:?}", built.recalc);
        let failed: Vec<&str> = built.failed.iter().map(|(n, _)| n.as_str()).collect();
        assert!(failed.contains(&"nobody"), "{failed:?}");
        assert!(!failed.contains(&"name"), "{failed:?}");
        assert_eq!(built.tried, vals);
        assert!(built.render.is_some());
        assert_eq!(FormValue::of(built.fields.iter().find(|f| f.name == "name").unwrap()), FormValue::Text("Ada".into()));
        // fill alone on a document with a form.
        let mut doc = open(form_pdf());
        let (n, failed, recalc) = fill(&mut doc, &vals);
        assert_eq!(n + failed.len(), 3);
        assert!(recalc.is_none());
        // Nothing to fill.
        assert_eq!(fill(&mut doc, &BTreeMap::new()), (0, vec![], None));
        // A document with no form: every value fails as such, no recalc error.
        let mut plain = open(tiny_pdf());
        let (n, failed, recalc) = fill(&mut plain, &values(&[("a", FormValue::Text("x".into())), ("b", FormValue::Check(true))]));
        assert_eq!((n, failed.len()), (0, 2));
        assert!(recalc.is_none());
        // Non-form build gives the default Built.
        let b = build(Arc::new(tiny_pdf()), None, &values(&[("a", FormValue::Text("x".into()))]), &Hidden::new()).unwrap();
        assert!(b.recalc.is_none() && b.failed.is_empty() && b.tried.is_empty() && b.render.is_none());
    }

    #[test]
    fn built_tried_echoes_the_values_when_nothing_failed() {
        let vals = values(&[("name", FormValue::Text("Bo".into())), ("agree", FormValue::Check(true))]);
        let b = build(Arc::new(form_pdf()), None, &vals, &Hidden::new()).unwrap();
        assert!(b.failed.is_empty(), "{:?}", b.failed);
        assert!(b.recalc.is_none());
        assert_eq!(b.tried, vals);
        assert_eq!(b.base["name"], FormValue::Text(String::new()));
    }

    #[test]
    fn multiline_round_trip_through_the_bar() {
        let cases = ["a\nb", "a\n", "\n", "a\n\nb", "\n\n\n", "a\r\nb", "a\r\n", "a\r\n\r\nb", "x\ry", "", "no breaks", "trailing\n\n", "\nleading"];
        for c in cases {
            let shown = bar_text(c, true);
            assert!(!shown.contains('\n') && !shown.contains('\r'), "{c:?} -> {shown:?}");
            let back = field_text(&shown, true);
            let want = c.replace("\r\n", "\n").replace('\r', "\n");
            assert_eq!(back, want, "{c:?} via {shown:?}");
            // Editing the bar's text and committing it unchanged is a no-op on the normalized text.
            assert_eq!(field_text(&bar_text(&back, true), true), back);
        }
        assert_eq!(bar_text("a\n", true), "a\\n");
        assert_eq!(bar_text("a\n\nb", true), "a\\n\\nb");
        assert_eq!(bar_text("a\r\nb", true), "a\\nb");
        assert_eq!(bar_text("a\r\n", true), "a\\n");
        // A single-line field flattens, never keeps a break.
        assert_eq!(bar_text("a\r\n\r\nb", false), "a  b");
        assert_eq!(field_text("a\\nb", false), "a\\nb");
    }

    #[test]
    fn multiline_backslash_n_text_survives_the_bar() {
        // A document value with a literal backslash and an n looks like a break once in the bar.
        let original = "C:\\new";
        let back = field_text(&bar_text(original, true), true);
        assert_eq!(back, original, "a literal backslash-n in a multiline value is not turned into a line break");
    }

    #[test]
    fn multiline_escapes_work_in_both_directions() {
        assert_eq!(bar_text("C:\\new", true), "C:\\\\new");
        assert_eq!(bar_text("a\\\nb", true), "a\\\\\\nb", "a backslash, then a break");
        assert_eq!(bar_text("C:\\new", false), "C:\\new", "one line fields keep them as they are");
        assert_eq!(field_text("C:\\\\new", true), "C:\\new");
        assert_eq!(field_text("a\\nb", true), "a\nb");
        assert_eq!(field_text("a\\\\\\nb", true), "a\\\nb");
        assert_eq!(field_text("a\\qb\\", true), "a\\qb\\", "a lone backslash stays");
        assert_eq!(field_text("\\\\\\", true), "\\\\", "an odd one out at the end stays");
        assert_eq!(field_text("C:\\new", false), "C:\\new");
        for text in ["\\", "\\\\", "\\n", "\\\n", "\n\\", "a\\b\nc\\\\n", "\\x\\"] {
            assert_eq!(field_text(&bar_text(text, true), true), text, "{text:?}");
        }
    }

    #[test]
    fn a_shared_name_tree_leaf_with_many_pairs_is_read_once_and_quickly() {
        let pairs: String = (0..300_000).map(|i| format!("(k{i}) [3 0 R /Fit] ")).collect();
        let kids: String = (0..10_000).map(|_| "7 0 R ").collect();
        // The tree is 6, with one leaf, 7, listed over and over; the last key is past what a lookup may examine.
        let more = vec![format!("<< /Kids [{kids}] >>"), format!("<< /Names [{pairs}] >>")];
        let start = std::time::Instant::now();
        let c = hostile("", "/A << /S /GoTo /D (k299999) >>", "/Names << /Dests 6 0 R >>", more.clone());
        assert_eq!(c, ButtonCmd::Unsupported);
        // Generous: this only guards against a hang, not speed.
        assert!(start.elapsed() < std::time::Duration::from_secs(60), "{:?}", start.elapsed());
        // A key early in the leaf is still found.
        let c = hostile("", "/A << /S /GoTo /D (k5) >>", "/Names << /Dests 6 0 R >>", more);
        assert_eq!(c, ButtonCmd::GoTo(0));
    }

    #[test]
    fn name_tree_keys_compare_in_any_text_encoding() {
        let more = vec!["<< /Names [<FEFF00E9> [3 0 R /Fit]] >>".to_owned()];
        let c = hostile("", "/A << /S /GoTo /D <FEFF00E9> >>", "/Names << /Dests 6 0 R >>", more);
        assert_eq!(c, ButtonCmd::GoTo(0));
    }

    #[test]
    fn an_oversize_fields_list_makes_the_button_unsupported() {
        let ok: String = (0..MAX_TARGETS).map(|i| format!("(f{i}) ")).collect();
        let c = hostile("", &format!("/A << /S /ResetForm /Fields [{ok}] >>"), "", vec![]);
        assert!(matches!(c, ButtonCmd::Reset { fields: Some(f), .. } if f.len() == MAX_TARGETS));
        let too_many: String = (0..=MAX_TARGETS).map(|i| format!("(f{i}) ")).collect();
        let c = hostile("", &format!("/A << /S /ResetForm /Fields [{too_many}] /Flags 1 >>"), "", vec![]);
        assert_eq!(c, ButtonCmd::Unsupported, "not cut short, which would reset what the form keeps");
    }

    #[test]
    fn reset_names_reads_the_dotted_prefixes() {
        let all = named_fields(&["a", "a.b", "a.b.c", "ab", "b", "c.a", "x.y.z"]);
        assert_eq!(reset_names(&all, Some(&s(&["a.b", "x.y"])), false), s(&["a.b", "a.b.c", "x.y.z"]));
        assert_eq!(reset_names(&all, Some(&s(&["a.b", "x.y"])), true), s(&["a", "ab", "b", "c.a"]));
        assert_eq!(reset_names(&all, Some(&s(&["x"])), false), s(&["x.y.z"]));
        assert_eq!(reset_names(&all, Some(&s(&["y.z"])), false), Vec::<String>::new(), "a middle part is not a prefix");
        assert_eq!(reset_names(&all, Some(&s(&["a.b.c.d"])), false), Vec::<String>::new());
        let many: Vec<String> = (0..50_000).map(|i| format!("n{i}")).collect();
        assert_eq!(reset_names(&named_fields(&["n49999.z", "q"]), Some(&many), false), s(&["n49999.z"]));
    }

    #[test]
    fn a_null_fields_list_counts_as_absent() {
        let c = hostile("", "/A << /S /ResetForm /Fields null >>", "", vec![]);
        assert_eq!(c, ButtonCmd::Reset { fields: None, exclude: false });
    }

    #[test]
    fn a_rejected_change_gives_back_what_it_replaced() {
        let text = |t: &str| Some(FormValue::Text(t.into()));
        let change = |name: &str, before, after| FormChange { name: name.into(), before, after };
        let mut undo =
            vec![Edit::Form(vec![change("n", None, text("5"))]), Edit::Form(vec![change("n", text("5"), text("abc")), change("m", None, text("x"))])];
        assert_eq!(forget_change(&mut undo, "n", text("abc").as_ref()), Some(text("5")), "the value it replaced");
        assert_eq!(undo.len(), 2, "the step keeps its other change");
        // Restored "5" fails next: its step is the first, which holds it as an after, and no other.
        assert_eq!(forget_change(&mut undo, "n", text("5").as_ref()), Some(None));
        assert_eq!(undo.len(), 1);
        assert_eq!(forget_change(&mut undo, "n", text("5").as_ref()), None, "nothing left: it is removed, not restored");
        assert_eq!(forget_change(&mut undo, "m", text("x").as_ref()), Some(None));
        assert!(undo.is_empty(), "a step with no change left goes");
        assert_eq!(forget_change(&mut undo, "n", None), None);
    }

    /// Text fields, push buttons (merged, in a parent, with two widgets), NeedAppearances set,
    /// and the page's /Annots in the reverse of the /Fields order.
    fn mixed_pdf() -> Vec<u8> {
        let w = |extra: &str| format!("/Type /Annot /Subtype /Widget /Rect [10 10 90 30] /P 3 0 R {extra}");
        let objs: Vec<String> = vec![
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R 7 0 R 8 0 R 11 0 R] /NeedAppearances true /DA (/Helv 0 Tf 0 g) >> >>".into(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << >> /Annots [13 0 R 12 0 R 11 0 R 10 0 R 9 0 R 8 0 R 7 0 R 5 0 R 4 0 R] >>".into(),
            format!("<< /FT /Tx /T (t1) {} >>", w("")),
            format!("<< /FT /Btn /Ff 65536 /T (b1) /A << /S /ResetForm /Fields [(t1)] >> {} >>", w("")),
            "<< /T (grp) /Kids [9 0 R 10 0 R] >>".into(),
            format!("<< /FT /Tx /T (t2) {} >>", w("")),
            format!("<< /FT /Btn /Ff 65536 /T (b2) /A << /S /Named /N /NextPage >> {} >>", w("")),
            format!("<< /FT /Tx /T (x) /Parent 6 0 R {} >>", w("")),
            format!("<< /FT /Btn /Ff 65536 /T (y) /Parent 6 0 R /A << /S /GoTo /D [3 0 R /Fit] >> {} >>", w("")),
            "<< /FT /Btn /Ff 65536 /T (b3) /Kids [12 0 R 13 0 R] >>".into(),
            format!("<< /Parent 11 0 R /A << /S /URI /URI (http://one) >> {} >>", w("")),
            format!("<< /Parent 11 0 R /A << /S /URI /URI (http://two) >> {} >>", w("")),
        ];
        let refs: Vec<&str> = objs.iter().map(String::as_str).collect();
        raw_pdf(&refs)
    }

    #[test]
    fn index_consistency_between_original_and_filled_forms() {
        let bytes = mixed_pdf();
        let orig = open(bytes.clone());
        let of = fields(&orig);
        let names: Vec<&str> = of.iter().map(|f| f.name.as_str()).collect();
        eprintln!("original order: {names:?}");
        assert_eq!(of.len(), 7, "{names:?}");
        let set = values(&[("t1", FormValue::Text("hello".into())), ("t2", FormValue::Text("w".into())), ("grp.x", FormValue::Text("z".into()))]);
        let built = build(Arc::new(bytes.clone()), None, &set, &Hidden::new()).unwrap();
        assert!(built.failed.is_empty(), "{:?}", built.failed);
        let render = built.render.clone().expect("rendered");
        // The same read of the filled render bytes, as the viewer re-reads them.
        let from_render = fields(&open(render));
        let key = |fs: &[Field]| fs.iter().map(|f| (f.name.clone(), f.obj, f.widgets.iter().map(|w| w.obj).collect::<Vec<_>>())).collect::<Vec<_>>();
        assert_eq!(key(&of), key(&built.fields), "original vs form::build's fields");
        assert_eq!(key(&of), key(&from_render), "original vs fields of the filled render bytes");
        // The values did go in.
        assert_eq!(FormValue::of(built.fields.iter().find(|f| f.name == "t1").unwrap()), FormValue::Text("hello".into()));
        // button_at_click: the filled field at `fi`, applied to the original document.
        let mut seen = Vec::new();
        for (fi, f) in built.fields.iter().enumerate() {
            if !matches!(f.kind, FieldKind::PushButton) {
                continue;
            }
            for wi in 0..f.widgets.len() {
                let via_click = button_command(&orig, &built.fields[fi], wi);
                let via_original = button_command(&orig, &of[fi], wi);
                assert_eq!(via_click, via_original, "{} widget {wi}", f.name);
                seen.push((f.name.clone(), wi, via_click));
            }
        }
        eprintln!("{seen:?}");
        let get = |n: &str, i: usize| seen.iter().find(|(a, b, _)| a == n && *b == i).map(|t| t.2.clone());
        assert_eq!(get("b1", 0), Some(ButtonCmd::Reset { fields: Some(vec!["t1".into()]), exclude: false }));
        assert_eq!(get("b2", 0), Some(ButtonCmd::Named("NextPage".into())));
        assert_eq!(get("grp.y", 0), Some(ButtonCmd::GoTo(0)));
        assert_eq!(get("b3", 0), Some(ButtonCmd::Uri("http://one".into())));
        assert_eq!(get("b3", 1), Some(ButtonCmd::Uri("http://two".into())));
        // Wrong lookups (a filled field's index into the render's own bytes) are the same too.
        let fdoc = open(built.render.unwrap());
        for (fi, f) in built.fields.iter().enumerate() {
            for wi in 0..f.widgets.len() {
                if matches!(f.kind, FieldKind::PushButton) {
                    assert_eq!(button_command(&fdoc, f, wi), button_command(&orig, &of[fi], wi), "{} {wi}", f.name);
                }
            }
        }
    }

    #[test]
    fn index_consistency_holds_for_the_real_fixture_with_no_needappearances() {
        let bytes = form_pdf();
        let orig = fields(&open(bytes.clone()));
        let set = values(&[("name", FormValue::Text("Ada".into())), ("agree", FormValue::Check(true))]);
        let built = build(Arc::new(bytes), None, &set, &Hidden::new()).unwrap();
        let k = |fs: &[Field]| fs.iter().map(|f| (f.name.clone(), f.obj, f.widgets.iter().map(|w| w.obj).collect::<Vec<_>>())).collect::<Vec<_>>();
        assert_eq!(k(&orig), k(&built.fields));
        assert_eq!(k(&orig), k(&fields(&open(built.render.unwrap()))));
    }

    /// A page whose /Names /Dests tree is object 6 and whose button goes to key `key`.
    fn dest_lookup(key: &str, more: Vec<String>) -> ButtonCmd {
        hostile("", &format!("/A << /S /GoTo /D {key} >>"), "/Names << /Dests 6 0 R >>", more)
    }

    #[test]
    fn search_tree_shared_huge_leaf_is_fast_and_exact() {
        // Hundreds of kids all pointing at one big leaf (only read once), key found early and absent.
        let pairs: String = (0..200_000).map(|i| format!("(k{i}) [3 0 R /Fit] ")).collect();
        let kids: String = (0..50_000).map(|_| "7 0 R ").collect();
        let more = vec![format!("<< /Kids [{kids}] >>"), format!("<< /Names [{pairs}] >>")];
        let t = std::time::Instant::now();
        assert_eq!(dest_lookup("(k0)", more.clone()), ButtonCmd::GoTo(0));
        assert_eq!(dest_lookup("(missing)", more.clone()), ButtonCmd::Unsupported);
        assert_eq!(dest_lookup("(k150)", more), ButtonCmd::GoTo(0));
        // Generous: this only guards against a hang or a blow-up, not speed.
        assert!(t.elapsed() < std::time::Duration::from_secs(60), "{:?}", t.elapsed());
    }

    #[test]
    fn search_tree_cycles_and_self_reference() {
        let t = std::time::Instant::now();
        // 6 -> 7 -> 6, and 7 also lists itself.
        let more = vec!["<< /Kids [7 0 R] >>".to_owned(), "<< /Kids [6 0 R 7 0 R] /Names [(a) [3 0 R /Fit]] >>".to_owned()];
        assert_eq!(dest_lookup("(a)", more.clone()), ButtonCmd::GoTo(0), "a key in the cycle is found");
        assert_eq!(dest_lookup("(b)", more), ButtonCmd::Unsupported);
        // A node that is its own kid, with nothing else.
        assert_eq!(dest_lookup("(a)", vec!["<< /Kids [6 0 R] >>".to_owned()]), ButtonCmd::Unsupported);
        // Generous: this only guards against a hang, not speed.
        assert!(t.elapsed() < std::time::Duration::from_secs(60));
    }

    #[test]
    fn search_tree_depth_limits() {
        // 10 deep: found. 16 levels below the root: found. 17+: not.
        let chain = |n: usize| -> Vec<String> {
            let mut more = Vec::new();
            for i in 0..n {
                more.push(format!("<< /Kids [{} 0 R] >>", 7 + i));
            }
            more.push("<< /Names [(deep) [3 0 R /Fit]] >>".into());
            more
        };
        assert_eq!(dest_lookup("(deep)", chain(10)), ButtonCmd::GoTo(0));
        assert_eq!(dest_lookup("(deep)", chain(15)), ButtonCmd::GoTo(0));
        // chain(n) puts the leaf n levels below the root.
        let at_limit = (1..40).filter(|&n| dest_lookup("(deep)", chain(n)) == ButtonCmd::GoTo(0)).max();
        eprintln!("deepest leaf found: {at_limit:?} levels below the root");
        assert_eq!(at_limit, Some(16));
        assert_eq!(dest_lookup("(deep)", chain(30_000)), ButtonCmd::Unsupported);
    }

    #[test]
    fn search_tree_keys_in_other_encodings() {
        // "é" as UTF-16BE with BOM, as PDFDocEncoding byte, and as UTF-8 with BOM; also a Name key.
        for key in ["<FEFF00E9>", "<E9>", "<EFBBBFC3A9>", "(\\351)"] {
            let more = vec![format!("<< /Names [{key} [3 0 R /Fit]] >>")];
            let r = dest_lookup("<FEFF00E9>", more.clone());
            let r2 = dest_lookup("<E9>", more.clone());
            let r3 = dest_lookup("(\\351)", more);
            eprintln!("key {key}: {r:?} {r2:?} {r3:?}");
            assert_eq!(r, ButtonCmd::GoTo(0), "tree key {key} looked up as UTF-16");
            assert_eq!(r2, ButtonCmd::GoTo(0), "tree key {key} looked up as latin byte");
            assert_eq!(r3, ButtonCmd::GoTo(0));
        }
        // CJK / astral: a surrogate pair and a long UTF-16 key.
        let more = vec!["<< /Names [<FEFFD83DDE00> [3 0 R /Fit] <FEFF65E5672C> [3 0 R /Fit]] >>".to_owned()];
        assert_eq!(dest_lookup("<FEFFD83DDE00>", more.clone()), ButtonCmd::GoTo(0));
        assert_eq!(dest_lookup("<FEFF65E5672C>", more.clone()), ButtonCmd::GoTo(0));
        assert_eq!(dest_lookup("<FEFF65E5>", more), ButtonCmd::Unsupported);
        // A name that is not a prefix of another.
        let more = vec!["<< /Names [(abc) [3 0 R /Fit]] >>".to_owned()];
        assert_eq!(dest_lookup("(ab)", more.clone()), ButtonCmd::Unsupported);
        assert_eq!(dest_lookup("(abcd)", more), ButtonCmd::Unsupported);
    }

    #[test]
    fn search_tree_oversize_key_is_skipped_not_read() {
        let long = "a".repeat(100_000);
        let more = vec![format!("<< /Names [({long}) [3 0 R /Fit] (b) [3 0 R /Fit]] >>")];
        assert_eq!(dest_lookup("(b)", more), ButtonCmd::GoTo(0));
    }

    fn change(name: &str, before: Option<&str>, after: Option<&str>) -> FormChange {
        let t = |s: &str| FormValue::Text(s.into());
        FormChange { name: name.into(), before: before.map(t), after: after.map(t) }
    }

    type Step = Vec<(String, Option<FormValue>, Option<FormValue>)>;

    fn step_names(undo: &[Edit]) -> Vec<Step> {
        undo.iter()
            .map(|e| match e {
                Edit::Form(c) => c.iter().map(|c| (c.name.clone(), c.before.clone(), c.after.clone())).collect(),
                _ => panic!("not a form step"),
            })
            .collect()
    }

    #[test]
    fn forget_change_picks_the_newest_matching_step_among_several() {
        let t = |s: &str| Some(FormValue::Text(s.into()));
        let mut undo = vec![
            Edit::Form(vec![change("n", None, Some("1"))]),
            Edit::Form(vec![change("n", Some("1"), Some("2"))]),
            Edit::Form(vec![change("n", Some("2"), Some("3")), change("m", None, Some("q"))]),
            Edit::Form(vec![change("m", Some("q"), Some("r"))]),
        ];
        // "3" is rejected: it is in step 2, and the step keeps m.
        assert_eq!(forget_change(&mut undo, "n", t("3").as_ref()), Some(t("2")));
        assert_eq!(undo.len(), 4);
        // "2" now (the restored value fails): step 1 is dropped?  It only had n -> 2, so it goes.
        assert_eq!(forget_change(&mut undo, "n", t("2").as_ref()), Some(t("1")));
        assert_eq!(undo.len(), 3);
        assert_eq!(forget_change(&mut undo, "n", t("1").as_ref()), Some(None));
        assert_eq!(undo.len(), 2, "emptied step dropped");
        assert_eq!(forget_change(&mut undo, "n", t("1").as_ref()), None);
        assert_eq!(step_names(&undo).len(), 2);
    }

    #[test]
    fn forget_change_name_only_in_an_older_step() {
        let t = |s: &str| Some(FormValue::Text(s.into()));
        let mut undo =
            vec![Edit::Form(vec![change("old", None, Some("a"))]), Edit::Form(vec![change("other", None, Some("b"))]), Edit::Added(Vec::new())];
        assert_eq!(forget_change(&mut undo, "old", t("a").as_ref()), Some(None));
        assert_eq!(undo.len(), 2);
        assert!(matches!(undo[0], Edit::Form(_)) && matches!(undo[1], Edit::Added(_)), "other steps and non-form edits untouched");
        assert_eq!(step_names(&undo[..1])[0][0].0, "other");
    }

    #[test]
    fn forget_change_value_that_does_not_match_changes_nothing() {
        let t = |s: &str| Some(FormValue::Text(s.into()));
        let mut undo = vec![Edit::Form(vec![change("n", None, Some("a"))])];
        let snapshot = step_names(&undo);
        assert_eq!(forget_change(&mut undo, "n", t("b").as_ref()), None);
        assert_eq!(forget_change(&mut undo, "n", None), None);
        assert_eq!(forget_change(&mut undo, "other", t("a").as_ref()), None);
        assert_eq!(forget_change(&mut Vec::new(), "n", t("a").as_ref()), None);
        assert_eq!(step_names(&undo), snapshot);
        // A cleared value (after = None) is matched by None.
        let mut undo = vec![Edit::Form(vec![change("n", Some("a"), None)])];
        assert_eq!(forget_change(&mut undo, "n", None), Some(t("a")));
        assert!(undo.is_empty());
    }

    #[test]
    fn forget_change_same_name_twice_in_one_step_and_the_step_is_emptied() {
        let t = |s: &str| Some(FormValue::Text(s.into()));
        let mut undo = vec![Edit::Form(vec![change("n", Some("0"), Some("a")), change("n", Some("0"), Some("a"))])];
        assert_eq!(forget_change(&mut undo, "n", t("a").as_ref()), Some(t("0")));
        assert!(undo.is_empty(), "all copies removed, empty step dropped");
    }

    #[test]
    fn multiline_escaping_round_trips_over_generated_strings() {
        // xorshift, no dependency.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let alphabet = ['\\', 'n', '\n', '\r', 'a', ' ', 'é', '\u{1F600}', '"', 't'];
        for _ in 0..50_000 {
            let len = (next() % 14) as usize;
            let raw: String = (0..len).map(|_| alphabet[(next() % alphabet.len() as u64) as usize]).collect();
            let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
            let shown = bar_text(&raw, true);
            assert!(!shown.contains('\n') && !shown.contains('\r'), "{raw:?} -> {shown:?}");
            assert_eq!(field_text(&shown, true), normalized, "{raw:?} via {shown:?}");
            // single line: field_text is the identity, bar_text is one_line.
            assert_eq!(field_text(&raw, false), raw);
            assert_eq!(bar_text(&raw, false), one_line(&raw));
            // the stable property too: normalized text round trips exactly
            assert_eq!(field_text(&bar_text(&normalized, true), true), normalized);
        }
    }

    #[test]
    fn multiline_typed_text_with_a_lone_trailing_backslash() {
        assert_eq!(field_text("abc\\", true), "abc\\");
        assert_eq!(field_text("\\", true), "\\");
        assert_eq!(field_text("a\\\\\\", true), "a\\\\", "pair + lone");
        assert_eq!(field_text("\\q", true), "\\q");
        // And as it comes back out of the bar: a value ending in a backslash is doubled, so it is not lone.
        assert_eq!(bar_text("abc\\", true), "abc\\\\");
        assert_eq!(field_text(&bar_text("abc\\", true), true), "abc\\");
        // Single line fields never touch it.
        assert_eq!(field_text("abc\\", false), "abc\\");
        assert_eq!(field_text("a\\nb\\\\", false), "a\\nb\\\\");
    }

    #[test]
    fn reset_names_with_a_hostile_name_and_many_dots_is_fast() {
        let hostile = format!("a{}", ".".repeat(200_000));
        let all = named_fields(&[hostile.as_str(), "a.b", "c"]);
        let t = std::time::Instant::now();
        let hit = reset_names(&all, Some(&["a".to_owned()]), false);
        let miss = reset_names(&all, Some(&["c".to_owned()]), true);
        let exact = reset_names(&all, Some(std::slice::from_ref(&hostile)), false);
        // Generous: this only guards against a hang, not speed.
        assert!(t.elapsed() < std::time::Duration::from_secs(60), "{:?}", t.elapsed());
        assert_eq!(hit, ["a.b"], "a name over the cap is only matched whole");
        assert_eq!(miss, [hostile.clone(), "a.b".to_owned()]);
        assert_eq!(exact, [hostile]);
    }

    #[test]
    fn a_destination_name_over_the_cap_makes_the_button_unsupported() {
        let long = "a".repeat(5000);
        let more = vec![format!("<< /Names [({long}) [3 0 R /Fit] (b) [3 0 R /Fit]] >>")];
        assert_eq!(dest_lookup(&format!("({long})"), more.clone()), ButtonCmd::Unsupported);
        assert_eq!(dest_lookup("(b)", more), ButtonCmd::GoTo(0));
    }

    #[test]
    fn reset_names_ten_thousand_by_ten_thousand_is_fast() {
        let field_names: Vec<String> = (0..10_000).map(|i| format!("form.f{i}.sub")).collect();
        let refs: Vec<&str> = field_names.iter().map(String::as_str).collect();
        let all = named_fields(&refs);
        let targets: Vec<String> = (0..10_000).map(|i| format!("form.f{}", i * 2)).collect();
        let t = std::time::Instant::now();
        let hit = reset_names(&all, Some(&targets), false);
        let miss = reset_names(&all, Some(&targets), true);
        let all_of_it = reset_names(&all, None, false);
        let el = t.elapsed();
        assert_eq!(hit.len(), 5_000);
        assert_eq!(miss.len(), 5_000);
        assert_eq!(all_of_it.len(), 10_000);
        assert_eq!(hit[0], "form.f0.sub");
        assert!(el < std::time::Duration::from_secs(60), "{el:?}");
        eprintln!("reset_names 10k x 10k: {el:?}");
    }
}
