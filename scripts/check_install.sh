#!/usr/bin/env bash
# PKG-1 acceptance: piping install.sh into sh installs the release per user on clean Arch,
# Ubuntu 24.04, and Fedora; the checksum is verified; double-clicking a CSV opens it; and
# without GTK 4 it says how to get it.
#
#   scripts/check_install.sh dist/spreadsheet-X.Y.Z-x86_64-linux.tar.gz
#
# Serves a throwaway download site (the tarball, latest.json, install.sh) on localhost and,
# per distro, in fresh containers as a normal user:
#   1. without GTK 4: install, and the hint names the distro's package;
#   2. with a tampered tarball: refused, nothing installed;
#   3. with GTK 4: install, then `gio open t.csv` (what a file manager's double-click
#      does) must start the app on that file: the app's recent-files list gets it. Then
#      reinstall (update) and --uninstall.
# Needs docker and python3. VERBOSE=1 prints each container's transcript; ONLY=Fedora (or
# Arch, Ubuntu) runs one distro.
set -euo pipefail
cd "$(dirname "$0")/.."
tar=$(realpath "${1:?usage: check_install.sh TARBALL}")
name=$(basename "$tar")

site=$(mktemp -d); trap 'kill $server 2>/dev/null || true; rm -rf "$site"' EXIT
mkdir -p "$site/good/releases" "$site/bad/releases"
cp "$tar" "$site/good/releases/"
cp "$tar" "$site/bad/releases/"; printf 'x' >> "$site/bad/releases/$name"
port=$((20000 + RANDOM % 20000))
for kind in good bad; do
  sha=$(sha256sum "$tar" | cut -d' ' -f1)
  printf '{"version":"9.9.9","date":"2026-10-10","url":"http://127.0.0.1:%s/%s/releases/%s","sha256":"%s","bytes":1}\n' \
    "$port" "$kind" "$name" "$sha" > "$site/$kind/latest.json"
done
cp site/install.sh "$site/install.sh"
chmod -R a+rX "$site"
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$site" >/dev/null 2>&1 &
server=$!
sleep 1

# distro | image | GTK 4 runtime + tools the test itself uses | the hint expected without GTK
checks=(
  "Arch|archlinux:latest|pacman -Syu --noconfirm --needed gtk4 >/dev/null|pacman -S gtk4"
  "Ubuntu 24.04|ubuntu:24.04|apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends libgtk-4-1 libgtk-4-bin libglib2.0-bin >/dev/null|apt install libgtk-4-1"
  "Fedora|fedora:latest|dnf install -y -q gtk4 glib2 >/dev/null|dnf install gtk4"
)
base="curl ca-certificates"
fail=0
for c in "${checks[@]}"; do
  IFS='|' read -r distro image gtk hint <<<"$c"
  [ -n "${ONLY:-}" ] && [[ "$distro" != "$ONLY"* ]] && continue
  case "$image" in
    archlinux*) prep="pacman -Sy --noconfirm --needed curl >/dev/null" ;;
    ubuntu*) prep="apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends $base >/dev/null" ;;
    fedora*) prep="dnf install -y -q curl tar gzip util-linux >/dev/null" ;; # as on Workstation
  esac
  out=$(docker run --rm --network host "$image" sh -c "
    set -e
    $prep
    useradd -m tester
    as() { su tester -c \"cd; \$1\"; }
    url=http://127.0.0.1:$port
    printf 'id,name\n1,a\n' > /tmp/t.csv; chmod a+r /tmp/t.csv

    echo '== no gtk'
    as \"curl -fsSL \$url/install.sh | SPREADSHEET_DOWNLOAD_URL=\$url/good sh\" && echo 'exit 0' || echo \"exit \$?\"

    echo '== tampered'
    as 'rm -rf ~/.local ~/.config'
    as \"curl -fsSL \$url/install.sh | SPREADSHEET_DOWNLOAD_URL=\$url/bad sh\" 2>&1 && echo 'exit 0' || echo \"exit \$?\"
    as 'ls ~/.local/bin/spreadsheet 2>/dev/null || echo nothing-installed'

    echo '== with gtk'
    $gtk
    as 'mkdir -p ~/.config; printf \"[Default Applications]\nimage/png=eog.desktop\ntext/csv=libreoffice-calc.desktop\n\" > ~/.config/mimeapps.list'
    as \"curl -fsSL \$url/install.sh | SPREADSHEET_DOWNLOAD_URL=\$url/good sh\"
    as 'cat ~/.config/mimeapps.list; grep ^Exec= ~/.local/share/applications/spreadsheet.desktop'
    as 'gio mime text/csv | head -1'
    as 'gtk4-broadwayd :5 >/dev/null 2>&1 & sleep 1; GDK_BACKEND=broadway BROADWAY_DISPLAY=:5 GDK_DEBUG=no-portals gio open /tmp/t.csv; sleep 4; cat ~/.local/state/spreadsheet/recent'

    echo '== update'
    as \"curl -fsSL \$url/install.sh | SPREADSHEET_DOWNLOAD_URL=\$url/good sh\" | head -1

    echo '== uninstall'
    as \"curl -fsSL \$url/install.sh | sh -s -- --uninstall\" >/dev/null
    as 'ls ~/.local/bin/spreadsheet ~/.local/share/applications/spreadsheet.desktop 2>&1 | grep -c \"No such\"; grep -c spreadsheet ~/.config/mimeapps.list || true'
    as 'grep ^image/png= ~/.config/mimeapps.list | sed s/^/kept:/'
  " 2>&1) || true
  ok=true
  check() { grep -q -- "$1" <<<"$out" || { echo "FAIL $distro: $2"; ok=false; }; }
  check "Installed Spreadsheet 9.9.9" "no install"
  check "$hint" "no '$hint' hint without GTK 4"
  check "checksum mismatch" "tampered tarball not refused"
  check "nothing-installed" "tampered tarball left files"
  check "text/csv=spreadsheet.desktop" "CSV default not set"
  check "Exec=/home/tester/.local/bin/spreadsheet %f" "desktop entry not pointing at the binary"
  check "^/tmp/t.csv$" "double-click (gio open) did not open the CSV"
  check "^2$" "uninstall left files"
  check "^kept:image/png=eog.desktop$" "install or uninstall lost another app's default"
  if $ok; then
    echo "ok   $distro: installs, refuses a bad checksum, hints '$hint', gio open opens the CSV, uninstalls"
    [ -n "${VERBOSE:-}" ] && sed 's/^/     /' <<<"$out"
  else
    sed 's/^/     /' <<<"$out" | tail -40; fail=1
  fi
done
[ $fail = 0 ] && echo "INSTALL OK" || { echo "INSTALL CHECK FAILED"; exit 1; }
