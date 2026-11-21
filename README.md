<div align="center">
  <h1>yavcpdf</h1>
  <p>A minimal, keyboard-driven PDF viewer</p>

  [![Linux](https://img.shields.io/badge/Linux-131415?style=for-the-badge&logo=linux&logoColor=white)](#installation)
  [![Windows](https://img.shields.io/badge/Windows-131415?style=for-the-badge&logo=data:image/svg%2bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCI%2BPHBhdGggZmlsbD0iI2ZmZmZmZiIgZD0iTTAgMGgxMXYxMUgwek0xMyAwaDExdjExSDEzek0wIDEzaDExdjExSDB6TTEzIDEzaDExdjExSDEzeiIvPjwvc3ZnPg==)](#installation)
  [![macOS](https://img.shields.io/badge/macOS-131415?style=for-the-badge&logo=apple&logoColor=white)](#installation)
  [![Build status](https://img.shields.io/github/actions/workflow/status/araaha/yavcpdf/release.yml?style=for-the-badge&logo=github&logoColor=white&label=build)](https://github.com/araaha/yavcpdf/actions/workflows/release.yml)
  [![Latest release](https://img.shields.io/github/v/release/araaha/yavcpdf?style=for-the-badge&logo=github&logoColor=white)](https://github.com/araaha/yavcpdf/releases)
  [![Nix flake](https://img.shields.io/badge/Nix-flake-131415?style=for-the-badge&logo=nixos&logoColor=white)](#package-managers)
  [![AUR version](https://img.shields.io/aur/version/yavcpdf?style=for-the-badge&logo=archlinux&logoColor=white)](https://aur.archlinux.org/packages/yavcpdf)
  [![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue?style=for-the-badge)](LICENSE-MIT)

  <a href="#installation">Install</a> · <a href="#usage">Usage</a> · <a href="#features">Features</a> · <a href="docs/keys.md">Keys</a> · <a href="docs/configuration.md">Configuration</a> · <a href="#documentation">Docs</a>
</div>

yavcpdf is a PDF viewer you drive from the keyboard. The whole window is the
document, with a status line at the bottom. It uses PdfCraft's render engine,
and its annotation and form writers for `:export`.

https://github.com/user-attachments/assets/9895c9b9-81cb-4ec5-91cd-6c0546bdb202

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
    * [First keys](#first-keys)
* [Features](#features)
* [Documentation](#documentation)
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

<details>
<summary><b>Nix</b></summary>
<br>

The repository is a flake that builds yavcpdf from source for x86_64 and
aarch64 Linux. Try it without installing:

```sh
nix run github:araaha/yavcpdf -- file.pdf
```

Install it into your profile:

```sh
nix profile install github:araaha/yavcpdf
```

On NixOS or with Home Manager, add the flake as an input and use its package
(or `overlays.default`, which adds `pkgs.yavcpdf`):

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    yavcpdf.url = "github:araaha/yavcpdf";
    yavcpdf.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { nixpkgs, yavcpdf, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ({ pkgs, ... }: {
          environment.systemPackages = [ yavcpdf.packages.${pkgs.stdenv.hostPlatform.system}.default ];
        })
      ];
    };
  };
}
```

For Home Manager, put the same package in `home.packages`. Releases after
0.7.0 include the flake: append a tag, as in `github:araaha/yavcpdf/vX.Y.Z`,
to pin one.

The package installs `yavcpdf.desktop`, the icon, and the reference
configuration at `share/doc/yavcpdf/config.toml`. The binary is wrapped so it
finds the OpenGL, Wayland, X11, xkbcommon and dbus libraries it loads at run
time, and `fc-match` and `xdg-open`. The file chooser still needs
`xdg-desktop-portal` with a backend, which NixOS sets up with
`xdg.portal.enable`.

On other distributions, the OpenGL from Nix can't find your system's GPU
drivers, which NixOS keeps in `/run/opengl-driver`, and yavcpdf stops with
"Found no glutin configs". Start it through
[nixGL](https://github.com/nix-community/nixGL):

```sh
nix run --impure github:nix-community/nixGL -- yavcpdf file.pdf
```

or install yavcpdf another way on those systems.

`nix develop` opens a shell with Rust, clippy, rustfmt and rust-analyzer, in
which `cargo run` works.
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
install -Dm644 assets/yavcpdf.desktop ~/.local/share/applications/yavcpdf.desktop
xdg-mime default yavcpdf.desktop application/pdf    # to make it the default
```
</details>

Usage
-----

```sh
yavcpdf [OPTIONS] [FILE]
```

With no file, yavcpdf reopens the last document. If that file is gone or won't open (the reason is in the status bar), or there is none, it opens a blank page instead, where `:open` or `:e` open a file; a file named on the command line that won't open is an error. The blank page lists the recent files that still exist: type a row's label, or move with j/k and press Enter, or click; Esc closes the list and `:recent` shows it again, over a document too. It reloads the PDF when the
file changes. A password-protected PDF opens a [password menu](docs/features.md#password-protected-pdfs). On Windows, and on macOS when not started from a terminal,
`--help` and argument errors show in a dialog, as do fatal errors. An argument error prints only the error and the usage line; `--help` has the full text.

| Option | Effect |
| --- | --- |
| `--page N` | Open at page N |
| `--password PW` | The password of an encrypted PDF; without it, or if it is wrong, a menu asks for the password |
| `--geometry WxH[+X+Y]` | Set the window's inner size in points, and optionally its position |
| `--fullscreen` / `--no-fullscreen` | Override `view.fullscreen` for this launch |
| `--statusbar` / `--no-statusbar` | Override `statusbar.visible` for this launch |
| `-h`, `--help` | Print the options and the paths of the configuration and data |
| `-V`, `--version` | Print the version |

The last of the fullscreen or the status bar options given wins.
`view.fullscreen` and `statusbar.visible` only take effect at startup; a
configuration reload does not apply them again.

`--geometry 1000x700` sets the size, and `--geometry 1000x700+40+30` also sets
the position. Width and height are 1 to 16384 and the offsets are 0 to
16384. Negative offsets are not supported, so X11's offsets
from the right or bottom edge do not work. On Wayland the compositor decides
where a window goes, so `+X+Y` may be ignored.

### First keys

> [!TIP]
> Press <kbd>:</kbd> to see nearly every action with its keys, and type to filter them.

| Keys | Action |
| --- | --- |
| <kbd>h</kbd> <kbd>j</kbd> <kbd>k</kbd> <kbd>l</kbd> | Scroll |
| <kbd>J</kbd> / <kbd>K</kbd> | Next / previous page |
| <kbd>g</kbd> <kbd>g</kbd> / <kbd>G</kbd> | First page / end; `NG` goes to page N |
| <kbd>=</kbd>, <kbd>+</kbd> / <kbd>-</kbd> / <kbd>W</kbd> / <kbd>H</kbd> | Zoom in / zoom out / fit width / fit height |
| <kbd>/</kbd> / <kbd>?</kbd>, <kbd>n</kbd> / <kbd>N</kbd> | Search forward / backward, next / previous match |
| <kbd>Tab</kbd> | Table of contents |
| <kbd>g</kbd> <kbd>d</kbd> / <kbd>g</kbd> <kbd>o</kbd> | Smart jump / overview, by hint |
| <kbd>F</kbd> / <kbd>v</kbd> / <kbd>i</kbd> | Follow a link / select text / fill in a form field, by hint |
| <kbd>Alt</kbd>+<kbd>O</kbd> / <kbd>Alt</kbd>+<kbd>I</kbd> | Jump back / forward |
| <kbd>t</kbd> <kbd>h</kbd> / <kbd>t</kbd> <kbd>n</kbd> / <kbd>t</kbd> <kbd>f</kbd> | Highlight, sticky note, fill & sign tool |
| <kbd>u</kbd> / <kbd>U</kbd> | Undo / redo |
| <kbd>:</kbd> / <kbd>q</kbd> | Command menu / quit |

Every key, every `:` command and the editing keys of the bars are in
[docs/keys.md](docs/keys.md), and each can be rebound under `[keys]`.

Features
--------

Each feature in a few lines, with a demo. The full description of each is in
[docs/features.md](docs/features.md).

### Marks and bookmarks

<kbd>m</kbd> and a letter mark a place and <kbd>'</kbd> and the letter come back to it. <kbd>M</kbd> names a bookmark and <kbd>g</kbd> <kbd>b</kbd> lists them to jump to, filter or delete. Both are kept per document between sessions.

https://github.com/user-attachments/assets/d4ad8107-5d1f-4269-bbc7-878879eb4962

https://github.com/user-attachments/assets/e64f41f8-5a58-4326-a49c-09cd2027165d

More: [Keys](docs/keys.md#keys)

### Smart jump and references

<kbd>g</kbd> <kbd>d</kbd> follows a reference such as `Figure 3`, `Eq. (4)` or `[12]` to what it names, even where the PDF has no link; the video at the top shows it. <kbd>g</kbd> <kbd>r</kbd> goes the other way: pick a caption, heading, equation or bibliography entry and every place that refers to it is highlighted, for <kbd>n</kbd> and <kbd>N</kbd> to step through.

https://github.com/user-attachments/assets/5249ae5c-1b97-45f0-ab3f-8e2ec25ebaeb

More: [Smart jump](docs/features.md#smart-jump)

### Overview

<kbd>g</kbd> <kbd>o</kbd> shows what a reference points at in a window over the page, so you can read a figure or a citation without losing your place. <kbd>Enter</kbd> goes there and <kbd>Esc</kbd> closes it.

https://github.com/user-attachments/assets/d8c3f2b1-07d9-4972-96e8-dc91ab29a871

More: [Overview](docs/features.md#overview)

### Table of contents

<kbd>Tab</kbd> opens the document's outline, or one read from its printed contents pages when it has none. <kbd>/</kbd> filters it and <kbd>Enter</kbd> goes to the entry.

https://github.com/user-attachments/assets/47991ad4-8362-49a9-80f2-675ffb887e77

More: [Table of contents](docs/features.md#table-of-contents)

### Annotations

Highlight, draw, erase and add sticky notes from the hotbar tools. They are kept beside the PDF, not in it. <kbd>g</kbd> <kbd>a</kbd> lists them, and `:export` writes them into a copy.

https://github.com/user-attachments/assets/46adf676-bf83-4f53-9ee7-2eb02d1cef15

More: [Annotations](docs/features.md#annotations)

### Fill & sign from the keyboard

<kbd>a</kbd> and a letter (<kbd>n</kbd> note, <kbd>T</kbd> text, <kbd>D</kbd> date, <kbd>C</kbd> check, <kbd>X</kbd> cross, <kbd>O</kbd> dot, <kbd>L</kbd> line, <kbd>S</kbd> signature) cut the page into a grid of hints. Pick a cell, nudge the preview with <kbd>h</kbd> <kbd>j</kbd> <kbd>k</kbd> <kbd>l</kbd>, resize it with <kbd>[</kbd> and <kbd>]</kbd>, and press <kbd>Enter</kbd> to place it. The signature is drawn once on a pad and kept for every document.

https://github.com/user-attachments/assets/f3326dfe-927d-43a6-9ab9-dd9c429287dd

https://github.com/user-attachments/assets/3b020986-9fb5-4886-a988-326362a44aff

More: [Annotations](docs/features.md#annotations)

### Forms

<kbd>i</kbd> labels the fields of a PDF form: type into text fields, toggle check boxes, pick radio buttons and drop-down options, press buttons and sign signature fields. The values are kept beside the PDF and `:export` writes them into a copy that stays fillable. Scripts in the form are never run.

https://github.com/user-attachments/assets/c0b22abf-bdee-4225-99f1-4a7270ea46ea

More: [Forms](docs/features.md#forms)

### Portals

Press <kbd>p</kbd> where you are reading, go somewhere else and press <kbd>p</kbd> again: the view comes back, and a small window keeps showing the other place.

https://github.com/user-attachments/assets/2e97555b-a7a8-443b-98f0-642113809e23

More: [Portals](docs/features.md#portals)

### Recent files

With no file given, yavcpdf reopens the last document. `:recent`, or the blank page when there is none, lists the recent files to pick from by label, with <kbd>j</kbd> <kbd>k</kbd> and <kbd>Enter</kbd>, or a click.

https://github.com/user-attachments/assets/f1a00cb7-e6cf-4d49-bc75-b70ea350ffa0

More: [Opening files](docs/features.md#opening-files)

### The `:` menu

<kbd>:</kbd> lists the commands and nearly every action, each with its keys, filtered as you type: letters in order are enough, so `tgl` finds `toggle_links`. Arguments such as file names and themes are completed too.

https://github.com/user-attachments/assets/cf7c2eff-9bc9-48a3-bb73-05820fa3224e

More: [Commands and the `:` menu](docs/keys.md#commands-and-the--menu)

### Themes and live configuration

`config.toml` is reloaded while the viewer runs, so a change shows at once. <kbd>T</kbd> cycles through the themes, which set every colour and whether the pages are recoloured.

https://github.com/user-attachments/assets/51f11062-8efe-4ab2-9886-17e7c245c851

More: [Configuration](docs/configuration.md)

Also: [presentation mode](docs/features.md#presentation-mode) (<kbd>F5</kbd>),
[hints](docs/features.md#hints) for words and links,
[password-protected PDFs](docs/features.md#password-protected-pdfs),
[writing a copy with or without a password](docs/features.md#protecting-a-copy-with-a-password)
and [searching the selected text on the web](docs/configuration.md#searching-the-web).

Documentation
-------------

- [Keys and commands](docs/keys.md): every key, every `:` command and the `:`
  menu, and the editing keys of the bars
- [Features](docs/features.md): presentation mode, hints, smart jump, overview,
  table of contents, annotations, forms, portals, opening files, passwords and
  exporting
- [Configuration](docs/configuration.md): `config.toml`, key bindings, searching
  the web, themes, and what is stored where

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
