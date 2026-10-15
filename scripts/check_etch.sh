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
# ETCH-9 acceptance: the fill bucket.
#  14. a box drawn with the pencil (size 3), Red the foreground and Indigo the background:
#      F picks the fill (the tool column shows it on) and hides the size; a click inside
#      the box fills it red to its edges, the white outside untouched; a right-click
#      outside fills the outside indigo, the inside staying red. Ctrl+Z takes back the
#      outside, another Ctrl+Z the inside, and Ctrl+Y fills the inside again.
# ETCH-10 acceptance: the colour picker.
#  15. `etch t.png` (section 11's image), the brush the tool: I picks the colour picker
#      (the tool column shows it on) and hides the size; a click on the red half makes the
#      foreground #FF0000 (the colour row's accessible description) and goes back to the
#      brush; I and a click on the half-transparent blue make it #0000FF80, alpha
#      included; I and a right-click on the transparent green make the background
#      #00FF0000. Picking changes no pixel: the title stays unmarked.
# ETCH-11 acceptance: line, rectangle, and ellipse with previews.
#  16. the shapes' style hides while the pencil is the tool; R picks the rectangle (the tool
#      column shows it on) with Outline, Filled, and Both beside the size, Outline on, size
#      1. At size 5, a drag held still before its release shows the rectangle on the
#      canvas (its top band 5 px black, the inside white) while the title stays
#      "Untitled": the image is untouched. The release lands it pixel for pixel as
#      previewed (as many black pixels around it) and marks the title; one Ctrl+Z
#      removes it entirely (no ink) and clears the mark. O and Both, with Red the
#      background: the held drag previews a black-outlined red ellipse, unmarked, which
#      lands the same; one Ctrl+Z takes it all back. L hides the style; a held line drag
#      previews the line along the pointer's path, Escape before the release drops it,
#      and after the release there's no ink and no mark; a line drawn then lands, and
#      Ctrl+Z takes it back.
# ETCH-12 acceptance: text.
#  17. the font controls are hidden while the pencil is the tool; T picks text (the tool
#      column shows it on) with Bold and Italic off, size 24. A click on the canvas and
#      "Hello" typed (wtype) show dark text in the box while the title stays "Untitled"
#      (the canvas describes the text typed); " pebfilrostx" typed goes into the text
#      (described) and switches no tool (Text still on, Pencil and Eraser off) nor
#      swaps the colours. Enter puts it on the image: the title marked, ink there, no text
#      typed; one Ctrl+Z takes it all back (no ink, unmarked). Another click and "Gone"
#      typed, then Escape: no ink, unmarked, no text. A click, "Click" typed, and a click
#      elsewhere put it on (marked, ink, no new text there); Ctrl+Z takes it back. Then
#      "illicit fillip" typed: Bold makes it darker (more black pixels); in DejaVu Sans
#      Mono, picked from the font list (searched), it draws wider than in Sans, and typing
#      goes on into it ("!" added); Escape drops it. Through an input method on the seat
#      (scripts/ime_helpers.py): new text shows the composition "かな" (ink, nothing typed
#      yet), takes the commit "仮名", and Enter puts it on the image.
# ETCH-13 acceptance: New Image's background, and its size remembered.
#  18. Ctrl+N's dialog offers White (on), Transparent, and Background colour (described
#      #FFFFFF, the window's background); Alt+T picks Transparent, and 300x200 makes
#      Untitled 2: at 100% the checkerboard everywhere in it, light and grey squares and
#      nothing else. Saved (Save As, /tmp/clear.png) it reads back 300x200 with alpha, its
#      largest alpha 0. The settings file (~/.config/etch/settings) holds the size and
#      background; after quitting and starting etch again, Ctrl+N offers 300 by 200,
#      Transparent on.
# ETCH-14 acceptance: the fill's tolerance.
#  19. `etch n.png` (400x300: the left half white, the right half 249,249,249, six levels
#      off): Tolerance hides while the pencil is the tool and shows beside the fill (F), 0
#      at first. Red the foreground, a click on the left half at 0 fills it and leaves the
#      right half; Ctrl+Z, then at 5 (Alt+T, typed) the same; Ctrl+Z, then at 6 both halves
#      fill red. Switching to the pencil and back keeps 6.
# ETCH-15 acceptance: the rectangular selection.
#  20. `etch t.png` (section 11's image): S picks the selection (the tool column shows it
#      on), which has no size. A drag over the red half selects 100x80: the status says
#      "Selection: 100×80" and the outline shows along its top and left edges, black
#      dashes on white; the title stays unmarked. A drag inside it moves its pixels 200 px
#      right, into the transparent quarter: on screen the place they left shows the
#      checkerboard and the new place is red, outlined, while the title stays unmarked
#      (they float). Escape lands them and deselects (no selection in the status, the
#      title marked, the screen the same); one Ctrl+Z gives back the red and the
#      transparency and clears the mark, and Ctrl+Y moves them again. Shift+Right moves a
#      new selection's pixels 10 px (the strip they left transparent, the strip they
#      reached red), and choosing the pencil (P) lands them and deselects. Ctrl+A with
#      the pencil the tool picks the selection and selects the whole image (400×300);
#      Escape deselects. Delete clears a selection to transparency (the checkerboard),
#      marking the title, and Ctrl+Z gives the red back. While text is typed, Delete
#      deletes a character and Ctrl+A selects nothing.
# ETCH-16 acceptance: cut, copy, and paste through the system clipboard.
#  21. `etch t.png`: a 100x80 selection over red, transparent, and half-transparent blue
#      pixels, Ctrl+C: the clipboard offers image/png (wl-paste -l), and another GTK 4 app
#      (/tmp/clip.py, its own window) reads it as a 100x80 texture with the pixels exact,
#      straight alpha: red, transparent, blue at alpha 128. Ctrl+X clears it to
#      transparency, marking the title, and the other app reads the same image; one
#      Ctrl+Z gives the red back, unmarked. The other app puts a 60x40 image (left half
#      opaque blue, right half blue at alpha 128) on the clipboard; Ctrl+V in Etch picks
#      the selection and floats it at the image's top-left: the status says
#      "Selection: 60×40", the screen shows it blue, the title stays unmarked. Ctrl+Z
#      drops it (red, no selection); pasted again and dragged 100 px right and down, the
#      red shows again where it was and it's blue where it went, still unmarked; Escape
#      lands it, marking the title, and Select All and Copy give the other app the image
#      with it in place (opaque blue, blue at alpha 128, red beside). Ctrl+Z takes the
#      paste back (red there, unmarked) and Ctrl+Y lands it again. While text is typed
#      Ctrl+V pastes nothing.
# ETCH-17 acceptance: crop to selection.
#  22. `etch crop.png` (a copy of t.png): Ctrl+Shift+X without a selection does nothing
#      (400×300, unmarked). A selection of image pixels 150 to 250 across and 100 to 200
#      down (red, transparent, and half-transparent blue), then the Image menu by keyboard
#      (Alt+I, then C for Crop to Selection; GTK names no popover menu item to AT-SPI):
#      the status says 100×100 with no selection, the title is marked, and on screen the
#      100x100 image shows its left half red, its top right the checkerboard, its bottom
#      right blue blended over it.
#      Ctrl+S writes a 100x100 PNG whose corners are the pixels exactly (ff0000ff,
#      transparent, 0000ff80). One Ctrl+Z gives back 400×300 and every region of t.png on
#      screen (the title marked: the file is the crop), and Ctrl+Y crops again. Pixels
#      moved and still floating land first: a 100x80 red selection dragged 200 px right
#      and cropped (Ctrl+Shift+X) leaves a 100×80 image all red; one Ctrl+Z gives back
#      400×300 with the moved pixels landed.
# ETCH-18 acceptance: resize image.
#  23. `etch resize.png` (a copy of t.png): Ctrl+R asks Resize Image with Width 400,
#      Height 300, Keep aspect ratio on, and Smooth on (Nearest neighbour the other
#      choice); Escape closes it, changing nothing. Again, Width 600 typed (Alt+W) sets
#      Height to 450 (linked); Alt+N picks Nearest neighbour and Enter in the width
#      resizes: the status says 600×450, the title is marked, and on screen the left half
#      is red, the top right the checkerboard, the bottom right blue blended. Saved, the
#      file is 600x450 with the halves meeting exactly at x 300 and y 225 (ff0000ff, the
#      transparent pixel still hiding its green 00ff0000, 0000ff80): nearest copies
#      pixels. One Ctrl+Z gives back 400×300 and every region. Smooth to 200x150: the
#      pixels either side of the red half's edge are red at alpha 223 and 32 (ff0000df,
#      ff000020), their colour unmixed with the transparent green beside it (the triangle
#      filter over 4 source pixels, weighted by alpha); Ctrl+Z gives back 400×300.
# ETCH-19 acceptance: resize canvas.
#  24. `etch canvas.png` (a copy of t.png): Ctrl+Shift+R asks Resize Canvas with Width
#      400, Height 300, the anchor's nine toggle buttons (named Top left to Bottom right)
#      with Centre on, and the new area Transparent (on) or Background colour. 600 by 400
#      with Top left pressed (AT-SPI) makes 600×400, marked: on screen the image at the
#      top left (its red half there) and the checkerboard right of it and below; saved,
#      the old pixels sit at 0,0 (ff0000ff to the blue corner 0000ff80 at 399,299) and
#      the new area is transparent (00000000). One Ctrl+Z gives back 400×300 and every
#      region. Indigo the background (right-click), 500 by 400 at Bottom right with
#      Background colour (Alt+G) puts the old top-left pixel at 100,100 and fills the new
#      area indigo (3f48ccff). Shrinking to 300 by 200 at the centre crops 50 px off each
#      side (the red half's edge at x 150, the blue corner at 299,199); Ctrl+Z gives back
#      400×300 with every region.
# ETCH-20 acceptance: rotate and flip.
#  25. `etch rot.png` (a copy of t.png): Ctrl+H turns it 90° right: the status says
#      300×400, the title is marked, and on screen the red half is on top and the
#      checkerboard bottom right; saved, its corners are the old ones turned (ff0000ff at
#      0,0, 0000ff80 at 0,399, the transparent 00ff0000 at 299,399). One Ctrl+Z gives back
#      400×300 and every region. The Image menu by keyboard (Alt+I, V) flips it
#      vertically, Ctrl+G turns it 90° left, Ctrl+J 180°, and Alt+I, H flips it
#      horizontally: each saved with its corners where the turn or mirror puts them, and
#      each undone by one Ctrl+Z (400×300). Four Ctrl+H give back 400×300, and saved, the
#      file's pixels are t.png's exactly; one Ctrl+Z after them leaves three turns
#      (300×400).
# ETCH-21 acceptance: drag and drop to open.
#  26. `etch t.png`; a drag source (dnd_helpers.py source, floating in a corner, as a file
#      manager offers files) drags one.png and two.jpg onto its window: each opens in a
#      window of its own (sway's tree shows one.png, t.png, and two.jpg). After a click in
#      the rightmost window, dropping t.png on it opens no second window and asks for
#      t.png's to come forward (sway marks it urgent); dropping notes.png (text) alerts
#      "Cannot open “notes.png”" with the reason, and closing the alert leaves the three
#      windows.
# ETCH-22 acceptance: recent files.
#  27. With a state folder of its own (XDG_STATE_HOME), `etch r1.png`, r2, r3, then r1
#      again, each run quit, list r1 r3 r2; an Untitled image saved as r4.png goes on top.
#      A new run's File → Open Recent (Alt+F, R) shows r4.png r1.png r3.png r2.png in
#      that order, each with its folder ("r4.png  (/tmp)", read through AT-SPI: a menu
#      item's text is in the label it's labelled by), and Enter opens r4.png. After r2.png
#      is deleted, choosing it alerts "Cannot open “r2.png”" ("The file doesn't exist.")
#      and it leaves the list and the menu.
# ETCH-23 acceptance: hex entry and the colour chooser.
#  28. Alt+H puts the keys in the hex field; "#3366CC" and Enter make the foreground
#      #3366CC, "#f80" #FF8800, "#3366CC80" #3366CC80. Every tool key and X typed there
#      ("pebfilrostx") is text: the pencil stays the tool; Enter marks the field invalid
#      and keeps the colour, as "#12345" does; a click on the canvas shows the colour
#      again. Custom… opens GTK's chooser titled Foreground Colour; Cancel changes
#      nothing; its editor (+) with "#20A040" and alpha 0.25 makes the foreground
#      #20A04040. A right-click on Custom… opens Background Colour: "#FFCC00" makes the
#      background #FFCC00.
# ETCH-24 acceptance: every tool on its key.
#  29. On the canvas P, B, E, F, I, T, R, L, S, and O each pick their tool (the tool column
#      shows it on), and X swaps the colours (and back). While text is typed every tool key
#      and X ("pebfilrostx") goes into the text: Text stays the tool and the colours stay.
#      So in the size field (Alt+S), the tolerance (Alt+T), the hex field (Alt+H), and the
#      font list's search: the tool stays, and no colour swaps; back on the canvas, B picks
#      the brush. In an open menu a tool key is the menu's: Alt+V, F fits the image to the
#      window (the zoom leaves 100%) and the eraser stays the tool.
# ETCH-25 acceptance: crash recovery.
#  30. `etch k.png` (a copy of t.png): a stroke of size 5 across its red half marks it; its
#      journal (in $XDG_STATE_HOME/etch/recovery) appears 5 s after the press, not before
#      (polled every 0.1 s from 0.5 s before the press). Killed (SIGKILL),
#      the journal stays and the file is byte for byte as it was. `etch k.png` asks
#      "Restore unsaved edits?" (Etch closed unexpectedly...) with Discard and Restore;
#      Restore shows the stroke (black on screen), the title "• k.png", the file still
#      unchanged, and no crashed journal left; one Ctrl+Z gives back the file's red,
#      unmarked. Ctrl+Y, killed again: `etch` alone opens k.png and asks; Discard leaves
#      the file's red and no journal. Edited, a journal; undoing back to the file removes
#      it; redone, a journal; Ctrl+W and Discard close etch with no journal and the file
#      unchanged. Edited and killed, then the file touched (its time changed): `etch
#      k.png` says "“k.png” changed after the crash" with Keep for Later and Discard;
#      Keep for Later shows the file and keeps the journal. An Untitled image drawn on
#      and killed: `etch` alone opens an Untitled 1024×768 image and asks (an untitled
#      image); Restore shows as much ink as was drawn, "• Untitled", and Ctrl+Z none.
# ETCH-26 acceptance: tiles meet without seams; the checkerboard snaps to pixels.
#  31. At output scale 1, then 2 (2560x1440), then 1.5 (1920x1080, fractional: the canvas
#      starts between device pixels), each with the default renderer (GL here, on
#      llvmpipe; the renderer logged by GSK_DEBUG=renderer) and with GSK_RENDERER=cairo:
#      `etch seam-1024x768.png` (opaque #3366CC, 4x3 tiles of 256 px) at 100%, Fit, 50%,
#      and 200%, and a 600x450 one at Fit (above 100%), show only that colour inside the
#      box its exact pixels span (2 px in, within 1 a channel): no line along a tile edge.
#      Transparent 400x300 and 401x301 images at 100% (one of them half a pixel off the
#      pixel grid each way) and 1501x1001 at Fit show the checkerboard crisp to their
#      edges: only its light and grey, over the image's whole rectangle (400x300 device
#      pixels at scale 1, 800x600 at 2, 600x450 at 1.5), the line around the image right
#      outside it.
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
pacman -Syu --noconfirm --needed sway wtype grim gtk4 ttf-dejavu wqy-zenhei dbus python-gobject \
  at-spi2-core python-pywayland wlr-protocols wayland pkgconf wl-clipboard >/dev/null 2>&1
mkdir -p /shots /tmp/rt /tmp/proto && chmod 700 /tmp/rt && touch /tmp/proto/__init__.py
python -m pywayland.scanner -i /usr/share/wayland/wayland.xml \
  /usr/share/wlr-protocols/unstable/wlr-virtual-pointer-unstable-v1.xml -o /tmp/proto/wlr_proto >/dev/null
python -m pywayland.scanner -i /usr/share/wayland/wayland.xml \
  /scripts/protocols/input-method-unstable-v2.xml -o /tmp/proto/im_proto >/dev/null 2>&1
export PYTHONPATH=/tmp/proto XDG_RUNTIME_DIR=/tmp/rt GDK_DEBUG=no-portals GDK_BACKEND=wayland
# The other app of section 21 and the drag source of section 26 float in a corner, clear
# of the image.
printf "output HEADLESS-1 resolution 1280x720 position 0 0\ndefault_border none\nfor_window [app_id=dev.cronkite.ClipCheck] floating enable, resize set 160 80, move position 1100 600\nfor_window [app_id=dev.brick.DragSource] floating enable, resize set 200 120, move position 1060 580\n" > /tmp/sway.conf
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
    # The centre of the first widget named NAME (or described so: a drop-down is named by
    # the item it shows), of role ROLE if given, to click.
    role = sys.argv[3] if len(sys.argv) > 3 else None
    desktop = Atspi.get_desktop(0)
    stack = [desktop.get_child_at_index(i) for i in range(desktop.get_child_count())]
    stack = [a for a in stack if a is not None and a.get_name() == "etch"]
    while stack:
        n = stack.pop(0)
        if n is None:
            continue
        named = sys.argv[2] in (n.get_name(), n.get_description())
        if named and (role is None or n.get_role_name() == role) and n.get_component_iface() is not None:
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
elif cmd == "count":
    # Pixels of colour HEX (within 8 a channel) in the screen rectangle X0 Y0 X1 Y1.
    at = pixels(sys.argv[2])
    x0, y0, x1, y1 = map(int, sys.argv[3:7])
    want = tuple(int(sys.argv[7][i:i + 2], 16) for i in (0, 2, 4))
    box = [at(x, y) for y in range(y0, y1) for x in range(x0, x1)]
    n = sum(1 for p in box if all(abs(a - b) <= 8 for a, b in zip(p, want)))
    print("all" if n == len(box) else f"{n}/{len(box)}")
elif cmd == "solid":
    # Section 31: the opaque image of colour HEX on screen: the box its exact pixels span
    # (from runs of 8), and the pixels inside it, 2 px in, more than 1 a channel off it.
    p = GdkPixbuf.Pixbuf.new_from_file(sys.argv[2])
    w, h, rs, nc, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
    want = bytes.fromhex(sys.argv[3])
    unit = want + (b"\xff" if nc == 4 else b"")
    rows = []
    for y in range(h):
        row = px[y * rs:y * rs + w * nc]
        i = row.find(unit * 8)
        while i >= 0 and i % nc:
            i = row.find(unit * 8, i + 1)
        j = row.rfind(unit * 8)
        while j >= 0 and j % nc:
            j = row.rfind(unit * 8, 0, j + 8 * nc - 1)
        if i >= 0:
            rows.append((y, i // nc, j // nc + 8))
    if not rows:
        sys.exit("no pixel of " + sys.argv[3])
    t, b = rows[0][0], rows[-1][0] + 1
    l, r = min(a for _, a, _ in rows), max(z for _, _, z in rows)
    off = 0
    for y in range(t + 2, b - 2):
        row = px[y * rs:y * rs + w * nc]
        if row[(l + 2) * nc:(r - 2) * nc] == unit * (r - l - 4):
            continue
        off += sum(1 for x in range(l + 2, r - 2) if max(abs(row[x * nc + k] - want[k]) for k in range(3)) > 1)
    print(f"{r - l}x{b - t} off {off}")
elif cmd == "crisp":
    # Section 31: the checkerboard of a transparent image: the box its squares span (its
    # grey runs, grown over whole rows and columns of its two colours), its pixels neither
    # the light (255) nor the grey (204), within 1 a channel, and whether the ring just
    # outside the box is the line around the image (darker than 180): squares end on pixel
    # edges, none blended with what is beside them.
    p = GdkPixbuf.Pixbuf.new_from_file(sys.argv[2])
    w, h, rs, nc, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
    at = lambda x, y: px[y * rs + x * nc:y * rs + x * nc + 3]
    ok = lambda p: max(p) - min(p) <= 1 and (min(p) >= 254 or 203 <= min(p) <= 205)
    grey = bytes([204, 204, 204] + [255] * (nc - 3)) * 6
    rows = []
    for y in range(h):
        row = px[y * rs:y * rs + w * nc]
        i = row.find(grey)
        while i >= 0 and i % nc:
            i = row.find(grey, i + 1)
        j = row.rfind(grey)
        while j >= 0 and j % nc:
            j = row.rfind(grey, 0, j + len(grey) - 1)
        if i >= 0:
            rows.append((y, i // nc, j // nc + 6))
    if not rows:
        sys.exit("no checkerboard")
    t, b = rows[0][0], rows[-1][0] + 1
    l, r = min(a for _, a, _ in rows), max(z for _, _, z in rows)
    while l > 0 and all(ok(at(l - 1, y)) for y in range(t, b)):
        l -= 1
    while r < w and all(ok(at(r, y)) for y in range(t, b)):
        r += 1
    while t > 0 and all(ok(at(x, t - 1)) for x in range(l, r)):
        t -= 1
    while b < h and all(ok(at(x, b)) for x in range(l, r)):
        b += 1
    off = sum(1 for y in range(t, b) for x in range(l, r) if not ok(at(x, y)))
    ring = [at(x, y) for x in range(l - 1, r + 1) for y in (t - 1, b)]
    ring += [at(x, y) for y in range(t, b) for x in (l - 1, r)]
    framed = "framed" if all(max(p) < 180 for p in ring) else "not framed"
    print(f"{r - l}x{b - t} off {off} {framed}")
elif cmd == "dark":
    # In the screen rectangle X0 Y0 X1 Y1: black pixels (darker than 60 in every channel),
    # and the width the pixels darker than 100 span (the ink of text, not the grey frame).
    at = pixels(sys.argv[2])
    x0, y0, x1, y1 = map(int, sys.argv[3:7])
    dark = [(x, y) for y in range(y0, y1) for x in range(x0, x1) if max(at(x, y)) < 100]
    black = sum(1 for x, y in dark if max(at(x, y)) < 60)
    width = max(x for x, _ in dark) - min(x for x, _ in dark) + 1 if dark else 0
    print(f"black {black} width {width}")
elif cmd == "clear":
    # The W x H image centred at 100%, 4 px in from its edges: the checkerboard (neutral
    # greys from its grey to white) everywhere, with both its light and grey squares.
    at = pixels(sys.argv[2])
    W, H = int(sys.argv[3]), int(sys.argv[4])
    cx, cy, cw, ch = canvas()
    l, t = cx + (cw - W) // 2, cy + (ch - H) // 2
    box = [at(x, y) for y in range(t + 4, t + H - 4) for x in range(l + 4, l + W - 4)]
    checker = sum(1 for p in box if min(p) >= 195 and max(p) - min(p) <= 3)
    light = sum(1 for p in box if min(p) >= 245)
    grey = sum(1 for p in box if all(195 <= c <= 213 for c in p))
    every = "all" if checker == len(box) else f"{checker}/{len(box)}"
    print(f"checker {every} light {min(light, 1)} grey {min(grey, 1)}")
elif cmd == "alpha":
    # A PNG as saved: its size, whether it has alpha, and its largest alpha.
    p = GdkPixbuf.Pixbuf.new_from_file(sys.argv[2])
    w, h, rs, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_pixels()
    top = max(px[y * rs + x * 4 + 3] for y in range(h) for x in range(w)) if p.get_has_alpha() else 255
    print(f"{w}x{h} alpha {p.get_has_alpha()} max {top}")
elif cmd == "checked":
    # The names of the radio and check buttons on.
    desktop = Atspi.get_desktop(0)
    stack = [desktop.get_child_at_index(i) for i in range(desktop.get_child_count())]
    stack = [a for a in stack if a is not None and a.get_name() == "etch"]
    on = []
    while stack:
        n = stack.pop(0)
        if n is None:
            continue
        n.clear_cache()
        if n.get_role() in (Atspi.Role.RADIO_BUTTON, Atspi.Role.CHECK_BOX) and n.get_state_set().contains(Atspi.StateType.CHECKED):
            on.append(n.get_name())
        stack.extend(n.get_child_at_index(i) for i in range(n.get_child_count()))
    print(", ".join(on) or "none")
elif cmd == "dashes":
    # The selection outline along the screen segment X0 Y0 X1 Y1 (a row or a column), on
    # the line within 2 px across it with the most black: black dashes on white, about
    # half each.
    at = pixels(sys.argv[2])
    x0, y0, x1, y1 = map(int, sys.argv[3:7])
    best = None
    for k in range(-2, 3):
        if y0 == y1:
            line = [at(x, y0 + k) for x in range(x0, x1)]
        else:
            line = [at(x0 + k, y) for y in range(y0, y1)]
        black = sum(1 for p in line if max(p) < 60)
        white = sum(1 for p in line if min(p) > 230)
        if best is None or black > best[0]:
            best = (black, white, len(line))
    black, white, n = best
    half = lambda c: 0.3 * n <= c <= 0.7 * n
    print("dashed" if half(black) and half(white) else f"not dashed: black {black} white {white} of {n}")
elif cmd == "checkered":
    # In the screen rectangle X0 Y0 X1 Y1: the checkerboard (neutral greys from its grey
    # to white) everywhere, with both its light and grey squares.
    at = pixels(sys.argv[2])
    x0, y0, x1, y1 = map(int, sys.argv[3:7])
    box = [at(x, y) for y in range(y0, y1) for x in range(x0, x1)]
    checker = sum(1 for p in box if min(p) >= 195 and max(p) - min(p) <= 3)
    light = sum(1 for p in box if min(p) >= 245)
    grey = sum(1 for p in box if all(195 <= c <= 213 for c in p))
    every = "all" if checker == len(box) else f"{checker}/{len(box)}"
    print(f"checker {every} light {min(light, 1)} grey {min(grey, 1)}")
elif cmd == "rgba":
    # An image file: its size and its pixels at X,Y ... as straight-alpha RRGGBBAA.
    p = GdkPixbuf.Pixbuf.new_from_file(sys.argv[2])
    rs, nc, data = p.get_rowstride(), p.get_n_channels(), p.get_pixels()
    out = [f"{p.get_width()}x{p.get_height()}"]
    for xy in sys.argv[3:]:
        x, y = map(int, xy.split(","))
        out.append(bytes(data[y * rs + x * nc:y * rs + x * nc + nc]).hex())
    print(*out)
elif cmd == "same":
    # Whether two image files have the same size and every pixel the same, alpha too.
    def rows(path):
        p = GdkPixbuf.Pixbuf.new_from_file(path)
        rs, n, data = p.get_rowstride(), p.get_width() * p.get_n_channels(), p.get_pixels()
        return p.get_width(), p.get_height(), [bytes(data[y * rs:y * rs + n]) for y in range(p.get_height())]
    print("same" if rows(sys.argv[2]) == rows(sys.argv[3]) else "different")
elif cmd == "menu":
    # The items of the menus open, in order: a popover menu item has no name of its own,
    # but the label it is labelled by holds its text.
    desktop = Atspi.get_desktop(0)
    stack = [desktop.get_child_at_index(i) for i in range(desktop.get_child_count())]
    stack = [a for a in stack if a is not None and a.get_name() == "etch"]
    while stack:
        n = stack.pop(0)
        if n is None:
            continue
        n.clear_cache()
        if n.get_role() == Atspi.Role.MENU_ITEM and not n.get_name():
            for r in n.get_relation_set():
                if r.get_relation_type() == Atspi.RelationType.LABELLED_BY:
                    for i in range(r.get_n_targets()):
                        t = r.get_target(i).get_text_iface()
                        if t is not None:
                            print(Atspi.Text.get_text(t, 0, Atspi.Text.get_character_count(t)))
        stack[:0] = [n.get_child_at_index(i) for i in range(n.get_child_count())]
elif cmd in ("at", "set", "invalid", "pressed"):
    def walk(n):
        if n is None:
            return
        n.clear_cache()
        yield n
        for i in range(n.get_child_count()):
            yield from walk(n.get_child_at_index(i))
    desktop = Atspi.get_desktop(0)
    apps = [desktop.get_child_at_index(i) for i in range(desktop.get_child_count())]
    nodes = [n for a in apps if a is not None and a.get_name() == "etch" for n in walk(a)]
    # The first widget named NAME, not counting labels (a label names its field).
    named = lambda name: next((n for n in nodes if n.get_name() == name and n.get_role() != Atspi.Role.LABEL), None) or sys.exit("nothing named " + name)
    if cmd == "at":
        # The screen centre of the widget named NAME in the window titled TITLE: its place
        # in the window, offset by where sway put the window (a dialog floats).
        import json, subprocess
        name, title = sys.argv[2], sys.argv[3]
        tree = json.loads(subprocess.run(["swaymsg", "-t", "get_tree"], capture_output=True, text=True).stdout)
        def cons(n):
            yield n
            for c in n.get("nodes", []) + n.get("floating_nodes", []):
                yield from cons(c)
        rect = next(c["rect"] for c in cons(tree) if c.get("name") == title)
        window = next(w for w in nodes if w.get_role() in (Atspi.Role.FRAME, Atspi.Role.DIALOG) and w.get_name() == title)
        n = next(n for n in walk(window) if n.get_name() == name)
        e = n.get_component_iface().get_extents(Atspi.CoordType.WINDOW)
        print(rect["x"] + e.x + e.width // 2, rect["y"] + e.y + e.height // 2)
    elif cmd == "set":
        # The slider named NAME set to VALUE.
        v = named(sys.argv[2]).get_value_iface()
        Atspi.Value.set_current_value(v, float(sys.argv[3]))
    elif cmd == "invalid":
        # Whether the field named NAME is marked invalid.
        invalid = named(sys.argv[2]).get_state_set().contains(Atspi.StateType.INVALID_ENTRY)
        print("invalid" if invalid else "valid")
    else:
        # The toggle buttons pressed: the tool on, and the choices shown.
        on = [n.get_name() for n in nodes if n.get_role() == Atspi.Role.TOGGLE_BUTTON
              and (n.get_state_set().contains(Atspi.StateType.CHECKED) or n.get_state_set().contains(Atspi.StateType.PRESSED))]
        print(", ".join(on) or "none")
PY
# Section 21: another GTK 4 app on the clipboard. "read X,Y ...": its window takes the
# keyboard (Wayland offers the clipboard to the focused window), waits for an image there,
# reads it as a texture, and prints its size and its pixels at each X,Y as straight-alpha
# RRGGBBAA. "set": a key pressed in its window (Wayland takes a selection only with an
# input event serial) puts a 60x40 image on the clipboard, the left half opaque blue and
# the right half blue at alpha 128, and it stays there until the app is killed.
cat > /tmp/clip.py <<"PY"
import sys
import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")
from gi.repository import Gdk, Gio, GLib, Gtk

mode, points = sys.argv[1], sys.argv[2:]
app = Gtk.Application(application_id="dev.cronkite.ClipCheck", flags=Gio.ApplicationFlags.NON_UNIQUE)

def read(clipboard):
    def done(cb, result):
        try:
            texture = cb.read_texture_finish(result)
        except GLib.Error as e:
            print(f"clipboard: error {e.message}", flush=True)
            app.quit()
            return
        d = Gdk.TextureDownloader.new(texture)
        d.set_format(Gdk.MemoryFormat.R8G8B8A8)
        data, stride = d.download_bytes()
        data = data.get_data()
        px = []
        for p in points:
            x, y = map(int, p.split(","))
            px.append(data[y * stride + x * 4:y * stride + x * 4 + 4].hex())
        print(f"clipboard: {texture.get_width()}x{texture.get_height()} " + " ".join(px), flush=True)
        app.quit()
    tries = [0]
    def poll():
        if clipboard.get_formats().contain_mime_type("image/png"):
            clipboard.read_texture_async(None, done)
            return False
        tries[0] += 1
        if tries[0] > 50:
            print("clipboard: no image", flush=True)
            app.quit()
            return False
        return True
    GLib.timeout_add(200, poll)

def put(clipboard):
    row = bytes([0, 0, 255, 255] * 30 + [0, 0, 255, 128] * 30)
    texture = Gdk.MemoryTexture.new(60, 40, Gdk.MemoryFormat.R8G8B8A8, GLib.Bytes.new(row * 40), 240)
    clipboard.set_content(Gdk.ContentProvider.new_for_bytes("image/png", texture.save_to_png_bytes()))
    print("set: 60x40", flush=True)

def activate(app):
    win = Gtk.ApplicationWindow(application=app, title="Clip check")
    if mode == "read":
        read(win.get_clipboard())
    else:
        keys = Gtk.EventControllerKey()
        keys.connect("key-pressed", lambda *_: put(win.get_clipboard()) or True)
        win.add_controller(keys)
    win.present()

app.connect("activate", activate)
app.run([])
PY
# The images sections 11, 12, 19, and 31 open.
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
make("/tmp/n.png", 400, 300, lambda x, y: (255,) * 3 if x < 200 else (249,) * 3, "png", False)
opaque = lambda x, y: (0x33, 0x66, 0xCC, 255)
make("/tmp/seam-1024x768.png", 1024, 768, opaque, "png", True)
make("/tmp/seam-600x450.png", 600, 450, opaque, "png", True)
for w, h in ((400, 300), (401, 301), (1501, 1001)):
    make(f"/tmp/seam-{w}x{h}.png", w, h, lambda x, y: (0, 0, 0, 0), "png", True)
PY
# Section 26: the titles of the etch windows (sorted), which has the focus, and which
# asked to come forward (xdg-activation: sway marks such a window urgent).
cat > /tmp/tree.py <<"PY"
import json
import subprocess

tree = json.loads(subprocess.run(["swaymsg", "-t", "get_tree"], capture_output=True, text=True).stdout)

def walk(n):
    yield n
    for c in n.get("nodes", []) + n.get("floating_nodes", []):
        yield from walk(c)

wins = [n for n in walk(tree) if n.get("app_id") == "dev.cronkite.Etch" and n.get("name")]
print("windows: " + ", ".join(sorted(n["name"] for n in wins)))
print("focused: " + next((n["name"] for n in wins if n.get("focused")), "none"))
print("urgent: " + ", ".join(sorted(n["name"] for n in wins if n.get("urgent"))))
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
  # SIGTERM ends etch without closing its windows, so an image with changes not saved
  # leaves its recovery journal as a crash does (section 30): each section starts without.
  quit() { pkill -x etch; sleep 1; rm -rf "${XDG_STATE_HOME:-$HOME/.local/state}/etch/recovery"; }

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
  quit
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
  quit

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
  quit

  echo "== 11 open"
  python3 /tmp/mk.py
  etch /tmp/t.png >>/tmp/open.log 2>&1 & sleep 3
  shown opened | grep -E "^opened: (t\.png|400×300|[0-9.]+%)$"
  shot opened; px opened /tmp/shot.png 400 300 | sed "s/^/opened: /"
  quit
  etch >>/tmp/app.log 2>&1 & sleep 3
  keys -M ctrl o -m ctrl; sleep 1.5
  shown chooser | grep -E "^chooser: (Open|Images \(PNG, JPEG, BMP\))$"
  keys -M ctrl l -m ctrl; keys /tmp/c.jpg; keys -k Return; sleep 3
  shot ctrl-o; shown ctrl-o | grep -E "^ctrl-o: (c\.jpg|320×200|Untitled)$"
  quit
  head -c 200 /tmp/t.png > /tmp/bad.png
  etch /tmp/bad.png >>/tmp/open.log 2>&1 & sleep 3
  shot damaged; shown damaged | grep -E "Cannot open|damaged"
  keys -k Return; sleep 1.5
  pgrep -x etch >/dev/null && echo "after alert: running" || echo "after alert: exited"
  printf "shopping: eggs, milk\n" > /tmp/notes.png
  etch /tmp/notes.png >>/tmp/open.log 2>&1 & sleep 3
  shown unsupported | grep -E "Cannot open|image Etch can open"
  quit
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
  quit
  etch >>/tmp/save.log 2>&1 & sleep 3
  stroke 0; keys -M ctrl s -m ctrl; sleep 1.5
  shown untitled | grep -E "^untitled: (Save As|Untitled\.png)$"
  quit
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
  quit

  echo "== 14 fill"
  etch >>/tmp/app.log 2>&1 & sleep 3
  keys -M ctrl 0 -m ctrl
  read mx my <<<"$(px centre)"
  l=$((mx - 100)); r=$((mx + 100)); t=$((my - 60)); b=$((my + 60))
  keys -M alt s -m alt; keys -M ctrl a -m ctrl; keys 3; keys -k Return
  for seg in "$l $t $r $t" "$r $t $r $b" "$r $b $l $b" "$l $b $l $t"; do
    python3 /scripts/dnd_helpers.py drag $seg; sleep 0.3
  done
  python3 /tmp/a11y.py press Red; sleep 0.5
  read x y <<<"$(px where Indigo)"; python3 /scripts/dnd_helpers.py click $x $y 1 right; sleep 0.5
  keys f; echo "key f: fill $(tool Fill) pencil $(tool Pencil)"
  echo "sizes shown: $(python3 /tmp/a11y.py text | grep -c "^Size$")"
  inside() { px count /tmp/shot.png $((l + 4)) $((t + 4)) $((r - 3)) $((b - 3)) $1; }
  outside() { px count /tmp/shot.png $((l - 60)) $t $((l - 20)) $b $1; }
  python3 /scripts/dnd_helpers.py click $mx $my; sleep 0.5
  shot filled; echo "filled: inside red $(inside ED1C24) outside white $(outside FFFFFF)"
  python3 /scripts/dnd_helpers.py click $((l - 40)) $my 1 right; sleep 0.5
  shot outside; echo "right: inside red $(inside ED1C24) outside indigo $(outside 3F48CC)"
  keys -M ctrl z -m ctrl; shot fill-undone
  echo "undone outside: inside red $(inside ED1C24) outside white $(outside FFFFFF)"
  keys -M ctrl z -m ctrl; shot fill-undone2; echo "undone inside: inside white $(inside FFFFFF)"
  keys -M ctrl y -m ctrl; shot fill-redone; echo "redone: inside red $(inside ED1C24)"
  quit

  echo "== 15 picker"
  etch /tmp/t.png >>/tmp/app.log 2>&1 & sleep 3
  read cx cy cw ch <<<"$(px canvas)"
  l=$((cx + (cw - 400) / 2)); t=$((cy + (ch - 300) / 2))
  colours() { python3 /tmp/a11y.py text | grep "^Foreground" | head -1 | sed "s/^/$1: /"; }
  picker() { python3 /tmp/a11y.py state "Colour Picker" 2>&1; }
  colours start
  keys b; keys i; echo "key i: picker $(picker) brush $(tool Brush)"
  echo "sizes shown: $(python3 /tmp/a11y.py text | grep -c "^Size$")"
  python3 /scripts/dnd_helpers.py click $((l + 100)) $((t + 150)); sleep 0.5
  colours red; echo "after red: picker $(picker) brush $(tool Brush)"
  keys i; python3 /scripts/dnd_helpers.py click $((l + 300)) $((t + 225)); sleep 0.5
  colours blue; echo "after blue: picker $(picker) brush $(tool Brush)"
  keys i; python3 /scripts/dnd_helpers.py click $((l + 300)) $((t + 75)) 1 right; sleep 0.5
  colours clear; echo "after clear: picker $(picker) brush $(tool Brush)"
  echo "title: $(python3 /tmp/a11y.py text | grep -E "^(• )?t\.png$" | head -1)"
  quit

  echo "== 16 shapes"
  etch >>/tmp/app.log 2>&1 & sleep 3
  keys -M ctrl 0 -m ctrl
  title() { python3 /tmp/a11y.py text | grep -E "^(• )?Untitled$" | head -1; }
  # A drag from (X1, Y1) to (X2, Y2) held still 6 s before its release, in the background:
  # the screen is read while the shape previews.
  held() { python3 /scripts/dnd_helpers.py drag $1 $2 $3 $4 6 & d=$!; sleep 3.5; }
  dark() { px count /tmp/shot.png $1 $2 $3 $4 000000; }
  echo "start: style $(tool Outline)"
  keys r; echo "key r: rectangle $(tool Rectangle) pencil $(tool Pencil)"
  echo "style: outline $(tool Outline) filled $(tool Filled) both $(tool Both)"
  echo "rectangle size: $(size)"
  keys -M alt s -m alt; keys -M ctrl a -m ctrl; keys 5; keys -k Return
  read mx my <<<"$(px centre)"
  l=$((mx - 100)); t=$((my - 60)); r=$((mx + 100)); b=$((my + 60))
  box() { dark $((l - 10)) $((t - 10)) $((r + 10)) $((b + 10)); }
  held $l $t $r $b; shot rect-held
  band=$(dark $((l + 6)) $((t + 1)) $((r - 5)) $((t + 4)))
  inside=$(px count /tmp/shot.png $((l + 8)) $((t + 8)) $((r - 7)) $((b - 7)) FFFFFF)
  echo "rect held: $(title) band $band inside $inside"; around=$(box)
  wait $d; sleep 0.5; shot rect-drawn
  echo "rect drawn: $(title) same $(same "$around" "$(box)")"
  keys -M ctrl z -m ctrl; shot rect-undone; echo "rect undone: $(title) ink $(px ink /tmp/shot.png)"
  read x y <<<"$(px where Red)"; python3 /scripts/dnd_helpers.py click $x $y 1 right; sleep 0.5
  keys o; echo "key o: ellipse $(tool Ellipse) rectangle $(tool Rectangle)"
  python3 /tmp/a11y.py press Both; sleep 0.5
  echo "pressed: outline $(tool Outline) both $(tool Both)"
  red() { px count /tmp/shot.png $((mx - 30)) $((my - 15)) $((mx + 30)) $((my + 15)) $1; }
  held $l $t $r $b; shot ellipse-held
  corner=$(px count /tmp/shot.png $((l + 1)) $((t + 1)) $((l + 12)) $((t + 8)) FFFFFF)
  echo "ellipse held: $(title) inside $(red ED1C24) corner $corner"; around=$(box)
  wait $d; sleep 0.5; shot ellipse-drawn
  echo "ellipse drawn: $(title) inside $(red ED1C24) same $(same "$around" "$(box)")"
  keys -M ctrl z -m ctrl; shot ellipse-undone
  echo "ellipse undone: $(title) ink $(px ink /tmp/shot.png) inside $(red FFFFFF)"
  keys l; echo "key l: line $(tool Line) style $(tool Outline)"
  held $l $t $r $b; shot line-held
  echo "line held: $(title) line $(px line /tmp/shot.png $l $t $r $b)"
  keys -k Escape; shot line-escaped; echo "line escaped: ink $(px ink /tmp/shot.png)"
  wait $d; sleep 0.5; shot line-released
  echo "line released: $(title) ink $(px ink /tmp/shot.png)"
  python3 /scripts/dnd_helpers.py drag $l $t $r $b; sleep 0.5; shot line-drawn
  echo "line drawn: $(title) line $(px line /tmp/shot.png $l $t $r $b)"
  keys -M ctrl z -m ctrl; shot line-undone; echo "line undone: $(title) ink $(px ink /tmp/shot.png)"
  quit

  echo "== 17 text"
  etch >>/tmp/app.log 2>&1 & sleep 3
  keys -M ctrl 0 -m ctrl
  desc() { python3 /tmp/a11y.py text | grep -E "^[0-9]+×[0-9]+ image" | head -1; }
  colours() { python3 /tmp/a11y.py text | grep "^Foreground" | head -1 | sed "s/^/$1: /"; }
  click() { python3 /scripts/dnd_helpers.py click $1 $2; sleep 0.5; }
  echo "start: font $(tool Bold) $(tool Italic)"
  keys t; echo "key t: text $(tool Text) pencil $(tool Pencil)"
  echo "font: bold $(tool Bold) italic $(tool Italic)"
  echo "text size: $(size)"
  read mx my <<<"$(px centre)"
  l=$((mx - 200)); t=$((my - 100))
  # The text (24 px) and its frame, from where it was clicked.
  ink() { px dark /tmp/shot.png $((l - 8)) $((t - 8)) $((l + 460)) $((t + 60)); }
  click $l $t; keys Hello; shot typed
  echo "typed: $(title) $(desc)"; echo "typed ink: $(ink)"
  keys " pebfilrostx"; shot letters
  echo "letters: text $(tool Text) pencil $(tool Pencil) eraser $(tool Eraser) $(desc)"
  colours letters
  keys -k Return; shot committed
  echo "committed: $(title) $(desc)"; echo "committed ink: $(px ink /tmp/shot.png)"
  keys -M ctrl z -m ctrl; shot text-undone; echo "text undone: $(title) ink $(px ink /tmp/shot.png)"
  click $l $t; keys Gone; shot gone; echo "gone: $(desc)"
  keys -k Escape; shot escaped; echo "escaped: $(title) ink $(px ink /tmp/shot.png) $(desc)"
  click $l $t; keys Click; click $((l + 300)) $((t + 200)); shot clicked-away
  echo "clicked away: $(title) $(desc)"; echo "clicked away ink: $(px ink /tmp/shot.png)"
  keys -M ctrl z -m ctrl; shot click-undone; echo "click undone: $(title) ink $(px ink /tmp/shot.png)"
  more() { [ "$2" -gt "$1" ] 2>/dev/null && echo yes || echo "no ($1 then $2)"; }
  click $l $t; keys "illicit fillip"; shot sans
  read _ sans _ sans_w <<<"$(ink)"; echo "sans: black $sans width $sans_w"
  python3 /tmp/a11y.py press Bold; sleep 0.5; shot bold
  read _ bold _ _ <<<"$(ink)"; echo "bold: $(tool Bold) darker $(more "$sans" "$bold")"
  python3 /tmp/a11y.py press Bold; sleep 0.5
  click $(px where Font); keys "DejaVu Sans Mono"; sleep 1; shot font-list
  click $(px where "DejaVu Sans Mono" label); sleep 0.5; shot mono
  read _ _ _ mono_w <<<"$(ink)"; echo "mono: wider $(more "$sans_w" "$mono_w")"
  keys "!"; echo "mono typed: $(desc)"
  keys -k Escape; shot font-escaped; echo "font escaped: $(title) ink $(px ink /tmp/shot.png) $(desc)"
  # An input method on the seat (scripts/ime_helpers.py, zwp_input_method_v2 to the
  # window): it composes "かな" and commits "仮名" into new text; a keyboard stays on the
  # seat throughout.
  mkfifo /tmp/ime.fifo
  ime() { timeout 5 sh -c "echo \"$1\" > /tmp/ime.fifo"; sleep 1; }
  wtype -s 60000 & board=$!; sleep 1
  python3 /scripts/ime_helpers.py serve /tmp/ime.fifo /tmp/ime.log 2>>/tmp/app.log & sleep 1
  click $l $t; ime "preedit かな 6"; shot ime-preedit
  read _ composing _ _ <<<"$(ink)"; echo "composing: $(desc) black $composing"
  ime "commit 仮名"; shot ime-committed; echo "composed: $(desc)"
  keys -k Return; shot ime-on; echo "ime on: $(title) $(desc)"; echo "ime ink: $(px ink /tmp/shot.png)"
  kill $board

  echo "== 18 new image"
  quit
  etch >>/tmp/app.log 2>&1 & sleep 3
  keys -M ctrl n -m ctrl; sleep 1
  shown dialog | grep -E "^dialog: (White|Transparent|Background colour|#FFFFFF)$"
  echo "dialog checked: $(px checked)"
  keys -M alt t -m alt; echo "picked: $(px checked)"
  keys -M alt w -m alt; keys -M ctrl a -m ctrl; keys 300; keys -k Tab
  keys -M ctrl a -m ctrl; keys 200; keys -k Return; sleep 2
  # The first window goes: the new one fills the screen.
  swaymsg "[title=\"^Untitled$\"] kill" >/dev/null; sleep 1.5
  keys -M ctrl 0 -m ctrl
  shown new | grep -E "^new: (Untitled 2|300×200|100%)$"
  shot transparent; echo "transparent: $(px clear /tmp/shot.png 300 200)"
  keys -M ctrl s -m ctrl; sleep 1.5
  keys -M ctrl a -m ctrl; keys /tmp/clear.png; keys -k Return; sleep 3
  echo "saved: $(px alpha /tmp/clear.png)"
  quit
  echo "settings: $(tr "\n" " " < ~/.config/etch/settings)"
  etch >>/tmp/app.log 2>&1 & sleep 3
  keys -M ctrl n -m ctrl; sleep 1
  shown again | grep -E "^again: (300|200)$"
  echo "again checked: $(px checked)"
  keys -k Escape; sleep 1
  quit

  echo "== 19 tolerance"
  etch /tmp/n.png >>/tmp/app.log 2>&1 & sleep 3
  # The text of the field, after its name and its description (the tooltip).
  tolerance() { python3 /tmp/a11y.py text | grep -A2 "^Tolerance$" | grep -E "^[0-9]+$" | tail -1; }
  echo "pencil: tolerance $(python3 /tmp/a11y.py text | grep -c "^Tolerance$")"
  keys f; echo "fill: $(tool Fill) tolerance $(tolerance)"
  python3 /tmp/a11y.py press Red; sleep 0.5
  read mx my <<<"$(px centre)"
  halves() { echo "left $(px count /tmp/shot.png $((mx - 196)) $((my - 146)) $((mx - 4)) $((my + 146)) ED1C24) right $(px count /tmp/shot.png $((mx + 4)) $((my - 146)) $((mx + 196)) $((my + 146)) ED1C24)"; }
  for tol in 0 5 6; do
    if [ $tol != 0 ]; then
      keys -M ctrl z -m ctrl
      keys -M alt t -m alt; keys -M ctrl a -m ctrl; keys $tol; keys -k Return
    fi
    python3 /scripts/dnd_helpers.py click $((mx - 100)) $my; sleep 0.5
    shot tolerance-$tol; echo "at $(tolerance): $(halves)"
  done
  keys p; keys f; echo "kept: $(tolerance)"
  quit

  echo "== 20 selection"
  etch /tmp/t.png >>/tmp/app.log 2>&1 & sleep 3
  read cx cy cw ch <<<"$(px canvas)"
  l=$((cx + (cw - 400) / 2)); t=$((cy + (ch - 300) / 2))
  title() { python3 /tmp/a11y.py text | grep -E "^(• )?t\.png$" | head -1; }
  sel() { s=$(python3 /tmp/a11y.py text | grep -E "^Selection: " | head -1); echo "${s:-no selection}"; }
  # Image pixels X0 Y0 X1 Y1 on screen, 4 px in from each edge (clear of the outline).
  inset() { echo $((l + $1 + 4)) $((t + $2 + 4)) $((l + $3 - 4)) $((t + $4 - 4)); }
  red() { px count /tmp/shot.png $(inset "$@") FF0000; }
  hole() { px checkered /tmp/shot.png $(inset "$@"); }
  keys s; echo "key s: selection $(tool Selection) pencil $(tool Pencil)"
  echo "sizes shown: $(python3 /tmp/a11y.py text | grep -c "^Size$")"
  # Image pixels 50 to 150 across and 50 to 130 down, in the red half.
  python3 /scripts/dnd_helpers.py drag $((l + 50)) $((t + 50)) $((l + 150)) $((t + 130)); sleep 0.5
  shot selected; echo "selected: $(sel) $(title)"
  echo "outline: top $(px dashes /tmp/shot.png $((l + 60)) $((t + 50)) $((l + 140)) $((t + 50))) left $(px dashes /tmp/shot.png $((l + 50)) $((t + 60)) $((l + 50)) $((t + 120)))"
  # A drag inside it: its pixels move 200 px right, into the transparent quarter.
  python3 /scripts/dnd_helpers.py drag $((l + 100)) $((t + 90)) $((l + 300)) $((t + 90)); sleep 0.5
  shot moved; echo "moved: $(sel) $(title)"
  echo "moved: left $(hole 50 50 150 130) reached red $(red 250 50 350 130)"
  echo "moved outline: $(px dashes /tmp/shot.png $((l + 260)) $((t + 50)) $((l + 340)) $((t + 50)))"
  keys -k Escape; shot landed; echo "landed: $(sel) $(title)"
  echo "landed: left $(hole 50 50 150 130) reached red $(red 250 50 350 130)"
  keys -M ctrl z -m ctrl; shot move-undone
  echo "move undone: $(title) red $(red 50 50 150 130) clear $(hole 250 50 350 130)"
  keys -M ctrl y -m ctrl; shot move-redone
  echo "move redone: $(title) left $(hole 50 50 150 130) reached red $(red 250 50 350 130)"
  keys -M ctrl z -m ctrl
  # Image pixels 120 to 200 across, 160 to 240 down: Shift+Right moves them 10 px right.
  python3 /scripts/dnd_helpers.py drag $((l + 120)) $((t + 160)) $((l + 200)) $((t + 240)); sleep 0.5
  keys -M shift -k Right -m shift; shot nudged
  echo "nudged: $(sel) left $(px checkered /tmp/shot.png $((l + 122)) $((t + 164)) $((l + 128)) $((t + 236))) reached $(px count /tmp/shot.png $((l + 202)) $((t + 164)) $((l + 208)) $((t + 236)) FF0000)"
  keys p; shot nudge-landed; echo "nudge landed: $(sel) $(title) pencil $(tool Pencil)"
  keys -M ctrl z -m ctrl
  keys -M ctrl a -m ctrl; echo "select all: $(sel) selection $(tool Selection) pencil $(tool Pencil)"
  keys -k Escape; echo "deselected: $(sel) $(title)"
  python3 /scripts/dnd_helpers.py drag $((l + 50)) $((t + 50)) $((l + 150)) $((t + 130)); sleep 0.5
  keys -k Delete; shot deleted; echo "deleted: $(sel) $(title) $(hole 50 50 150 130)"
  keys -M ctrl z -m ctrl; shot delete-undone; echo "delete undone: $(title) red $(red 50 50 150 130)"
  keys t; click $((l + 50)) $((t + 200)); keys ab; keys -k Left; keys -k Delete
  keys -M ctrl a -m ctrl
  echo "typing: $(desc) $(sel) text $(tool Text) selection $(tool Selection)"
  keys -k Escape
  quit

  echo "== 21 clipboard"
  etch /tmp/t.png >>/tmp/app.log 2>&1 & sleep 3
  read cx cy cw ch <<<"$(px canvas)"
  l=$((cx + (cw - 400) / 2)); t=$((cy + (ch - 300) / 2))
  focus() { swaymsg "[app_id=dev.cronkite.Etch] focus" >/dev/null; sleep 0.5; }
  # The other app reads the clipboard: its size and the pixels at X,Y ...
  other() { timeout 20 python3 /tmp/clip.py read "$@" 2>>/tmp/clip.log; focus; }
  keys s
  # Image pixels 150 to 250 across and 110 to 190 down: red, transparent, and blue.
  python3 /scripts/dnd_helpers.py drag $((l + 150)) $((t + 110)) $((l + 250)) $((t + 190)); sleep 0.5
  keys -M ctrl c -m ctrl; sleep 0.5
  echo "offered: $(wl-paste -l | grep -c "^image/png$") $(sel) $(title)"
  other 0,0 99,0 99,79 | sed "s/^/copied /"
  keys -M ctrl x -m ctrl; shot cut; echo "cut: $(sel) $(title) $(hole 150 110 200 190)"
  other 0,0 99,0 99,79 | sed "s/^/cut /"
  keys -M ctrl z -m ctrl; shot cut-undone; echo "cut undone: $(title) red $(red 150 110 200 190)"
  keys -k Escape
  python3 /tmp/clip.py set >>/tmp/clip.log 2>&1 & setter=$!; sleep 3
  # The PNG on the clipboard now: its size, from its header.
  keys c; echo "other set: $(wl-paste -t image/png | python3 -c "import struct, sys; print(*struct.unpack(\">II\", sys.stdin.buffer.read()[16:24]), sep=\"x\")")"
  focus
  keys -M ctrl v -m ctrl; sleep 1; shot pasted
  echo "pasted: $(sel) $(title) selection $(tool Selection) blue $(px count /tmp/shot.png $(inset 0 0 30 40) 0000FF)"
  keys -M ctrl z -m ctrl; shot paste-dropped; echo "dropped: $(sel) $(title) red $(red 0 0 60 40)"
  keys -M ctrl v -m ctrl; sleep 1
  python3 /scripts/dnd_helpers.py drag $((l + 15)) $((t + 20)) $((l + 115)) $((t + 120)); sleep 0.5
  shot paste-moved
  echo "paste moved: $(sel) $(title) left red $(red 0 0 60 40) blue $(px count /tmp/shot.png $(inset 100 100 130 140) 0000FF)"
  keys -k Escape; shot paste-landed; echo "paste landed: $(sel) $(title)"
  keys -M ctrl a -m ctrl; keys -M ctrl c -m ctrl; sleep 0.5
  other 100,100 145,100 99,100 | sed "s/^/landed /"
  keys -k Escape
  keys -M ctrl z -m ctrl; shot paste-undone; echo "paste undone: $(title) red $(red 100 100 160 140)"
  keys -M ctrl y -m ctrl; shot paste-redone
  echo "paste redone: $(title) blue $(px count /tmp/shot.png $(inset 100 100 130 140) 0000FF)"
  kill $setter 2>/dev/null
  keys t; click $((l + 50)) $((t + 250)); keys ab
  keys -M ctrl v -m ctrl; sleep 1
  echo "typing: $(desc) $(sel)"
  keys -k Escape
  quit
  echo "clip log: $(grep -c . /tmp/clip.log)"; sed "s/^/clip: /" /tmp/clip.log

  # The image size in the status; the title of FILE; the W x H image centred at 100% in
  # the last screenshot: pixels of colour HEX in its image pixels X0 Y0 X1 Y1, 4 px in.
  size() { python3 /tmp/a11y.py text | grep -E "^[0-9]+×[0-9]+$" | head -1; }
  titled() { python3 /tmp/a11y.py text | grep -E "^(• )?$1$" | head -1; }
  centred() { # W H X0 Y0 X1 Y1 HEX
    read cx cy cw ch <<<"$(px canvas)"
    local l=$((cx + (cw - $1) / 2)) t=$((cy + (ch - $2) / 2))
    px count /tmp/shot.png $((l + $3 + 4)) $((t + $4 + 4)) $((l + $5 - 4)) $((t + $6 - 4)) $7
  }

  echo "== 22 crop"
  cp /tmp/t.png /tmp/crop.png
  etch /tmp/crop.png >>/tmp/app.log 2>&1 & sleep 3
  read cx cy cw ch <<<"$(px canvas)"
  l=$((cx + (cw - 400) / 2)); t=$((cy + (ch - 300) / 2))
  keys -M ctrl -M shift x -m shift -m ctrl; sleep 1
  echo "unselected: $(size) $(titled crop.png)"
  keys s
  python3 /scripts/dnd_helpers.py drag $((l + 150)) $((t + 100)) $((l + 250)) $((t + 200)); sleep 0.5
  echo "selected: $(sel)"
  keys -M alt i -m alt; sleep 1; shot image-menu
  keys c; sleep 1.5; shot cropped
  echo "cropped: $(size) $(sel) $(titled crop.png)"
  px opened /tmp/shot.png 100 100 | sed "s/^/cropped: /"
  keys -M ctrl s -m ctrl; sleep 2
  echo "saved: $(px rgba /tmp/crop.png 0,0 99,0 99,99)"
  keys -M ctrl z -m ctrl; sleep 1; shot crop-undone
  echo "crop undone: $(size) $(titled crop.png)"
  px opened /tmp/shot.png 400 300 | sed "s/^/crop undone: /"
  keys -M ctrl y -m ctrl; sleep 1; echo "crop redone: $(size) $(titled crop.png)"
  keys -M ctrl z -m ctrl; sleep 1
  python3 /scripts/dnd_helpers.py drag $((l + 50)) $((t + 50)) $((l + 150)) $((t + 130)); sleep 0.5
  python3 /scripts/dnd_helpers.py drag $((l + 100)) $((t + 90)) $((l + 300)) $((t + 90)); sleep 0.5
  keys -M ctrl -M shift x -m shift -m ctrl; sleep 1.5; shot float-cropped
  echo "float cropped: $(size) $(sel) red $(centred 100 80 0 0 100 80 FF0000)"
  keys -M ctrl z -m ctrl; sleep 1; shot float-crop-undone
  echo "float crop undone: $(size) red $(centred 400 300 250 50 350 130 FF0000) left $(px checkered /tmp/shot.png $((l + 54)) $((t + 54)) $((l + 146)) $((t + 126)))"
  quit

  echo "== 23 resize"
  cp /tmp/t.png /tmp/resize.png
  etch /tmp/resize.png >>/tmp/app.log 2>&1 & sleep 3
  # The number in the field named NAME.
  field() { python3 /tmp/a11y.py text | grep -A3 "^$1$" | grep -E "^[0-9]+$" | head -1; }
  keys -M ctrl r -m ctrl; sleep 1
  shown dialog | grep -E "^dialog: (Resize Image|Keep aspect ratio|Smooth|Nearest neighbour)$"
  echo "dialog: width $(field Width) height $(field Height) checked $(px checked)"
  keys -k Escape; sleep 1
  echo "cancelled: $(size) $(titled resize.png) $(python3 /tmp/a11y.py text | grep -c "^Resize Image$")"
  keys -M ctrl r -m ctrl; sleep 1
  keys -M alt w -m alt; keys -M ctrl a -m ctrl; keys 600; keys -k Tab; sleep 0.5
  echo "linked: width $(field Width) height $(field Height)"
  keys -M alt n -m alt; echo "picked: $(px checked)"
  keys -M alt w -m alt; keys -k Return; sleep 2; shot resized-nearest
  echo "nearest: $(size) $(sel) $(titled resize.png)"
  px opened /tmp/shot.png 600 450 | sed "s/^/nearest: /"
  keys -M ctrl s -m ctrl; sleep 2
  echo "nearest saved: $(px rgba /tmp/resize.png 299,0 300,0 300,225 599,449)"
  keys -M ctrl z -m ctrl; sleep 1; shot resize-undone
  echo "resize undone: $(size) $(titled resize.png)"
  px opened /tmp/shot.png 400 300 | sed "s/^/resize undone: /"
  keys -M ctrl r -m ctrl; sleep 1
  keys -M alt w -m alt; keys -M ctrl a -m ctrl; keys 200; keys -k Tab; sleep 0.5
  keys -M alt w -m alt; keys -k Return; sleep 2; shot resized-smooth
  echo "smooth: $(size) $(titled resize.png)"
  keys -M ctrl s -m ctrl; sleep 2
  echo "smooth saved: $(px rgba /tmp/resize.png 0,0 99,10 100,10)"
  keys -M ctrl z -m ctrl; sleep 1; echo "smooth undone: $(size)"
  quit

  echo "== 24 canvas"
  cp /tmp/t.png /tmp/canvas.png
  etch /tmp/canvas.png >>/tmp/app.log 2>&1 & sleep 3
  anchors() {
    for a in "Top left" Top "Top right" Left Centre Right "Bottom left" Bottom "Bottom right"; do
      python3 /tmp/a11y.py state "$a"
    done | tr "\n" " " | sed "s/ $//"
  }
  # The canvas dialog: width W, height H (typed), then the anchor named A pressed.
  ask_canvas() {
    keys -M ctrl -M shift r -m shift -m ctrl; sleep 1
    keys -M alt w -m alt; keys -M ctrl a -m ctrl; keys $1; keys -k Tab
    keys -M alt h -m alt; keys -M ctrl a -m ctrl; keys $2; keys -k Tab
    python3 /tmp/a11y.py press "$3"; sleep 0.5
  }
  keys -M ctrl -M shift r -m shift -m ctrl; sleep 1
  shown dialog | grep -E "^dialog: (Resize Canvas|Top left|Bottom right|Centre|Transparent|Background colour)$"
  echo "dialog: width $(field Width) height $(field Height) anchors $(anchors) checked $(px checked)"
  keys -k Escape; sleep 1
  ask_canvas 600 400 "Top left"
  echo "pressed: anchors $(anchors)"
  keys -M alt w -m alt; keys -k Return; sleep 2; shot canvas-top-left
  read cx cy cw ch <<<"$(px canvas)"
  l=$((cx + (cw - 600) / 2)); t=$((cy + (ch - 400) / 2))
  echo "top left: $(size) $(titled canvas.png)"
  echo "top left: red $(centred 600 400 0 0 200 300 FF0000) right $(px checkered /tmp/shot.png $((l + 404)) $((t + 4)) $((l + 596)) $((t + 396))) below $(px checkered /tmp/shot.png $((l + 4)) $((t + 304)) $((l + 396)) $((t + 396)))"
  keys -M ctrl s -m ctrl; sleep 2
  echo "top left saved: $(px rgba /tmp/canvas.png 0,0 399,299 400,0 0,300 599,399)"
  keys -M ctrl z -m ctrl; sleep 1; shot canvas-undone
  echo "canvas undone: $(size) $(titled canvas.png)"
  px opened /tmp/shot.png 400 300 | sed "s/^/canvas undone: /"
  read x y <<<"$(px where Indigo)"; python3 /scripts/dnd_helpers.py click $x $y 1 right; sleep 0.5
  ask_canvas 500 400 "Bottom right"
  keys -M alt g -m alt; echo "background: $(px checked)"
  keys -M alt w -m alt; keys -k Return; sleep 2
  keys -M ctrl s -m ctrl; sleep 2
  echo "bottom right saved: $(px rgba /tmp/canvas.png 0,0 99,99 100,100 499,399)"
  keys -M ctrl z -m ctrl; sleep 1
  ask_canvas 300 200 Centre
  keys -M alt w -m alt; keys -k Return; sleep 2
  echo "shrunk: $(size)"
  keys -M ctrl s -m ctrl; sleep 2
  echo "shrunk saved: $(px rgba /tmp/canvas.png 0,0 149,0 150,0 299,199)"
  keys -M ctrl z -m ctrl; sleep 1; shot shrink-undone
  echo "shrink undone: $(size)"
  px opened /tmp/shot.png 400 300 | sed "s/^/shrink undone: /"
  quit

  echo "== 25 rotate"
  cp /tmp/t.png /tmp/rot.png
  etch /tmp/rot.png >>/tmp/app.log 2>&1 & sleep 3
  # Saves, then prints the file pixels at X,Y ...
  saved() { keys -M ctrl s -m ctrl; sleep 2; px rgba /tmp/rot.png "$@"; }
  keys -M ctrl h -m ctrl; sleep 1.5; shot rotated-right
  read cx cy cw ch <<<"$(px canvas)"
  l=$((cx + (cw - 300) / 2)); t=$((cy + (ch - 400) / 2))
  echo "right: $(size) $(titled rot.png)"
  echo "right: red $(centred 300 400 0 0 300 200 FF0000) clear $(px checkered /tmp/shot.png $((l + 154)) $((t + 204)) $((l + 296)) $((t + 396)))"
  echo "right saved: $(saved 0,0 0,399 299,399)"
  keys -M ctrl z -m ctrl; sleep 1; shot rotate-undone
  echo "right undone: $(size)"
  px opened /tmp/shot.png 400 300 | sed "s/^/right undone: /"
  keys -M alt i -m alt; sleep 1; keys v; sleep 1.5
  echo "vertical: $(size) $(saved 0,0 399,0 399,299)"
  keys -M ctrl z -m ctrl; sleep 1; echo "vertical undone: $(size)"
  keys -M ctrl g -m ctrl; sleep 1.5
  echo "left: $(size) $(saved 0,0 0,399 299,0)"
  keys -M ctrl z -m ctrl; sleep 1; echo "left undone: $(size)"
  keys -M ctrl j -m ctrl; sleep 1.5
  echo "half: $(size) $(saved 0,0 399,299 0,299)"
  keys -M ctrl z -m ctrl; sleep 1; echo "half undone: $(size)"
  keys -M alt i -m alt; sleep 1; keys h; sleep 1.5
  echo "horizontal: $(size) $(saved 0,0 0,299 399,0)"
  keys -M ctrl z -m ctrl; sleep 1; shot flip-undone
  echo "horizontal undone: $(size)"
  px opened /tmp/shot.png 400 300 | sed "s/^/horizontal undone: /"
  for i in 1 2 3 4; do keys -M ctrl h -m ctrl; sleep 1.5; done
  keys -M ctrl s -m ctrl; sleep 2
  echo "four turns: $(size) $(titled rot.png) $(px same /tmp/rot.png /tmp/t.png)"
  keys -M ctrl z -m ctrl; sleep 1; echo "three turns: $(size) $(titled rot.png)"
  quit

  echo "== 26 drop"
  cp /tmp/t.png /tmp/one.png; cp /tmp/c.jpg /tmp/two.jpg
  printf "shopping: eggs, milk\n" > /tmp/notes.png
  # Its own log: the text file failing is logged (the app log must stay clean).
  etch /tmp/t.png >>/tmp/drop.log 2>&1 & sleep 3
  # Drags FILE... from the source in the corner onto the screen at X Y.
  drop() {
    local x=$1 y=$2; shift 2
    python3 /scripts/dnd_helpers.py source "$@" >>/tmp/dnd.log 2>&1 & src=$!; sleep 3
    python3 /scripts/dnd_helpers.py drag 1160 640 $x $y; sleep 4
    kill $src; sleep 1
  }
  python3 /tmp/tree.py | sed "s/^/before: /"
  drop 300 360 /tmp/one.png /tmp/two.jpg; shot dropped
  python3 /tmp/tree.py | sed "s/^/dropped: /"
  # Onto the window at the right, so t.png has to come forward. A click in that window
  # first (on its status bar, changing nothing), as someone working in Etch makes: the
  # compositor takes a request to come forward only from an app that had input lately.
  python3 /scripts/dnd_helpers.py click 1000 708; sleep 0.5
  drop 1000 300 /tmp/t.png
  python3 /tmp/tree.py | sed "s/^/again: /"
  drop 600 300 /tmp/notes.png
  shown alert | grep -E "^alert: (Cannot open|It isn)"
  keys -k Return; sleep 1
  python3 /tmp/tree.py | sed "s/^/after alert: /"
  quit
  grep -E "^etch: |panicked|CRITICAL" /tmp/drop.log | sed "s/^/log: /"

  echo "== 27 recent"
  # A state folder of its own: the sections before opened and saved images too.
  rm -rf /tmp/state27
  for f in r1 r2 r3; do cp /tmp/t.png /tmp/$f.png; done
  run() { XDG_STATE_HOME=/tmp/state27 etch "$@" >>/tmp/recent.log 2>&1 & sleep 3; }
  stored() { sed "s|^/tmp/||" /tmp/state27/etch/recent | tr "\n" " " | sed "s/ $//"; }
  # File, Open Recent: the submenu open by keyboard, its first item selected.
  recent_menu() { keys -M alt f -m alt; sleep 1; keys r; sleep 1; }
  # The image items of the menus open, by name, in order.
  items() { px menu | grep -E "\.png  \(" | cut -d" " -f1 | tr "\n" " " | sed "s/ $//"; }
  for f in r1 r2 r3 r1; do run /tmp/$f.png; quit; done
  echo "opened: $(stored)"
  run; keys -M ctrl s -m ctrl; sleep 1.5
  keys -M ctrl a -m ctrl; keys /tmp/r4.png; keys -k Return; sleep 3
  quit
  echo "saved: $(stored)"
  run
  recent_menu; shot recent-menu
  echo "menu: $(items)"
  px menu | grep -E "^r4\.png" | sed "s/^/menu item: /"
  keys -k Return; sleep 3
  python3 /tmp/tree.py | grep windows | sed "s/^/chosen: /"
  rm /tmp/r2.png
  recent_menu; keys -k Down -k Down -k Down; keys -k Return; sleep 2
  shown missing | grep -E "^missing: (Cannot open|The file)"
  keys -k Return; sleep 1
  python3 /tmp/tree.py | grep windows | sed "s/^/missing: /"
  echo "missing: $(stored)"
  recent_menu; echo "after missing: $(items)"
  keys -k Escape -k Escape; quit
  grep -E "^etch: |panicked|CRITICAL" /tmp/recent.log | sed "s/^/log: /"

  echo "== 28 colours"
  etch >>/tmp/app.log 2>&1 & sleep 3
  colours() { python3 /tmp/a11y.py text | grep "^Foreground" | head -1 | sed "s/^/$1: /"; }
  # What the hex field shows (after the label and the field named Hex, and its tooltip).
  field() { python3 /tmp/a11y.py text | grep -A2 "^Hex$" | tail -1; }
  # Alt+H puts the keys in the hex field, its text selected; TEXT typed, then Enter.
  hex() { keys -M alt h -m alt; keys "$1"; keys -k Return; sleep 0.5; }
  # In the chooser titled TITLE: + (a custom colour) opens the editor; its hex field
  # takes HEX, its alpha slider ALPHA (0 to 1); then Select.
  custom() {
    read x y <<<"$(px at "Add Color" "$1")"; python3 /scripts/dnd_helpers.py click $x $y; sleep 1.5
    read x y <<<"$(px at "Hexadecimal color or color name" "$1")"
    python3 /scripts/dnd_helpers.py click $x $y 3; sleep 0.5; keys "$2"; keys -k Return; sleep 0.5
    px set "Alpha value" $3; sleep 0.5; shot "editor-$2"
    python3 /tmp/a11y.py press Select; sleep 1
  }
  colours start
  echo "start: $(field)"
  hex "#3366CC"; colours typed; echo "typed: $(field) $(px invalid Hex)"
  hex "#f80"; colours short
  hex "#3366CC80"; colours alpha
  # Every tool key and X typed in the field: text, not commands.
  keys -M alt h -m alt; keys pebfilrostx; sleep 0.5; echo "keys: $(field) tools $(px pressed)"
  keys -k Return; sleep 0.5; shot hex-invalid
  colours invalid; echo "invalid: $(px invalid Hex) tools $(px pressed)"
  hex "#12345"; colours five; echo "five: $(px invalid Hex)"
  # Clicking the canvas leaves the field showing the colour again.
  read mx my <<<"$(px centre)"; python3 /scripts/dnd_helpers.py click $mx $my; sleep 0.5
  keys -M ctrl z -m ctrl; echo "left: $(field) $(px invalid Hex)"
  python3 /tmp/a11y.py press "Custom…"; sleep 2; shot chooser
  shown chooser | grep -E "^chooser: (Foreground Colour|Background Colour|Select|Cancel)$"
  python3 /tmp/a11y.py press Cancel; sleep 1; colours cancelled
  python3 /tmp/a11y.py press "Custom…"; sleep 2
  custom "Foreground Colour" "#20A040" 0.25; colours chosen; echo "chosen: $(field)"
  read x y <<<"$(px where "Custom…")"; python3 /scripts/dnd_helpers.py click $x $y 1 right; sleep 2
  shown right | grep -E "^right: (Foreground Colour|Background Colour)$"
  custom "Background Colour" "#FFCC00" 1; colours background
  quit

  echo "== 29 keys"
  etch >>/tmp/app.log 2>&1 & sleep 3
  keys -M ctrl 0 -m ctrl
  colours() { python3 /tmp/a11y.py text | grep "^Foreground" | head -1 | sed "s/^/$1: /"; }
  desc() { python3 /tmp/a11y.py text | grep -E "^[0-9]+×[0-9]+ image" | head -1; }
  # The tool on: the first toggle button pressed, in the tool column.
  tool_on() { px pressed | cut -d, -f1; }
  read mx my <<<"$(px centre)"
  # Each tool key on the canvas picks its tool.
  for k in p b e f i t r l s o; do keys $k; echo "key $k: $(tool_on)"; done
  colours before; keys x; colours swapped; keys x; colours back
  # Text being typed takes them all as letters.
  keys t; python3 /scripts/dnd_helpers.py click $((mx - 200)) $((my - 100)); sleep 0.5
  keys pebfilrostx; echo "typing: $(tool_on) $(desc)"; colours typing
  keys -k Escape; sleep 0.5
  # So do the size field (Alt+S), the tolerance (Alt+T), the hex field (Alt+H), and the
  # search of the font list.
  keys p; keys -M alt s -m alt; keys pebfilrostx; echo "size: $(tool_on)"; colours size
  python3 /scripts/dnd_helpers.py click $mx $my; sleep 0.5; keys -M ctrl z -m ctrl
  keys b; echo "after size: $(tool_on)"
  keys f; keys -M alt t -m alt; keys pebfilrostx; echo "tolerance: $(tool_on)"; colours tolerance
  python3 /scripts/dnd_helpers.py click $((mx + 600)) $my; sleep 0.5
  keys -M alt h -m alt; keys pebfilrostx; echo "hex: $(tool_on)"; colours hex
  python3 /scripts/dnd_helpers.py click $((mx + 600)) $my; sleep 0.5
  keys t; python3 /scripts/dnd_helpers.py click $(px where Font); sleep 0.5
  keys pebfilrostx; sleep 0.5; shot font-search; echo "font search: $(tool_on)"; colours font
  # Escape stops the search, a second closes the list; a click off the image (which
  # starts no text) gives the keys back to the canvas.
  keys -k Escape -k Escape; sleep 0.5
  python3 /scripts/dnd_helpers.py click $((mx + 600)) $my; sleep 0.5
  # In an open menu a letter picks an item: Alt+V, F fits the image to the window.
  keys e; keys -M alt v -m alt; keys f; sleep 0.5; echo "menu: $(tool_on) $(zoom)"
  quit

  echo "== 30 recovery"
  jdir=${XDG_STATE_HOME:-$HOME/.local/state}/etch/recovery
  journals() { ls "$jdir"/*.journal 2>/dev/null | wc -l; }
  title() { python3 /tmp/a11y.py text | grep -E "^(• )?(k\.png|Untitled)$" | head -1; }
  unchanged() { [ "$(md5sum </tmp/k.png)" = "$sum" ] && echo "file unchanged" || echo "file changed"; }
  start() { etch "$@" >>/tmp/recovery.log 2>&1 & sleep 3; }
  # A crash: SIGKILL, so nothing runs on the way out.
  crash() { pkill -9 -x etch; sleep 1; }
  # k.png (a copy of t.png) at 100%: the band of its red half where the stroke goes.
  cp /tmp/t.png /tmp/k.png; sum=$(md5sum </tmp/k.png)
  start /tmp/k.png
  read cx cy cw ch <<<"$(px canvas)"
  l=$((cx + (cw - 400) / 2)); t=$((cy + (ch - 300) / 2))
  band() { shot "$1"; px count /tmp/shot.png $((l + 30)) $((t + 99)) $((l + 170)) $((t + 102)) "$2"; }
  keys -M alt s -m alt; keys -M ctrl a -m ctrl; keys 5; keys -k Return
  t0=$(date +%s.%N)
  python3 /scripts/dnd_helpers.py drag $((l + 20)) $((t + 100)) $((l + 180)) $((t + 100))
  for i in $(seq 100); do [ "$(journals)" -gt 0 ] && break; sleep 0.1; done
  echo "journal after $(python3 -c "import sys, time; print(round(time.time() - float(sys.argv[1]), 1))" $t0) s"
  echo "drawn: $(title) black $(band drawn 000000)"
  crash; echo "killed: $(journals) journals, $(unchanged)"
  start /tmp/k.png
  shot restore-asked; shown asked | grep -E "Restore unsaved edits|closed unexpectedly|^asked: (Discard|Restore)$"
  python3 /tmp/a11y.py press Restore; sleep 1
  echo "restored: $(title) black $(band restored 000000) $(unchanged) $(journals) journals"
  keys -M ctrl z -m ctrl; sleep 0.5
  echo "undone: $(title) red $(band restore-undone FF0000)"
  # Again, then kill it, and etch alone opens k.png to offer it back: Discard.
  keys -M ctrl y -m ctrl; sleep 6.5
  crash; echo "again: $(journals) journals"
  start
  shown alone | grep -E "^alone: (• )?k\.png$|Restore unsaved edits"
  python3 /tmp/a11y.py press Discard; sleep 1
  echo "discarded: $(title) red $(band discarded FF0000) $(journals) journals"
  # Undoing back to the file removes the journal, and so does closing with Discard.
  python3 /scripts/dnd_helpers.py drag $((l + 20)) $((t + 100)) $((l + 180)) $((t + 100)); sleep 6.5
  echo "edited: $(journals) journals"
  keys -M ctrl z -m ctrl; sleep 1; echo "undone to the file: $(title) $(journals) journals"
  keys -M ctrl y -m ctrl; sleep 6.5; echo "redone: $(journals) journals"
  keys -M ctrl w -m ctrl; sleep 1; python3 /tmp/a11y.py press Discard; sleep 1.5
  pgrep -x etch >/dev/null && echo "closed: running" || echo "closed: exited, $(journals) journals, $(unchanged)"
  # A file changed after the crash: its journal is only kept or discarded.
  start /tmp/k.png
  python3 /scripts/dnd_helpers.py drag $((l + 20)) $((t + 100)) $((l + 180)) $((t + 100)); sleep 6.5
  crash; touch /tmp/k.png; sum=$(md5sum </tmp/k.png)
  start /tmp/k.png
  shot changed; shown changed | grep -E "changed after the crash|^changed: (Keep for Later|Discard)$"
  python3 /tmp/a11y.py press "Keep for Later"; sleep 1
  echo "kept: $(title) red $(band kept FF0000) $(journals) journals"
  quit
  # The journal of an Untitled image: etch alone offers it back on a new Untitled image.
  start
  keys -M ctrl 0 -m ctrl
  read mx my <<<"$(px centre)"
  python3 /scripts/dnd_helpers.py drag $((mx - 150)) $((my - 80)) $((mx + 150)) $((my + 80)); sleep 0.5
  shot untitled-drawn; ink=$(px ink /tmp/shot.png); echo "untitled: $(title) ink $ink"
  sleep 6.5; crash; echo "untitled killed: $(journals) journals"
  start
  shown untitled-asked | grep -E "^untitled-asked: (Untitled|1024×768)$|Restore unsaved edits|an untitled image"
  python3 /tmp/a11y.py press Restore; sleep 1; keys -M ctrl 0 -m ctrl
  shot untitled-restored; echo "untitled restored: $(title) ink $(same "$ink" "$(px ink /tmp/shot.png)")"
  keys -M ctrl z -m ctrl; shot untitled-undone
  echo "untitled undone: $(title) ink $(px ink /tmp/shot.png)"
  quit
  grep -E "journaled|restored| (WARN|ERROR) |panicked|CRITICAL" /tmp/recovery.log | sed -E "s/^.* INFO etch::[a-z_]+: //; s/^/log: /"

  echo "== 31 seams"
  # Each way a new etch, its renderer logged: the default one, then cairo (the software
  # renderer of GTK); at output scale 1, then 2, then 1.5.
  seams() { # SCALE RENDERER
    local tag="s$1 $2" log=/tmp/seams-$1-$2.log env=()
    [ "$2" = cairo ] && env=(env GSK_RENDERER=cairo)
    launch() { GSK_DEBUG=renderer "${env[@]}" etch "$@" >>"$log" 2>&1 & sleep 3; }
    solid() { shot "seams-$1-$2"; echo "$tag $2 $(zoom): $(px solid /tmp/shot.png 3366CC)"; }
    launch /tmp/seam-1024x768.png
    keys -M ctrl 0 -m ctrl; solid "$1" 100
    keys -M ctrl 9 -m ctrl; solid "$1" fit
    keys -M ctrl 0 -m ctrl; keys -M ctrl -k minus -k minus -m ctrl; solid "$1" out
    keys -M ctrl 0 -m ctrl; keys -M ctrl -k equal -k equal -m ctrl; solid "$1" in
    quit
    launch /tmp/seam-600x450.png; keys -M ctrl 9 -m ctrl; solid "$1" small; quit
    for f in 400x300 401x301 1501x1001; do
      launch /tmp/seam-$f.png
      shot "seams-$1-$2-$f"; echo "$tag $f $(zoom): $(px crisp /tmp/shot.png)"
      quit
    done
    echo "$tag renderer: $(sed -n "s/^Using renderer .\(Gsk[A-Za-z]*\). .*/\1/p" "$log" | sort -u | paste -sd,)"
  }
  seams 1 default; seams 1 cairo
  swaymsg output HEADLESS-1 resolution 2560x1440 scale 2 >/dev/null; sleep 1
  seams 2 default; seams 2 cairo
  swaymsg output HEADLESS-1 resolution 1920x1080 scale 1.5 >/dev/null; sleep 1
  seams 1.5 default; seams 1.5 cairo
  swaymsg output HEADLESS-1 resolution 1280x720 scale 1 >/dev/null
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
# ETCH-9: the fill bucket.
check "14: F picks the fill" "14 fill" "^key f: fill on pencil off$"
check "14: ... which has no size" "14 fill" "^sizes shown: 0$"
check "14: a click fills the box red to its edges, the outside untouched" "14 fill" "^filled: inside red all outside white all$"
check "14: a right-click fills the outside with the background" "14 fill" "^right: inside red all outside indigo all$"
check "14: Ctrl+Z takes back the outside's fill" "14 fill" "^undone outside: inside red all outside white all$"
check "14: another Ctrl+Z the inside's" "14 fill" "^undone inside: inside white all$"
check "14: Ctrl+Y fills it again" "14 fill" "^redone: inside red all$"
# ETCH-10: the colour picker.
check "15: I picks the colour picker" "15 picker" "^key i: picker on brush off$"
check "15: ... which has no size" "15 picker" "^sizes shown: 0$"
check "15: the colours start black on white" "15 picker" "^start: Foreground #000000, background #FFFFFF$"
check "15: a click on red makes it the foreground" "15 picker" "^red: Foreground #FF0000, background #FFFFFF$"
check "15: ... and goes back to the brush" "15 picker" "^after red: picker off brush on$"
check "15: a click on half-transparent blue picks its alpha too" "15 picker" "^blue: Foreground #0000FF80, background #FFFFFF$"
check "15: ... back to the brush again" "15 picker" "^after blue: picker off brush on$"
check "15: a right-click on transparent green makes it the background" "15 picker" "^clear: Foreground #0000FF80, background #00FF0000$"
check "15: ... back to the brush" "15 picker" "^after clear: picker off brush on$"
check "15: picking changes no pixel: the title stays unmarked" "15 picker" "^title: t\.png$"
# ETCH-11: line, rectangle, and ellipse with previews.
check "16: the shapes' style is hidden while the pencil is the tool" "16 shapes" "^start: style hidden$"
check "16: R picks the rectangle" "16 shapes" "^key r: rectangle on pencil off$"
check "16: ... showing its style, Outline on" "16 shapes" "^style: outline on filled off both off$"
check "16: ... at size 1" "16 shapes" "^rectangle size: 1$"
check "16: a held drag previews the rectangle (5 px band, white inside), the image untouched" "16 shapes" "^rect held: Untitled band all inside all$"
check "16: the release lands it as previewed, marking the title" "16 shapes" "^rect drawn: • Untitled same yes$"
check "16: one Ctrl+Z removes it entirely" "16 shapes" "^rect undone: Untitled ink 0$"
check "16: O picks the ellipse" "16 shapes" "^key o: ellipse on rectangle off$"
check "16: Both turns on" "16 shapes" "^pressed: outline off both on$"
check "16: a held drag previews the ellipse filled red (its box's corner white), the image untouched" "16 shapes" "^ellipse held: Untitled inside all corner all$"
check "16: the release lands it as previewed" "16 shapes" "^ellipse drawn: • Untitled inside all same yes$"
check "16: one Ctrl+Z takes it all back" "16 shapes" "^ellipse undone: Untitled ink 0 inside all$"
check "16: L picks the line, which has no style" "16 shapes" "^key l: line on style hidden$"
check "16: a held drag previews the line along the pointer's path, the image untouched" "16 shapes" "^line held: Untitled line 41/41$"
check "16: Escape drops it" "16 shapes" "^line escaped: ink 0$"
check "16: ... and the release then draws nothing" "16 shapes" "^line released: Untitled ink 0$"
check "16: a line dragged lands" "16 shapes" "^line drawn: • Untitled line 41/41$"
check "16: ... and Ctrl+Z takes it back" "16 shapes" "^line undone: Untitled ink 0$"
# ETCH-12: text.
check "17: the font controls are hidden while the pencil is the tool" "17 text" "^start: font hidden hidden$"
check "17: T picks text" "17 text" "^key t: text on pencil off$"
check "17: ... showing Bold and Italic, off" "17 text" "^font: bold off italic off$"
check "17: ... at size 24" "17 text" "^text size: 24$"
check "17: typing shows the text on the canvas, the image untouched" "17 text" "^typed: Untitled 1024×768 image, typing “Hello”$"
check "17: ... dark text in the box" "17 text" "^typed ink: black [1-9][0-9]{2,} width [1-9][0-9]+$"
check "17: tool keys typed go into the text and switch no tool" "17 text" "^letters: text on pencil off eraser off 1024×768 image, typing “Hello pebfilrostx”$"
check "17: ... nor swap the colours" "17 text" "^letters: Foreground #000000, background #FFFFFF$"
check "17: Enter puts it on the image, marking the title" "17 text" "^committed: • Untitled 1024×768 image$"
absent "17: ... ink and all" "17 text" "^committed ink: 0$"
check "17: one Ctrl+Z takes it all back" "17 text" "^text undone: Untitled ink 0$"
check "17: another click starts new text" "17 text" "^gone: 1024×768 image, typing “Gone”$"
check "17: Escape drops it: no ink, no mark" "17 text" "^escaped: Untitled ink 0 1024×768 image$"
check "17: a click elsewhere puts the text on, starting none there" "17 text" "^clicked away: • Untitled 1024×768 image$"
absent "17: ... ink and all" "17 text" "^clicked away ink: 0$"
check "17: ... and Ctrl+Z takes it back" "17 text" "^click undone: Untitled ink 0$"
check "17: Bold makes the text being typed darker" "17 text" "^bold: on darker yes$"
check "17: DejaVu Sans Mono, picked from the font list, draws it wider than Sans" "17 text" "^mono: wider yes$"
check "17: ... and typing goes on into it" "17 text" "^mono typed: 1024×768 image, typing “illicit fillip!”$"
check "17: Escape drops it" "17 text" "^font escaped: Untitled ink 0 1024×768 image$"
check "17: an input method's composition shows in new text, nothing typed yet" "17 text" "^composing: 1024×768 image, typing “” black [1-9][0-9]+$"
check "17: ... and its commit is typed" "17 text" "^composed: 1024×768 image, typing “仮名”$"
check "17: Enter puts it on the image" "17 text" "^ime on: • Untitled 1024×768 image$"
absent "17: ... ink and all" "17 text" "^ime ink: 0$"
check "18: New Image offers White" "18 new" "^dialog: White$"
check "18: ... Transparent" "18 new" "^dialog: Transparent$"
check "18: ... and the background colour" "18 new" "^dialog: Background colour$"
check "18: ... described as the window's background, white" "18 new" "^dialog: #FFFFFF$"
check "18: ... White on at first" "18 new" "^dialog checked: White$"
check "18: Alt+T picks Transparent" "18 new" "^picked: Transparent$"
check "18: a transparent 300×200 image opens in Untitled 2" "18 new" "^new: Untitled 2$"
check "18: ... 300×200" "18 new" "^new: 300×200$"
check "18: ... showing the checkerboard everywhere" "18 new" "^transparent: checker all light 1 grey 1$"
check "18: ... and saves as all alpha 0" "18 new" "^saved: 300x200 alpha True max 0$"
check "18: the settings file holds the size and background" "18 new" "^settings: new-width=300 new-height=200 new-background=transparent $"
check "18: after a restart New Image offers 300" "18 new" "^again: 300$"
check "18: ... by 200" "18 new" "^again: 200$"
check "18: ... Transparent" "18 new" "^again checked: Transparent$"
check "19: Tolerance hides while the pencil is the tool" "19 tolerance" "^pencil: tolerance 0$"
check "19: ... and shows beside the fill, 0 at first" "19 tolerance" "^fill: on tolerance 0$"
check "19: at 0 a fill takes the white half and not the half six levels off" "19 tolerance" "^at 0: left all right 0/"
check "19: ... nor at 5" "19 tolerance" "^at 5: left all right 0/"
check "19: at 6 it takes both" "19 tolerance" "^at 6: left all right all$"
check "19: the fill keeps its tolerance across tools" "19 tolerance" "^kept: 6$"
check "20: S picks the selection" "20 selection" "^key s: selection on pencil off$"
check "20: ... which has no size" "20 selection" "^sizes shown: 0$"
check "20: a drag selects 100×80, the status saying so" "20 selection" "^selected: Selection: 100×80 t\.png$"
check "20: ... outlined in dashes along its edges" "20 selection" "^outline: top dashed left dashed$"
check "20: a drag inside moves it, still selected, the image untouched until it lands" "20 selection" "^moved: Selection: 100×80 t\.png$"
check "20: ... its old place transparent, its new place red" "20 selection" "^moved: left checker all light 1 grey 1 reached red all$"
check "20: ... outlined there" "20 selection" "^moved outline: dashed$"
check "20: Escape lands it and deselects, marking the title" "20 selection" "^landed: no selection • t\.png$"
check "20: ... as it showed" "20 selection" "^landed: left checker all light 1 grey 1 reached red all$"
check "20: one Ctrl+Z restores the move entirely" "20 selection" "^move undone: t\.png red all clear checker all light 1 grey 1$"
check "20: Ctrl+Y moves it again" "20 selection" "^move redone: • t\.png left checker all light 1 grey 1 reached red all$"
check "20: Shift+Right moves the selection 10 px" "20 selection" "^nudged: Selection: 80×80 left checker all light 1 grey 1 reached all$"
check "20: choosing another tool lands it and deselects" "20 selection" "^nudge landed: no selection • t\.png pencil on$"
check "20: Ctrl+A picks the selection and selects the whole image" "20 selection" "^select all: Selection: 400×300 selection on pencil off$"
check "20: Escape deselects" "20 selection" "^deselected: no selection t\.png$"
check "20: Delete clears the selection to transparency" "20 selection" "^deleted: Selection: 100×80 • t\.png checker all light 1 grey 1$"
check "20: ... and Ctrl+Z gives it back" "20 selection" "^delete undone: t\.png red all$"
check "20: while text is typed Delete deletes a character and Ctrl+A selects nothing" "20 selection" "^typing: 400×300 image, typing “a” no selection text on selection off$"
check "21: Copy offers the selection as image/png" "21 clipboard" "^offered: 1 Selection: 100×80 t\.png$"
check "21: another GTK app reads it as the image, straight alpha" "21 clipboard" "^copied clipboard: 100x80 ff0000ff [0-9a-f]{6}00 0000ff80$"
check "21: Cut clears it to transparency, marking the title" "21 clipboard" "^cut: Selection: 100×80 • t\.png checker all light 1 grey 1$"
check "21: ... and the other app reads the same image" "21 clipboard" "^cut clipboard: 100x80 ff0000ff [0-9a-f]{6}00 0000ff80$"
check "21: one Ctrl+Z undoes the cut" "21 clipboard" "^cut undone: t\.png red all$"
check "21: the other app puts a 60×40 image on the clipboard" "21 clipboard" "^other set: 60x40$"
check "21: an image the other app copied pastes as a floating selection, unmarked" "21 clipboard" "^pasted: Selection: 60×40 t\.png selection on blue all$"
check "21: Ctrl+Z drops it while it floats" "21 clipboard" "^dropped: no selection t\.png red all$"
check "21: dragged, it moves, the image untouched" "21 clipboard" "^paste moved: Selection: 60×40 t\.png left red all blue all$"
check "21: Escape lands it, marking the title" "21 clipboard" "^paste landed: no selection • t\.png$"
check "21: ... exactly: opaque blue, blue at alpha 128, red beside" "21 clipboard" "^landed clipboard: 400x300 0000ffff 0000ff80 ff0000ff$"
check "21: one Ctrl+Z takes the paste back" "21 clipboard" "^paste undone: t\.png red all$"
check "21: ... and Ctrl+Y lands it again" "21 clipboard" "^paste redone: • t\.png blue all$"
check "21: while text is typed Ctrl+V pastes nothing" "21 clipboard" "^typing: 400×300 image, typing “ab” no selection$"
check "22: Crop to Selection does nothing without a selection" "22 crop" "^unselected: 400×300 crop\.png$"
check "22: a 100×100 selection" "22 crop" "^selected: Selection: 100×100$"
check "22: its C crops to the selection: 100×100, no selection, marked" "22 crop" "^cropped: 100×100 no selection • crop\.png$"
check "22: ... the selection's red half on the left" "22 crop" "^cropped: red all$"
check "22: ... its transparent quarter top right" "22 crop" "^cropped: checker all light 1 grey 1 green 0$"
check "22: ... its half-transparent blue bottom right" "22 crop" "^cropped: blended all$"
check "22: the cropped image saves 100x100 with the pixels exact" "22 crop" "^saved: 100x100 ff0000ff [0-9a-f]{6}00 0000ff80$"
check "22: one Ctrl+Z gives back 400×300" "22 crop" "^crop undone: 400×300 • crop\.png$"
check "22: ... with every pixel as it was: red" "22 crop" "^crop undone: red all$"
check "22: ... transparent" "22 crop" "^crop undone: checker all light 1 grey 1 green 0$"
check "22: ... and half-transparent blue" "22 crop" "^crop undone: blended all$"
check "22: Ctrl+Y crops again (the file's image: unmarked)" "22 crop" "^crop redone: 100×100 crop\.png$"
check "22: floating pixels land, then the image is cropped to them" "22 crop" "^float cropped: 100×80 no selection red all$"
check "22: ... and one Ctrl+Z gives back 400×300 with them landed" "22 crop" "^float crop undone: 400×300 red all left checker all light 1 grey 1$"
check "23: Ctrl+R asks Resize Image" "23 resize" "^dialog: Resize Image$"
check "23: ... with Keep aspect ratio" "23 resize" "^dialog: Keep aspect ratio$"
check "23: ... Smooth" "23 resize" "^dialog: Smooth$"
check "23: ... and Nearest neighbour" "23 resize" "^dialog: Nearest neighbour$"
check "23: ... offering the image's 400 by 300, aspect kept and Smooth on" "23 resize" "^dialog: width 400 height 300 checked Keep aspect ratio, Smooth$"
check "23: Escape closes it, changing nothing" "23 resize" "^cancelled: 400×300 resize\.png 0$"
check "23: a width of 600 sets the height to 450" "23 resize" "^linked: width 600 height 450$"
check "23: Alt+N picks Nearest neighbour" "23 resize" "^picked: Keep aspect ratio, Nearest neighbour$"
check "23: Enter resizes to 600×450, marked" "23 resize" "^nearest: 600×450 no selection • resize\.png$"
check "23: ... the red half on the left" "23 resize" "^nearest: red all$"
check "23: ... the transparent quarter top right" "23 resize" "^nearest: checker all light 1 grey 1 green 0$"
check "23: ... the half-transparent quarter bottom right" "23 resize" "^nearest: blended all$"
check "23: nearest copies pixels: the halves meet exactly, hidden colour kept" "23 resize" "^nearest saved: 600x450 ff0000ff 00ff0000 0000ff80 0000ff80$"
check "23: one Ctrl+Z gives back 400×300" "23 resize" "^resize undone: 400×300 • resize\.png$"
check "23: ... red" "23 resize" "^resize undone: red all$"
check "23: ... transparent" "23 resize" "^resize undone: checker all light 1 grey 1 green 0$"
check "23: ... and blue, as they were" "23 resize" "^resize undone: blended all$"
check "23: Smooth to 200 wide makes 200×150" "23 resize" "^smooth: 200×150 • resize\.png$"
check "23: ... its red edge fading in alpha, never tinted by the transparent green" "23 resize" "^smooth saved: 200x150 ff0000ff ff0000df ff000020$"
check "23: ... and Ctrl+Z gives back 400×300" "23 resize" "^smooth undone: 400×300$"
check "24: Ctrl+Shift+R asks Resize Canvas" "24 canvas" "^dialog: Resize Canvas$"
check "24: ... with the anchor's buttons, named" "24 canvas" "^dialog: Bottom right$"
check "24: ... and the new area Transparent" "24 canvas" "^dialog: Transparent$"
check "24: ... or the background colour" "24 canvas" "^dialog: Background colour$"
check "24: ... offering 400 by 300, the centre anchored, Transparent on" "24 canvas" "^dialog: width 400 height 300 anchors off off off off on off off off off checked Transparent$"
check "24: pressing Top left anchors there alone" "24 canvas" "^pressed: anchors on off off off off off off off off$"
check "24: 600 by 400 makes 600×400, marked" "24 canvas" "^top left: 600×400 • canvas\.png$"
check "24: ... the image at the top left, the new area right and below transparent" "24 canvas" "^top left: red all right checker all light 1 grey 1 below checker all light 1 grey 1$"
check "24: ... saved: the old pixels at 0,0, the new area transparent" "24 canvas" "^top left saved: 600x400 ff0000ff 0000ff80 00000000 00000000 00000000$"
check "24: one Ctrl+Z gives back 400×300" "24 canvas" "^canvas undone: 400×300 • canvas\.png$"
check "24: ... red" "24 canvas" "^canvas undone: red all$"
check "24: ... transparent" "24 canvas" "^canvas undone: checker all light 1 grey 1 green 0$"
check "24: ... and blue, as they were" "24 canvas" "^canvas undone: blended all$"
check "24: Alt+G picks the background colour for the new area" "24 canvas" "^background: Background colour$"
check "24: Bottom right puts the old pixels at 100,100, the new area indigo" "24 canvas" "^bottom right saved: 500x400 3f48ccff 3f48ccff ff0000ff 0000ff80$"
check "24: shrinking to 300 by 200 makes 300×200" "24 canvas" "^shrunk: 300×200$"
check "24: ... cropping 50 px off each side about the centre" "24 canvas" "^shrunk saved: 300x200 ff0000ff ff0000ff 00ff0000 0000ff80$"
check "24: Ctrl+Z gives back 400×300" "24 canvas" "^shrink undone: 400×300$"
check "24: ... red" "24 canvas" "^shrink undone: red all$"
check "24: ... transparent" "24 canvas" "^shrink undone: checker all light 1 grey 1 green 0$"
check "24: ... and blue" "24 canvas" "^shrink undone: blended all$"
check "25: Ctrl+H turns it 90° right: 300×400, marked" "25 rotate" "^right: 300×400 • rot\.png$"
check "25: ... the red half on top, the checkerboard bottom right" "25 rotate" "^right: red all clear checker all light 1 grey 1$"
check "25: ... saved with the corners turned" "25 rotate" "^right saved: 300x400 ff0000ff 0000ff80 00ff0000$"
check "25: one Ctrl+Z gives back 400×300" "25 rotate" "^right undone: 400×300$"
check "25: ... red" "25 rotate" "^right undone: red all$"
check "25: ... transparent" "25 rotate" "^right undone: checker all light 1 grey 1 green 0$"
check "25: ... and blue, as they were" "25 rotate" "^right undone: blended all$"
check "25: the Image menu's V flips it vertically" "25 rotate" "^vertical: 400×300 400x300 ff0000ff 0000ff80 00ff0000$"
check "25: ... and Ctrl+Z undoes it" "25 rotate" "^vertical undone: 400×300$"
check "25: Ctrl+G turns it 90° left" "25 rotate" "^left: 300×400 300x400 00ff0000 ff0000ff 0000ff80$"
check "25: ... and Ctrl+Z undoes it" "25 rotate" "^left undone: 400×300$"
check "25: Ctrl+J turns it 180°" "25 rotate" "^half: 400×300 400x300 0000ff80 ff0000ff 00ff0000$"
check "25: ... and Ctrl+Z undoes it" "25 rotate" "^half undone: 400×300$"
check "25: the Image menu's H flips it horizontally" "25 rotate" "^horizontal: 400×300 400x300 00ff0000 0000ff80 ff0000ff$"
check "25: ... and Ctrl+Z undoes it" "25 rotate" "^horizontal undone: 400×300$"
check "25: ... every region as it was: red" "25 rotate" "^horizontal undone: red all$"
check "25: ... transparent" "25 rotate" "^horizontal undone: checker all light 1 grey 1 green 0$"
check "25: ... and blue" "25 rotate" "^horizontal undone: blended all$"
check "25: four quarter turns give back t.png exactly" "25 rotate" "^four turns: 400×300 rot\.png same$"
check "25: one Ctrl+Z after them leaves three turns" "25 rotate" "^three turns: 300×400 • rot\.png$"
check "26: etch t.png opens one window" "26 drop" "^before: windows: t\.png$"
check "26: two files dropped open in windows of their own" "26 drop" "^dropped: windows: one\.png, t\.png, two\.jpg$"
check "26: ... none of them asking to come forward yet" "26 drop" "^dropped: urgent: $"
check "26: dropping an open file opens no other window" "26 drop" "^again: windows: one\.png, t\.png, two\.jpg$"
check "26: ... and asks for its own to come forward" "26 drop" "^again: urgent: t\.png$"
check "26: a dropped text file alerts naming it" "26 drop" "^alert: Cannot open “notes\.png”$"
check "26: ... and why" "26 drop" "^alert: It isn't an image Etch can open: PNG, JPEG, or BMP\.$"
check "26: closing the alert leaves the three windows" "26 drop" "^after alert: windows: one\.png, t\.png, two\.jpg$"
check "26: ... and the failure is logged with the reason" "26 drop" "^log: etch: cannot open /tmp/notes\.png: It isn't an image"
absent "26: ... and nothing else goes wrong" "26 drop" "^log: .*(panicked|CRITICAL)"
check "27: images opened in four runs are listed newest first, once each" "27 recent" "^opened: r1\.png r3\.png r2\.png$"
check "27: an image saved goes on top" "27 recent" "^saved: r4\.png r1\.png r3\.png r2\.png$"
check "27: the next run's File → Open Recent lists them in that order" "27 recent" "^menu: r4\.png r1\.png r3\.png r2\.png$"
check "27: ... each with its folder" "27 recent" "^menu item: r4\.png  \(/tmp\)$"
check "27: choosing one opens it" "27 recent" "^chosen: windows: Untitled, r4\.png$"
check "27: choosing a deleted one alerts naming it" "27 recent" "^missing: Cannot open “r2\.png”$"
check "27: ... and why" "27 recent" "^missing: The file doesn't exist\.$"
check "27: ... opening no window" "27 recent" "^missing: windows: Untitled, r4\.png$"
check "27: ... and it leaves the list" "27 recent" "^missing: r4\.png r1\.png r3\.png$"
check "27: ... and the menu" "27 recent" "^after missing: r4\.png r1\.png r3\.png$"
check "27: the failure is logged" "27 recent" "^log: etch: cannot open /tmp/r2\.png: The file doesn.t exist\.$"
absent "27: ... and nothing else goes wrong" "27 recent" "^log: .*(panicked|CRITICAL)"
check "28: the hex field shows the foreground" "28 colours" "^start: #000000$"
check "28: typing #3366CC sets the foreground" "28 colours" "^typed: Foreground #3366CC, background #FFFFFF$"
check "28: ... the field showing it, valid" "28 colours" "^typed: #3366CC valid$"
check "28: #RGB doubles each digit" "28 colours" "^short: Foreground #FF8800, background #FFFFFF$"
check "28: #RRGGBBAA sets alpha too" "28 colours" "^alpha: Foreground #3366CC80, background #FFFFFF$"
check "28: tool keys and X typed in the field are text, switching no tool" "28 colours" "^keys: pebfilrostx tools Pencil$"
check "28: ... rejected: the colour stays" "28 colours" "^invalid: Foreground #3366CC80, background #FFFFFF$"
check "28: ... the field marked invalid, the pencil still the tool" "28 colours" "^invalid: invalid tools Pencil$"
check "28: five digits are rejected too" "28 colours" "^five: Foreground #3366CC80, background #FFFFFF$"
check "28: leaving the field shows the colour again" "28 colours" "^left: #3366CC80 valid$"
check "28: Custom… opens the chooser for the foreground" "28 colours" "^chooser: Foreground Colour$"
check "28: ... Cancel changes nothing" "28 colours" "^cancelled: Foreground #3366CC80, background #FFFFFF$"
check "28: the chooser's colour, alpha included, sets the foreground" "28 colours" "^chosen: Foreground #20A04040, background #FFFFFF$"
check "28: ... and the field" "28 colours" "^chosen: #20A04040$"
check "28: a right-click on Custom… chooses the background" "28 colours" "^right: Background Colour$"
check "28: ... which takes the colour chosen" "28 colours" "^background: Foreground #20A04040, background #FFCC00$"
for kt in p:Pencil b:Brush e:Eraser f:Fill i:"Colour Picker" t:Text r:Rectangle l:Line s:Selection o:Ellipse; do
  check "29: ${kt%%:*} picks the ${kt#*:} tool" "29 keys" "^key ${kt%%:*}: ${kt#*:}$"
done
check "29: X swaps the colours" "29 keys" "^swapped: Foreground #FFFFFF, background #000000$"
check "29: ... and again back" "29 keys" "^back: Foreground #000000, background #FFFFFF$"
check "29: tool keys and X typed into text are its letters" "29 keys" "^typing: Text .*typing “pebfilrostx”$"
check "29: ... swapping no colours" "29 keys" "^typing: Foreground #000000, background #FFFFFF$"
check "29: ... nor in the size field" "29 keys" "^size: Pencil$"
check "29: ... swapping no colours there" "29 keys" "^size: Foreground #000000, background #FFFFFF$"
check "29: back on the canvas, B picks the brush again" "29 keys" "^after size: Brush$"
check "29: ... nor in the tolerance" "29 keys" "^tolerance: Fill$"
check "29: ... nor in the hex field" "29 keys" "^hex: Fill$"
check "29: ... nor in the font list search" "29 keys" "^font search: Text$"
absent "29: ... swapping no colours in any of them" "29 keys" "^(tolerance|hex|font): Foreground #FFFFFF"
check "29: in an open menu a tool key is the item's (Alt+V, F fits)" "29 keys" "^menu: Eraser [0-9.]+%$"
absent "29: ... the zoom no longer 100%" "29 keys" "^menu: .* 100%$"
# ETCH-25: crash recovery.
check "30: a stroke across k.png's red half marks it" "30 recovery" "^drawn: • k\.png black all$"
check "30: its journal appears 5 s after the press (0.5 s into the drag), not before" "30 recovery" "^journal after (5\.[5-9]|6\.[0-9]) s$"
check "30: killed, the journal stays and the file is unchanged" "30 recovery" "^killed: 1 journals, file unchanged$"
check "30: etch k.png asks Restore unsaved edits?" "30 recovery" "^asked: Restore unsaved edits\?$"
check "30: ... saying when Etch closed unexpectedly" "30 recovery" "^asked: Etch closed unexpectedly at [0-9]{2}:[0-9]{2} while “k\.png” had unsaved edits\.$"
check "30: ... with Discard" "30 recovery" "^asked: Discard$"
check "30: ... and Restore" "30 recovery" "^asked: Restore$"
check "30: Restore shows the stroke, marked, the file unchanged, no crashed journal" "30 recovery" "^restored: • k\.png black all file unchanged 0 journals$"
check "30: one Ctrl+Z gives back the file's pixels, unmarked" "30 recovery" "^undone: k\.png red all$"
check "30: redone and killed, a journal" "30 recovery" "^again: 1 journals$"
check "30: etch alone opens k.png" "30 recovery" "^alone: k\.png$"
check "30: ... and asks" "30 recovery" "^alone: Restore unsaved edits\?$"
check "30: Discard leaves the file's pixels and no journal" "30 recovery" "^discarded: k\.png red all 0 journals$"
check "30: an edit is journaled" "30 recovery" "^edited: 1 journals$"
check "30: undoing back to the file removes the journal" "30 recovery" "^undone to the file: k\.png 0 journals$"
check "30: redone, journaled again" "30 recovery" "^redone: 1 journals$"
check "30: closing with Discard leaves no journal and the file unchanged" "30 recovery" "^closed: exited, 0 journals, file unchanged$"
check "30: a file changed after the crash says so" "30 recovery" "^changed: “k\.png” changed after the crash$"
check "30: ... offering Keep for Later" "30 recovery" "^changed: Keep for Later$"
check "30: Keep for Later shows the file and keeps the journal" "30 recovery" "^kept: k\.png red all 1 journals$"
check "30: an Untitled image drawn on" "30 recovery" "^untitled: • Untitled ink [1-9][0-9]*$"
check "30: ... killed, its journal stays" "30 recovery" "^untitled killed: 1 journals$"
check "30: etch alone opens an Untitled image" "30 recovery" "^untitled-asked: Untitled$"
check "30: ... 1024×768" "30 recovery" "^untitled-asked: 1024×768$"
check "30: ... asking to restore an untitled image" "30 recovery" "^untitled-asked: .*while an untitled image had unsaved edits\.$"
check "30: Restore shows as much ink as was drawn, marked" "30 recovery" "^untitled restored: • Untitled ink yes$"
check "30: one Ctrl+Z takes it all back" "30 recovery" "^untitled undone: Untitled ink 0$"
check "30: journals are written off the UI thread, logged" "30 recovery" "^log: journaled .* bytes written in [0-9]+ ms$"
absent "30: no warnings, errors, or panics" "30 recovery" "^log: .*(WARN|ERROR|panicked|CRITICAL)"
for s in 1 2 1.5; do
  # The transparent images at 100%: their rectangles in device pixels (401 x 1.5 rounds).
  case $s in
    1) small=400x300 odd=401x301 ;;
    2) small=800x600 odd=802x602 ;;
    1.5) small=600x450 odd="60[12]x45[12]" ;;
  esac
  for r in default cairo; do
    w="31: scale $s, $r renderer:"
    check "$w the renderer is $r" "31 seams" "^s$s $r renderer: $([ $r = cairo ] && echo GskCairoRenderer || echo "Gsk(GL|Vulkan)Renderer")$"
    check "$w an opaque image over 12 tiles shows only its colour at 100%" "31 seams" "^s$s $r 100 100%: [0-9]+x[0-9]+ off 0$"
    check "$w ... at Fit" "31 seams" "^s$s $r fit [0-9.]+%: [0-9]+x[0-9]+ off 0$"
    check "$w ... at 50%" "31 seams" "^s$s $r out 50%: [0-9]+x[0-9]+ off 0$"
    check "$w ... at 200%" "31 seams" "^s$s $r in 200%: [0-9]+x[0-9]+ off 0$"
    check "$w ... and a smaller one at Fit, above 100%" "31 seams" "^s$s $r small 1[0-9]{2}(\.[0-9]+)?%: [0-9]+x[0-9]+ off 0$"
    check "$w the checkerboard of 400x300 at 100% is crisp to its edges" "31 seams" "^s$s $r 400x300 100%: $small off 0 framed$"
    check "$w ... and of 401x301 (the other half-pixel offset)" "31 seams" "^s$s $r 401x301 100%: $odd off 0 framed$"
    check "$w ... and of 1501x1001 fitted" "31 seams" "^s$s $r 1501x1001 [0-9.]+%: [0-9]+x[0-9]+ off 0 framed$"
  done
done
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
