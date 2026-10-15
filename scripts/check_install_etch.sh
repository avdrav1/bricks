#!/usr/bin/env bash
# ETCH-5 and ETCH-6 on the release: piping Etch's install.sh into sh installs the release
# per user on clean Arch, Ubuntu 24.04, and Fedora; the checksum is verified; the menu
# entry starts Etch; Etch becomes the default for PNG, JPEG, and BMP and double-clicking a
# .png opens it in Etch; and without GTK 4 it says how to get it.
#
#   scripts/check_install_etch.sh dist/etch-X.Y.Z-x86_64-linux.tar.gz
#
# Serves a throwaway download site (the tarball, latest.json, install.sh, and a .png to
# open) on localhost and, per distro, in fresh containers as a normal user:
#   1. without GTK 4: install, and the hint names the distro's package;
#   2. with a tampered tarball: refused, nothing installed;
#   3. with GTK 4 and another app's defaults for .png, .jpg, .gif, and .txt: install; the
#      .png, .jpg, and .bmp defaults are Etch's (gio names it), the .gif and .txt ones stay;
#      the menu entry runs the installed binary and `gio launch` on it (what a menu click
#      does) starts Etch; `gio open` on t.png (what a file manager's double-click does)
#      starts Etch on that file, which logs that it opened the image (16×12), and it is
#      still running 4 s later. Then reinstall (update) and --uninstall: nothing of Etch's
#      is left, no default names Etch, and the .gif and .txt defaults are as they were.
# Needs docker and python3. VERBOSE=1 prints each container's transcript; ONLY=Fedora (or
# Arch, Ubuntu) runs one distro.
set -euo pipefail
cd "$(dirname "$0")/.."
tar=$(realpath "${1:?usage: check_install_etch.sh TARBALL}")
name=$(basename "$tar")

site=$(mktemp -d); trap 'kill $server 2>/dev/null || true; rm -rf "$site"' EXIT
mkdir -p "$site/good/releases" "$site/bad/releases"
cp "$tar" "$site/good/releases/"
cp "$tar" "$site/bad/releases/"; printf 'x' >> "$site/bad/releases/$name"
port=$((20000 + RANDOM % 20000))
for kind in good bad; do
  sha=$(sha256sum "$tar" | cut -d' ' -f1)
  printf '{"version":"9.9.9","date":"2026-10-14","url":"http://127.0.0.1:%s/%s/releases/%s","sha256":"%s","bytes":1}\n' \
    "$port" "$kind" "$name" "$sha" > "$site/$kind/latest.json"
done
cp sites/etch/install.sh "$site/install.sh"
# A 16×12 PNG with transparency (image-io's fixture).
cp apps/etch/crates/image-io/tests/data/alpha.png "$site/t.png"
chmod -R a+rX "$site"
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$site" >/dev/null 2>&1 &
server=$!
sleep 1

# distro | image | GTK 4 runtime + tools the test itself uses | the hint expected without GTK
checks=(
  "Arch|archlinux:latest|pacman -Syu --noconfirm --needed gtk4 shared-mime-info >/dev/null|pacman -S gtk4"
  "Ubuntu 24.04|ubuntu:24.04|apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends libgtk-4-1 libgtk-4-bin libglib2.0-bin shared-mime-info >/dev/null|apt install libgtk-4-1"
  "Fedora|fedora:latest|dnf install -y -q gtk4 glib2 shared-mime-info >/dev/null|dnf install gtk4"
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
    curl -fsS -o /tmp/t.png \$url/t.png; chmod a+r /tmp/t.png
    gdk='GDK_BACKEND=broadway BROADWAY_DISPLAY=:5 GDK_DEBUG=no-portals'
    # The command lines of the Etch processes running.
    running='for p in /proc/[0-9]*; do tr \"\\\\0\" \" \" < \$p/cmdline 2>/dev/null; echo; done | grep \"^/home/tester/.local/bin/etch\" | sed \"s/^/running:/\"'

    echo '== no gtk'
    as \"curl -fsSL \$url/install.sh | ETCH_DOWNLOAD_URL=\$url/good sh\" && echo 'exit 0' || echo \"exit \$?\"

    echo '== tampered'
    as 'rm -rf ~/.local ~/.config'
    as \"curl -fsSL \$url/install.sh | ETCH_DOWNLOAD_URL=\$url/bad sh\" 2>&1 && echo 'exit 0' || echo \"exit \$?\"
    as 'ls ~/.local/bin/etch 2>/dev/null || echo nothing-installed'

    echo '== install'
    $gtk
    as 'mkdir -p ~/.config; printf \"[Default Applications]\nimage/png=eog.desktop\nimage/jpeg=eog.desktop\nimage/gif=eog.desktop\ntext/plain=gedit.desktop\n\" > ~/.config/mimeapps.list'
    as \"curl -fsSL \$url/install.sh | ETCH_DOWNLOAD_URL=\$url/good sh\"
    as 'cat ~/.config/mimeapps.list; grep ^Exec= ~/.local/share/applications/dev.cronkite.Etch.desktop'
    as 'ls ~/.local/share/icons/hicolor/scalable/apps/dev.cronkite.Etch.svg >/dev/null && echo icon-installed'
    for t in image/png image/jpeg image/bmp; do as \"gio mime \$t | head -1 | sed 's|^|gio-\$t: |'\"; done
    as 'nohup gtk4-broadwayd :5 >/dev/null 2>&1 & sleep 1'
    as \"\$gdk gio launch ~/.local/share/applications/dev.cronkite.Etch.desktop; sleep 4\"
    as \"\$running\" | sed 's/^running:/launched:/'
    as \"\$gdk gio open /tmp/t.png 2>&1; sleep 4\"
    as \"\$running\"

    echo '== update'
    as \"curl -fsSL \$url/install.sh | ETCH_DOWNLOAD_URL=\$url/good sh\" | head -1

    echo '== uninstall'
    as \"curl -fsSL \$url/install.sh | sh -s -- --uninstall\" >/dev/null
    as 'ls ~/.local/bin/etch ~/.local/share/applications/dev.cronkite.Etch.desktop ~/.local/share/icons/hicolor/scalable/apps/dev.cronkite.Etch.svg 2>&1 | grep -c \"No such\" | sed s/^/gone:/'
    as 'grep -c Etch ~/.config/mimeapps.list | sed s/^/etch-defaults:/ || true'
    as 'grep -e ^image/gif= -e ^text/plain= ~/.config/mimeapps.list | sed s/^/kept:/'
  " 2>&1) || true
  ok=true
  check() { grep -q -- "$1" <<<"$out" || { echo "FAIL $distro: $2"; ok=false; }; }
  check "Installed Etch 9.9.9" "no install"
  check "$hint" "no '$hint' hint without GTK 4"
  check "checksum mismatch" "tampered tarball not refused"
  check "nothing-installed" "tampered tarball left files"
  check "^Exec=/home/tester/.local/bin/etch %F$" "desktop entry not pointing at the binary"
  check "^icon-installed$" "no icon"
  for t in image/png image/jpeg image/bmp; do
    check "^$t=dev.cronkite.Etch.desktop$" "$t default not set"
    check "^gio-$t: Default application for .*dev.cronkite.Etch.desktop" "gio doesn't name Etch for $t"
  done
  check "^image/gif=eog.desktop$" "another type's default (image/gif) lost on install"
  check "^text/plain=gedit.desktop$" "another type's default (text/plain) lost on install"
  check "^launched:/home/tester/.local/bin/etch $" "the menu entry (gio launch) did not start Etch"
  check "^running:/home/tester/.local/bin/etch /tmp/t.png $" "double-click (gio open) did not start Etch on the .png"
  check "opened /tmp/t.png: 16×12" "Etch didn't open the .png"
  check "^Updated Etch\|^Installed Etch 9.9.9" "reinstall failed"
  check "^gone:3$" "uninstall left files"
  check "^etch-defaults:0$" "uninstall left Etch's defaults"
  check "^kept:image/gif=eog.desktop$" "install or uninstall lost the image/gif default"
  check "^kept:text/plain=gedit.desktop$" "install or uninstall lost the text/plain default"
  if $ok; then
    echo "ok   $distro: installs, refuses a bad checksum, hints '$hint', starts from its menu entry, takes PNG, JPEG, and BMP, gio open opens a .png, keeps other defaults, uninstalls"
    [ -n "${VERBOSE:-}" ] && sed 's/^/     /' <<<"$out"
  else
    sed 's/^/     /' <<<"$out" | tail -40; fail=1
  fi
done
[ $fail = 0 ] && echo "INSTALL OK" || { echo "INSTALL CHECK FAILED"; exit 1; }
