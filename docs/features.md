# Features

Part of the [yavcpdf](../README.md) documentation.

## Presentation mode

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

## Hints

Hints cover the current page only (the one the status bar shows), the part of
it showing, however far out you zoom. <kbd>v</kbd> labels every word there. Type a label for the first word, then
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
the first pair in turn. Every key sits centred in a cell of the same width.

Labels shrink as you zoom out, and further where the words on screen are too
close together for them: a label is kept no taller than its word and no wider
than the room to the next word, ignoring the few tightest words. A label that
is wider than its word moves right out of the one before it, but never onto the
next word; at the smallest size, crowded labels overlap instead.
`hints.font_size` is the size in points at 100% zoom and the largest a label
gets before `hints.scale_factor` is applied, unless `hints.min_font_size` is
larger. `hints.scale = false` keeps every label at `font_size` times
`scale_factor`, floored at `min_font_size`. `hints.min_font_size` (4 by default)
is the smallest size a label is ever shown at. `hints.scale_factor` (1.0 by
default) multiplies the label size, after the fitting and before that floor is
applied. The limits on height and width above hold before the factor is
applied, so a factor above 1 can make labels overlap neighbouring words.

## Smart jump

<kbd>g</kbd> <kbd>d</kbd> labels the references on the current page, whether or not
the PDF made them links: `Figure 3`, `Table 2`, `Section 3.1`, `Theorem 1.2`,
`Eq. (4)`, `(4)`, `[12]`, `Smith et al., 2020`. Pick one to go to the caption,
heading, equation or bibliography entry it names, by reading the text of the
whole document. It lands where `view.jump_position` says, the middle of the
window by default. <kbd>Alt</kbd>+<kbd>O</kbd> comes back.

> [!NOTE]
> It is a good guess rather than a certainty: a reference split over two lines
> isn't seen, and a scanned document has no text to read.

**Where is this referred to?** <kbd>g</kbd> <kbd>r</kbd> is smart jump the other
way round. It labels what the current page defines: figure and table captions,
numbered and theorem-like headings, equation numbers and bibliography entries.
Pick one and every reference elsewhere in the document that smart jump would
take to it is highlighted like a search match, and the view goes to the first
one after where you are, at `view.jump_position`, so <kbd>Alt</kbd>+<kbd>O</kbd>
comes back. <kbd>n</kbd> and <kbd>N</kbd> step through them, the status line
says `Figure 3: 2/5 references` or `no references to Figure 3`, and
<kbd>Esc</kbd> or `:noh` clears them, as for a search. The caption or entry
itself is not counted. It needs the text of every page, so it waits until
that has been read, saying `reading the text of the pages…`; <kbd>Esc</kbd> stops
the wait. Reloading the document drops the highlighted references.

## Overview

Shows what a reference points at in a window over the page, so you can read a
figure or a citation without losing your place. It finds what smart jump finds, on the current page.
While it is open, the vertical scrolling keys (<kbd>j</kbd> <kbd>k</kbd>, the
half-page and the screen keys) move it, as does the wheel over it. <kbd>Enter</kbd> or a click on it goes there,
<kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>PageDown</kbd> and
<kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>PageUp</kbd> step through other candidates
for the reference, and <kbd>Esc</kbd>, <kbd>Ctrl</kbd>+<kbd>C</kbd>, a click
beside it closes it. Any other key closes it and then does its usual
action, so <kbd>h</kbd> and <kbd>l</kbd> close it and scroll the page. `[overview]` sets its size and place.

## Table of contents

It comes from the PDF's bookmarks. A PDF with none, such as a Google Docs
export, gets one from its printed contents pages when their entries are links:
the titles are the text of those links, and their indents are the levels. A PDF
with neither has no table of contents. An entry that names a spot on its page
lands there at `view.jump_position`; one that names only the page lands at its top.

## Annotations

Choose a tool from the hotbar keys: <kbd>t</kbd> <kbd>h</kbd> highlights,
<kbd>t</kbd> <kbd>d</kbd> draws, <kbd>t</kbd> <kbd>e</kbd> erases and
<kbd>t</kbd> <kbd>n</kbd> adds sticky notes. They are kept beside the PDF, not
in it; see [What is stored where](configuration.md#what-is-stored-where).

**The list.** <kbd>g</kbd> <kbd>a</kbd> (or `:annotations`) lists every annotation in the order of
the document, by page and then top to bottom. A row shows the page label, a swatch of the
colour, the kind and the text: a highlight's text with its lines joined and
its hyphenation undone, as copying does, a note's text, or the text of fill & sign text and dates;
ink and other marks show just the kind. <kbd>/</kbd> filters by that text. Enter, <kbd>l</kbd> or a
click jumps there, at `view.jump_position`, through the jump history. <kbd>d</kbd> or <kbd>Delete</kbd>
deletes the highlighted one, as one step that <kbd>u</kbd> takes back.

**Sticky notes.** With <kbd>t</kbd> <kbd>n</kbd>, click a place and type the
note in the bar. Its text shows when the pointer is on it, and all of them show
while the tool is in hand. They also appear among the <kbd>F</kbd> link hints; picking one pins its text
until you press <kbd>Esc</kbd> or click elsewhere. Click a note to edit it.

**Notes in the PDF.** The comments a PDF carries (sticky notes, free text and
other markup with text) show their author and text, read-only, when the pointer
is on them. Click one, or pick it with <kbd>F</kbd>, to keep it shown until a click
elsewhere or <kbd>Esc</kbd>. A click on a link under a note follows the link. Replies and
statuses are shown in the popup of the note they answer. They are never written to the
sidecar or exported again; the hover popups are not drawn while the draw, erase, note or
fill tool is in hand.

**Fill & sign.** Pick an item and click where it goes, or press <kbd>v</kbd>
and pick a word to put it just after that word. The first signature asks you to
draw one on a pad and is kept for every document in
`signature.json` in the data folder (see the
[Settings folders table](../README.md#installation) under Installation), readable by you alone; `:sign` draws a
new one. On the pad, draw with the mouse; the undo key (<kbd>u</kbd> by default)
removes the last stroke, <kbd>Enter</kbd> keeps the signature and <kbd>Esc</kbd>
cancels. Move something by undoing it and placing it again.

Items can also be placed with the keyboard alone, without a tool in hand:
<kbd>a</kbd> <kbd>n</kbd> (note), <kbd>a</kbd> <kbd>T</kbd> (text), <kbd>a</kbd> <kbd>D</kbd>
(date), <kbd>a</kbd> <kbd>C</kbd>, <kbd>a</kbd> <kbd>X</kbd>, <kbd>a</kbd> <kbd>O</kbd>,
<kbd>a</kbd> <kbd>L</kbd> (check, cross, dot, line) and <kbd>a</kbd> <kbd>S</kbd> (signature)
cut the visible part of the current page into a grid of cells, each with a hint label
(`hints.grid_size`, in page points, 50 by default). Type a label to put a preview
of the item at the centre of that cell. Then <kbd>h</kbd> / <kbd>l</kbd> /
<kbd>k</kbd> / <kbd>j</kbd> move the preview left, right, up and down by `hints.place_step`
page points (4 by default), never off the page, and <kbd>[</kbd> / <kbd>]</kbd> make the
preview smaller or larger by changing the fill size (a note is a fixed size); they are the `place_left`, `place_right`,
`place_up` and `place_down` actions, which work only while placing, so they don't clash
with the same keys elsewhere. <kbd>Enter</kbd> places the item as a click would
(a note or text asks for its text, a signature for the pad if none is saved yet),
and <kbd>Esc</kbd> or <kbd>Ctrl</kbd>+<kbd>C</kbd> leaves without placing.

## Forms

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

Picking a **signature field** (or clicking it) previews your saved signature in
it: scaled to fit the field but never larger than a signature placed by hand at
the current fill size, left-aligned and sitting on the bottom of the field.
<kbd>h</kbd> / <kbd>l</kbd> / <kbd>k</kbd> / <kbd>j</kbd> move the preview,
<kbd>[</kbd> / <kbd>]</kbd> make it smaller or larger (the fill size), <kbd>Enter</kbd>
signs exactly as shown and <kbd>Esc</kbd> cancels, adding nothing. The first time, with
none saved, the signature pad opens (as for fill & sign) and finishing it shows the preview. Picking a field with a signature of
yours mostly inside it, including one placed by hand with the fill tool,
replaces that signature, and one undo brings the old one back. Read-only
signature fields can't be picked, and one that the PDF has already signed is
refused. The signature is kept in the
sidecar like other fill & sign marks and leaves the viewer through `:export`;
it is a drawing, not a cryptographic signature.

## Portals

Press <kbd>p</kbd> where you are reading, go to the target, press <kbd>p</kbd>
again. The view returns, and the portal window shows the target of the portal
nearest the view.

## Opening files

<kbd>Ctrl</kbd>+<kbd>T</kbd> or `:open` shows the desktop's file chooser,
through the [XDG portal](https://flatpak.github.io/xdg-desktop-portal/) when
there is one, so a portal backend such as `xdg-desktop-portal-termfilechooser`
gets used. It says "no file chosen" if the chooser is cancelled, and also if
none could be shown.

## Password-protected PDFs

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

## Protecting a copy with a password

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

## Exporting annotations and forms

`:export-notes` writes the highlights and notes, and only those, to a Markdown file:
`{name}-notes.md` beside the document unless given a path, never replacing a file
without `:export-notes!`. It has a `# ` heading with the document's title (or its file
name), a `## Page` heading with the page's label for each page that has any, each
highlight as a `>` quote and each note as plain text after it. A note on or right next
to a highlight goes under that highlight. Ink and fill & sign marks are left out; with no
highlight or note nothing is written. A highlight keeps its text with it, so no page
has to be loaded first.

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
