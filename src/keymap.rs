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
    Yank,
    Portal,
    GotoPortal,
    DeletePortal,
    EditPortal,
    TogglePortalWindow,
    ToggleToc,
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
    ToggleLightDark,
    ToggleLinks,
    SmartJump,
    Overview,
    GotoOverview,
    OverviewNext,
    OverviewPrev,
    ToggleFullscreen,
    Reload,
    Quit,
    Open,
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
}

impl Action {
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
    fn the_editing_keys_edit_a_field() {
        let map = default_keymap();
        assert_eq!(map.edit(&stroke("<C-bs>")), Some(Edit::EraseWord));
        assert_eq!(map.edit(&stroke("<C-left>")), Some(Edit::WordLeft));
        assert_eq!(map.edit(&stroke("<C-right>")), Some(Edit::WordRight));
        assert_eq!(map.edit(&stroke("<C-del>")), Some(Edit::DeleteWord));
        assert_eq!(map.edit(&stroke("<left>")), Some(Edit::Left));
        assert_eq!(map.edit(&stroke("<bs>")), Some(Edit::Backspace));
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
    fn control_r_toggles_the_theme_and_redo_has_its_own_key() {
        let config = crate::config::Config::load_str("").unwrap();
        let (map, _) = Keymap::new(&config.keys);
        assert_eq!(map.exact(&parse_sequence("<C-r>").unwrap()), Some(Action::ToggleLightDark));
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
}
