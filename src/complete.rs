//! Tab completion for the `:` bar: pure functions from the text before the cursor to candidates,
//! and the cycle that the following Tabs step through.
use std::path::{Path, PathBuf};

use crate::field::Field;

/// The commands, and whether each takes an argument. This must match `App::command` in
/// actions.rs; `the_names_are_the_ones_the_dispatcher_knows` checks that each is mentioned there.
const COMMANDS: &[(&str, bool)] = &[
    ("q", false),
    ("quit", false),
    ("q!", false),
    ("wq", false),
    ("x", false),
    ("w", false),
    ("write", false),
    ("e", true),
    ("edit", true),
    ("open", true),
    ("noh", false),
    ("nohlsearch", false),
    ("theme", true),
    ("zoom", true),
    ("reload", false),
    ("toc", false),
    ("hotbar", false),
    ("export", true),
    ("export!", true),
    ("sign", false),
    ("marks", false),
];

/// How many directory entries are read, how many matches are kept, and how many candidates are
/// listed and cycled. Hitting either of the first two makes the listing incomplete.
#[cfg(not(test))]
const MAX_SCANNED: usize = 100_000;
#[cfg(test)]
const MAX_SCANNED: usize = 500;
#[cfg(not(test))]
const MAX_MATCHES: usize = 5000;
#[cfg(test)]
const MAX_MATCHES: usize = 400;
const MAX_CANDIDATES: usize = 300;
const _: () = assert!(MAX_CANDIDATES < MAX_MATCHES && MAX_MATCHES < MAX_SCANNED);
/// The most labels, and the most characters, that the message lists.
const MAX_LABELS: usize = 15;
const MAX_MESSAGE_CHARS: usize = 200;

/// What a command's argument is made of.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Argument {
    Theme,
    /// A directory or a PDF.
    Document,
    /// Any file or directory.
    Path,
}

fn argument_of(command: &str) -> Option<Argument> {
    match command {
        "theme" => Some(Argument::Theme),
        "e" | "edit" | "open" => Some(Argument::Document),
        "export" | "export!" => Some(Argument::Path),
        _ => None,
    }
}

/// The ways to finish the word that starts at byte `start` of the text.
#[derive(Debug, PartialEq, Eq)]
pub struct Completion {
    pub start: usize,
    /// Whole replacements for the word, each with its trailing space or slash.
    pub candidates: Vec<String>,
    /// The directory listing was cut short, so `candidates` may miss some matches.
    pub incomplete: bool,
}

/// The candidates for the text before the cursor, if any.
pub fn complete(before: &str, themes: &[String], home: &Path) -> Option<Completion> {
    let lead = before.len() - before.trim_start().len();
    let rest = before.get(lead..)?;
    let Some(end) = rest.find(char::is_whitespace) else {
        let candidates: Vec<String> = COMMANDS
            .iter()
            .filter(|(name, _)| name.starts_with(rest))
            .map(|(name, takes)| if *takes { format!("{name} ") } else { (*name).to_owned() })
            .collect();
        return (!candidates.is_empty()).then_some(Completion { start: lead, candidates, incomplete: false });
    };
    let argument = rest.get(end..)?;
    let offset = end + (argument.len() - argument.trim_start().len());
    let word = rest.get(offset..)?;
    let (candidates, incomplete) = match argument_of(rest.get(..end)?)? {
        Argument::Theme => (themes.iter().filter(|t| t.starts_with(word)).cloned().collect(), false),
        Argument::Document => paths(word, home, true),
        Argument::Path => paths(word, home, false),
    };
    // A cut-short listing with nothing seen is still an answer: Tab says so instead of staying silent.
    (!candidates.is_empty() || incomplete).then_some(Completion { start: lead + offset, candidates, incomplete })
}

fn is_pdf(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// The items whose name continues `prefix` (dotfiles only when the prefix starts with a dot),
/// and whether the scan or the match cap cut the list short. The name is checked before anything
/// is done with the item, so a long directory costs only its names.
fn matching<T>(
    items: impl Iterator<Item = (String, T)>,
    prefix: &str,
    show_hidden: bool,
    max_scanned: usize,
    max_matches: usize,
) -> (Vec<(String, T)>, bool) {
    let mut kept = Vec::new();
    for (scanned, (name, item)) in items.enumerate() {
        if scanned >= max_scanned {
            return (kept, true);
        }
        if !name.starts_with(prefix) || (name.starts_with('.') && !show_hidden) {
            continue;
        }
        if kept.len() >= max_matches {
            return (kept, true);
        }
        kept.push((name, item));
    }
    (kept, false)
}

/// The files and directories that continue `word`, and whether the listing is incomplete.
/// Directories end in a slash. Unreadable directories give nothing.
fn paths(word: &str, home: &Path, documents_only: bool) -> (Vec<String>, bool) {
    if word == "~" {
        return (vec!["~/".to_owned()], false);
    }
    let (directory, prefix) = word.rsplit_once('/').map_or(("", word), |(d, p)| (word.get(..=d.len()).unwrap_or(""), p));
    let folder: PathBuf = match (directory, directory.strip_prefix("~/")) {
        ("", _) => PathBuf::from("."),
        (_, Some(under_home)) => home.join(under_home),
        _ => PathBuf::from(directory),
    };
    let Ok(entries) = std::fs::read_dir(folder) else { return (Vec::new(), false) };
    let named = entries.flatten().filter_map(|entry| Some((entry.file_name().to_str()?.to_owned(), entry)));
    let (matches, incomplete) = matching(named, prefix, prefix.starts_with('.'), MAX_SCANNED, MAX_MATCHES);
    let mut found = Vec::new();
    for (name, entry) in matches {
        let is_dir = match entry.file_type() {
            Ok(kind) if kind.is_symlink() => std::fs::metadata(entry.path()).is_ok_and(|m| m.is_dir()),
            Ok(kind) => kind.is_dir(),
            Err(_) => false,
        };
        if documents_only && !is_dir && !is_pdf(&name) {
            continue;
        }
        found.push(format!("{directory}{name}{}", if is_dir { "/" } else { "" }));
    }
    found.sort_by_cached_key(|c| (c.to_lowercase(), c.clone()));
    (found, incomplete)
}

/// The longest start that all the candidates share.
pub fn common_prefix(candidates: &[String]) -> String {
    let Some((first, others)) = candidates.split_first() else { return String::new() };
    let mut end = first.len();
    for other in others {
        let shared: usize = first.chars().zip(other.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum();
        end = end.min(shared);
    }
    first.get(..end).unwrap_or("").to_owned()
}

/// What to show for a candidate: the last part of a path.
fn label(candidate: &str) -> String {
    let trimmed = candidate.trim_end();
    let body = trimmed.strip_suffix('/').unwrap_or(trimmed);
    let tail = body.rsplit('/').next().unwrap_or(body);
    if trimmed.ends_with('/') { format!("{tail}/") } else { tail.to_owned() }
}

/// The message that lists the candidates: a few labels, a bounded length, and the true total.
fn listing(candidates: &[String], total: usize, incomplete: bool) -> String {
    let mut message = String::new();
    let mut length = 0;
    let mut shown = 0;
    for candidate in candidates.iter().take(MAX_LABELS) {
        let text = label(candidate);
        let size = text.chars().count();
        if shown == 0 {
            message.extend(text.chars().take(MAX_MESSAGE_CHARS));
            length = size.min(MAX_MESSAGE_CHARS);
        } else if length + 2 + size <= MAX_MESSAGE_CHARS {
            message.push_str("  ");
            message.push_str(&text);
            length += 2 + size;
        } else {
            break;
        }
        shown += 1;
    }
    let more = total.saturating_sub(shown);
    if more > 0 {
        message.push_str(&format!("  \u{2026} +{more} more"));
    }
    if incomplete {
        message.push_str(" (listing incomplete)");
    }
    message
}

/// Where a run of Tabs is: valid only while the field still reads as this run left it.
#[derive(Clone, Debug)]
pub struct Cycle {
    start: usize,
    candidates: Vec<String>,
    shown: Option<usize>,
    before: String,
    after: String,
    /// The listing was cut short, so a lone candidate is only taken by the next Tab.
    incomplete: bool,
}

/// The result of a Tab: the new text before the cursor, and a message listing the candidates.
#[derive(Debug, PartialEq, Eq)]
pub struct Applied {
    pub before: String,
    pub message: Option<String>,
}

/// One Tab (or Shift-Tab) on the text around the cursor, given the cycle in progress.
pub fn tab(cycle: &mut Option<Cycle>, before: &str, after: &str, backward: bool, themes: &[String], home: &Path) -> Option<Applied> {
    if let Some(run) =
        cycle.as_mut().filter(|c| c.before == before && c.after == after && (c.candidates.len() > 1 || (c.incomplete && !c.candidates.is_empty())))
    {
        let count = run.candidates.len();
        let next = match (run.shown, backward) {
            (None, false) => 0,
            (None, true) => count - 1,
            (Some(i), false) => (i + 1) % count,
            (Some(i), true) => (i + count - 1) % count,
        };
        let word = run.candidates.get(next)?;
        let new = format!("{}{word}", before.get(..run.start).unwrap_or(""));
        run.shown = Some(next);
        run.before.clone_from(&new);
        return Some(Applied { before: new, message: None });
    }
    *cycle = None;
    let Completion { start, mut candidates, incomplete } = complete(before, themes, home)?;
    if candidates.is_empty() {
        *cycle = None;
        return Some(Applied { before: before.to_owned(), message: Some("no matches found (listing incomplete)".to_owned()) });
    }
    let kept = before.get(..start).unwrap_or("");
    if let ([only], false) = (candidates.as_slice(), incomplete) {
        return Some(Applied { before: format!("{kept}{only}"), message: None });
    }
    // With a cut-short listing the shared start of what was seen may be too long: add nothing.
    let new = if incomplete { before.to_owned() } else { format!("{kept}{}", common_prefix(&candidates)) };
    let total = candidates.len();
    candidates.truncate(MAX_CANDIDATES);
    let message = listing(&candidates, total, incomplete);
    *cycle = Some(Cycle { start, candidates, shown: None, before: new.clone(), after: after.to_owned(), incomplete });
    Some(Applied { before: new, message: Some(message) })
}

/// Completes in the field, and returns the message to show, if any.
pub fn tab_field(field: &mut Field, cycle: &mut Option<Box<Cycle>>, backward: bool, themes: &[String]) -> Option<String> {
    let before = field.before_cursor().to_owned();
    let after = field.text.get(before.len()..).unwrap_or("").to_owned();
    let mut run = cycle.take().map(|c| *c);
    let applied = tab(&mut run, &before, &after, backward, themes, &crate::config::home());
    *cycle = run.map(Box::new);
    let applied = applied?;
    let behind = after.chars().count();
    *field = Field::new(format!("{}{after}", applied.before));
    for _ in 0..behind {
        field.apply(crate::field::Edit::Left);
    }
    applied.message
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn candidates(before: &str) -> Vec<String> {
        complete(before, &strings(&["dark", "light", "dim"]), Path::new("/nonexistent-home")).map_or_else(Vec::new, |c| c.candidates)
    }

    /// A scratch directory, removed when it goes out of scope.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("yavcpdf-complete-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("sub")).unwrap();
            for file in ["a.pdf", "B.PDF", "c.txt", ".hidden.pdf"] {
                std::fs::write(root.join(file), b"").unwrap();
            }
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
    fn the_names_are_the_ones_the_dispatcher_knows() {
        let dispatcher = include_str!("actions.rs");
        for (name, _) in COMMANDS {
            assert!(dispatcher.contains(&format!("\"{name}\"")), "{name} is not in App::command");
        }
    }

    #[test]
    fn command_names_complete() {
        assert_eq!(candidates("no"), ["noh", "nohlsearch"]);
        assert_eq!(candidates("q"), ["q", "quit", "q!"]);
        assert_eq!(candidates("  ma"), ["marks"]);
        assert!(candidates("zzz").is_empty());
        assert!(candidates("12").is_empty());
    }

    #[test]
    fn a_unique_match_is_taken_with_a_space_only_for_an_argument() {
        let themes = strings(&[]);
        let home = Path::new("/");
        let mut cycle = None;
        assert_eq!(tab(&mut cycle, "the", "", false, &themes, home).unwrap().before, "theme ");
        assert_eq!(tab(&mut cycle, "rel", "", false, &themes, home).unwrap().before, "reload");
        assert!(cycle.is_none());
    }

    #[test]
    fn several_matches_insert_the_common_prefix_and_list_them() {
        let mut cycle = None;
        let applied = tab(&mut cycle, "no", "", false, &[], Path::new("/")).unwrap();
        assert_eq!(applied.before, "noh");
        assert_eq!(applied.message.as_deref(), Some("noh  nohlsearch"));
        assert_eq!(common_prefix(&strings(&["export ", "export! "])), "export");
        assert_eq!(common_prefix(&strings(&["日本", "日本語", "日x"])), "日");
        assert_eq!(common_prefix(&[]), "");
    }

    #[test]
    fn more_tabs_cycle_forward_and_back() {
        let home = Path::new("/");
        let mut cycle = None;
        let mut text = "e".to_owned();
        let mut press = |text: &mut String, backward: bool| {
            *text = tab(&mut cycle, text, "", backward, &[], home).unwrap().before;
        };
        press(&mut text, false);
        assert_eq!(text, "e", "the common prefix is all there is");
        press(&mut text, false);
        assert_eq!(text, "e ");
        press(&mut text, false);
        assert_eq!(text, "edit ");
        press(&mut text, false);
        assert_eq!(text, "export ");
        press(&mut text, true);
        assert_eq!(text, "edit ");
        press(&mut text, true);
        press(&mut text, true);
        assert_eq!(text, "export! ", "backward from the first wraps to the last");
    }

    #[test]
    fn other_edits_end_the_cycle() {
        let home = Path::new("/");
        let mut cycle = None;
        tab(&mut cycle, "no", "", false, &[], home).unwrap();
        // Typed after the completion: a fresh completion of "nohx" finds nothing.
        assert!(tab(&mut cycle, "nohx", "", false, &[], home).is_none());
        assert!(cycle.is_none());
        // Text after the cursor is part of the run too: a fresh run lists, a cycle step does not.
        let first = tab(&mut cycle, "no", "", false, &[], home).unwrap();
        assert!(first.message.is_some());
        let applied = tab(&mut cycle, "noh", "x", false, &[], home).unwrap();
        assert_eq!(applied.before, "noh");
        assert!(applied.message.is_some(), "a fresh completion, not a step of the old cycle");
    }

    #[test]
    fn theme_names_complete_after_theme() {
        assert_eq!(candidates("theme "), ["dark", "light", "dim"]);
        assert_eq!(candidates("theme d"), ["dark", "dim"]);
        assert_eq!(candidates("theme   l"), ["light"]);
        assert_eq!(complete("theme   l", &strings(&["light"]), Path::new("/")).unwrap().start, 8);
        assert!(candidates("zoom 1").is_empty());
    }

    #[test]
    fn open_lists_directories_and_pdfs_only() {
        let scratch = Scratch::new("open");
        let dir = scratch.path();
        let found = candidates(&format!("e {dir}/"));
        assert_eq!(found, [format!("{dir}/a.pdf"), format!("{dir}/B.PDF"), format!("{dir}/sub/")]);
        assert_eq!(candidates(&format!("open {dir}/s")), [format!("{dir}/sub/")]);
    }

    #[test]
    fn export_lists_every_file_and_dotfiles_need_a_dot() {
        let scratch = Scratch::new("export");
        let dir = scratch.path();
        let all = candidates(&format!("export! {dir}/"));
        assert_eq!(all.len(), 4);
        assert!(all.contains(&format!("{dir}/c.txt")));
        assert!(!all.iter().any(|c| c.contains(".hidden")));
        assert_eq!(candidates(&format!("export {dir}/.")), [format!("{dir}/.hidden.pdf")]);
    }

    #[test]
    fn a_tilde_means_home_and_a_bad_directory_means_nothing() {
        let scratch = Scratch::new("home");
        let home = Path::new(scratch.path());
        let found = complete("edit ~/s", &[], home).unwrap();
        assert_eq!((found.start, found.candidates), (5, strings(&["~/sub/"])));
        assert_eq!(complete("edit ~", &[], home).unwrap().candidates, ["~/"]);
        assert!(complete("edit /nonexistent-dir-for-yavcpdf/x", &[], home).is_none());
    }

    #[test]
    fn the_field_keeps_what_follows_the_cursor() {
        let mut cycle = None;
        let mut field = Field::new("thex".to_owned());
        field.apply(crate::field::Edit::Left);
        assert!(tab_field(&mut field, &mut cycle, false, &[]).is_none());
        assert_eq!(field.text, "theme x");
        assert_eq!(field.before_cursor(), "theme ");
    }

    #[test]
    fn unicode_text_never_splits_a_character() {
        let home = Path::new("/");
        let mut cycle = None;
        assert!(tab(&mut cycle, "日本語", "日", false, &[], home).is_none());
        assert!(tab(&mut cycle, "theme 日", "本", false, &strings(&["日本"]), home).is_some());
    }

    #[test]
    fn text_after_the_cursor_may_be_multibyte() {
        let mut cycle = None;
        let mut field = Field::new("theme li日本".to_owned());
        for _ in 0..2 {
            field.apply(crate::field::Edit::Left);
        }
        let themes = strings(&["light"]);
        assert!(tab_field(&mut field, &mut cycle, false, &themes).is_none());
        assert_eq!(field.text, "theme light日本");
        assert_eq!(field.before_cursor(), "theme light");
    }

    #[test]
    fn the_prefix_comes_from_every_match_not_the_listed_ones() {
        let scratch = Scratch::new("many");
        let dir = scratch.path();
        for n in 0..=MAX_CANDIDATES {
            std::fs::write(format!("{dir}/ra{n}.pdf"), b"").unwrap();
        }
        std::fs::write(format!("{dir}/rb.pdf"), b"").unwrap();
        let mut cycle = None;
        let before = format!("e {dir}/r");
        let applied = tab(&mut cycle, &before, "", false, &[], Path::new("/")).unwrap();
        assert_eq!(applied.before, before, "ra and rb share only r");
        let message = applied.message.unwrap();
        assert!(message.contains("+287 more"), "{message}");
        assert!(message.chars().count() <= MAX_MESSAGE_CHARS + 30, "{message}");
        assert_eq!(cycle.unwrap().candidates.len(), MAX_CANDIDATES);
    }

    #[test]
    fn the_message_is_bounded_and_cut_at_characters() {
        let long: Vec<String> = (0..40).map(|n| format!("日本語の長い名前{n}")).collect();
        let message = listing(&long, 40, false);
        assert!(message.chars().count() <= MAX_MESSAGE_CHARS + 20, "{message}");
        assert!(message.ends_with(" more"));
        let huge = vec!["日".repeat(500)];
        assert_eq!(listing(&huge, 1, false).chars().count(), MAX_MESSAGE_CHARS);
    }

    #[test]
    fn the_cap_counts_matches_and_not_the_entries_scanned() {
        let names = |n: usize| (0..n).map(|i| (if i % 2 == 0 { format!("x{i}") } else { format!("y{i}") }, ()));
        // Only the odd-numbered ones match "x"; the matches among 1000 entries are all found.
        let (found, incomplete) = matching(names(1000), "x", false, 10_000, 600);
        assert_eq!((found.len(), incomplete), (500, false));
        // Too many matches: cut short, and said so.
        let (found, incomplete) = matching(names(1000), "x", false, 10_000, 100);
        assert_eq!((found.len(), incomplete), (100, true));
        // Too many entries scanned.
        let (found, incomplete) = matching(names(1000), "x", false, 10, 600);
        assert_eq!((found.len(), incomplete), (5, true));
        // Exactly at the cap is not cut short.
        assert!(!matching(names(10), "x", false, 10, 5).1);
    }

    #[test]
    fn a_cut_short_listing_never_completes_a_unique_match() {
        let scratch = Scratch::new("cut");
        let dir = scratch.path();
        for n in 0..MAX_SCANNED + 5 {
            std::fs::write(format!("{dir}/z{n:03}.pdf"), b"").unwrap();
        }
        let mut cycle = None;
        let before = format!("e {dir}/z00");
        let applied = tab(&mut cycle, &before, "", false, &[], Path::new("/")).unwrap();
        assert_eq!(applied.before, before);
        assert!(applied.message.unwrap().ends_with("(listing incomplete)"));
    }

    #[test]
    fn a_cut_short_listing_with_no_matches_says_so() {
        let scratch = Scratch::new("none");
        let dir = scratch.path();
        for n in 0..MAX_SCANNED + 5 {
            std::fs::write(format!("{dir}/y{n:03}.pdf"), b"").unwrap();
        }
        let mut cycle = None;
        let before = format!("e {dir}/zz");
        let applied = tab(&mut cycle, &before, "", false, &[], Path::new("/")).unwrap();
        assert_eq!(applied.before, before);
        assert_eq!(applied.message.as_deref(), Some("no matches found (listing incomplete)"));
        assert!(cycle.is_none(), "an empty cycle is never stored");
    }

    #[test]
    fn a_lone_match_of_a_cut_short_listing_is_taken_by_the_next_tab() {
        // The directory is not read: the stored cycle alone decides, so the second Tab inserts.
        let before = "e /nonexistent-dir-for-yavcpdf/z";
        let mut cycle = Some(Cycle {
            start: 2,
            candidates: strings(&["/nonexistent-dir-for-yavcpdf/z1.pdf"]),
            shown: None,
            before: before.to_owned(),
            after: String::new(),
            incomplete: true,
        });
        let applied = tab(&mut cycle, before, "", false, &[], Path::new("/")).unwrap();
        assert_eq!(applied.before, "e /nonexistent-dir-for-yavcpdf/z1.pdf");
        assert!(applied.message.is_none());
    }

    fn tab_once(before: &str, backward: bool) -> Option<Applied> {
        tab(&mut None, before, "", backward, &strings(&["dark", "light"]), Path::new("/nonexistent-home"))
    }

    #[test]
    fn only_spaces_offer_every_command_at_the_end() {
        let found = complete("   ", &[], Path::new("/")).unwrap();
        assert_eq!((found.start, found.candidates.len(), found.incomplete), (3, COMMANDS.len(), false));
        let applied = tab_once("   ", false).unwrap();
        assert_eq!(applied.before, "   ", "nothing is shared by every command");
        assert!(applied.message.is_some());
    }

    #[test]
    fn leading_whitespace_is_kept_and_start_skips_it() {
        assert_eq!(complete("  th", &[], Path::new("/")).unwrap().start, 2);
        assert_eq!(tab_once("  th", false).unwrap().before, "  theme ");
        assert_eq!(tab_once("\trel", false).unwrap().before, "\treload");
        assert_eq!(complete("  theme d", &strings(&["dark"]), Path::new("/")).unwrap().start, 8);
    }

    #[test]
    fn a_page_number_has_no_completion() {
        assert!(tab_once("12", false).is_none());
        assert!(tab_once("12", true).is_none());
        assert!(tab_once("1", false).is_none());
    }

    #[test]
    fn a_name_that_prefixes_others_lists_itself_first() {
        assert_eq!(candidates("e"), ["e ", "edit ", "export ", "export! "]);
        assert_eq!(candidates("export"), ["export ", "export! "]);
        assert_eq!(candidates("export!"), ["export! "]);
        assert_eq!(candidates("edit"), ["edit "]);
        assert_eq!(candidates("w"), ["wq", "w", "write"]);
        assert_eq!(tab_once("export", false).unwrap().before, "export");
        assert_eq!(tab_once("export!", false).unwrap().before, "export! ");
        // An exact full name that is not a prefix of anything else is taken as it is.
        assert_eq!(tab_once("edit", false).unwrap().before, "edit ");
    }

    #[test]
    fn theme_with_nothing_matching_gives_nothing() {
        assert!(candidates("theme zz").is_empty());
        assert!(tab_once("theme zz", false).is_none());
        assert!(complete("theme ", &[], Path::new("/")).is_none());
        assert_eq!(tab_once("theme li", false).unwrap().before, "theme light");
        // Theme names are matched case-sensitively as typed.
        assert!(candidates("theme D").is_empty());
    }

    #[test]
    fn a_path_ending_in_a_slash_lists_that_directory() {
        let scratch = Scratch::new("slash");
        let dir = scratch.path();
        std::fs::write(format!("{dir}/sub/inner.pdf"), b"").unwrap();
        assert_eq!(candidates(&format!("e {dir}/sub/")), [format!("{dir}/sub/inner.pdf")]);
        let before = format!("e {dir}/sub/");
        assert_eq!(tab_once(&before, false).unwrap().before, format!("e {dir}/sub/inner.pdf"));
        // An empty directory has nothing to offer.
        std::fs::create_dir(format!("{dir}/empty")).unwrap();
        assert!(candidates(&format!("export {dir}/empty/")).is_empty());
    }

    #[test]
    fn a_missing_directory_gives_nothing_and_leaves_no_cycle() {
        let mut cycle = None;
        assert!(tab(&mut cycle, "e /nonexistent-dir-for-yavcpdf/", "", false, &[], Path::new("/")).is_none());
        assert!(cycle.is_none());
        assert!(candidates("export /nonexistent-dir-for-yavcpdf/a/").is_empty());
        let scratch = Scratch::new("notdir");
        // A file used as a directory is unreadable too.
        assert!(candidates(&format!("export {}/c.txt/", scratch.path())).is_empty());
    }

    #[test]
    fn file_names_with_spaces_complete_whole() {
        let scratch = Scratch::new("spaces");
        let dir = scratch.path();
        std::fs::write(format!("{dir}/with space.pdf"), b"").unwrap();
        std::fs::write(format!("{dir}/with spaces too.pdf"), b"").unwrap();
        assert_eq!(candidates(&format!("e {dir}/with sp")), [format!("{dir}/with space.pdf"), format!("{dir}/with spaces too.pdf")]);
        let applied = tab_once(&format!("e {dir}/with spaces t"), false).unwrap();
        assert_eq!(applied.before, format!("e {dir}/with spaces too.pdf"));
        // The common prefix runs through the space.
        let applied = tab_once(&format!("e {dir}/wi"), false).unwrap();
        assert_eq!(applied.before, format!("e {dir}/with space"));
    }

    #[test]
    fn multibyte_file_names_complete_at_character_boundaries() {
        let scratch = Scratch::new("utf8");
        let dir = scratch.path();
        for name in ["日本語.pdf", "日本酒.pdf", "é.pdf"] {
            std::fs::write(format!("{dir}/{name}"), b"").unwrap();
        }
        assert_eq!(tab_once(&format!("e {dir}/é"), false).unwrap().before, format!("e {dir}/é.pdf"));
        let applied = tab_once(&format!("e {dir}/日"), false).unwrap();
        assert_eq!(applied.before, format!("e {dir}/日本"), "the shared start stops before the differing character");
        let message = applied.message.unwrap();
        assert_eq!(message, "日本語.pdf  日本酒.pdf");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_to_a_directory_is_a_directory() {
        let scratch = Scratch::new("link");
        let dir = scratch.path();
        std::os::unix::fs::symlink(format!("{dir}/sub"), format!("{dir}/linkdir")).unwrap();
        std::os::unix::fs::symlink(format!("{dir}/a.pdf"), format!("{dir}/linkfile.pdf")).unwrap();
        assert_eq!(candidates(&format!("e {dir}/linkd")), [format!("{dir}/linkdir/")]);
        assert_eq!(candidates(&format!("export {dir}/linkd")), [format!("{dir}/linkdir/")]);
        assert_eq!(candidates(&format!("e {dir}/linkf")), [format!("{dir}/linkfile.pdf")]);
    }

    #[cfg(unix)]
    #[test]
    fn a_broken_symlink_is_a_file_and_never_a_crash() {
        let scratch = Scratch::new("broken");
        let dir = scratch.path();
        std::os::unix::fs::symlink(format!("{dir}/missing"), format!("{dir}/dangling")).unwrap();
        std::os::unix::fs::symlink(format!("{dir}/missing"), format!("{dir}/dangling.pdf")).unwrap();
        // Not a directory, and not a .pdf by name: hidden from open, shown (without a slash) by export.
        assert!(candidates(&format!("e {dir}/dangl")) == [format!("{dir}/dangling.pdf")]);
        assert_eq!(candidates(&format!("export {dir}/dangling")), [format!("{dir}/dangling"), format!("{dir}/dangling.pdf")]);
        assert!(candidates(&format!("e {dir}/dangling.pdf/")).is_empty());
    }

    #[test]
    fn shift_tab_as_the_first_tab_completes_like_tab_then_goes_back() {
        let home = Path::new("/");
        let mut cycle = None;
        let first = tab(&mut cycle, "e", "", true, &[], home).unwrap();
        assert_eq!(first.before, "e");
        assert!(first.message.is_some(), "a first Shift-Tab lists, it does not step");
        let second = tab(&mut cycle, "e", "", true, &[], home).unwrap();
        assert_eq!(second.before, "export! ", "the next Shift-Tab goes to the last");
        assert!(second.message.is_none());
        // And a unique match with Shift-Tab first is simply taken.
        assert_eq!(tab_once("rel", true).unwrap().before, "reload");
    }

    #[test]
    fn cycling_wraps_in_both_directions() {
        let home = Path::new("/");
        let all = ["e ", "edit ", "export ", "export! "];
        let run = |directions: &[bool]| {
            let mut cycle = None;
            let mut text = "e".to_owned();
            // The first Tab only lists.
            text = tab(&mut cycle, &text, "", false, &[], home).unwrap().before;
            let mut seen = Vec::new();
            for backward in directions {
                text = tab(&mut cycle, &text, "", *backward, &[], home).unwrap().before;
                seen.push(text.clone());
            }
            seen
        };
        assert_eq!(run(&[false; 6]), [all[0], all[1], all[2], all[3], all[0], all[1]]);
        assert_eq!(run(&[true; 6]), [all[3], all[2], all[1], all[0], all[3], all[2]]);
        assert_eq!(run(&[false, true, true]), [all[0], all[3], all[2]]);
    }

    #[test]
    fn editing_the_text_mid_cycle_restarts_it() {
        let home = Path::new("/");
        let mut cycle = None;
        tab(&mut cycle, "e", "", false, &[], home).unwrap();
        let stepped = tab(&mut cycle, "e", "", false, &[], home).unwrap();
        assert_eq!(stepped.before, "e ");
        // The user types "x" after the completion: a fresh completion, which finds nothing.
        assert!(tab(&mut cycle, "e x", "", false, &[], home).is_none());
        assert!(cycle.is_none());
    }
}
