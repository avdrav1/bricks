#!/usr/bin/env bash
# STYLE-6 acceptance: editing formatting.
#
#   scripts/check_format.sh [BINARY]   (default target/release/bricks)
#
# In an Arch container under headless sway, with real key presses (wtype); the format
# bar's Bold toggle is read through the accessibility tree (scripts/a11y_helpers.py), the
# saved workbook's XML with python.
#   1. on a copy of the fixture: Ctrl+B on Data!A2 turns the Bold toggle on, and after
#      Ctrl+S the saved file's A2 points at a cell format whose font is bold;
#   2. before saving, Ctrl+Z turns the toggle off and Ctrl+Y on again;
#   3. on a CSV, Ctrl+B then Ctrl+S opens Save As saying a CSV file can't keep
#      formatting, and the CSV stays as it was;
#   4. Ctrl+B on a CSV, wait for the journal, kill -9, relaunch, Enter on Restore: the
#      Bold toggle is on for that cell again;
#   5. (STYLE-7) the bold Data!A2 copied and pasted on D5: D5 is bold, and one Ctrl+Z
#      takes it back; text another program copied (wl-copy) pasted on a bold D6 leaves D6
#      bold.
# Needs docker. VERBOSE=1 prints the transcript; SHOTS=DIR saves screenshots.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(realpath "${1:-target/release/bricks}")
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p bricks)"; exit 1; }
fixture=$(realpath apps/bricks/crates/xlsx/tests/data/fixture.xlsx)

shots=()
if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS"; shots=(-v "$(realpath "$SHOTS"):/shots"); fi
out=$(docker run --rm --cap-add SYS_NICE -v "$bin:/usr/local/bin/bricks:ro" \
  -v "$fixture:/fixture.xlsx:ro" -v "$PWD/scripts/a11y_helpers.py:/a11y.py:ro" "${shots[@]}" \
  archlinux:latest bash -c '
set -uo pipefail
pacman -Syu --noconfirm --needed sway wtype grim gtk4 ttf-dejavu dbus python-gobject at-spi2-core wl-clipboard >/dev/null 2>&1
mkdir -p /shots /tmp/rt && chmod 700 /tmp/rt
export XDG_RUNTIME_DIR=/tmp/rt GDK_DEBUG=no-portals XDG_STATE_HOME=/tmp/state GDK_BACKEND=wayland
printf "output HEADLESS-1 resolution 1600x900\ndefault_border none\n" > /tmp/sway.conf
cp /fixture.xlsx /tmp/f.xlsx
printf "a,b\n1,2\n" > /tmp/t.csv
printf "a,b\n1,2\n" > /tmp/r.csv
# Whether cell CELL of sheet SHEET in workbook FILE has a bold font, from its XML.
cat > /tmp/cell.py <<"PY"
import sys, zipfile
import xml.etree.ElementTree as ET
M = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
ns = {"m": M}
path, sheet, cell = sys.argv[1:4]
z = zipfile.ZipFile(path)
book = ET.fromstring(z.read("xl/workbook.xml"))
rid = next(s.get(f"{{{R}}}id") for s in book.find("m:sheets", ns) if s.get("name") == sheet)
rels = ET.fromstring(z.read("xl/_rels/workbook.xml.rels"))
target = next(r.get("Target") for r in rels if r.get("Id") == rid)
part = target.lstrip("/") if target.startswith("/") else "xl/" + target
c = ET.fromstring(z.read(part)).find(f".//m:c[@r=\"{cell}\"]", ns)
s = int(c.get("s", "0")) if c is not None else 0
styles = ET.fromstring(z.read("xl/styles.xml"))
xf = list(styles.find("m:cellXfs", ns))[s]
font = list(styles.find("m:fonts", ns))[int(xf.get("fontId", "0"))]
b = font.find("m:b", ns)
bold = b is not None and b.get("val", "1") not in ("0", "false")
word = "yes" if bold else "no"
print(f"{cell} s={s} bold: {word}")
PY
session() {
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway --unsupported-gpu -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
  for i in $(seq 50); do ls /tmp/rt/wayland-[0-9] >/dev/null 2>&1 && break; sleep 0.1; done
  export WAYLAND_DISPLAY=$(basename $(ls /tmp/rt/wayland-[0-9] | head -1))
  # The headless seat has no keyboard: each wtype adds a virtual one, and focus arrives
  # with it. Keys sent in the same instant can be lost, so wait before and after.
  keys() { wtype -s 400 "$@" -s 400; }
  go() { keys -M ctrl g -m ctrl -s 300 "$1" -s 300 -k Return; sleep 0.5; }
  bold() { echo "$1: $(python3 /a11y.py state Bold 2>&1)"; }
  shot() { grim /shots/$1.png 2>/dev/null || true; }

  bricks /tmp/f.xlsx >>/tmp/app.log 2>&1 & sleep 6
  echo "== 2 undo and redo"
  go Data!A2; bold before
  keys -M ctrl b -m ctrl; sleep 1; bold after-ctrl-b; shot bold
  keys -M ctrl z -m ctrl; sleep 1; bold after-undo
  keys -M ctrl y -m ctrl; sleep 1; bold after-redo
  echo "== 1 bold and save"
  bold before-save
  keys -M ctrl s -m ctrl; sleep 6
  python3 /tmp/cell.py /tmp/f.xlsx Data A2 2>&1 | sed "s/^/saved /"

  echo "== 5 paste"
  go Data!A2; keys -M ctrl c -m ctrl; sleep 1
  go Data!D5; bold d5-before
  keys -M ctrl v -m ctrl; sleep 1; bold d5-pasted
  keys -M ctrl z -m ctrl; sleep 1; bold d5-undone
  go Data!D6; keys -M ctrl b -m ctrl; sleep 1
  printf "elsewhere" | wl-copy; sleep 1
  keys -M ctrl v -m ctrl; sleep 1; bold d6-pasted
  echo "d6: $(python3 /a11y.py text | grep "^D6: ")"
  pkill -x bricks; sleep 1

  echo "== 3 csv"
  bricks /tmp/t.csv >>/tmp/app.log 2>&1 & sleep 3
  keys -M ctrl b -m ctrl; sleep 1; bold csv-bold
  keys -M ctrl s -m ctrl; sleep 2; shot save-as
  python3 /a11y.py text | grep -i "csv file" | sed "s/^/note: /"
  echo "csv unchanged: $(tr "\n" " " < /tmp/t.csv)"
  pkill -x bricks; sleep 1

  echo "== 4 recovery"
  bricks /tmp/r.csv >>/tmp/app.log 2>&1 & sleep 3
  keys -M ctrl b -m ctrl; sleep 1; bold before-crash
  sleep 6.5; pkill -9 -x bricks; sleep 1
  bricks /tmp/r.csv >>/tmp/app.log 2>&1 & sleep 4
  keys -k Return; sleep 3
  bold restored
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
check "1: A2 isn't bold to begin with" "2 undo" "^before: off$"
check "1: Ctrl+B on Data!A2 turns Bold on" "2 undo" "^after-ctrl-b: on$"
check "1: the saved A2 has a bold font" "1 bold" "^saved A2 s=[0-9]+ bold: yes$"
check "2: Ctrl+Z turns it off" "2 undo" "^after-undo: off$"
check "2: Ctrl+Y turns it on again" "2 undo" "^after-redo: on$"
check "3: Ctrl+B on a CSV" "3 csv" "^csv-bold: on$"
check "3: a CSV with formatting saves only as .xlsx" "3 csv" "^note: A CSV file can.t keep formatting"
check "3: the CSV file is untouched" "3 csv" "^csv unchanged: a,b 1,2 $"
check "4: crash recovery restores the bold" "4 recovery" "^restored: on$"
check "5: D5 isn't bold to begin with" "5 paste" "^d5-before: off$"
check "5: pasting the bold A2 makes D5 bold" "5 paste" "^d5-pasted: on$"
check "5: one Ctrl+Z takes text and style back" "5 paste" "^d5-undone: off$"
check "5: another program's text keeps D6 bold" "5 paste" "^d6-pasted: on$"
check "5: and it pasted" "5 paste" "^d6: D6: elsewhere$"
if [ $fail = 0 ]; then echo "FORMAT OK"; else
  [ -n "${VERBOSE:-}" ] || sed 's/^/  /' <<<"$out" | tail -40
  echo "FORMAT CHECK FAILED"; exit 1
fi
