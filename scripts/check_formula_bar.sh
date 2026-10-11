#!/usr/bin/env bash
# FML-9 acceptance: the formula bar shows the cursor cell's formula and the name box its
# reference; editing in the bar is one undo step; the name box goes where it is told; a
# formula with a syntax error is refused with the character where it fails.
#
#   scripts/check_formula_bar.sh [BINARY]   (default target/release/bricks)
#
# In an Arch container under headless sway; the entries' text, the note under them, and
# the grid's description of the cursor cell (`B2: 4`) are read through the accessibility
# tree (scripts/a11y_helpers.py). /tmp/t.csv is `a,b\n2,=A2*2\n` (its first row a header):
#   1. Right to B2: the bar shows =A2*2, the name box B2;
#   2. Ctrl+G, A2, Enter: the name box shows A2 and the cursor is there;
#   3. on B2, Ctrl+G and Tab to the bar, =A2*5 Enter: B2 shows 10; Ctrl+Z: 4 again;
#   4. =A2+ in the bar: "Formula error at character 5";
#   5. Ctrl+G, Nope!A1, Enter: "No sheet named that";
#   6. from a second, empty sheet, Ctrl+G t!B2 goes back to the file's sheet, onto B2;
#   7. (FML-12) B2 copied and pasted on B3: B3 holds =A3*2.
# Needs docker. VERBOSE=1 prints the transcript; SHOTS=DIR saves screenshots.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(realpath "${1:-target/release/bricks}")
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p bricks)"; exit 1; }

shots=()
if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS"; shots=(-v "$(realpath "$SHOTS"):/shots"); fi
# Arch's sway binary carries cap_sys_nice: exec fails without it in the container's set.
out=$(docker run --rm --cap-add SYS_NICE -v "$bin:/usr/local/bin/bricks:ro" \
  -v "$PWD/scripts/a11y_helpers.py:/a11y.py:ro" "${shots[@]}" archlinux:latest bash -c '
set -uo pipefail
pacman -Syu --noconfirm --needed sway wtype grim gtk4 ttf-dejavu dbus python-gobject at-spi2-core >/dev/null 2>&1
mkdir -p /shots /tmp/rt && chmod 700 /tmp/rt
export XDG_RUNTIME_DIR=/tmp/rt GDK_DEBUG=no-portals XDG_STATE_HOME=/tmp/state GDK_BACKEND=wayland
printf "output HEADLESS-1 resolution 1280x720\ndefault_border none\n" > /tmp/sway.conf
printf "a,b\n2,=A2*2\n" > /tmp/t.csv
session() {
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway --unsupported-gpu -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
  for i in $(seq 50); do ls /tmp/rt/wayland-[0-9] >/dev/null 2>&1 && break; sleep 0.1; done
  export WAYLAND_DISPLAY=$(basename $(ls /tmp/rt/wayland-[0-9] | head -1))
  shot() { grim /shots/$1.png 2>/dev/null || true; }
  # The headless seat has no keyboard: each wtype adds a virtual one, and focus arrives
  # with it. Keys sent in the same instant can be lost, so wait before and after. Keys
  # that move the focus between entries go in one wtype run: the focus goes with it.
  keys() { wtype -s 400 "$@" -s 400; }
  shown() { python3 /a11y.py text | sed "s/^/${1:-shown}: /"; }

  bricks /tmp/t.csv >>/tmp/app.log 2>&1 & sleep 4
  echo "== 1 formula"
  keys -k Right; shot 1-formula; shown

  echo "== 2 name box"
  keys -M ctrl g -m ctrl -s 300 A2 -s 300 -k Return; sleep 1; shot 2-name; shown

  echo "== 3 edit"
  keys -k Right
  keys -M ctrl g -m ctrl -s 300 -k Tab -s 300 "=A2*5" -s 300 -k Return; sleep 1; shown edited
  keys -M ctrl z -m ctrl; sleep 1; shot 3-undone; shown undone

  echo "== 4 refused"
  keys -M ctrl g -m ctrl -s 300 -k Tab -s 300 "=A2+" -s 300 -k Return; sleep 1; shot 4-refused; shown
  keys -k Escape

  echo "== 5 no sheet"
  keys -M ctrl g -m ctrl -s 300 "Nope!A1" -s 300 -k Return; sleep 1; shot 5-nosheet; shown
  keys -k Escape

  echo "== 6 other sheet"
  keys -M shift -k F11 -m shift
  keys -M ctrl g -m ctrl -s 300 "t!B2" -s 300 -k Return; sleep 1; shot 6-other; shown

  echo "== 7 paste"
  keys -M ctrl c -m ctrl; sleep 1; keys -k Down; keys -M ctrl v -m ctrl; sleep 1
  shot 7-paste; shown
  pkill -x bricks
  swaymsg exit >/dev/null 2>&1 || true
}
dbus-run-session -- bash -c "$(declare -f session); session" 2>/tmp/dbus.log
echo "== app log"; grep "^bricks:" /tmp/app.log | head -20
' 2>&1) || true
[ -n "${VERBOSE:-}" ] && sed 's/^/  /' <<<"$out"
section() { awk -v s="== $1" '/^== /{on=($0 ~ "^" s)} on' <<<"$out"; }
fail=0
check() { # WHAT SECTION PATTERN
  if section "$2" | grep -qE -- "$3"; then echo "ok   $1"; else echo "FAIL $1 (no match for: $3)"; fail=1; fi
}
check "1: the bar shows B2's formula" "1 formula" "^shown: =A2\*2$"
check "1: the name box shows B2" "1 formula" "^shown: B2$"
check "2: the name box goes to A2" "2 name box" "^shown: A2: 2$"
check "2: and shows it" "2 name box" "^shown: A2$"
check "3: editing in the bar sets the cell" "3 edit" "^edited: B2: 10$"
check "3: one Ctrl+Z takes it back" "3 edit" "^undone: B2: 4$"
check "4: a formula with an error is refused, saying where" "4 refused" "^shown: Formula error at character 5: unexpected end$"
check "5: an unknown sheet is named" "5 no sheet" "^shown: No sheet named that$"
check "6: the name box goes to another sheet's cell" "6 other sheet" "^shown: B2: 4$"
check "7: a pasted formula follows its new row" "7 paste" "^shown: =A3\*2$"
check "7: on B3" "7 paste" "^shown: B3$"
if [ $fail = 0 ]; then echo "FORMULA BAR OK"; else
  echo "FORMULA BAR CHECK FAILED"; [ -n "${VERBOSE:-}" ] || echo "(VERBOSE=1 for the transcript)"
  exit 1
fi
