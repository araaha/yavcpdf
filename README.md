<div align="center">
  <h1>yavcpdf</h1>
  <p>A minimal, keyboard-driven PDF viewer</p>

  [![Linux](https://img.shields.io/badge/Linux-131415?style=for-the-badge&logo=linux&logoColor=white)](#installation)
  [![Windows](https://img.shields.io/badge/Windows-131415?style=for-the-badge&logo=data:image/svg%2bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCI%2BPHBhdGggZmlsbD0iI2ZmZmZmZiIgZD0iTTAgMGgxMXYxMUgwek0xMyAwaDExdjExSDEzek0wIDEzaDExdjExSDB6TTEzIDEzaDExdjExSDEzeiIvPjwvc3ZnPg==)](#installation)
  [![macOS](https://img.shields.io/badge/macOS-131415?style=for-the-badge&logo=apple&logoColor=white)](#installation)
  [![Build status](https://img.shields.io/github/actions/workflow/status/araaha/yavcpdf/release.yml?style=for-the-badge&logo=github&logoColor=white&label=build)](https://github.com/araaha/yavcpdf/actions/workflows/release.yml)
  [![Latest release](https://img.shields.io/github/v/release/araaha/yavcpdf?style=for-the-badge&logo=github&logoColor=white)](https://github.com/araaha/yavcpdf/releases)
  [![AUR version](https://img.shields.io/aur/version/yavcpdf?style=for-the-badge&logo=archlinux&logoColor=white)](https://aur.archlinux.org/packages/yavcpdf)
  [![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=for-the-badge)](LICENSE-MIT)

  <a href="#installation">Install</a> · <a href="#usage">Usage</a> · <a href="#configuration">Configuration</a> · <a href="#what-is-stored-where">Storage</a> · <a href="#exporting-annotations-and-forms">Exporting</a> · <a href="#engine">Engine</a>
</div>

yavcpdf is a PDF viewer you drive from the keyboard. The whole window is the
document, with a status line at the bottom. It uses PdfCraft's render engine,
and its annotation and form writers for `:export`.
<!-- demo: add a screenshot or video here -->

Highlights
----------

- **Vim-like** Keys such as `j`, `gg` and `G`, counts like `10j` and `42G`,
  `/` search, marks, and every binding rebindable in a TOML file
- **Every command in reach** `:` opens a menu of the commands and of nearly
  every action, each with its keys on the right, filtered as you type
- **Hints** Select words, follow links and delete annotations by typing a label
- **Smart jump and overview** Go to, or peek at, the figure, table, equation,
  section or citation a reference names, even where the PDF made no link
- **Portals** Mark a place, go to another and come back
- **Annotations** Highlight, draw, sticky notes, and fill & sign are kept
  beside the PDF and written into a copy on demand
- **Forms** Fill in the text fields, check boxes, radio buttons and drop-downs
  of a PDF form by hint or click, and write the answers into a copy on demand
- **Live reload** The PDF reloads when the file changes, and so does the
  configuration

Table of Contents
-----------------

* [Installation](#installation)
    * [Package managers](#package-managers)
    * [Other methods](#other-methods)
* [Usage](#usage)
    * [Keys](#keys)
    * [Hints](#hints)
    * [Smart jump](#smart-jump)
    * [Overview](#overview)
    * [Table of contents](#table-of-contents-1)
    * [Annotations](#annotations)
    * [Forms](#forms)
    * [Portals](#portals)
    * [Opening files](#opening-files)
    * [Password-protected PDFs](#password-protected-pdfs)
    * [Protecting a copy with a password](#protecting-a-copy-with-a-password)
    * [Commands and the `:` menu](#commands-and-the--menu)
    * [Editing text](#editing-text)
* [Configuration](#configuration)
    * [Key bindings](#key-bindings)
    * [Themes](#themes)
* [What is stored where](#what-is-stored-where)
* [Exporting annotations and forms](#exporting-annotations-and-forms)
* [Engine](#engine)
* [Not supported](#not-supported)
* [VS](#vs)
* [License](#license)

Installation
------------

yavcpdf is built for Linux x86_64 (X11 and Wayland), Windows x86_64, and a
universal macOS app.
### Package managers

<details>
<summary><b>Arch Linux (AUR)</b></summary>
<br>

Build it from source with `yavcpdf`, or install the release binary with
`yavcpdf-bin`, using your AUR helper:

```sh
yay -S yavcpdf        # or yavcpdf-bin
```

OR

```sh
paru -S yavcpdf       # or yavcpdf-bin
```
> The two packages conflict with each other. Both install `/usr/bin/yavcpdf`,
> `yavcpdf.desktop`, and the reference configuration at
> `/usr/share/doc/yavcpdf/config.toml`.
</details>

### Other methods

<details>
<summary><b>Binary releases</b></summary>
<br>

Download `yavcpdf-vX.Y.Z-x86_64-linux.tar.gz` from the
[releases page](https://github.com/araaha/yavcpdf/releases), together with its
`.sha256` file. Each archive contains the `yavcpdf` binary, `yavcpdf.desktop`,
this `README.md`, the reference configuration `config.toml`, `LICENSE-MIT`,
`LICENSE-APACHE`, `NOTICE`, `ATTRIBUTION.md` and the `licenses/` directory.
The binary is built on Ubuntu 22.04, so it runs on systems with that glibc
(2.35) or newer.

At run time it needs OpenGL/EGL drivers, Wayland or X11 libraries (libwayland,
libX11, libXcursor, libXi, libXrender, libxkbcommon and libxkbcommon-x11), dbus
and fontconfig (`fc-match`). Optional: `xdg-desktop-portal` with a backend for
the file chooser, and `xdg-utils` (`xdg-open`) for links.
</details>

<details>
<summary><b>Windows</b></summary>
<br>

Download `yavcpdf-vX.Y.Z-x86_64-windows.zip` from the
[releases page](https://github.com/araaha/yavcpdf/releases), together with its
`.sha256` file. Unzip it anywhere and run `yavcpdf.exe`; with no file given it
opens the last document, or a blank page if there is none, where `:open` or
`:e` open a file. To make it your PDF
viewer, right-click a PDF, choose **Open with → Choose another app**, pick
`yavcpdf.exe` and tick "Always use this app". Links open in your default
browser. Windows may show a SmartScreen warning for the unsigned program:
choose "More info", then "Run anyway".
</details>

<details>
<summary><b>macOS</b></summary>
<br>

Download `yavcpdf-vX.Y.Z-universal-macos.zip` (one build for Apple silicon and
Intel) from the [releases page](https://github.com/araaha/yavcpdf/releases),
together with its `.sha256` file. Unzip it and move `yavcpdf.app` to
`/Applications`. The app is ad-hoc signed but not notarized, so Gatekeeper
blocks the first launch: open System Settings → Privacy & Security and click
"Open Anyway", or run

```sh
xattr -dr com.apple.quarantine /Applications/yavcpdf.app
```

Run it from Terminal with a file:

```sh
/Applications/yavcpdf.app/Contents/MacOS/yavcpdf file.pdf
open -a yavcpdf --args /absolute/path/to/file.pdf
```

Use an absolute path with `open`: LaunchServices starts the app with `/` as its
working directory.

Double-clicking a PDF in Finder and "Open With" do not open files in yavcpdf
yet, so run it from Terminal with a file, or launch the app on its own, which
opens the last document, or a blank page if there is none, where `:open` or `:e` open a file.
</details>

<details>
<summary><b>Settings folders and fonts on each system</b></summary>
<br>

`config.toml` and the per-document data (annotations, signature) live in:

| System | Settings and data |
| --- | --- |
| Linux | `~/.config/yavcpdf/` and `~/.local/share/yavcpdf/` |
| Windows | `%APPDATA%\yavcpdf\` for both (`%USERPROFILE%\AppData\Roaming` if `APPDATA` is unset) |
| macOS | `~/Library/Application Support/yavcpdf/` for both |

On every system `$XDG_CONFIG_HOME` and `$XDG_DATA_HOME` win when set.

The `font` setting is looked up with fontconfig's `fc-match`. Windows and macOS
don't have it, so the built-in font is used there unless you install
fontconfig (for example `brew install fontconfig`); a missing `fc-match` is not
an error. `fc-match` installed by Homebrew lives in `/opt/homebrew/bin`, which
is not on `PATH` for an app started from Finder or the Dock, so the `font`
setting only takes effect when yavcpdf is started from a terminal.
</details>

<details>
<summary><b>From source</b></summary>
<br>

These build instructions are for Linux. Building needs Rust 1.95 or newer. The
build fetches PdfCraft and the egui and winit forks it patches in from GitHub,
so it needs network access. The run-time needs are the same as for the binary
release: OpenGL/EGL drivers, Wayland or X11 libraries, dbus and fontconfig, and
optionally `xdg-desktop-portal` and `xdg-utils`.

```sh
git clone https://github.com/araaha/yavcpdf.git
cd yavcpdf
cargo build --release --locked
```

Then install it by hand:

```sh
install -Dm755 target/release/yavcpdf ~/.local/bin/yavcpdf
install -Dm644 yavcpdf.desktop ~/.local/share/applications/yavcpdf.desktop
xdg-mime default yavcpdf.desktop application/pdf    # to make it the default
```
</details>

Usage
-----

```sh
yavcpdf [--page N] [--password PW] [--geometry WxH[+X+Y]]
        [--fullscreen|--no-fullscreen] [--statusbar|--no-statusbar] [file.pdf]
```

With no file, yavcpdf reopens the last document. If that file is gone or won't open (the reason is in the status bar), or there is none, it opens a blank page instead, where `:open` or `:e` open a file; a file named on the command line that won't open is an error. The blank page lists the recent files that still exist: type a row's label, or move with j/k and press Enter, or click; Esc closes the list and `:recent` shows it again, over a document too. It reloads the PDF when the
file changes. A password-protected PDF opens a [password menu](#password-protected-pdfs). On Windows, and on macOS when not started from a terminal,
`--help` and argument errors show in a dialog, as do fatal errors.

| Option | Effect |
| --- | --- |
| `--page N` | Open at page N |
| `--password PW` | The password of an encrypted PDF; without it, or if it is wrong, a menu asks for the password |
| `--geometry WxH[+X+Y]` | Set the window's inner size in points, and optionally its position |
| `--fullscreen` / `--no-fullscreen` | Override `view.fullscreen` for this launch |
| `--statusbar` / `--no-statusbar` | Override `statusbar.visible` for this launch |

The last of the fullscreen or the status bar options given wins.
`view.fullscreen` and `statusbar.visible` only take effect at startup; a
configuration reload does not apply them again.

`--geometry 1000x700` sets the size, and `--geometry 1000x700+40+30` also sets
the position. Width and height are 1 to 16384 and the offsets are 0 to
16384. Negative offsets are not supported, so X11's offsets
from the right or bottom edge do not work. On Wayland the compositor decides
where a window goes, so `+X+Y` may be ignored.

### Keys

Every binding can be changed under `[keys]` in the configuration. A count works
before most motions: `10j`, `42G`. With a tool in hand, the digits
<kbd>1</kbd> to <kbd>N</kbd> (N is the number of hotbar colours, 5 by default)
choose a colour instead, so counts such as `42G` don't work then.

> [!TIP]
> Press <kbd>:</kbd> to see nearly every action with its keys, and type to filter them.

| Keys | Action |
| --- | --- |
| <kbd>h</kbd> <kbd>j</kbd> <kbd>k</kbd> <kbd>l</kbd> | Scroll |
| <kbd>Ctrl</kbd>+<kbd>D</kbd> / <kbd>Ctrl</kbd>+<kbd>U</kbd> | Scroll half a screen |
| <kbd>Ctrl</kbd>+<kbd>F</kbd> / <kbd>Ctrl</kbd>+<kbd>B</kbd>, <kbd>Space</kbd> / <kbd>Shift</kbd>+<kbd>Space</kbd> | Scroll a screen down / up |
| <kbd>J</kbd> / <kbd>K</kbd> | Next / previous page |
| <kbd>g</kbd> <kbd>g</kbd> / <kbd>G</kbd> | First page / end; `NG` goes to page N |
| <kbd>0</kbd>, <kbd>^</kbd> / <kbd>$</kbd> / <kbd>z</kbd> <kbd>z</kbd> | Left edge / right edge / centre |
| <kbd>=</kbd>, <kbd>+</kbd> / <kbd>-</kbd> / <kbd>W</kbd> / <kbd>H</kbd> | Zoom in / zoom out / fit width / fit height |
| <kbd>/</kbd> / <kbd>?</kbd> | Search forward / backward |
| <kbd>n</kbd> / <kbd>N</kbd> | Next / previous match |
| <kbd>m</kbd> then a key / <kbd>'</kbd> or <kbd>`</kbd> then a key / <kbd>'</kbd> <kbd>'</kbd> | Set a mark / go to a mark / jump back |
| <kbd>M</kbd> / <kbd>g</kbd> <kbd>b</kbd> | Name a bookmark here (opens `:bookmark `) / pick a bookmark: <kbd>j</kbd> <kbd>k</kbd> move, <kbd>Enter</kbd>, <kbd>l</kbd> or a click jumps (<kbd>Alt</kbd>+<kbd>O</kbd> comes back), <kbd>/</kbd> filters, <kbd>Esc</kbd> clears the filter and then closes, <kbd>q</kbd> closes, <kbd>d</kbd> or <kbd>Delete</kbd> deletes the highlighted one when not filtering |
| <kbd>Alt</kbd>+<kbd>O</kbd> / <kbd>Alt</kbd>+<kbd>I</kbd> | Jump history, back / forward; the last 100 places are kept per document between sessions |
| <kbd>f</kbd> / <kbd>y</kbd> | Select by hint / copy the selection, with its lines joined and hyphenation undone (`view.copy_reflow = false` copies the lines as they are) |
| <kbd>s</kbd> <kbd>s</kbd> / <kbd>s</kbd> <kbd>g</kbd> / <kbd>s</kbd> <kbd>w</kbd> / <kbd>s</kbd> <kbd>a</kbd> | Search the selected text on Google Scholar / Google / Wikipedia / arXiv, in the browser; the selection stays. [Sites are configurable](#searching-the-web) |
| <kbd>F</kbd> | Follow a link by hint; one that names a spot lands there at `view.jump_position`. The PDF's notes (sticky notes and other comments with text) appear among the link hints; picking one pins its text |
| <kbd>i</kbd> | Fill in a form field, press a form button, or sign into a signature field, by hint; a click does it too |
| <kbd>g</kbd> <kbd>d</kbd>, <kbd>Ctrl</kbd>+<kbd>]</kbd> | Smart jump by hint; middle click does it with the mouse |
| <kbd>g</kbd> <kbd>o</kbd>, <kbd>Alt</kbd>+<kbd>C</kbd> | Overview by hint; right click does it with the mouse |
| <kbd>Tab</kbd> | Table of contents (<kbd>j</kbd> <kbd>k</kbd>, <kbd>/</kbd> filters, <kbd>Enter</kbd>) |
| <kbd>p</kbd> / <kbd>g</kbd> <kbd>p</kbd> / <kbd>d</kbd> <kbd>p</kbd> / <kbd>e</kbd> <kbd>p</kbd> / <kbd>F12</kbd> | Make, go to, delete and edit a portal; portal window |
| <kbd>P</kbd> <kbd>p</kbd> / <kbd>P</kbd> <kbd>u</kbd> | Write a copy with a password, or without one, [beside the document](#protecting-a-copy-with-a-password) |
| <kbd>t</kbd> <kbd>t</kbd> | Show or hide the hotbar, also while a tool is in hand (the tool stays in hand) |
| <kbd>t</kbd> <kbd>h</kbd> / <kbd>t</kbd> <kbd>d</kbd> / <kbd>t</kbd> <kbd>e</kbd> / <kbd>t</kbd> <kbd>n</kbd> | Highlight, draw, erase, sticky note tool (shows the hotbar) |
| <kbd>t</kbd> <kbd>f</kbd> | Fill & sign tool |
| <kbd>t</kbd> <kbd>T</kbd> / <kbd>D</kbd> / <kbd>C</kbd> / <kbd>X</kbd> / <kbd>O</kbd> / <kbd>L</kbd> / <kbd>S</kbd> | Fill & sign: text, date, check, cross, dot, line, signature |
| <kbd>1</kbd>-<kbd>5</kbd>, <kbd>[</kbd> <kbd>]</kbd> | With a tool: colour, thinner, thicker (fill & sign: smaller, larger) |
| <kbd>d</kbd> <kbd>h</kbd> | Delete an annotation by hint |
| <kbd>u</kbd> / <kbd>U</kbd> | Undo / redo |
| <kbd>T</kbd> | Next theme |
| <kbd>g</kbd> <kbd>l</kbd> / <kbd>g</kbd> <kbd>i</kbd> / <kbd>Ctrl</kbd>+<kbd>S</kbd> / <kbd>Alt</kbd>+<kbd>F</kbd> | Tint on links / form fields / status line / fullscreen |
| <kbd>></kbd> / <kbd><</kbd> | Turn the pages a quarter turn clockwise / counter-clockwise (a count turns more); [kept for the document](#what-is-stored-where) |
| <kbd>D</kbd> / <kbd>g</kbd> <kbd>D</kbd> | Show pages side by side in pairs / with the first page alone on the right, as in a printed book; [kept for the document](#what-is-stored-where). Page keys, `:N` and the fits then work by the row |
| <kbd>F5</kbd> | [Presentation mode](#presentation-mode) |
| <kbd>Ctrl</kbd>+<kbd>T</kbd> | Open a file with the desktop's file chooser |
| <kbd>R</kbd> / <kbd>:</kbd> / <kbd>q</kbd>, <kbd>Z</kbd> <kbd>Q</kbd>, <kbd>Z</kbd> <kbd>Z</kbd>, <kbd>Ctrl</kbd>+<kbd>W</kbd> | Reload / command line / quit |
| <kbd>Esc</kbd>, <kbd>Ctrl</kbd>+<kbd>C</kbd> | Cancel: closes a bar, hints, the contents or a choice panel; otherwise takes back the last thing: keys, overview, jump, portal, selection, tool, search. Rebind it as `cancel` |

Some actions have more default keys than shown, such as the arrow keys,
<kbd>PageUp</kbd> and <kbd>PageDown</kbd>, <kbd>Home</kbd> and <kbd>End</kbd>.
[`src/default_config.toml`](src/default_config.toml) lists them all.

### Presentation mode

<kbd>F5</kbd> or `:present` (the `toggle_presentation` action) goes fullscreen and shows one page, fit whole to the
screen on the theme's background, with no status bar, hotbar, bars or tool. The document's own annotations (highlights, ink, notes, fill and sign items, signatures) are shown, but tints, links, search matches and the portal window are not. It starts on the page in the
middle of the view. The next two pages and the previous one are kept rendered, so turning a page is instant. On the
blank page it only says "no document open".

| Keys | Action |
| --- | --- |
| <kbd>J</kbd>, <kbd>j</kbd>, <kbd>l</kbd>, <kbd>Space</kbd>, <kbd>PageDown</kbd>, <kbd>Down</kbd>, <kbd>Right</kbd>, left click, wheel down | Next page |
| <kbd>K</kbd>, <kbd>k</kbd>, <kbd>h</kbd>, <kbd>Shift</kbd>+<kbd>Space</kbd>, <kbd>Backspace</kbd>, <kbd>PageUp</kbd>, <kbd>Up</kbd>, <kbd>Left</kbd>, wheel up | Previous page |
| <kbd>g</kbd> <kbd>g</kbd>, <kbd>Home</kbd> / <kbd>G</kbd>, <kbd>End</kbd> | First / last page |
| `N` <kbd>G</kbd>, `N` <kbd>Enter</kbd> | Page N |
| <kbd>t</kbd> / <kbd>r</kbd> | Show or hide the timer / restart it |
| <kbd>Esc</kbd>, <kbd>Ctrl</kbd>+<kbd>C</kbd>, <kbd>q</kbd>, <kbd>F5</kbd> | Leave (<kbd>Esc</kbd> and <kbd>Ctrl</kbd>+<kbd>C</kbd> first drop a count or <kbd>g</kbd> typed) |

The wheel turns one page per notch, and one per flick of a trackpad. Every other
key, including <kbd>:</kbd>, <kbd>Ctrl</kbd>+<kbd>W</kbd> and <kbd>Z</kbd><kbd>Z</kbd>, does nothing. The window re-fits when resized.

The timer shows the time since the presentation started, as `mm:ss` (`h:mm:ss`
from an hour on), in a corner. `presentation.timer = true` shows it from the
start.

Leaving restores fullscreen, the status bar, the hotbar, the tool and the zoom, and
goes to the top of the page last shown (or back to where the view was, if still on
the page it started on). The jump history gets at most one entry for the whole
presentation.

### Hints

Hints cover the current page only (the one the status bar shows), the part of
it showing, however far out you zoom. <kbd>f</kbd> labels every word there. Type a label for the first word, then
one for the last word, or <kbd>Enter</kbd> for just the first. With the
highlight tool on, the selection is highlighted directly; without it,
<kbd>y</kbd> copies it.

<kbd>F</kbd> covers every link there in the theme's `hint_link` colour and numbers
them from 1. Type a number to follow its link: it is taken as soon as no longer
number could be meant, and <kbd>Enter</kbd> takes it at once (1 when there is
also a 12).

A label is drawn in two parts, each with its own colours in the theme: the key
to press next in `hint_key1_background` and `hint_key1_text`, the keys after it
in `hint_key2_background` and `hint_key2_text`. As you type, the next key takes
the first pair in turn.

Labels shrink as you zoom out. `hints.font_size` is the size in points at 100%
zoom and the largest a label gets, `hints.scale = false` keeps every label at
that size, and `hints.min_font_size` is the smallest a label gets when scaling
(or `font_size`, if that is smaller).

### Smart jump

<kbd>g</kbd> <kbd>d</kbd> labels the references on the current page, whether or not
the PDF made them links: `Figure 3`, `Table 2`, `Section 3.1`, `Theorem 1.2`,
`Eq. (4)`, `(4)`, `[12]`, `Smith et al., 2020`. Pick one to go to the caption,
heading, equation or bibliography entry it names, by reading the text of the
whole document. It lands where `view.jump_position` says, the middle of the
window by default. <kbd>Alt</kbd>+<kbd>O</kbd> comes back.

> [!NOTE]
> It is a good guess rather than a certainty: a reference split over two lines
> isn't seen, and a scanned document has no text to read.

### Overview

Shows what a reference points at in a window over the page, so you can read a
figure or a citation without losing your place. It finds what smart jump finds, on the current page.
While it is open, the vertical scrolling keys (<kbd>j</kbd> <kbd>k</kbd>, the
half-page and the screen keys) move it, as does the wheel over it. <kbd>Enter</kbd> or a click on it goes there,
<kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>PageDown</kbd> and
<kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>PageUp</kbd> step through other candidates
for the reference, and <kbd>Esc</kbd>, <kbd>Ctrl</kbd>+<kbd>C</kbd>, a click
beside it closes it. Any other key closes it and then does its usual
action, so <kbd>h</kbd> and <kbd>l</kbd> close it and scroll the page. `[overview]` sets its size and place.

### Table of contents

It comes from the PDF's bookmarks. A PDF with none, such as a Google Docs
export, gets one from its printed contents pages when their entries are links:
the titles are the text of those links, and their indents are the levels. A PDF
with neither has no table of contents. An entry that names a spot on its page
lands there at `view.jump_position`; one that names only the page lands at its top.

### Annotations

Choose a tool from the hotbar keys: <kbd>t</kbd> <kbd>h</kbd> highlights,
<kbd>t</kbd> <kbd>d</kbd> draws, <kbd>t</kbd> <kbd>e</kbd> erases and
<kbd>t</kbd> <kbd>n</kbd> adds sticky notes. They are kept beside the PDF, not
in it; see [What is stored where](#what-is-stored-where).

**Sticky notes.** With <kbd>t</kbd> <kbd>n</kbd>, click a place and type the
note in the bar. Its text shows when the pointer is on it, and all of them show
while the tool is in hand. Click a note to edit it.

**Notes in the PDF.** The comments a PDF carries (sticky notes, free text and
other markup with text) show their author and text, read-only, when the pointer
is on them. Click one, or pick it with <kbd>F</kbd>, to keep it shown until a click
elsewhere or <kbd>Esc</kbd>. A click on a link under a note follows the link. Replies and
statuses are shown in the popup of the note they answer. They are never written to the
sidecar or exported again; the hover popups are not drawn while the draw, erase, note or
fill tool is in hand.

**Fill & sign.** Pick an item and click where it goes, or press <kbd>f</kbd>
and pick a word to put it just after that word. The first signature asks you to
draw one on a pad and is kept for every document in
`signature.json` in the data folder (see the
Settings folders table under Installation), readable by you alone; `:sign` draws a
new one. On the pad, draw with the mouse; the undo key (<kbd>u</kbd> by default)
removes the last stroke, <kbd>Enter</kbd> keeps the signature and <kbd>Esc</kbd>
cancels. Move something by undoing it and placing it again.

### Forms

A PDF with a form (AcroForm) shows its fields as they are. Press <kbd>i</kbd>
to label the fields on the current page (hidden and read-only ones get no label), or click
one with no tool in hand, and then:

- **Text fields** open the bar with the current text. <kbd>Enter</kbd> keeps
  what you typed and <kbd>Esc</kbd> drops it. A field's length limit is
  enforced: a text that is too long stays in the bar to be shortened. A
  multi-line field is edited on one line, with its line breaks shown as the two
  characters `\n`; type `\n` for a line break and `\\` for a backslash (a
  backslash before anything else stays as it is). A value the form refuses (a
  number field given letters, say) puts the previous value back and says why.
- **Check boxes** toggle. **Radio buttons** select the one you chose.
- **Drop-downs and list boxes** open a panel that looks like the table of
  contents, headed by the field's tooltip, or its name when it has none, with a
  ✓ at the right of the option or options chosen now. It opens on the chosen
  option. A value the field has that is none of the options (set by another
  viewer, or typed earlier) gets a ticked row of its own, after `(none)` (at
  the top in a list of several choices); choosing it keeps the value and
  marking it again in a list of several choices removes it. The mouse wheel
  scrolls the list, and the page behind does not move.
  <kbd>j</kbd>/<kbd>k</kbd>, <kbd>Up</kbd>/<kbd>Down</kbd> and
  <kbd>Ctrl</kbd>+<kbd>N</kbd>/<kbd>Ctrl</kbd>+<kbd>P</kbd> move,
  <kbd>Ctrl</kbd>+<kbd>D</kbd>/<kbd>Ctrl</kbd>+<kbd>U</kbd> move by ten, and
  <kbd>g</kbd>/<kbd>G</kbd> go to the first and last. <kbd>/</kbd> filters the
  options by part of their text (the editing keys work, <kbd>Esc</kbd> ends the
  filter). <kbd>Enter</kbd> or <kbd>l</kbd> chooses the highlighted option and
  closes; the first row, `(none)`, clears the choice. In a list that takes
  several choices <kbd>Space</kbd> marks or unmarks the highlighted option and
  <kbd>Enter</kbd> or <kbd>l</kbd> keeps the marked ones (none clears it). A
  drop-down you can type in (an editable combo box) shows a last row
  `use "text"` for what you typed in the filter, which sets that text.
  <kbd>Esc</kbd>, <kbd>q</kbd> or <kbd>Tab</kbd> close the panel and change
  nothing. With the mouse, a click on an option chooses it (marks it, in a
  list of several choices) and a click outside the panel closes it. A quick
  second click right after the panel opens or closes is ignored. <kbd>Enter</kbd>
  with a filter that matches nothing leaves the panel open.
- **Buttons** do what their PDF action says, when it isn't a script: reset the
  form (it removes the values you filled in, so the fields show what the
  document itself contains; a `/DV` default value is not applied), go to the
  next, previous, first or last page or to a place, open a web link, and show
  or hide fields. Fields shown or hidden are kept with what you filled in, and
  a hidden field can't be filled in until it shows again. Other buttons say why
  they do nothing.

Fields that calculate their value from others with Acrobat's built-in
calculations (sum, product, average, minimum, maximum) update when those fields
change. A calculation written as a script does not. Undo (<kbd>u</kbd>) and redo
(<kbd>U</kbd>) go through the changes you made, and `:clearform` forgets all of
them at once, putting back the fields a button showed or hid.

What you fill in is kept beside the PDF, like annotations, and the PDF is never
changed. What you fill in can be personal, so that file is readable only by you
(mode 0600). The pages are drawn from a copy filled in memory, which is why a
page may take a moment to refresh after a change.

> [!NOTE]
> **JavaScript is never run.** Scripts that format, check, calculate or react to
> a click do nothing, and a button that needs one says so. Submitting a form is
> not supported, and nothing is ever sent. Password fields can't be filled in,
> as what is typed would be kept as plain text in the sidecar.

Picking a **signature field** (or clicking it) signs into it: your saved
signature is scaled to fit the field, centred. The first time, with none saved,
the signature pad opens (as for fill & sign) and the drawing is signed into the
field when you finish it. Picking a field with a signature of
yours mostly inside it, including one placed by hand with the fill tool,
replaces that signature, and one undo brings the old one back. Read-only
signature fields can't be picked, and one that the PDF has already signed is
refused. The signature is kept in the
sidecar like other fill & sign marks and leaves the viewer through `:export`;
it is a drawing, not a cryptographic signature.

### Portals

Press <kbd>p</kbd> where you are reading, go to the target, press <kbd>p</kbd>
again. The view returns, and the portal window shows the target of the portal
nearest the view.

### Opening files

<kbd>Ctrl</kbd>+<kbd>T</kbd> or `:open` shows the desktop's file chooser,
through the [XDG portal](https://flatpak.github.io/xdg-desktop-portal/) when
there is one, so a portal backend such as `xdg-desktop-portal-termfilechooser`
gets used. It says "no file chosen" if the chooser is cancelled, and also if
none could be shown.

### Password-protected PDFs

A PDF that needs a password, at startup, with `:e` or `:open`, from the file
chooser, or after a reload that no longer accepts the old password, opens a
centred menu headed `Password for <file>`. Type the password, shown as bullets,
and press <kbd>Enter</kbd>. A wrong one leaves the menu open with "wrong
password" and what you typed. Every printable key types, <kbd>u</kbd> included,
and the editing keys and pasting work as in the `:` bar. <kbd>Ctrl</kbd>+<kbd>Z</kbd>
(`edit_undo`) clears the whole field. <kbd>Esc</kbd> closes the menu and keeps
the open document; at startup, with none to fall back to, it quits. The page,
the hotbar and the `:` bar don't respond while the menu is open. The password is
kept in memory only, and nothing is saved for the document until it is open.

### Protecting a copy with a password

<kbd>P</kbd> <kbd>p</kbd> (`:protect`) writes a copy of the document with a
password, and <kbd>P</kbd> <kbd>u</kbd> (`:unprotect`) a copy without its
password. Both only write a copy: the open document is never changed, and there
is no way to protect it in place.

- **Where.** `:protect [FILE]` and `:unprotect [FILE]` write to `FILE` (`~/` is
  your home folder), or beside the document as `<name>-protected.pdf` and
  `<name>-unprotected.pdf`. An existing file is not replaced unless the command
  is `:protect!` or `:unprotect!`. The document itself is never replaced, `!` or not.
- **The password.** A menu asks for the new password, then for it again. It is
  both the password that opens the copy and the owner password, AES-256 with
  nothing restricted. An empty password is refused. Protecting a document that
  already has a password changes it. `:unprotect` says "not password protected"
  for a document with none.
- **The owner password.** A document opened with a user password alone can't
  have its password changed or removed without the owner password, so the menu
  asks for that first, and a wrong one keeps the menu open. A document opened as
  its owner, or whose two passwords are the same, doesn't ask.
- **Cancelling.** <kbd>Esc</kbd> cancels at any step, and nothing is written
  until the password has been typed twice (or the owner password accepted, for
  `:unprotect`). A wrong owner password keeps the menu open with "wrong owner
  password".
- **Signatures.** The copy is a rewrite of the file, so a digital signature in the
  document no longer verifies in it. The message says so when there is one.
- **Annotations and form values** are not written into the PDF: use `:export`
  for that. The copy still opens in yavcpdf with the annotations, form values,
  marks and portals the document has now.

### Commands and the `:` menu

| Command | Action |
| --- | --- |
| `:N` | Go to page N |
| `:q`, `:quit`, `:q!`, `:wq`, `:x` | Quit |
| `:w`, `:write` | Save the position, marks, bookmarks, jump history, portals and annotations |
| `:e FILE`, `:edit FILE`, `:open [FILE]` | Open a file; `:open` with none shows the file chooser |
| `:noh`, `:nohlsearch` | Clear the search highlight |
| `:theme NAME` | Switch theme; with an unknown name it lists them |
| `:zoom PERCENT` | Set the zoom |
| `:reload` | Reload the document |
| `:rotate [90\|180\|270\|0\|cw\|ccw]` | Turn all the pages clockwise to that angle, or a quarter turn; with no argument, clockwise. Rotation is a setting of the view, kept per document: the PDF's own `/Rotate` is never changed, not even by `:export` |
| `:twopage [on\|off]` | Show pages in pairs (the config's `two_page`), or one at a time; with no argument, switch. Kept per document |
| `:cover [on\|off]` | With two pages, keep the first page alone on the right (`cover_alone`); with no argument, switch. Kept per document, and set even when not in two-page mode |
| `:toc` | Open the table of contents |
| `:hotbar` | Show or hide the hotbar |
| `:recent` | List the recent files |
| `:present` | Start or leave [presentation mode](#presentation-mode) |
| `:marks` | List the marks, and count the bookmarks |
| `:bookmark NAME`, `:bm NAME` | Add a named bookmark at this place; an existing name, in any case, moves to it |
| `:bookmarks` | Pick a bookmark: type <kbd>/</kbd> to filter, <kbd>j</kbd> <kbd>k</kbd> or the arrows and the wheel move, <kbd>Enter</kbd>, <kbd>l</kbd> or a click jumps, <kbd>d</kbd> or <kbd>Delete</kbd> deletes when not filtering, <kbd>Esc</kbd> clears the filter and then closes, <kbd>q</kbd> closes. Names match without regard to case. They are listed in the order of the document, with their page |
| `:sign` | Draw a new signature |
| `:export [FILE]`, `:export! [FILE]` | Write a copy with the annotations and the filled-in form; `!` replaces an existing file |
| `:protect [FILE]`, `:protect! [FILE]` | Write a copy with a password; `!` replaces an existing file |
| `:unprotect [FILE]`, `:unprotect! [FILE]` | Write a copy without its password; `!` replaces an existing file |
| `:clearform` | Forget everything filled into the form (undoable) |

`:` also opens a menu of the commands and of nearly every action, each with its
keys on the right.

> [!TIP]
> Letters in order are enough to find an entry: `tgl` ranks `toggle_links`
> first. Press <kbd>Tab</kbd> (or <kbd>Down</kbd>) to choose it, then
> <kbd>Enter</kbd> runs it. Without a choice, <kbd>Enter</kbd> runs the typed
> text, which gives "unknown command".

- **Choosing.** <kbd>Up</kbd>/<kbd>Down</kbd>, <kbd>Ctrl</kbd>+<kbd>N</kbd>/<kbd>Ctrl</kbd>+<kbd>P</kbd>
  and <kbd>Tab</kbd>/<kbd>Shift</kbd>+<kbd>Tab</kbd> choose an entry. Each puts
  the entry in the field, so the field always shows what <kbd>Enter</kbd> runs.
  A click on an entry runs it, unless there is text after the cursor, when it
  only fills the field, as <kbd>Tab</kbd> does. A click on a directory
  candidate descends into it.
- **Half pages.** <kbd>Ctrl</kbd>+<kbd>D</kbd> or <kbd>PageDown</kbd> and
  <kbd>Ctrl</kbd>+<kbd>U</kbd> or <kbd>PageUp</kbd> move by half the visible
  rows, stopping at the ends. In the `:` bar, while the menu is shown, they take
  precedence over `edit_erase_to_start` (<kbd>Ctrl</kbd>+<kbd>U</kbd>).
- **What runs.** What is highlighted is what <kbd>Enter</kbd> runs, and nothing
  else. With no highlight, <kbd>Enter</kbd> runs what you typed.
- **Auto-highlight.** Without a choice, an entry is highlighted only on the
  first word: when it is exactly what you typed, or the only name that
  continues it (same case) and is not one to run by accident. `q`, `quit`,
  `q!`, `wq`, `x`, `export`, `export!`, `protect`, `protect!`, `unprotect`, `unprotect!` and the `delete_*` actions are never
  picked that way; they run only when typed in full.
- **Text after the cursor.** <kbd>Enter</kbd> runs exactly what is typed, and
  nothing is highlighted.
- **Arguments.** After a command and a space the menu lists its arguments:
  theme names, the folders and PDFs for `:e` and `:open`, and every file for
  `:export`. These are never picked for you: choose one (a folder fills the
  field and goes into it, a file runs the command), or <kbd>Enter</kbd> uses
  your text. For arguments (files, themes), <kbd>Tab</kbd> with nothing chosen
  first adds the start that the candidates share, and on the only matching
  folder it goes into it and lists its contents.
- **Count.** When not everything fits, the menu shows a count such as `3/85`.
- **Hotbar.** It hides while the menu is open.
- **Actions by name.** An action can be run by name, such as `:zoom_in`, and
  `:zoom_in 3` gives it a count.
- **Fixed keys.** <kbd>Up</kbd>, <kbd>Down</kbd>, <kbd>Ctrl</kbd>+<kbd>N</kbd>,
  <kbd>Ctrl</kbd>+<kbd>P</kbd>, <kbd>Tab</kbd>, <kbd>Ctrl</kbd>+<kbd>D</kbd>,
  <kbd>Ctrl</kbd>+<kbd>U</kbd>, <kbd>PageDown</kbd> and <kbd>PageUp</kbd> in
  the menu are not set under `[keys]`.

### Editing text

The <kbd>/</kbd> <kbd>?</kbd> <kbd>:</kbd> bars, the note and text bars and the
contents and choice filters, and the password field, have a cursor.

| Keys | Action |
| --- | --- |
| <kbd>←</kbd> <kbd>→</kbd> | Move the cursor |
| <kbd>Ctrl</kbd>+<kbd>←</kbd> <kbd>Ctrl</kbd>+<kbd>→</kbd>, <kbd>Alt</kbd>+<kbd>B</kbd> <kbd>Alt</kbd>+<kbd>F</kbd> | Move by word |
| <kbd>Home</kbd> <kbd>End</kbd>, <kbd>Ctrl</kbd>+<kbd>A</kbd> <kbd>Ctrl</kbd>+<kbd>E</kbd> | Move to the ends |
| <kbd>Ctrl</kbd>+<kbd>Backspace</kbd>, <kbd>Ctrl</kbd>+<kbd>Delete</kbd> | Erase a word either side |
| <kbd>Ctrl</kbd>+<kbd>W</kbd> | Erase back to the last space |
| <kbd>Ctrl</kbd>+<kbd>U</kbd> <kbd>Ctrl</kbd>+<kbd>K</kbd> | Erase to either end |
| <kbd>Ctrl</kbd>+<kbd>Z</kbd> | Clear the [password](#password-protected-pdfs) field; it does nothing in the other fields |

The `edit_*` actions rebind them.

Configuration
-------------

`config.toml` in the config folder (`~/.config/yavcpdf/` on Linux, other systems:
see the Settings folders table under Installation),
reloaded while the viewer runs. Put only what you want to change in it:
[`src/default_config.toml`](src/default_config.toml) holds every setting with
its default, and the packages install it as
`/usr/share/doc/yavcpdf/config.toml`. A short example:

```toml
view.border = 2.0         # width of every interface outline, 0 to 8

[themes.dark]
background = "#000000"    # around and between the pages
link = "#0000ff55"        # links to places in the document

[keys]
quit = ["q"]              # ZQ, ZZ and Ctrl-W no longer quit
```

`view.border` sets the width of every interface outline, 0 to 8.
`view.jump_position` is where a
jump puts its target, as a percentage of the window height from the bottom (0)
to the top (100); the default is 50, the middle.
Destinations at the very top of a page, common in LaTeX/hyperref chapter and section links, show the page top at that height, so at 50 the end of the previous page fills the upper half of the window; set 100 for the old top-of-window behaviour.
At values near 0 the target sits at the bottom edge, so what follows it is below the window.
`view.two_page` (default false) shows pages side by side in pairs, and `view.cover_alone` (default true) keeps
the first page alone on the right of the spine, as in a printed book; the second applies only in two-page mode.
A lone cover or a trailing page is fit with the width of a full spread, so it keeps its place beside the spine.

### Searching the web

Select text (for example with <kbd>f</kbd>) and press <kbd>s</kbd> <kbd>s</kbd> to
search it on Google Scholar. The text is read as for copying, with its lines joined, and
opens in your browser. A count is ignored, and the selection is kept so that
you can search it elsewhere too. The sites are under `[web_search]`:

```toml
[web_search]
scholar = { keys = "ss", url = "https://scholar.google.com/scholar?q={}" }
```

Each site has `keys`, a key sequence written as under [Key bindings](#key-bindings),
and `url`, an `http://` or `https://` address in which every `{}` is replaced by
the text, percent-encoded. A site's settings merge with the defaults, so you can change
just its keys (`scholar = { keys = "sx" }`) or just its `url`, add a site under a new
name, or turn a default off with `keys = ""`. A url without `{}` or with another scheme,
or keys that can't be read, make the configuration fail to load. Keys that an action already uses, or
that an earlier site (by name) uses, are reported and skipped. A site key may begin another binding, such as
`g` and `gg`: the shorter one then runs after a short pause.

### Key bindings

Keys are written as in vim, for example `j`, `gg`, `<C-f>`, `<A-o>`, `<S-space>`
or `<esc>`.

> [!IMPORTANT]
> A list under `[keys]` **replaces** all of that action's default keys.

Esc and <kbd>Ctrl</kbd>+<kbd>C</kbd> are the `cancel` action. The `edit_*`
actions (see [Editing text](#editing-text)) apply in the bars, the contents and
choice filters and the password field only, and a key that types a character can't be used for them.

### Themes

Colours are in the themes, and one key is enough to change one. `dark` and
`light` ship by default; `theme` names the one in use, <kbd>T</kbd> cycles
through them and `:theme NAME` picks one. `background` is what shows around and
between the pages, and `page` is the sheet itself. `recolor = false` shows the
document's own colours.

What is stored where
--------------------

Position, marks, bookmarks, the jump history, the rotation, the two-page layout and cover setting, portals, annotations and the values filled into a form are kept in
`{hash}.json` in the data folder (`~/.local/share/yavcpdf/` on Linux, other
systems: see the Settings folders table under Installation).
The PDF itself is never changed, so other viewers don't show them. A file keeps
its data when it moves, and when it is rebuilt at the same path.

Exporting annotations and forms
-------------------------------

`:export` is how annotations leave the viewer: it writes a copy with all of them
in it as PDF annotations, as `{name}-annotated.pdf` beside the document unless
given a path. The values filled into a form go into the copy as the fields' real
values, with their appearances, so any viewer shows them, and the form stays
fillable; a document with only filled-in fields and no annotations exports too.
It won't replace a file without `:export!`, and never the document
itself. What fill & sign adds is black in the copy, whatever colour the theme
shows it in. The copy's pages are not turned: the rotation is only how the
viewer shows them, and the annotations are written in the page's own
coordinates.

Engine
------

The viewer uses PdfCraft's engine crates
(<https://github.com/storytold/pdfcraft>), pinned to a commit in `Cargo.toml`.
To update, run `scripts/update-pdfcraft.sh [rev]`; without a rev it takes the
current `main`. It changes every pdfcraft rev in `Cargo.toml` together, compares
`[patch.crates-io]` with PdfCraft's, and runs the tests and clippy. If it
reports a difference in the patch list, change `[patch.crates-io]` to match
upstream's and run it again.

yavcpdf is based on PdfCraft by the ArtCraft team. It is not made, sponsored or
endorsed by them.

Not supported
-------------

- Writing annotations or form values into the PDF itself; they leave only through `:export`
- JavaScript in PDFs (form scripts, formatting and validation), submitting a
  form, password and signature fields
- More than one document per window
- Printing
- Text in scanned documents; there is no OCR, and smart jump needs text

VS
--

- **zathura** is a plugin-based document viewer with a minimal interface and
  vim-like keys.
- **sioyek** is a PDF viewer for research papers, with a command menu, smart
  jump and portals.
- **mupdf** is a lightweight PDF viewer and library with a small interface.
- **yavcpdf** is a small, keyboard-driven viewer with annotations that are
  drawn and exported into the PDF, fill & sign, and fillable forms.

License
-------

yavcpdf is free software, licensed under either of
[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

It is based on PdfCraft and bundles third-party material under its own
licenses. See [NOTICE](NOTICE), [ATTRIBUTION.md](ATTRIBUTION.md) and
[licenses/](licenses/).
