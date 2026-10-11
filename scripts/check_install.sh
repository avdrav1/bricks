#!/usr/bin/env bash
# PKG-1 acceptance: piping install.sh into sh installs the release per user on clean Arch,
# Ubuntu 24.04, and Fedora; the checksum is verified; double-clicking a CSV opens it; and
# without GTK 4 it says how to get it. PKG-4: installing over v0.1.0 (named Spreadsheet,
# command `spreadsheet`) leaves one app, Bricks, with its recent files.
#
#   scripts/check_install.sh dist/bricks-X.Y.Z-x86_64-linux.tar.gz
#
# Serves a throwaway download site (the tarball, latest.json, install.sh, and v0.1.0 with
# its own installer) on localhost and, per distro, in fresh containers as a normal user:
#   1. without GTK 4: install, and the hint names the distro's package;
#   2. with a tampered tarball: refused, nothing installed;
#   3. with GTK 4: install v0.1.0 and open a CSV with it, then install this release over
#      it: the old binary, menu entry, icon, and CSV default are gone, and `gio open t.csv`
#      (what a file manager's double-click does) starts Bricks on that file, its
#      recent-files list holding both files. Then reinstall (update) and --uninstall.
# Needs docker, git, and python3; v0.1.0's tarball comes from dist/ or the download site
# (OLD_TARBALL=path overrides). VERBOSE=1 prints each container's transcript; ONLY=Fedora
# (or Arch, Ubuntu) runs one distro.
set -euo pipefail
cd "$(dirname "$0")/.."
tar=$(realpath "${1:?usage: check_install.sh TARBALL}")
name=$(basename "$tar")
old_name=spreadsheet-0.1.0-x86_64-linux.tar.gz

site=$(mktemp -d); trap 'kill $server 2>/dev/null || true; rm -rf "$site"' EXIT
mkdir -p "$site/good/releases" "$site/bad/releases" "$site/old/releases"
cp "$tar" "$site/good/releases/"
cp "$tar" "$site/bad/releases/"; printf 'x' >> "$site/bad/releases/$name"
old=${OLD_TARBALL:-dist/$old_name}
if [ -f "$old" ]; then cp "$old" "$site/old/releases/$old_name"
else curl -fsSL -o "$site/old/releases/$old_name" "https://dl.cronkite.cc/releases/v0.1.0/$old_name"; fi
git show v0.1.0:site/install.sh > "$site/old-install.sh"
port=$((20000 + RANDOM % 20000))
for kind in good bad; do
  sha=$(sha256sum "$tar" | cut -d' ' -f1)
  printf '{"version":"9.9.9","date":"2026-10-10","url":"http://127.0.0.1:%s/%s/releases/%s","sha256":"%s","bytes":1}\n' \
    "$port" "$kind" "$name" "$sha" > "$site/$kind/latest.json"
done
printf '{"version":"0.1.0","date":"2026-10-10","url":"http://127.0.0.1:%s/old/releases/%s","sha256":"%s","bytes":1}\n' \
  "$port" "$old_name" "$(sha256sum "$site/old/releases/$old_name" | cut -d' ' -f1)" > "$site/old/latest.json"
cp sites/bricks/install.sh "$site/install.sh"
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
    printf 'id,name\n1,a\n' > /tmp/t.csv; cp /tmp/t.csv /tmp/old.csv; chmod a+r /tmp/t.csv /tmp/old.csv
    open='GDK_BACKEND=broadway BROADWAY_DISPLAY=:5 GDK_DEBUG=no-portals gio open'

    echo '== no gtk'
    as \"curl -fsSL \$url/install.sh | BRICKS_DOWNLOAD_URL=\$url/good sh\" && echo 'exit 0' || echo \"exit \$?\"

    echo '== tampered'
    as 'rm -rf ~/.local ~/.config'
    as \"curl -fsSL \$url/install.sh | BRICKS_DOWNLOAD_URL=\$url/bad sh\" 2>&1 && echo 'exit 0' || echo \"exit \$?\"
    as 'ls ~/.local/bin/bricks 2>/dev/null || echo nothing-installed'

    echo '== v0.1.0'
    $gtk
    as 'mkdir -p ~/.config; printf \"[Default Applications]\nimage/png=eog.desktop\ntext/csv=libreoffice-calc.desktop\n\" > ~/.config/mimeapps.list'
    as \"curl -fsSL \$url/old-install.sh | SPREADSHEET_DOWNLOAD_URL=\$url/old sh\" | head -1
    as 'nohup gtk4-broadwayd :5 >/dev/null 2>&1 & sleep 1'
    as \"\$open /tmp/old.csv; sleep 4; sed s/^/old-recent:/ ~/.local/state/spreadsheet/recent\"

    echo '== upgrade'
    as \"curl -fsSL \$url/install.sh | BRICKS_DOWNLOAD_URL=\$url/good sh\"
    as 'cat ~/.config/mimeapps.list; grep ^Exec= ~/.local/share/applications/dev.bricks.Bricks.desktop'
    as 'grep -c spreadsheet ~/.config/mimeapps.list | sed s/^/old-defaults:/'
    as 'ls ~/.local/bin/spreadsheet ~/.local/share/applications/spreadsheet.desktop ~/.local/share/icons/hicolor/scalable/apps/spreadsheet.svg 2>&1 | grep -c \"No such\" | sed s/^/old-gone:/'
    as 'gio mime text/csv | head -1'
    as \"\$open /tmp/t.csv; sleep 4; sed s/^/recent:/ ~/.local/state/bricks/recent\"
    as 'echo state: \$(ls ~/.local/state)'

    echo '== update'
    as \"curl -fsSL \$url/install.sh | BRICKS_DOWNLOAD_URL=\$url/good sh\" | head -1

    echo '== uninstall'
    as \"curl -fsSL \$url/install.sh | sh -s -- --uninstall\" >/dev/null
    as 'ls ~/.local/bin/bricks ~/.local/share/applications/dev.bricks.Bricks.desktop 2>&1 | grep -c \"No such\"; grep -c \"Bricks\" ~/.config/mimeapps.list || true'
    as 'grep ^image/png= ~/.config/mimeapps.list | sed s/^/kept:/'
  " 2>&1) || true
  ok=true
  check() { grep -q -- "$1" <<<"$out" || { echo "FAIL $distro: $2"; ok=false; }; }
  check "Installed Bricks 9.9.9" "no install"
  check "$hint" "no '$hint' hint without GTK 4"
  check "checksum mismatch" "tampered tarball not refused"
  check "nothing-installed" "tampered tarball left files"
  check "^old-recent:/tmp/old.csv$" "v0.1.0 didn't install and open a CSV (setup)"
  check "Updated Bricks 0.1.0 -> 9.9.9" "upgrade not reported as one"
  check "^old-gone:3$" "v0.1.0's binary, menu entry, or icon left behind"
  check "^old-defaults:0$" "v0.1.0's CSV default left behind"
  check "text/csv=dev.bricks.Bricks.desktop" "CSV default not set"
  check "Exec=/home/tester/.local/bin/bricks %f" "desktop entry not pointing at the binary"
  check "^recent:/tmp/t.csv$" "double-click (gio open) did not open the CSV"
  check "^recent:/tmp/old.csv$" "v0.1.0's recent files lost"
  check "^state: bricks$" "state folder not moved to bricks"
  check "^2$" "uninstall left files"
  check "^kept:image/png=eog.desktop$" "install or uninstall lost another app's default"
  if $ok; then
    echo "ok   $distro: installs, refuses a bad checksum, hints '$hint', replaces v0.1.0 keeping its recent files, gio open opens the CSV, uninstalls"
    [ -n "${VERBOSE:-}" ] && sed 's/^/     /' <<<"$out"
  else
    sed 's/^/     /' <<<"$out" | tail -40; fail=1
  fi
done
[ $fail = 0 ] && echo "INSTALL OK" || { echo "INSTALL CHECK FAILED"; exit 1; }
