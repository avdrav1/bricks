#!/usr/bin/env bash
# SHEET-1 acceptance: sheets are added, renamed, and taken back with Ctrl+Z; each keeps
# its own cursor; a CSV with two sheets saves only as .xlsx.
#
#   scripts/check_sheets.sh [BINARY]   (default target/release/brick)
#
# In an Arch container under headless sway; the tabs and the grid's description of the
# cursor cell are read through the accessibility tree (scripts/a11y_helpers.py).
# /tmp/t.csv is `a,b\n2,=A2*2\n`:
#   1. Shift+F11 twice: tabs t, Sheet1, Sheet2;
#   2. Ctrl+Tab to the tabs, Right to Sheet1, F2: it is renamed to Q1 Sales;
#   3. Ctrl+Z renames it back; Ctrl+Z again takes Sheet2 away;
#   4. x typed in Sheet1 A1; Ctrl+Page Up and Down: each sheet shows its own cursor cell;
#   5. Ctrl+S opens Save As, saying a CSV file holds one sheet; choosing the file there
#      saves /tmp/t.xlsx with both sheets;
#   6. (SHEET-2) =t!A2*10 on Sheet1 shows 20; renaming t to Data keeps it showing 20
#      (the reference follows), and Ctrl+Z gives the tab its name back.
# Needs docker. VERBOSE=1 prints the transcript; SHOTS=DIR saves screenshots.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(realpath "${1:-target/release/brick}")
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p brick)"; exit 1; }

shots=()
if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS"; shots=(-v "$(realpath "$SHOTS"):/shots"); fi
# Arch's sway binary carries cap_sys_nice: exec fails without it in the container's set.
out=$(docker run --rm --cap-add SYS_NICE -v "$bin:/usr/local/bin/brick:ro" \
  -v "$PWD/scripts/a11y_helpers.py:/a11y.py:ro" "${shots[@]}" archlinux:latest bash -c '
set -uo pipefail
pacman -Syu --noconfirm --needed sway wtype grim gtk4 ttf-dejavu dbus python-gobject at-spi2-core unzip >/dev/null 2>&1
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
  # with it. Keys sent in the same instant can be lost, so wait before and after.
  keys() { wtype -s 400 "$@" -s 400; }
  shown() { python3 /a11y.py text | sed "s/^/${1:-shown}: /"; }

  brick /tmp/t.csv >>/tmp/app.log 2>&1 & sleep 4
  echo "== 1 added"
  keys -M shift -k F11 -m shift; keys -M shift -k F11 -m shift; shot 1-added; shown

  echo "== 2 renamed"
  # Ctrl+Tab takes the focus from the grid to the first tab, Right to the next one. In one
  # wtype run: the virtual keyboard, and with it the window focus, goes when wtype exits.
  wtype -s 400 -M ctrl -k Tab -m ctrl -s 300 -k Right -s 300 -k F2 -s 600 "Q1 Sales" -s 300 -k Return -s 600
  shot 2-renamed; shown

  echo "== 3 undone"
  keys -M ctrl z -m ctrl; shown first
  keys -M ctrl z -m ctrl; shot 3-undone; shown second

  echo "== 4 own cursors"
  keys x; keys -k Return; keys -k Up; shown sheet1
  keys -M ctrl -k Page_Up -m ctrl; shown t
  keys -M ctrl -k Page_Down -m ctrl; shown back

  echo "== 5 save as"
  keys -M ctrl s -m ctrl; sleep 1; shot 5-saveas; shown
  python3 /a11y.py press "Choose File…" && echo "chosen"; sleep 2; shot 5-file
  keys -k Return; sleep 6; shot 5-saved
  echo "xlsx: $(ls /tmp/t.xlsx 2>&1)"
  echo "sheets: $(unzip -p /tmp/t.xlsx xl/workbook.xml 2>/dev/null | grep -o "<sheet name=\"[^\"]*\"" | tr "\n" " ")"

  echo "== 6 cross-sheet"
  keys -k Right; keys "=t!A2*10"; keys -k Return; keys -k Up; sleep 1; shown formula
  wtype -s 400 -M ctrl -k Tab -m ctrl -s 300 -k F2 -s 600 "Data" -s 300 -k Return -s 600
  sleep 2; shown renamed
  # Undo shows the renamed sheet; the formula is on the next one.
  keys -M ctrl z -m ctrl; sleep 1; shown undone
  keys -M ctrl -k Page_Down -m ctrl; shown again
  pkill -x brick
  swaymsg exit >/dev/null 2>&1 || true
}
dbus-run-session -- bash -c "$(declare -f session); session" 2>/tmp/dbus.log
echo "== app log"; grep "^brick:" /tmp/app.log | head -20
' 2>&1) || true
[ -n "${VERBOSE:-}" ] && sed 's/^/  /' <<<"$out"
section() { awk -v s="== $1" '/^== /{on=($0 ~ "^" s)} on' <<<"$out"; }
fail=0
check() { # WHAT SECTION PATTERN
  if section "$2" | grep -E -- "$3" >/dev/null; then echo "ok   $1"; else echo "FAIL $1 (no match for: $3)"; fail=1; fi
}
refute() { # WHAT SECTION PATTERN
  if section "$2" | grep -E -- "$3" >/dev/null; then echo "FAIL $1 (matched: $3)"; fail=1; else echo "ok   $1"; fi
}
check "1: the file's own tab" "1 added" "^shown: t$"
check "1: Shift+F11 adds Sheet1" "1 added" "^shown: Sheet1$"
check "1: and Sheet2" "1 added" "^shown: Sheet2$"
check "2: F2 renames Sheet1 to Q1 Sales" "2 renamed" "^shown: Q1 Sales$"
refute "2: no Sheet1 left" "2 renamed" "^shown: Sheet1$"
check "3: Ctrl+Z renames it back" "3 undone" "^first: Sheet1$"
refute "3: the name it had is gone" "3 undone" "^first: Q1 Sales$"
refute "3: Ctrl+Z again takes Sheet2 away" "3 undone" "^second: Sheet2$"
check "3: and keeps Sheet1" "3 undone" "^second: Sheet1$"
check "4: Sheet1's A1 shows what was typed" "4 own cursors" "^sheet1: A1: x$"
check "4: the file's sheet has its own cursor (its first row, under the header)" "4 own cursors" "^t: A2: 2$"
check "4: and Sheet1 comes back as it was" "4 own cursors" "^back: A1: x$"
check "5: Ctrl+S asks for .xlsx" "5 save as" "A CSV file holds one sheet"
check "5: the workbook saves as .xlsx" "5 save as" "^xlsx: /tmp/t.xlsx$"
check "5: with both sheets" "5 save as" "^sheets: <sheet name=\"t\" <sheet name=\"Sheet1\""
check "6: a reference to another sheet shows its cell" "6 cross-sheet" "^formula: B1: 20$"
check "6: renaming that sheet keeps the reference working" "6 cross-sheet" "^renamed: B1: 20$"
check "6: the tab has its new name" "6 cross-sheet" "^renamed: Data$"
check "6: Ctrl+Z gives the old name back" "6 cross-sheet" "^undone: t$"
check "6: and the reference still works" "6 cross-sheet" "^again: B1: 20$"
if [ $fail = 0 ]; then echo "SHEETS OK"; else
  echo "SHEETS CHECK FAILED"; [ -n "${VERBOSE:-}" ] || echo "(VERBOSE=1 for the transcript)"
  exit 1
fi
