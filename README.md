# yavcpdf

A minimal, keyboard-driven PDF viewer. The whole window is the document, with a status line at the
bottom. It uses PdfCraft's render engine, and its annotation writer for `:export`, and nothing else
from the PdfCraft app.

    cargo build --release
    target/release/yavcpdf [--page N] [--password PW] [--geometry WxH[+X+Y]]
                         [--fullscreen|--no-fullscreen] [--statusbar|--no-statusbar] [file.pdf]

With no file it reopens the last document. It reloads the PDF when the file changes.

`--geometry 1000x700` sets the window's inner size in points, and `--geometry 1000x700+40+30` also
sets its position. Width and height are 1 to 16384 and the offsets are 0 to 16384 (no negative offsets: X11's offsets from
the right or bottom edge are not supported). On Wayland the compositor decides where a window goes,
so `+X+Y` may be ignored. `--fullscreen` / `--no-fullscreen` override `view.fullscreen` and
`--statusbar` / `--no-statusbar` override `statusbar.visible` for this launch; the last one given wins.
`view.fullscreen` and `statusbar.visible` only take effect at startup; a config reload does not
re-apply them. The window is limited to the monitor's size.

## Keys

Every binding can be changed under `[keys]` in the config. A count works before most motions:
`10j`, `42G`.

| Keys | Action |
| --- | --- |
| `h` `j` `k` `l` | scroll |
| `Ctrl-d` `Ctrl-u` | half a screen |
| `Ctrl-f` `Ctrl-b` | a screen |
| `J` `K` | next / previous page |
| `gg` `G` `{n}G` | first page, end, page n |
| `0` `$` `zz` | left edge, right edge, centre |
| `=` `-` `W` `H` | zoom in, zoom out, fit width, fit height |
| `/` `?` `n` `N` | search forward, backward, next, previous |
| `m{x}` `'{x}` `''` | set mark, go to mark, jump back |
| `Alt-o` `Alt-i` | jump history |
| `f` `y` | hint select, copy the selection |
| `F` | follow a link by hint |
| `gd` `Ctrl-]` | smart jump by hint; middle click does it with the mouse |
| `go` `Alt-c` | overview by hint; right click does it with the mouse |
| `Tab` | table of contents (`j` `k`, `/` filters, `Enter`); see below for a PDF with no bookmarks |
| `p` `gp` `dp` `P` `F12` | make, go to, delete and edit a portal; portal window |
| `tt` | shows or hides the hotbar, also while a tool is in hand (the tool stays in hand) |
| `th` `td` `te` `tn` | highlight, draw, erase, sticky note tool (shows the hotbar) |
| `tf` | fill & sign tool |
| `tT` `tD` `tC` `tX` `tO` `tL` `tS` | fill & sign: text, date, check, cross, dot, line, signature |
| `1`-`5` `[` `]` | with a tool: colour, thinner, thicker (fill & sign: smaller, larger) |
| `dh` | delete an annotation by hint |
| `u` `U` | undo, redo |
| `Ctrl-r` `T` | light or dark theme, next theme |
| `gl` `Ctrl-s` `Alt-f` | tint on links, status line, fullscreen |
| `Ctrl-t` | open a file with the desktop's file chooser |
| `R` `:` `q` | reload, command line, quit |
| `Esc` `Ctrl-c` | cancel: closes a bar, hints or the contents; otherwise takes back the last thing: keys, overview, jump, portal, selection, tool, search. Rebind it as `cancel` |

**Hints.** `f` labels every word showing. Type a label for the first word, then one for the last
word, or `Enter` for just the first. With the highlight tool on, the selection is highlighted
directly; without it, `y` copies it.

**Smart jump.** `gd` labels the references on the screen, whether or not the PDF made them links:
`Figure 3`, `Table 2`, `Section 3.1`, `Theorem 1.2`, `Eq. (4)`, `(4)`, `[12]`, `Smith et al., 2020`.
Pick one to go to the caption, heading, equation or bibliography entry it names, by reading the text
of the whole document. It is a good guess rather than a certainty: a reference split over two lines
isn't seen, and a scanned document has no text to read. `Alt-o` comes back.

**Overview.** Shows what a reference points at in a window over the page, so you can read a figure
or a citation without losing your place. It finds what smart jump finds. While it is open, `j` `k`
and the other scrolling keys move it, as does the wheel over it. `Enter` or a click on it goes
there, `Ctrl-Alt-PageDown` and `Ctrl-Alt-PageUp` step through other candidates for the reference,
and `Esc`, `Ctrl-c`, a click beside it or any other key closes it. `[overview]` sets its size and
place.

**Table of contents.** It comes from the PDF's bookmarks. A PDF with none, such as a Google Docs
export, gets one from its printed contents pages when their entries are links: the titles are the
text of those links, and their indents are the levels. A PDF with neither has no table of contents.

**Editing text.** The `/` `?` `:` bars, the note and text bars and the contents filter have a cursor.
`Left` `Right` move it, `Ctrl-Left` `Ctrl-Right` or `Alt-b` `Alt-f` by word, `Home` `End` or `Ctrl-a`
`Ctrl-e` to the ends. `Ctrl-Backspace` and `Ctrl-Delete` erase a word either side, `Ctrl-w` erases
back to the last space, and `Ctrl-u` `Ctrl-k` erase to either end. The `edit_*` actions rebind them.

**Sticky notes.** With `tn`, click a place and type the note in the bar. Its text shows when the
pointer is on it, and all of them show while the tool is in hand. Click a note to edit it.

**Fill & sign.** Pick an item and click where it goes, or press `f` and pick a word to put it just
after that word. The first signature asks you to draw one on a pad and is kept for every document
in `~/.local/share/yavcpdf/signature.json`, readable by you alone; `:sign` draws a new one. Move
something by undoing it and placing it again.

**Portals.** Press `p` where you are reading, go to the target, press `p` again. The view returns,
and the portal window shows the target of the portal nearest the view.

**Opening files.** `Ctrl-t` or `:open` shows the desktop's file chooser, through the
[XDG portal](https://flatpak.github.io/xdg-desktop-portal/) when there is one, so a portal backend
such as `xdg-desktop-portal-termfilechooser` gets used. It says "no file chosen" if the chooser is
cancelled, and also if none could be shown.

**Commands.** `:{page}`, `:q`, `:w`, `:e {file}`, `:open [file]`, `:noh`, `:theme {name}`, `:zoom {percent}`,
`:reload`, `:toc`, `:hotbar`, `:marks`, `:sign`, `:export [file]`, `:export! [file]`.
`Tab` completes command names, theme names and file paths; more `Tab`s cycle through the candidates, and
`Shift-Tab` goes back. `:e` and `:open` list only folders and PDFs. Tab in the `:` bar is fixed, not set under `[keys]`.

## Configuration

`~/.config/yavcpdf/config.toml`, reloaded while the viewer runs. Put only what you want to change
in it: [`src/default_config.toml`](src/default_config.toml) holds every setting with its default.
Colours are in the themes, and one key is enough to change one:

    [themes.dark]
    background = "#000000"    # around and between the pages
    link = "#0000ff55"        # links to places in the document

## Engine

The viewer uses PdfCraft's engine crates (https://github.com/storytold/pdfcraft), pinned to a commit
in `Cargo.toml`. To update, run `scripts/update-pdfcraft.sh [rev]`; without a rev it takes the
current `main`. It changes every pdfcraft rev in `Cargo.toml` together, compares `[patch.crates-io]`
with PdfCraft's, and runs the tests and clippy. If it reports a difference in the patch list, change
`[patch.crates-io]` to match upstream's and run it again.

## Install

    install -Dm755 target/release/yavcpdf ~/.local/bin/yavcpdf
    install -Dm644 yavcpdf.desktop ~/.local/share/applications/yavcpdf.desktop
    xdg-mime default yavcpdf.desktop application/pdf    # to make it the default

### Upgrading from yapdf

If the old `yapdf` binary and `yapdf.desktop` stay installed as the PDF handler, yapdf 0.2 can
recreate `~/.local/share/yapdf` after yavcpdf has moved it, and your data ends up in two folders.
Remove them and make yavcpdf the handler:

    rm ~/.local/bin/yapdf ~/.local/share/applications/yapdf.desktop
    xdg-mime default yavcpdf.desktop application/pdf

## What is stored where

Position, marks, portals and annotations are kept in `~/.local/share/yavcpdf/{hash}.json`. The PDF
itself is never changed, so other viewers don't show them. A file keeps its data when it moves, and
when it is rebuilt at the same path.

This program was called yapdf before 0.3 and pdfviewer before 0.2. On its first run it moves the old `~/.config` and `~/.local/share` folders to their new names, unless those already exist.

`:export` is how annotations leave the viewer: it writes a copy with all of them in it as PDF
annotations, as `{name}-annotated.pdf` beside the document unless given a path. It won't replace a
file without `:export!`, and never the document itself. What fill & sign adds is black in the copy,
whatever colour the theme shows it in.
