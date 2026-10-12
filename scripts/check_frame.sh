#!/usr/bin/env bash
# FRAME-4 acceptance: Frame's viewer window; FRAME-5: its slideshow; FRAME-6: text editing;
# FRAME-7: adding, duplicating, deleting, and moving slides; FRAME-8: saving; FRAME-12:
# inserting; FRAME-13: the outline; FRAME-14: new decks; FRAME-15: the reference task;
# FRAME-9: installing.
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
#   5. without a file, the new deck window shows (Untitled, Slide 1 of 1); a file dropped
#      on it opens in its own window (a drag source window offers it as text/uri-list,
#      as file managers do; the pointer drags it through the wlr-virtual-pointer
#      protocol, scripts/dnd_helpers.py);
#   6. files that can't be read alert with the file name and the reason: a .ppt ("Save
#      it as .pptx first") and a damaged .pptx.
# FRAME-5 acceptance: the full-screen slideshow.
#   7. on the fixture: Shift+F5 on slide 4 starts at slide 4; F5 on slide 3 starts at slide
#      1 and fills the screen (corners and top edge white, the slide's background, >= 90%
#      of the screen white); Space shows slide 2; Escape returns to the window on slide 2.
#      Shift+F5 on slide 4, then Space: slide 5 is hidden, so the show ends, back on 4.
#   8. a 4:3 deck (SampleShow.pptx) shows letterboxed in black on the 16:9 screen.
# FRAME-6 acceptance: editing slide text, with undo.
#   9. in layouts.pptx (PowerPoint's), a click at the end of slide 1's subtitle (a
#      placeholder whose look all comes from its layout and master) puts the caret there,
#      and Left moves it, not the slide; typing " here" changes the canvas description to
#      "subtitle here" and draws more dark pixels where the subtitle is; Ctrl+Z undoes
#      "here", then " "; Ctrl+Shift+Z and Ctrl+Y redo them; Enter and typing start a
#      paragraph; Ctrl+A and Delete empty the placeholder, and typing fills it; Escape
#      stops editing, so Page Down shows slide 2. On the fixture, a click at the end of
#      slide 3's text box and "!" change it. Clicks go through the virtual pointer.
# FRAME-7 acceptance: slides, with undo.
#  10. in layouts.pptx, Ctrl+M on the title slide adds slide 2 on Title and Content (the
#      canvas's accessible name says the layout) and the list grows to 11; the New Slide
#      button adds one on the current layout; the menu beside it lists the layouts, and
#      picking Two Content adds a slide on it; each Ctrl+Z takes one out again, Ctrl+Y puts
#      one back; Ctrl+D duplicates slide 3 and Ctrl+Up twice moves the copy to 2; a click
#      on thumbnail 1 and Delete delete it, Ctrl+Z restores it; dragging thumbnail 1 to
#      the lower half of thumbnail 3 moves the slide to 3, and Ctrl+Z moves it back.
# FRAME-8 acceptance, the window's half (LibreOffice's reading of saved decks is
# `cargo test -p pptx --test save -- --ignored`):
#  11. on a copy of layouts.pptx, typing into slide 1's subtitle, Ctrl+M and typing the
#      new slide's title, then Ctrl+S: the status says it saved; the file's slide 1 part
#      has the typed text, its slide list 11 slides, and the new slide a part of its own;
#      reopened, it shows them. Ctrl+Shift+S and a typed path save it as another file,
#      the window takes that file's name, and the file reopens.
# FRAME-10 acceptance, the window's half (snapping itself: `cargo test -p deck-edit --test
# frames`):
#  12. on the fixture's slide 3, dragging the blue rectangle so its centre is 3 px right of
#      the slide's: mid-drag a guide runs down the slide's middle, and it lands centred;
#      Ctrl+Z puts it back. With Alt held it lands where dropped. Shift+Right and Right
#      nudge it 11 px; Ctrl+Z undoes the last nudge.
# FRAME-11 acceptance, the window's half (the edits: `cargo test -p deck-edit --test style`):
#  13. on a copy of layouts.pptx, the inspector shows slide 1's layout; a click in the
#      subtitle and Ctrl+A show its text rows, the inherited ones captioned; Bold, Size 40,
#      colour Red C00000, and Align right apply at once (more ink, red pixels, ink in the
#      right half); Ctrl+Z undoes the alignment, Ctrl+Y redoes it. Slide 2 moves to Title
#      Only through the Layout list and takes a light blue background. Ctrl+S writes them
#      into the slide parts. On the fixture, slide 3's blue rectangle takes a red fill, and
#      Ctrl+Z gives back the blue.
# FRAME-12 acceptance, the window's half (the edits: `cargo test -p deck-edit --test insert`):
#  14. on a copy of layouts.pptx, a long word typed onto slide 1's subtitle shrinks it into
#      its box (more ink inside, none below it), and one Ctrl+Z undoes the word and the
#      shrink (the ink as before). Ctrl+M adds a Title and Content slide; Insert, Picture…
#      and a typed path to a green PNG fill its body placeholder; Ctrl+Z empties it. On
#      a new Title Only slide, the PNG dropped on the canvas lands centred at its size.
#      Insert, Text and typing make a text box; Insert, Shape, Oval adds an accent oval;
#      Insert, Table a table with an accent header and banded rows; Delete deletes the
#      selected table and Ctrl+Z restores it. Ctrl+S writes normAutofit, the pictures,
#      the oval, the table, and the text box into the slide parts, the pictures into the
#      package.
# FRAME-13 acceptance, the window's half (the edits: `cargo test -p deck-edit --test
# outline`):
#  15. on a copy of layouts.pptx, the Outline button shows the outline; at the end of
#      slide 1's title, Enter, Highlights, Enter, Growth, Tab, Enter, Margins, Enter, Tab,
#      and Up 3% make slide 2 on Title and Content with those bullets, Up 3% a level down,
#      11 slides; Ctrl+Shift+O shows the canvas on slide 2 with that text. Shift+Tab twice
#      on Up 3% makes it slide 3's title; each Ctrl+Z steps back one. Text typed on the
#      canvas shows in the outline; Home there goes to the title text, past the slide
#      number; Ctrl+Z in the outline undoes the canvas typing too (one history).
#      Ctrl+S writes the new slide into a part of its own, Up 3% at lvl="1", slide 1 as
#      it was. On the fixture, a slide with shapes shows how many other items it holds.
# FRAME-14 acceptance, the window's half (LibreOffice's reading of the saved blank deck is
# `cargo test -p pptx --test blank -- --ignored`):
#  16. without a file, the window is Untitled on slide 1 of 1, a Title Slide, the slide
#      16:9 on the canvas; the New Slide menu lists exactly the five layouts. A click on
#      the title and typing "Quarterly review", then Ctrl+S: Save As asks, and a typed
#      path saves it; the window takes the file name, the slide part holds the title and
#      the package five layouts. Ctrl+N opens another Untitled window, and Ctrl+N again
#      an Untitled 2.
# FRAME-15 acceptance: the spec's reference task (a 4-slide deck: "Quarterly review" with
# a subtitle; "Highlights" with three bullets, one at the second level; "Revenue" with a
# picture from a file; "Next steps" with two bullets; saved as .pptx) from `frame` without
# a file, by keyboard and pointer, its input actions counted (clicks, and key presses or
# chords other than the content's own characters):
#  17. on the canvas: Ctrl+Enter from placeholder to placeholder and on to new slides,
#      Tab for the second level, Insert, Picture…, Ctrl+M, Ctrl+S;
#  18. in the outline: Enter, Tab, Shift+Tab, Ctrl+Enter for a new slide, Insert,
#      Picture… from the outline, Ctrl+S.
#      Each count is below PowerPoint's (the BACKLOG.md note on FRAME-15), and LibreOffice
#      on this machine (scripts/lo_pptx.py dump) reads each saved deck as the task.
# FRAME-9 acceptance:
#  19. the installer (sites/frame/install.sh) takes this binary, packaged with
#      packaging/dev.cronkite.Frame.{desktop,svg} as a release served on localhost, and
#      makes Frame the .pptx default with a menu entry running ~/.local/bin/frame;
#      `gio open` on a .pptx (a file manager's double-click) shows it in that Frame;
#      --uninstall removes the binary, entry, and icon, and keeps other apps' defaults.
# Needs docker, and LibreOffice for 17 and 18. VERBOSE=1 prints the transcript; SHOTS=DIR
# saves screenshots.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(realpath "${1:-target/release/frame}")
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p frame)"; exit 1; }
data=$(realpath apps/frame/crates/pptx/tests/data)
# PowerPoint's input actions for the reference task: Microsoft 365's shortest documented
# path, step by step with sources in the BACKLOG.md note on FRAME-15.
powerpoint=21

shots=()
if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS"; shots=(-v "$(realpath "$SHOTS"):/shots"); fi
# The task decks (17, 18) come out here, for LibreOffice on this machine to read.
ref=$(mktemp -d); trap 'rm -rf "$ref"' EXIT
# Arch's sway binary carries cap_sys_nice: exec fails without it in the container's set.
out=$(docker run --rm --cap-add SYS_NICE -v "$bin:/usr/local/bin/frame:ro" -v "$data:/data:ro" \
  -v "$PWD/scripts:/scripts:ro" -v "$PWD/packaging:/packaging:ro" -v "$PWD/sites:/sites:ro" \
  -v "$ref:/out" "${shots[@]}" archlinux:latest bash -c '
set -uo pipefail
pacman -Syu --noconfirm --needed sway wtype grim gtk4 ttf-dejavu dbus python-gobject at-spi2-core \
  python-pywayland wlr-protocols wayland pkgconf curl shared-mime-info >/dev/null 2>&1
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
    # The canvas: the largest image (a new slide has no text, so no description).
    desktop = Atspi.get_desktop(0)
    stack = [desktop.get_child_at_index(i) for i in range(desktop.get_child_count())]
    stack = [a for a in stack if a is not None and a.get_name() == "frame"]
    best = None
    while stack:
        n = stack.pop()
        if n is None:
            continue
        if n.get_role() == Atspi.Role.IMAGE:
            e = n.get_component_iface().get_extents(Atspi.CoordType.WINDOW)
            if best is None or e.width * e.height > best[2] * best[3]:
                best = (e.x, e.y, e.width, e.height)
        stack.extend(n.get_child_at_index(i) for i in range(n.get_child_count()))
    if best is None:
        sys.exit("no canvas")
    return best

def slide(path):
    # The slide in the screenshot: what differs from the canvas backdrop in its middle row
    # and column. Returns the pixel reader and the left, top, width, and height of the slide.
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
    return at, row[0], col[0], row[-1] - row[0] + 1, col[-1] - col[0] + 1

def main(path, points):
    at, left, top, w, h = slide(path)
    print(f"slide at {left},{top} {w}x{h}")
    for spec in points:
        name, fx, fy, want = spec.split(":")
        x = round(left + float(fx) * w)
        y = round(top + float(fy) * h)
        got = at(x, y)
        wc = tuple(int(want[i:i + 2], 16) for i in (0, 2, 4))
        res = "no" if any(abs(got[k] - wc[k]) > 12 for k in range(3)) else "yes"
        print(f"match {name}: {res} (got {bytes(got).hex().upper()} at {x},{y})")

def point(path, fx, fy):
    # The screen point at fractions of the slide, to click.
    _, left, top, w, h = slide(path)
    print(round(left + float(fx) * w), round(top + float(fy) * h))

def ink(path, name, fx0, fy0, fx1, fy1):
    # Pixels drawn darker than light grey in a box given in fractions of the slide.
    at, left, top, w, h = slide(path)
    xs = range(round(left + float(fx0) * w), round(left + float(fx1) * w))
    ys = range(round(top + float(fy0) * h), round(top + float(fy1) * h))
    print(f"ink {name}: {sum(1 for y in ys for x in xs if max(at(x, y)) < 200)}")

def part(path, name, needle):
    # How often NEEDLE occurs in part NAME of the package at PATH.
    import zipfile
    text = zipfile.ZipFile(path).read(name).decode()
    print(f"part {name} {needle}: {text.count(needle)}")

def parts(path, pattern):
    # How many part names of the package at PATH match the regular expression PATTERN.
    import re, zipfile
    n = sum(1 for p in zipfile.ZipFile(path).namelist() if re.search(pattern, p))
    print(f"parts {pattern}: {n}")

def aspect(path):
    # The slide in the screenshot and whether its sides are 16:9, within two pixels.
    _, _, _, w, h = slide(path)
    print(f"aspect {w}x{h}: " + ("yes" if abs(w * 9 / 16 - h) <= 2 else "no"))

def blue(path, name, fy="0.30"):
    # Where the fixture slide 3 blue rectangle is: its centre, in screen pixels from the
    # slide centre, along a row through it at FY of the slide.
    at, left, top, w, h = slide(path)
    y = round(top + float(fy) * h)
    want = (0x44, 0x72, 0xC4)
    xs = [x for x in range(left, left + w) if all(abs(a - b) <= 12 for a, b in zip(at(x, y), want))]
    print(f"blue {name}: " + (str(round((xs[0] + xs[-1]) / 2 - (left + w / 2))) if xs else "none"))

def count(path, name, want, fx0, fy0, fx1, fy1):
    # Pixels of colour WANT in a box given in fractions of the slide.
    at, left, top, w, h = slide(path)
    wc = tuple(int(want[i:i + 2], 16) for i in (0, 2, 4))
    xs = range(round(left + float(fx0) * w), round(left + float(fx1) * w))
    ys = range(round(top + float(fy0) * h), round(top + float(fy1) * h))
    n = sum(1 for y in ys for x in xs if all(abs(a - b) <= 40 for a, b in zip(at(x, y), wc)))
    print(f"count {name}: {n}")

def where(name):
    # The centre of the first widget named NAME, else of the first described as NAME (a
    # drop-down is named by the item it shows), to click or drag it.
    desktop = Atspi.get_desktop(0)
    apps = [desktop.get_child_at_index(i) for i in range(desktop.get_child_count())]
    apps = [a for a in apps if a is not None and a.get_name() == "frame"]
    for field in (lambda n: n.get_name(), lambda n: n.get_description()):
        stack = list(apps)
        while stack:
            n = stack.pop(0)
            if n is None:
                continue
            if field(n) == name and n.get_component_iface() is not None:
                e = n.get_component_iface().get_extents(Atspi.CoordType.WINDOW)
                if e.width > 0:
                    print(e.x + e.width // 2, e.y + e.height // 2)
                    return
            stack.extend(n.get_child_at_index(i) for i in range(n.get_child_count()))
    sys.exit(f"nothing named {name}")

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

def png(path):
    # A 400x200 PNG all 00FF00, to insert.
    import struct, zlib
    w, h = 400, 200
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    rows = b"".join(b"\x00" + bytes([0, 255, 0]) * w for _ in range(h))
    head = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", head) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))

def pictures(path, slide):
    # How many of the pictures slide part SLIDE links to are in the package at PATH.
    import posixpath, zipfile
    import xml.etree.ElementTree as ET
    z = zipfile.ZipFile(path)
    rels = ET.fromstring(z.read(f"ppt/slides/_rels/{slide}.rels"))
    links = [r.get("Target") for r in rels if r.get("Type", "").endswith("/image")]
    there = [t for t in links if posixpath.normpath(posixpath.join("ppt/slides", t)) in z.namelist()]
    print(f"pictures {slide}: {len(there)} of {len(links)}")

if sys.argv[1] == "screen":
    screen(sys.argv[2], sys.argv[3:])
elif sys.argv[1] == "point":
    point(*sys.argv[2:5])
elif sys.argv[1] == "ink":
    ink(*sys.argv[2:8])
elif sys.argv[1] == "where":
    where(sys.argv[2])
elif sys.argv[1] == "part":
    part(*sys.argv[2:5])
elif sys.argv[1] == "parts":
    parts(*sys.argv[2:4])
elif sys.argv[1] == "aspect":
    aspect(sys.argv[2])
elif sys.argv[1] == "blue":
    blue(*sys.argv[2:5])
elif sys.argv[1] == "count":
    count(*sys.argv[2:9])
elif sys.argv[1] == "png":
    png(sys.argv[2])
elif sys.argv[1] == "pictures":
    pictures(*sys.argv[2:4])
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

  echo "== 9 editing"
  frame /tmp/f/layouts.pptx >>/tmp/app.log 2>&1 & sleep 3
  shot edit0
  python3 /tmp/px.py ink /shots/edit0.png before 0.15 0.57 0.85 0.65
  read x y < <(python3 /tmp/px.py point /shots/edit0.png 0.828 0.6095)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 0.5
  keys -k Left; sleep 0.3; shown left
  keys -k End " here"; sleep 0.5; shot edit1; shown typed
  python3 /tmp/px.py ink /shots/edit1.png after 0.15 0.57 0.85 0.65
  keys -M ctrl z -m ctrl; sleep 0.5; shown undo1
  keys -M ctrl z -m ctrl; sleep 0.5; shown undo2
  keys -M ctrl -M shift z -m shift -m ctrl; sleep 0.5; shown redo1
  keys -M ctrl y -m ctrl; sleep 0.5; shown redo2
  keys -k Return "second"; sleep 0.5; shown enter
  keys -M ctrl a -m ctrl -k Delete; sleep 0.5; shot edit-empty; shown cleared
  keys "Typed"; sleep 0.5; shown retyped
  keys -k Escape -k Next; sleep 0.5; shown escape
  pkill -x frame; sleep 0.5
  frame /tmp/f/fixture.pptx >>/tmp/app.log 2>&1 & sleep 3
  keys -k Next -k Next; sleep 0.5; shot textbox0
  read x y < <(python3 /tmp/px.py point /shots/textbox0.png 0.730 0.878)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 0.5
  keys "!"; sleep 0.5; shot textbox1; shown textbox
  pkill -x frame; sleep 0.5

  echo "== 10 slides"
  frame /tmp/f/layouts.pptx >>/tmp/app.log 2>&1 & sleep 3
  keys -M ctrl m -m ctrl; sleep 0.5; shown ctrlm
  python3 /tmp/a11y.py press "New Slide"; sleep 0.5; shown button
  read x y < <(python3 /tmp/px.py where Layouts)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 1; shot layouts; shown menu
  python3 /tmp/a11y.py press "Two Content"; sleep 1; shown picked
  keys -M ctrl z -m ctrl; sleep 0.5; shown undo1
  keys -M ctrl z -m ctrl -M ctrl z -m ctrl; sleep 0.5; shown undo3
  keys -M ctrl y -m ctrl; sleep 0.5; shown redo1
  keys -M ctrl z -m ctrl; sleep 0.5
  keys -k Next -k Next -M ctrl d -m ctrl; sleep 0.5; shown dup
  keys -M ctrl -k Up -k Up -m ctrl; sleep 0.5; shown up
  read x y < <(python3 /tmp/px.py where "Slide 1")
  python3 /scripts/dnd_helpers.py click $x $y; sleep 0.5
  keys -k Delete; sleep 0.5; shown deleted
  keys -M ctrl z -m ctrl; sleep 0.5; shown undelete
  read x1 y1 < <(python3 /tmp/px.py where "Slide 1")
  read x3 y3 < <(python3 /tmp/px.py where "Slide 3")
  python3 /scripts/dnd_helpers.py drag $x1 $y1 $x3 $((y3 + 30)); sleep 1; shot dragged; shown dragged
  keys -M ctrl z -m ctrl; sleep 0.5; shown undrag
  pkill -x frame; sleep 0.5

  echo "== 11 save"
  cp /tmp/f/layouts.pptx /tmp/f/save.pptx
  frame /tmp/f/save.pptx >>/tmp/app.log 2>&1 & sleep 3
  shot save0
  read x y < <(python3 /tmp/px.py point /shots/save0.png 0.828 0.6095)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 0.5
  keys -k End " saved" -k Escape -M ctrl m -m ctrl; sleep 1; shot save1
  read x y < <(python3 /tmp/px.py point /shots/save1.png 0.5 0.1234)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 0.5
  keys "Agenda" -k Escape -M ctrl s -m ctrl; sleep 3; shown saved
  python3 /tmp/px.py part /tmp/f/save.pptx ppt/slides/slide1.xml "subtitle saved"
  python3 /tmp/px.py part /tmp/f/save.pptx ppt/presentation.xml "<p:sldId "
  python3 /tmp/px.py part /tmp/f/save.pptx ppt/slides/slide11.xml ">Agenda<"
  pkill -x frame; sleep 0.5
  frame /tmp/f/save.pptx >>/tmp/app.log 2>&1 & sleep 3
  shown reopened
  keys -k Next; sleep 0.5; shown second
  keys -M ctrl -M shift s -m shift -m ctrl; sleep 2
  keys -M ctrl a -m ctrl "/tmp/f/saved-as.pptx" -k Return; sleep 3; shot saveas; shown saveas
  pkill -x frame; sleep 0.5
  frame /tmp/f/saved-as.pptx >>/tmp/app.log 2>&1 & sleep 3
  shown reopened2
  pkill -x frame; sleep 0.5

  echo "== 12 arrange"
  frame /tmp/f/fixture.pptx >>/tmp/app.log 2>&1 & sleep 3
  keys -k Next -k Next; sleep 1; shot arrange0
  b0=$(python3 /tmp/px.py blue /shots/arrange0.png before | cut -d" " -f3); echo "blue before: $b0"
  python3 /tmp/px.py count /shots/arrange0.png noguide E8308C 0.49 0 0.51 1
  # Down into the free space under the shapes (row 0.93 crosses only it), its centre 3 px
  # right of the slide centre.
  read x y < <(python3 /tmp/px.py point /shots/arrange0.png 0.125 0.30)
  read x2 y2 < <(python3 /tmp/px.py point /shots/arrange0.png 0.503 0.81)
  python3 /scripts/dnd_helpers.py drag $x $y $x2 $y2 3 & d=$!; sleep 2.2; shot arrange1; wait $d; sleep 0.5
  shot arrange2
  python3 /tmp/px.py count /shots/arrange1.png guide E8308C 0.49 0 0.51 1
  python3 /tmp/px.py blue /shots/arrange2.png centred 0.93
  keys -M ctrl z -m ctrl; sleep 0.5; shot arrange3
  b=$(python3 /tmp/px.py blue /shots/arrange3.png undone | cut -d" " -f3)
  echo "undone: $([ "$b" = "$b0" ] && echo yes || echo no) ($b)"
  wtype -M alt -s 5000 -m alt & k=$!; sleep 0.3
  python3 /scripts/dnd_helpers.py drag $x $y $x2 $y2; wait $k; sleep 0.5; shot arrange4
  python3 /tmp/px.py blue /shots/arrange4.png free 0.93
  keys -M ctrl z -m ctrl; sleep 0.5
  keys -M shift -k Right -m shift -k Right; sleep 0.5; shot arrange5
  b=$(python3 /tmp/px.py blue /shots/arrange5.png nudged | cut -d" " -f3); echo "nudged: $((b - b0))"
  keys -M ctrl z -m ctrl; sleep 0.5; shot arrange6
  b=$(python3 /tmp/px.py blue /shots/arrange6.png unnudged | cut -d" " -f3); echo "unnudged: $((b - b0))"
  pkill -x frame; sleep 0.5

  echo "== 13 inspector"
  cp /tmp/f/layouts.pptx /tmp/f/look.pptx
  frame /tmp/f/look.pptx >>/tmp/app.log 2>&1 & sleep 3
  shot look0; shown slide
  read x y < <(python3 /tmp/px.py point /shots/look0.png 0.828 0.6095)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 0.5
  keys -M ctrl a -m ctrl; sleep 0.5; shot look1; shown text
  python3 /tmp/px.py ink /shots/look1.png before 0.05 0.53 0.95 0.85
  python3 /tmp/a11y.py press Bold; sleep 0.5
  echo "bold: $(python3 /tmp/a11y.py state Bold)"
  read x y < <(python3 /tmp/px.py where "Font size")
  python3 /scripts/dnd_helpers.py click $x $y; sleep 0.3
  keys -M ctrl a -m ctrl "40" -k Return; sleep 0.5
  read x y < <(python3 /tmp/px.py where "Text colour")
  python3 /scripts/dnd_helpers.py click $x $y; sleep 0.5
  python3 /tmp/a11y.py press "Red C00000"; sleep 0.5
  python3 /tmp/a11y.py press "Align right"; sleep 0.5; shot look2; shown styled
  python3 /tmp/px.py ink /shots/look2.png after 0.05 0.53 0.95 0.85
  python3 /tmp/px.py ink /shots/look2.png right-left 0.05 0.53 0.5 0.85
  python3 /tmp/px.py ink /shots/look2.png right-right 0.5 0.53 0.95 0.85
  keys -M ctrl z -m ctrl; sleep 0.5; shot look3
  echo "right after undo: $(python3 /tmp/a11y.py state "Align right")"
  python3 /tmp/px.py ink /shots/look3.png undone-left 0.05 0.53 0.5 0.85
  python3 /tmp/px.py ink /shots/look3.png undone-right 0.5 0.53 0.95 0.85
  keys -M ctrl y -m ctrl; sleep 0.5
  echo "right after redo: $(python3 /tmp/a11y.py state "Align right")"
  # Stopped editing, the text shows without the selection over it.
  keys -k Escape; sleep 0.5; shot look-red
  python3 /tmp/px.py count /shots/look-red.png red C00000 0.05 0.53 0.95 0.85
  read x y < <(python3 /tmp/px.py where "Slide 2")
  python3 /scripts/dnd_helpers.py click $x $y; sleep 0.5; shown slide2
  read x y < <(python3 /tmp/px.py where "Slide layout")
  python3 /scripts/dnd_helpers.py click $x $y; sleep 1; shot look-layouts
  read x y < <(python3 /tmp/px.py where "Title Only")
  python3 /scripts/dnd_helpers.py click $x $y; sleep 1; shown relayout
  read x y < <(python3 /tmp/px.py where "Background colour")
  python3 /scripts/dnd_helpers.py click $x $y; sleep 0.5
  python3 /tmp/a11y.py press "Light blue 00B0F0"; sleep 0.5; shot look4
  python3 /tmp/px.py /shots/look4.png background:0.03:0.97:00B0F0
  keys -M ctrl s -m ctrl; sleep 3; shown saved
  python3 /tmp/px.py part /tmp/f/look.pptx ppt/slides/slide1.xml "b=\"1\""
  python3 /tmp/px.py part /tmp/f/look.pptx ppt/slides/slide1.xml "sz=\"4000\""
  python3 /tmp/px.py part /tmp/f/look.pptx ppt/slides/slide1.xml "C00000"
  python3 /tmp/px.py part /tmp/f/look.pptx ppt/slides/slide1.xml "algn=\"r\""
  python3 /tmp/px.py part /tmp/f/look.pptx ppt/slides/_rels/slide2.xml.rels "slideLayout6.xml"
  python3 /tmp/px.py part /tmp/f/look.pptx ppt/slides/slide2.xml "00B0F0"
  pkill -x frame; sleep 0.5
  frame /tmp/f/fixture.pptx >>/tmp/app.log 2>&1 & sleep 3
  keys -k Next -k Next; sleep 1; shot fill0
  read x y < <(python3 /tmp/px.py point /shots/fill0.png 0.125 0.30)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 0.5; shown rectangle
  read x y < <(python3 /tmp/px.py where "Fill colour")
  python3 /scripts/dnd_helpers.py click $x $y; sleep 0.5
  python3 /tmp/a11y.py press "Red C00000"; sleep 0.5; shot fill1
  python3 /tmp/px.py /shots/fill1.png filled:0.125:0.3492:C00000
  keys -M ctrl z -m ctrl; sleep 0.5; shot fill2
  python3 /tmp/px.py /shots/fill2.png unfilled:0.125:0.3492:4472C4
  pkill -x frame; sleep 0.5

  echo "== 14 insert"
  cp /tmp/f/layouts.pptx /tmp/f/insert.pptx
  python3 /tmp/px.py png /tmp/f/green.png
  frame /tmp/f/insert.pptx >>/tmp/app.log 2>&1 & sleep 3
  shot ins0
  python3 /tmp/px.py ink /shots/ins0.png fit-before 0.16 0.57 0.84 0.82
  read x y < <(python3 /tmp/px.py point /shots/ins0.png 0.6 0.62)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 0.5
  keys -k End "$(printf "M%.0s" $(seq 120))"; sleep 1; shot ins1
  python3 /tmp/px.py ink /shots/ins1.png fit-inside 0.16 0.57 0.84 0.82
  python3 /tmp/px.py ink /shots/ins1.png fit-below 0.16 0.835 0.84 0.92
  keys -M ctrl z -m ctrl -k Escape; sleep 0.5; shot ins2; shown undone
  python3 /tmp/px.py ink /shots/ins2.png fit-undone 0.16 0.57 0.84 0.82
  keys -M ctrl y -m ctrl -M ctrl m -m ctrl; sleep 1; shot ins3
  python3 /tmp/px.py /shots/ins3.png empty:0.5:0.6:FFFFFF
  read x y < <(python3 /tmp/px.py where Insert)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 1
  python3 /tmp/a11y.py press "Picture…"; sleep 2
  keys -M ctrl l -m ctrl; keys "/tmp/f/green.png"; keys -k Return; sleep 3; shot ins4
  python3 /tmp/px.py /shots/ins4.png filled:0.5:0.6:00FF00
  python3 /tmp/a11y.py text | grep -E "^[0-9]+ × [0-9]+$" | sed "s/^/picture: /"
  keys -M ctrl z -m ctrl; sleep 0.5; shot ins5
  python3 /tmp/px.py /shots/ins5.png unfilled:0.5:0.6:FFFFFF
  keys -M ctrl y -m ctrl; sleep 0.5
  read x y < <(python3 /tmp/px.py where Layouts)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 1
  python3 /tmp/a11y.py press "Title Only"; sleep 1
  # The drag source floats at the bottom right, over the inspector, so the canvas keeps
  # its size.
  python3 /scripts/dnd_helpers.py source /tmp/f/green.png >/dev/null 2>&1 & src=$!; sleep 2
  SWAYSOCK=$(ls /tmp/rt/sway-ipc.*.sock | head -1) swaymsg "[title=dragsrc] floating enable, resize set 200 120, move position 1060 580"; sleep 1
  shot ins6
  read x y < <(python3 /tmp/px.py point /shots/ins6.png 0.5 0.5)
  python3 /scripts/dnd_helpers.py drag 1160 640 $x $y
  sleep 2; kill $src; wait $src 2>/dev/null; sleep 1; shot ins7
  python3 /tmp/px.py /shots/ins7.png dropped:0.5:0.5:00FF00 beside:0.1:0.5:FFFFFF
  read x y < <(python3 /tmp/px.py where Insert)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 1
  python3 /tmp/a11y.py press Text; sleep 1
  keys "Inserted"; sleep 0.5; shown text; keys -k Escape
  read x y < <(python3 /tmp/px.py where Insert)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 1
  python3 /tmp/a11y.py press Shape; sleep 0.5
  python3 /tmp/a11y.py press Oval; sleep 1; shot ins8
  python3 /tmp/px.py /shots/ins8.png oval:0.5:0.45:4472C4
  read x y < <(python3 /tmp/px.py where Insert)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 1
  python3 /tmp/a11y.py press Table; sleep 1; shot ins9
  python3 /tmp/px.py /shots/ins9.png header:0.2:0.445:4472C4 band:0.2:0.5:CFD5EA
  keys -k Delete; sleep 0.5; shot ins10
  python3 /tmp/px.py /shots/ins10.png deleted:0.2:0.5:FFFFFF
  keys -M ctrl z -m ctrl; sleep 0.5; shot ins11
  python3 /tmp/px.py /shots/ins11.png restored:0.2:0.5:CFD5EA
  keys -M ctrl s -m ctrl; sleep 3; shown saved
  python3 /tmp/px.py part /tmp/f/insert.pptx ppt/slides/slide1.xml "normAutofit fontScale="
  python3 /tmp/px.py part /tmp/f/insert.pptx ppt/slides/slide11.xml "<p:pic"
  for n in "<p:pic" "prst=\"ellipse\"" "<a:tbl>" ">Inserted<"; do
    python3 /tmp/px.py part /tmp/f/insert.pptx ppt/slides/slide12.xml "$n"
  done
  python3 /tmp/px.py pictures /tmp/f/insert.pptx slide11.xml
  python3 /tmp/px.py pictures /tmp/f/insert.pptx slide12.xml
  pkill -x frame; sleep 0.5

  echo "== 15 outline"
  cp /tmp/f/layouts.pptx /tmp/f/outline.pptx
  frame /tmp/f/outline.pptx >>/tmp/app.log 2>&1 & sleep 3
  python3 /tmp/a11y.py press Outline; sleep 1; shot out0; shown on
  keys -M ctrl -k Home -m ctrl -k End -k Return "Highlights" -k Return "Growth" -k Tab \
    -k Return "Margins" -k Return -k Tab "Up 3%"; sleep 1; shot out1; shown typed
  keys -M ctrl -M shift o -m shift -m ctrl; sleep 1; shot out2; shown off
  python3 /tmp/a11y.py press Outline; sleep 1
  keys -k Down -k Down -k Down -k End -M shift -k Tab -k Tab -m shift; sleep 1; shot out3; shown promoted
  keys -M ctrl z -m ctrl; sleep 0.5; shown undo1
  keys -M ctrl z -m ctrl; sleep 0.5; shown undo2
  keys -M ctrl -M shift o -m shift -m ctrl; sleep 1; shot out4
  read x y < <(python3 /tmp/px.py point /shots/out4.png 0.5 0.1234)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 0.5
  keys -k End " 2027" -k Escape; sleep 0.5
  python3 /tmp/a11y.py press Outline; sleep 1; shot out5; shown canvas
  keys -k Home "Q"; sleep 0.5; shown home
  keys -M ctrl z -m ctrl -M ctrl z -m ctrl; sleep 0.5; shown shared
  keys -M ctrl y -m ctrl -M ctrl s -m ctrl; sleep 3; shown saved
  python3 /tmp/px.py part /tmp/f/outline.pptx ppt/presentation.xml "<p:sldId "
  for n in ">Highlights 2027<" ">Growth<" ">Margins<" ">Up 3%<" "lvl=\"1\""; do
    python3 /tmp/px.py part /tmp/f/outline.pptx ppt/slides/slide11.xml "$n"
  done
  python3 /tmp/px.py part /tmp/f/outline.pptx ppt/slides/slide1.xml ">subtitle<"
  pkill -x frame; sleep 0.5
  frame /tmp/f/fixture.pptx >>/tmp/app.log 2>&1 & sleep 3
  python3 /tmp/a11y.py press Outline; sleep 1; shot out6; shown fixture
  pkill -x frame; sleep 0.5

  echo "== 5 drop"
  frame >>/tmp/app.log 2>&1 & sleep 3
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

  echo "== 16 new"
  frame >>/tmp/app.log 2>&1 & sleep 3
  shot new0; shown new
  python3 /tmp/px.py aspect /shots/new0.png
  read x y < <(python3 /tmp/px.py where Layouts)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 1; shot newmenu; shown menu
  keys -k Escape; sleep 0.5
  read x y < <(python3 /tmp/px.py point /shots/new0.png 0.5 0.3377)
  python3 /scripts/dnd_helpers.py click $x $y; sleep 0.5
  keys "Quarterly review" -k Escape; sleep 0.5; shot new1; shown typed
  keys -M ctrl s -m ctrl; sleep 2; shot newdialog; shown dialog
  keys -M ctrl a -m ctrl "/tmp/f/new.pptx" -k Return; sleep 3; shot newsaved; shown saved
  python3 /tmp/px.py part /tmp/f/new.pptx ppt/slides/slide1.xml ">Quarterly review<"
  python3 /tmp/px.py parts /tmp/f/new.pptx "^ppt/slideLayouts/slideLayout[0-9]+[.]xml$"
  keys -M ctrl n -m ctrl; sleep 3; shown second
  keys -M ctrl n -m ctrl; sleep 3; shot new2; shown third
  pkill -x frame; sleep 0.5

  # FRAME-15: the spec reference task, by keyboard and pointer, on the canvas (17) and in
  # the outline (18), its input actions counted: press (each a key or a chord: Return,
  # shift+Tab, ctrl+Return) and tap (a click on the widget named) count one each; say
  # types the content (titles, bullets, file paths), which does not count.
  press() {
    local k key m a mods
    for k in "$@"; do
      key=${k##*+}; a=(); mods=()
      [ "$key" != "$k" ] && IFS=+ read -ra mods <<<"${k%+*}"
      for m in "${mods[@]}"; do a+=(-M "$m"); done
      if [ ${#key} = 1 ]; then a+=("$key"); else a+=(-k "$key"); fi
      for m in "${mods[@]}"; do a+=(-m "$m"); done
      keys "${a[@]}"; n=$((n + 1))
    done
  }
  tap() {
    read x y < <(python3 /tmp/px.py where "$1")
    python3 /scripts/dnd_helpers.py click $x $y; n=$((n + 1)); sleep 1
  }
  say() { keys "$1"; }
  python3 /tmp/px.py png /tmp/f/green.png

  echo "== 17 task on the canvas"
  n=0
  frame >>/tmp/app.log 2>&1 & sleep 3
  say "Quarterly review"; press ctrl+Return
  say "Frame against PowerPoint"; press ctrl+Return
  say "Highlights"; press ctrl+Return
  say "Growth"; press Return
  say "Margins"; press Return Tab
  say "Up 3%"; press ctrl+Return
  say "Revenue"; tap Insert; tap "Picture…"
  say "/tmp/f/green.png"; press Return; sleep 3
  press ctrl+m
  say "Next steps"; press ctrl+Return
  say "Hire"; press Return
  say "Ship"; sleep 1; shot task-canvas; shown canvas
  press ctrl+s; sleep 2
  say "/tmp/f/canvas-task"; press Return; sleep 3; shown saved
  echo "actions: $n"
  cp /tmp/f/canvas-task.pptx /out/
  pkill -x frame; sleep 0.5

  echo "== 18 task in the outline"
  n=0
  frame >>/tmp/app.log 2>&1 & sleep 3
  press ctrl+shift+o; sleep 1
  say "Quarterly review"; press Return
  say "Frame against PowerPoint"; press Tab ctrl+Return
  say "Highlights"; press Return
  say "Growth"; press Tab Return
  say "Margins"; press Return Tab
  say "Up 3%"; press ctrl+Return
  say "Revenue"; tap Insert; tap "Picture…"
  say "/tmp/f/green.png"; press Return; sleep 3
  press ctrl+Return
  say "Next steps"; press Return
  say "Hire"; press Tab Return
  say "Ship"; sleep 1; shot task-outline; shown outline
  press ctrl+s; sleep 2
  say "/tmp/f/outline-task"; press Return; sleep 3; shown saved
  echo "actions: $n"
  cp /tmp/f/outline-task.pptx /out/
  pkill -x frame; sleep 0.5

  echo "== 19 install"
  # A release as the installer takes it: latest.json beside a tarball of one folder with
  # the binary, its desktop entry, and its icon (the layout of Bricks releases).
  rel=frame-9.9.9-x86_64-linux
  mkdir -p /tmp/site/$rel
  cp /usr/local/bin/frame /packaging/dev.cronkite.Frame.desktop /packaging/dev.cronkite.Frame.svg /tmp/site/$rel/
  tar -C /tmp/site -czf /tmp/site/$rel.tar.gz $rel
  sha=$(sha256sum /tmp/site/$rel.tar.gz | cut -d" " -f1)
  printf "{\"version\":\"9.9.9\",\"url\":\"http://127.0.0.1:8765/%s.tar.gz\",\"sha256\":\"%s\"}\n" \
    $rel $sha > /tmp/site/latest.json
  python3 -m http.server 8765 --bind 127.0.0.1 --directory /tmp/site >/dev/null 2>&1 & web=$!; sleep 1
  mkdir -p ~/.config
  printf "[Default Applications]\nimage/png=eog.desktop\napplication/vnd.openxmlformats-officedocument.presentationml.presentation=impress.desktop\n" \
    > ~/.config/mimeapps.list
  FRAME_DOWNLOAD_URL=http://127.0.0.1:8765 sh /sites/frame/install.sh 2>&1 | sed "s/^/install: /"
  gio mime application/vnd.openxmlformats-officedocument.presentationml.presentation 2>&1 |
    head -1 | sed "s/^/mime: /"
  grep ^Exec= ~/.local/share/applications/dev.cronkite.Frame.desktop | sed "s/^/entry: /"
  # gio open is what a file manager does on a double-click.
  cp /tmp/f/layouts.pptx /tmp/f/clicked.pptx
  gio open /tmp/f/clicked.pptx >>/tmp/app.log 2>&1; sleep 3; shot clicked; shown clicked
  pgrep -a frame | sed "s/^[0-9]* /process: /"
  pkill -x frame; sleep 0.5
  sh /sites/frame/install.sh --uninstall | sed "s/^/uninstall: /"
  ls ~/.local/bin/frame ~/.local/share/applications/dev.cronkite.Frame.desktop \
    ~/.local/share/icons/hicolor/scalable/apps/dev.cronkite.Frame.svg 2>&1 | grep -c "No such" | sed "s/^/gone: /"
  sed "s/^/after: /" ~/.config/mimeapps.list
  kill $web
  swaymsg exit >/dev/null 2>&1 || true
}
dbus-run-session -- bash -c "$(declare -f session); session" 2>/tmp/dbus.log
echo "== app log"; grep -E "^frame:|panicked|^thread" -A2 /tmp/app.log | head -30
' 2>&1) || true
# 17, 18: the task decks as LibreOffice reads them: a line per paragraph with text (slide,
# shape kind, level, text) and per picture (slide, type, pixel size), then the slides.
lo_summary='
import sys
slide, kind, slides = 0, "", 0
for line in sys.stdin:
    f = line.rstrip("\n").split("\t")
    if f[0] == "slide":
        slide, slides = int(f[1]), slides + 1
    elif f[0] == "shape":
        kind = f[2]
    elif f[0] == "para" and f[3]:
        print(f"lo {slide} {kind} {f[1]} {f[3]}")
    elif f[0] == "picture":
        print(f"lo {slide} picture {f[1]} {f[2]}")
print(f"lo slides: {slides}")
'
for d in 17:canvas-task 18:outline-task; do
  [ -f "$ref/${d#*:}.pptx" ] || continue
  out+=$'\n== '"${d%%:*} lo"$'\n'$(python3 scripts/lo_pptx.py dump "$ref/${d#*:}.pptx" 2>&1 |
    python3 -c "$lo_summary")
done
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
check "5: without a file, the new deck window shows" "5 drop" "^start: Untitled$"
check "5: ... on its one slide" "5 drop" "^start: Slide 1 of 1$"
check "5: a dropped deck opens in its own window" "5 drop" "^shown: layouts\.pptx$"
check "5: ... and shows it" "5 drop" "^shown: Slide 1 of 10$"
check "5: ... beside the new deck" "5 drop" "^shown: Untitled$"
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
# FRAME-6: editing slide text.
check "9: a click puts the caret in the subtitle; Left moves it, not the slide" "9 editing" "^left: Slide 1 of 10$"
check "9: typing changes the placeholder" "9 editing" "^typed: subtitle here$"
b=$(section "9 editing" | sed -n "s/^ink before: //p"); a=$(section "9 editing" | sed -n "s/^ink after: //p")
if [ -n "$a" ] && [ -n "$b" ] && [ "$a" -gt $((b + 100)) ]; then echo "ok   9: ... and the slide draws the typed text ($b -> $a dark pixels)"; else echo "FAIL 9: the slide draws the typed text (${b:-?} -> ${a:-?} dark pixels)"; fail=1; fi
check "9: Ctrl+Z undoes the last word" "9 editing" "^undo1: subtitle $"
check "9: ... and then the space before it" "9 editing" "^undo2: subtitle$"
check "9: Ctrl+Shift+Z redoes the space" "9 editing" "^redo1: subtitle $"
check "9: Ctrl+Y redoes the word" "9 editing" "^redo2: subtitle here$"
check "9: Enter starts a paragraph" "9 editing" "^enter: second$"
check "9: ... after the first" "9 editing" "^enter: subtitle here$"
absent "9: Ctrl+A and Delete empty the placeholder" "9 editing" "^cleared: (subtitle|second)"
check "9: typing fills the empty placeholder" "9 editing" "^retyped: Typed$"
check "9: Escape stops editing; Page Down changes slides again" "9 editing" "^escape: Slide 2 of 10$"
check "9: typing in a text box changes it" "9 editing" "^textbox: Text box: bold blue and theme in Liberation Serif!$"
# FRAME-7: adding, duplicating, deleting, and moving slides.
check "10: Ctrl+M after the title slide adds slide 2" "10 slides" "^ctrlm: Slide 2 of 11$"
check "10: ... on Title and Content" "10 slides" "^ctrlm: Slide 2, Title and Content$"
check "10: ... and the slide list follows" "10 slides" "^ctrlm: Slide 11$"
check "10: New Slide adds one on the current slide's layout" "10 slides" "^button: Slide 3, Title and Content$"
check "10: ... after it" "10 slides" "^button: Slide 3 of 12$"
for l in "Title Slide" "Title and Content" "Two Content" "Picture with Caption" "Vertical Title and Text"; do
  check "10: the New Slide menu shows the layout $l" "10 slides" "^menu: $l$"
done
check "10: picking Two Content adds a slide on it" "10 slides" "^picked: Slide 4, Two Content$"
check "10: ... after the current one" "10 slides" "^picked: Slide 4 of 13$"
check "10: Ctrl+Z takes it out, back on the slide before" "10 slides" "^undo1: Slide 3 of 12$"
check "10: ... and the others, one step each" "10 slides" "^undo3: Slide 1 of 10$"
absent "10: ... the list too" "10 slides" "^undo3: Slide 11$"
check "10: Ctrl+Y adds the first back" "10 slides" "^redo1: Slide 2, Title and Content$"
check "10: Ctrl+D duplicates slide 3" "10 slides" "^dup: Slide 4, Section Header$"
check "10: ... after it" "10 slides" "^dup: Slide 4 of 11$"
check "10: Ctrl+Up twice moves the copy to slide 2" "10 slides" "^up: Slide 2, Section Header$"
check "10: Delete in the slide list deletes slide 1" "10 slides" "^deleted: Slide 1 of 10$"
absent "10: ... and its text is gone" "10 slides" "^deleted: Centered Title$"
check "10: Ctrl+Z brings it back" "10 slides" "^undelete: Slide 1, Title Slide$"
check "10: dragging slide 1 below slide 3 moves it to 3" "10 slides" "^dragged: Slide 3, Title Slide$"
check "10: ... the slide list shows it there" "10 slides" "^dragged: Slide 3 of 11$"
check "10: Ctrl+Z moves it back" "10 slides" "^undrag: Slide 1, Title Slide$"
# FRAME-8: saving by patching the package.
check "11: Ctrl+S saves" "11 save" "^saved: Saved “save\.pptx”$"
check "11: ... the typed subtitle into slide 1's part" "11 save" "^part ppt/slides/slide1.xml subtitle saved: 1$"
check "11: ... the new slide into the slide list" "11 save" "^part ppt/presentation.xml <p:sldId : 11$"
check "11: ... and into a part of its own" "11 save" "^part ppt/slides/slide11.xml >Agenda<: 1$"
check "11: the saved deck reopens with 11 slides" "11 save" "^reopened: Slide 1 of 11$"
check "11: ... the subtitle as typed" "11 save" "^reopened: subtitle saved$"
check "11: ... the new slide second, on Title and Content" "11 save" "^second: Slide 2, Title and Content$"
check "11: ... its title typed" "11 save" "^second: Agenda$"
check "11: Save As saves under the name typed" "11 save" "^saveas: Saved “saved-as\.pptx”$"
check "11: ... and the window is that file's" "11 save" "^saveas: saved-as\.pptx$"
check "11: the saved-as deck reopens" "11 save" "^reopened2: Slide 1 of 11$"
# FRAME-10: moving shapes with snapping and guides.
check "12: before a drag, no guide" "12 arrange" "^count noguide: 0$"
check "12: mid-drag near the centre, a guide runs down the middle" "12 arrange" "^count guide: ([3-9][0-9]{2}|[0-9]{4,})$"
check "12: ... and the shape lands centred" "12 arrange" "^blue centred: (-1|0|1)$"
absent "12: ... (it moved)" "12 arrange" "^blue before: (-1|0|1)$"
check "12: Ctrl+Z puts it back" "12 arrange" "^undone: yes "
check "12: with Alt held it lands where dropped" "12 arrange" "^blue free: [2-5]$"
check "12: Shift+Right and Right nudge it 11 px" "12 arrange" "^nudged: (10|11|12)$"
check "12: Ctrl+Z undoes the last nudge" "12 arrange" "^unnudged: (9|10|11)$"
# FRAME-11: the inspector.
check "13: nothing selected, the inspector shows the slide's layout" "13 inspector" "^slide: Slide layout$"
check "13: ... Title Slide" "13 inspector" "^slide: Title Slide$"
check "13: ... and where the background comes from" "13 inspector" "^slide: From the (layout|master|presentation)$"
check "13: text selected shows the text rows" "13 inspector" "^text: Font family$"
check "13: ... the inherited ones captioned" "13 inspector" "^text: From the (layout|master|presentation)$"
check "13: Bold turns bold on" "13 inspector" "^bold: on$"
v13() { section "13 inspector" | sed -n "s/^$1: //p" | head -1; }
b=$(v13 "ink before"); a=$(v13 "ink after"); r=$(v13 "count red")
if [ -n "$a" ] && [ -n "$b" ] && [ "$a" -gt $((b + 300)) ]; then echo "ok   13: bold at 40 points draws more ink ($b -> $a dark pixels)"; else echo "FAIL 13: bold at 40 points draws more ink (${b:-?} -> ${a:-?} dark pixels)"; fail=1; fi
if [ -n "$r" ] && [ "$r" -ge 300 ]; then echo "ok   13: ... in red C00000 ($r pixels)"; else echo "FAIL 13: the text draws in red C00000 (${r:-?} pixels, want >= 300)"; fail=1; fi
l=$(v13 "ink right-left"); r=$(v13 "ink right-right")
if [ -n "$l" ] && [ -n "$r" ] && [ "$r" -gt $((4 * l)) ]; then echo "ok   13: Align right moves the text right ($l left, $r right)"; else echo "FAIL 13: Align right moves the text right (${l:-?} left, ${r:-?} right)"; fail=1; fi
check "13: Ctrl+Z undoes the alignment" "13 inspector" "^right after undo: off$"
l=$(v13 "ink undone-left"); r=$(v13 "ink undone-right")
if [ -n "$l" ] && [ -n "$r" ] && [ "$l" -gt 100 ] && [ "$r" -gt 100 ]; then echo "ok   13: ... the text centred again ($l left, $r right)"; else echo "FAIL 13: Ctrl+Z centres the text again (${l:-?} left, ${r:-?} right)"; fail=1; fi
check "13: Ctrl+Y redoes it" "13 inspector" "^right after redo: on$"
check "13: Layout moves slide 2 to Title Only" "13 inspector" "^relayout: Slide 2, Title Only$"
check "13: Background colours the slide light blue" "13 inspector" "^match background: yes"
check "13: Ctrl+S saves" "13 inspector" "^saved: Saved “look\.pptx”$"
for n in 'b="1"' 'sz="4000"' 'C00000' 'algn="r"'; do
  check "13: ... $n into slide 1's part" "13 inspector" "^part ppt/slides/slide1.xml $n: [1-9]"
done
check "13: ... the Title Only layout into slide 2's relationships" "13 inspector" "^part ppt/slides/_rels/slide2.xml.rels slideLayout6.xml: [1-9]"
check "13: ... the background into slide 2's part" "13 inspector" "^part ppt/slides/slide2.xml 00B0F0: [1-9]"
check "13: Fill colours the fixture's rectangle red" "13 inspector" "^match filled: yes"
check "13: ... and Ctrl+Z gives back the blue" "13 inspector" "^match unfilled: yes"
# FRAME-12: inserting, and text shrinking to fit.
v14() { section "14 insert" | sed -n "s/^$1: //p" | head -1; }
b=$(v14 "ink fit-before"); a=$(v14 "ink fit-inside"); o=$(v14 "ink fit-below")
if [ -n "$a" ] && [ -n "$b" ] && [ -n "$o" ] && [ "$a" -gt $((2 * b)) ] && [ "$o" -le 20 ]; then echo "ok   14: a long word typed into the subtitle shrinks to fit its box ($b -> $a dark pixels, $o below it)"; else echo "FAIL 14: a long word typed into the subtitle shrinks to fit its box (${b:-?} -> ${a:-?} dark pixels, ${o:-?} below it)"; fail=1; fi
u=$(v14 "ink fit-undone")
if [ -n "$u" ] && [ -n "$b" ] && [ "$u" -ge $((b * 9 / 10)) ] && [ "$u" -le $((b * 11 / 10)) ]; then echo "ok   14: ... one Ctrl+Z undoes the typing and the shrink ($u dark pixels, $b before)"; else echo "FAIL 14: one Ctrl+Z undoes the typing and the shrink (${u:-?} dark pixels, ${b:-?} before)"; fail=1; fi
check "14: ... the subtitle as it was" "14 insert" "^undone: subtitle$"
check "14: Ctrl+M adds a Title and Content slide, its body empty" "14 insert" "^match empty: yes"
check "14: Insert, Picture… and a typed path fill the body placeholder" "14 insert" "^match filled: yes"
check "14: ... the inspector shows the picture" "14 insert" "^picture: 400 × 200$"
check "14: ... and Ctrl+Z empties it again" "14 insert" "^match unfilled: yes"
check "14: a picture dropped on a Title Only slide lands centred" "14 insert" "^match dropped: yes"
check "14: ... at its size, not over the whole slide" "14 insert" "^match beside: yes"
check "14: Insert, Text and typing make a text box" "14 insert" "^text: Inserted$"
check "14: Insert, Shape, Oval adds an accent oval at the centre" "14 insert" "^match oval: yes"
check "14: Insert, Table adds a table with an accent header" "14 insert" "^match header: yes"
check "14: ... and banded rows" "14 insert" "^match band: yes"
check "14: Delete deletes the selected table" "14 insert" "^match deleted: yes"
check "14: ... and Ctrl+Z restores it" "14 insert" "^match restored: yes"
check "14: Ctrl+S saves" "14 insert" "^saved: Saved “insert\.pptx”$"
check "14: ... the shrunk subtitle as normAutofit" "14 insert" "^part ppt/slides/slide1.xml normAutofit fontScale=: [1-9]"
check "14: ... the filled placeholder as a picture" "14 insert" "^part ppt/slides/slide11.xml <p:pic: [1-9]"
check "14: ... the dropped picture" "14 insert" "^part ppt/slides/slide12.xml <p:pic: [1-9]"
check "14: ... the oval" "14 insert" "^part ppt/slides/slide12.xml prst=\"ellipse\": [1-9]"
check "14: ... the table" "14 insert" "^part ppt/slides/slide12.xml <a:tbl>: [1-9]"
check "14: ... the text box" "14 insert" "^part ppt/slides/slide12.xml >Inserted<: [1-9]"
check "14: ... and their pictures into the package" "14 insert" "^pictures slide11.xml: 1 of 1$"
check "14: ... (the dropped one)" "14 insert" "^pictures slide12.xml: 1 of 1$"
# FRAME-13: the outline.
check "15: the Outline button shows the outline" "15 outline" "^on: 1  Centered Title$"
check "15: ... the bullets under their title" "15 outline" "^on: • subtitle$"
check "15: Enter after a title starts a slide, its title typed" "15 outline" "^typed: 2  Highlights$"
check "15: ... Tab turns the next title into its bullet" "15 outline" "^typed: • Growth$"
check "15: ... Enter starts a bullet" "15 outline" "^typed: • Margins$"
check "15: ... and Tab takes it a level down" "15 outline" "^typed: ◦ Up 3%$"
check "15: ... slide 1 as it was" "15 outline" "^typed: • subtitle$"
check "15: ... the slides are 11, slide 2 shown" "15 outline" "^typed: Slide 2 of 11$"
check "15: ... the slide list has 11" "15 outline" "^typed: Slide 11$"
absent "15: ... not 12" "15 outline" "^typed: Slide 12$"
check "15: Ctrl+Shift+O shows the canvas on slide 2, Title and Content" "15 outline" "^off: Slide 2, Title and Content$"
for t in Highlights Growth Margins "Up 3%"; do check "15: ... the slide holds $t" "15 outline" "^off: $t$"; done
check "15: Shift+Tab twice makes Up 3% slide 3's title" "15 outline" "^promoted: 3  Up 3%$"
check "15: ... a slide of its own" "15 outline" "^promoted: Slide 3 of 12$"
check "15: Ctrl+Z makes it a bullet again" "15 outline" "^undo1: • Up 3%$"
absent "15: ... not a slide" "15 outline" "^undo1: 3  Up 3%$"
check "15: ... the slides 11 again" "15 outline" "^undo1: Slide 2 of 11$"
check "15: Ctrl+Z again takes it a level down again" "15 outline" "^undo2: ◦ Up 3%$"
check "15: text typed on the canvas shows in the outline" "15 outline" "^canvas: 2  Highlights 2027$"
check "15: Home in the outline goes to the start of the title, past its number" "15 outline" "^home: 2  QHighlights 2027$"
check "15: Ctrl+Z in the outline undoes the canvas typing" "15 outline" "^shared: 2  Highlights $"
check "15: Ctrl+S saves" "15 outline" "^saved: Saved “outline\.pptx”$"
check "15: ... 11 slides" "15 outline" "^part ppt/presentation.xml <p:sldId : 11$"
for n in ">Highlights 2027<" ">Growth<" ">Margins<" ">Up 3%<" "lvl=\"1\""; do
  check "15: ... $n into the new slide part" "15 outline" "^part ppt/slides/slide11.xml $n: [1-9]"
done
check "15: ... slide 1 as it was" "15 outline" "^part ppt/slides/slide1.xml >subtitle<: 1$"
check "15: a slide with shapes shows how many other items it holds" "15 outline" "^fixture: 8 other items$"
# FRAME-14: new decks.
check "16: without a file, an Untitled window" "16 new" "^new: Untitled$"
check "16: ... on slide 1 of 1" "16 new" "^new: Slide 1 of 1$"
check "16: ... a Title Slide" "16 new" "^new: Slide 1, Title Slide$"
check "16: ... 16:9 on the canvas" "16 new" "^aspect [0-9]+x[0-9]+: yes$"
for l in "Title Slide" "Title and Content" "Section Header" "Two Content" "Title Only"; do
  check "16: the New Slide menu lists $l" "16 new" "^menu: $l$"
done
for l in "Blank" "Comparison" "Content with Caption" "Picture with Caption" "Title and Vertical Text"; do
  absent "16: ... and not $l" "16 new" "^menu: $l$"
done
check "16: a click on the title and typing fill it" "16 new" "^typed: Quarterly review$"
check "16: Ctrl+S on a new deck asks where, as Save As" "16 new" "^dialog: Save As$"
check "16: ... a typed path saves it" "16 new" "^saved: Saved “new\.pptx”$"
check "16: ... and the window is that file's" "16 new" "^saved: new\.pptx$"
check "16: ... the title into slide 1's part" "16 new" "^part ppt/slides/slide1.xml >Quarterly review<: 1$"
check "16: ... and the five layouts into the package" "16 new" "^parts .*slideLayout.*: 5$"
check "16: Ctrl+N opens another Untitled window" "16 new" "^second: Untitled$"
check "16: ... beside the saved one" "16 new" "^second: new\.pptx$"
check "16: Ctrl+N again opens Untitled 2" "16 new" "^third: Untitled 2$"
# FRAME-15: each way builds the task in fewer input actions than PowerPoint, and
# LibreOffice reads the saved deck as the task: these lines, no others.
task="lo 1 TitleTextShape 0 Quarterly review
lo 1 SubtitleShape 0 Frame against PowerPoint
lo 2 TitleTextShape 0 Highlights
lo 2 OutlinerShape 0 Growth
lo 2 OutlinerShape 0 Margins
lo 2 OutlinerShape 1 Up 3%
lo 3 TitleTextShape 0 Revenue
lo 3 picture image/png 400x200
lo 4 TitleTextShape 0 Next steps
lo 4 OutlinerShape 0 Hire
lo 4 OutlinerShape 0 Ship
lo slides: 4"
for t in "17 task on the canvas:canvas-task" "18 task in the outline:outline-task"; do
  run=${t%%:*}; file=${t#*:}; n=${run%% *}
  check "$n: Ctrl+S and a typed path save the task" "$run" "^saved: Saved “$file\.pptx”$"
  actions=$(section "$run" | sed -n "s/^actions: //p")
  if [ -n "$actions" ] && [ "$actions" -lt "$powerpoint" ]; then
    echo "ok   $n: the task ${run#* task } takes $actions input actions, fewer than PowerPoint's $powerpoint"
  else
    echo "FAIL $n: the task ${run#* task } takes ${actions:-uncounted} input actions, not fewer than PowerPoint's $powerpoint"
    fail=1
  fi
  got=$(section "$n lo" | grep "^lo ")
  if [ "$got" = "$task" ]; then echo "ok   $n: LibreOffice reads the saved deck as the task"; else
    echo "FAIL $n: LibreOffice reads the saved deck as:"
    sed "s/^/       /" <<<"${got:-(nothing; is LibreOffice installed?)}"; fail=1
  fi
done
# FRAME-9: installing, and double-clicking a .pptx.
check "19: the installer installs Frame" "19 install" "^install: Installed Frame 9\.9\.9$"
check "19: ... as the default for .pptx" "19 install" "^mime: .*: dev\.cronkite\.Frame\.desktop$"
check "19: ... its menu entry running the installed binary" "19 install" "^entry: Exec=/root/\.local/bin/frame %f$"
check "19: double-clicking a .pptx (gio open) opens it in Frame" "19 install" "^clicked: clicked\.pptx$"
check "19: ... showing its slides" "19 install" "^clicked: Slide 1 of 10$"
check "19: ... the installed Frame" "19 install" "^process: /root/\.local/bin/frame /tmp/f/clicked\.pptx$"
check "19: --uninstall removes the binary, menu entry, and icon" "19 install" "^gone: 3$"
check "19: ... keeping other apps' defaults" "19 install" "^after: image/png=eog\.desktop$"
absent "19: ... and dropping its .pptx default" "19 install" "^after: .*dev\.cronkite\.Frame"
if [ $fail = 0 ]; then echo "FRAME OK"; else
  echo "FRAME CHECK FAILED"; [ -n "${VERBOSE:-}" ] || echo "(VERBOSE=1 for the transcript)"
  exit 1
fi
