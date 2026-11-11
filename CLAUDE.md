# yavcpdf

A minimal, keyboard-driven PDF viewer in Rust (edition 2024): egui/eframe for the window,
PdfCraft for rendering and for writing annotations and forms. One binary crate, no workspace.

## Commands

- Build: `cargo build` (the first build is slow: the dependencies are compiled at opt-level 2)
- Test: `cargo test`, or `cargo test <module>::` for one module, e.g. `cargo test view::`
- Lint: `cargo clippy --all-targets -- -D warnings`
- Format: `cargo fmt` (max width 150, see `rustfmt.toml`)

Run the tests and clippy before calling a change done. CI runs `cargo test --locked`.

## Layout

Everything is in `src/`, one module per file, each opening with a `//!` line saying what it is.
Tests are `#[cfg(test)]` modules at the bottom of the file they test; there is no `tests/` directory.

- `main.rs`, `app.rs`: start-up, the application state and the frame loop
- `keymap.rs`: the `Action` enum, key notation, bindings, counts and multi-key sequences
- `actions.rs`: what each action and `:` command does
- `input.rs`: keyboard and pointer input
- `menu.rs`, `complete.rs`, `field.rs`: the `:` bar: its menu, argument completion, the text field
- `config.rs`, `default_config.toml`: built-in defaults with the user's TOML merged over them
- `doc.rs`, `cache.rs`: an open document and its render workers; rendered pages as textures
- `view.rs`, `draw.rs`, `chrome.rs`: page layout and the view; drawing pages; bars and panels
- `store.rs`: what is kept per document outside the PDF (position, marks, bookmarks, history, annotations)
- `search.rs`, `hint.rs`, `smart.rs`, `overview.rs`, `contents.rs`, `bookmarks.rs`, `recent.rs`,
  `present.rs`, `reflow.rs`: one feature each
- `form.rs`, `export.rs`, `protect.rs`: forms, and writing copies of the PDF

## Rules

- The open PDF is never modified. User state goes in the sidecar (`store.rs`); `:export` and
  the protect commands only ever write a new file.
- `unsafe` is forbidden. Clippy denies `cognitive_complexity` (threshold 30), `too_many_arguments`
  and wildcard imports: split a function rather than allowing the lint.
- Keep logic in pure functions that can be tested without a window, as `menu.rs` and `view.rs` do.
- A new action touches several places: the `Action` enum and `Action::ALL` in `keymap.rs` (a test
  checks they agree), its behaviour in `actions.rs`, a default binding in `default_config.toml`,
  and the README's key tables.
- User-visible behaviour is documented in `README.md`; update it in the same change.
- Never edit the pdfcraft revs in `Cargo.toml` by hand. They must all be the same commit:
  use `scripts/update-pdfcraft.sh [REV]`.

## Commits

One commit per feature, with a plain sentence in the imperative as the subject, e.g.
"Keep the jump history of each document between sessions". Do not commit unless asked.
