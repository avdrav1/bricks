#!/usr/bin/env bash
# PKG-1 acceptance: piping install.sh into sh installs the release per user on clean Arch,
# Ubuntu 24.04, and Fedora; the checksum is verified; double-clicking a CSV opens it; and
# without GTK 4 it says how to get it. PKG-4 and PKG-5: installing over an earlier release
# (v0.2.1, named Bricks, command `bricks`; v0.1.0, named Spreadsheet, command
# `spreadsheet`) leaves one app, Brick, with its recent files.
#
#   scripts/check_install.sh dist/brick-X.Y.Z-x86_64-linux.tar.gz
#
# Serves a throwaway download site (the tarball, latest.json, install.sh, and v0.2.1 and
# v0.1.0 with their own installers) on localhost and, per distro, in fresh containers as a
# normal user:
#   1. without GTK 4: install, and the hint names the distro's package;
#   2. with a tampered tarball: refused, nothing installed;
#   3. with GTK 4: install v0.2.1 (Bricks) and open a CSV with it, then install this
#      release over it: the old binary, menu entry, icon, and CSV default are gone, and
#      `gio open t.csv` (what a file manager's double-click does) starts Brick on that
#      file, its recent-files list holding both files; its licences are in
#      ~/.local/share/doc/brick, THIRD-PARTY-RUST.txt naming gtk4 (PKG-6). Then reinstall
#      (update) and --uninstall, which removes them too;
#   4. the same from v0.1.0 (Spreadsheet): its app is replaced and its recent files kept.
# Needs docker, git, and python3; the old tarballs come from dist/ or the download site
# (BRICKS_TARBALL=path and SPREADSHEET_TARBALL=path override). VERBOSE=1 prints each
# container's transcript; ONLY=Fedora (or Arch, Ubuntu) runs one distro.
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
  printf '{"version":"9.9.9","date":"2026-10-12","url":"http://127.0.0.1:%s/%s/releases/%s","sha256":"%s","bytes":1}\n' \
    "$port" "$kind" "$name" "$sha" > "$site/$kind/latest.json"
done
# Earlier releases: KIND VERSION TARBALL, each with the installer it shipped with.
old_release() {
  local kind=$1 version=$2 file=$3 override=$4
  mkdir -p "$site/$kind/releases"
  local src=${!override:-dist/$file}
  if [ -f "$src" ]; then cp "$src" "$site/$kind/releases/$file"
  else curl -fsSL -o "$site/$kind/releases/$file" "https://dl.cronkite.cc/releases/v$version/$file"; fi
  printf '{"version":"%s","date":"2026-10-10","url":"http://127.0.0.1:%s/%s/releases/%s","sha256":"%s","bytes":1}\n' \
    "$version" "$port" "$kind" "$file" "$(sha256sum "$site/$kind/releases/$file" | cut -d' ' -f1)" \
    > "$site/$kind/latest.json"
  git show "v$version:site/install.sh" > "$site/$kind-install.sh"
}
old_release bricks 0.2.1 bricks-0.2.1-x86_64-linux.tar.gz BRICKS_TARBALL
old_release spreadsheet 0.1.0 spreadsheet-0.1.0-x86_64-linux.tar.gz SPREADSHEET_TARBALL
cp sites/brick/install.sh "$site/install.sh"
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
    printf 'id,name\n1,a\n' > /tmp/t.csv; cp /tmp/t.csv /tmp/old.csv; cp /tmp/t.csv /tmp/oldest.csv
    chmod a+r /tmp/t.csv /tmp/old.csv /tmp/oldest.csv
    open='GDK_BACKEND=broadway BROADWAY_DISPLAY=:5 GDK_DEBUG=no-portals gio open'
    # How many of an app's binary, menu entry, and icon are gone: gone COMMAND ID.
    gone() { h=/home/tester/.local; ls \$h/bin/\$1 \$h/share/applications/\$2.desktop \$h/share/icons/hicolor/scalable/apps/\$2.svg 2>&1 | grep -c 'No such' || true; }

    echo '== no gtk'
    as \"curl -fsSL \$url/install.sh | BRICK_DOWNLOAD_URL=\$url/good sh\" && echo 'exit 0' || echo \"exit \$?\"

    echo '== tampered'
    as 'rm -rf ~/.local ~/.config'
    as \"curl -fsSL \$url/install.sh | BRICK_DOWNLOAD_URL=\$url/bad sh\" 2>&1 && echo 'exit 0' || echo \"exit \$?\"
    as 'ls ~/.local/bin/brick 2>/dev/null || echo nothing-installed'

    echo '== v0.2.1'
    $gtk
    as 'mkdir -p ~/.config; printf \"[Default Applications]\nimage/png=eog.desktop\ntext/csv=libreoffice-calc.desktop\n\" > ~/.config/mimeapps.list'
    as \"curl -fsSL \$url/bricks-install.sh | BRICKS_DOWNLOAD_URL=\$url/bricks sh\" | head -1
    as 'nohup gtk4-broadwayd :5 >/dev/null 2>&1 & sleep 1'
    as \"\$open /tmp/old.csv; sleep 4; sed s/^/old-recent:/ ~/.local/state/bricks/recent\"

    echo '== upgrade'
    as \"curl -fsSL \$url/install.sh | BRICK_DOWNLOAD_URL=\$url/good sh\"
    as 'cat ~/.config/mimeapps.list; grep ^Exec= ~/.local/share/applications/dev.cronkite.Brick.desktop'
    as 'cd ~/.local/share/doc/brick && echo licences: \$(ls) && echo notice: \$(wc -c < THIRD-PARTY-RUST.txt) bytes, gtk4 \$(grep -c \"^gtk4 [0-9]\" THIRD-PARTY-RUST.txt)' || echo 'licences: none'
    as 'grep -c dev.bricks.Bricks ~/.config/mimeapps.list | sed s/^/old-defaults:/ || true'
    gone bricks dev.bricks.Bricks | sed s/^/old-gone:/
    as 'gio mime text/csv | head -1'
    as \"\$open /tmp/t.csv; sleep 4; sed s/^/recent:/ ~/.local/state/brick/recent\"
    as 'echo state: \$(ls ~/.local/state)'

    echo '== update'
    as \"curl -fsSL \$url/install.sh | BRICK_DOWNLOAD_URL=\$url/good sh\" | head -1

    echo '== uninstall'
    as \"curl -fsSL \$url/install.sh | sh -s -- --uninstall\" >/dev/null
    gone brick dev.cronkite.Brick | sed s/^/gone:/
    as 'ls -d ~/.local/share/doc/brick 2>/dev/null || echo doc-gone'
    as 'grep -c Brick ~/.config/mimeapps.list | sed s/^/brick-defaults:/ || true'
    as 'grep ^image/png= ~/.config/mimeapps.list | sed s/^/kept:/'

    echo '== from v0.1.0'
    as 'rm -rf ~/.local/state'
    as \"curl -fsSL \$url/spreadsheet-install.sh | SPREADSHEET_DOWNLOAD_URL=\$url/spreadsheet sh\" | head -1
    as \"\$open /tmp/oldest.csv; sleep 4; sed s/^/oldest-recent:/ ~/.local/state/spreadsheet/recent\"
    as \"curl -fsSL \$url/install.sh | BRICK_DOWNLOAD_URL=\$url/good sh\" | grep '^Updated' | sed s/^/from-oldest:/ || true
    gone spreadsheet spreadsheet | sed s/^/oldest-gone:/
    as 'grep -c spreadsheet.desktop ~/.config/mimeapps.list | sed s/^/oldest-defaults:/ || true'
    as \"\$open /tmp/t.csv; sleep 4; sed s/^/oldest-then:/ ~/.local/state/brick/recent\"
  " 2>&1) || true
  ok=true
  check() { grep -q -- "$1" <<<"$out" || { echo "FAIL $distro: $2"; ok=false; }; }
  check "Installed Brick 9.9.9" "no install"
  check "$hint" "no '$hint' hint without GTK 4"
  check "checksum mismatch" "tampered tarball not refused"
  check "nothing-installed" "tampered tarball left files"
  check "^old-recent:/tmp/old.csv$" "v0.2.1 didn't install and open a CSV (setup)"
  check "Updated Brick 0.2.1 -> 9.9.9" "upgrade from Bricks not reported as one"
  check "^old-gone:3$" "Bricks' binary, menu entry, or icon left behind"
  check "^old-defaults:0$" "Bricks' CSV default left behind"
  check "text/csv=dev.cronkite.Brick.desktop" "CSV default not set"
  check "Exec=/home/tester/.local/bin/brick %f" "desktop entry not pointing at the binary"
  check "^recent:/tmp/t.csv$" "double-click (gio open) did not open the CSV"
  check "^recent:/tmp/old.csv$" "Bricks' recent files lost"
  check "^state: brick$" "state folder not moved to brick"
  check "^licences: LICENSE LICENSE-APACHE THIRD-PARTY-RUST.txt$" "licences not installed in ~/.local/share/doc/brick"
  check "^notice: [0-9]\{5,\} bytes, gtk4 1$" "THIRD-PARTY-RUST.txt empty or without gtk4"
  check "^gone:3$" "uninstall left files"
  check "^doc-gone$" "uninstall left ~/.local/share/doc/brick"
  check "^brick-defaults:0$" "uninstall left Brick's defaults"
  check "^kept:image/png=eog.desktop$" "install or uninstall lost another app's default"
  check "^oldest-recent:/tmp/oldest.csv$" "v0.1.0 didn't install and open a CSV (setup)"
  check "^from-oldest:Updated Brick 0.1.0 -> 9.9.9$" "upgrade from Spreadsheet not reported as one"
  check "^oldest-gone:3$" "Spreadsheet's binary, menu entry, or icon left behind"
  check "^oldest-defaults:0$" "Spreadsheet's CSV default left behind"
  check "^oldest-then:/tmp/oldest.csv$" "Spreadsheet's recent files lost"
  if $ok; then
    echo "ok   $distro: installs, refuses a bad checksum, hints '$hint', replaces Bricks v0.2.1 and Spreadsheet v0.1.0 keeping their recent files, gio open opens the CSV, installs its licences, uninstalls"
    [ -n "${VERBOSE:-}" ] && sed 's/^/     /' <<<"$out"
  else
    sed 's/^/     /' <<<"$out" | tail -40; fail=1
  fi
done
[ $fail = 0 ] && echo "INSTALL OK" || { echo "INSTALL CHECK FAILED"; exit 1; }
