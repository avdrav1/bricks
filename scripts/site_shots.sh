#!/usr/bin/env bash
# Screenshots for cronkite.cc's gallery (sites/cronkite/index.html), taken from each app's
# latest release.
#
# Usage: scripts/site_shots.sh [OUTDIR]   (sites/cronkite/shots by default)
#
# Downloads Brick, Slate, Frame, and Etch from dl.cronkite.cc (checking each sha256), builds
# the demo files (scripts/site_demo.py), and runs the apps in an Arch container under
# headless sway at 1440x900, scale 2, with GNOME's fonts and icons. Each shot is the whole
# screen:
#   brick-rows       the 1 GB CSV (corpus/rows_1024mb.csv), Ctrl+End at row 19 million
#   brick-find       the same file with Ctrl+F portland, matches counted
#   brick-workbook   budget.xlsx with the grand total's =SUM in the formula bar
#   slate-docx       report.docx on Slate's pageless column
#   slate-find       notes.md with Ctrl+F tune-up highlighting its matches
#   frame-deck       review.pptx on the chart slide, slide list and inspector beside it
#   frame-outline    the same deck in the outline view (Ctrl+Shift+O)
#   etch-paint       a poster designed on Etch's blank image with its fill, ellipse, line,
#                    rectangle, and text tools (scripts/etch_poster.py, through a virtual
#                    pointer and the accessibility tree)
# and each becomes NAME.webp (2880 px wide), NAME-1920.webp, and NAME-960.webp.
# Needs docker, curl, LibreOffice with Python UNO, ImageMagick, and the corpus
# (python3 scripts/gen_corpus.py; CORPUS=DIR points at another worktree's corpus/).
# VERBOSE=1 prints the container's transcript.
set -euo pipefail
cd "$(dirname "$0")/.."
out=$(realpath -m "${1:-sites/cronkite/shots}")
csv=$(realpath -m "${CORPUS:-corpus}/rows_1024mb.csv")
[ -f "$csv" ] || { echo "no $csv (python3 scripts/gen_corpus.py)"; exit 1; }
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin" "$tmp/raw" "$out"

# The app's latest release: download, check, and unpack its binary into $tmp/bin.
fetch() {
  local app=$1 base=$2 json url sum
  json=$(curl -fsS "$base/latest.json")
  url=$(printf '%s' "$json" | sed -n 's/.*"url":"\([^"]*\)".*/\1/p')
  sum=$(printf '%s' "$json" | sed -n 's/.*"sha256":"\([^"]*\)".*/\1/p')
  curl -fsSL -o "$tmp/$app.tar.gz" "$url"
  echo "$sum  $tmp/$app.tar.gz" | sha256sum -c --quiet
  tar -xzf "$tmp/$app.tar.gz" -C "$tmp" --wildcards "*/$app"
  mv "$tmp"/"$app"-*/"$app" "$tmp/bin/$app"
  echo "$app $(printf '%s' "$json" | sed -n 's/.*"version":"\([^"]*\)".*/\1/p')"
}
fetch brick https://dl.cronkite.cc
fetch slate https://dl.cronkite.cc/slate
fetch frame https://dl.cronkite.cc/frame
fetch etch https://dl.cronkite.cc/etch
python3 scripts/site_demo.py "$tmp/files" >/dev/null

# Arch's sway binary carries cap_sys_nice: exec fails without it in the container's set.
log=$(docker run --rm --cap-add SYS_NICE -v "$tmp/bin:/opt/bin:ro" -v "$tmp/files:/data:ro" \
  -v "$csv:/big/rows.csv:ro" -v "$PWD/scripts:/scripts:ro" -v "$tmp/raw:/shots" archlinux:latest bash -c '
set -uo pipefail
pacman -Syu --noconfirm --needed sway wtype grim gtk4 dbus adwaita-fonts adwaita-icon-theme \
  noto-fonts ttf-liberation ttf-carlito ttf-caladea shared-mime-info python-gobject \
  python-pywayland wlr-protocols wayland pkgconf at-spi2-core >/dev/null 2>&1
mkdir -p /tmp/rt /tmp/home /tmp/f /tmp/proto && chmod 700 /tmp/rt && cp /data/* /tmp/f/
touch /tmp/proto/__init__.py
python -m pywayland.scanner -i /usr/share/wayland/wayland.xml \
  /usr/share/wlr-protocols/unstable/wlr-virtual-pointer-unstable-v1.xml -o /tmp/proto/wlr_proto >/dev/null
export PYTHONPATH=/tmp/proto
export PATH=/opt/bin:$PATH XDG_RUNTIME_DIR=/tmp/rt HOME=/tmp/home GDK_DEBUG=no-portals GDK_BACKEND=wayland
printf "output HEADLESS-1 resolution 2880x1800 scale 2 position 0 0\ndefault_border none\n" > /tmp/sway.conf
WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway --unsupported-gpu -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
for i in $(seq 50); do ls /tmp/rt/wayland-[0-9] >/dev/null 2>&1 && break; sleep 0.1; done
export WAYLAND_DISPLAY=$(basename $(ls /tmp/rt/wayland-[0-9] | head -1))
export SWAYSOCK=$(ls /tmp/rt/sway-ipc.*.sock | head -1)
key() { wtype -s 300 "$@" -s 300; }
shot() { sleep 2; grim /shots/$1.png && echo "shot $1"; }
quit() { pkill -x "$1"; sleep 1; pkill -9 -x "$1" 2>/dev/null; sleep 0.5; }

brick /big/rows.csv >>/tmp/app.log 2>&1 & sleep 12
key -M ctrl -k End -m ctrl
shot brick-rows
key -M ctrl -k Home -m ctrl
key -M ctrl -k f -m ctrl
key portland
sleep 8
shot brick-find
quit brick

brick /tmp/f/budget.xlsx >>/tmp/app.log 2>&1 & sleep 4
key -M ctrl -k Home -m ctrl
for i in $(seq 10); do key -k Down; done
for i in 1 2 3 4 5; do key -k Right; done
shot brick-workbook
quit brick

slate /tmp/f/report.docx >>/tmp/app.log 2>&1 & sleep 4
shot slate-docx
quit slate

slate /tmp/f/notes.md >>/tmp/app.log 2>&1 & sleep 4
key -M ctrl -k f -m ctrl
key tune-up
shot slate-find
quit slate

frame /tmp/f/review.pptx >>/tmp/app.log 2>&1 & sleep 5
key -k Next
key -k Next
shot frame-deck
key -M ctrl -M shift -k o -m shift -m ctrl
shot frame-outline
quit frame

# The Etch poster presses the tool bar through the accessibility tree: a D-Bus session.
etch_poster() {
  etch >>/tmp/app.log 2>&1 & sleep 4
  key -M ctrl 0 -m ctrl
  sleep 1; grim /tmp/blank.png
  python3 /scripts/etch_poster.py /tmp/blank.png 1440 900 2 >>/tmp/app.log 2>&1 || echo "etch poster failed"
  shot etch-paint
  quit etch
}
dbus-run-session -- bash -c "$(declare -f key shot quit etch_poster); etch_poster" 2>/tmp/dbus.log
swaymsg exit >/dev/null 2>&1 || true
echo "== app log"; cat /tmp/app.log
' 2>&1) || { echo "$log"; exit 1; }
[ -n "${VERBOSE:-}" ] && echo "$log"

for png in "$tmp"/raw/*.png; do
  name=$(basename "$png" .png)
  magick "$png" -quality 82 -define webp:method=6 "$out/$name.webp"
  for w in 1920 960; do
    magick "$png" -resize ${w}x -quality 80 -define webp:method=6 "$out/$name-$w.webp"
  done
  echo "$name: $(identify -format %wx%h "$png")"
done
ls -la "$out"
