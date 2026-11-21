# Keys and commands

Part of the [yavcpdf](../README.md) documentation.

## Keys

Every binding can be changed under `[keys]` in the configuration. A count works
before most motions: `10j`, `42G`. With a tool in hand, the digits
<kbd>1</kbd> to <kbd>N</kbd> (N is the number of hotbar colours, 5 by default)
choose a colour instead, so counts such as `42G` don't work then.

An alternative complete config with GNU Emacs bindings is in
[`src/emacs_config.toml`](../src/emacs_config.toml).

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
| <kbd>v</kbd> / <kbd>y</kbd> | Select by hint / copy the selection, with its lines joined and hyphenation undone (`view.copy_reflow = false` copies the lines as they are) |
| <kbd>s</kbd> <kbd>s</kbd> / <kbd>s</kbd> <kbd>g</kbd> / <kbd>s</kbd> <kbd>w</kbd> / <kbd>s</kbd> <kbd>a</kbd> | Search the selected text on Google Scholar / Google / Wikipedia / arXiv, in the browser; the selection stays. [Sites are configurable](configuration.md#searching-the-web) |
| <kbd>F</kbd> | Follow a link by hint; one that names a spot lands there at `view.jump_position`. The PDF's notes (sticky notes and other comments with text) and your own sticky notes appear among the link hints; picking one pins its text |
| <kbd>i</kbd> | Fill in a form field, press a form button, or sign into a signature field, by hint; a click does it too |
| <kbd>g</kbd> <kbd>a</kbd> | List your annotations (`:annotations`): <kbd>j</kbd> <kbd>k</kbd> move, <kbd>Enter</kbd>, <kbd>l</kbd> or a click jumps, <kbd>/</kbd> filters by text, <kbd>Esc</kbd> clears the filter and then closes, <kbd>q</kbd> closes, <kbd>d</kbd> or <kbd>Delete</kbd> deletes the highlighted one when not filtering (<kbd>u</kbd> brings it back) |
| <kbd>g</kbd> <kbd>d</kbd>, <kbd>Ctrl</kbd>+<kbd>]</kbd> | Smart jump by hint; middle click does it with the mouse |
| <kbd>g</kbd> <kbd>r</kbd> | [List what refers to](features.md#smart-jump) a caption, heading, equation or bibliography entry on the page, by hint; <kbd>n</kbd> / <kbd>N</kbd> step through them |
| <kbd>g</kbd> <kbd>o</kbd>, <kbd>Alt</kbd>+<kbd>C</kbd> | Overview by hint; right click does it with the mouse |
| <kbd>Tab</kbd> | Table of contents (<kbd>j</kbd> <kbd>k</kbd>, <kbd>/</kbd> filters, <kbd>Enter</kbd>) |
| <kbd>p</kbd> / <kbd>g</kbd> <kbd>p</kbd> / <kbd>d</kbd> <kbd>p</kbd> / <kbd>e</kbd> <kbd>p</kbd> / <kbd>F12</kbd> | Make, go to, delete and edit a portal; portal window |
| <kbd>P</kbd> <kbd>p</kbd> / <kbd>P</kbd> <kbd>u</kbd> | Write a copy with a password, or without one, [beside the document](features.md#protecting-a-copy-with-a-password) |
| <kbd>t</kbd> <kbd>t</kbd> | Show or hide the hotbar, also while a tool is in hand (the tool stays in hand) |
| <kbd>t</kbd> <kbd>h</kbd> / <kbd>t</kbd> <kbd>d</kbd> / <kbd>t</kbd> <kbd>e</kbd> / <kbd>t</kbd> <kbd>n</kbd> | Highlight, draw, erase, sticky note tool (shows the hotbar) |
| <kbd>t</kbd> <kbd>f</kbd> | Fill & sign tool |
| <kbd>t</kbd> <kbd>T</kbd> / <kbd>D</kbd> / <kbd>C</kbd> / <kbd>X</kbd> / <kbd>O</kbd> / <kbd>L</kbd> / <kbd>S</kbd> | Fill & sign: text, date, check, cross, dot, line, signature |
| <kbd>a</kbd> <kbd>n</kbd> / <kbd>T</kbd> / <kbd>D</kbd> / <kbd>C</kbd> / <kbd>X</kbd> / <kbd>O</kbd> / <kbd>L</kbd> / <kbd>S</kbd> | Place a sticky note, text, date, check, cross, dot, line or signature by grid hint |
| <kbd>1</kbd>-<kbd>5</kbd>, <kbd>[</kbd> <kbd>]</kbd> | With a tool: colour, thinner, thicker (fill & sign: smaller, larger) |
| <kbd>d</kbd> <kbd>h</kbd> | Delete an annotation by hint |
| <kbd>u</kbd> / <kbd>U</kbd> | Undo / redo |
| <kbd>T</kbd> | Next theme |
| <kbd>g</kbd> <kbd>l</kbd> / <kbd>g</kbd> <kbd>i</kbd> / <kbd>Ctrl</kbd>+<kbd>S</kbd> / <kbd>Alt</kbd>+<kbd>F</kbd> | Tint on links / form fields / status line / fullscreen |
| <kbd>></kbd> / <kbd><</kbd> | Turn the pages a quarter turn clockwise / counter-clockwise (a count turns more); [kept for the document](configuration.md#what-is-stored-where) |
| <kbd>D</kbd> / <kbd>g</kbd> <kbd>D</kbd> | Show pages side by side in pairs / with the first page alone on the right, as in a printed book; [kept for the document](configuration.md#what-is-stored-where). Page keys, `:N` and the fits then work by the row |
| (unbound) | `toggle_recolor`: recolour the pages in the theme's colours or show them as they are, over the theme's `recolor` until the next toggle and across theme switches; not saved. Also `:recolor [on\|off]` |
| <kbd>F5</kbd> | [Presentation mode](features.md#presentation-mode) |
| <kbd>Ctrl</kbd>+<kbd>T</kbd> | Open a file with the desktop's file chooser |
| <kbd>R</kbd> / <kbd>:</kbd> / <kbd>q</kbd>, <kbd>Z</kbd> <kbd>Q</kbd>, <kbd>Z</kbd> <kbd>Z</kbd>, <kbd>Ctrl</kbd>+<kbd>W</kbd> | Reload / command line / quit |
| <kbd>Esc</kbd>, <kbd>Ctrl</kbd>+<kbd>C</kbd> | Cancel: closes a bar, hints, the contents or a choice panel; otherwise takes back the last thing: keys, overview, jump, portal, selection, tool, search. Rebind it as `cancel` |

Some actions have more default keys than shown, such as the arrow keys,
<kbd>PageUp</kbd> and <kbd>PageDown</kbd>, <kbd>Home</kbd> and <kbd>End</kbd>.
[`src/default_config.toml`](../src/default_config.toml) lists them all.

## Commands and the `:` menu

| Command | Action |
| --- | --- |
| `:N` | Go to page N |
| `:p LABEL`, `:page LABEL` | Go to the page printed with that label (`xii`, `A-3`), ignoring case; if several pages share it, the next one after this page, wrapping around. With no label, shows this page's label |
| `:q`, `:quit`, `:q!`, `:wq`, `:x` | Quit |
| `:w`, `:write` | Save the position, marks, bookmarks, jump history, portals and annotations |
| `:e FILE`, `:edit FILE`, `:open [FILE]` | Open a file; `:open` with none shows the file chooser |
| `:noh`, `:nohlsearch` | Clear the search highlight |
| `:theme NAME` | Switch theme; with an unknown name it lists them |
| `:zoom PERCENT` | Set the zoom |
| `:reload` | Reload the document |
| `:rotate [90\|180\|270\|0\|cw\|ccw]` | Turn all the pages clockwise to that angle, or a quarter turn; with no argument, clockwise. Rotation is a setting of the view, kept per document: the PDF's own `/Rotate` is never changed, not even by `:export` |
| `:twopage [on\|off]` | Show pages in pairs (the config's `two_page`), or one at a time; with no argument, switch. Kept per document |
| `:recolor [on\|off]` | Recolour the pages in the theme's colours, or show them as they are; with no argument, switch. Overrides the theme's `recolor` until the next toggle, and is not saved |
| `:cover [on\|off]` | With two pages, keep the first page alone on the right (`cover_alone`); with no argument, switch. Kept per document, and set even when not in two-page mode |
| `:toc` | Open the table of contents |
| `:hotbar` | Show or hide the hotbar |
| `:recent` | List the recent files |
| `:info` | Show the document's properties in a panel: the file and its size, the page count, the size of the current page (in points and millimetres, with its paper name such as A4 or Letter when it is close to one), the PDF version, the title, author, subject, keywords, creator, producer and dates, and whether it is encrypted, has a form, an outline, digital signatures or tags. Rows with nothing to show are left out. <kbd>j</kbd> <kbd>k</kbd> or the wheel scroll it when it is taller than the window; <kbd>Esc</kbd>, <kbd>Ctrl+C</kbd> or <kbd>q</kbd> close it |
| `:present` | Start or leave [presentation mode](features.md#presentation-mode) |
| `:marks` | List the marks, and count the bookmarks |
| `:bookmark NAME`, `:bm NAME` | Add a named bookmark at this place; an existing name, in any case, moves to it |
| `:bookmarks` | Pick a bookmark: type <kbd>/</kbd> to filter, <kbd>j</kbd> <kbd>k</kbd> or the arrows and the wheel move, <kbd>Enter</kbd>, <kbd>l</kbd> or a click jumps, <kbd>d</kbd> or <kbd>Delete</kbd> deletes when not filtering, <kbd>Esc</kbd> clears the filter and then closes, <kbd>q</kbd> closes. Names match without regard to case. They are listed in the order of the document, with their page |
| `:sign` | Draw a new signature |
| `:annotations` | The same as <kbd>g</kbd> <kbd>a</kbd>: list the annotations in the order of the document, each with its page label, kind, colour and text |
| `:export-notes [FILE]`, `:export-notes! [FILE]` | Write the highlights and notes to a Markdown file, `{name}-notes.md` beside the document unless given a path; `!` replaces an existing file |
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

## Editing text

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
| <kbd>Ctrl</kbd>+<kbd>Z</kbd> | Clear the [password](features.md#password-protected-pdfs) field; it does nothing in the other fields |
| <kbd>Ctrl</kbd>+<kbd>P</kbd> <kbd>Ctrl</kbd>+<kbd>N</kbd> | In the <kbd>/</kbd> and <kbd>?</kbd> bars, recall the older or newer search; in the `:` bar they move the menu |

The `edit_*` actions rebind them (`edit_history_prev` and `edit_history_next` for
<kbd>Ctrl</kbd>+<kbd>P</kbd> and <kbd>Ctrl</kbd>+<kbd>N</kbd>).

Every search run with <kbd>Enter</kbd> in the <kbd>/</kbd> or <kbd>?</kbd> bar is
remembered, up to 100, without repeats. <kbd>Ctrl</kbd>+<kbd>P</kbd> puts the
next older one in the field and <kbd>Ctrl</kbd>+<kbd>N</kbd> the next newer one;
past the newest comes back what you had typed. As in vim, text typed before the
first <kbd>Ctrl</kbd>+<kbd>P</kbd> limits the choice to the searches that start
with it. Editing the field ends the browsing.
