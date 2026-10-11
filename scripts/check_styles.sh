#!/usr/bin/env bash
# STYLE-3 acceptance: the grid draws cells in their styles.
#
#   scripts/check_styles.sh [BINARY]   (default target/release/bricks)
#
# In an Arch container under headless sway. Screenshots are compared pixel by pixel
# (python-gobject's GdkPixbuf); the cursor cell's text is read through the accessibility
# tree (scripts/a11y_helpers.py).
#   1. a number right-aligns: in `a,1` the 1 sits more than 60 px right of where `b`
#      sits in `a,b` (each found as what differs from `a,`);
#   2. text longer than its column overflows into the empty one beside it: it spans more
#      than 140 px (a column is 120);
#   3. the fixture's Styles sheet draws its fills (yellow, three theme tints, dark blue),
#      red and blue text, and its dark-blue cell's white text;
#   4. number formats show: B8 `1,234.50`, B9 `12.50%`, B10 `(1,234)` (accounting), B13
#      `Dec 9, 2025`.
# Needs docker. VERBOSE=1 prints the transcript; SHOTS=DIR saves screenshots.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(realpath "${1:-target/release/bricks}")
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p bricks)"; exit 1; }
fixture=$(realpath crates/xlsx/tests/data/fixture.xlsx)

shots=()
if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS"; shots=(-v "$(realpath "$SHOTS"):/shots"); fi
out=$(docker run --rm --cap-add SYS_NICE -v "$bin:/usr/local/bin/bricks:ro" \
  -v "$fixture:/fixture.xlsx:ro" -v "$PWD/scripts/a11y_helpers.py:/a11y.py:ro" "${shots[@]}" \
  archlinux:latest bash -c '
set -uo pipefail
pacman -Syu --noconfirm --needed sway wtype grim gtk4 ttf-dejavu dbus python-gobject at-spi2-core >/dev/null 2>&1
mkdir -p /shots /tmp/rt && chmod 700 /tmp/rt
export XDG_RUNTIME_DIR=/tmp/rt GDK_DEBUG=no-portals XDG_STATE_HOME=/tmp/state GDK_BACKEND=wayland
printf "output HEADLESS-1 resolution 1280x720\ndefault_border none\n" > /tmp/sway.conf
cp /fixture.xlsx /tmp/f.xlsx
printf "a,\n" > /tmp/n0.csv; printf "a,1\n" > /tmp/n1.csv; printf "a,b\n" > /tmp/n2.csv
printf " \n" > /tmp/o0.csv; printf "This text is much longer than its column\n" > /tmp/o1.csv
cat > /tmp/px.py <<"PY"
import sys
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf

def load(path):
    p = GdkPixbuf.Pixbuf.new_from_file(path)
    return p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()

def diff_xs(a, b):
    # x of the pixels that differ in the lowest band that differs: the cell (the type
    # tag in the column header above it differs too).
    w, h, rs, n, pa = load(a)
    _, _, rsb, nb, pb = load(b)
    pts = []
    for y in range(h):
        for x in range(w):
            i, j = y * rs + x * n, y * rsb + x * nb
            if any(abs(pa[i + k] - pb[j + k]) > 40 for k in range(3)):
                pts.append((x, y))
    if not pts:
        return []
    bottom = max(y for _, y in pts)
    return [x for x, y in pts if y > bottom - 16]

def count(path, rgb, tol):
    w, h, rs, n, p = load(path)
    r0, g0, b0 = (rgb >> 16) & 255, (rgb >> 8) & 255, rgb & 255
    c = 0
    for y in range(h):
        for x in range(w):
            i = y * rs + x * n
            if abs(p[i] - r0) <= tol and abs(p[i + 1] - g0) <= tol and abs(p[i + 2] - b0) <= tol:
                c += 1
    return c

cmd = sys.argv[1]
if cmd == "align":
    one, b = diff_xs("/shots/n1.png", "/shots/n0.png"), diff_xs("/shots/n2.png", "/shots/n0.png")
    if one and b:
        print(f"align: {sum(one) / len(one) - sum(b) / len(b):.0f}")
elif cmd == "overflow":
    xs = diff_xs("/shots/o1.png", "/shots/o0.png")
    if xs:
        print(f"overflow: {max(xs) - min(xs)}")
else:
    for name in sys.argv[2:]:
        rgb, tol = int(name, 16), 0 if name in ("FFFF00", "C4FEBB", "127A02", "E9706D", "305496") else 30
        print(f"color {name}: {count(cmd, rgb, tol)}")
PY
session() {
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway --unsupported-gpu -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
  for i in $(seq 50); do ls /tmp/rt/wayland-[0-9] >/dev/null 2>&1 && break; sleep 0.1; done
  export WAYLAND_DISPLAY=$(basename $(ls /tmp/rt/wayland-[0-9] | head -1))
  shot() { grim /shots/$1.png 2>/dev/null || true; }
  keys() { wtype -s 400 "$@" -s 400; }
  shown() { python3 /a11y.py text | sed "s/^/${1:-shown}: /"; }
  # Every variant opens as t.csv, so the title and tab stay the same.
  open_shot() {
    cp /tmp/$1.csv /tmp/t.csv; bricks /tmp/t.csv >>/tmp/app.log 2>&1 & sleep 4
    shot $1; pkill -x bricks; sleep 1
  }

  echo "== 1 align"
  for f in n0 n1 n2; do open_shot $f; done
  python3 /tmp/px.py align
  echo "== 2 overflow"
  for f in o0 o1; do open_shot $f; done
  python3 /tmp/px.py overflow

  bricks /tmp/f.xlsx >>/tmp/app.log 2>&1 & sleep 6
  echo "== 4 formats"
  for cell in B8 B9 B10 B13; do
    keys -M ctrl g -m ctrl -s 300 "Styles!$cell" -s 300 -k Return; sleep 0.5; shown
  done
  echo "== 3 colours"
  keys -M ctrl g -m ctrl -s 300 "Styles!J30" -s 300 -k Return; sleep 1; shot styles
  python3 /tmp/px.py /shots/styles.png FFFF00 C4FEBB 127A02 E9706D 305496 C00000 0070C0 FFFFFF
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
at_least() { # WHAT SECTION KEY MIN
  local v
  v=$(section "$2" | sed -n "s/^$3: //p" | head -1)
  if [ -n "$v" ] && [ "$v" -ge "$4" ]; then echo "ok   $1 ($v)"; else echo "FAIL $1 (${v:-nothing}, want >= $4)"; fail=1; fi
}
at_least "1: a number right-aligns" "1 align" "align" 60
at_least "2: long text overflows into the empty column" "2 overflow" "overflow" 140
for c in FFFF00 C4FEBB 127A02 E9706D 305496; do
  at_least "3: the fill $c" "3 colours" "color $c" 300
done
at_least "3: red text" "3 colours" "color C00000" 8
at_least "3: blue text" "3 colours" "color 0070C0" 8
check "4: #,##0.00" "4 formats" "^shown: B8: 1,234\.50$"
check "4: 0.00%" "4 formats" "^shown: B9: 12\.50%$"
check "4: accounting" "4 formats" "^shown: B10: \(1,234\)$"
check "4: a date format" "4 formats" "^shown: B13: Dec 9, 2025$"
if [ $fail = 0 ]; then echo "STYLES OK"; else
  echo "STYLES CHECK FAILED"; [ -n "${VERBOSE:-}" ] || echo "(VERBOSE=1 for the transcript)"
  exit 1
fi
