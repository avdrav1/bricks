#!/usr/bin/env bash
# GRID-6 acceptance: empty cells fill the window past the data and grow as you go.
#
#   scripts/check_reach.sh [BINARY]   (default target/release/bricks)
#
# In an Arch container under headless sway (1280x720); the cursor cell is read from the
# grid's description through the accessibility tree (scripts/a11y_helpers.py), the
# pointer is the wlr-virtual-pointer protocol (scripts/dnd_helpers.py).
#   1. Ctrl+N gives a blank sheet: the cursor moves 30 rows down and 12 columns right
#      (before GRID-6 a blank sheet was one cell, and it stayed on A1);
#   2. /tmp/t.csv (a header and two rows): E40 typed via the name box makes 39 rows; one
#      Ctrl+Z takes them all back (2 rows) and Ctrl+End then goes to the last cell with
#      data, B3; Ctrl+Y and Ctrl+S write 40 lines, the last ",,,,x";
#   3. Page Down 20 times from A1 goes on past every empty row first shown;
#   4. 200 wheel notches down over the grid, then a click mid-grid: the cell clicked is far
#      below the rows first shown (past the data and the first screens of empty cells);
#   5. Ctrl+B on C45, five rows past the saved data, adds the rows and bolds the cell, as
#      one step Ctrl+Z takes back; Delete on a range from A60 up into the data, the cursor
#      on A60, empties the data's cells and adds no rows.
# Needs docker. VERBOSE=1 prints the transcript; SHOTS=DIR saves screenshots.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(realpath "${1:-target/release/bricks}")
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p bricks)"; exit 1; }

shots=()
if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS"; shots=(-v "$(realpath "$SHOTS"):/shots"); fi
# Arch's sway binary carries cap_sys_nice: exec fails without it in the container's set.
out=$(docker run --rm --cap-add SYS_NICE -v "$bin:/usr/local/bin/bricks:ro" \
  -v "$PWD/scripts:/scripts:ro" "${shots[@]}" archlinux:latest bash -c '
set -uo pipefail
pacman -Syu --noconfirm --needed sway wtype grim gtk4 ttf-dejavu dbus python-gobject \
  at-spi2-core python-pywayland wlr-protocols wayland pkgconf >/dev/null 2>&1
mkdir -p /shots /tmp/rt /tmp/proto && chmod 700 /tmp/rt && touch /tmp/proto/__init__.py
python -m pywayland.scanner -i /usr/share/wayland/wayland.xml \
  /usr/share/wlr-protocols/unstable/wlr-virtual-pointer-unstable-v1.xml -o /tmp/proto/wlr_proto >/dev/null
export PYTHONPATH=/tmp/proto XDG_RUNTIME_DIR=/tmp/rt GDK_DEBUG=no-portals XDG_STATE_HOME=/tmp/state GDK_BACKEND=wayland
printf "output HEADLESS-1 resolution 1280x720 position 0 0\ndefault_border none\n" > /tmp/sway.conf
printf "a,b\n1,2\n3,4\n" > /tmp/t.csv
session() {
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway --unsupported-gpu -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
  for i in $(seq 50); do ls /tmp/rt/wayland-[0-9] >/dev/null 2>&1 && break; sleep 0.1; done
  export WAYLAND_DISPLAY=$(basename $(ls /tmp/rt/wayland-[0-9] | head -1))
  shot() { grim /shots/$1.png 2>/dev/null || true; }
  keys() { wtype -s 300 "$@" -s 300; }
  go() { wtype -s 400 -M ctrl g -m ctrl -s 300 "$1" -s 300 -k Return -s 600; }
  cursor() { python3 /scripts/a11y_helpers.py text | grep -E "^[A-Z]+[0-9]+: " | head -1 | sed "s/^/$1: /"; }
  rows() { python3 /scripts/a11y_helpers.py text | grep -E "^[0-9,]+ rows" | head -1 | sed "s/^/$1: /"; }
  bold() { echo "$1: bold $(python3 /scripts/a11y_helpers.py state Bold 2>&1)"; }
  # wtype may drop its first key while the compositor takes its keymap: wait first.
  times() { local n=$1; shift; local args=(); for i in $(seq $n); do args+=("$@" -s 60); done; wtype -s 300 "${args[@]}"; sleep 0.5; }

  bricks >>/tmp/app.log 2>&1 & sleep 3
  echo "== 1 blank"
  keys -M ctrl n -m ctrl; sleep 2
  shot 1-blank
  times 30 -k Down; times 12 -k Right
  cursor blank
  shot 1-moved
  pkill -x bricks; sleep 1

  bricks /tmp/t.csv >>/tmp/app.log 2>&1 & sleep 3
  echo "== 2 typed"
  go E40; keys x -k Return; rows typed
  keys -M ctrl z -m ctrl; rows undone
  keys -M ctrl -k End -m ctrl; cursor end
  keys -M ctrl y -m ctrl; keys -M ctrl s -m ctrl; sleep 2
  echo "lines: $(wc -l < /tmp/t.csv)"; echo "line40: $(sed -n 40p /tmp/t.csv)"
  shot 2-typed

  echo "== 3 pages"
  keys -M ctrl -k Home -m ctrl; times 20 -k Page_Down; cursor pages
  shot 3-pages

  echo "== 4 wheel"
  keys -M ctrl -k Home -m ctrl
  python /scripts/dnd_helpers.py wheel 640 400 200; sleep 1
  shot 4-wheel
  python /scripts/dnd_helpers.py click 640 400; sleep 1
  cursor wheel

  echo "== 5 format and clear"
  go C45; keys -M ctrl b -m ctrl; sleep 1; rows formatted; bold formatted
  keys -M ctrl z -m ctrl; rows unformatted; bold unformatted
  # A range from past the data into it, the cursor at its far end, cleared.
  go A60; keys -M shift -M ctrl -k Home -m ctrl -m shift; keys -k Delete; sleep 1
  rows cleared; go A2; cursor cleared
  pkill -x bricks
  swaymsg exit >/dev/null 2>&1 || true
}
dbus-run-session -- bash -c "$(declare -f session); session" 2>/tmp/dbus.log
echo "== app log"; grep -E "^bricks:|panicked" /tmp/app.log | head -20
' 2>&1) || true
[ -n "${VERBOSE:-}" ] && sed 's/^/  /' <<<"$out"
section() { awk -v s="== $1" '/^== /{on=($0 ~ "^" s)} on' <<<"$out"; }
fail=0
check() { # WHAT SECTION PATTERN
  if section "$2" | grep -E -- "$3" >/dev/null; then echo "ok   $1"; else echo "FAIL $1 (no match for: $3)"; fail=1; fi
}
# The number on the cursor line, for range checks.
row_of() { section "$1" | sed -nE "s/^$2: [A-Z]+([0-9]+): .*/\1/p" | head -1; }
at_least() { # WHAT SECTION LABEL MIN
  local r; r=$(row_of "$2" "$3")
  if [ -n "$r" ] && [ "$r" -ge "$4" ]; then echo "ok   $1 (row $r)"; else echo "FAIL $1 (row '$r', want >= $4)"; fail=1; fi
}
check "1: a blank sheet's cursor moves into the empty cells" "1 blank" "^blank: M31: "
check "2: E40 adds the rows up to it" "2 typed" "^typed: 39 rows"
check "2: one Ctrl+Z takes them back" "2 typed" "^undone: 2 rows"
check "2: Ctrl+End goes to the last cell with data" "2 typed" "^end: B3: 4$"
check "2: E40 saves 40 lines" "2 typed" "^lines: 40$"
check "2: the 40th is the typed cell" "2 typed" "^line40: ,,,,x$"
at_least "3: Page Down goes on past the empty rows" "3 pages" pages 300
at_least "4: the wheel scrolls on past the empty rows" "4 wheel" wheel 200
check "5: Ctrl+B past the data adds the rows" "5 format" "^formatted: 44 rows"
check "5: and the cell is bold" "5 format" "^formatted: bold on$"
check "5: one Ctrl+Z takes back both" "5 format" "^unformatted: 39 rows"
check "5: the bold too" "5 format" "^unformatted: bold off$"
check "5: Delete from past the data adds no rows" "5 format" "^cleared: 39 rows"
check "5: and empties the cells in the data" "5 format" "^cleared: A2: $"
if section "app log" | grep -q panicked; then echo "FAIL the app panicked"; fail=1; else echo "ok   no panic"; fi
if [ $fail = 0 ]; then echo "REACH OK"; else
  echo "REACH CHECK FAILED"; [ -n "${VERBOSE:-}" ] || echo "(VERBOSE=1 for the transcript)"
  exit 1
fi
