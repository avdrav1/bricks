#!/usr/bin/env bash
# APP-5 acceptance: dropping files on the app opens them, on Wayland and on X11.
#
#   scripts/check_drop.sh [BINARY]   (default target/release/spreadsheet)
#
# In an Arch container, once under headless sway (Wayland) and once under Xvfb (X11):
#   1. the app starts with no file (start window), a drag source window (a GTK window that
#      offers a file as text/uri-list, as file managers do) starts next to it;
#   2. the pointer drags the source onto the app: the file must open in a window titled
#      with its name;
#   3. a second file is dragged onto that file's window: it opens too.
# The pointer is real input to the display server: sway gets it through the
# wlr-virtual-pointer protocol (dnd_helpers.py drag), Xvfb through XTEST (xdotool).
# Needs docker. VERBOSE=1 prints the transcript.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(realpath "${1:-target/release/spreadsheet}")
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p spreadsheet)"; exit 1; }

# Arch's sway binary carries cap_sys_nice: exec fails without it in the container's set.
out=$(docker run --rm --cap-add SYS_NICE -v "$bin:/usr/local/bin/spreadsheet:ro" -v "$PWD/scripts:/scripts:ro" archlinux:latest bash -c '
set -euo pipefail
pacman -Syu --noconfirm --needed sway xorg-server-xvfb xdotool python-gobject python-pywayland \
  wlr-protocols wayland pkgconf gtk4 jq ttf-dejavu dbus >/dev/null 2>&1
mkdir -p /tmp/proto /tmp/rt && chmod 700 /tmp/rt && touch /tmp/proto/__init__.py
python -m pywayland.scanner -i /usr/share/wayland/wayland.xml \
  /usr/share/wlr-protocols/unstable/wlr-virtual-pointer-unstable-v1.xml -o /tmp/proto/wlr_proto >/dev/null
export PYTHONPATH=/tmp/proto XDG_RUNTIME_DIR=/tmp/rt GDK_DEBUG=no-portals
printf "id,name\n1,a\n" > /tmp/first.csv; printf "id,name\n2,b\n" > /tmp/second.csv
helpers=/scripts/dnd_helpers.py

wayland() {
  export XDG_STATE_HOME=/tmp/state-wayland
  printf "output HEADLESS-1 resolution 1280x720 position 0 0\ndefault_border none\n" > /tmp/sway.conf
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
  for i in $(seq 50); do [ -S /tmp/rt/wayland-1 ] && break; sleep 0.1; done
  [ -S /tmp/rt/wayland-1 ] || { echo "sway did not start:"; tail -15 /tmp/sway.log; ls /tmp/rt; }
  export WAYLAND_DISPLAY=wayland-1 GDK_BACKEND=wayland SWAYSOCK=$(ls /tmp/rt/sway-ipc.*.sock)
  titles() { swaymsg -t get_tree | jq -r ".. | objects | select(.type==\"con\" and .name!=null) | .name" | sort | tr "\n" ","; }
  spreadsheet >/tmp/app-w.log 2>&1 & sleep 2
  for f in first second; do
    python $helpers source /tmp/$f.csv >/dev/null 2>&1 & src=$!; sleep 2
    echo "wayland before $f: $(titles)"
    python $helpers drag 960 360 320 360
    sleep 2; kill $src; wait $src 2>/dev/null || true; sleep 1
    echo "wayland after $f: $(titles)"
  done
  echo "wayland recent: $(tr "\n" " " < $XDG_STATE_HOME/spreadsheet/recent)"
  swaymsg exit >/dev/null 2>&1 || true
  unset WAYLAND_DISPLAY SWAYSOCK
}

x11() {
  export XDG_STATE_HOME=/tmp/state-x11 GDK_BACKEND=x11 DISPLAY=:9
  Xvfb :9 -screen 0 1280x720x24 >/dev/null 2>&1 & sleep 1
  titles() { for w in $(xdotool search --onlyvisible --name "." 2>/dev/null); do xdotool getwindowname $w; done | sort | tr "\n" ","; }
  spreadsheet >/tmp/app-x.log 2>&1 &
  xdotool search --sync --onlyvisible --name "^Spreadsheet$" windowmove 0 0 windowsize 600 600 >/dev/null
  for f in first second; do
    python $helpers source /tmp/$f.csv >/dev/null 2>&1 & src=$!
    xdotool search --sync --onlyvisible --name "^dragsrc$" windowmove 650 0 windowsize 600 600 >/dev/null
    sleep 1
    echo "x11 before $f: $(titles)"
    xdotool mousemove 950 300 sleep 0.3 mousedown 1
    for x in $(seq 930 -20 300); do xdotool mousemove $x 300; sleep 0.02; done
    xdotool sleep 0.5 mouseup 1
    sleep 2; kill $src; wait $src 2>/dev/null || true; sleep 1
    echo "x11 after $f: $(titles)"
    # The new window maps at 0,0 on top: where the next drop lands.
    for w in $(xdotool search --onlyvisible --name "\.csv$" 2>/dev/null); do xdotool windowmove $w 0 0 windowsize $w 600 600; done
  done
  echo "x11 recent: $(tr "\n" " " < $XDG_STATE_HOME/spreadsheet/recent)"
}

dbus-run-session -- bash -c "$(declare -f wayland x11); helpers=$helpers; wayland; x11"
' 2>&1) || true
[ -n "${VERBOSE:-}" ] && sed 's/^/  /' <<<"$out"
fail=0
for display in wayland x11; do
  first=$(grep "^$display after first:" <<<"$out" || true)
  second=$(grep "^$display after second:" <<<"$out" || true)
  recent=$(grep "^$display recent:" <<<"$out" || true)
  if [[ "$first" == *first.csv* && "$second" == *second.csv* && "$recent" == *"/tmp/second.csv /tmp/first.csv"* ]]; then
    echo "ok   $display: first.csv dropped on the start window opened; second.csv dropped on its window opened"
  else
    echo "FAIL $display"; grep "^$display" <<<"$out" | sed 's/^/     /'; fail=1
  fi
done
if [ $fail = 0 ]; then echo "DROP OK"; else
  [ -n "${VERBOSE:-}" ] || tail -25 <<<"$out" | sed 's/^/  /'
  echo "DROP CHECK FAILED"; exit 1
fi
