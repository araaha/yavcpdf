#!/usr/bin/env bash
# Move the PdfCraft engine pinned in Cargo.toml to another commit.
#
#   scripts/update-pdfcraft.sh [REV]
#
# REV is a full 40-character commit of https://github.com/storytold/pdfcraft. Without it, the
# current tip of its main branch is used. The script
#   1. compares our [patch.crates-io] list (with the new rev) with the one in PdfCraft's root
#      Cargo.toml at that rev, and stops with a non-zero exit, printing the difference, if they
#      differ or the fetch fails; nothing is edited before this passes,
#   2. replaces every pdfcraft rev in Cargo.toml,
#   3. re-resolves the git dependencies in Cargo.lock, then runs the tests and clippy.
# If anything fails after Cargo.toml is edited, Cargo.toml and Cargo.lock are put back.
# Running it again with the same rev changes nothing.
set -euo pipefail

repo=https://github.com/storytold/pdfcraft
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

# Crates upstream patches that this viewer does not depend on, so it leaves them out of its own list.
ignored="egui_demo_lib egui_extras egui_inspection egui_kittest egui_system_font_provider"

rev=${1:-}
if [ -z "$rev" ]; then
  rev=$(git ls-remote "$repo" refs/heads/main | cut -f1)
fi
if ! [[ $rev =~ ^[0-9a-f]{40}$ ]]; then
  echo "error: '$rev' is not a full 40-character commit hash" >&2
  exit 2
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/update-pdfcraft.XXXXXX")
saved=0
cleanup() {
  status=$?
  if [ "$status" -ne 0 ] && [ "$saved" = 1 ]; then
    cp "$tmp/Cargo.toml.orig" Cargo.toml
    cp "$tmp/Cargo.lock.orig" Cargo.lock
    echo "error: the script failed; Cargo.toml and Cargo.lock were restored" >&2
  fi
  rm -rf "$tmp"
  exit "$status"
}
trap cleanup EXIT

# 1. Compare the patch lists before touching anything. Each line of a normalised list is
# "name url rev". A `path = "vendor/x"` patch upstream is the same crate as our git patch of pdfcraft
# at the same rev.
normalise() { # $1 = Cargo.toml, $2 = rev to give a vendor/ path
  awk -v repo="$repo" -v rev="$2" '
    /^\[/ { in_patch = ($0 == "[patch.crates-io]"); next }
    in_patch && /^[A-Za-z0-9_-]+ *=/ {
      name = $1
      if (match($0, /path = "vendor\/[^"]*"/)) { print name, repo, rev; next }
      url = ""; r = ""
      if (match($0, /git = "[^"]*"/)) url = substr($0, RSTART + 7, RLENGTH - 8)
      if (match($0, /rev = "[^"]*"/)) r = substr($0, RSTART + 7, RLENGTH - 8)
      if (url == "" || r == "") { print name, "UNSUPPORTED", $0; next }
      print name, url, r
    }' "$1" | sort
}

if [ -n "${UPSTREAM_CARGO_TOML:-}" ]; then
  upstream_toml=$UPSTREAM_CARGO_TOML # for testing the check
else
  git -C "$tmp" init -q up
  if ! git -C "$tmp/up" fetch -q --depth 1 --filter=blob:none "$repo" "$rev"; then
    echo "error: could not fetch $rev from $repo; nothing was changed" >&2
    exit 1
  fi
  git -C "$tmp/up" show "FETCH_HEAD:Cargo.toml" > "$tmp/upstream.toml"
  upstream_toml=$tmp/upstream.toml
fi

# Cargo.toml with every pdfcraft rev replaced, in the dependencies and in [patch.crates-io].
sed -E "s#(git = \"$repo\", rev = \")[0-9a-f]+\"#\1$rev\"#" Cargo.toml > "$tmp/Cargo.toml.new"

normalise "$tmp/Cargo.toml.new" "$rev" > "$tmp/ours"
# Only grep's "no match" (exit 1) is fine here; a failure of awk or sort stops the script.
{ normalise "$upstream_toml" "$rev" | { grep -v -E "^($(echo "$ignored" | tr ' ' '|')) " || [ $? -eq 1 ]; } > "$tmp/theirs"; }

if ! diff -u --label "Cargo.toml (ours)" --label "pdfcraft $rev (upstream)" "$tmp/ours" "$tmp/theirs"; then
  echo >&2
  echo "error: [patch.crates-io] in Cargo.toml differs from PdfCraft's at $rev." >&2
  echo "Change it to match upstream's (see the diff above), then run this script again." >&2
  echo "Cargo.toml and Cargo.lock were not changed." >&2
  exit 1
fi
echo "[patch.crates-io] matches upstream"

# 2. Edit Cargo.toml. From here on a failure restores both files from the copies in $tmp.
cp Cargo.toml "$tmp/Cargo.toml.orig"
cp Cargo.lock "$tmp/Cargo.lock.orig"
saved=1
cp "$tmp/Cargo.toml.new" Cargo.toml # (sed without -i: `sed -i` with no suffix is GNU only)
echo "pdfcraft rev in Cargo.toml: $rev"

# 3. Re-resolve the git revs, then test.
cargo update -p pdfcraft-render -p pdfcraft-annot -p pdfcraft-cos -p pdfcraft-forms
cargo test
cargo clippy --all-targets -- -D warnings
echo "done: pdfcraft $rev"
