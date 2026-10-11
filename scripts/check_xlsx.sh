#!/usr/bin/env bash
# XLSX-1 and XLSX-2 acceptance: an .xlsx workbook opens with every sheet as a tab, names
# what a save would drop, asks before saving over it, saves as .xlsx, and a crash with
# edits on two sheets restores both.
#
#   scripts/check_xlsx.sh [BINARY]   (default target/release/bricks)
#
# In an Arch container under headless sway; what the app shows is read through the
# accessibility tree (scripts/a11y_helpers.py): the tab buttons, and the grid's
# description of the cursor cell (`A1: 4`). /tmp/f.xlsx is a copy of the LibreOffice
# fixture (crates/xlsx/tests/data/fixture.xlsx):
#   1. the tabs Data, Q1 Sales, and Summary show;
#   2. Ctrl+Page Down shows Q1 Sales: its A1 is `=Data!B2*2`;
#   5. a bar names what saving drops: cell formatting, 1 chart, comments, merged cells;
#   6. 42 typed in Q1 Sales!A1, Ctrl+S: an alert asks "Save over “f.xlsx”?"; Cancel
#      leaves the file as it was;
#   3. Ctrl+S again, Save: the saved sheet2.xml holds <v>42</v>;
#   4. an edit on two sheets, 6.5 s for the journal, kill -9, relaunch, Restore: both
#      edits are back.
# Needs docker. VERBOSE=1 prints the transcript; SHOTS=DIR saves screenshots.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(realpath "${1:-target/release/bricks}")
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p bricks)"; exit 1; }
fixture=$(realpath crates/xlsx/tests/data/fixture.xlsx)
# Data!B2, doubled, is what Q1 Sales!A1 shows.
b2=$(awk -F'\t' '/^== /{s=$0} s=="== Data" && $1=="B2"{print $3}' crates/xlsx/tests/data/fixture-libreoffice.txt)
want_a1=$(python3 -c "v=float('$b2')*2; print(int(v) if v==int(v) else v)")

shots=()
if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS"; shots=(-v "$(realpath "$SHOTS"):/shots"); fi
# Arch's sway binary carries cap_sys_nice: exec fails without it in the container's set.
out=$(docker run --rm --cap-add SYS_NICE -v "$bin:/usr/local/bin/bricks:ro" \
  -v "$fixture:/fixture.xlsx:ro" -v "$PWD/scripts/a11y_helpers.py:/a11y.py:ro" "${shots[@]}" \
  archlinux:latest bash -c '
set -uo pipefail
pacman -Syu --noconfirm --needed sway wtype grim gtk4 ttf-dejavu dbus python-gobject at-spi2-core unzip >/dev/null 2>&1
mkdir -p /shots /tmp/rt && chmod 700 /tmp/rt
export XDG_RUNTIME_DIR=/tmp/rt GDK_DEBUG=no-portals XDG_STATE_HOME=/tmp/state GDK_BACKEND=wayland
printf "output HEADLESS-1 resolution 1280x720\ndefault_border none\n" > /tmp/sway.conf
cp /fixture.xlsx /tmp/f.xlsx
session() {
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway --unsupported-gpu -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
  for i in $(seq 50); do ls /tmp/rt/wayland-[0-9] >/dev/null 2>&1 && break; sleep 0.1; done
  export WAYLAND_DISPLAY=$(basename $(ls /tmp/rt/wayland-[0-9] | head -1))
  shot() { grim /shots/$1.png 2>/dev/null || true; }
  # The headless seat has no keyboard: each wtype adds a virtual one, and focus arrives
  # with it. Keys sent in the same instant can be lost, so wait before and after.
  keys() { wtype -s 400 "$@" -s 400; }
  shown() { python3 /a11y.py text | sed "s/^/${1:-shown}: /"; }

  bricks /tmp/f.xlsx >>/tmp/app.log 2>&1 & sleep 6
  echo "== 1 tabs"
  shot 1-tabs; shown

  echo "== 2 next sheet"
  keys -M ctrl -k Page_Down -m ctrl; shot 2-next; shown

  echo "== 6 asked"
  before=$(stat -c %Y.%s /tmp/f.xlsx)
  keys 42; keys -k Return; keys -k Up; keys -M ctrl s -m ctrl; sleep 1; shot 6-asked; shown
  python3 /a11y.py press Cancel && echo "cancelled"; sleep 2
  echo "unchanged: $([ "$(stat -c %Y.%s /tmp/f.xlsx)" = "$before" ] && echo yes || echo no)"

  echo "== 3 saved"
  keys -M ctrl s -m ctrl; sleep 1; python3 /a11y.py press Save && echo "confirmed"; sleep 6
  shot 3-saved
  echo "sheet2: $(unzip -p /tmp/f.xlsx xl/worksheets/sheet2.xml | grep -o "<c r=\"A1\"[^>]*>\(<[^/][^>]*>[^<]*</[^>]*>\)*" | head -1)"
  shown after

  echo "== 4 restored"
  keys -M ctrl -k Page_Up -m ctrl; keys first; keys -k Return; keys -k Up
  keys -M ctrl -k Page_Down -m ctrl; keys second; keys -k Return; keys -k Up
  sleep 6.5
  pkill -9 -x bricks; sleep 1
  bricks /tmp/f.xlsx >>/tmp/app.log 2>&1 & sleep 6
  shot 4-asked; shown asked
  keys -k Return; sleep 2; shot 4-restored; shown restored
  keys -M ctrl -k Page_Up -m ctrl; shown other
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
check "1: the Data tab" "1 tabs" "^shown: Data$"
check "1: the Q1 Sales tab" "1 tabs" "^shown: Q1 Sales$"
check "1: the Summary tab" "1 tabs" "^shown: Summary$"
for item in "cell formatting" "1 chart" "comments" "merged cells"; do
  check "5: the bar names $item" "1 tabs" "^shown: Bricks shows this workbook.s values, formulas, and sheets\. Saving will drop: .*$item"
done
check "6: Ctrl+S over it asks first" "6 asked" "^shown: Save over “f\.xlsx”\?$"
check "6: Cancel leaves the file as it was" "6 asked" "^unchanged: yes$"
check "2: Ctrl+Page Down shows Q1 Sales!A1 (=Data!B2*2)" "2 next sheet" "^shown: A1: $want_a1$"
check "3: the save wrote 42 into Q1 Sales!A1" "3 saved" "^sheet2: .*<v>42</v>"
check "3: and shows it after reading the file back" "3 saved" "^after: A1: 42$"
check "4: a restore is offered after kill -9" "4 restored" "^asked: Restore unsaved edits\?$"
check "4: the edit on the active sheet is back" "4 restored" "^restored: A1: second$"
check "4: and the one on the other sheet" "4 restored" "^other: A1: first$"
if [ $fail = 0 ]; then echo "XLSX OK"; else
  echo "XLSX CHECK FAILED"; [ -n "${VERBOSE:-}" ] || echo "(VERBOSE=1 for the transcript)"
  exit 1
fi
