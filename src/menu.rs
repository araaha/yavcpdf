//! The menu under the `:` bar: what can be run, filtered by what is typed. Everything here is
//! pure, so the drawing and the key handling stay thin and this is tested without a window.
//!
//! What is highlighted is exactly what Enter uses, and nothing else. An entry is highlighted when
//! the user chose it (Up, Down, Tab, a click), or, on the first word only, when it is an exact
//! match for what was typed or the only name that continues it and is not one to run by accident.
//! With no highlight, Enter runs the text as typed.
use std::collections::BTreeMap;
use std::path::Path;

use crate::complete::{self, Completion, DirCache};
use crate::keymap::Action;

/// The most rows the menu shows at once, a footer included.
const MAX_ROWS: usize = 12;
/// The most argument candidates kept.
const MAX_ENTRIES: usize = 500;

/// What an entry is, and so what Enter does with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Action(Action),
    /// `argument` is `Some(required)` for a command that takes one.
    Command {
        name: &'static str,
        argument: Option<bool>,
    },
    /// A completion of the argument; a directory is descended into instead of run.
    Candidate {
        dir: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub label: String,
    /// What the argument looks like, such as `{file}`; empty for most.
    pub hint: String,
    /// The bound key sequences, in the notation of `[keys]`.
    pub keys: String,
    pub kind: Kind,
    /// The text of the field after Tab, or after Enter descends or asks for the argument.
    pub fill: String,
    /// Which characters of the label the query matched.
    pub matched: Vec<usize>,
}

/// What Enter does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Close the bar and run the action.
    Run(Action),
    /// Close the bar and run this command line.
    Command(String),
    /// Put this in the field and keep the bar open.
    Fill(String),
    /// Nothing is highlighted: run what was typed.
    Typed,
}

#[derive(Clone, Debug)]
pub struct Menu {
    /// The field text before the cursor that the entries were made for: the query.
    text: String,
    /// The field text that Tab put there, which keeps these entries instead of making new ones.
    filled: Option<String>,
    pub entries: Vec<Entry>,
    /// The listing of files was cut short.
    pub incomplete: bool,
    pub selected: usize,
    pub scroll: usize,
    /// `selected` is in effect: chosen, or picked for being what was typed.
    lit: bool,
    /// The user chose, with a key or a click, as against the menu picking.
    chosen: bool,
    /// The query is an argument, after the command and a space.
    argument: bool,
    /// There is text after the cursor: nothing is highlighted and Enter runs the text as typed.
    tail: bool,
    /// Wheel movement that has not yet made a whole row.
    carry: f32,
    /// The directory that the argument is in, kept while the menu is made again.
    cache: DirCache,
}

fn is_page_number(text: &str) -> bool {
    text.parse::<usize>().is_ok()
}

fn lower(text: &str) -> Vec<char> {
    text.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect()
}

/// How well a name matches: the tier, the characters used, and the order within the tier.
type Rank = (u8, Vec<usize>, (usize, usize, usize));

/// Whether the character at `at` starts the name or follows an underscore.
fn word_start(name: &[char], at: usize) -> bool {
    at == 0 || at.checked_sub(1).and_then(|before| name.get(before)) == Some(&'_')
}

/// The letters of `query` in order in `name`, preferring word starts and then the shortest span.
/// Returns the positions, how many are not at a word start, and the span.
fn best_subsequence(name: &[char], query: &[char]) -> Option<(Vec<usize>, usize, usize)> {
    let (n, k) = (name.len(), query.len());
    let last = k.checked_sub(1)?;
    let off = |at: usize| usize::from(!word_start(name, at));
    let mut best: Option<((usize, usize), Vec<usize>)> = None;
    for start in 0..n {
        if name.get(start) != query.first() {
            continue;
        }
        // The cheapest way to put query[i] at name[j], and where query[i - 1] was then.
        let mut cost: Vec<Option<(usize, usize)>> = vec![None; n.saturating_mul(k)];
        if let Some(slot) = cost.get_mut(start) {
            *slot = Some((off(start), start));
        }
        for i in 1..k {
            let mut run: Option<(usize, usize)> = None;
            for j in 0..n {
                if let Some(p) = j.checked_sub(1)
                    && let Some(Some((c, _))) = cost.get((i - 1) * n + p)
                    && run.is_none_or(|(best_cost, _)| *c < best_cost)
                {
                    run = Some((*c, p));
                }
                if name.get(j) == query.get(i)
                    && let Some((c, p)) = run
                    && let Some(slot) = cost.get_mut(i * n + j)
                {
                    *slot = Some((c.saturating_add(off(j)), p));
                }
            }
        }
        for end in start..n {
            let Some(Some((c, _))) = cost.get(last * n + end) else { continue };
            let key = (*c, end - start);
            if best.as_ref().is_none_or(|(b, _)| key < *b) {
                let mut used = vec![end];
                let mut at = end;
                for i in (1..k).rev() {
                    let Some(Some((_, p))) = cost.get(i * n + at) else { break };
                    used.push(*p);
                    at = *p;
                }
                used.reverse();
                best = Some((key, used));
            }
        }
    }
    best.map(|((off, span), used)| (used, off, span))
}

/// How well `query` matches `name` (0 exact, 1 prefix, 2 substring, 3 subsequence), and which
/// characters of the name it uses. Case does not matter. Within the subsequence tier, matches on
/// word starts come first, then the most compact, then the shorter name.
fn rank(name: &str, query: &str) -> Option<Rank> {
    let (name, query) = (lower(name), lower(query));
    let span = |start: usize| (start..start + query.len()).collect::<Vec<usize>>();
    if name == query {
        return Some((0, span(0), (0, 0, 0)));
    }
    if name.starts_with(&query) {
        return Some((1, span(0), (0, 0, 0)));
    }
    if let Some(start) = name.windows(query.len().max(1)).position(|w| w == query.as_slice()) {
        return Some((2, span(start), (0, 0, 0)));
    }
    let mut at = 0;
    let mut used = Vec::new();
    for wanted in &query {
        let found = name.get(at..)?.iter().position(|c| c == wanted)?;
        used.push(at + found);
        at += found + 1;
    }
    Some(match best_subsequence(&name, &query) {
        Some((better, off, span)) => (3, better, (off, span, name.len())),
        None => (3, used, (usize::MAX, usize::MAX, name.len())),
    })
}

fn keys_of(action: Action, table: &BTreeMap<Action, Vec<String>>) -> String {
    table.get(&action).map_or_else(String::new, |keys| keys.join("  "))
}

/// The commands and actions that the first word of the line picks out, best first.
fn names(lead: &str, query: &str, table: &BTreeMap<Action, Vec<String>>) -> Vec<Entry> {
    if is_page_number(query) {
        return Vec::new();
    }
    let mut found: Vec<(Rank, Entry)> = Vec::new();
    let mut add = |ranked: Option<Rank>, mut entry: Entry| {
        if let Some(rank) = ranked {
            entry.matched.clone_from(&rank.1);
            found.push((rank, entry));
        }
    };
    for command in complete::COMMANDS {
        let name = command.name;
        let entry = Entry {
            label: name.to_owned(),
            hint: command.argument.map_or_else(String::new, |a| a.hint.to_owned()),
            // A command that is also an action name (`quit`) shows that action's keys.
            keys: Action::from_name(name).map_or_else(String::new, |a| keys_of(a, table)),
            kind: Kind::Command { name, argument: command.argument.map(|a| a.required) },
            fill: format!("{lead}{name}{}", if command.argument.is_some() { " " } else { "" }),
            matched: Vec::new(),
        };
        add(rank(name, query), entry);
    }
    for action in Action::ALL.into_iter().filter(|a| a.is_listed()) {
        let name = action.name();
        if complete::COMMANDS.iter().any(|command| command.name == name) {
            continue;
        }
        let entry = Entry {
            label: name.to_owned(),
            hint: String::new(),
            keys: keys_of(action, table),
            kind: Kind::Action(action),
            fill: format!("{lead}{name}"),
            matched: Vec::new(),
        };
        add(rank(name, query), entry);
    }
    found.sort_by(|((a_tier, _, a_key), x), ((b_tier, _, b_key), y)| {
        a_tier
            .cmp(b_tier)
            .then_with(|| a_key.cmp(b_key))
            .then_with(|| x.label.to_lowercase().cmp(&y.label.to_lowercase()))
            .then_with(|| x.label.cmp(&y.label))
    });
    found.into_iter().map(|(_, entry)| entry).collect()
}

/// Names that are not picked for the user by a part of the name: they end the program, or write
/// or delete something that may not be undone.
fn is_irreversible(name: &str) -> bool {
    matches!(name, "q" | "quit" | "q!" | "wq" | "x" | "export" | "export!") || name.starts_with("delete_")
}

/// The entry to highlight for the query, when the user has not chosen: an exact match (the case
/// as typed), or the only entry that the query begins and that is safe to run unasked.
fn pick(entries: &[Entry], query: &str) -> Option<usize> {
    if query.is_empty() {
        return None;
    }
    if entries.first().is_some_and(|e| e.label == query) {
        return Some(0);
    }
    let mut begun = entries.iter().enumerate().filter(|(_, e)| e.label.starts_with(query));
    let (at, only) = begun.next()?;
    (begun.next().is_none() && !is_irreversible(&only.label)).then_some(at)
}

/// The entries for the candidates of an argument: an exact match for the word first, then at most
/// `limit`, and whether the listing is incomplete, as cut short by the directory or by `limit`.
fn argument_entries(text: &str, completion: Completion, limit: usize) -> (Vec<Entry>, bool) {
    let Completion { start, mut candidates, incomplete } = completion;
    let (kept, word) = (text.get(..start).unwrap_or(""), text.get(start..).unwrap_or(""));
    if let Some(at) = candidates.iter().position(|c| c == word) {
        let exact = candidates.remove(at);
        candidates.insert(0, exact);
    }
    let cut = candidates.len() > limit;
    candidates.truncate(limit);
    let entries = candidates
        .iter()
        .map(|candidate| Entry {
            label: complete::label(candidate),
            hint: String::new(),
            keys: String::new(),
            kind: Kind::Candidate { dir: candidate.ends_with('/') },
            fill: format!("{kept}{candidate}"),
            matched: Vec::new(),
        })
        .collect();
    (entries, incomplete || cut)
}

impl Menu {
    /// The menu for the text before the cursor in the `:` bar.
    #[cfg(test)]
    pub fn build(before: &str, table: &BTreeMap<Action, Vec<String>>, themes: &[String], home: &Path) -> Self {
        Self::with_cache(before, table, themes, home, DirCache::default())
    }

    /// The same, keeping the directory listing that the last menu read.
    pub fn with_cache(before: &str, table: &BTreeMap<Action, Vec<String>>, themes: &[String], home: &Path, mut cache: DirCache) -> Self {
        let rest = before.trim_start();
        let lead = before.get(..before.len() - rest.len()).unwrap_or("");
        let argument = rest.contains(char::is_whitespace);
        let (entries, incomplete) = if argument {
            let found = complete::complete(before, themes, home, &mut cache);
            found.map_or((Vec::new(), false), |completion| argument_entries(before, completion, MAX_ENTRIES))
        } else {
            (names(lead, rest, table), false)
        };
        let picked = if argument { None } else { pick(&entries, rest) };
        Self {
            text: before.to_owned(),
            filled: None,
            entries,
            incomplete,
            selected: picked.unwrap_or(0),
            scroll: 0,
            lit: picked.is_some(),
            chosen: false,
            argument,
            tail: false,
            carry: 0.0,
            cache,
        }
    }

    /// Hands over the directory listing, for the menu that replaces this one.
    pub fn take_cache(&mut self) -> DirCache {
        std::mem::take(&mut self.cache)
    }

    /// Whether these entries are for this text before the cursor: the query, or what Tab filled.
    pub fn is_for(&self, before: &str) -> bool {
        self.text == before || self.filled.as_deref() == Some(before)
    }

    /// Whether anything is drawn.
    pub fn is_shown(&self) -> bool {
        !self.entries.is_empty() || self.incomplete
    }

    /// Tells the menu whether there is text after the cursor. With some, the picked entry is
    /// dropped (a chosen one stays, for Tab to step on from) and nothing is highlighted.
    pub fn set_tail(&mut self, tail: bool) {
        self.tail = tail;
        if tail && !self.chosen {
            self.lit = false;
        }
    }

    /// The entry that is in effect for the keys, whether or not it is drawn.
    fn current(&self) -> Option<usize> {
        (self.lit && self.selected < self.entries.len()).then_some(self.selected)
    }

    /// The entry that is highlighted, and so the one Enter uses.
    pub fn highlighted(&self) -> Option<usize> {
        self.current().filter(|_| !self.tail)
    }

    /// The rows of the list and whether there is a footer, given the rows the view has room for.
    pub fn layout(&self, room: usize) -> (usize, bool) {
        let list = self.entries.len().min(MAX_ROWS.min(room).saturating_sub(usize::from(self.incomplete)));
        (if list == 0 && !self.entries.is_empty() { 1 } else { list }, self.incomplete)
    }

    /// Which entry is highlighted and how many there are, when they do not all fit.
    pub fn count_label(&self, room: usize) -> Option<String> {
        let total = self.entries.len();
        (total > self.layout(room).0).then(|| self.highlighted().map_or_else(|| total.to_string(), |at| format!("{}/{total}", at + 1)))
    }

    /// `room` is the rows the view has room for, as for `layout`.
    fn keep_visible(&mut self, room: usize) {
        let rows = self.layout(room).0.max(1);
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll.saturating_add(rows) {
            self.scroll = (self.selected + 1).saturating_sub(rows);
        }
        self.scroll = self.scroll.min(self.entries.len().saturating_sub(rows));
    }

    /// Chooses an entry and scrolls to keep it in sight.
    fn choose(&mut self, index: usize, room: usize) {
        self.selected = index;
        self.lit = true;
        self.chosen = true;
        self.keep_visible(room);
    }

    /// Moves the highlight, wrapping at both ends, and chooses where it lands. Returns the text
    /// for the field before the cursor, as Tab does. With nothing
    /// highlighted, forward goes to the first and back to the last.
    pub fn move_by(&mut self, delta: isize, room: usize) -> Option<String> {
        let len = isize::try_from(self.entries.len()).unwrap_or(isize::MAX);
        if len == 0 {
            return None;
        }
        let next = match self.current() {
            Some(at) => {
                let at = isize::try_from(at).unwrap_or(0);
                (at.saturating_add(delta)).rem_euclid(len)
            }
            None if delta < 0 => len - 1,
            None => 0,
        };
        self.choose(usize::try_from(next).unwrap_or(0), room);
        self.fill_chosen()
    }

    /// Moves the highlight by half the rows shown (at least one), stopping at the ends. With
    /// nothing highlighted, down lands on the row `step - 1` and up on the first. Returns the text
    /// for the field before the cursor, as `move_by` does.
    pub fn move_page(&mut self, down: bool, room: usize) -> Option<String> {
        let last = self.entries.len().checked_sub(1)?;
        let step = (self.layout(room).0 / 2).max(1);
        let next = match (self.current(), down) {
            (Some(at), true) => at.saturating_add(step),
            (Some(at), false) => at.saturating_sub(step),
            (None, true) => step - 1,
            (None, false) => 0,
        };
        self.choose(next.min(last), room);
        self.fill_chosen()
    }

    /// Chooses an entry, if there is one at `index`. Returns the text for the field before the
    /// cursor, as Tab does.
    pub fn select(&mut self, index: usize, room: usize) -> Option<String> {
        if index < self.entries.len() {
            self.choose(index, room);
            return self.fill_chosen();
        }
        None
    }

    /// The chosen entry's fill, which the field then shows while the menu keeps its entries.
    fn fill_chosen(&mut self) -> Option<String> {
        let fill = self.entries.get(self.selected)?.fill.clone();
        self.filled = Some(fill.clone());
        Some(fill)
    }

    /// One Tab (or Shift-Tab): returns the text for the field before the cursor, if it changes.
    /// In an argument with nothing chosen, the first Tab adds what all the candidates share, if
    /// that is more than was typed. Otherwise Tab steps the choice, and the field shows it while
    /// the menu keeps the entries of what was typed.
    pub fn tab(&mut self, backward: bool, room: usize) -> Option<String> {
        let len = self.entries.len();
        if len == 0 {
            return None;
        }
        // The only candidate being a folder, the field goes into it, chosen or not: the menu is left
        // not for the new text, so it is made again and lists what the folder holds. A cut-short
        // listing may hide other matches, so there Tab only fills it.
        if len == 1
            && !self.incomplete
            && let Some(entry) = self.entries.first().filter(|e| matches!(e.kind, Kind::Candidate { dir: true }))
        {
            let fill = entry.fill.clone();
            self.choose(0, room);
            self.filled = None;
            return Some(fill);
        }
        if !self.chosen {
            // With a cut-short listing the shared start of what was seen may be too long.
            if self.argument && len > 1 && !self.incomplete {
                let fills: Vec<String> = self.entries.iter().map(|e| e.fill.clone()).collect();
                let shared = complete::common_prefix(&fills);
                if shared.len() > self.text.len() && shared.starts_with(&self.text) {
                    return Some(shared);
                }
            }
            if len == 1 {
                self.choose(0, room);
            } else if let Some(at) = self.current() {
                // Picked for what was typed: the first Tab takes it.
                self.choose(at, room);
            } else {
                self.choose(if backward { len - 1 } else { 0 }, room);
            }
        } else {
            let at = self.current().unwrap_or(0);
            self.choose(if backward { (at + len - 1) % len } else { (at + 1) % len }, room);
        }
        self.fill_chosen()
    }

    /// Scrolls the list by the wheel, without moving the highlight.
    pub fn scroll_pixels(&mut self, pixels: f32, row_height: f32, room: usize) {
        if !pixels.is_finite() || !row_height.is_finite() || row_height <= 0.0 {
            return;
        }
        // The ratio overflows to infinity for a tiny row; it never means more than the whole list.
        let bound = self.entries.len() as f32;
        self.carry += (pixels / row_height).clamp(-bound, bound);
        if !self.carry.is_finite() {
            self.carry = 0.0;
        }
        let whole = self.carry.trunc();
        self.carry -= whole;
        let max = self.entries.len().saturating_sub(self.layout(room).0.max(1));
        let moved = (self.scroll as f32 + whole).clamp(0.0, max as f32);
        self.scroll = moved as usize;
    }

    /// What Enter does now: what is highlighted, or else the text as typed.
    pub fn enter(&self) -> Outcome {
        if self.tail {
            return Outcome::Typed;
        }
        let Some(entry) = self.highlighted().and_then(|at| self.entries.get(at)) else { return Outcome::Typed };
        match &entry.kind {
            Kind::Action(action) => Outcome::Run(*action),
            Kind::Command { argument: Some(true), .. } | Kind::Candidate { dir: true } => Outcome::Fill(entry.fill.clone()),
            Kind::Command { name, .. } => Outcome::Command((*name).to_owned()),
            Kind::Candidate { dir: false } => Outcome::Command(entry.fill.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::complete::Completion;
    use crate::config::Config;

    fn keys() -> BTreeMap<Action, Vec<String>> {
        Config::load_str("").unwrap().keys
    }

    fn themes() -> Vec<String> {
        ["dark", "light"].map(str::to_owned).to_vec()
    }

    fn menu(text: &str) -> Menu {
        Menu::build(text, &keys(), &themes(), Path::new("/nonexistent-home"))
    }

    fn labels(menu: &Menu) -> Vec<&str> {
        menu.entries.iter().map(|e| e.label.as_str()).collect()
    }

    fn position(menu: &Menu, label: &str) -> usize {
        menu.entries.iter().position(|e| e.label == label).unwrap()
    }

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("yavcpdf-menu-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("sub")).unwrap();
            std::fs::write(root.join("a.pdf"), b"").unwrap();
            Self(root)
        }

        fn path(&self) -> &str {
            self.0.to_str().unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn matches_are_ranked_exact_then_prefix_then_substring_then_subsequence() {
        let m = menu("zoom");
        let order = ["zoom", "zoom_in", "zoom_out"];
        let at: Vec<usize> = order.iter().map(|l| position(&m, l)).collect();
        assert_eq!(at, [0, 1, 2]);
        let (prefix, substring, subsequence) = (rank("toggle_toc", "tog"), rank("next_page", "page"), rank("toggle_toc", "ttc"));
        assert_eq!(prefix.map(|r| r.0), Some(1));
        assert_eq!(substring.map(|r| r.0), Some(2));
        assert_eq!(subsequence.map(|r| r.0), Some(3));
        assert_eq!(rank("zoom", "zoom").map(|r| r.0), Some(0));
        assert_eq!(rank("zoom", "zx"), None);
        let m = menu("tt");
        let tiers: Vec<u8> = m.entries.iter().filter_map(|e| rank(&e.label, "tt").map(|r| r.0)).collect();
        assert!(tiers.windows(2).all(|w| w[0] <= w[1]), "{tiers:?}");
    }

    #[test]
    fn ties_are_alphabetical() {
        let m = menu("tool_");
        let names: Vec<&str> = labels(&m).into_iter().filter(|l| l.starts_with("tool_")).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        assert!(names.len() >= 5);
    }

    #[test]
    fn matching_ignores_case_and_records_the_characters() {
        assert_eq!(labels(&menu("ZOOM"))[..3], ["zoom", "zoom_in", "zoom_out"]);
        let m = menu("tgl");
        let entry = m.entries.iter().find(|e| e.label == "toggle_links").unwrap();
        assert_eq!(entry.matched, [0, 2, 7], "the l of links starts a word");
    }

    #[test]
    fn the_empty_query_lists_everything() {
        let m = menu("");
        let listed = Action::ALL.iter().filter(|a| a.is_listed()).count();
        let clashes = Action::ALL.iter().filter(|a| a.is_listed() && complete::COMMANDS.iter().any(|c| c.name == a.name())).count();
        assert_eq!(m.entries.len(), complete::COMMANDS.len() + listed - clashes);
        assert!(m.entries.iter().any(|e| e.label == "toc") && m.entries.iter().any(|e| e.label == "zoom_in"));
        assert_eq!(menu("  ").entries.len(), m.entries.len());
    }

    #[test]
    fn a_page_number_lists_nothing() {
        assert!(menu("12").entries.is_empty());
        assert!(!menu("12").is_shown());
        assert_eq!(menu("12").enter(), Outcome::Typed);
    }

    #[test]
    fn excluded_actions_are_absent_and_the_rest_are_there() {
        let m = menu("");
        for action in Action::ALL {
            let present = m.entries.iter().any(|e| e.label == action.name());
            let clash = complete::COMMANDS.iter().any(|c| c.name == action.name());
            assert_eq!(present, action.is_listed() || clash, "{}", action.name());
        }
        for gone in ["command", "cancel", "edit_left", "edit_erase_to_end"] {
            assert!(m.entries.iter().all(|e| e.label != gone), "{gone}");
        }
    }

    #[test]
    fn keys_are_shown_from_the_table() {
        let m = menu("");
        let keys_of = |label: &str| m.entries.iter().find(|e| e.label == label).unwrap().keys.clone();
        assert_eq!(keys_of("scroll_down"), "j  <down>");
        assert_eq!(keys_of("quit"), "q  ZQ  ZZ  <C-w>", "a command with an action's name shows its keys");
        assert_eq!(keys_of("toc"), "");
        let mut table = BTreeMap::new();
        table.insert(Action::ZoomIn, vec!["<C-i>".to_owned()]);
        let m = Menu::build("zoom_in", &table, &[], Path::new("/nonexistent-home"));
        assert_eq!(m.entries.first().map(|e| e.keys.as_str()), Some("<C-i>"));
        let m = Menu::build("zoom_out", &table, &[], Path::new("/nonexistent-home"));
        assert_eq!(m.entries.first().map(|e| e.keys.as_str()), Some(""));
    }

    #[test]
    fn commands_with_arguments_show_a_hint() {
        let m = menu("");
        for (label, hint) in
            [("e", "{file}"), ("open", "[file]"), ("theme", "{name}"), ("zoom", "{percent}"), ("export", "[file]"), ("export!", "[file]")]
        {
            assert_eq!(m.entries[position(&m, label)].hint, hint);
        }
        assert_eq!(m.entries[position(&m, "w")].hint, "");
    }

    #[test]
    fn only_the_first_word_picks_the_names() {
        assert_eq!(labels(&menu("zoom_in")).first(), Some(&"zoom_in"));
        assert_eq!(labels(&menu("  zoo")).first(), Some(&"zoom"));
        assert_eq!(menu("  zoo").entries[0].fill, "  zoom ");
    }

    #[test]
    fn arguments_list_candidates_instead() {
        let m = menu("theme ");
        assert_eq!(labels(&m), ["dark", "light"]);
        assert_eq!(m.entries[0].fill, "theme dark");
        assert_eq!(labels(&menu("theme l")), ["light"]);
        assert!(menu("zoom 15").entries.is_empty());
        assert!(menu("nonsense x").entries.is_empty());
    }

    #[test]
    fn file_candidates_come_with_directories_marked() {
        let dir = Scratch::new("files");
        let m = menu(&format!("e {}/", dir.path()));
        assert_eq!(labels(&m), ["a.pdf", "sub/"]);
        assert_eq!(m.entries[1].kind, Kind::Candidate { dir: true });
        assert_eq!(m.enter(), Outcome::Typed, "an argument is never picked for the user");
        let mut m = m;
        m.move_by(1, 5);
        assert_eq!(m.enter(), Outcome::Command(format!("e {}/a.pdf", dir.path())));
        m.move_by(1, 5);
        assert_eq!(m.enter(), Outcome::Fill(format!("e {}/sub/", dir.path())));
        let exact = menu(&format!("e {}/sub", dir.path()));
        assert_eq!(labels(&exact), ["sub/"]);
    }

    #[test]
    fn an_exact_candidate_comes_first() {
        let dir = Scratch::new("exact");
        let m = menu(&format!("export {}/a.pdf", dir.path()));
        assert_eq!(labels(&m), ["a.pdf"]);
    }

    #[test]
    fn a_cut_short_listing_is_shown_as_a_footer() {
        let dir = Scratch::new("cut");
        for i in 0..600 {
            std::fs::write(dir.0.join(format!("f{i}.pdf")), b"").unwrap();
        }
        let m = menu(&format!("e {}/", dir.path()));
        assert!(m.incomplete && m.is_shown());
        assert_eq!(m.layout(20), (MAX_ROWS - 1, true));
    }

    #[test]
    fn layout_fits_the_room() {
        let m = menu("");
        assert_eq!(m.layout(100), (MAX_ROWS, false));
        assert_eq!(m.layout(5), (5, false));
        assert_eq!(m.layout(0), (1, false));
        assert_eq!(menu("12").layout(5), (0, false));
    }

    #[test]
    fn the_highlight_wraps_and_stays_in_range() {
        let mut m = menu("zoom");
        let len = m.entries.len();
        assert_eq!(m.selected, 0);
        m.move_by(-1, 5);
        assert_eq!(m.selected, len - 1);
        m.move_by(1, 5);
        assert_eq!(m.selected, 0);
        m.move_by(1, 5);
        assert_eq!(m.selected, 1);
        m.selected = 1000;
        m.move_by(1, 5);
        assert_eq!(m.selected, 0, "a highlight past the end counts as the last");
        let mut none = menu("12");
        none.move_by(1, 5);
        none.move_by(-1, 0);
        none.select(3, 5);
        assert_eq!((none.selected, none.scroll), (0, 0));
        assert_eq!(none.enter(), Outcome::Typed);
    }

    #[test]
    fn half_pages_step_by_half_the_rows_and_stop_at_the_ends() {
        let mut m = menu("");
        assert!(m.entries.len() > 20);
        let last = m.entries.len() - 1;
        let step = m.layout(8).0 / 2;
        assert_eq!(step, 4);
        assert!(m.move_page(true, 8).is_some());
        assert_eq!(m.selected, step - 1, "from nothing, down lands on row step - 1");
        m.move_page(true, 8);
        assert_eq!(m.selected, 2 * step - 1);
        assert!(m.scroll <= m.selected && m.selected < m.scroll + 8);
        m.move_page(false, 8);
        assert_eq!(m.selected, step - 1);
        m.move_page(false, 8);
        assert_eq!(m.selected, 0, "clamped, not wrapped");
        m.select(last, 8);
        m.move_page(true, 8);
        assert_eq!(m.selected, last);
        let mut fresh = menu("");
        fresh.move_page(false, 8);
        assert_eq!(fresh.selected, 0);
        assert_eq!(m.move_page(false, 0), Some(m.entries[last - 1].fill.clone()), "a step is at least one");
    }

    #[test]
    fn half_pages_on_an_empty_menu_do_nothing() {
        let mut m = menu("12");
        assert_eq!(m.move_page(true, 8), None);
        assert_eq!(m.move_page(false, 0), None);
    }

    #[test]
    fn the_list_scrolls_to_keep_the_highlight_visible() {
        let mut m = menu("");
        let rows = 5;
        m.move_by(1, rows);
        assert_eq!(m.highlighted(), Some(0), "the first Down chooses the first");
        for _ in 0..rows {
            m.move_by(1, rows);
            assert!(m.scroll <= m.selected && m.selected < m.scroll + rows);
        }
        assert_eq!((m.selected, m.scroll), (5, 1));
        m.move_by(-1, rows);
        m.move_by(-1, rows);
        assert_eq!(m.scroll, 1);
        m.move_by(-3, rows);
        assert_eq!((m.selected, m.scroll), (0, 0));
        m.move_by(-1, rows);
        assert_eq!(m.selected, m.entries.len() - 1);
        assert_eq!(m.scroll, m.entries.len() - rows);
    }

    #[test]
    fn the_wheel_scrolls_within_bounds() {
        let mut m = menu("");
        m.scroll_pixels(-100.0, 10.0, 5);
        assert_eq!(m.scroll, 0);
        m.scroll_pixels(30.0, 10.0, 5);
        assert_eq!(m.scroll, 3);
        m.scroll_pixels(1.0e9, 10.0, 5);
        assert_eq!(m.scroll, m.entries.len() - 5);
        m.scroll_pixels(f32::NAN, 10.0, 5);
        m.scroll_pixels(5.0, 0.0, 5);
        assert_eq!(m.scroll, m.entries.len() - 5);
        let mut m = menu("");
        m.scroll_pixels(4.0, 10.0, 5);
        m.scroll_pixels(4.0, 10.0, 5);
        assert_eq!(m.scroll, 0);
        m.scroll_pixels(4.0, 10.0, 5);
        assert_eq!(m.scroll, 1, "small steps add up");
    }

    #[test]
    fn enter_runs_an_action() {
        assert_eq!(menu("zoom_in").enter(), Outcome::Run(Action::ZoomIn));
        assert_eq!(menu("toggle_toc").enter(), Outcome::Run(Action::ToggleToc));
    }

    #[test]
    fn enter_asks_for_a_required_argument() {
        for name in ["e", "edit", "theme", "zoom"] {
            assert_eq!(menu(name).enter(), Outcome::Fill(format!("{name} ")), "{name}");
        }
    }

    #[test]
    fn enter_runs_a_command_that_may_do_without_its_argument() {
        for name in ["open", "export", "export!"] {
            assert_eq!(menu(name).enter(), Outcome::Command(name.to_owned()), "{name}");
            assert_eq!(menu(&format!("{name} ")).enter(), Outcome::Typed, "{name} with a space and no argument");
        }
    }

    #[test]
    fn enter_runs_a_command_and_a_candidate() {
        assert_eq!(menu("w").enter(), Outcome::Command("w".to_owned()));
        assert_eq!(menu("q").enter(), Outcome::Command("q".to_owned()));
        let mut m = menu("theme l");
        m.move_by(1, 5);
        assert_eq!(m.enter(), Outcome::Command("theme light".to_owned()));
    }

    #[test]
    fn enter_with_nothing_found_runs_what_was_typed() {
        for text in ["12", "zoom 150", "nonsense", "e /nonexistent-dir/x.pdf", ""] {
            let m = menu(text);
            assert_eq!(m.enter(), Outcome::Typed, "{text}");
        }
    }

    #[test]
    fn typed_commands_in_full_pick_themselves() {
        for text in ["w", "q", "write", "quit", "wq", "q!", "noh"] {
            assert_eq!(menu(text).enter(), Outcome::Command(text.to_owned()), "{text}");
        }
    }

    #[test]
    fn a_command_beats_an_action_of_the_same_name() {
        let dispatcher = include_str!("actions.rs");
        let mut clashes: Vec<&str> = complete::COMMANDS.iter().map(|c| c.name).filter(|c| Action::from_name(c).is_some()).collect();
        clashes.sort_unstable();
        assert_eq!(clashes, ["open", "quit", "reload"]);
        for name in clashes {
            assert!(dispatcher.contains(&format!("\"{name}\"")), "{name} must be a command arm before the action fallback");
        }
    }

    #[test]
    fn edge_unicode_queries_and_names_do_not_panic() {
        for query in ["é", "ZOOM", "ÀÉÎ", "日本語", "zoom\u{301}", "İ", "ß", "😀", "\u{0}", "z o"] {
            let m = menu(query);
            for entry in &m.entries {
                let count = entry.label.chars().count();
                assert!(entry.matched.iter().all(|i| *i < count), "{query}: {entry:?}");
            }
            let _ = m.enter();
        }
        assert!(menu("日本語").entries.is_empty());
        assert_eq!(rank("日本語テキスト", "本語").map(|r| (r.0, r.1)), Some((2, vec![1, 2])));
        assert_eq!(rank("日本語", "日語").map(|r| r.1), Some(vec![0, 2]));
        assert_eq!(rank("ÉCOLE", "école").map(|r| r.0), Some(0));
        assert_eq!(rank("zoom", "").map(|r| r.0), Some(1));
        assert_eq!(rank("", "").map(|r| r.0), Some(0));
        assert_eq!(rank("", "a"), None);
    }

    #[test]
    fn edge_a_query_longer_than_every_name_matches_nothing() {
        let long = "z".repeat(500);
        let m = menu(&long);
        assert!(m.entries.is_empty() && !m.is_shown());
        assert_eq!(m.enter(), Outcome::Typed);
        assert_eq!(rank("zoom", "zoom_in_and_more"), None);
        assert_eq!(rank("zoom", "zoomm"), None);
    }

    #[test]
    fn edge_a_stale_highlight_after_refiltering_to_nothing_is_harmless() {
        let mut m = menu("");
        m.move_by(7, 12);
        m.scroll_pixels(40.0, 10.0, 12);
        let mut gone = menu("qqqqqqqqqq");
        gone.selected = m.selected;
        gone.scroll = m.scroll;
        assert!(gone.entries.is_empty());
        assert_eq!(gone.enter(), Outcome::Typed);
        gone.move_by(1, 12);
        gone.move_by(-1, 12);
        gone.select(3, 12);
        gone.scroll_pixels(100.0, 10.0, 12);
        assert_eq!(gone.tab(false, 12), None);
        assert_eq!(gone.layout(12), (0, false));
        let mut few = menu("zoom_in");
        few.selected = 99;
        few.scroll = 99;
        assert_eq!(few.enter(), Outcome::Typed, "a highlight past the end picks nothing");
        few.move_by(1, 12);
        assert!(few.selected < few.entries.len() && few.scroll < few.entries.len().max(1));
    }

    #[test]
    fn edge_move_by_on_an_empty_menu_and_with_extreme_deltas() {
        let mut m = menu("12");
        for delta in [0, 1, -1, isize::MAX, isize::MIN] {
            m.move_by(delta, 0);
            assert_eq!((m.selected, m.scroll), (0, 0));
        }
        let mut full = menu("");
        let len = full.entries.len();
        for delta in [isize::MAX, isize::MIN, 0, -1] {
            full.move_by(delta, 12);
            assert!(full.selected < len);
            assert!(full.scroll + full.layout(12).0 <= len);
        }
    }

    #[test]
    fn edge_scroll_arithmetic_with_no_room() {
        let mut m = menu("");
        m.select(0, 0);
        for room in [0, 1] {
            m.move_by(5, room);
            assert_eq!(m.selected, 5);
            assert_eq!(m.scroll, 5, "one visible row follows the highlight");
            m.move_by(-5, room);
            m.scroll_pixels(30.0, 10.0, room);
            assert!(m.scroll < m.entries.len());
        }
        m.move_by(-1, 0);
        assert_eq!(m.selected, m.entries.len() - 1);
        assert_eq!(m.scroll, m.entries.len() - 1);
        let mut empty = menu("12");
        empty.scroll_pixels(30.0, 10.0, 0);
        assert_eq!(empty.scroll, 0);
    }

    #[test]
    fn edge_the_wheel_ignores_nonsense_and_survives_huge_deltas() {
        let mut m = menu("");
        let len = m.entries.len();
        for (pixels, row) in
            [(f32::NAN, 10.0), (f32::INFINITY, 10.0), (f32::NEG_INFINITY, 10.0), (10.0, f32::NAN), (10.0, 0.0), (10.0, -3.0), (10.0, f32::INFINITY)]
        {
            m.scroll_pixels(pixels, row, 12);
            assert_eq!(m.scroll, 0, "{pixels} {row}");
        }
        m.scroll_pixels(f32::MAX, 1.0, 12);
        assert_eq!(m.scroll, len - MAX_ROWS);
        m.scroll_pixels(-f32::MAX, 1.0, 12);
        assert_eq!(m.scroll, 0);
        m.scroll_pixels(1e-30, 1e30, 12);
        assert!(m.carry.is_finite());
    }

    #[test]
    fn edge_a_wheel_ratio_that_overflows_does_not_poison_the_carry() {
        let mut m = menu("");
        m.scroll_pixels(f32::MAX, f32::MIN_POSITIVE, 12);
        assert!(m.carry.is_finite(), "carry is {}: pixels / row_height overflowed to infinity and the wheel would stall for good", m.carry);
        m.scroll_pixels(-f32::MAX, 1.0, 12);
        m.scroll_pixels(30.0, 10.0, 12);
        assert_eq!(m.scroll, 3, "the wheel still works afterwards");
    }

    #[test]
    fn edge_an_argument_query_on_a_missing_directory() {
        for text in [
            "e /no/such/dir/",
            "e /no/such/dir/x",
            "open /no/such/",
            "e ~/no-such-dir-yavcpdf/",
            "e \u{65e5}\u{672c}/",
            "e ",
            "e  ",
            "theme zzz",
            "theme \u{65e5}",
        ] {
            let m = menu(text);
            let _ = m.layout(12);
            let _ = m.enter();
        }
        assert!(menu("e /no/such/dir/").entries.is_empty());
        assert_eq!(menu("e /no/such/dir/x").enter(), Outcome::Typed);
    }

    #[test]
    fn edge_unicode_file_names_are_listed_and_filled_whole() {
        let dir = Scratch::new("unicode");
        std::fs::write(dir.0.join("\u{65e5}\u{672c}.pdf"), b"").unwrap();
        std::fs::create_dir_all(dir.0.join("caf\u{e9}")).unwrap();
        let m = menu(&format!("e {}/", dir.path()));
        let names = labels(&m);
        assert!(names.contains(&"\u{65e5}\u{672c}.pdf") && names.contains(&"caf\u{e9}/"), "{names:?}");
        let at = position(&m, "caf\u{e9}/");
        let mut m = m;
        m.select(at, 12);
        assert_eq!(m.enter(), Outcome::Fill(format!("e {}/caf\u{e9}/", dir.path())));
    }

    // What is highlighted is what Enter runs.

    fn tab_text(m: &mut Menu, backward: bool) -> Option<String> {
        m.tab(backward, 5)
    }

    #[test]
    fn enter_never_overwrites_a_file_by_an_argument() {
        let dir = Scratch::new("overwrite");
        std::fs::write(dir.0.join("notes.txt"), b"").unwrap();
        for text in
            [format!("export! {}/notes", dir.path()), format!("export! {}/", dir.path()), format!("export {}/a", dir.path()), "export! ".to_owned()]
        {
            let m = menu(&text);
            assert_eq!(m.highlighted(), None, "{text}");
            assert_eq!(m.enter(), Outcome::Typed, "{text}");
        }
    }

    #[test]
    fn enter_runs_exactly_what_was_typed_when_nothing_is_highlighted() {
        // `e!` and `x!` once fuzzy-matched `export!`, which overwrote the default export.
        for text in ["e!", "x!", "d", "n", "Q", "theme dar", "e ", "e", "zoom_in 3", "", "   "] {
            let m = menu(text);
            if text == "e" {
                // An exact name is highlighted, and Enter asks for its argument.
                assert_eq!(m.enter(), Outcome::Fill("e ".to_owned()));
                continue;
            }
            assert_eq!(m.highlighted(), None, "{text:?}");
            assert_eq!(m.enter(), Outcome::Typed, "{text:?}");
        }
    }

    #[test]
    fn a_name_that_is_the_only_one_to_continue_is_picked_unless_it_is_irreversible() {
        assert_eq!(menu("zoom_i").enter(), Outcome::Run(Action::ZoomIn));
        assert_eq!(menu("ZOOM_I").enter(), Outcome::Typed, "a prefix in another case is not picked: commands are case-sensitive");
        assert_eq!(menu("wri").enter(), Outcome::Command("write".to_owned()));
        for text in ["qui", "wq", "delete_a", "delete_p", "expo", "export"] {
            let m = menu(text);
            let exact = m.entries.first().is_some_and(|e| e.label == text);
            assert_eq!(m.highlighted().is_some(), exact, "{text}");
        }
        assert_eq!(menu("qui").enter(), Outcome::Typed);
        assert_eq!(menu("delete_a").enter(), Outcome::Typed);
        assert_eq!(menu("delete_annotation").enter(), Outcome::Run(Action::DeleteAnnotation));
        assert_eq!(menu("q!").enter(), Outcome::Command("q!".to_owned()));
        assert_eq!(menu("export!").enter(), Outcome::Command("export!".to_owned()));
    }

    #[test]
    fn an_exact_match_is_case_sensitive() {
        assert_eq!(menu("Q").enter(), Outcome::Typed);
        assert_eq!(menu("Zoom_In").enter(), Outcome::Typed, "in another case it is not picked either");
        for text in ["U", "Zoom_I", "ZOOM_IN", "Wri"] {
            let m = menu(text);
            assert_eq!(m.highlighted(), None, "{text}");
            assert_eq!(m.enter(), Outcome::Typed, "{text}");
        }
        assert!(menu("Zoom_I").entries.iter().any(|e| e.label == "zoom_in"), "the list still ignores case");
    }

    #[test]
    fn text_after_the_cursor_picks_nothing_and_enter_runs_what_is_typed() {
        // `wq` with the cursor in the middle, `q|!`, `u|nknown`, `nex|tfoo`.
        for before in ["w", "q", "u", "nex", "wq", "zoom_in"] {
            let mut m = menu(before);
            m.set_tail(true);
            assert_eq!(m.highlighted(), None, "{before}");
            assert_eq!(m.enter(), Outcome::Typed, "{before}");
        }
        let mut chosen = menu("zoo");
        chosen.move_by(1, 5);
        chosen.set_tail(true);
        assert_eq!((chosen.highlighted(), chosen.enter()), (None, Outcome::Typed), "even a chosen entry");
        let mut theme = menu("theme ");
        theme.select(0, 5);
        theme.set_tail(true);
        assert_eq!(theme.enter(), Outcome::Typed, "`th|eme dark` is not filled over");
        let mut back = menu("nex");
        back.set_tail(true);
        back.set_tail(false);
        assert_eq!(back.enter(), Outcome::Typed, "nothing is picked again until the menu is made again");
    }

    #[test]
    fn tab_with_text_after_the_cursor_still_fills_before_it() {
        let mut m = menu("th");
        m.set_tail(true);
        assert_eq!(tab_text(&mut m, false).as_deref(), Some("theme "));
    }

    #[test]
    fn up_and_down_put_the_entry_in_the_field_like_tab() {
        let mut m = menu("zoo");
        let fill = m.move_by(1, 5).unwrap();
        assert_eq!(fill, m.entries[m.highlighted().unwrap()].fill);
        assert!(m.is_for("zoo") && m.is_for(&fill));
        let up = m.move_by(-1, 5).unwrap();
        assert_eq!(Some(up.as_str()), m.entries.get(m.highlighted().unwrap()).map(|e| e.fill.as_str()));
        assert_eq!(menu("zoo").select(1, 5).as_deref(), Some(menu("zoo").entries[1].fill.as_str()));
        assert_eq!(menu("12").move_by(1, 5), None);
    }

    #[test]
    fn down_on_export_bang_shows_the_whole_file_name_before_enter() {
        let dir = Scratch::new("exportdown");
        std::fs::write(dir.0.join("notes.txt"), b"").unwrap();
        let mut m = menu(&format!("export! {}/no", dir.path()));
        assert_eq!(m.enter(), Outcome::Typed);
        let shown = m.move_by(1, 5).unwrap();
        assert_eq!(shown, format!("export! {}/notes.txt", dir.path()));
        assert_eq!(m.enter(), Outcome::Command(shown), "Enter runs exactly what the field shows");
    }

    #[test]
    fn several_names_continuing_the_query_pick_nothing() {
        for text in ["d", "n", "to", "zoom_", "ex", "t"] {
            assert_eq!(menu(text).highlighted(), None, "{text}");
        }
    }

    #[test]
    fn the_keys_choose_and_choosing_is_what_enter_runs() {
        let mut m = menu("d");
        assert_eq!(m.enter(), Outcome::Typed);
        m.move_by(1, 5);
        assert_eq!(m.highlighted(), Some(0));
        assert_ne!(m.enter(), Outcome::Typed, "what was chosen is what Enter uses");
        let mut up = menu("d");
        up.move_by(-1, 5);
        assert_eq!(up.highlighted(), Some(up.entries.len() - 1));
        let mut click = menu("d");
        click.select(2, 5);
        assert_eq!(click.highlighted(), Some(2));
        click.select(10_000, 5);
        assert_eq!(click.highlighted(), Some(2), "a click past the end changes nothing");
    }

    #[test]
    fn choosing_an_argument_candidate_is_the_only_way_to_run_it() {
        let dir = Scratch::new("choose");
        let mut m = menu(&format!("e {}/", dir.path()));
        assert_eq!(m.enter(), Outcome::Typed);
        m.select(1, 5);
        assert_eq!(m.enter(), Outcome::Fill(format!("e {}/sub/", dir.path())), "a directory descends");
        m.select(0, 5);
        assert_eq!(m.enter(), Outcome::Command(format!("e {}/a.pdf", dir.path())), "a file runs the command");
        let mut theme = menu("theme dar");
        assert_eq!(theme.enter(), Outcome::Typed);
        theme.select(0, 5);
        assert_eq!(theme.enter(), Outcome::Command("theme dark".to_owned()));
    }

    // Tab.

    #[test]
    fn tab_completes_a_name_that_is_listed_though_not_a_prefix_of_the_commands() {
        let mut m = menu("zoom_i");
        assert_eq!(tab_text(&mut m, false).as_deref(), Some("zoom_in"));
        assert_eq!(m.highlighted().and_then(|i| m.entries.get(i)).map(|e| e.label.as_str()), Some("zoom_in"));
        assert_eq!(m.enter(), Outcome::Run(Action::ZoomIn));
    }

    #[test]
    fn the_menu_keeps_filtering_on_the_query_while_tabs_fill_the_field() {
        let mut m = menu("zoo");
        let all: Vec<String> = m.entries.iter().map(|e| e.fill.clone()).collect();
        assert!(all.len() >= 3);
        let first = tab_text(&mut m, false).unwrap();
        assert_eq!(first, all[0]);
        assert!(m.is_for("zoo") && m.is_for(&first), "the entries stay for the query and for what Tab put in the field");
        assert!(!m.is_for("zoom_x"));
        let second = tab_text(&mut m, false).unwrap();
        assert_eq!(second, all[1]);
        assert_eq!(tab_text(&mut m, true).as_deref(), Some(all[0].as_str()));
        assert_eq!(tab_text(&mut m, true).as_deref(), Some(all[all.len() - 1].as_str()), "back from the first wraps to the last");
        for _ in 0..all.len() {
            tab_text(&mut m, false);
        }
        assert_eq!(m.entries.len(), all.len());
    }

    #[test]
    fn shift_tab_first_goes_to_the_last_entry() {
        let mut m = menu("zoo");
        let last = m.entries.last().unwrap().fill.clone();
        assert_eq!(tab_text(&mut m, true), Some(last));
    }

    #[test]
    fn tab_on_an_exact_name_takes_it_and_the_next_steps_on() {
        let mut m = menu("e");
        assert_eq!(tab_text(&mut m, false).as_deref(), Some("e "));
        let next = m.entries[1].fill.clone();
        assert_eq!(tab_text(&mut m, false), Some(next));
    }

    #[test]
    fn tab_with_one_entry_fills_it() {
        let dir = Scratch::new("one");
        let mut m = menu(&format!("e {}/su", dir.path()));
        assert_eq!(tab_text(&mut m, false), Some(format!("e {}/sub/", dir.path())));
        // The only candidate is a folder, so the menu is made again for it and lists its contents.
        assert!(!m.is_for(&format!("e {}/sub/", dir.path())));
        let into = menu(&format!("e {}/sub/", dir.path()));
        assert_eq!(labels(&into), Vec::<&str>::new());
        let mut theme = menu("theme li");
        assert_eq!(tab_text(&mut theme, false).as_deref(), Some("theme light"));
    }

    #[test]
    fn tab_on_the_only_folder_goes_into_it_and_lists_its_contents() {
        let dir = Scratch::new("tab-into");
        std::fs::create_dir_all(dir.0.join("sub/inner")).unwrap();
        std::fs::write(dir.0.join("sub/b.pdf"), b"").unwrap();
        std::fs::write(dir.0.join("sub/skip.txt"), b"").unwrap();
        let mut m = menu(&format!("e {}/su", dir.path()));
        let filled = format!("e {}/sub/", dir.path());
        assert_eq!(tab_text(&mut m, false), Some(filled.clone()));
        assert!(!m.is_for(&filled));
        let mut m = Menu::with_cache(&filled, &keys(), &themes(), Path::new("/nonexistent-home"), m.take_cache());
        let mut got = labels(&m);
        got.sort_unstable();
        assert_eq!(got, ["b.pdf", "inner/"]);
        assert_eq!(m.highlighted(), None);
        // The next Tab works on the listing as usual: it chooses the first.
        assert_eq!(tab_text(&mut m, false), Some(format!("{filled}{}", m.entries[0].label)));
    }

    #[test]
    fn tab_with_several_candidates_cycles_onto_folders_without_descending() {
        let dir = Scratch::new("cycle");
        std::fs::create_dir_all(dir.0.join("sbook")).unwrap();
        std::fs::write(dir.0.join("sbook.pdf"), b"").unwrap();
        let mut m = menu(&format!("e {}/s", dir.path()));
        let first = tab_text(&mut m, false).unwrap();
        let second = tab_text(&mut m, false).unwrap();
        assert_ne!(first, second);
        assert!(m.is_for(&second), "the menu keeps its entries while cycling");
        assert_eq!(m.entries.len(), 3);
    }

    /// The `:` bar as `App::bar_key` and `make_menu` drive it: Tab fills the field before the
    /// cursor, then the menu is made again unless it is still for that text.
    struct Bar {
        field: crate::field::Field,
        menu: Menu,
        home: std::path::PathBuf,
    }

    impl Bar {
        fn new(text: &str) -> Self {
            Self::with_home(text, "/nonexistent-home")
        }

        fn with_home(text: &str, home: &str) -> Self {
            let home = std::path::PathBuf::from(home);
            let field = crate::field::Field::new(text.to_owned());
            let menu = Menu::with_cache(field.before_cursor(), &keys(), &themes(), &home, DirCache::default());
            Self { field, menu, home }
        }

        fn tab(&mut self, backward: bool) {
            if let Some(text) = self.menu.tab(backward, 5) {
                self.field.replace_before_cursor(&text);
            }
            let (before, tail) = (self.field.before_cursor().to_owned(), !self.field.at_end());
            if self.menu.is_for(&before) {
                self.menu.set_tail(tail);
                return;
            }
            let cache = self.menu.take_cache();
            self.menu = Menu::with_cache(&before, &keys(), &themes(), &self.home, cache);
            self.menu.set_tail(tail);
        }

        fn labels(&self) -> Vec<String> {
            let mut v: Vec<String> = self.menu.entries.iter().map(|e| e.label.clone()).collect();
            v.sort();
            v
        }
    }

    fn tree(name: &str) -> Scratch {
        let dir = Scratch::new(name);
        std::fs::remove_dir_all(dir.0.join("sub")).unwrap();
        std::fs::remove_file(dir.0.join("a.pdf")).unwrap();
        dir
    }

    #[test]
    fn tab_on_the_only_folder_lists_what_the_command_accepts_for_each_of_its_names() {
        let dir = tree("t1");
        std::fs::create_dir_all(dir.0.join("Books/inner")).unwrap();
        std::fs::write(dir.0.join("Books/b.pdf"), b"").unwrap();
        std::fs::write(dir.0.join("Books/n.txt"), b"").unwrap();
        for cmd in ["e", "edit", "open"] {
            let mut bar = Bar::new(&format!("{cmd} {}/Boo", dir.path()));
            bar.tab(false);
            assert_eq!(bar.field.text, format!("{cmd} {}/Books/", dir.path()));
            assert_eq!(bar.labels(), ["b.pdf", "inner/"], "{cmd}");
            assert_eq!(bar.menu.highlighted(), None);
        }
        let mut bar = Bar::new(&format!("export {}/Boo", dir.path()));
        bar.tab(false);
        assert_eq!(bar.labels(), ["b.pdf", "inner/", "n.txt"]);
    }

    #[test]
    fn after_going_into_a_folder_tab_adds_the_shared_start_then_cycles_and_wraps() {
        let dir = tree("t2");
        std::fs::create_dir_all(dir.0.join("Books")).unwrap();
        std::fs::write(dir.0.join("Books/abc1.pdf"), b"").unwrap();
        std::fs::write(dir.0.join("Books/abc2.pdf"), b"").unwrap();
        let mut bar = Bar::new(&format!("e {}/Boo", dir.path()));
        bar.tab(false);
        bar.tab(false);
        assert_eq!(bar.field.text, format!("e {}/Books/abc", dir.path()), "shared start");
        bar.tab(false);
        assert_eq!(bar.field.text, format!("e {}/Books/abc1.pdf", dir.path()));
        bar.tab(false);
        assert_eq!(bar.field.text, format!("e {}/Books/abc2.pdf", dir.path()));
        bar.tab(false);
        assert_eq!(bar.field.text, format!("e {}/Books/abc1.pdf", dir.path()), "wraps");
        // No shared start: cycles from the first.
        std::fs::write(dir.0.join("Books/x.pdf"), b"").unwrap();
        let mut bar = Bar::new(&format!("e {}/Boo", dir.path()));
        bar.tab(false);
        bar.tab(false);
        assert!(bar.field.text.ends_with("Books/abc1.pdf"), "{}", bar.field.text);
    }

    #[test]
    fn tab_goes_down_a_chain_of_only_folders_to_the_file() {
        let dir = tree("t3");
        std::fs::create_dir_all(dir.0.join("a/b")).unwrap();
        std::fs::write(dir.0.join("a/b/c.pdf"), b"").unwrap();
        std::fs::write(dir.0.join("a/b/d.txt"), b"").unwrap();
        let mut bar = Bar::new(&format!("e {}/a", dir.path()));
        bar.tab(false);
        assert_eq!(bar.field.text, format!("e {}/a/", dir.path()));
        assert_eq!(bar.labels(), ["b/"]);
        bar.tab(false);
        assert_eq!(bar.field.text, format!("e {}/a/b/", dir.path()));
        assert_eq!(bar.labels(), ["c.pdf"]);
        bar.tab(false);
        assert_eq!(bar.field.text, format!("e {}/a/b/c.pdf", dir.path()));
    }

    #[cfg(unix)]
    #[test]
    fn tab_goes_into_the_only_folder_that_is_a_symlink() {
        let dir = tree("t4");
        std::fs::create_dir_all(dir.0.join("real")).unwrap();
        std::fs::write(dir.0.join("real/r.pdf"), b"").unwrap();
        std::os::unix::fs::symlink(dir.0.join("real"), dir.0.join("lnk")).unwrap();
        let mut bar = Bar::new(&format!("e {}/ln", dir.path()));
        assert_eq!(bar.labels(), ["lnk/"]);
        bar.tab(false);
        assert_eq!(bar.field.text, format!("e {}/lnk/", dir.path()));
        assert_eq!(bar.labels(), ["r.pdf"]);
    }

    #[test]
    fn tab_on_a_tilde_goes_into_home_and_lists_it() {
        let home = tree("t5");
        std::fs::write(home.0.join("h.pdf"), b"").unwrap();
        std::fs::create_dir_all(home.0.join("docs")).unwrap();
        let mut bar = Bar::with_home("e ~", home.path());
        bar.tab(false);
        assert_eq!(bar.field.text, "e ~/");
        assert_eq!(bar.labels(), ["docs/", "h.pdf"]);
        let mut bar = Bar::with_home("e ~/do", home.path());
        bar.tab(false);
        assert_eq!(bar.field.text, "e ~/docs/");
        assert!(bar.labels().is_empty());
    }

    #[test]
    fn tab_with_several_candidates_cycles_both_ways_and_enter_fills_a_folder() {
        let dir = tree("t6");
        std::fs::create_dir_all(dir.0.join("sa")).unwrap();
        std::fs::write(dir.0.join("sa/in.pdf"), b"").unwrap();
        std::fs::write(dir.0.join("sb.pdf"), b"").unwrap();
        std::fs::create_dir_all(dir.0.join("sc")).unwrap();
        let p = dir.path();
        let mut bar = Bar::new(&format!("e {p}/s"));
        let seen: Vec<String> = (0..4)
            .map(|_| {
                bar.tab(false);
                bar.field.text.clone()
            })
            .collect();
        assert_eq!(seen[0], format!("e {p}/sa/"));
        assert_eq!(seen[1], format!("e {p}/sb.pdf"));
        assert_eq!(seen[2], format!("e {p}/sc/"));
        assert_eq!(seen[3], seen[0], "wraps");
        assert_eq!(bar.menu.entries.len(), 3, "never descended");
        bar.tab(true);
        assert_eq!(bar.field.text, seen[2], "shift-tab goes back and wraps");
        bar.tab(true);
        assert_eq!(bar.field.text, seen[1]);
        bar.tab(true);
        assert_eq!(bar.field.text, seen[0]);
        assert_eq!(bar.menu.enter(), Outcome::Fill(seen[0].clone()));
        let mut bar = Bar::new(&format!("e {p}/s"));
        bar.tab(true);
        assert_eq!(bar.field.text, seen[2]);
    }

    #[test]
    fn tab_on_the_only_folder_keeps_the_text_after_the_cursor() {
        let dir = tree("t7");
        std::fs::create_dir_all(dir.0.join("Books")).unwrap();
        std::fs::write(dir.0.join("Books/b.pdf"), b"").unwrap();
        let before = format!("e {}/Boo", dir.path());
        let mut bar = Bar::new(&format!("{before}TAIL"));
        for _ in 0..4 {
            bar.field.apply(crate::field::Edit::Left);
        }
        bar.menu = Menu::with_cache(bar.field.before_cursor(), &keys(), &themes(), &bar.home, DirCache::default());
        bar.menu.set_tail(true);
        bar.tab(false);
        assert_eq!(bar.field.text, format!("e {}/Books/TAIL", dir.path()));
        assert_eq!(bar.field.before_cursor(), format!("e {}/Books/", dir.path()));
        assert_eq!(bar.labels(), ["b.pdf"]);
    }

    #[test]
    fn tab_into_an_empty_only_folder_lists_nothing_and_tab_then_does_nothing() {
        let dir = tree("t8");
        std::fs::create_dir_all(dir.0.join("void")).unwrap();
        let mut bar = Bar::new(&format!("e {}/vo", dir.path()));
        bar.tab(false);
        assert_eq!(bar.field.text, format!("e {}/void/", dir.path()));
        assert!(bar.menu.entries.is_empty());
        assert!(!bar.menu.is_shown());
        let before = bar.field.text.clone();
        bar.tab(false);
        bar.tab(true);
        assert_eq!(bar.field.text, before);
    }

    #[test]
    fn tab_goes_into_the_only_folder_with_spaces_or_multibyte_names() {
        let dir = tree("t9");
        for name in ["my books", "caf\u{e9} \u{65e5}\u{672c}"] {
            std::fs::create_dir_all(dir.0.join(name)).unwrap();
            std::fs::write(dir.0.join(name).join("x.pdf"), b"").unwrap();
        }
        let mut bar = Bar::new(&format!("e {}/my", dir.path()));
        bar.tab(false);
        assert_eq!(bar.labels(), ["x.pdf"], "{:?}", bar.field.text);
        assert!(bar.field.text.ends_with("my books/") || bar.field.text.ends_with("my\\ books/"), "{}", bar.field.text);
        let mut bar = Bar::new(&format!("e {}/caf", dir.path()));
        bar.tab(false);
        assert!(bar.field.text.ends_with('/'), "{}", bar.field.text);
        assert_eq!(bar.labels(), ["x.pdf"]);
    }

    #[test]
    fn down_then_tab_on_the_only_folder_goes_into_it() {
        let dir = tree("down-tab");
        std::fs::create_dir_all(dir.0.join("Books")).unwrap();
        std::fs::write(dir.0.join("Books/b.pdf"), b"").unwrap();
        let mut bar = Bar::new(&format!("e {}/Boo", dir.path()));
        let books = format!("e {}/Books/", dir.path());
        let down = bar.menu.move_by(1, 5).unwrap();
        bar.field.replace_before_cursor(&down);
        assert_eq!(bar.field.text, books);
        assert!(bar.menu.is_for(&books), "the menu is kept after Down");
        bar.tab(false);
        assert_eq!(bar.field.text, books);
        assert_eq!(bar.labels(), ["b.pdf"]);
        assert!(bar.menu.is_for(&books));
        assert_eq!(bar.menu.highlighted(), None);
    }

    #[test]
    fn tab_on_the_only_folder_of_a_cut_short_listing_only_fills_it() {
        let dir = tree("cut-short");
        std::fs::create_dir_all(dir.0.join("Books")).unwrap();
        std::fs::write(dir.0.join("Books/b.pdf"), b"").unwrap();
        let mut m = menu(&format!("e {}/Boo", dir.path()));
        m.incomplete = true;
        let books = format!("e {}/Books/", dir.path());
        assert_eq!(tab_text(&mut m, false), Some(books.clone()));
        assert!(m.is_for(&books), "the menu is kept, not made again");
    }

    #[test]
    fn tab_on_the_only_pdf_keeps_the_menu() {
        let dir = Scratch::new("pdf");
        let mut m = menu(&format!("e {}/a.p", dir.path()));
        let filled = format!("e {}/a.pdf", dir.path());
        assert_eq!(tab_text(&mut m, false), Some(filled.clone()));
        assert!(m.is_for(&filled));
        assert_eq!(m.enter(), Outcome::Command(filled));
    }

    #[test]
    fn the_first_tab_in_an_argument_adds_what_the_candidates_share() {
        let dir = Scratch::new("shared");
        for name in ["abc1.pdf", "abc2.pdf"] {
            std::fs::write(dir.0.join(name), b"").unwrap();
        }
        let mut m = menu(&format!("e {}/ab", dir.path()));
        assert_eq!(tab_text(&mut m, false), Some(format!("e {}/abc", dir.path())));
        assert_eq!(m.highlighted(), None, "adding the shared start chooses nothing");
        assert_eq!(m.enter(), Outcome::Typed);
        // The field now reads that, so the menu is made again for it, and the next Tab chooses.
        assert!(!m.is_for(&format!("e {}/abc", dir.path())));
        let mut m = menu(&format!("e {}/abc", dir.path()));
        assert_eq!(tab_text(&mut m, false), Some(format!("e {}/abc1.pdf", dir.path())));
        assert_eq!(tab_text(&mut m, false), Some(format!("e {}/abc2.pdf", dir.path())));
    }

    #[test]
    fn tab_with_no_entries_does_nothing_and_leaves_no_highlight() {
        let mut m = menu("zzzzz");
        assert_eq!(tab_text(&mut m, false), None);
        assert_eq!((m.highlighted(), m.enter()), (None, Outcome::Typed));
        let mut page = menu("12");
        assert_eq!(tab_text(&mut page, true), None);
        let mut missing = menu("e /nonexistent-dir-for-yavcpdf/x");
        assert_eq!(tab_text(&mut missing, false), None);
    }

    #[test]
    fn tab_asks_for_the_argument_of_a_command_and_enter_then_lists_the_arguments() {
        let mut m = menu("the");
        assert_eq!(tab_text(&mut m, false).as_deref(), Some("theme "));
        assert_eq!(m.enter(), Outcome::Fill("theme ".to_owned()));
    }

    // The listing of a directory.

    #[test]
    fn the_directory_is_not_read_again_while_typing_in_it() {
        let dir = Scratch::new("listing");
        let table = keys();
        let home = Path::new("/nonexistent-home");
        let mut first = Menu::with_cache(&format!("e {}/", dir.path()), &table, &[], home, DirCache::default());
        assert_eq!(labels(&first), ["a.pdf", "sub/"]);
        std::fs::write(dir.0.join("ab.pdf"), b"").unwrap();
        let cache = first.take_cache();
        let mut second = Menu::with_cache(&format!("e {}/a", dir.path()), &table, &[], home, cache);
        assert_eq!(labels(&second), ["a.pdf"], "the new file is not seen: the same folder is not read again");
        let cache = second.take_cache();
        let third = Menu::with_cache(&format!("e {}/sub/", dir.path()), &table, &[], home, cache);
        assert!(third.entries.is_empty());
        let fourth = Menu::with_cache(&format!("e {}/a", dir.path()), &table, &[], home, DirCache::default());
        assert_eq!(labels(&fourth), ["a.pdf", "ab.pdf"]);
    }

    #[test]
    fn more_candidates_than_are_kept_make_the_listing_incomplete() {
        let candidates: Vec<String> = (0..5).map(|n| format!("/d/f{n}.pdf")).collect();
        let text = "e /d/f";
        let (entries, incomplete) = argument_entries(text, Completion { start: 2, candidates: candidates.clone(), incomplete: false }, 3);
        assert_eq!((entries.len(), incomplete), (3, true));
        let (entries, incomplete) = argument_entries(text, Completion { start: 2, candidates: candidates.clone(), incomplete: false }, 5);
        assert_eq!((entries.len(), incomplete), (5, false));
        let (_, incomplete) = argument_entries(text, Completion { start: 2, candidates, incomplete: true }, 5);
        assert!(incomplete);
    }

    // The count.

    #[test]
    fn the_count_shows_the_highlight_and_the_total_when_not_all_fit() {
        let mut m = menu("");
        let total = m.entries.len();
        assert_eq!(m.count_label(20), Some(total.to_string()), "nothing is highlighted");
        m.move_by(2, 20);
        assert_eq!(m.count_label(20), Some(format!("1/{total}")));
        m.move_by(2, 20);
        assert_eq!(m.count_label(20), Some(format!("3/{total}")));
        assert_eq!(menu("zoom_in").count_label(20), None, "all of one entry fit");
        assert_eq!(menu("12").count_label(20), None);
    }

    // Ranking.

    #[test]
    fn a_subsequence_prefers_word_starts() {
        let m = menu("tgl");
        assert_eq!(m.entries[0].label, "toggle_links");
        assert_eq!(rank("toggle_links", "tgl").map(|r| r.1), Some(vec![0, 2, 7]));
    }

    #[test]
    fn a_subsequence_then_prefers_the_compact_span() {
        let tight = rank("a_xbx_c", "abc").unwrap();
        let loose = rank("a_xxxbxxx_c", "abc").unwrap();
        assert!(tight.0 == 3 && loose.0 == 3 && tight.2 < loose.2);
        assert_eq!(rank("xa_xb", "ab").map(|r| r.1), Some(vec![1, 4]));
    }

    #[test]
    fn the_ranking_search_survives_long_input() {
        let name = "a_".repeat(60);
        let query = "a".repeat(40);
        assert!(rank(&name, &query).is_some());
        assert_eq!(rank(&name, &"a".repeat(61)), None);
    }

    #[test]
    fn the_wheel_clamps_a_ratio_that_overflows() {
        let mut m = menu("");
        m.scroll_pixels(f32::MAX, f32::MIN_POSITIVE, 12);
        assert!(m.carry.is_finite());
        assert_eq!(m.scroll, m.entries.len() - MAX_ROWS);
    }

    // Edge cases added in review.

    #[test]
    fn tabbing_at_a_mid_field_cursor_fills_only_the_text_before_it() {
        let dir = Scratch::new("midfield");
        for name in ["reading.pdf", "report1.pdf"] {
            std::fs::write(dir.0.join(name), b"").unwrap();
        }
        // The field is `e <dir>/re` with the cursor two to the left: the menu sees `e <dir>/`.
        let before = format!("e {}/", dir.path());
        let mut m = menu(&before);
        let fill = tab_text(&mut m, false).unwrap();
        assert!(fill.starts_with(&before) && !fill.ends_with("re"), "{fill}");
        assert_eq!(fill, format!("e {}/a.pdf", dir.path()));
        // The menu still answers to the query, and to what Tab filled, but not to anything else.
        assert!(m.is_for(&before) && m.is_for(&fill));
        assert!(!m.is_for(&format!("{fill}re")));
    }

    #[test]
    fn a_full_cycle_of_tabs_returns_to_the_start_in_both_directions() {
        let dir = Scratch::new("cycle");
        for name in ["reading.pdf", "report1.pdf", "report2.pdf"] {
            std::fs::write(dir.0.join(name), b"").unwrap();
        }
        std::fs::create_dir_all(dir.0.join("refs")).unwrap();
        let text = format!("e {}/re", dir.path());
        let mut m = menu(&text);
        assert_eq!(labels(&m), ["reading.pdf", "refs/", "report1.pdf", "report2.pdf"]);
        // `re` is all the candidates share, so the first Tab chooses the first.
        let forward: Vec<String> = (0..5).filter_map(|_| tab_text(&mut m, false)).collect();
        let want: Vec<String> =
            ["reading.pdf", "refs/", "report1.pdf", "report2.pdf", "reading.pdf"].map(|n| format!("e {}/{n}", dir.path())).to_vec();
        assert_eq!(forward, want);
        assert_eq!(labels(&m), ["reading.pdf", "refs/", "report1.pdf", "report2.pdf"], "the entries never change while tabbing");
        let back: Vec<String> = (0..2).filter_map(|_| tab_text(&mut m, true)).collect();
        assert!(back[0].ends_with("report2.pdf") && back[1].ends_with("report1.pdf"), "{back:?}");
    }

    #[test]
    fn a_tabbed_directory_is_descended_into_by_enter_and_a_tabbed_file_is_run() {
        let dir = Scratch::new("descend");
        std::fs::write(dir.0.join("sub/inner.pdf"), b"").unwrap();
        let mut m = menu(&format!("e {}/s", dir.path()));
        let fill = tab_text(&mut m, false).unwrap();
        assert_eq!(fill, format!("e {}/sub/", dir.path()));
        assert_eq!(m.enter(), Outcome::Fill(fill.clone()));
        let mut inner = menu(&fill);
        assert_eq!(labels(&inner), ["inner.pdf"]);
        // One entry: Tab fills it, then Enter runs exactly that.
        let file = tab_text(&mut inner, false).unwrap();
        assert_eq!(file, format!("e {}/sub/inner.pdf", dir.path()));
        assert_eq!(inner.enter(), Outcome::Command(file));
    }

    #[test]
    fn a_listing_cached_for_one_folder_is_dropped_for_another_and_read_again_on_return() {
        let dir = Scratch::new("recache");
        let table = keys();
        let home = Path::new("/nonexistent-home");
        let mut m = Menu::with_cache(&format!("e {}/", dir.path()), &table, &[], home, DirCache::default());
        std::fs::write(dir.0.join("late.pdf"), b"").unwrap();
        let mut m2 = Menu::with_cache(&format!("e {}/l", dir.path()), &table, &[], home, m.take_cache());
        assert!(m2.entries.is_empty(), "same folder: cached, the new file is unseen");
        let mut sub = Menu::with_cache(&format!("e {}/sub/", dir.path()), &table, &[], home, m2.take_cache());
        let back = Menu::with_cache(&format!("e {}/l", dir.path()), &table, &[], home, sub.take_cache());
        assert_eq!(labels(&back), ["late.pdf"], "a different folder in between drops the cache");
    }

    #[test]
    fn tab_on_a_cut_short_listing_does_not_add_a_shared_start() {
        let mut m = menu("e /nonexistent-dir-for-yavcpdf/x");
        m.entries = ["xa1.pdf", "xa2.pdf"]
            .map(|n| Entry {
                label: n.to_owned(),
                hint: String::new(),
                keys: String::new(),
                kind: Kind::Candidate { dir: false },
                fill: format!("e /d/{n}"),
                matched: Vec::new(),
            })
            .to_vec();
        m.text = "e /d/x".to_owned();
        m.argument = true;
        m.incomplete = true;
        assert_eq!(tab_text(&mut m, false).as_deref(), Some("e /d/xa1.pdf"));
    }

    #[test]
    fn tab_in_an_argument_never_picks_without_a_tab_even_for_an_irreversible_command() {
        let dir = Scratch::new("irrev");
        std::fs::write(dir.0.join("notes.txt"), b"x").unwrap();
        for command in ["export!", "export", "x", "wq", "q!"] {
            let m = menu(&format!("{command} {}/notes", dir.path()));
            assert_eq!((m.highlighted(), m.enter()), (None, Outcome::Typed), "{command}");
        }
    }
}
