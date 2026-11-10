//! Candidates for the argument of a `:` command: pure functions from the text before the cursor,
//! and the listing of a directory that is read once and filtered in memory as the word grows.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// What a command's argument is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Of {
    Theme,
    /// A directory or a PDF.
    Document,
    /// Any file or directory.
    Path,
    /// A number, which has nothing to complete.
    Number,
    /// Free text, which has nothing to complete.
    Name,
}

/// The argument of a command: what it is made of, how the menu shows it, and whether the command
/// is useless without it.
#[derive(Clone, Copy, Debug)]
pub struct Argument {
    pub of: Of,
    pub hint: &'static str,
    pub required: bool,
}

#[derive(Debug)]
pub struct Command {
    pub name: &'static str,
    pub argument: Option<Argument>,
}

const fn plain(name: &'static str) -> Command {
    Command { name, argument: None }
}

const fn takes(name: &'static str, of: Of, hint: &'static str, required: bool) -> Command {
    Command { name, argument: Some(Argument { of, hint, required }) }
}

/// The commands, and the argument each takes. This must match `App::command` in actions.rs;
/// `the_names_are_the_ones_the_dispatcher_knows` checks that each is mentioned there.
pub const COMMANDS: &[Command] = &[
    plain("q"),
    plain("quit"),
    plain("q!"),
    plain("wq"),
    plain("x"),
    plain("w"),
    plain("write"),
    takes("e", Of::Document, "{file}", true),
    takes("edit", Of::Document, "{file}", true),
    takes("open", Of::Document, "[file]", false),
    plain("noh"),
    plain("nohlsearch"),
    takes("theme", Of::Theme, "{name}", true),
    takes("zoom", Of::Number, "{percent}", true),
    plain("reload"),
    takes("rotate", Of::Name, "[90|180|270|0|cw|ccw]", false),
    takes("twopage", Of::Name, "[on|off]", false),
    takes("cover", Of::Name, "[on|off]", false),
    plain("toc"),
    plain("recent"),
    takes("bookmark", Of::Name, "{name}", true),
    takes("bm", Of::Name, "{name}", true),
    plain("bookmarks"),
    plain("hotbar"),
    plain("present"),
    takes("export", Of::Path, "[file]", false),
    takes("export!", Of::Path, "[file]", false),
    takes("protect", Of::Path, "[file]", false),
    takes("protect!", Of::Path, "[file]", false),
    takes("unprotect", Of::Path, "[file]", false),
    takes("unprotect!", Of::Path, "[file]", false),
    plain("clearform"),
    plain("sign"),
    plain("marks"),
];

/// The argument that the command of this name takes, if it takes one.
pub fn argument_of(name: &str) -> Option<Argument> {
    COMMANDS.iter().find(|c| c.name == name)?.argument
}

/// How many directory entries are read, and how many matches are kept. Hitting either makes the
/// listing incomplete.
#[cfg(not(test))]
const MAX_SCANNED: usize = 100_000;
#[cfg(test)]
const MAX_SCANNED: usize = 500;
#[cfg(not(test))]
const MAX_MATCHES: usize = 5000;
#[cfg(test)]
const MAX_MATCHES: usize = 400;
const _: () = assert!(MAX_MATCHES < MAX_SCANNED);

/// The names in one directory, read once: typing more of a name filters this in memory instead of
/// reading the directory again. It is kept for the folder it was read from.
#[derive(Clone, Debug, Default)]
pub struct DirCache {
    folder: Option<PathBuf>,
    /// Each name and what it is, without following symlinks.
    items: Vec<(String, Node)>,
    /// What the symlinks looked at so far point to: whether it is a directory. Followed lazily,
    /// only for names that match the word, so a big folder is not statted whole.
    links: HashMap<String, bool>,
    /// The directory had more than `MAX_SCANNED` names.
    cut: bool,
}

/// What a directory entry is, before any symlink is followed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Node {
    Dir,
    File,
    Link,
}

impl DirCache {
    /// Reads `folder` unless it is the one already held. An unreadable folder holds nothing.
    fn load(&mut self, folder: PathBuf) {
        if self.folder.as_ref() == Some(&folder) {
            return;
        }
        self.items.clear();
        self.links.clear();
        self.cut = false;
        if let Ok(entries) = std::fs::read_dir(&folder) {
            for entry in entries.flatten() {
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
                if self.items.len() >= MAX_SCANNED {
                    self.cut = true;
                    break;
                }
                let node = match entry.file_type() {
                    Ok(kind) if kind.is_symlink() => Node::Link,
                    Ok(kind) if kind.is_dir() => Node::Dir,
                    _ => Node::File,
                };
                self.items.push((name, node));
            }
        }
        self.folder = Some(folder);
    }
}

/// The ways to finish the word that starts at byte `start` of the text.
#[derive(Debug, PartialEq, Eq)]
pub struct Completion {
    pub start: usize,
    /// Whole replacements for the word, each with its trailing slash.
    pub candidates: Vec<String>,
    /// The directory listing was cut short, so `candidates` may miss some matches.
    pub incomplete: bool,
}

/// The candidates for the argument in the text before the cursor, if there are any. A text with
/// no space yet is a command name, which the menu lists itself.
pub fn complete(before: &str, themes: &[String], home: &Path, cache: &mut DirCache) -> Option<Completion> {
    let lead = before.len() - before.trim_start().len();
    let rest = before.get(lead..)?;
    let end = rest.find(char::is_whitespace)?;
    let argument = rest.get(end..)?;
    let offset = end + (argument.len() - argument.trim_start().len());
    let word = rest.get(offset..)?;
    let (candidates, incomplete) = match argument_of(rest.get(..end)?)?.of {
        Of::Theme => (themes.iter().filter(|t| t.starts_with(word)).cloned().collect(), false),
        Of::Document => paths(word, home, true, cache),
        Of::Path => paths(word, home, false, cache),
        Of::Number | Of::Name => return None,
    };
    // A cut-short listing with nothing seen is still an answer: the menu says so instead of staying silent.
    (!candidates.is_empty() || incomplete).then_some(Completion { start: lead + offset, candidates, incomplete })
}

fn is_pdf(name: &str) -> bool {
    Path::new(name).extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// The items whose name continues `prefix` (dotfiles only when the prefix starts with a dot),
/// that `keep` accepts, and whether the match cap cut the list short. Only the items kept count
/// toward `max_matches`.
fn matching<N: AsRef<str>, T>(
    items: impl Iterator<Item = (N, T)>,
    prefix: &str,
    show_hidden: bool,
    max_matches: usize,
    mut keep: impl FnMut(&str, &T) -> bool,
) -> (Vec<(N, T)>, bool) {
    let mut kept = Vec::new();
    for (name, item) in items {
        let text = name.as_ref();
        if !text.starts_with(prefix) || (text.starts_with('.') && !show_hidden) || !keep(text, &item) {
            continue;
        }
        if kept.len() >= max_matches {
            return (kept, true);
        }
        kept.push((name, item));
    }
    (kept, false)
}

/// Whether `c` separates folders in a typed path: `/` everywhere, and `\` too on Windows.
fn is_separator(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

#[cfg(windows)]
fn strip_home_backslash(directory: &str) -> Option<&str> {
    directory.strip_prefix("~\\")
}

#[cfg(not(windows))]
fn strip_home_backslash(_directory: &str) -> Option<&str> {
    None
}

/// The files and directories that continue `word`, and whether the listing is incomplete.
/// Directories end in a slash. Unreadable directories give nothing.
fn paths(word: &str, home: &Path, documents_only: bool, cache: &mut DirCache) -> (Vec<String>, bool) {
    if word == "~" {
        return (vec!["~/".to_owned()], false);
    }
    let (directory, prefix) = word.rfind(is_separator).map_or(("", word), |i| (word.get(..=i).unwrap_or(""), word.get(i + 1..).unwrap_or("")));
    let folder: PathBuf = match (directory, directory.strip_prefix("~/").or_else(|| strip_home_backslash(directory))) {
        ("", _) => PathBuf::from("."),
        (_, Some(under_home)) => home.join(under_home),
        _ => PathBuf::from(directory),
    };
    cache.load(folder);
    let DirCache { folder, items, links, .. } = &mut *cache;
    let folder = folder.clone().unwrap_or_default();
    // A symlink is a directory if it points to one; a broken one counts as a file.
    let mut dir_of = |name: &str, node: Node| match node {
        Node::Dir => true,
        Node::File => false,
        Node::Link => *links.entry(name.to_owned()).or_insert_with(|| std::fs::metadata(folder.join(name)).is_ok_and(|m| m.is_dir())),
    };
    let named = items.iter().map(|(name, node)| (name.as_str(), *node));
    let wanted = |name: &str, node: &Node| !documents_only || is_pdf(name) || dir_of(name, *node);
    let (matches, capped) = matching(named, prefix, prefix.starts_with('.'), MAX_MATCHES, wanted);
    let mut found = Vec::new();
    for (name, node) in matches {
        let is_dir = dir_of(name, node);
        found.push(format!("{directory}{name}{}", if is_dir { "/" } else { "" }));
    }
    found.sort_by_cached_key(|c| (c.to_lowercase(), c.clone()));
    (found, capped || cache.cut)
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
pub fn label(candidate: &str) -> String {
    let trimmed = candidate.trim_end();
    let ends_in_separator = trimmed.ends_with(is_separator);
    let body = trimmed.strip_suffix(is_separator).unwrap_or(trimmed);
    let tail = body.rsplit(is_separator).next().unwrap_or(body);
    if ends_in_separator { format!("{tail}/") } else { tail.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn done(before: &str, themes: &[String], home: &Path) -> Option<Completion> {
        complete(before, themes, home, &mut DirCache::default())
    }

    fn candidates(before: &str) -> Vec<String> {
        done(before, &strings(&["dark", "light", "dim"]), Path::new("/nonexistent-home")).map_or_else(Vec::new, |c| c.candidates)
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
        for command in COMMANDS {
            assert!(dispatcher.contains(&format!("\"{}\"", command.name)), "{} is not in App::command", command.name);
        }
    }

    #[test]
    fn the_table_says_what_each_argument_is() {
        let hint = |name: &str| argument_of(name).map(|a| (a.of, a.hint, a.required));
        assert_eq!(hint("e"), Some((Of::Document, "{file}", true)));
        assert_eq!(hint("open"), Some((Of::Document, "[file]", false)));
        assert_eq!(hint("export!"), Some((Of::Path, "[file]", false)));
        assert_eq!(hint("theme"), Some((Of::Theme, "{name}", true)));
        assert_eq!(hint("zoom"), Some((Of::Number, "{percent}", true)));
        assert_eq!(hint("w"), None);
        assert_eq!(hint("nonsense"), None);
    }

    #[test]
    fn a_name_with_no_space_has_no_argument_to_complete() {
        assert!(candidates("the").is_empty());
        assert!(candidates("12").is_empty());
        assert!(candidates("").is_empty());
    }

    #[test]
    fn theme_names_complete_after_theme() {
        assert_eq!(candidates("theme "), ["dark", "light", "dim"]);
        assert_eq!(candidates("theme d"), ["dark", "dim"]);
        assert_eq!(candidates("theme   l"), ["light"]);
        assert_eq!(done("theme   l", &strings(&["light"]), Path::new("/")).unwrap().start, 8);
        assert_eq!(done("  theme d", &strings(&["dark"]), Path::new("/")).unwrap().start, 8);
        assert!(candidates("zoom 1").is_empty());
        assert!(candidates("theme zz").is_empty());
        assert!(done("theme ", &[], Path::new("/")).is_none());
        // Theme names are matched case-sensitively as typed.
        assert!(candidates("theme D").is_empty());
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
        let found = done("edit ~/s", &[], home).unwrap();
        assert_eq!((found.start, found.candidates), (5, strings(&["~/sub/"])));
        assert_eq!(done("edit ~", &[], home).unwrap().candidates, ["~/"]);
        assert!(done("edit /nonexistent-dir-for-yavcpdf/x", &[], home).is_none());
    }

    #[test]
    fn the_directory_is_read_once_and_again_only_when_it_changes() {
        let scratch = Scratch::new("cache");
        let dir = scratch.path();
        let mut cache = DirCache::default();
        let names = |cache: &mut DirCache, word: &str| complete(&format!("export {dir}/{word}"), &[], Path::new("/"), cache).map(|c| c.candidates);
        assert_eq!(names(&mut cache, "a").unwrap(), [format!("{dir}/a.pdf")]);
        // A file made now is not seen while the folder is the same: the listing is not read again.
        std::fs::write(format!("{dir}/ab.pdf"), b"").unwrap();
        assert_eq!(names(&mut cache, "a").unwrap(), [format!("{dir}/a.pdf")]);
        assert_eq!(names(&mut cache, "").unwrap().len(), 4);
        // Another folder reads again; coming back reads once more and so finds the new file.
        std::fs::write(format!("{dir}/sub/z.pdf"), b"").unwrap();
        assert_eq!(complete(&format!("export {dir}/sub/"), &[], Path::new("/"), &mut cache).unwrap().candidates, [format!("{dir}/sub/z.pdf")]);
        assert_eq!(names(&mut cache, "a").unwrap(), [format!("{dir}/a.pdf"), format!("{dir}/ab.pdf")]);
    }

    #[test]
    fn non_documents_do_not_count_toward_the_cap_of_an_e_listing() {
        let scratch = Scratch::new("noncap");
        let dir = scratch.path();
        for i in 0..MAX_MATCHES + 50 {
            std::fs::write(format!("{dir}/n{i:04}.txt"), b"").unwrap();
        }
        // Before the cap was applied to the matches alone, the many .txt files hid the PDFs.
        let found = done(&format!("e {dir}/"), &[], Path::new("/")).unwrap();
        assert_eq!(found.candidates, [format!("{dir}/a.pdf"), format!("{dir}/B.PDF"), format!("{dir}/sub/")]);
    }

    #[test]
    fn the_cap_counts_matches_and_not_the_entries_scanned() {
        let names = |n: usize| (0..n).map(|i| (if i % 2 == 0 { format!("x{i}") } else { format!("y{i}") }, ()));
        // Only the even-numbered ones match "x"; the matches among 1000 entries are all found.
        let (found, incomplete) = matching(names(1000), "x", false, 600, |_, _| true);
        assert_eq!((found.len(), incomplete), (500, false));
        // Too many matches: cut short, and said so.
        let (found, incomplete) = matching(names(1000), "x", false, 100, |_, _| true);
        assert_eq!((found.len(), incomplete), (100, true));
        // Exactly at the cap is not cut short.
        assert!(!matching(names(10), "x", false, 5, |_, _| true).1);
    }

    #[test]
    fn a_cut_short_listing_says_so() {
        let scratch = Scratch::new("cut");
        let dir = scratch.path();
        for n in 0..MAX_SCANNED + 5 {
            std::fs::write(format!("{dir}/z{n:03}.pdf"), b"").unwrap();
        }
        let found = done(&format!("e {dir}/z00"), &[], Path::new("/")).unwrap();
        assert!(found.incomplete && !found.candidates.is_empty());
    }

    #[test]
    fn a_cut_short_listing_with_no_matches_is_still_an_answer() {
        let scratch = Scratch::new("none");
        let dir = scratch.path();
        for n in 0..MAX_SCANNED + 5 {
            std::fs::write(format!("{dir}/y{n:03}.pdf"), b"").unwrap();
        }
        let found = done(&format!("e {dir}/zz"), &[], Path::new("/")).unwrap();
        assert!(found.incomplete && found.candidates.is_empty());
    }

    #[test]
    fn too_many_matches_make_the_listing_incomplete() {
        let scratch = Scratch::new("matches");
        let dir = scratch.path();
        for n in 0..=MAX_MATCHES {
            std::fs::write(format!("{dir}/m{n:03}.pdf"), b"").unwrap();
        }
        let found = done(&format!("e {dir}/m"), &[], Path::new("/")).unwrap();
        assert_eq!((found.candidates.len(), found.incomplete), (MAX_MATCHES, true));
    }

    #[test]
    fn a_path_ending_in_a_slash_lists_that_directory() {
        let scratch = Scratch::new("slash");
        let dir = scratch.path();
        std::fs::write(format!("{dir}/sub/inner.pdf"), b"").unwrap();
        assert_eq!(candidates(&format!("e {dir}/sub/")), [format!("{dir}/sub/inner.pdf")]);
        // An empty directory has nothing to offer.
        std::fs::create_dir(format!("{dir}/empty")).unwrap();
        assert!(candidates(&format!("export {dir}/empty/")).is_empty());
    }

    #[test]
    fn a_missing_directory_gives_nothing() {
        assert!(done("e /nonexistent-dir-for-yavcpdf/", &[], Path::new("/")).is_none());
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
        assert_eq!(candidates(&format!("e {dir}/with spaces t")), [format!("{dir}/with spaces too.pdf")]);
    }

    #[test]
    fn multibyte_file_names_complete_at_character_boundaries() {
        let scratch = Scratch::new("utf8");
        let dir = scratch.path();
        for name in ["日本語.pdf", "日本酒.pdf", "é.pdf"] {
            std::fs::write(format!("{dir}/{name}"), b"").unwrap();
        }
        assert_eq!(candidates(&format!("e {dir}/é")), [format!("{dir}/é.pdf")]);
        assert_eq!(candidates(&format!("e {dir}/日")), [format!("{dir}/日本語.pdf"), format!("{dir}/日本酒.pdf")]);
        assert_eq!(done("theme 日", &strings(&["日本"]), Path::new("/")).unwrap().candidates, ["日本"]);
    }

    #[test]
    fn the_common_prefix_stops_at_a_character() {
        assert_eq!(common_prefix(&strings(&["export ", "export! "])), "export");
        assert_eq!(common_prefix(&strings(&["日本", "日本語", "日x"])), "日");
        assert_eq!(common_prefix(&strings(&["日本語.pdf", "日本酒.pdf"])), "日本");
        assert_eq!(common_prefix(&[]), "");
    }

    #[test]
    fn a_label_is_the_last_part_of_a_path() {
        assert_eq!(label("/a/b/c.pdf"), "c.pdf");
        assert_eq!(label("/a/b/sub/"), "sub/");
        assert_eq!(label("dark"), "dark");
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
}

#[cfg(all(test, not(windows)))]
mod port_tests {
    use super::*;

    #[test]
    fn a_backslash_is_not_a_separator_off_windows() {
        assert!(is_separator('/'));
        assert!(!is_separator('\\'));
        assert_eq!(strip_home_backslash("~\\"), None);
        assert_eq!(strip_home_backslash("~\\sub\\"), None);
    }

    #[test]
    fn backslashes_in_names_stay_literal() {
        let root = std::env::temp_dir().join(format!("yavcpdf-bs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("d\\e")).unwrap();
        std::fs::write(root.join("a\\b.pdf"), b"").unwrap();
        std::fs::write(root.join("a.pdf"), b"").unwrap();
        let dir = root.to_str().unwrap();
        let go = |line: &str| complete(line, &[], Path::new("/nonexistent-home"), &mut DirCache::default()).map_or_else(Vec::new, |c| c.candidates);
        assert_eq!(go(&format!("e {dir}/a\\")), [format!("{dir}/a\\b.pdf")]);
        assert_eq!(go(&format!("e {dir}/a")), [format!("{dir}/a.pdf"), format!("{dir}/a\\b.pdf")]);
        assert_eq!(go(&format!("e {dir}/d\\")), [format!("{dir}/d\\e/")]);
        // `~\` is not home off Windows: it is a name in the current folder, and nothing matches.
        assert!(go("e ~\\").is_empty());
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(label("/x/a\\b.pdf"), "a\\b.pdf");
        assert_eq!(label("/x/d\\e/"), "d\\e/");
        assert_eq!(label("C:\\x\\y"), "C:\\x\\y");
        assert_eq!(common_prefix(&["a\\b".to_owned(), "a\\c".to_owned()]), "a\\");
    }
}
