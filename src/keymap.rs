//! Key notation, bindings and the vim-style pending state: counts and multi-key sequences.
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::field::Edit;

use serde::Deserialize;

/// How long a sequence that is both complete and the start of a longer one waits for more keys.
const AMBIGUOUS_WAIT: Duration = Duration::from_millis(800);

/// Everything a key can be bound to. The snake_case names are the `[keys]` table's keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Cancel,
    ScrollDown,
    ScrollUp,
    ScrollLeft,
    ScrollRight,
    HalfDown,
    HalfUp,
    ScreenDown,
    ScreenUp,
    NextPage,
    PrevPage,
    GotoFirst,
    GotoLast,
    EdgeLeft,
    EdgeRight,
    Center,
    ZoomIn,
    ZoomOut,
    FitWidth,
    FitHeight,
    RotateCw,
    RotateCcw,
    ToggleTwoPage,
    ToggleCover,
    SearchForward,
    SearchBackward,
    SearchNext,
    SearchPrev,
    SetMark,
    GotoMark,
    JumpBack,
    JumpForward,
    Command,
    HintSelect,
    HintLink,
    HintForm,
    Yank,
    Portal,
    GotoPortal,
    DeletePortal,
    EditPortal,
    TogglePortalWindow,
    ToggleToc,
    ShowBookmarks,
    AddBookmark,
    ToggleStatusbar,
    ToggleHotbar,
    ToolHighlight,
    ToolDraw,
    ToolErase,
    ToolNote,
    ToolFill,
    FillText,
    FillDate,
    FillCheck,
    FillCross,
    FillDot,
    FillLine,
    FillSign,
    ToolWidthDown,
    ToolWidthUp,
    DeleteAnnotation,
    Undo,
    Redo,
    ToggleTheme,
    ToggleLinks,
    ToggleFields,
    SmartJump,
    Overview,
    GotoOverview,
    OverviewNext,
    OverviewPrev,
    ToggleFullscreen,
    TogglePresentation,
    Reload,
    Quit,
    Open,
    Protect,
    Unprotect,
    EditLeft,
    EditRight,
    EditWordLeft,
    EditWordRight,
    EditStart,
    EditEnd,
    EditBackspace,
    EditDelete,
    EditEraseWord,
    EditDeleteWord,
    EditEraseBlank,
    EditEraseToStart,
    EditEraseToEnd,
    EditUndo,
}

impl Action {
    /// Every action, in declaration order. `the_list_of_actions_is_complete` keeps it whole.
    pub const ALL: [Self; 93] = [
        Self::Cancel,
        Self::ScrollDown,
        Self::ScrollUp,
        Self::ScrollLeft,
        Self::ScrollRight,
        Self::HalfDown,
        Self::HalfUp,
        Self::ScreenDown,
        Self::ScreenUp,
        Self::NextPage,
        Self::PrevPage,
        Self::GotoFirst,
        Self::GotoLast,
        Self::EdgeLeft,
        Self::EdgeRight,
        Self::Center,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::FitWidth,
        Self::FitHeight,
        Self::RotateCw,
        Self::RotateCcw,
        Self::ToggleTwoPage,
        Self::ToggleCover,
        Self::SearchForward,
        Self::SearchBackward,
        Self::SearchNext,
        Self::SearchPrev,
        Self::SetMark,
        Self::GotoMark,
        Self::JumpBack,
        Self::JumpForward,
        Self::Command,
        Self::HintSelect,
        Self::HintLink,
        Self::HintForm,
        Self::Yank,
        Self::Portal,
        Self::GotoPortal,
        Self::DeletePortal,
        Self::EditPortal,
        Self::TogglePortalWindow,
        Self::ToggleToc,
        Self::ShowBookmarks,
        Self::AddBookmark,
        Self::ToggleStatusbar,
        Self::ToggleHotbar,
        Self::ToolHighlight,
        Self::ToolDraw,
        Self::ToolErase,
        Self::ToolNote,
        Self::ToolFill,
        Self::FillText,
        Self::FillDate,
        Self::FillCheck,
        Self::FillCross,
        Self::FillDot,
        Self::FillLine,
        Self::FillSign,
        Self::ToolWidthDown,
        Self::ToolWidthUp,
        Self::DeleteAnnotation,
        Self::Undo,
        Self::Redo,
        Self::ToggleTheme,
        Self::ToggleLinks,
        Self::ToggleFields,
        Self::SmartJump,
        Self::Overview,
        Self::GotoOverview,
        Self::OverviewNext,
        Self::OverviewPrev,
        Self::ToggleFullscreen,
        Self::TogglePresentation,
        Self::Reload,
        Self::Quit,
        Self::Open,
        Self::Protect,
        Self::Unprotect,
        Self::EditLeft,
        Self::EditRight,
        Self::EditWordLeft,
        Self::EditWordRight,
        Self::EditStart,
        Self::EditEnd,
        Self::EditBackspace,
        Self::EditDelete,
        Self::EditEraseWord,
        Self::EditDeleteWord,
        Self::EditEraseBlank,
        Self::EditEraseToStart,
        Self::EditEraseToEnd,
        Self::EditUndo,
    ];

    /// The name under `[keys]`, which is also what `:name` runs. The match has no catch-all, so a
    /// new action does not compile until it is named here.
    pub fn name(self) -> &'static str {
        match self {
            Self::Cancel => "cancel",
            Self::ScrollDown => "scroll_down",
            Self::ScrollUp => "scroll_up",
            Self::ScrollLeft => "scroll_left",
            Self::ScrollRight => "scroll_right",
            Self::HalfDown => "half_down",
            Self::HalfUp => "half_up",
            Self::ScreenDown => "screen_down",
            Self::ScreenUp => "screen_up",
            Self::NextPage => "next_page",
            Self::PrevPage => "prev_page",
            Self::GotoFirst => "goto_first",
            Self::GotoLast => "goto_last",
            Self::EdgeLeft => "edge_left",
            Self::EdgeRight => "edge_right",
            Self::Center => "center",
            Self::ZoomIn => "zoom_in",
            Self::ZoomOut => "zoom_out",
            Self::FitWidth => "fit_width",
            Self::FitHeight => "fit_height",
            Self::RotateCw => "rotate_cw",
            Self::RotateCcw => "rotate_ccw",
            Self::ToggleTwoPage => "toggle_two_page",
            Self::ToggleCover => "toggle_cover",
            Self::SearchForward => "search_forward",
            Self::SearchBackward => "search_backward",
            Self::SearchNext => "search_next",
            Self::SearchPrev => "search_prev",
            Self::SetMark => "set_mark",
            Self::GotoMark => "goto_mark",
            Self::JumpBack => "jump_back",
            Self::JumpForward => "jump_forward",
            Self::Command => "command",
            Self::HintSelect => "hint_select",
            Self::HintLink => "hint_link",
            Self::HintForm => "hint_form",
            Self::Yank => "yank",
            Self::Portal => "portal",
            Self::GotoPortal => "goto_portal",
            Self::DeletePortal => "delete_portal",
            Self::EditPortal => "edit_portal",
            Self::TogglePortalWindow => "toggle_portal_window",
            Self::ToggleToc => "toggle_toc",
            Self::ShowBookmarks => "show_bookmarks",
            Self::AddBookmark => "add_bookmark",
            Self::ToggleStatusbar => "toggle_statusbar",
            Self::ToggleHotbar => "toggle_hotbar",
            Self::ToolHighlight => "tool_highlight",
            Self::ToolDraw => "tool_draw",
            Self::ToolErase => "tool_erase",
            Self::ToolNote => "tool_note",
            Self::ToolFill => "tool_fill",
            Self::FillText => "fill_text",
            Self::FillDate => "fill_date",
            Self::FillCheck => "fill_check",
            Self::FillCross => "fill_cross",
            Self::FillDot => "fill_dot",
            Self::FillLine => "fill_line",
            Self::FillSign => "fill_sign",
            Self::ToolWidthDown => "tool_width_down",
            Self::ToolWidthUp => "tool_width_up",
            Self::DeleteAnnotation => "delete_annotation",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::ToggleTheme => "toggle_theme",
            Self::ToggleLinks => "toggle_links",
            Self::ToggleFields => "toggle_fields",
            Self::SmartJump => "smart_jump",
            Self::Overview => "overview",
            Self::GotoOverview => "goto_overview",
            Self::OverviewNext => "overview_next",
            Self::OverviewPrev => "overview_prev",
            Self::ToggleFullscreen => "toggle_fullscreen",
            Self::TogglePresentation => "toggle_presentation",
            Self::Reload => "reload",
            Self::Quit => "quit",
            Self::Open => "open",
            Self::Protect => "protect",
            Self::Unprotect => "unprotect",
            Self::EditLeft => "edit_left",
            Self::EditRight => "edit_right",
            Self::EditWordLeft => "edit_word_left",
            Self::EditWordRight => "edit_word_right",
            Self::EditStart => "edit_start",
            Self::EditEnd => "edit_end",
            Self::EditBackspace => "edit_backspace",
            Self::EditDelete => "edit_delete",
            Self::EditEraseWord => "edit_erase_word",
            Self::EditDeleteWord => "edit_delete_word",
            Self::EditEraseBlank => "edit_erase_blank",
            Self::EditEraseToStart => "edit_erase_to_start",
            Self::EditEraseToEnd => "edit_erase_to_end",
            Self::EditUndo => "edit_undo",
        }
    }

    /// The action with this name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.name() == name)
    }

    /// Whether the command menu lists this, and `:name` runs it: not `command` and `cancel`, which
    /// only make sense from a key, and not the `edit_*` actions, which work in a text field alone.
    pub fn is_listed(self) -> bool {
        !matches!(self, Self::Command | Self::Cancel) && self.edit().is_none()
    }

    /// What this does to a text field, if it is one of the `edit_*` actions. Those work in a text
    /// field and nowhere else, so they can share keys with what the same keys do in normal mode.
    fn edit(self) -> Option<Edit> {
        Some(match self {
            Self::EditLeft => Edit::Left,
            Self::EditRight => Edit::Right,
            Self::EditWordLeft => Edit::WordLeft,
            Self::EditWordRight => Edit::WordRight,
            Self::EditStart => Edit::Start,
            Self::EditEnd => Edit::End,
            Self::EditBackspace => Edit::Backspace,
            Self::EditDelete => Edit::Delete,
            Self::EditEraseWord => Edit::EraseWord,
            Self::EditDeleteWord => Edit::DeleteWord,
            Self::EditEraseBlank => Edit::EraseBlank,
            Self::EditEraseToStart => Edit::EraseToStart,
            Self::EditEraseToEnd => Edit::EraseToEnd,
            Self::EditUndo => Edit::Undo,
            _ => return None,
        })
    }
}

/// One key press. Printable keys hold their character as typed (`G`, `/`), so shift is only set
/// for named keys and for keys held with control or alt.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Stroke {
    pub key: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Stroke {
    /// A key with no modifiers.
    pub fn plain(key: &str) -> Self {
        Self { key: key.to_owned(), ctrl: false, alt: false, shift: false }
    }

    /// The stroke for a typed character.
    pub fn from_char(c: char) -> Self {
        if c == ' ' { Self::plain("space") } else { Self::plain(&c.to_string()) }
    }

    /// The stroke for a key event, or `None` for a printable key without control or alt: those
    /// arrive as text, which respects the keyboard layout.
    pub fn from_key(key: egui::Key, mods: egui::Modifiers) -> Option<Self> {
        let chord = mods.ctrl || mods.alt;
        let name = match named_key(key) {
            Some(name) => name.to_owned(),
            None if chord => key.symbol_or_name().to_lowercase(),
            None => return None,
        };
        if name == "space" && !chord && !mods.shift {
            return None;
        }
        Some(Self { key: name, ctrl: mods.ctrl, alt: mods.alt, shift: mods.shift })
    }

    /// The character this stroke types into a text field, if it types one.
    pub fn text(&self) -> Option<char> {
        if self.ctrl || self.alt {
            return None;
        }
        if self.key == "space" {
            return Some(' ');
        }
        let mut chars = self.key.chars();
        let c = chars.next()?;
        chars.next().is_none().then_some(c)
    }

    /// Whether this is the named key with no modifiers.
    pub fn is(&self, key: &str) -> bool {
        self.key == key && !self.ctrl && !self.alt && !self.shift
    }

    /// Whether this is control and `key`.
    pub fn is_ctrl(&self, key: &str) -> bool {
        self.key == key && self.ctrl && !self.alt
    }

    /// The stroke in the notation of the `[keys]` table.
    pub fn notation(&self) -> String {
        let mods = format!("{}{}{}", if self.ctrl { "C-" } else { "" }, if self.alt { "A-" } else { "" }, if self.shift { "S-" } else { "" });
        if mods.is_empty() && self.key.chars().count() == 1 { self.key.clone() } else { format!("<{mods}{}>", self.key) }
    }
}

fn named_key(key: egui::Key) -> Option<&'static str> {
    use egui::Key;
    Some(match key {
        Key::Escape => "esc",
        Key::Tab => "tab",
        Key::Enter => "cr",
        Key::Backspace => "bs",
        Key::Delete => "del",
        Key::Space => "space",
        Key::ArrowUp => "up",
        Key::ArrowDown => "down",
        Key::ArrowLeft => "left",
        Key::ArrowRight => "right",
        Key::PageUp => "pageup",
        Key::PageDown => "pagedown",
        Key::Home => "home",
        Key::End => "end",
        Key::F1 => "f1",
        Key::F2 => "f2",
        Key::F3 => "f3",
        Key::F4 => "f4",
        Key::F5 => "f5",
        Key::F6 => "f6",
        Key::F7 => "f7",
        Key::F8 => "f8",
        Key::F9 => "f9",
        Key::F10 => "f10",
        Key::F11 => "f11",
        Key::F12 => "f12",
        _ => return None,
    })
}

/// The canonical name for a key inside `<...>`.
fn canonical_key(name: &str) -> Option<String> {
    if name.chars().count() == 1 {
        return Some(name.to_owned());
    }
    let lower = name.to_lowercase();
    let named = match lower.as_str() {
        "esc" | "escape" => "esc",
        "cr" | "enter" | "return" => "cr",
        "bs" | "backspace" => "bs",
        "del" | "delete" => "del",
        "lt" => "<",
        "gt" => ">",
        "tab" | "space" | "up" | "down" | "left" | "right" | "pageup" | "pagedown" | "home" | "end" => lower.as_str(),
        f if f.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=12).contains(&n)) => f,
        _ => return None,
    };
    Some(named.to_owned())
}

/// Parses what is between `<` and `>`.
fn parse_bracket(token: &str) -> Option<Stroke> {
    let (mut ctrl, mut alt, mut shift) = (false, false, false);
    let mut rest = token;
    // A modifier needs a key after it, so `<C-->` is control and minus.
    while rest.len() > 2 {
        match rest.get(..2)? {
            "C-" | "c-" => ctrl = true,
            "A-" | "a-" | "M-" | "m-" => alt = true,
            "S-" | "s-" => shift = true,
            _ => break,
        }
        rest = &rest[2..];
    }
    let mut key = canonical_key(rest)?;
    if key.chars().count() == 1 {
        if ctrl || alt {
            key = key.to_lowercase();
        } else if shift {
            // `<S-g>` is `G`, as typed; `<S-$>` is `$`.
            key = key.to_uppercase();
            shift = false;
        }
    }
    Some(Stroke { key, ctrl, alt, shift })
}

/// Parses a binding such as `gg`, `<C-d>`, `<S-g>` or `g<S-c>`.
pub fn parse_sequence(text: &str) -> Result<Vec<Stroke>, String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let bracket = (c == '<').then(|| rest[1..].find('>').map(|end| (end, parse_bracket(&rest[1..=end])))).flatten();
        match bracket {
            Some((end, Some(stroke))) => {
                out.push(stroke);
                rest = &rest[end + 2..];
            }
            Some((_, None)) if rest.len() > 1 => return Err(format!("unknown key in \"{text}\"")),
            _ => {
                out.push(Stroke::from_char(c));
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    if out.is_empty() { Err("empty key binding".to_owned()) } else { Ok(out) }
}

/// All bindings.
#[derive(Clone, Debug, Default)]
pub struct Keymap {
    binds: Vec<(Vec<Stroke>, Action)>,
}

impl Keymap {
    /// Builds the keymap from the config table. Invalid bindings are skipped and reported.
    pub fn new(table: &BTreeMap<Action, Vec<String>>) -> (Self, Vec<String>) {
        let mut map = Self::default();
        let mut errors = Vec::new();
        for (action, keys) in table {
            for key in keys {
                match parse_sequence(key) {
                    Ok(sequence) => map.binds.push((sequence, *action)),
                    Err(e) => errors.push(e),
                }
            }
        }
        (map, errors)
    }

    /// Whether this one stroke is bound to `cancel`. A printable key never is here, so that it
    /// can still be typed into a bar: bound to `cancel`, it works in normal mode alone.
    pub fn cancels(&self, stroke: &Stroke) -> bool {
        stroke.text().is_none() && self.binds.iter().any(|(s, a)| *a == Action::Cancel && s.as_slice() == std::slice::from_ref(stroke))
    }

    /// Whether this one stroke alone is bound to `action`. Sequences are not considered.
    pub fn is(&self, stroke: &Stroke, action: Action) -> bool {
        self.binds.iter().any(|(s, a)| *a == action && s.as_slice() == std::slice::from_ref(stroke))
    }

    /// The characters that one printable stroke alone, bound to any of `actions`, types.
    pub fn typed_by(&self, actions: &[Action]) -> Vec<char> {
        self.binds.iter().filter(|(s, a)| actions.contains(a) && s.len() == 1).filter_map(|(s, _)| s[0].text()).collect()
    }

    /// What this one stroke does in a text field. As with `cancel`, a printable key never does
    /// anything here, so that it can still be typed.
    pub fn edit(&self, stroke: &Stroke) -> Option<Edit> {
        if stroke.text().is_some() {
            return None;
        }
        self.binds.iter().find(|(s, a)| a.edit().is_some() && s.as_slice() == std::slice::from_ref(stroke)).and_then(|(_, a)| a.edit())
    }

    fn exact(&self, strokes: &[Stroke]) -> Option<Action> {
        self.binds.iter().find(|(s, a)| s == strokes && a.edit().is_none()).map(|(_, a)| *a)
    }

    fn has_longer(&self, strokes: &[Stroke]) -> bool {
        self.binds.iter().any(|(s, _)| s.len() > strokes.len() && s.starts_with(strokes))
    }
}

/// The keys typed so far of a count and a sequence.
#[derive(Clone, Debug, Default)]
pub struct Pending {
    count: Option<u32>,
    strokes: Vec<Stroke>,
    since: Option<Instant>,
}

impl Pending {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// What is typed so far, for the status bar.
    pub fn display(&self) -> String {
        let count = self.count.map(|c| c.to_string()).unwrap_or_default();
        let keys: String = self.strokes.iter().map(Stroke::notation).collect();
        format!("{count}{keys}")
    }

    /// Feeds one stroke, returning the action it completes with its count.
    pub fn feed(&mut self, map: &Keymap, stroke: Stroke) -> Option<(Action, Option<u32>)> {
        if self.strokes.is_empty() && self.feed_count(&stroke) {
            return None;
        }
        self.strokes.push(stroke);
        if map.has_longer(&self.strokes) {
            self.since = Some(Instant::now());
            return None;
        }
        let action = map.exact(&self.strokes);
        let count = self.count;
        self.clear();
        Some((action?, count))
    }

    /// Takes a digit into the count. `0` is a key of its own unless a count has started.
    fn feed_count(&mut self, stroke: &Stroke) -> bool {
        let digit = stroke.text().and_then(|c| c.to_digit(10));
        match digit {
            Some(d) if d != 0 || self.count.is_some() => {
                self.count = Some(self.count.unwrap_or(0).saturating_mul(10).saturating_add(d));
                true
            }
            _ => false,
        }
    }

    /// When a sequence that is also a binding has waited long enough, runs it. Returns how long
    /// to keep waiting otherwise.
    pub fn tick(&mut self, map: &Keymap, now: Instant) -> (Option<(Action, Option<u32>)>, Option<Duration>) {
        let Some(since) = self.since else { return (None, None) };
        let Some(action) = map.exact(&self.strokes) else { return (None, None) };
        let waited = now.saturating_duration_since(since);
        if waited >= AMBIGUOUS_WAIT {
            let count = self.count;
            self.clear();
            (Some((action, count)), None)
        } else {
            (None, Some(AMBIGUOUS_WAIT - waited))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(binds: &[(&str, Action)]) -> Keymap {
        Keymap { binds: binds.iter().map(|(k, a)| (parse_sequence(k).unwrap(), *a)).collect() }
    }

    #[test]
    fn is_matches_a_single_stroke_binding_only() {
        let m = map(&[("u", Action::Undo), ("gx", Action::Cancel)]);
        assert!(m.is(&Stroke::plain("u"), Action::Undo));
        assert!(!m.is(&Stroke::plain("u"), Action::Cancel));
        assert!(!m.is(&Stroke::plain("g"), Action::Cancel));
        assert!(!m.is(&Stroke::plain("x"), Action::Undo));
    }

    fn type_keys(p: &mut Pending, m: &Keymap, keys: &str) -> Option<(Action, Option<u32>)> {
        parse_sequence(keys).unwrap().into_iter().filter_map(|s| p.feed(m, s)).last()
    }

    #[test]
    fn parses_the_notation() {
        assert_eq!(parse_sequence("gg").unwrap().len(), 2);
        let c_d = &parse_sequence("<C-d>").unwrap()[0];
        assert!(c_d.ctrl && c_d.key == "d");
        assert_eq!(parse_sequence("<S-g>").unwrap()[0], Stroke::plain("G"));
        assert_eq!(parse_sequence("<S-$>").unwrap()[0], Stroke::plain("$"));
        assert_eq!(parse_sequence("g<S-c>").unwrap()[1], Stroke::plain("C"));
        assert_eq!(parse_sequence("<esc>").unwrap()[0], Stroke::plain("esc"));
        assert_eq!(parse_sequence("<f12>").unwrap()[0], Stroke::plain("f12"));
        let c_minus = &parse_sequence("<C-->").unwrap()[0];
        assert!(c_minus.ctrl && c_minus.key == "-");
        let s_space = &parse_sequence("<S-space>").unwrap()[0];
        assert!(s_space.shift && s_space.key == "space");
    }

    #[test]
    fn a_lone_angle_bracket_is_a_key_and_a_bad_name_is_an_error() {
        assert_eq!(parse_sequence("<").unwrap()[0], Stroke::plain("<"));
        assert!(parse_sequence("<nonsense>").is_err());
        assert!(parse_sequence("").is_err());
    }

    fn default_keymap() -> Keymap {
        let (map, errors) = Keymap::new(&crate::config::Config::load_str("").unwrap().keys);
        assert_eq!(errors, Vec::<String>::new());
        map
    }

    fn stroke(text: &str) -> Stroke {
        parse_sequence(text).unwrap().remove(0)
    }

    #[test]
    fn gi_toggles_fields_gl_toggles_links_and_i_hints_a_form_at_once() {
        let map = default_keymap();
        assert_eq!(map.exact(&[stroke("g"), stroke("i")]), Some(Action::ToggleFields));
        assert_eq!(map.exact(&[stroke("g"), stroke("l")]), Some(Action::ToggleLinks));
        assert_eq!(Action::from_name("toggle_fields"), Some(Action::ToggleFields));
        let mut p = Pending::default();
        assert_eq!(p.feed(&map, stroke("i")), Some((Action::HintForm, None)));
        assert_eq!(type_keys(&mut p, &map, "gi"), Some((Action::ToggleFields, None)));
        assert_eq!(type_keys(&mut p, &map, "gl"), Some((Action::ToggleLinks, None)));
    }

    #[test]
    fn p_and_e_prefixes_protect_and_edit_portal() {
        let map = default_keymap();
        assert_eq!(map.exact(&[stroke("P"), stroke("p")]), Some(Action::Protect));
        assert_eq!(map.exact(&[stroke("P"), stroke("u")]), Some(Action::Unprotect));
        assert_eq!(map.exact(&[stroke("e"), stroke("p")]), Some(Action::EditPortal));
        assert_eq!(map.exact(&[stroke("P")]), None);
        assert_eq!(map.exact(&[stroke("e")]), None);
        let mut p = Pending::default();
        assert_eq!(p.feed(&map, stroke("P")), None);
        assert_eq!(p.tick(&map, Instant::now() + Duration::from_secs(5)), (None, None));
        assert_eq!(type_keys(&mut p, &map, "u"), Some((Action::Unprotect, None)));
        assert_eq!(p.feed(&map, stroke("e")), None);
        assert_eq!(p.tick(&map, Instant::now() + Duration::from_secs(5)), (None, None));
        assert_eq!(type_keys(&mut p, &map, "p"), Some((Action::EditPortal, None)));
        assert_eq!(type_keys(&mut p, &map, "Pp"), Some((Action::Protect, None)));
        // An unbound second key ends the sequence without an action.
        assert_eq!(type_keys(&mut p, &map, "Pz"), None);
        assert_eq!(p.display(), "");
    }

    #[test]
    fn no_other_default_binding_starts_with_p_or_e() {
        let map = default_keymap();
        let mut found: Vec<(String, Action)> = map
            .binds
            .iter()
            .filter(|(seq, _)| seq[0] == stroke("P") || seq[0] == stroke("e"))
            .map(|(seq, a)| (seq.iter().map(Stroke::notation).collect::<String>(), *a))
            .collect();
        found.sort();
        assert_eq!(found, vec![("Pp".to_owned(), Action::Protect), ("Pu".to_owned(), Action::Unprotect), ("ep".to_owned(), Action::EditPortal)]);
        assert_eq!(Action::from_name("protect"), Some(Action::Protect));
        assert_eq!(Action::from_name("unprotect"), Some(Action::Unprotect));
    }

    #[test]
    fn gb_picks_a_bookmark_and_capital_m_names_one_without_clashing() {
        let map = default_keymap();
        assert_eq!(map.exact(&[stroke("g"), stroke("b")]), Some(Action::ShowBookmarks));
        assert_eq!(map.exact(&[stroke("M")]), Some(Action::AddBookmark));
        let on = |a: Action| map.binds.iter().filter(|(_, x)| *x == a).count();
        assert_eq!((on(Action::ShowBookmarks), on(Action::AddBookmark)), (1, 1));
        let starts_with_g = map.binds.iter().filter(|(seq, _)| seq[0] == stroke("g")).filter(|(seq, _)| seq.get(1) == Some(&stroke("b"))).count();
        assert_eq!(starts_with_g, 1);
        assert_eq!(map.binds.iter().filter(|(seq, _)| seq[0] == stroke("M")).count(), 1);
    }

    #[test]
    fn an_old_copied_keys_table_still_gets_the_bookmark_keys_and_pending_feed_resolves_them() {
        use crate::config::Config;
        let defaults = Config::load_str("").unwrap();
        let mut old = String::from("[keys]\n");
        for (action, keys) in &defaults.keys {
            if matches!(action, Action::ShowBookmarks | Action::AddBookmark) {
                continue;
            }
            let list: Vec<String> = keys.iter().map(|k| format!("{k:?}")).collect();
            old.push_str(&format!("{} = [{}]\n", action.name(), list.join(", ")));
        }
        assert!(!old.contains("bookmark"));
        let config = Config::load_str(&old).unwrap();
        assert_eq!(config.keys[&Action::ShowBookmarks], ["gb"]);
        assert_eq!(config.keys[&Action::AddBookmark], ["M"]);
        let (map, errors) = Keymap::new(&config.keys);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(map.exact(&[stroke("g"), stroke("b")]), Some(Action::ShowBookmarks));
        assert_eq!(map.exact(&[stroke("M")]), Some(Action::AddBookmark));
        let mut p = Pending::default();
        assert_eq!(type_keys(&mut p, &map, "gb"), Some((Action::ShowBookmarks, None)));
        assert_eq!(type_keys(&mut p, &map, "M"), Some((Action::AddBookmark, None)));
        // A user override of one of them keeps the other's default.
        let config = Config::load_str("[keys]\nshow_bookmarks = [\"B\"]").unwrap();
        assert_eq!(config.keys[&Action::ShowBookmarks], ["B"]);
        assert_eq!(config.keys[&Action::AddBookmark], ["M"]);
        // Names are in the table and listed.
        assert_eq!(Action::from_name("show_bookmarks"), Some(Action::ShowBookmarks));
        assert_eq!(Action::from_name("add_bookmark"), Some(Action::AddBookmark));
    }

    #[test]
    fn the_editing_keys_edit_a_field() {
        let map = default_keymap();
        assert_eq!(map.edit(&stroke("<C-bs>")), Some(Edit::EraseWord));
        assert_eq!(map.edit(&stroke("<C-left>")), Some(Edit::WordLeft));
        assert_eq!(map.edit(&stroke("<C-right>")), Some(Edit::WordRight));
        assert_eq!(map.edit(&stroke("<C-del>")), Some(Edit::DeleteWord));
        assert_eq!(map.edit(&stroke("<left>")), Some(Edit::Left));
        assert_eq!(map.edit(&stroke("<bs>")), Some(Edit::Backspace));
        assert_eq!(map.edit(&stroke("<C-z>")), Some(Edit::Undo));
        assert_eq!(map.exact(&[stroke("<C-z>")]), None, "editing alone");
        assert_eq!(Action::from_name("edit_undo"), Some(Action::EditUndo));
    }

    #[test]
    fn the_editing_keys_keep_their_meaning_in_normal_mode() {
        let map = default_keymap();
        assert_eq!(map.exact(&[stroke("<left>")]), Some(Action::ScrollLeft));
        assert_eq!(map.exact(&[stroke("<C-u>")]), Some(Action::HalfUp));
        assert_eq!(map.exact(&[stroke("<C-w>")]), Some(Action::Quit));
        assert_eq!(map.exact(&[stroke("<home>")]), Some(Action::GotoFirst));
        assert_eq!(map.exact(&[stroke("<C-bs>")]), None, "editing alone");
    }

    #[test]
    fn a_printable_key_bound_to_an_edit_still_types() {
        let table = BTreeMap::from([(Action::EditLeft, vec!["x".to_owned()])]);
        let (map, _) = Keymap::new(&table);
        assert_eq!(map.edit(&Stroke::plain("x")), None);
    }

    #[test]
    fn every_default_binding_parses() {
        let config = crate::config::Config::load_str("").unwrap();
        let (_, errors) = Keymap::new(&config.keys);
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn cancel_is_escape_and_control_c_by_default_and_can_be_rebound() {
        let config = crate::config::Config::load_str("").unwrap();
        let (map, _) = Keymap::new(&config.keys);
        let ctrl_c = parse_sequence("<C-c>").unwrap().remove(0);
        assert!(map.cancels(&Stroke::plain("esc")) && map.cancels(&ctrl_c));
        assert!(!map.cancels(&Stroke::plain("c")));
        let config = crate::config::Config::load_str("[keys]\ncancel = [\"<C-g>\", \"x\"]").unwrap();
        let (map, _) = Keymap::new(&config.keys);
        assert!(!map.cancels(&ctrl_c) && !map.cancels(&Stroke::plain("esc")));
        assert!(map.cancels(&parse_sequence("<C-g>").unwrap()[0]));
        assert!(!map.cancels(&Stroke::plain("x")), "a printable key must stay typable in a bar");
    }

    #[test]
    fn redo_has_its_own_key() {
        let config = crate::config::Config::load_str("").unwrap();
        let (map, _) = Keymap::new(&config.keys);
        assert_eq!(map.exact(&parse_sequence("U").unwrap()), Some(Action::Redo));
    }

    #[test]
    fn counts_prefix_actions() {
        let m = map(&[("j", Action::ScrollDown), ("gg", Action::GotoFirst), ("0", Action::EdgeLeft)]);
        let mut p = Pending::default();
        assert_eq!(type_keys(&mut p, &m, "12j"), Some((Action::ScrollDown, Some(12))));
        assert_eq!(type_keys(&mut p, &m, "10gg"), Some((Action::GotoFirst, Some(10))));
        assert_eq!(type_keys(&mut p, &m, "0"), Some((Action::EdgeLeft, None)));
    }

    #[test]
    fn an_unknown_sequence_is_dropped_with_its_count() {
        let m = map(&[("gg", Action::GotoFirst), ("j", Action::ScrollDown)]);
        let mut p = Pending::default();
        assert_eq!(type_keys(&mut p, &m, "5gx"), None);
        assert_eq!(p.display(), "");
        assert_eq!(type_keys(&mut p, &m, "j"), Some((Action::ScrollDown, None)));
    }

    #[test]
    fn an_ambiguous_binding_waits_then_fires() {
        let m = map(&[("d", Action::Undo), ("dh", Action::DeleteAnnotation)]);
        let mut p = Pending::default();
        assert_eq!(type_keys(&mut p, &m, "d"), None);
        let (fired, wait) = p.tick(&m, Instant::now());
        assert!(fired.is_none() && wait.is_some());
        let (fired, _) = p.tick(&m, Instant::now() + AMBIGUOUS_WAIT);
        assert_eq!(fired, Some((Action::Undo, None)));
        assert_eq!(type_keys(&mut p, &m, "dh"), Some((Action::DeleteAnnotation, None)));
    }

    #[test]
    fn chords_come_from_key_events_and_plain_letters_do_not() {
        let ctrl = egui::Modifiers { ctrl: true, ..Default::default() };
        let stroke = Stroke::from_key(egui::Key::O, ctrl).unwrap();
        assert!(stroke.is_ctrl("o"));
        assert!(Stroke::from_key(egui::Key::O, egui::Modifiers::NONE).is_none());
        assert_eq!(Stroke::from_key(egui::Key::Escape, egui::Modifiers::NONE), Some(Stroke::plain("esc")));
        // Ctrl-i is its own key, not tab.
        assert_ne!(Stroke::from_key(egui::Key::I, ctrl).unwrap().key, "tab");
    }

    #[test]
    fn every_action_name_reads_back_as_the_same_action() {
        use serde::de::IntoDeserializer;
        for action in Action::ALL {
            let read = Action::deserialize(action.name().into_deserializer());
            assert_eq!(read, Ok::<_, serde::de::value::Error>(action), "{}", action.name());
            assert_eq!(Action::from_name(action.name()), Some(action));
        }
        assert_eq!(Action::from_name("no_such_action"), None);
    }

    #[test]
    fn the_list_of_actions_has_no_duplicates() {
        let mut names: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Action::ALL.len());
    }

    /// Every action has a default binding, so a variant missing from `ALL` is missing here too.
    /// (The match in `name` does not compile without every variant, which names them all.)
    #[test]
    fn the_list_of_actions_is_complete() {
        let config = crate::config::Config::load_str("").unwrap();
        let bound: Vec<Action> = config.keys.keys().copied().collect();
        let mut listed = Action::ALL.to_vec();
        listed.sort_unstable();
        assert_eq!(bound, listed);
    }

    /// `ALL` is the enum in declaration order, whether or not a variant has a default binding: each
    /// sits at its own discriminant, and there are as many as the enum declares.
    #[test]
    fn the_list_of_actions_matches_the_enum_variant_for_variant() {
        for (at, action) in Action::ALL.iter().enumerate() {
            assert_eq!(*action as usize, at, "{} is out of place in Action::ALL", action.name());
        }
        // The last variant's discriminant is the number of variants less one, and `name` has no
        // catch-all, so the compiler makes sure that the variant is named.
        assert_eq!(Action::EditUndo as usize + 1, Action::ALL.len());
        let source = include_str!("keymap.rs");
        let body = source.split_once("pub enum Action {").and_then(|(_, rest)| rest.split_once("\n}")).map_or("", |(body, _)| body);
        let declared = body.lines().map(str::trim).filter(|l| l.ends_with(',') && !l.starts_with(['#', '/'])).count();
        assert_eq!(declared, Action::ALL.len(), "a variant of Action is missing from Action::ALL, or the other way round");
    }

    #[test]
    fn only_some_actions_can_be_run_by_name() {
        assert!(Action::ZoomIn.is_listed() && Action::ToggleToc.is_listed() && Action::Quit.is_listed());
        for action in [Action::Command, Action::Cancel, Action::EditLeft, Action::EditEraseToEnd] {
            assert!(!action.is_listed(), "{}", action.name());
        }
        assert_eq!(Action::ALL.iter().filter(|a| !a.is_listed()).count(), 2 + 14);
    }

    #[test]
    fn edit_undo_is_ctrl_z_and_can_be_rebound_or_removed() {
        let ctrl_z = stroke("<C-z>");
        let config = crate::config::Config::load_str("").unwrap();
        assert_eq!(config.keys.get(&Action::EditUndo), Some(&vec!["<C-z>".to_owned()]));
        assert!(config.keys.iter().filter(|(a, _)| **a != Action::EditUndo).all(|(_, keys)| !keys.iter().any(|k| k == "<C-z>")));
        let config = crate::config::Config::load_str("[keys]\nedit_undo = [\"<C-y>\"]").unwrap();
        let (map, errors) = Keymap::new(&config.keys);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(map.edit(&ctrl_z), None);
        assert_eq!(map.edit(&stroke("<C-y>")), Some(Edit::Undo));
        let config = crate::config::Config::load_str("[keys]\nedit_undo = []").unwrap();
        let (map, errors) = Keymap::new(&config.keys);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(map.edit(&ctrl_z), None);
        // Bound to a normal-mode action by the user, it still edits in a field and acts in the document.
        let config = crate::config::Config::load_str("[keys]\nquit = [\"<C-z>\"]").unwrap();
        let (map, _) = Keymap::new(&config.keys);
        assert_eq!(map.exact(std::slice::from_ref(&ctrl_z)), Some(Action::Quit));
        assert_eq!(map.edit(&ctrl_z), Some(Edit::Undo));
    }

    fn typed(table: &str, actions: &[Action]) -> Vec<char> {
        let config = crate::config::Config::load_str(table).unwrap();
        let (map, errors) = Keymap::new(&config.keys);
        assert!(errors.is_empty(), "{errors:?}");
        let mut chars = map.typed_by(actions);
        chars.sort_unstable();
        chars
    }

    #[test]
    fn typed_by_takes_single_printable_strokes_only() {
        let all = [Action::Quit, Action::Command, Action::Open, Action::ToggleFullscreen];
        // Sequences, chords and named keys type nothing.
        assert_eq!(typed("[keys]\nquit = [\"ZZ\", \"<C-q>\", \"<A-q>\", \"<esc>\", \"<F4>\"]", &[Action::Quit]), Vec::<char>::new());
        // A single letter, a capital and a symbol each type themselves.
        assert_eq!(typed("[keys]\nquit = [\"x\", \"Q\", \"ZZ\", \"<C-q>\"]", &[Action::Quit]), ['Q', 'x']);
        // Only the asked-for actions count.
        assert_eq!(typed("[keys]\nquit = [\"x\"]\nopen = [\"y\"]", &[Action::Quit]), ['x']);
        assert_eq!(typed("[keys]\nquit = [\"x\"]\nopen = [\"y\"]", &[]), Vec::<char>::new());
        let defaults = typed("", &all);
        assert!(defaults.contains(&'q') && defaults.contains(&':'), "{defaults:?}");
        assert!(!defaults.contains(&'j') && !defaults.contains(&'k'), "{defaults:?}");
        // A removed binding leaves nothing.
        assert_eq!(typed("[keys]\nquit = []\ncommand = []\nopen = []\ntoggle_fullscreen = []", &all), Vec::<char>::new());
        assert_eq!(typed("[keys]\nquit = [\"<space>\"]", &[Action::Quit]), [' ']);
    }

    #[test]
    fn ctrl_z_is_not_a_normal_mode_action_by_default() {
        let map = default_keymap();
        let ctrl_z = stroke("<C-z>");
        assert_eq!(map.exact(std::slice::from_ref(&ctrl_z)), None);
        assert!(map.is(&stroke("<C-z>"), Action::EditUndo));
    }

    #[test]
    fn every_action_name_round_trips_and_is_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for action in Action::ALL {
            assert_eq!(Action::from_name(action.name()), Some(action));
            assert!(seen.insert(action.name()), "{}", action.name());
        }
        assert_eq!(seen.len(), Action::ALL.len());
    }
    #[test]
    fn the_default_rotation_keys_are_free_and_clash_with_nothing() {
        let config = crate::config::Config::load_str("").unwrap();
        assert_eq!(config.keys.get(&Action::RotateCw), Some(&vec![">".to_owned()]));
        assert_eq!(config.keys.get(&Action::RotateCcw), Some(&vec!["<".to_owned()]));
        let mut seen = std::collections::BTreeMap::new();
        // The `edit_*` actions share keys with the normal ones on purpose.
        for (action, keys) in config.keys.iter().filter(|(a, _)| a.edit().is_none()) {
            for key in keys {
                if let Some(other) = seen.insert(key.clone(), action.name()) {
                    panic!("{key} is bound to both {other} and {}", action.name());
                }
            }
        }
        let map = default_keymap();
        assert_eq!(map.exact(&[stroke(">")]), Some(Action::RotateCw));
        assert_eq!(map.exact(&[stroke("<")]), Some(Action::RotateCcw));
    }

    #[test]
    fn the_rotation_keys_can_be_written_several_ways() {
        for text in ["<", "<lt>", "<LT>", "<Lt>"] {
            assert_eq!(parse_sequence(text).unwrap(), vec![Stroke::plain("<")], "{text}");
        }
        assert_eq!(parse_sequence(">").unwrap(), vec![Stroke::plain(">")]);
        for text in [">", "<gt>", "<GT>"] {
            assert_eq!(parse_sequence(text).unwrap(), vec![Stroke::plain(">")], "{text}");
        }
        let table: BTreeMap<Action, Vec<String>> = [(Action::RotateCw, vec![">".to_owned()]), (Action::RotateCcw, vec!["<lt>".to_owned()])].into();
        let (map, errors) = Keymap::new(&table);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(map.exact(&[stroke("<")]), Some(Action::RotateCcw));
        assert_eq!(map.exact(&[stroke(">")]), Some(Action::RotateCw));
    }

    #[test]
    fn the_two_page_keys_are_free() {
        let config = crate::config::Config::load_str("").unwrap();
        assert_eq!(config.keys.get(&Action::ToggleTwoPage), Some(&vec!["D".to_owned()]));
        assert_eq!(config.keys.get(&Action::ToggleCover), Some(&vec!["gD".to_owned()]));
        let (map, errors) = Keymap::new(&config.keys);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(map.exact(&[stroke("D")]), Some(Action::ToggleTwoPage));
        assert_eq!(map.exact(&[stroke("g"), stroke("D")]), Some(Action::ToggleCover));
        let mut seen = std::collections::BTreeMap::new();
        for (action, keys) in config.keys.iter().filter(|(a, _)| a.edit().is_none()) {
            for key in keys {
                if let Some(other) = seen.insert(key.clone(), action.name()) {
                    panic!("{key} is bound to both {other} and {}", action.name());
                }
            }
        }
    }
}
