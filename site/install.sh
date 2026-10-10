#!/bin/sh
# Spreadsheet installer (PKG-1). Installs or updates the latest release for this user,
# without sudo:
#
#   curl -fsSL https://<download page>/install.sh | sh
#   curl -fsSL https://<download page>/install.sh | sh -s -- --uninstall
#
# Puts the binary in ~/.local/bin, its icon and desktop entry in ~/.local/share, and makes
# it the default for CSV and TSV files (~/.config/mimeapps.list), so double-clicking a CSV
# opens it. The download is checked against the release's sha256 before anything is
# installed. GTK 4 comes from the system: if it is missing, this says how to install it.
set -eu

# Where latest.json is: filled in when the page is deployed (scripts/ship.sh site).
BASE_URL="${SPREADSHEET_DOWNLOAD_URL:-@DOWNLOAD_BASE_URL@}"
APP=spreadsheet

BIN_DIR="$HOME/.local/bin"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}"
DESKTOP="$DATA/applications/$APP.desktop"
ICON="$DATA/icons/hicolor/scalable/apps/$APP.svg"
MIMEAPPS="$CONFIG/mimeapps.list"
TYPES="text/csv text/tab-separated-values"

say() { printf '%s\n' "$*"; }
die() { printf 'spreadsheet install: %s\n' "$*" >&2; exit 1; }

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

# Make APP.desktop the default for each of $TYPES in mimeapps.list, or with no app, drop
# the defaults that point at us (another app's stay). Every other line stays as it is.
set_defaults() { # APP.desktop or empty
  mkdir -p "$CONFIG"
  [ -f "$MIMEAPPS" ] || : > "$MIMEAPPS"
  out="$MIMEAPPS.new.$$"
  awk -v app="$1" -v ours="$APP.desktop" -v types="$TYPES" '
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

refresh_caches() {
  command -v update-desktop-database >/dev/null 2>&1 &&
    update-desktop-database -q "$DATA/applications" 2>/dev/null || true
  command -v gtk-update-icon-cache >/dev/null 2>&1 &&
    gtk-update-icon-cache -q -t "$DATA/icons/hicolor" 2>/dev/null || true
}

uninstall() {
  rm -f "$BIN_DIR/$APP" "$DESKTOP" "$ICON"
  [ -f "$MIMEAPPS" ] && set_defaults ""
  refresh_caches
  say "Removed Spreadsheet. Your files and its recent-files list (~/.local/state/spreadsheet) are untouched."
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
  case "$BASE_URL" in @*) die "no download URL set (SPREADSHEET_DOWNLOAD_URL)" ;; esac
  tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
  fetch "$BASE_URL/latest.json" "$tmp/latest.json" || die "cannot reach $BASE_URL/latest.json"
  version=$(field version "$tmp/latest.json")
  url=$(field url "$tmp/latest.json")
  want=$(field sha256 "$tmp/latest.json")
  [ -n "$version" ] && [ -n "$url" ] && [ -n "$want" ] || die "latest.json is not a release"
  say "Downloading Spreadsheet $version"
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
  cp "$dir/$APP.svg" "$ICON"
  # The full path, so it starts from a launcher even when ~/.local/bin isn't on its PATH.
  sed "s|^Exec=$APP |Exec=$BIN_DIR/$APP |" "$dir/$APP.desktop" > "$DESKTOP"
  set_defaults "$APP.desktop"
  refresh_caches

  if [ -n "$before" ] && [ "$before" != "$version" ]; then say "Updated Spreadsheet $before -> $version"
  else say "Installed Spreadsheet $version"; fi
  say "  $BIN_DIR/$APP; CSV and TSV files open with it"
  case ":$PATH:" in *":$BIN_DIR:"*) ;; *) say "  $BIN_DIR is not on your PATH: start it from your app menu, or add it to PATH" ;; esac

  if ! err=$("$BIN_DIR/$APP" --version 2>&1 >/dev/null); then
    say ""
    say "Spreadsheet needs GTK 4, which is missing or too old here ($err)."
    say "Install it, then start Spreadsheet:"
    gtk_hint
    exit 2
  fi
}

case "${1:-}" in
  --uninstall) uninstall ;;
  "") install_app ;;
  *) die "unknown option $1 (use --uninstall, or nothing to install or update)" ;;
esac
