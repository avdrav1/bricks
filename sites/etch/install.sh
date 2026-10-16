#!/bin/sh
# Etch installer (ETCH-5, ETCH-6, MAC-7). Installs or updates the latest release for this
# user, without sudo:
#
#   curl -fsSL https://etch.cronkite.cc/install.sh | sh
#   curl -fsSL https://etch.cronkite.cc/install.sh | sh -s -- --uninstall
#
# Linux: puts the binary in ~/.local/bin, its icon and desktop entry in ~/.local/share, its
# licences in ~/.local/share/doc/etch (THIRD-PARTY-RUST.txt: the Rust crates in it), and
# makes it the default for PNG, JPEG, and BMP images (~/.config/mimeapps.list), so
# double-clicking an image opens it in Etch; uninstalling drops those defaults, and every
# other type's stays as it was. GTK 4 comes from the system: if it is missing, this says
# how to install it. A release is a latest.json ({"version", "url", "sha256"}) beside a
# tarball holding one folder with `etch`, dev.cronkite.Etch.desktop, dev.cronkite.Etch.svg,
# and the licence files, as the other apps' are (sites/frame/install.sh).
#
# macOS (Apple Silicon, macOS 14 or later): puts Etch.app, which carries its own GTK, in
# ~/Applications and registers it with Launch Services, so PNG, JPEG, and BMP images list
# it under Open With. Default apps stay the user's choice on macOS (Finder's Get Info,
# Open with, Change All). The tarball is latest.json's "platforms" "macos-arm64" entry.
#
# Either way, the download is checked against the release's sha256 before anything is
# installed.
set -eu

# Where latest.json is: filled in when the page is deployed (scripts/ship.sh etch site).
BASE_URL="${ETCH_DOWNLOAD_URL:-@DOWNLOAD_BASE_URL@}"
APP=etch
ID=dev.cronkite.Etch

BIN_DIR="$HOME/.local/bin"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}"
DESKTOP="$DATA/applications/$ID.desktop"
ICON="$DATA/icons/hicolor/scalable/apps/$ID.svg"
DOC="$DATA/doc/$APP"
MIMEAPPS="$CONFIG/mimeapps.list"
TYPES="image/png image/jpeg image/bmp"

say() { printf '%s\n' "$*"; }
die() { printf 'etch install: %s\n' "$*" >&2; exit 1; }

fetch() { # URL FILE
  if command -v curl >/dev/null 2>&1; then curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then wget -qO "$2" "$1"
  else die "needs curl or wget"; fi
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# A string field of latest.json (one flat object).
field() { sed -n "s/.*\"$1\" *: *\"\([^\"]*\)\".*/\1/p" "$2"; }

refresh_caches() {
  command -v update-desktop-database >/dev/null 2>&1 &&
    update-desktop-database -q "$DATA/applications" 2>/dev/null || true
  command -v gtk-update-icon-cache >/dev/null 2>&1 &&
    gtk-update-icon-cache -q -t "$DATA/icons/hicolor" 2>/dev/null || true
}

# Make APP.desktop the default for each of $TYPES in mimeapps.list, or with no app, drop
# the defaults that point at us (another app's stay). Every other line stays as it is.
set_defaults() { # APP.desktop or empty
  mkdir -p "$CONFIG"
  [ -f "$MIMEAPPS" ] || : > "$MIMEAPPS"
  out="$MIMEAPPS.new.$$"
  awk -v app="$1" -v ours="$ID.desktop" -v types="$TYPES" '
    BEGIN { n = split(types, t, " "); for (i = 1; i <= n; i++) mine[t[i]] = 1 }
    function add() { if (app != "" && !done) { for (i = 1; i <= n; i++) print t[i] "=" app; done = 1 } }
    function flush() { for (; held > 0; held--) print "" }
    /^\[/ { if (section == "[Default Applications]") add(); flush(); section = $0; print; next }
    section == "[Default Applications]" {
      if ($0 == "") { held++; next } # blank lines go after our entries
      split($0, kv, "=")
      if ((kv[1] in mine) && (app != "" || kv[2] == ours)) next
    }
    { flush(); print }
    END {
      if (section == "[Default Applications]") add()
      else if (app != "" && !done) { print "[Default Applications]"; add() }
      flush()
    }' "$MIMEAPPS" > "$out"
  mv "$out" "$MIMEAPPS"
}

uninstall() {
  [ -f "$MIMEAPPS" ] && set_defaults ""
  rm -f "$BIN_DIR/$APP" "$DESKTOP" "$ICON"
  rm -rf "$DOC"
  refresh_caches
  say "Removed Etch. Your images are untouched."
}

# How to get GTK 4 on this system.
gtk_hint() {
  id=""; like=""
  if [ -r /etc/os-release ]; then
    id=$(sed -n 's/^ID=//p' /etc/os-release | tr -d '"')
    like=$(sed -n 's/^ID_LIKE=//p' /etc/os-release | tr -d '"')
  fi
  case " $id $like " in
    *" arch "*) say "  sudo pacman -S gtk4" ;;
    *" fedora "*|*" rhel "*) say "  sudo dnf install gtk4" ;;
    *" ubuntu "*|*" debian "*) say "  sudo apt install libgtk-4-1" ;;
    *" opensuse"*|*" suse "*) say "  sudo zypper install libgtk-4-1" ;;
    *) say "  Install GTK 4 (4.14 or newer) with your package manager." ;;
  esac
}

install_app() {
  [ "$(uname -m)" = x86_64 ] || die "releases are x86_64 only; this is $(uname -m)"
  case "$BASE_URL" in @*) die "no download URL set (ETCH_DOWNLOAD_URL)" ;; esac
  tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
  fetch "$BASE_URL/latest.json" "$tmp/latest.json" || die "cannot reach $BASE_URL/latest.json"
  version=$(field version "$tmp/latest.json")
  url=$(field url "$tmp/latest.json")
  want=$(field sha256 "$tmp/latest.json")
  [ -n "$version" ] && [ -n "$url" ] && [ -n "$want" ] || die "latest.json is not a release"
  say "Downloading Etch $version"
  fetch "$url" "$tmp/release.tar.gz" || die "cannot download $url"
  got=$(sha256 "$tmp/release.tar.gz")
  [ "$got" = "$want" ] || die "checksum mismatch (expected $want, got $got); nothing installed"
  tar -xzf "$tmp/release.tar.gz" -C "$tmp"
  src=""
  for f in "$tmp"/*/"$APP"; do [ -f "$f" ] && src=$f && break; done
  [ -n "$src" ] || die "the release has no $APP binary"
  dir=$(dirname "$src")

  before=""
  [ -x "$BIN_DIR/$APP" ] && before=$("$BIN_DIR/$APP" --version 2>/dev/null | cut -d' ' -f2 || true)
  mkdir -p "$BIN_DIR" "$(dirname "$DESKTOP")" "$(dirname "$ICON")"
  # Replace, never write into, a binary that may be running.
  cp "$src" "$BIN_DIR/.$APP.new" && chmod 755 "$BIN_DIR/.$APP.new" && mv -f "$BIN_DIR/.$APP.new" "$BIN_DIR/$APP"
  cp "$dir/$ID.svg" "$ICON"
  # Its licences and the Rust crates' compiled into it (PKG-6).
  rm -rf "$DOC"; mkdir -p "$DOC"
  for f in LICENSE LICENSE-APACHE THIRD-PARTY-RUST.txt; do
    if [ -f "$dir/$f" ]; then cp "$dir/$f" "$DOC/"; fi
  done
  # The full path, so it starts from a launcher even when ~/.local/bin isn't on its PATH.
  sed "s|^Exec=$APP\$|Exec=$BIN_DIR/$APP|; s|^Exec=$APP |Exec=$BIN_DIR/$APP |" "$dir/$ID.desktop" > "$DESKTOP"
  set_defaults "$ID.desktop"
  refresh_caches

  if [ -n "$before" ] && [ "$before" != "$version" ]; then say "Updated Etch $before -> $version"
  else say "Installed Etch $version"; fi
  say "  $BIN_DIR/$APP; PNG, JPEG, and BMP images open with it"
  case ":$PATH:" in *":$BIN_DIR:"*) ;; *) say "  $BIN_DIR is not on your PATH: start it from your app menu, or add it to PATH" ;; esac

  if ! err=$("$BIN_DIR/$APP" --version 2>&1 >/dev/null); then
    say ""
    say "Etch needs GTK 4, which is missing or too old here ($err)."
    say "Install it, then start Etch:"
    gtk_hint
    exit 2
  fi
}

# macOS (MAC-7). Files curl downloads carry no quarantine flag, so Gatekeeper lets the
# ad-hoc-signed app start without asking.
MAC_APP="$HOME/Applications/Etch.app"
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

# Platform $1's flat object in latest.json $2 ("url":…,"sha256":…), on one line.
platform() { sed -n "s/.*\"$1\" *: *{\([^}]*\)}.*/\1/p" "$2"; }

mac_uninstall() {
  if [ -d "$MAC_APP" ]; then
    "$LSREGISTER" -u "$MAC_APP" 2>/dev/null || true
    rm -rf "$MAC_APP"
  fi
  say "Removed Etch. Your images are untouched."
}

mac_install() {
  [ "$(uname -m)" = arm64 ] || [ "$(sysctl -n hw.optional.arm64 2>/dev/null)" = 1 ] ||
    die "the macOS release is for Apple Silicon Macs (M1 or later); this Mac has an Intel processor"
  os=$(sw_vers -productVersion)
  [ "${os%%.*}" -ge 14 ] || die "needs macOS 14 Sonoma or later; this is macOS $os"
  case "$BASE_URL" in @*) die "no download URL set (ETCH_DOWNLOAD_URL)" ;; esac
  tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
  fetch "$BASE_URL/latest.json" "$tmp/latest.json" || die "cannot reach $BASE_URL/latest.json"
  version=$(field version "$tmp/latest.json")
  [ -n "$version" ] || die "latest.json is not a release"
  platform macos-arm64 "$tmp/latest.json" > "$tmp/mac"
  url=$(field url "$tmp/mac")
  want=$(field sha256 "$tmp/mac")
  [ -n "$url" ] && [ -n "$want" ] || die "Etch $version has no macOS release yet"
  say "Downloading Etch $version for macOS"
  fetch "$url" "$tmp/release.tar.gz" || die "cannot download $url"
  got=$(sha256 "$tmp/release.tar.gz")
  [ "$got" = "$want" ] || die "checksum mismatch (expected $want, got $got); nothing installed"
  mkdir "$tmp/x"
  tar -xzf "$tmp/release.tar.gz" -C "$tmp/x"
  [ -x "$tmp/x/Etch.app/Contents/MacOS/$APP" ] || die "the release has no Etch.app"

  before=""
  if [ -x "$MAC_APP/Contents/MacOS/$APP" ]; then before=$("$MAC_APP/Contents/MacOS/$APP" --version 2>/dev/null | cut -d' ' -f2 || true); fi
  mkdir -p "$HOME/Applications"
  # The whole bundle is replaced; an Etch still running keeps the files it has open.
  rm -rf "$MAC_APP"
  mv "$tmp/x/Etch.app" "$MAC_APP"
  "$LSREGISTER" -f "$MAC_APP" || say "  Launch Services didn't take it now; Finder registers it when it sees it"
  "$MAC_APP/Contents/MacOS/$APP" --version >/dev/null 2>&1 || die "the installed Etch doesn't start ($MAC_APP)"

  if [ -n "$before" ] && [ "$before" != "$version" ]; then say "Updated Etch $before -> $version"
  else say "Installed Etch $version"; fi
  say "  $MAC_APP: open it from Launchpad, Spotlight, or Finder"
  say "  PNG, JPEG, and BMP images list it under Open With; to make it their default, select one"
  say "  in Finder, then File > Get Info > Open with: Etch > Change All"
}

mac=false; [ "$(uname -s)" = Darwin ] && mac=true
case "${1:-}" in
  --uninstall) if $mac; then mac_uninstall; else uninstall; fi ;;
  "") if $mac; then mac_install; else install_app; fi ;;
  *) die "unknown option $1 (use --uninstall, or nothing to install or update)" ;;
esac
