#!/bin/sh
# hubcodes installer for macOS (Apple silicon) and Linux (x86_64, aarch64;
# also Windows through WSL 2, see install.ps1).
#
#   gh api -H "Accept: application/vnd.github.raw" repos/HubStudio-id/hubcodes/contents/install.sh | sh
#
# macOS: puts hubcodes.app in /Applications (or $HUBCODES_APP_DIR) and links
# the bundled CLI to ~/.local/bin/hubcodes.
# Linux: puts the CLI in ~/.local/bin/hubcodes, the desktop app (an AppImage,
# which updates itself) in ~/.local/bin/hubcodes-desktop, and a menu entry in
# ~/.local/share/applications. HUBCODES_NO_DESKTOP=1 installs the CLI only.
#
# Needs curl and tar, and tmux to run agents. A private repo needs a
# logged-in `gh` (or GH_TOKEN) so the release assets can be fetched.
set -eu

REPO="${HUBCODES_REPO:-HubStudio-id/hubcodes}"
APP_DIR="${HUBCODES_APP_DIR:-/Applications}"
BIN_DIR="${HUBCODES_BIN_DIR:-$HOME/.local/bin}"
VERSION="${HUBCODES_VERSION:-latest}"

say() { printf '\033[1m%s\033[0m\n' "$*"; }
die() { printf 'install: %s\n' "$*" >&2; exit 1; }

os="$(uname -s)"
arch="$(uname -m)"
case "$os/$arch" in
  Darwin/arm64) ;;
  Darwin/*) die "only an Apple silicon build is published for macOS" ;;
  Linux/x86_64 | Linux/amd64) arch=x86_64 ;;
  Linux/aarch64 | Linux/arm64) arch=aarch64 ;;
  *) die "no build for $os/$arch (macOS on Apple silicon, Linux x86_64 or aarch64, Windows through WSL 2)" ;;
esac
command -v curl >/dev/null || die "curl is required"
command -v tar >/dev/null || die "tar is required"
if ! command -v tmux >/dev/null; then
  if [ "$os" = Darwin ]; then say "tmux not found: install it with  brew install tmux  before starting agents"
  else say "tmux not found: install it with  sudo apt install tmux  (or your distribution's package manager) before starting agents"; fi
fi

# Auth for private repos: GH_TOKEN, else a logged-in gh.
token="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
if [ -z "$token" ] && command -v gh >/dev/null 2>&1; then
  token="$(gh auth token 2>/dev/null || true)"
fi
auth=""
[ -n "$token" ] && auth="Authorization: Bearer $token"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
use_gh=0
command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1 && use_gh=1

# The asset this machine installs from: it decides which release fits.
if [ "$os" = Darwin ]; then need=hubcodes.app.tar.gz; else need="hubcodes-cli-linux-$arch.tar.gz"; fi

# The release: its tag and asset list. "latest" is the newest published
# release that has this machine's build: the Linux builds are attached by CI
# some minutes after the Mac release, so meanwhile Linux gets the one before.
if [ "$use_gh" = 1 ]; then
  if [ "$VERSION" = latest ]; then
    tag="$(gh api "repos/$REPO/releases?per_page=15" --jq "[.[] | select((.draft or .prerelease) | not) | select(any(.assets[]; .name == \"$need\"))][0].tag_name // empty")" || die "no release found in $REPO (no access? ask a repository admin)"
    [ -n "$tag" ] || die "no release in $REPO has $need yet"
  else
    tag="$VERSION"
  fi
else
  command -v python3 >/dev/null || die "needs python3 (or install gh and run: gh auth login)"
  if [ "$VERSION" = latest ]; then
    curl -fsSL ${auth:+-H "$auth"} -H "Accept: application/vnd.github+json" "https://api.github.com/repos/$REPO/releases?per_page=15" -o "$tmp/releases.json" || die "could not read the releases (private repo? run: gh auth login, or set GH_TOKEN)"
    python3 -c '
import json, sys
rels, need, out = json.load(open(sys.argv[1])), sys.argv[2], sys.argv[3]
fit = [r for r in rels if not r.get("draft") and not r.get("prerelease") and any(a["name"] == need for a in r.get("assets", []))]
if not fit:
    sys.exit("no release has " + need + " yet")
json.dump(fit[0], open(out, "w"))' "$tmp/releases.json" "$need" "$tmp/release.json" || die "no release in $REPO has $need yet"
  else
    curl -fsSL ${auth:+-H "$auth"} -H "Accept: application/vnd.github+json" "https://api.github.com/repos/$REPO/releases/tags/$VERSION" -o "$tmp/release.json" || die "could not read release $VERSION (private repo? run: gh auth login, or set GH_TOKEN)"
  fi
  tag="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["tag_name"])' "$tmp/release.json")"
fi
v="${tag#v}"

# fetch <asset name> <file>: one release asset into a file.
fetch() {
  if [ "$use_gh" = 1 ]; then
    gh release download "$tag" -R "$REPO" -p "$1" -O "$2" --clobber || die "release $tag has no $1"
  else
    id="$(python3 -c 'import json,sys; print(next((a["id"] for a in json.load(open(sys.argv[1]))["assets"] if a["name"] == sys.argv[2]), ""))' "$tmp/release.json" "$1")"
    [ -n "$id" ] || die "release $tag has no $1"
    curl -fsSL ${auth:+-H "$auth"} -H "Accept: application/octet-stream" "https://api.github.com/repos/$REPO/releases/assets/$id" -o "$2"
  fi
}

say "hubcodes: installing $tag from $REPO"
mkdir -p "$BIN_DIR"

if [ "$os" = Darwin ]; then
  fetch "$need" "$tmp/app.tar.gz"
  tar -xzf "$tmp/app.tar.gz" -C "$tmp"
  [ -d "$tmp/hubcodes.app" ] || die "archive did not contain hubcodes.app"
  if [ -d "$APP_DIR/hubcodes.app" ]; then
    if pgrep -f "hubcodes.app/Contents/MacOS/hubcodes-desktop" >/dev/null 2>&1; then
      say "hubcodes: the app is running, quitting it first"
      osascript -e 'tell application "hubcodes" to quit' >/dev/null 2>&1 || pkill -f "hubcodes.app/Contents/MacOS/hubcodes-desktop" || true
      sleep 1
    fi
    rm -rf "$APP_DIR/hubcodes.app"
  fi
  mv "$tmp/hubcodes.app" "$APP_DIR/hubcodes.app"
  # The build is signed but not notarized yet, so clear the quarantine flag the
  # download added; otherwise Gatekeeper refuses to open it.
  xattr -dr com.apple.quarantine "$APP_DIR/hubcodes.app" 2>/dev/null || true
  ln -sf "$APP_DIR/hubcodes.app/Contents/MacOS/hubcodes" "$BIN_DIR/hubcodes"
  open_cmd="open -a hubcodes"
else
  fetch "$need" "$tmp/cli.tar.gz"
  tar -xzf "$tmp/cli.tar.gz" -C "$tmp"
  [ -f "$tmp/hubcodes" ] || die "archive did not contain hubcodes"
  # Replace, not overwrite: a running daemon keeps its old file.
  install -m 755 "$tmp/hubcodes" "$BIN_DIR/.hubcodes.new" && mv -f "$BIN_DIR/.hubcodes.new" "$BIN_DIR/hubcodes"
  open_cmd="hubcodes --demo"
  if [ "${HUBCODES_NO_DESKTOP:-0}" != 1 ]; then
    fetch "hubcodes_${v}_$arch.AppImage" "$tmp/hubcodes.AppImage"
    pkill -f "$BIN_DIR/hubcodes-desktop" 2>/dev/null || true
    install -m 755 "$tmp/hubcodes.AppImage" "$BIN_DIR/.hubcodes-desktop.new" && mv -f "$BIN_DIR/.hubcodes-desktop.new" "$BIN_DIR/hubcodes-desktop"
    data="${XDG_DATA_HOME:-$HOME/.local/share}"
    apps="$data/applications"
    mkdir -p "$apps"
    # The menu icon comes out of the AppImage itself (no FUSE needed to extract).
    icon_line=""
    if (cd "$tmp" && "$BIN_DIR/hubcodes-desktop" --appimage-extract hubcodes.png >/dev/null 2>&1) && [ -s "$tmp/squashfs-root/hubcodes.png" ]; then
      icons="$data/icons/hicolor/256x256/apps"
      mkdir -p "$icons"
      cp "$tmp/squashfs-root/hubcodes.png" "$icons/hubcodes.png" && icon_line="Icon=hubcodes"
    fi
    cat > "$apps/hubcodes.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=hubcodes
Comment=Canvas for orchestrating AI coding agents on tmux
Exec=$BIN_DIR/hubcodes-desktop
$icon_line
Terminal=false
Categories=Development;
DESKTOP
    open_cmd="hubcodes-desktop"
    if ! ldconfig -p 2>/dev/null | grep -q libfuse.so.2; then
      say "hubcodes: the AppImage needs FUSE 2 (sudo apt install libfuse2, or libfuse2t64 on Ubuntu 24.04);"
      say "          without it run:  hubcodes-desktop --appimage-extract-and-run"
    fi
  fi
fi

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) say "hubcodes: add $BIN_DIR to PATH (for example in ~/.zshrc or ~/.bashrc):  export PATH=\"$BIN_DIR:\$PATH\"" ;;
esac

say "hubcodes $tag installed."
printf '  %-27s # the canvas\n' "$open_cmd"
printf '  hubcodes setup              # skills in every coding tool, then a health check\n'
printf '  hubcodes spawn lead -p claude --cwd ~/code/app\n'
