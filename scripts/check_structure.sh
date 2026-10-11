#!/usr/bin/env bash
# FML-10 smoke: formulas follow inserted and deleted rows, as one undo step with the rows.
#
#   scripts/check_structure.sh [BINARY]   (default target/release/bricks)
#
# In an Arch container under headless sway; formulas are read from the formula bar and
# results from the grid's description of the cursor cell, through the accessibility
# tree (scripts/a11y_helpers.py). /tmp/s.csv (no header row) is
#   1,=SUM(A1:A3),=A2
#   2,=A2*10
#   3,x
#   1. a row inserted above row 2: B1 reads =SUM(A1:A4), and B3 (the old B2) =A3*10, 20;
#   2. Ctrl+Z: B1 reads =SUM(A1:A3) again;
#   3. row 2 deleted: B1 reads =SUM(A1:A2) and C1 =#REF!; Ctrl+Z: C1 reads =A2 again.
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
printf "1,=SUM(A1:A3),=A2\n2,=A2*10\n3,x\n" > /tmp/s.csv
session() {
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway --unsupported-gpu -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
  for i in $(seq 50); do ls /tmp/rt/wayland-[0-9] >/dev/null 2>&1 && break; sleep 0.1; done
  export WAYLAND_DISPLAY=$(basename $(ls /tmp/rt/wayland-[0-9] | head -1))
  shot() { grim /shots/$1.png 2>/dev/null || true; }
  keys() { wtype -s 400 "$@" -s 400; }
  go() { wtype -s 400 -M ctrl g -m ctrl -s 300 "$1" -s 300 -k Return -s 600; }
  shown() { python3 /a11y.py text | sed "s/^/${1:-shown}: /"; }

  bricks /tmp/s.csv >>/tmp/app.log 2>&1 & sleep 4
  echo "== 1 inserted"
  go A2; keys -M ctrl -k KP_Add -m ctrl; sleep 1
  go B1; shown b1
  go B3; shown b3
  shot 1-inserted

  echo "== 2 undone"
  keys -M ctrl z -m ctrl; sleep 1; go B1; shown b1

  echo "== 3 deleted"
  go A2; keys -M ctrl -k KP_Subtract -m ctrl; sleep 1
  go B1; shown b1
  go C1; shown c1
  keys -M ctrl z -m ctrl; sleep 1; go C1; shown back
  shot 3-deleted
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
  if section "$2" | grep -E -- "$3" >/dev/null; then echo "ok   $1"; else echo "FAIL $1 (no match for: $3)"; fail=1; fi
}
check "1: a range grows over an inserted row" "1 inserted" "^b1: =SUM\(A1:A4\)$"
check "1: a moved formula's reference follows its row" "1 inserted" "^b3: =A3\*10$"
check "1: and still reads the same cell" "1 inserted" "^b3: B3: 20$"
check "2: one Ctrl+Z takes back rows and formulas" "2 undone" "^b1: =SUM\(A1:A3\)$"
check "3: a range shrinks over a deleted row" "3 deleted" "^b1: =SUM\(A1:A2\)$"
check "3: a reference to the deleted row is #REF!" "3 deleted" "^c1: =#REF!$"
check "3: Ctrl+Z gives it back" "3 deleted" "^back: =A2$"
if [ $fail = 0 ]; then echo "STRUCTURE OK"; else
  echo "STRUCTURE CHECK FAILED"; [ -n "${VERBOSE:-}" ] || echo "(VERBOSE=1 for the transcript)"
  exit 1
fi
