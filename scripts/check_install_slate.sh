#!/usr/bin/env bash
# SLATE-9 acceptance: piping Slate's install.sh into sh installs the release per user on
# clean Arch, Ubuntu 24.04, and Fedora; the checksum is verified; double-clicking a .docx
# or .md opens Slate; and without GTK 4 it says how to get it.
#
#   scripts/check_install_slate.sh dist/slate-X.Y.Z-x86_64-linux.tar.gz
#
# Serves a throwaway download site (the tarball, latest.json, install.sh, and a .docx and
# .md to open) on localhost and, per distro, in fresh containers as a normal user:
#   1. without GTK 4: install, and the hint names the distro's package;
#   2. with a tampered tarball: refused, nothing installed;
#   3. with GTK 4 and other apps' defaults for .docx, .md, and .png: install; the .docx and
#      .md defaults are Slate's, the .png one stays; `gio open` (what a file manager's
#      double-click does) on t.docx and t.md each starts Slate on that file, and it is
#      still running 4 s later (the .docx window holds its private package copy). Then
#      reinstall (update) and --uninstall: nothing of Slate's is left, the .png default is.
# Needs docker and python3. VERBOSE=1 prints each container's transcript; ONLY=Fedora (or
# Arch, Ubuntu) runs one distro.
set -euo pipefail
cd "$(dirname "$0")/.."
tar=$(realpath "${1:?usage: check_install_slate.sh TARBALL}")
name=$(basename "$tar")
DOCX=application/vnd.openxmlformats-officedocument.wordprocessingml.document

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
cp sites/slate/install.sh "$site/install.sh"
cp apps/slate/crates/docx/tests/data/fixture.docx "$site/t.docx"
printf '# Notes\n\nA *Markdown* file.\n' > "$site/t.md"
chmod -R a+rX "$site"
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$site" >/dev/null 2>&1 &
server=$!
sleep 1

# distro | image | GTK 4 runtime + tools the test itself uses | the hint expected without GTK
checks=(
  "Arch|archlinux:latest|pacman -Syu --noconfirm --needed gtk4 >/dev/null|pacman -S gtk4"
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
    curl -fsS -o /tmp/t.docx \$url/t.docx; curl -fsS -o /tmp/t.md \$url/t.md; chmod a+r /tmp/t.docx /tmp/t.md
    open='GDK_BACKEND=broadway BROADWAY_DISPLAY=:5 GDK_DEBUG=no-portals gio open'
    # The command lines of the Slate processes running.
    running='for p in /proc/[0-9]*; do tr \"\\\\0\" \" \" < \$p/cmdline 2>/dev/null; echo; done | grep \"^/home/tester/.local/bin/slate \" | sed \"s/^/running:/\"'

    echo '== no gtk'
    as \"curl -fsSL \$url/install.sh | SLATE_DOWNLOAD_URL=\$url/good sh\" && echo 'exit 0' || echo \"exit \$?\"

    echo '== tampered'
    as 'rm -rf ~/.local ~/.config'
    as \"curl -fsSL \$url/install.sh | SLATE_DOWNLOAD_URL=\$url/bad sh\" 2>&1 && echo 'exit 0' || echo \"exit \$?\"
    as 'ls ~/.local/bin/slate 2>/dev/null || echo nothing-installed'

    echo '== install'
    $gtk
    as 'mkdir -p ~/.config; printf \"[Default Applications]\nimage/png=eog.desktop\n$DOCX=libreoffice-writer.desktop\ntext/markdown=org.gnome.TextEditor.desktop\n\" > ~/.config/mimeapps.list'
    as \"curl -fsSL \$url/install.sh | SLATE_DOWNLOAD_URL=\$url/good sh\"
    as 'cat ~/.config/mimeapps.list; grep ^Exec= ~/.local/share/applications/dev.cronkite.Slate.desktop'
    as 'gio mime $DOCX | head -1 | sed s/^/docx-/; gio mime text/markdown | head -1 | sed s/^/md-/'
    as 'nohup gtk4-broadwayd :5 >/dev/null 2>&1 & sleep 1'
    as \"\$open /tmp/t.docx; \$open /tmp/t.md; sleep 4\"
    as \"\$running\"
    as 'echo package copies: \$(ls ~/.cache/slate | grep -c \"^original-.*\\.docx\$\")'

    echo '== update'
    as \"curl -fsSL \$url/install.sh | SLATE_DOWNLOAD_URL=\$url/good sh\" | head -1

    echo '== uninstall'
    as \"curl -fsSL \$url/install.sh | sh -s -- --uninstall\" >/dev/null
    as 'ls ~/.local/bin/slate ~/.local/share/applications/dev.cronkite.Slate.desktop ~/.local/share/icons/hicolor/scalable/apps/dev.cronkite.Slate.svg 2>&1 | grep -c \"No such\" | sed s/^/gone:/'
    as 'grep -c Slate ~/.config/mimeapps.list | sed s/^/slate-defaults:/ || true'
    as 'grep ^image/png= ~/.config/mimeapps.list | sed s/^/kept:/'
  " 2>&1) || true
  ok=true
  check() { grep -q -- "$1" <<<"$out" || { echo "FAIL $distro: $2"; ok=false; }; }
  check "Installed Slate 9.9.9" "no install"
  check "$hint" "no '$hint' hint without GTK 4"
  check "checksum mismatch" "tampered tarball not refused"
  check "nothing-installed" "tampered tarball left files"
  check "^$DOCX=dev.cronkite.Slate.desktop$" ".docx default not set"
  check "^text/markdown=dev.cronkite.Slate.desktop$" ".md default not set"
  check "^image/png=eog.desktop$" "another app's .png default lost on install"
  check "Exec=/home/tester/.local/bin/slate %F" "desktop entry not pointing at the binary"
  check "^docx-Default application for .*dev.cronkite.Slate.desktop" "gio doesn't name Slate for .docx"
  check "^md-Default application for .*dev.cronkite.Slate.desktop" "gio doesn't name Slate for .md"
  check "^running:/home/tester/.local/bin/slate /tmp/t.docx $" "double-click (gio open) did not start Slate on the .docx"
  check "^running:/home/tester/.local/bin/slate /tmp/t.md $" "double-click (gio open) did not start Slate on the .md"
  check "^package copies: 1$" "the .docx window doesn't hold its document"
  check "^gone:3$" "uninstall left files"
  check "^slate-defaults:0$" "uninstall left Slate's defaults"
  check "^kept:image/png=eog.desktop$" "install or uninstall lost another app's default"
  if $ok; then
    echo "ok   $distro: installs, refuses a bad checksum, hints '$hint', takes .docx and .md, gio open opens both, uninstalls"
    [ -n "${VERBOSE:-}" ] && sed 's/^/     /' <<<"$out"
  else
    sed 's/^/     /' <<<"$out" | tail -40; fail=1
  fi
done
[ $fail = 0 ] && echo "INSTALL OK" || { echo "INSTALL CHECK FAILED"; exit 1; }
