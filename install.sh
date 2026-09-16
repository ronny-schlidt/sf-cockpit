#!/bin/sh
# Installs sf-cockpit into ~/.local/bin (override with SF_COCKPIT_BIN_DIR).
#
# The repository is private, so downloads go through the GitHub CLI (gh auth login once):
#   gh api -H "Accept: application/vnd.github.raw" repos/ronny-schlidt/sf-cockpit/contents/install.sh | sh
# From a clone:
#   ./install.sh
#
# It installs the prebuilt binary of the latest release. Without a release, or on an unsupported platform,
# it builds from source with cargo.
set -eu

REPO="${SF_COCKPIT_REPO:-ronny-schlidt/sf-cockpit}"
BIN_DIR="${SF_COCKPIT_BIN_DIR:-$HOME/.local/bin}"
NAME="sf-cockpit"

say() { printf '%s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
fail() { printf 'error: %s\n' "$*" >&2; exit 1; }
has() { command -v "$1" >/dev/null 2>&1; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$BIN_DIR"

build_from() {
  has cargo || fail "cargo not found. Install Rust (macOS: brew install rust, others: https://rustup.rs) and run this again."
  say "Building $NAME from source, this takes a minute"
  cargo build --release --quiet --manifest-path "$1/Cargo.toml"
  install -m 755 "$1/target/release/$NAME" "$BIN_DIR/$NAME"
}

target() {
  case "$(uname -s)/$(uname -m)" in
    Darwin/arm64) echo aarch64-apple-darwin ;;
    Darwin/x86_64) echo x86_64-apple-darwin ;;
    Linux/x86_64) echo x86_64-unknown-linux-gnu ;;
    Linux/aarch64 | Linux/arm64) echo aarch64-unknown-linux-gnu ;;
    *) echo "" ;;
  esac
}

sha256() {
  if has shasum; then shasum -a 256 "$1" | cut -d ' ' -f 1; else sha256sum "$1" | cut -d ' ' -f 1; fi
}

download_release() {
  target=$(target)
  [ -n "$target" ] || return 1
  archive="$NAME-$target.tar.gz"
  if has gh; then
    gh release download --repo "$REPO" --pattern "$archive" --pattern "$archive.sha256" --dir "$tmp" 2>/dev/null || return 1
  else
    url="https://github.com/$REPO/releases/latest/download/$archive"
    curl -fsSL "$url" -o "$tmp/$archive" 2>/dev/null || return 1
    curl -fsSL "$url.sha256" -o "$tmp/$archive.sha256" 2>/dev/null || return 1
  fi
  [ "$(cut -d ' ' -f 1 "$tmp/$archive.sha256")" = "$(sha256 "$tmp/$archive")" ] || fail "checksum mismatch, refusing to install"
  tar -xzf "$tmp/$archive" -C "$tmp"
  install -m 755 "$tmp/$NAME-$target/$NAME" "$BIN_DIR/$NAME"
  if [ "$(uname -s)" = Darwin ]; then
    xattr -d com.apple.quarantine "$BIN_DIR/$NAME" 2>/dev/null || true
  fi
}

script_dir=""
case "$0" in
  */install.sh) script_dir=$(cd "$(dirname "$0")" && pwd) ;;
esac

if [ -n "$script_dir" ] && [ -f "$script_dir/Cargo.toml" ]; then
  build_from "$script_dir"
elif download_release; then
  say "Downloaded the latest release"
else
  if has gh; then
    gh repo clone "$REPO" "$tmp/src" -- --depth 1 --quiet 2>/dev/null \
      || fail "cannot access $REPO. Run 'gh auth login' with an account that can read the repository."
  else
    fail "the GitHub CLI is needed for the private repository. Install it (brew install gh), run 'gh auth login', then run this again."
  fi
  build_from "$tmp/src"
fi

say ""
say "Installed $("$BIN_DIR/$NAME" --version) to $BIN_DIR/$NAME"

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) warn "$BIN_DIR is not on your PATH. Add it: echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.zshrc" ;;
esac

if ! has sf; then
  warn "the Salesforce CLI (sf) is not installed. sf-cockpit needs it: https://developer.salesforce.com/tools/salesforcecli"
fi

say ""
say "Next steps:"
say "  1. Log in to the Dev Hub that owns your package, if you have not yet:"
say "       sf org login web --alias DevHub --set-default-dev-hub"
say "  2. Start sf-cockpit inside your Salesforce project folder (where sfdx-project.json is):"
say "       cd path/to/your/project && $NAME"
say "     If the Dev Hub or package is missing, sf-cockpit opens its Settings tab and asks for it."
say "  Try it without an org: $NAME --demo"
