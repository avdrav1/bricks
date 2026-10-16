#!/usr/bin/env bash
# FRAME-9 on the release: piping Frame's install.sh into sh installs the release per user
# on clean Arch, Ubuntu 24.04, and Fedora; the checksum is verified; double-clicking a
# .pptx opens Frame; and without GTK 4 it says how to get it.
#
#   scripts/check_install_frame.sh dist/frame-X.Y.Z-x86_64-linux.tar.gz
#
# Serves a throwaway download site (the tarball, latest.json, install.sh, and a .pptx to
# open) on localhost and, per distro, in fresh containers as a normal user:
#   1. without GTK 4: install, and the hint names the distro's package;
#   2. with a tampered tarball: refused, nothing installed;
#   3. with GTK 4 and other apps' defaults for .pptx and .png: install; the .pptx default
#      is Frame's, the .png one stays; `gio open` (what a file manager's double-click does)
#      on t.pptx starts Frame on that file, and 4 s later it is still running and holds
#      the deck's package copy (~/.cache/frame), so it read the deck; its licences are in
#      ~/.local/share/doc/frame, THIRD-PARTY-RUST.txt naming gtk4 (PKG-6). Then reinstall
#      (update) and --uninstall: nothing of Frame's is left, the .png default is.
# Needs docker and python3. VERBOSE=1 prints each container's transcript; ONLY=Fedora (or
# Arch, Ubuntu) runs one distro.
set -euo pipefail
cd "$(dirname "$0")/.."
tar=$(realpath "${1:?usage: check_install_frame.sh TARBALL}")
name=$(basename "$tar")
PPTX=application/vnd.openxmlformats-officedocument.presentationml.presentation

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
cp sites/frame/install.sh "$site/install.sh"
cp apps/frame/crates/pptx/tests/data/layouts.pptx "$site/t.pptx"
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
    curl -fsS -o /tmp/t.pptx \$url/t.pptx; chmod a+r /tmp/t.pptx
    open='GDK_BACKEND=broadway BROADWAY_DISPLAY=:5 GDK_DEBUG=no-portals gio open'
    # The command lines of the Frame processes running.
    running='for p in /proc/[0-9]*; do tr \"\\\\0\" \" \" < \$p/cmdline 2>/dev/null; echo; done | grep \"^/home/tester/.local/bin/frame \" | sed \"s/^/running:/\"'

    echo '== no gtk'
    as \"curl -fsSL \$url/install.sh | FRAME_DOWNLOAD_URL=\$url/good sh\" && echo 'exit 0' || echo \"exit \$?\"

    echo '== tampered'
    as 'rm -rf ~/.local ~/.config'
    as \"curl -fsSL \$url/install.sh | FRAME_DOWNLOAD_URL=\$url/bad sh\" 2>&1 && echo 'exit 0' || echo \"exit \$?\"
    as 'ls ~/.local/bin/frame 2>/dev/null || echo nothing-installed'

    echo '== install'
    $gtk
    as 'mkdir -p ~/.config; printf \"[Default Applications]\nimage/png=eog.desktop\n$PPTX=libreoffice-impress.desktop\n\" > ~/.config/mimeapps.list'
    as \"curl -fsSL \$url/install.sh | FRAME_DOWNLOAD_URL=\$url/good sh\"
    as 'cat ~/.config/mimeapps.list; grep ^Exec= ~/.local/share/applications/dev.cronkite.Frame.desktop'
    as 'cd ~/.local/share/doc/frame && echo licences: \$(ls) && echo notice: \$(wc -c < THIRD-PARTY-RUST.txt) bytes, gtk4 \$(grep -c \"^gtk4 [0-9]\" THIRD-PARTY-RUST.txt)' || echo 'licences: none'
    as 'gio mime $PPTX | head -1 | sed s/^/pptx-/'
    as 'nohup gtk4-broadwayd :5 >/dev/null 2>&1 & sleep 1'
    as \"\$open /tmp/t.pptx; sleep 4\"
    as \"\$running\"
    as 'echo package copies: \$(ls ~/.cache/frame 2>/dev/null | grep -c \"^original-.*\\.pptx\$\")'

    echo '== update'
    as \"curl -fsSL \$url/install.sh | FRAME_DOWNLOAD_URL=\$url/good sh\" | head -1

    echo '== uninstall'
    as \"curl -fsSL \$url/install.sh | sh -s -- --uninstall\" >/dev/null
    as 'ls ~/.local/bin/frame ~/.local/share/applications/dev.cronkite.Frame.desktop ~/.local/share/icons/hicolor/scalable/apps/dev.cronkite.Frame.svg 2>&1 | grep -c \"No such\" | sed s/^/gone:/'
    as 'ls -d ~/.local/share/doc/frame 2>/dev/null || echo doc-gone'
    as 'grep -c Frame ~/.config/mimeapps.list | sed s/^/frame-defaults:/ || true'
    as 'grep ^image/png= ~/.config/mimeapps.list | sed s/^/kept:/'
  " 2>&1) || true
  ok=true
  check() { grep -q -- "$1" <<<"$out" || { echo "FAIL $distro: $2"; ok=false; }; }
  check "Installed Frame 9.9.9" "no install"
  check "$hint" "no '$hint' hint without GTK 4"
  check "checksum mismatch" "tampered tarball not refused"
  check "nothing-installed" "tampered tarball left files"
  check "^$PPTX=dev.cronkite.Frame.desktop$" ".pptx default not set"
  check "^image/png=eog.desktop$" "another app's .png default lost on install"
  check "Exec=/home/tester/.local/bin/frame %f" "desktop entry not pointing at the binary"
  check "^pptx-Default application for .*dev.cronkite.Frame.desktop" "gio doesn't name Frame for .pptx"
  check "^running:/home/tester/.local/bin/frame /tmp/t.pptx $" "double-click (gio open) did not start Frame on the .pptx"
  check "^package copies: 1$" "Frame didn't read the deck"
  check "^licences: LICENSE LICENSE-APACHE THIRD-PARTY-RUST.txt$" "licences not installed in ~/.local/share/doc/frame"
  check "^notice: [0-9]\{5,\} bytes, gtk4 1$" "THIRD-PARTY-RUST.txt empty or without gtk4"
  check "^gone:3$" "uninstall left files"
  check "^doc-gone$" "uninstall left ~/.local/share/doc/frame"
  check "^frame-defaults:0$" "uninstall left Frame's default"
  check "^kept:image/png=eog.desktop$" "install or uninstall lost another app's default"
  if $ok; then
    echo "ok   $distro: installs, refuses a bad checksum, hints '$hint', takes .pptx, gio open opens it, installs its licences, uninstalls"
    [ -n "${VERBOSE:-}" ] && sed 's/^/     /' <<<"$out"
  else
    sed 's/^/     /' <<<"$out" | tail -40; fail=1
  fi
done
[ $fail = 0 ] && echo "INSTALL OK" || { echo "INSTALL CHECK FAILED"; exit 1; }
