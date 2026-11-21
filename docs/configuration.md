# Configuration

Part of the [yavcpdf](../README.md) documentation.

`config.toml` in the config folder (`~/.config/yavcpdf/` on Linux, other systems:
see the [Settings folders table](../README.md#installation) under Installation),
reloaded while the viewer runs. Put only what you want to change in it:
[`src/default_config.toml`](../src/default_config.toml) holds every setting with
its default. The AUR packages install it as
`/usr/share/doc/yavcpdf/config.toml`, and the Nix package as
`share/doc/yavcpdf/config.toml` in its output (with `nix profile`,
`~/.nix-profile/share/doc/yavcpdf/config.toml`). A short example:

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

## Searching the web

Select text (for example with <kbd>v</kbd>) and press <kbd>s</kbd> <kbd>s</kbd> to
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

## Key bindings

Keys are written as in vim, for example `j`, `gg`, `<C-f>`, `<A-o>`, `<S-space>`
or `<esc>`.

> [!IMPORTANT]
> A list under `[keys]` **replaces** all of that action's default keys.

Esc and <kbd>Ctrl</kbd>+<kbd>C</kbd> are the `cancel` action. The `edit_*`
actions (see [Editing text](keys.md#editing-text)) apply in the bars, the contents and
choice filters and the password field only, and a key that types a character can't be used for them.

## Themes

Colours are in the themes, and one key is enough to change one. `dark` and
`light` ship by default; `theme` names the one in use, <kbd>T</kbd> cycles
through them and `:theme NAME` picks one. `background` is what shows around and
between the pages, and `page` is the sheet itself. `recolor = false` shows the
document's own colours.

`toggle_recolor` (or `:recolor [on|off]`) overrides `recolor` at run time. It has
no key by default: bind it under `[keys]`, e.g. `toggle_recolor = ["gR"]`. The
choice holds across `T` and `:theme` switches and until the next toggle; it is
not saved in the PDF or the sidecar, and a new session starts from the theme again.

## What is stored where

Position, marks, bookmarks, the jump history, the rotation, the two-page layout and cover setting, portals, annotations and the values filled into a form are kept in
`{hash}.json` in the data folder (`~/.local/share/yavcpdf/` on Linux, other
systems: see the [Settings folders table](../README.md#installation) under Installation).
The PDF itself is never changed, so other viewers don't show them. A file keeps
its data when it moves, and when it is rebuilt at the same path.

The searches typed in the `/` and `?` bars are shared by all documents, in
`search_history` in the same folder, readable by you alone; delete the file to
forget them.
