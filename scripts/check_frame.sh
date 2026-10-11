#!/usr/bin/env bash
# FRAME-4 acceptance: Frame's viewer window; FRAME-5: its slideshow.
#
#   scripts/check_frame.sh [BINARY]   (default target/release/frame)
#
# In an Arch container under headless sway (1280x720). What the app shows is read through
# the accessibility tree (scripts/a11y_helpers.py, pointed at the app "frame");
# screenshots are compared pixel by pixel (python-gobject's GdkPixbuf).
#   1. each test deck (apps/frame/crates/pptx/tests/data/*.pptx) opens from the command
#      line: the status says "Slide 1 of N", N the deck's slide count;
#   2. Page Down moves to the next slide: the fixture's status says "Slide 2 of 5" and
#      the canvas's accessible description holds slide 2's text, not slide 1's;
#   3. fills and pictures draw where expected: on slide 3 the centre of the blue
#      rectangle (EMU 360000,1440000 1800000x1080000 on a 10080720x5670720 slide, as
#      fixture-libreoffice.txt has it) is 4472C4; on slide 4 the picture (360000,1440000
#      2880000x2160000; red left half, blue right half) is red at its left quarter and
#      blue at its right quarter. The slide is found in the screenshot inside the canvas
#      (its accessible extents) as what differs from the canvas backdrop;
#   4. Ctrl+O opens the file dialog; typing a path opens that deck in its own window;
#   5. a file dropped on the start window opens (a drag source window offers it as
#      text/uri-list, as file managers do; the pointer drags it through the
#      wlr-virtual-pointer protocol, scripts/dnd_helpers.py);
#   6. files that can't be read alert with the file name and the reason: a .ppt ("Save
#      it as .pptx first") and a damaged .pptx.
# FRAME-5 acceptance: the full-screen slideshow.
#   7. on the fixture: Shift+F5 on slide 4 starts at slide 4; F5 on slide 3 starts at slide
#      1 and fills the screen (corners and top edge white, the slide's background, >= 90%
#      of the screen white); Space shows slide 2; Escape returns to the window on slide 2.
#      Shift+F5 on slide 4, then Space: slide 5 is hidden, so the show ends, back on 4.
#   8. a 4:3 deck (SampleShow.pptx) shows letterboxed in black on the 16:9 screen.
# Needs docker. VERBOSE=1 prints the transcript; SHOTS=DIR saves screenshots.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(realpath "${1:-target/release/frame}")
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p frame)"; exit 1; }
data=$(realpath apps/frame/crates/pptx/tests/data)

shots=()
if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS"; shots=(-v "$(realpath "$SHOTS"):/shots"); fi
# Arch's sway binary carries cap_sys_nice: exec fails without it in the container's set.
out=$(docker run --rm --cap-add SYS_NICE -v "$bin:/usr/local/bin/frame:ro" -v "$data:/data:ro" \
  -v "$PWD/scripts:/scripts:ro" "${shots[@]}" archlinux:latest bash -c '
set -uo pipefail
pacman -Syu --noconfirm --needed sway wtype grim gtk4 ttf-dejavu dbus python-gobject at-spi2-core \
  python-pywayland wlr-protocols wayland pkgconf >/dev/null 2>&1
mkdir -p /shots /tmp/rt /tmp/proto /tmp/f && chmod 700 /tmp/rt && touch /tmp/proto/__init__.py
python -m pywayland.scanner -i /usr/share/wayland/wayland.xml \
  /usr/share/wlr-protocols/unstable/wlr-virtual-pointer-unstable-v1.xml -o /tmp/proto/wlr_proto >/dev/null
export PYTHONPATH=/tmp/proto XDG_RUNTIME_DIR=/tmp/rt GDK_DEBUG=no-portals GDK_BACKEND=wayland
printf "output HEADLESS-1 resolution 1280x720 position 0 0\ndefault_border none\n" > /tmp/sway.conf
sed "s/^APP = .*/APP = \"frame\"/" /scripts/a11y_helpers.py > /tmp/a11y.py
cp /data/*.pptx /tmp/f/
printf "\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1not really a presentation" > /tmp/f/old.ppt
head -c 3000 /data/fixture.pptx > /tmp/f/damaged.pptx
cat > /tmp/px.py <<"PY"
import sys
import gi
gi.require_version("Atspi", "2.0")
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import Atspi, GdkPixbuf

def canvas():
    # The canvas: the image whose description is the slide text (thumbnails have none).
    desktop = Atspi.get_desktop(0)
    stack = [desktop.get_child_at_index(i) for i in range(desktop.get_child_count())]
    stack = [a for a in stack if a is not None and a.get_name() == "frame"]
    while stack:
        n = stack.pop()
        if n is None:
            continue
        if n.get_role() == Atspi.Role.IMAGE and n.get_description():
            e = n.get_component_iface().get_extents(Atspi.CoordType.WINDOW)
            return e.x, e.y, e.width, e.height
        stack.extend(n.get_child_at_index(i) for i in range(n.get_child_count()))
    sys.exit("no canvas")

def main(path, points):
    cx, cy, cw, ch = canvas()
    p = GdkPixbuf.Pixbuf.new_from_file(path)
    rs, nc, px = p.get_rowstride(), p.get_n_channels(), p.get_pixels()
    at = lambda x, y: tuple(px[y * rs + x * nc + k] for k in range(3))
    far = lambda a, b: max(abs(a[k] - b[k]) for k in range(3)) > 24
    back = at(cx + 2, cy + 2)
    my, mx = cy + ch // 2, cx + cw // 2
    row = [x for x in range(cx, cx + cw) if far(at(x, my), back)]
    col = [y for y in range(cy, cy + ch) if far(at(mx, y), back)]
    if not row or not col:
        sys.exit("no slide in the canvas")
    left, right, top, bottom = row[0], row[-1], col[0], col[-1]
    print(f"slide at {left},{top} {right - left + 1}x{bottom - top + 1}")
    for spec in points:
        name, fx, fy, want = spec.split(":")
        x = round(left + float(fx) * (right - left + 1))
        y = round(top + float(fy) * (bottom - top + 1))
        got = at(x, y)
        w = tuple(int(want[i:i + 2], 16) for i in (0, 2, 4))
        res = "no" if any(abs(got[k] - w[k]) > 12 for k in range(3)) else "yes"
        print(f"match {name}: {res} (got {bytes(got).hex().upper()} at {x},{y})")

def screen(path, points):
    # Absolute points (the slideshow fills the screen) and the share of white pixels.
    p = GdkPixbuf.Pixbuf.new_from_file(path)
    w, h, rs, nc, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
    at = lambda x, y: tuple(px[y * rs + x * nc + k] for k in range(3))
    white = sum(1 for y in range(0, h, 2) for x in range(0, w, 2) if min(at(x, y)) >= 250)
    print(f"white: {white * 400 // (w * h)}")
    for spec in points:
        name, x, y, want = spec.split(":")
        got = at(int(x), int(y))
        wc = tuple(int(want[i:i + 2], 16) for i in (0, 2, 4))
        res = "no" if any(abs(got[k] - wc[k]) > 12 for k in range(3)) else "yes"
        print(f"match {name}: {res} (got {bytes(got).hex().upper()})")

if sys.argv[1] == "screen":
    screen(sys.argv[2], sys.argv[3:])
else:
    main(sys.argv[1], sys.argv[2:])
PY
session() {
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway --unsupported-gpu -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
  for i in $(seq 50); do ls /tmp/rt/wayland-[0-9] >/dev/null 2>&1 && break; sleep 0.1; done
  export WAYLAND_DISPLAY=$(basename $(ls /tmp/rt/wayland-[0-9] | head -1))
  shot() { grim /shots/$1.png 2>/dev/null || true; }
  keys() { wtype -s 400 "$@" -s 400; }
  shown() { python3 /tmp/a11y.py text 2>&1 | sed "s/^/${1:-shown}: /"; }

  echo "== 1 decks"
  for f in /tmp/f/*.pptx; do
    [ "$f" = /tmp/f/damaged.pptx ] && continue
    name=$(basename $f)
    count=$(python3 -c "import re, sys, zipfile; print(len(re.findall(\"<p:sldId \", zipfile.ZipFile(sys.argv[1]).read(\"ppt/presentation.xml\").decode())))" $f)
    frame $f >>/tmp/app.log 2>&1 & sleep 3
    echo "want $name: Slide 1 of $count"
    shown "$name" | grep -E "Slide [0-9]+ of" || echo "$name: nothing"
    pkill -x frame; sleep 0.5
  done

  echo "== 2 page down"
  frame /tmp/f/fixture.pptx >>/tmp/app.log 2>&1 & sleep 3
  shown before
  keys -k Next; sleep 0.5
  shown after
  echo "== 3 drawing"
  keys -k Next; sleep 1; shot slide3
  python3 /tmp/px.py /shots/slide3.png fill:0.125:0.3492:4472C4
  keys -k Next; sleep 1; shot slide4
  python3 /tmp/px.py /shots/slide4.png red:0.1071:0.4444:FF0000 blue:0.25:0.4444:0000FF
  echo "== 7 slideshow"
  keys -M shift -k F5 -m shift; sleep 2; shown shiftf5
  keys -k Escape; sleep 1
  keys -k Home; sleep 0.5
  keys -k Next -k Next; sleep 0.5
  keys -k F5; sleep 2; shot show1; shown f5
  python3 /tmp/px.py screen /shots/show1.png tl:2:2:FFFFFF tr:1277:2:FFFFFF bl:2:717:FFFFFF br:1277:717:FFFFFF top:640:2:FFFFFF
  keys -k space; sleep 1; shot show2; shown space
  keys -k Escape; sleep 1; shot back; shown escape
  keys -k End; sleep 0.5
  keys -k Up; sleep 0.5
  keys -M shift -k F5 -m shift; sleep 2; shown last
  keys -k space; sleep 1; shown past
  echo "== 4 open dialog"
  keys -M ctrl o -m ctrl; sleep 2
  keys -M ctrl l -m ctrl; keys "/tmp/f/shapes.pptx"; keys -k Return; sleep 3
  shot dialog
  shown
  pkill -x frame; sleep 0.5

  echo "== 8 letterbox"
  frame /tmp/f/SampleShow.pptx >>/tmp/app.log 2>&1 & sleep 3
  keys -k F5; sleep 2; shot letterbox
  python3 /tmp/px.py screen /shots/letterbox.png left:40:360:000000 right:1240:360:000000 slide:640:40:FFFFFF
  pkill -x frame; sleep 0.5

  echo "== 5 drop"
  frame >>/tmp/app.log 2>&1 & sleep 2
  python3 /scripts/dnd_helpers.py source /tmp/f/layouts.pptx >/dev/null 2>&1 & src=$!; sleep 2
  shown start
  python3 /scripts/dnd_helpers.py drag 960 360 320 360
  sleep 3; kill $src; wait $src 2>/dev/null; sleep 1
  shot drop
  shown
  pkill -x frame; sleep 0.5

  echo "== 6 alerts"
  for f in old.ppt damaged.pptx; do
    frame /tmp/f/$f >>/tmp/app.log 2>&1 & sleep 3
    shot alert-$f
    shown $f
    pkill -x frame; sleep 0.5
  done
  swaymsg exit >/dev/null 2>&1 || true
}
dbus-run-session -- bash -c "$(declare -f session); session" 2>/tmp/dbus.log
echo "== app log"; grep "^frame:" /tmp/app.log | head -20
' 2>&1) || true
[ -n "${VERBOSE:-}" ] && sed 's/^/  /' <<<"$out"
section() { awk -v s="== $1" '/^== /{on=($0 ~ "^" s)} on' <<<"$out"; }
fail=0
check() { # WHAT SECTION PATTERN
  if section "$2" | grep -E -- "$3" >/dev/null; then echo "ok   $1"; else echo "FAIL $1 (no match for: $3)"; fail=1; fi
}
absent() { # WHAT SECTION PATTERN
  if section "$2" | grep -E -- "$3" >/dev/null; then echo "FAIL $1 (found: $3)"; fail=1; else echo "ok   $1"; fi
}
decks=$(section "1 decks" | sed -n "s/^want \([^:]*\): .*/\1/p")
[ -n "$decks" ] || { echo "FAIL 1: no decks ran"; fail=1; }
for d in $decks; do
  want=$(section "1 decks" | sed -n "s/^want $d: //p")
  check "1: $d opens from the command line ($want)" "1 decks" "^$d: $want$"
done
check "2: before Page Down, slide 1" "2 page down" "^before: Slide 1 of 5$"
check "2: slide 1's text is the canvas description" "2 page down" "^before: Frame fixture$"
check "2: Page Down shows slide 2" "2 page down" "^after: Slide 2 of 5$"
check "2: slide 2's text is the canvas description" "2 page down" "^after: Levels and alignment$"
check "2: ... all of it" "2 page down" "^after: after a line break$"
absent "2: slide 1's text is gone" "2 page down" "^after: Frame fixture$"
check "3: the blue rectangle's fill draws where it is" "3 drawing" "^match fill: yes"
check "3: the picture's red half draws where it is" "3 drawing" "^match red: yes"
check "3: the picture's blue half draws where it is" "3 drawing" "^match blue: yes"
check "4: Ctrl+O opens a deck in its own window" "4 open dialog" "^shown: shapes\.pptx$"
check "4: ... next to the first" "4 open dialog" "^shown: fixture\.pptx$"
check "4: ... and shows it" "4 open dialog" "^shown: Slide 1 of 6$"
check "5: a dropped deck opens" "5 drop" "^shown: layouts\.pptx$"
check "5: ... and shows it" "5 drop" "^shown: Slide 1 of 10$"
check "5: without a file, a start window shows" "5 drop" "^start: Open a presentation$"
absent "5: the start window closes" "5 drop" "^shown: Open a presentation$"
check "6: a .ppt alerts with its name" "6 alerts" "^old\.ppt: Cannot open “old\.ppt”$"
check "6: ... and says to save it as .pptx" "6 alerts" "^old\.ppt: .*Save it as \.pptx first\.$"
check "6: a damaged deck alerts with its name" "6 alerts" "^damaged\.pptx: Cannot open “damaged\.pptx”$"
check "6: ... and the reason" "6 alerts" "^damaged\.pptx: This isn.t a PowerPoint presentation"
# FRAME-5: the slideshow.
check "7: Shift+F5 starts the slideshow from the current slide" "7 slideshow" "^shiftf5: Slideshow slide 4$"
check "7: F5 starts it from slide 1" "7 slideshow" "^f5: Slideshow slide 1$"
check "7: ... showing slide 1" "7 slideshow" "^f5: Frame fixture$"
for c in tl tr bl br top; do check "7: slide 1 fills the screen ($c)" "7 slideshow" "^match $c: yes"; done
v=$(section "7 slideshow" | sed -n "s/^white: //p" | head -1)
if [ -n "$v" ] && [ "$v" -ge 90 ]; then echo "ok   7: the screen is the white slide ($v% white)"; else echo "FAIL 7: the screen is the white slide (${v:-nothing}% white, want >= 90)"; fail=1; fi
check "7: Space advances" "7 slideshow" "^space: Slideshow slide 2$"
check "7: ... to slide 2" "7 slideshow" "^space: Levels and alignment$"
check "7: Escape returns to the window on the slide shown" "7 slideshow" "^escape: Slide 2 of 5$"
absent "7: ... and ends the slideshow" "7 slideshow" "^escape: Slideshow slide"
check "7: Shift+F5 on slide 4" "7 slideshow" "^last: Slideshow slide 4$"
check "7: Space past it skips the hidden slide 5 and ends the show on slide 4" "7 slideshow" "^past: Slide 4 of 5$"
absent "7: ... no slideshow left" "7 slideshow" "^past: Slideshow slide"
check "8: a 4:3 deck is letterboxed in black (left)" "8 letterbox" "^match left: yes"
check "8: ... (right)" "8 letterbox" "^match right: yes"
check "8: ... around the slide" "8 letterbox" "^match slide: yes"
if [ $fail = 0 ]; then echo "FRAME OK"; else
  echo "FRAME CHECK FAILED"; [ -n "${VERBOSE:-}" ] || echo "(VERBOSE=1 for the transcript)"
  exit 1
fi
