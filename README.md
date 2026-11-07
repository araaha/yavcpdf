# pdfviewer

A minimal, keyboard-driven PDF viewer. The whole window is the document, with a status line at the
bottom. It uses PdfCraft's render engine, and its annotation writer for `:export`, and nothing else
from the PdfCraft app.

    cargo build --release -p pdfviewer
    target/release/pdfviewer [--page N] [--password PW] [file.pdf]

With no file it reopens the last document. It reloads the PDF when the file changes.

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
| `tt` | hotbar |
| `th` `td` `te` `tn` | highlight, draw, erase, sticky note tool (shows the hotbar) |
| `tf` | fill & sign tool |
| `tT` `tD` `tC` `tX` `tO` `tL` `tS` | fill & sign: text, date, check, cross, dot, line, signature |
| `1`-`5` `[` `]` | with a tool: colour, thinner, thicker (fill & sign: smaller, larger) |
| `dh` | delete an annotation by hint |
| `u` `U` | undo, redo |
| `Ctrl-r` `T` | light or dark theme, next theme |
| `gl` `Ctrl-s` `Alt-f` | tint on links, status line, fullscreen |
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

**Sticky notes.** With `tn`, click a place and type the note in the bar. Its text shows when the
pointer is on it, and all of them show while the tool is in hand. Click a note to edit it.

**Fill & sign.** Pick an item and click where it goes, or press `f` and pick a word to put it just
after that word. The first signature asks you to draw one on a pad and is kept for every document
in `~/.local/share/pdfviewer/signature.json`, readable by you alone; `:sign` draws a new one. Move
something by undoing it and placing it again.

**Portals.** Press `p` where you are reading, go to the target, press `p` again. The view returns,
and the portal window shows the target of the portal nearest the view.

**Commands.** `:{page}`, `:q`, `:w`, `:e {file}`, `:noh`, `:theme {name}`, `:zoom {percent}`,
`:reload`, `:toc`, `:hotbar`, `:marks`, `:sign`, `:export [file]`, `:export! [file]`.

## Configuration

`~/.config/pdfviewer/config.toml`, reloaded while the viewer runs. Put only what you want to change
in it: [`src/default_config.toml`](src/default_config.toml) holds every setting with its default.
Colours are in the themes, and one key is enough to change one:

    [themes.dark]
    background = "#000000"    # around and between the pages
    link = "#0000ff55"        # links to places in the document

## What is stored where

Position, marks, portals and annotations are kept in `~/.local/share/pdfviewer/{hash}.json`. The PDF
itself is never changed, so other viewers don't show them. A file keeps its data when it moves, and
when it is rebuilt at the same path.

`:export` is how annotations leave the viewer: it writes a copy with all of them in it as PDF
annotations, as `{name}-annotated.pdf` beside the document unless given a path. It won't replace a
file without `:export!`, and never the document itself. What fill & sign adds is black in the copy,
whatever colour the theme shows it in.
