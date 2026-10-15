#!/usr/bin/env bash
# ETCH-2 acceptance: Etch's window and canvas.
#
#   scripts/check_etch.sh [BINARY]   (default target/release/etch)
#
# In an Arch container under headless sway (1280x720). What the app shows is read through
# the accessibility tree (scripts/a11y_helpers.py, pointed at the app "etch"): the status
# bar's pointer position, zoom, and image size. Screenshots (grim) are read pixel by pixel
# inside the canvas (its accessible extents); the pointer moves, drags, and turns the wheel
# through the wlr-virtual-pointer protocol (scripts/dnd_helpers.py).
#   1. etch shows a white 1024x768 image: the status says 1024×768, the white area is 4:3
#      and inkless; Ctrl+0 shows it at 100%;
#   2. a drag draws black pixels along the pointer's path: every sample of the line from
#      the press to the release has a black pixel within a pixel of it;
#   3. Ctrl+Z removes the stroke (no black left), Ctrl+Shift+Z restores it (as many black
#      pixels as before), and so do Ctrl+Z and Ctrl+Y;
#   4. resizing the window (floating, 900x560) and tiling it again keeps the drawing: as
#      many black pixels, still at 100%;
#   5. Ctrl+wheel up zooms in a step a notch, keeping the pixel under the pointer (the
#      status position doesn't move) through 150% to 800%; at 800% the stroke's pixels are
#      sharp squares: no pixel between black and white around it (away from the pointer,
#      where the tip's ring is);
#   6. the wheel pans: a notch down moves the image 48 screen px, 6 image px at 800%;
#      Shift+wheel sideways; a middle-button drag and a drag with Space held move the image
#      with the pointer (the pixel grabbed ends under it); Space's drag draws nothing: after
#      Fit to Window (Ctrl+9), one Ctrl+Z leaves no ink;
#   7. Ctrl+N's dialog makes a 640x480 image in a new window (Untitled 2), and offers that
#      size the next time; Escape closes it;
#   8. `etch --bench-stroke 120` draws a stroke on a 7680x4320 image at 100% a segment a
#      frame, each segment under 8 ms p99 (frame times printed; the frame target is the
#      reference box's, not this software-rendered container's).
# ETCH-3 acceptance: the eraser and the tool column.
#   9. on a new image the tool column shows Pencil on; a pencil line drawn, a click on
#      Eraser turns it on (and Pencil off), and the same drag erases the line: no black
#      left, and the checkerboard's grey shows through where it was; Ctrl+Z gives the
#      line back (as many black pixels, no grey); the P and E keys pick the tools.
# ETCH-4 acceptance: colours and sizes.
#  10. the colour row shows black on white; a click on Red makes red the foreground and a
#      right-click on Indigo makes indigo the background; Size 7 and a sideways drag
#      draw a red line 7 px thick (the canvas column through it); X swaps the colours,
#      and the next line is indigo, 7 px; the eraser keeps its own size (9) and the
#      pencil its 7.
# ETCH-6 acceptance: opening images.
#  11. `etch t.png` (400x300: the left half opaque red, the right half's top fully
#      transparent green, its bottom half-transparent blue) opens it in a window titled
#      t.png, at 100%, the status saying 400×300; on screen the left half is red, the top
#      right shows the checkerboard (white and grey, no green), and the bottom right blue
#      blended over it. In an Untitled window, Ctrl+O shows the file dialog; typing a path
#      (Ctrl+L) to a 320x200 JPEG opens it in a new window titled c.jpg (320×200) beside
#      the Untitled one. `etch bad.png` (t.png cut short) alerts "Cannot open “bad.png”"
#      with the reason, and Enter closes the alert and its window (etch exits);
#      `etch notes.png` (text) alerts that it isn't an image Etch can open.
# ETCH-7 acceptance: saving.
#  12. `etch s.png` (400x300, white) is titled s.png; a stroke marks it "• s.png"; Ctrl+S
#      clears the mark and the file (read back with GdkPixbuf) now has the stroke's black;
#      another stroke marks it, Ctrl+Z back to the saved image clears it, Ctrl+Y marks it.
#      Ctrl+W asks "Save changes to “s.png” before closing?" with Cancel, Discard, Save:
#      Cancel keeps the window (still marked), Discard closes it (etch exits) and leaves
#      the file byte for byte; reopened and edited, Save in the question saves (more black
#      in the file) and closes. Ctrl+Shift+S shows Save As, the PNG image filter first;
#      typing /tmp/s2.jpg writes a JPEG and the window becomes s2.jpg. Erasing the first
#      line then Ctrl+S asks before saving transparency as JPEG ("... become white"); Save
#      writes it with the line white (less black). Save As now offers the JPEG image
#      filter first. Ctrl+S on an Untitled image asks where: Save As, offering
#      Untitled.png.
# ETCH-8 acceptance: the brush.
#  13. B picks the brush (the tool column shows it on), whose Hard and Soft buttons show
#      beside the size only while it's the tool, Soft on, size 8 at first; at size 15 a
#      sideways drag draws a soft black line: down the canvas column through it, a black
#      core and grey pixels fading to white on each side; Hard and another drag draw a
#      line 15 px black with no grey; Ctrl+Z takes back the hard line and leaves the soft
#      one, another Ctrl+Z the soft one, and Ctrl+Y gives it back the same.
# Needs docker. VERBOSE=1 prints the transcript; SHOTS=DIR saves screenshots.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(realpath "${1:-target/release/etch}")
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p etch)"; exit 1; }

shots=()
if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS"; shots=(-v "$(realpath "$SHOTS"):/shots"); fi
# Arch's sway binary carries cap_sys_nice: exec fails without it in the container's set.
out=$(docker run --rm --cap-add SYS_NICE -v "$bin:/usr/local/bin/etch:ro" \
  -v "$PWD/scripts:/scripts:ro" "${shots[@]}" archlinux:latest bash -c '
set -uo pipefail
pacman -Syu --noconfirm --needed sway wtype grim gtk4 ttf-dejavu dbus python-gobject at-spi2-core \
  python-pywayland wlr-protocols wayland pkgconf >/dev/null 2>&1
mkdir -p /shots /tmp/rt /tmp/proto && chmod 700 /tmp/rt && touch /tmp/proto/__init__.py
python -m pywayland.scanner -i /usr/share/wayland/wayland.xml \
  /usr/share/wlr-protocols/unstable/wlr-virtual-pointer-unstable-v1.xml -o /tmp/proto/wlr_proto >/dev/null
export PYTHONPATH=/tmp/proto XDG_RUNTIME_DIR=/tmp/rt GDK_DEBUG=no-portals GDK_BACKEND=wayland
printf "output HEADLESS-1 resolution 1280x720 position 0 0\ndefault_border none\n" > /tmp/sway.conf
sed "s/^APP = .*/APP = \"etch\"/" /scripts/a11y_helpers.py > /tmp/a11y.py
cat > /tmp/px.py <<"PY"
import sys
import gi
gi.require_version("Atspi", "2.0")
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import Atspi, GdkPixbuf

def canvas():
    # The largest image in the app: the canvas.
    desktop = Atspi.get_desktop(0)
    stack = [desktop.get_child_at_index(i) for i in range(desktop.get_child_count())]
    stack = [a for a in stack if a is not None and a.get_name() == "etch"]
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

def pixels(path):
    p = GdkPixbuf.Pixbuf.new_from_file(path)
    rs, nc, px = p.get_rowstride(), p.get_n_channels(), p.get_pixels()
    return lambda x, y: tuple(px[y * rs + x * nc + k] for k in range(3))

def image(path):
    # The white image in the canvas: the white run through the canvas middle, each way.
    cx, cy, cw, ch = canvas()
    at = pixels(path)
    white = lambda p: min(p) >= 245
    my, mx = cy + ch // 2, cx + cw // 2
    row = [x for x in range(cx, cx + cw) if white(at(x, my))]
    col = [y for y in range(cy, cy + ch) if white(at(mx, y))]
    return at, row[0], col[0], row[-1] - row[0] + 1, col[-1] - col[0] + 1

def count(path, dark):
    # Pixels inside the image (2 px in from its edges) darker than DARK in every channel.
    at, l, t, w, h = image(path)
    return sum(1 for y in range(t + 2, t + h - 2) for x in range(l + 2, l + w - 2) if max(at(x, y)) < dark)

cmd = sys.argv[1]
if cmd == "canvas":
    print(*canvas())
elif cmd == "centre":
    cx, cy, cw, ch = canvas()
    print(cx + cw // 2, cy + ch // 2)
elif cmd == "image":
    _, l, t, w, h = image(sys.argv[2])
    print(f"image {w}x{h}: " + ("4:3" if abs(w * 3 - h * 4) <= 8 else "not 4:3"))
elif cmd == "black":
    print(count(sys.argv[2], 60))
elif cmd == "ink":
    print(count(sys.argv[2], 200))
elif cmd == "line":
    # Samples of the line from (X1, Y1) to (X2, Y2) with a black pixel within one of them.
    at = pixels(sys.argv[2])
    x1, y1, x2, y2 = map(float, sys.argv[3:7])
    n, hit = 40, 0
    for i in range(n + 1):
        x, y = round(x1 + (x2 - x1) * i / n), round(y1 + (y2 - y1) * i / n)
        if any(max(at(x + dx, y + dy)) < 60 for dx in (-1, 0, 1) for dy in (-1, 0, 1)):
            hit += 1
    print(f"{hit}/{n + 1}")
elif cmd == "sharp":
    # Around (X, Y): black pixels, and pixels between black and white (a smoothed edge).
    at = pixels(sys.argv[2])
    x0, y0, r = int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5])
    box = [at(x, y) for y in range(y0 - r, y0 + r) for x in range(x0 - r, x0 + r)]
    black = sum(1 for p in box if max(p) < 60)
    between = sum(1 for p in box if 60 <= max(p) and min(p) < 245)
    print(f"black {black} between {between}")
elif cmd == "checker":
    # Pixels inside the image of the checkerboard grey (CCCCCC): transparency shown.
    at, l, t, w, h = image(sys.argv[2])
    print(sum(1 for y in range(t + 2, t + h - 2) for x in range(l + 2, l + w - 2)
              if all(195 <= c <= 213 for c in at(x, y))))
elif cmd == "column":
    # Pixels of colour HEX in the image column at screen X.
    at, l, t, w, h = image(sys.argv[2])
    x, want = int(sys.argv[3]), sys.argv[4]
    wc = tuple(int(want[i:i + 2], 16) for i in (0, 2, 4))
    print(sum(1 for y in range(t, t + h) if all(abs(a - b) <= 8 for a, b in zip(at(x, y), wc))))
elif cmd == "where":
    # The centre of the first widget named NAME, to click.
    desktop = Atspi.get_desktop(0)
    stack = [desktop.get_child_at_index(i) for i in range(desktop.get_child_count())]
    stack = [a for a in stack if a is not None and a.get_name() == "etch"]
    while stack:
        n = stack.pop(0)
        if n is None:
            continue
        if n.get_name() == sys.argv[2] and n.get_component_iface() is not None:
            e = n.get_component_iface().get_extents(Atspi.CoordType.WINDOW)
            if e.width > 0:
                print(e.x + e.width // 2, e.y + e.height // 2)
                sys.exit(0)
        stack.extend(n.get_child_at_index(i) for i in range(n.get_child_count()))
    sys.exit("nothing named " + sys.argv[2])
elif cmd == "opened":
    # The opened W x H image, centred at 100%: its left half, top right, and bottom right
    # (4 px in from each edge).
    at = pixels(sys.argv[2])
    W, H = int(sys.argv[3]), int(sys.argv[4])
    cx, cy, cw, ch = canvas()
    l, t = cx + (cw - W) // 2, cy + (ch - H) // 2
    def region(x0, x1, y0, y1):
        return [at(x, y) for y in range(t + y0 + 4, t + y1 - 4) for x in range(l + x0 + 4, l + x1 - 4)]
    every = lambda n, ps: "all" if n == len(ps) else f"{n}/{len(ps)}"
    left = region(0, W // 2, 0, H)
    red = sum(1 for r, g, b in left if r >= 240 and g <= 15 and b <= 15)
    clear = region(W // 2, W, 0, H // 2)
    light = sum(1 for p in clear if min(p) >= 245)
    grey = sum(1 for p in clear if all(195 <= c <= 213 for c in p))
    # The checkerboard: neutral greys from its grey to white (the edges of its squares may
    # fall between pixels and blend).
    checker = sum(1 for p in clear if min(p) >= 195 and max(p) - min(p) <= 3)
    green = sum(1 for r, g, b in clear if g > 200 and r < 100)
    half = region(W // 2, W, H // 2, H)
    # Blue at alpha 128 over white is (127, 127, 255), over the grey (102, 102, 230).
    blended = sum(1 for r, g, b in half if b >= 220 and 90 <= r <= 140 and abs(r - g) <= 6)
    print(f"red {every(red, left)}")
    print(f"checker {every(checker, clear)} light {min(light, 1)} grey {min(grey, 1)} green {green}")
    print(f"blended {every(blended, half)}")
elif cmd == "file":
    # An image file as saved: its format (by content), size, and pixels darker than 60.
    fmt, w, h = GdkPixbuf.Pixbuf.get_file_info(sys.argv[2])
    at = pixels(sys.argv[2])
    black = sum(1 for y in range(h) for x in range(w) if max(at(x, y)) < 60)
    print(f"{fmt.get_name()} {w}x{h} black {black}")
elif cmd == "profile":
    # Down the screen column X from Y0 to Y1: black pixels, and greys between black and
    # white (a soft edge).
    at = pixels(sys.argv[2])
    x, y0, y1 = map(int, sys.argv[3:6])
    col = [at(x, y) for y in range(y0, y1)]
    black = sum(1 for p in col if max(p) < 60)
    grey = sum(1 for p in col if max(p) >= 60 and min(p) < 245 and max(p) - min(p) <= 3)
    print(f"black {black} soft {grey}")
PY
# The images sections 11 and 12 open.
cat > /tmp/mk.py <<"PY"
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf, GLib

def make(path, w, h, colour, kind, alpha, keys=(), values=()):
    data = bytes(c for y in range(h) for x in range(w) for c in colour(x, y))
    n = 4 if alpha else 3
    pb = GdkPixbuf.Pixbuf.new_from_bytes(GLib.Bytes.new(data), GdkPixbuf.Colorspace.RGB, alpha, 8, w, h, w * n)
    pb.savev(path, kind, list(keys), list(values))

def t(x, y):
    if x < 200:
        return (255, 0, 0, 255)
    return (0, 255, 0, 0) if y < 150 else (0, 0, 255, 128)

make("/tmp/t.png", 400, 300, t, "png", True)
make("/tmp/s.png", 400, 300, lambda x, y: (255, 255, 255, 255), "png", True)
make("/tmp/c.jpg", 320, 200, lambda x, y: (x * 255 // 319, y * 255 // 199, 128), "jpeg", False, ["quality"], ["90"])
PY
session() {
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway --unsupported-gpu -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
  for i in $(seq 50); do ls /tmp/rt/wayland-[0-9] >/dev/null 2>&1 && break; sleep 0.1; done
  export WAYLAND_DISPLAY=$(basename $(ls /tmp/rt/wayland-[0-9] | head -1))
  export SWAYSOCK=$(ls /tmp/rt/sway-ipc.*.sock | head -1)
  n=0
  shot() { n=$((n + 1)); grim /tmp/shot.png 2>/dev/null; cp /tmp/shot.png /shots/$n-$1.png 2>/dev/null || true; }
  keys() { wtype -s 300 "$@" -s 400; }
  shown() { python3 /tmp/a11y.py text 2>&1 | sed "s/^/${1:-shown}: /"; }
  # The status bar: the pointer position ("X Y"), and the zoom.
  pos() { python3 /tmp/a11y.py text | sed -n "s/^x: \([0-9]*\)  y: \([0-9]*\)$/\1 \2/p" | head -1; }
  zoom() { python3 /tmp/a11y.py text | grep -E "^[0-9.]+%$" | head -1; }
  point() { python3 /scripts/dnd_helpers.py wheel $1 $2 0; sleep 0.3; }
  # Waits on the key holder alone: a bare wait would wait for sway and etch too.
  ctrl_wheel() { wtype -M ctrl -s 1500 -m ctrl & k=$!; python3 /scripts/dnd_helpers.py wheel $1 $2 $3; wait $k; sleep 0.3; }
  px() { python3 /tmp/px.py "$@"; }
  same() { [ -n "$1" ] && [ "$1" = "$2" ] && echo yes || echo "no ($1 then $2)"; }
  near() { # X1 Y1 X2 Y2 DX DY: whether (X2, Y2) is (X1 + DX, Y1 + DY), within a pixel
    python3 -c "import sys; a = list(map(float, sys.argv[1:])); print(\"yes\" if abs(a[2] - a[0] - a[4]) <= 1 and abs(a[3] - a[1] - a[5]) <= 1 else f\"no ({a[0]:.0f},{a[1]:.0f} to {a[2]:.0f},{a[3]:.0f})\")" "$@"
  }

  echo "== 1 start"
  etch >>/tmp/app.log 2>&1 & sleep 3
  shown start
  shot start; px image /tmp/shot.png | sed "s/^/start: /"
  echo "start ink: $(px ink /tmp/shot.png)"
  keys -M ctrl 0 -m ctrl
  echo "actual: $(zoom)"

  echo "== 2 draw"
  read mx my <<<"$(px centre)"
  ax=$((mx - 150)); ay=$((my - 80)); bx=$((mx + 150)); by=$((my + 80))
  python3 /scripts/dnd_helpers.py drag $ax $ay $bx $by; sleep 0.5
  shot drawn
  echo "line: $(px line /tmp/shot.png $ax $ay $bx $by)"
  drawn=$(px black /tmp/shot.png); echo "black: $drawn"

  echo "== 3 undo"
  keys -M ctrl z -m ctrl; shot undone; echo "undone: $(px black /tmp/shot.png)"
  keys -M ctrl -M shift z -m shift -m ctrl; shot redone; echo "redone: $(same $drawn $(px black /tmp/shot.png))"
  keys -M ctrl z -m ctrl; keys -M ctrl y -m ctrl; shot redone-y; echo "redone y: $(same $drawn $(px black /tmp/shot.png))"

  echo "== 4 resize"
  swaymsg "[app_id=dev.cronkite.Etch] floating enable, resize set 900 560, move position 0 0" >/dev/null; sleep 1
  shot resized; echo "resized: $(same $drawn $(px black /tmp/shot.png))"
  echo "resized canvas: $(px canvas | cut -d" " -f3-)"
  swaymsg "[app_id=dev.cronkite.Etch] floating disable" >/dev/null; sleep 1
  shot tiled; echo "tiled: $(same $drawn $(px black /tmp/shot.png))"
  echo "zoom: $(zoom)"

  echo "== 5 zoom"
  read mx my <<<"$(px centre)"
  # A pointer that stays, so the status keeps showing the position between the helpers.
  python3 /scripts/dnd_helpers.py hold $mx $my 120 & held=$!; sleep 1
  point $mx $my; before=""
  for i in $(seq 10); do before=$(pos); [ -n "$before" ] && break; sleep 0.3; done
  echo "under: $before"
  for z in 150 200 300 400 600 800; do
    ctrl_wheel $mx $my -1
    echo "zoom $(zoom): $(same "$before" "$(pos)")"
  done
  # On the stroke, 200 px left of the pointer: clear of the tip ring around the pointer.
  shot zoomed; echo "sharp: $(px sharp /tmp/shot.png $((mx - 200)) $((my - 107)) 40)"

  echo "== 6 pan"
  point $mx $my; read x0 y0 <<<"$(pos)"
  python3 /scripts/dnd_helpers.py wheel $mx $my 1; sleep 0.3
  read x1 y1 <<<"$(pos)"; echo "wheel: $(near $x0 $y0 $x1 $y1 0 6)"
  wtype -M shift -s 1500 -m shift & k=$!; python3 /scripts/dnd_helpers.py wheel $mx $my 1; wait $k; sleep 0.3
  read x2 y2 <<<"$(pos)"; echo "shift wheel: $(near $x1 $y1 $x2 $y2 6 0)"
  nx=$((mx - 160)); ny=$((my - 96))
  python3 /scripts/dnd_helpers.py drag $mx $my $nx $ny 0.5 middle; sleep 0.3
  read x3 y3 <<<"$(pos)"; echo "middle: $(near $x2 $y2 $x3 $y3 0 0)"
  echo "middle zoom: $(zoom)"
  # The leading pause lets the app take the keymap wtype sends before the press.
  wtype -s 300 -P space -s 6000 -p space & k=$!; sleep 0.8
  python3 /scripts/dnd_helpers.py drag $nx $ny $mx $my; wait $k; sleep 0.3
  read x4 y4 <<<"$(pos)"; echo "space: $(near $x3 $y3 $x4 $y4 0 0)"
  keys -M ctrl 9 -m ctrl; shot fitted; echo "fitted ink: $(px ink /tmp/shot.png)"
  keys -M ctrl z -m ctrl; shot fitted-undone; echo "fitted undone ink: $(px ink /tmp/shot.png)"
  kill $held

  echo "== 7 new"
  keys -M ctrl n -m ctrl; sleep 1
  shown dialog | grep -E "New Image|Width|Height"
  keys -M ctrl a -m ctrl; keys 640; keys -k Tab; keys -M ctrl a -m ctrl; keys 480; keys -k Return; sleep 2
  shown new
  keys -M ctrl n -m ctrl; sleep 1
  shown again | grep -E "New Image|^again: (640|480)$"
  keys -k Escape; sleep 1
  shown closed | grep "New Image" || echo "closed: no dialog"

  echo "== 8 bench"
  pkill -x etch; sleep 1
  etch --bench-stroke 120 2>>/tmp/app.log | sed "s/^/bench: /"

  echo "== 9 eraser"
  etch >>/tmp/app.log 2>&1 & sleep 3
  keys -M ctrl 0 -m ctrl
  tool() { python3 /tmp/a11y.py state $1; }
  echo "start: pencil $(tool Pencil) eraser $(tool Eraser)"
  read mx my <<<"$(px centre)"
  ax=$((mx - 150)); ay=$((my - 80)); bx=$((mx + 150)); by=$((my + 80))
  python3 /scripts/dnd_helpers.py drag $ax $ay $bx $by; sleep 0.5
  shot pencilled; drawn=$(px black /tmp/shot.png); echo "black: $drawn"
  python3 /tmp/a11y.py press Eraser; sleep 0.5
  echo "pressed: pencil $(tool Pencil) eraser $(tool Eraser)"
  python3 /scripts/dnd_helpers.py drag $ax $ay $bx $by; sleep 0.5
  shot erased; echo "erased: black $(px black /tmp/shot.png) grey $(px checker /tmp/shot.png)"
  keys -M ctrl z -m ctrl; shot unerased
  echo "unerased: $(same $drawn $(px black /tmp/shot.png)) grey $(px checker /tmp/shot.png)"
  keys p; echo "key p: pencil $(tool Pencil) eraser $(tool Eraser)"
  keys e; echo "key e: pencil $(tool Pencil) eraser $(tool Eraser)"
  pkill -x etch; sleep 1

  echo "== 10 colours"
  etch >>/tmp/app.log 2>&1 & sleep 3
  keys -M ctrl 0 -m ctrl
  colours() { python3 /tmp/a11y.py text | grep "^Foreground" | sed "s/^/$1: /"; }
  colours start
  python3 /tmp/a11y.py press Red; sleep 0.5; colours red
  read x y <<<"$(px where Indigo)"; python3 /scripts/dnd_helpers.py click $x $y 1 right; colours indigo
  # Alt+S, the mnemonic of the Size label, puts the keys in the size field.
  keys -M alt s -m alt
  keys -M ctrl a -m ctrl; keys 7; keys -k Return
  echo "size: $(python3 /tmp/a11y.py text | grep -A1 "^Size$" | tail -1)"
  read mx my <<<"$(px centre)"
  python3 /scripts/dnd_helpers.py drag $((mx - 150)) $my $((mx + 150)) $my; sleep 0.5
  shot red; echo "red column: $(px column /tmp/shot.png $mx ED1C24)"
  keys x; colours swapped
  python3 /scripts/dnd_helpers.py drag $((mx - 150)) $((my + 60)) $((mx + 150)) $((my + 60)); sleep 0.5
  shot indigo; echo "indigo column: $(px column /tmp/shot.png $mx 3F48CC)"
  echo "red still: $(px column /tmp/shot.png $mx ED1C24)"
  keys e; echo "eraser size: $(python3 /tmp/a11y.py text | grep -A1 "^Size$" | tail -1)"
  keys p; echo "pencil size: $(python3 /tmp/a11y.py text | grep -A1 "^Size$" | tail -1)"
  pkill -x etch; sleep 1

  echo "== 11 open"
  python3 /tmp/mk.py
  etch /tmp/t.png >>/tmp/open.log 2>&1 & sleep 3
  shown opened | grep -E "^opened: (t\.png|400×300|[0-9.]+%)$"
  shot opened; px opened /tmp/shot.png 400 300 | sed "s/^/opened: /"
  pkill -x etch; sleep 1
  etch >>/tmp/app.log 2>&1 & sleep 3
  keys -M ctrl o -m ctrl; sleep 1.5
  shown chooser | grep -E "^chooser: (Open|Images \(PNG, JPEG, BMP\))$"
  keys -M ctrl l -m ctrl; keys /tmp/c.jpg; keys -k Return; sleep 3
  shot ctrl-o; shown ctrl-o | grep -E "^ctrl-o: (c\.jpg|320×200|Untitled)$"
  pkill -x etch; sleep 1
  head -c 200 /tmp/t.png > /tmp/bad.png
  etch /tmp/bad.png >>/tmp/open.log 2>&1 & sleep 3
  shot damaged; shown damaged | grep -E "Cannot open|damaged"
  keys -k Return; sleep 1.5
  pgrep -x etch >/dev/null && echo "after alert: running" || echo "after alert: exited"
  printf "shopping: eggs, milk\n" > /tmp/notes.png
  etch /tmp/notes.png >>/tmp/open.log 2>&1 & sleep 3
  shown unsupported | grep -E "Cannot open|image Etch can open"
  pkill -x etch; sleep 1
  grep -E "^etch: cannot open|panicked|CRITICAL" /tmp/open.log | sed "s/^/log: /"

  echo "== 12 save"
  title() { python3 /tmp/a11y.py text | grep -E "^(• )?(s\.png|s2\.jpg|Untitled)$" | head -1; }
  img() { px file "$1" | sed "s/^/$2: /"; }
  black() { px file "$1" | sed "s/.* black //"; }
  more() { [ "$2" -gt "$1" ] 2>/dev/null && echo yes || echo "no ($1 then $2)"; }
  stroke() { # DY: a sideways stroke DY px below the image centre
    read mx my <<<"$(px centre)"
    python3 /scripts/dnd_helpers.py drag $((mx - 100)) $((my + $1)) $((mx + 100)) $((my + $1)); sleep 0.5
  }
  img /tmp/s.png before
  etch /tmp/s.png >>/tmp/save.log 2>&1 & sleep 3
  echo "opened: $(title)"
  stroke -60; echo "stroked: $(title)"
  keys -M ctrl s -m ctrl; sleep 2
  echo "saved: $(title)"; img /tmp/s.png saved; b1=$(black /tmp/s.png)
  stroke 0; echo "again: $(title)"
  keys -M ctrl z -m ctrl; echo "undone to saved: $(title)"
  keys -M ctrl y -m ctrl; echo "redone: $(title)"
  saved=$(md5sum </tmp/s.png)
  keys -M ctrl w -m ctrl; sleep 1
  shot asked; shown asked | grep -E "Save changes|Changes not saved|^asked: (Cancel|Discard|Save)$"
  python3 /tmp/a11y.py press Cancel; sleep 1
  pgrep -x etch >/dev/null && echo "cancel: running $(title)" || echo "cancel: exited"
  keys -M ctrl w -m ctrl; sleep 1
  python3 /tmp/a11y.py press Discard; sleep 1.5
  pgrep -x etch >/dev/null && echo "discard: running" || echo "discard: exited"
  [ "$(md5sum </tmp/s.png)" = "$saved" ] && echo "discard: file unchanged" || echo "discard: file changed"
  etch /tmp/s.png >>/tmp/save.log 2>&1 & sleep 3
  stroke 60; echo "edited: $(title)"
  keys -M ctrl w -m ctrl; sleep 1
  python3 /tmp/a11y.py press Save; sleep 2.5
  pgrep -x etch >/dev/null && echo "save: running" || echo "save: exited"
  img /tmp/s.png closed; echo "closed more: $(more "$b1" "$(black /tmp/s.png)")"
  etch /tmp/s.png >>/tmp/save.log 2>&1 & sleep 3
  keys -M ctrl -M shift s -m shift -m ctrl; sleep 1.5
  shown chooser | grep -E "^chooser: (Save As|PNG image|JPEG image|BMP image)$"
  keys -M ctrl a -m ctrl; keys /tmp/s2.jpg; keys -k Return; sleep 3
  echo "save as: $(title)"; img /tmp/s2.jpg jpeg; b2=$(black /tmp/s2.jpg)
  # The eraser takes the line at -60 (the pencil drew it 1 px wide) to transparency.
  keys e; stroke -60; echo "erased: $(title)"
  keys -M ctrl s -m ctrl; sleep 1.5
  shot transparency; shown flatten | grep -E "without transparency|become white"
  python3 /tmp/a11y.py press Save; sleep 2
  echo "flattened: $(title)"; img /tmp/s2.jpg flattened
  echo "flattened less: $(more "$(black /tmp/s2.jpg)" "$b2")"
  # Save As from the JPEG offers its own format first.
  keys -M ctrl -M shift s -m shift -m ctrl; sleep 1.5
  shown jpeg-chooser | grep -E "^jpeg-chooser: (Save As|s2\.jpg|[A-Z]+ image)$"
  keys -k Escape; sleep 1
  pkill -x etch; sleep 1
  etch >>/tmp/save.log 2>&1 & sleep 3
  stroke 0; keys -M ctrl s -m ctrl; sleep 1.5
  shown untitled | grep -E "^untitled: (Save As|Untitled\.png)$"
  pkill -x etch; sleep 1
  grep -E "saved|cannot save|panicked|CRITICAL" /tmp/save.log | sed "s/^/log: /"

  echo "== 13 brush"
  etch >>/tmp/app.log 2>&1 & sleep 3
  keys -M ctrl 0 -m ctrl
  tool() { python3 /tmp/a11y.py state $1 2>&1 | sed "s/^no toggle button.*/hidden/"; }
  size() { python3 /tmp/a11y.py text | grep -A1 "^Size$" | tail -1; }
  echo "start: edge $(tool Hard) $(tool Soft)"
  keys b; echo "key b: brush $(tool Brush) pencil $(tool Pencil)"
  echo "edge: hard $(tool Hard) soft $(tool Soft)"
  echo "brush size: $(size)"
  keys -M alt s -m alt; keys -M ctrl a -m ctrl; keys 15; keys -k Return
  echo "size: $(size)"
  read mx my <<<"$(px centre)"
  python3 /scripts/dnd_helpers.py drag $((mx - 150)) $((my - 60)) $((mx + 150)) $((my - 60)); sleep 0.5
  shot soft; soft=$(px profile /tmp/shot.png $mx $((my - 80)) $((my - 40))); echo "soft: $soft"
  python3 /tmp/a11y.py press Hard; sleep 0.5
  echo "pressed: hard $(tool Hard) soft $(tool Soft)"
  python3 /scripts/dnd_helpers.py drag $((mx - 150)) $((my + 60)) $((mx + 150)) $((my + 60)); sleep 0.5
  shot hard; echo "hard: $(px profile /tmp/shot.png $mx $((my + 40)) $((my + 80)))"
  keys -M ctrl z -m ctrl; shot hard-undone
  echo "undone hard: $(px profile /tmp/shot.png $mx $((my + 40)) $((my + 80)))"
  echo "soft kept: $(same "$soft" "$(px profile /tmp/shot.png $mx $((my - 80)) $((my - 40)))")"
  keys -M ctrl z -m ctrl; shot soft-undone
  echo "undone soft: $(px profile /tmp/shot.png $mx $((my - 80)) $((my - 40)))"
  keys -M ctrl y -m ctrl; shot soft-redone
  echo "redone soft: $(same "$soft" "$(px profile /tmp/shot.png $mx $((my - 80)) $((my - 40)))")"
  keys p; echo "key p: edge $(tool Hard) $(tool Soft)"
  pkill -x etch; sleep 1
  swaymsg exit >/dev/null 2>&1 || true
}
dbus-run-session -- bash -c "$(declare -f session); session" 2>/tmp/dbus.log
echo "== app log"; grep -E "^etch:|panicked|^thread|CRITICAL" -A2 /tmp/app.log | head -30
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
check "1: etch opens a window titled Untitled" "1 start" "^start: Untitled$"
check "1: ... with a 1024×768 image" "1 start" "^start: 1024×768$"
check "1: ... white and 4:3 on the canvas" "1 start" "^start: image [0-9]+x[0-9]+: 4:3$"
check "1: ... with no ink on it" "1 start" "^start ink: 0$"
check "1: Ctrl+0 shows it at 100%" "1 start" "^actual: 100%$"
check "2: a drag draws black pixels all along its path" "2 draw" "^line: 41/41$"
absent "2: ... (some black at all)" "2 draw" "^black: 0$"
check "3: Ctrl+Z removes the stroke" "3 undo" "^undone: 0$"
check "3: Ctrl+Shift+Z restores it" "3 undo" "^redone: yes$"
check "3: Ctrl+Y redoes too" "3 undo" "^redone y: yes$"
check "4: resizing the window keeps the drawing" "4 resize" "^resized: yes$"
check "4: ... in the smaller window (900 px less the tool column)" "4 resize" "^resized canvas: 8[0-9]{2} "
check "4: tiling it again keeps it too" "4 resize" "^tiled: yes$"
check "4: ... still at 100%" "4 resize" "^zoom: 100%$"
for z in 150 200 300 400 600 800; do
  check "5: Ctrl+wheel zooms to $z% keeping the pixel under the pointer" "5 zoom" "^zoom $z%: yes$"
done
check "5: at 800% pixels are sharp squares" "5 zoom" "^sharp: black [1-9][0-9]* between 0$"
check "6: the wheel pans down 6 image px at 800%" "6 pan" "^wheel: yes$"
check "6: Shift+wheel pans sideways" "6 pan" "^shift wheel: yes$"
check "6: a middle-button drag pans with the pointer" "6 pan" "^middle: yes$"
check "6: ... without zooming" "6 pan" "^middle zoom: 800%$"
check "6: a drag with Space held pans with the pointer" "6 pan" "^space: yes$"
absent "6: Fit to Window shows the stroke" "6 pan" "^fitted ink: 0$"
check "6: ... and one undo takes all ink: Space's drag drew nothing" "6 pan" "^fitted undone ink: 0$"
check "7: Ctrl+N asks for the new image's size" "7 new" "^dialog: New Image$"
check "7: ... and opens it in a new window" "7 new" "^new: Untitled 2$"
check "7: ... 640×480" "7 new" "^new: 640×480$"
check "7: the next New Image offers 640" "7 new" "^again: 640$"
check "7: ... by 480" "7 new" "^again: 480$"
check "7: Escape closes the dialog" "7 new" "^closed: no dialog$"
# ETCH-3: the eraser and the tool column.
check "9: the tool column starts on Pencil" "9 eraser" "^start: pencil on eraser off$"
absent "9: the pencil draws" "9 eraser" "^black: 0$"
check "9: a click on Eraser picks it" "9 eraser" "^pressed: pencil off eraser on$"
check "9: erasing the line leaves no black, the checkerboard showing through" "9 eraser" "^erased: black 0 grey [1-9][0-9]*$"
check "9: Ctrl+Z gives the line back over white" "9 eraser" "^unerased: yes grey 0$"
check "9: P picks the pencil" "9 eraser" "^key p: pencil on eraser off$"
check "9: E picks the eraser" "9 eraser" "^key e: pencil off eraser on$"
# ETCH-4: colours and sizes.
check "10: the colour row starts black on white" "10 colours" "^start: Foreground #000000, background #FFFFFF$"
check "10: a click on Red makes it the foreground" "10 colours" "^red: Foreground #ED1C24, background #FFFFFF$"
check "10: a right-click on Indigo makes it the background" "10 colours" "^indigo: Foreground #ED1C24, background #3F48CC$"
check "10: the size takes 7" "10 colours" "^size: 7$"
check "10: the pencil draws a red line 7 px thick" "10 colours" "^red column: 7$"
check "10: X swaps the colours" "10 colours" "^swapped: Foreground #3F48CC, background #ED1C24$"
check "10: ... and the next line is indigo, 7 px thick" "10 colours" "^indigo column: 7$"
check "10: ... beside the red one" "10 colours" "^red still: 7$"
check "10: the eraser keeps its own size" "10 colours" "^eraser size: 9$"
check "10: ... and the pencil its" "10 colours" "^pencil size: 7$"
# ETCH-6: opening images.
check "11: etch t.png opens it in a window titled t.png" "11 open" "^opened: t\.png$"
check "11: ... the status saying 400×300" "11 open" "^opened: 400×300$"
check "11: ... at 100%" "11 open" "^opened: 100%$"
check "11: ... its opaque half red on screen" "11 open" "^opened: red all$"
check "11: ... its transparent quarter the checkerboard, no colour" "11 open" "^opened: checker all light 1 grey 1 green 0$"
check "11: ... its half-transparent quarter blue blended over it" "11 open" "^opened: blended all$"
check "11: Ctrl+O shows the file dialog" "11 open" "^chooser: Open$"
check "11: ... filtered to images" "11 open" "^chooser: Images \(PNG, JPEG, BMP\)$"
check "11: ... and opens the JPEG typed in a window titled c.jpg" "11 open" "^ctrl-o: c\.jpg$"
check "11: ... 320×200" "11 open" "^ctrl-o: 320×200$"
check "11: ... beside the Untitled window" "11 open" "^ctrl-o: Untitled$"
check "11: a damaged file alerts naming it" "11 open" "^damaged: Cannot open “bad\.png”$"
check "11: ... and why" "11 open" "^damaged: It's a damaged or incomplete PNG image"
check "11: ... and closing the alert closes its window" "11 open" "^after alert: exited$"
check "11: a text file named .png alerts naming it" "11 open" "^unsupported: Cannot open “notes\.png”$"
check "11: ... and why" "11 open" "^unsupported: It isn't an image Etch can open: PNG, JPEG, or BMP\.$"
check "11: the failures are logged with the reason" "11 open" "^log: etch: cannot open /tmp/bad\.png: It's a damaged"
absent "11: opening logs no panics" "11 open" "^log: .*(panicked|CRITICAL)"
# ETCH-7: saving.
check "12: the file starts white" "12 save" "^before: png 400x300 black 0$"
check "12: the opened image's title has no mark" "12 save" "^opened: s\.png$"
check "12: a stroke marks the title unsaved" "12 save" "^stroked: • s\.png$"
check "12: Ctrl+S clears the mark" "12 save" "^saved: s\.png$"
check "12: ... and the file holds the stroke" "12 save" "^saved: png 400x300 black [1-9][0-9]*$"
check "12: another stroke marks it again" "12 save" "^again: • s\.png$"
check "12: undoing back to the saved image clears it" "12 save" "^undone to saved: s\.png$"
check "12: ... and redoing marks it" "12 save" "^redone: • s\.png$"
check "12: closing with changes asks to save them" "12 save" "^asked: Save changes to “s\.png” before closing\?$"
check "12: ... with Cancel" "12 save" "^asked: Cancel$"
check "12: ... Discard" "12 save" "^asked: Discard$"
check "12: ... and Save" "12 save" "^asked: Save$"
check "12: Cancel keeps the window, unsaved" "12 save" "^cancel: running • s\.png$"
check "12: Discard closes it" "12 save" "^discard: exited$"
check "12: ... leaving the file as it was" "12 save" "^discard: file unchanged$"
check "12: an edit after reopening marks it" "12 save" "^edited: • s\.png$"
check "12: Save in the closing question closes the window" "12 save" "^save: exited$"
check "12: ... after saving the edit: a PNG with more black" "12 save" "^closed more: yes$"
check "12: Ctrl+Shift+S shows the Save As dialog" "12 save" "^chooser: Save As$"
check "12: ... the PNG's own format first (the PNG image filter)" "12 save" "^chooser: PNG image$"
check "12: Save As to s2.jpg makes the window s2.jpg" "12 save" "^save as: s2\.jpg$"
check "12: ... and writes a JPEG" "12 save" "^jpeg: jpeg 400x300 black [1-9][0-9]*$"
check "12: saving transparency as JPEG asks first" "12 save" "^flatten: Save “s2\.jpg” without transparency\?$"
check "12: ... saying it becomes white" "12 save" "^flatten: JPEG images can.t be transparent"
check "12: ... and saves when told to" "12 save" "^flattened: s2\.jpg$"
check "12: ... a JPEG still" "12 save" "^flattened: jpeg 400x300 black [0-9]+$"
check "12: ... with the erased line white: less black" "12 save" "^flattened less: yes$"
check "12: Save As from the JPEG offers the JPEG image filter first" "12 save" "^jpeg-chooser: JPEG image$"
check "12: Ctrl+S on an Untitled image asks where" "12 save" "^untitled: Save As$"
check "12: ... offering Untitled.png" "12 save" "^untitled: Untitled\.png$"
check "12: saves are logged" "12 save" "^log: .*saved /tmp/s\.png as PNG: copied in"
absent "12: saving logs no failures or panics" "12 save" "^log: .*(cannot save|panicked|CRITICAL)"
# ETCH-8: the brush.
check "13: the brush's edge is hidden while the pencil is the tool" "13 brush" "^start: edge hidden hidden$"
check "13: B picks the brush" "13 brush" "^key b: brush on pencil off$"
check "13: ... showing its edge, Soft on" "13 brush" "^edge: hard off soft on$"
check "13: ... at size 8" "13 brush" "^brush size: 8$"
check "13: the size takes 15" "13 brush" "^size: 15$"
check "13: a soft line has a black core fading through grey" "13 brush" "^soft: black [3-7] soft ([6-9]|1[0-2])$"
check "13: Hard turns the hard edge on" "13 brush" "^pressed: hard on soft off$"
check "13: a hard line is 15 px black with no grey" "13 brush" "^hard: black 15 soft 0$"
check "13: Ctrl+Z takes back the hard line" "13 brush" "^undone hard: black 0 soft 0$"
check "13: ... leaving the soft one" "13 brush" "^soft kept: yes$"
check "13: another Ctrl+Z takes back the soft line" "13 brush" "^undone soft: black 0 soft 0$"
check "13: Ctrl+Y gives it back the same" "13 brush" "^redone soft: yes$"
check "13: the edge hides again with the pencil" "13 brush" "^key p: edge hidden hidden$"
absent "the app logs no errors or panics" "app log" "^etch:|panicked|CRITICAL"
bench=$(section "8 bench" | sed -n "s/^bench: //p")
if [ -z "$bench" ]; then echo "FAIL 8: --bench-stroke printed nothing"; fail=1; else
  read seg frame <<<"$(python3 -c "import json, sys; b = json.loads(sys.argv[1]); print(b[\"segment_ms\"][\"p99\"], b[\"frame_cpu_ms\"][\"p99\"])" "$bench")"
  if python3 -c "import sys; sys.exit(not float(sys.argv[1]) < 8)" "$seg"; then
    echo "ok   8: a segment on 7680x4320 takes $seg ms p99 (under 8); frames here $frame ms p99"
  else echo "FAIL 8: a segment on 7680x4320 takes $seg ms p99, not under 8"; fail=1; fi
fi
if [ $fail = 0 ]; then echo "ETCH OK"; else
  echo "ETCH CHECK FAILED"; [ -n "${VERBOSE:-}" ] || echo "(VERBOSE=1 for the transcript)"
  exit 1
fi
