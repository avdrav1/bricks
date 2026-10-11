#!/usr/bin/env bash
# SLATE-4 acceptance: the viewer window opens, shows, and refuses documents.
#
#   scripts/check_slate.sh [BINARY]   (default target/release/slate)
#
# In an Arch container under headless sway. Text is read through the accessibility tree
# (AT-SPI), screenshots are measured pixel by pixel (python-gobject's GdkPixbuf), and
# windows are found by title (swaymsg).
#   1. `slate w.txt` (Windows-1252, CR line ends): the window is titled w.txt and the page's
#      accessible text equals the file's lines exactly;
#   2. `slate fixture.md`: titled fixture.md; headings, emphasis, lists, task boxes, code,
#      the table's cells, the link, and the picture (by its alt text) are in the accessible
#      text;
#   3. `slate fixture.docx`: every paragraph LibreOffice reads from it (its committed dump)
#      is in the accessible text, table cells included;
#   4. headings draw larger and bold text bolder: in a screenshot of `# HHHHHHHH`,
#      `HHHHHHHH`, `**HHHHHHHH**`, the heading's band of ink is at least 1.5 times as tall
#      as the plain line's, and the bold line has at least 1.15 times its ink;
#   5. Ctrl+O on the start window, a path typed into the file dialog, Enter: a window
#      titled fixture.md opens;
#   6. a .docx dragged onto a document window (wlr-virtual-pointer, as check_drop.sh) opens
#      in a window of its own;
#   7. a file that isn't a Word document, named .docx, and a .doc each alert with the file's
#      name and the reason ("Save it as .docx first" for the .doc); after Close the window
#      is gone and slate has exited.
# Needs docker. VERBOSE=1 prints the transcript; SHOTS=DIR saves screenshots.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(realpath "${1:-target/release/slate}")
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p slate)"; exit 1; }
model=$(realpath apps/slate/crates/document-model/tests/data)
docx=$(realpath apps/slate/crates/docx/tests/data)

shots=()
if [ -n "${SHOTS:-}" ]; then mkdir -p "$SHOTS"; shots=(-v "$(realpath "$SHOTS"):/shots"); fi
out=$(docker run --rm --cap-add SYS_NICE -v "$bin:/usr/local/bin/slate:ro" \
  -v "$model:/model:ro" -v "$docx:/docx:ro" -v "$PWD/scripts/dnd_helpers.py:/dnd.py:ro" "${shots[@]}" \
  archlinux:latest bash -c '
set -uo pipefail
pacman -Syu --noconfirm --needed sway wtype grim gtk4 ttf-dejavu dbus python-gobject at-spi2-core \
  python-pywayland wlr-protocols wayland pkgconf jq >/dev/null 2>&1
mkdir -p /shots /tmp/rt /tmp/md /tmp/proto && chmod 700 /tmp/rt && touch /tmp/proto/__init__.py
python -m pywayland.scanner -i /usr/share/wayland/wayland.xml \
  /usr/share/wlr-protocols/unstable/wlr-virtual-pointer-unstable-v1.xml -o /tmp/proto/wlr_proto >/dev/null
export PYTHONPATH=/tmp/proto XDG_RUNTIME_DIR=/tmp/rt GDK_DEBUG=no-portals XDG_STATE_HOME=/tmp/state GDK_BACKEND=wayland
printf "output HEADLESS-1 resolution 1280x720 position 0 0\ndefault_border none\n" > /tmp/sway.conf
cp /model/windows1252-cr.txt /tmp/w.txt
cp /model/fixture.md /model/picture.png /tmp/md/
cp /docx/fixture.docx /tmp/fixture.docx; cp /docx/fixture.docx /tmp/dropped.docx
cp /docx/poi/IllustrativeCases.docx /docx/poi/Numbering.docx /tmp/
printf "not a zip at all\n" > /tmp/bad.docx
printf "\320\317\021\340\241\261\032\341old" > /tmp/old.doc
printf "# HHHHHHHH\n\nHHHHHHHH\n\n**HHHHHHHH**\n" > /tmp/looks.md
cat > /tmp/md.expected <<"EOF"
Slate Markdown fixture
Second level heading
Sixth level heading
A block quote with bold inside.
First bullet
Third level bullet
Nested number
Seven starts this list
An open task
A done task
fn main() {
indented code block
Apples
Pears
Name
A blue picture
across a soft break.
The last paragraph.
EOF
# Every paragraph line LibreOffice reads from the .docx fixture.
python3 - > /tmp/docx.expected <<"PY"
import json
for line in open("/docx/fixture-libreoffice.txt", encoding="utf-8"):
    f = line.rstrip("\n").split("\t")
    if f[0] == "para":
        for part in json.loads(f[4]).split("\n"):
            if part.strip():
                print(part)
PY
cat > /tmp/a11y.py <<"PY"
import sys
import gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi

def app():
    d = Atspi.get_desktop(0)
    for i in range(d.get_child_count()):
        a = d.get_child_at_index(i)
        if a is not None and a.get_name() == "slate":
            return a
    return None

def walk(node, depth=0):
    if node is None or depth > 80:
        return
    node.clear_cache()
    yield node
    for i in range(node.get_child_count()):
        yield from walk(node.get_child_at_index(i), depth + 1)

def texts():
    a = app()
    out = []
    for node in walk(a):
        for t in (node.get_name(), node.get_description()):
            if t:
                out.append(t)
        ti = node.get_text_iface()
        if ti is not None:
            # To the end (-1): GTK counts embedded tables and pictures as characters but
            # leaves them out of the text, so a range of get_character_count() falls short.
            t = Atspi.Text.get_text(ti, 0, -1)
            if t:
                out.append(t)
    return out

cmd = sys.argv[1]
all_texts = texts()
if cmd == "dump":
    for t in all_texts:
        print(t.replace("\n", "\\n")[:300])
elif cmd == "equals":
    raw = open(sys.argv[2], encoding=sys.argv[3], newline="").read()
    want = "\n".join(raw.splitlines())
    print("text equals the file" if want in all_texts else "text differs from the file")
elif cmd == "contains":
    big = "\n".join(all_texts)
    want = [l.rstrip("\n") for l in open(sys.argv[2], encoding="utf-8") if l.strip()]
    missing = [w for w in want if w not in big]
    print(f"found {len(want) - len(missing)} of {len(want)}")
    for m in missing:
        print(f"missing: {m}")
PY
cat > /tmp/px.py <<"PY"
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf
p = GdkPixbuf.Pixbuf.new_from_file("/shots/looks.png")
w, h, rs, n, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
bands, cur = [], None
for y in range(70, h - 50):
    ink = 0
    for x in range(w):
        i = y * rs + x * n
        if px[i] + px[i + 1] + px[i + 2] < 300:
            ink += 1
    if ink:
        if cur is None:
            cur = [y, y, 0]
        cur[1], cur[2] = y, cur[2] + ink
    elif cur is not None:
        bands.append(cur)
        cur = None
print("bands: " + " ".join(f"{b[0]}-{b[1]}:{b[2]}" for b in bands))
if len(bands) >= 3:
    height = lambda b: b[1] - b[0] + 1
    print(f"heading height: {height(bands[0]) * 100 // height(bands[1])}")
    print(f"bold ink: {bands[2][2] * 100 // bands[1][2]}")
PY
session() {
  WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway --unsupported-gpu -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
  for i in $(seq 50); do ls /tmp/rt/wayland-[0-9] >/dev/null 2>&1 && break; sleep 0.1; done
  export WAYLAND_DISPLAY=$(basename $(ls /tmp/rt/wayland-[0-9] | head -1))
  export SWAYSOCK=$(ls /tmp/rt/sway-ipc.*.sock | head -1)
  shot() { grim /shots/$1.png 2>/dev/null || true; }
  keys() { wtype -s 400 "$@" -s 400; }
  titles() { echo "titles: $(swaymsg -t get_tree | jq -r ".. | objects | select(.type==\"con\" and .name!=null) | .name" | sort | tr "\n" ",")"; }
  a11y() { python3 /tmp/a11y.py "$@" 2>&1; }
  quit() { pkill -x slate; sleep 1; }

  echo "== 1 txt"
  slate /tmp/w.txt >>/tmp/app.log 2>&1 & sleep 4
  titles; a11y equals /tmp/w.txt cp1252; shot txt; quit
  echo "== 2 md"
  slate /tmp/md/fixture.md >>/tmp/app.log 2>&1 & sleep 4
  titles; a11y contains /tmp/md.expected; shot md
  keys -k Page_Down -k Page_Down; sleep 1; shot md2; quit
  echo "== 3 docx"
  slate /tmp/fixture.docx >>/tmp/app.log 2>&1 & sleep 4
  titles; a11y contains /tmp/docx.expected; shot docx
  keys -k Page_Down -k Page_Down; sleep 1; shot docx2; quit
  slate /tmp/IllustrativeCases.docx >>/tmp/app.log 2>&1 & sleep 4; shot poi; keys -k Page_Down -k Page_Down; sleep 1; shot poi2; quit
  slate /tmp/Numbering.docx >>/tmp/app.log 2>&1 & sleep 4; shot numbering; quit
  echo "== 4 looks"
  slate /tmp/looks.md >>/tmp/app.log 2>&1 & sleep 4
  shot looks; python3 /tmp/px.py; quit
  echo "== 5 open"
  slate >>/tmp/app.log 2>&1 & sleep 3
  titles
  keys -M ctrl o -m ctrl; sleep 2; shot dialog
  keys -M ctrl l -m ctrl; wtype -d 40 /tmp/md/fixture.md; sleep 1; keys -k Return; sleep 4
  titles; quit
  echo "== 6 drop"
  slate /tmp/w.txt >>/tmp/app.log 2>&1 & sleep 3
  python /dnd.py source /tmp/dropped.docx >/dev/null 2>&1 & src=$!; sleep 3
  titles
  python /dnd.py drag 960 360 320 360; sleep 4
  titles; shot drop; kill $src; quit
  echo "== 7 bad"
  for f in bad.docx old.doc; do
    slate /tmp/$f >>/tmp/app.log 2>&1 & sleep 4
    titles; a11y dump | grep -E "Cannot open|docx|Save it|readable" | sed "s/^/alert: /"; shot bad-${f%%.*}
    keys -k Return; sleep 2
    pgrep -x slate >/dev/null && echo "after close: slate still running" || echo "after close: slate exited"
    quit
  done
  swaymsg exit >/dev/null 2>&1 || true
}
dbus-run-session -- bash -c "$(declare -f session); session" 2>/tmp/dbus.log
echo "== app log"; grep -E "^slate:|panicked|CRITICAL" /tmp/app.log | head -20
' 2>&1) || true
[ -n "${VERBOSE:-}" ] && sed 's/^/  /' <<<"$out"
section() { awk -v s="== $1" '/^== /{on=($0 ~ "^" s)} on' <<<"$out"; }
fail=0
check() { # WHAT SECTION PATTERN
  if section "$2" | grep -E -- "$3" >/dev/null; then echo "ok   $1"; else echo "FAIL $1 (no match for: $3)"; fail=1; fi
}
at_least() { # WHAT SECTION KEY MIN
  local v
  v=$(section "$2" | sed -n "s/^$3: //p" | head -1)
  if [ -n "$v" ] && [ "$v" -ge "$4" ]; then echo "ok   $1 ($v)"; else echo "FAIL $1 (${v:-nothing}, want >= $4)"; fail=1; fi
}
check "1: the .txt window is titled w.txt" "1 txt" "^titles: .*w\.txt"
check "1: the accessible text equals the Windows-1252 file's lines" "1 txt" "^text equals the file$"
check "2: the .md window is titled fixture.md" "2 md" "^titles: .*fixture\.md"
check "2: every Markdown construct is in the accessible text" "2 md" "^found 18 of 18$"
check "3: the .docx window is titled fixture.docx" "3 docx" "^titles: .*fixture\.docx"
expected=$(python3 -c '
import json
n = 0
for line in open("apps/slate/crates/docx/tests/data/fixture-libreoffice.txt", encoding="utf-8"):
    f = line.rstrip("\n").split("\t")
    if f[0] == "para":
        n += sum(1 for p in json.loads(f[4]).split("\n") if p.strip())
print(n)')
check "3: every paragraph LibreOffice reads is in the accessible text" "3 docx" "^found $expected of $expected$"
at_least "4: a heading draws taller than body text (percent)" "4 looks" "heading height" 150
at_least "4: bold text draws with more ink (percent)" "4 looks" "bold ink" 115
check "5: the start window shows" "5 open" "^titles: .*Slate"
check "5: Ctrl+O and a typed path open fixture.md" "5 open" "^titles: .*fixture\.md"
check "6: a dropped .docx opens in its own window" "6 drop" "^titles: .*dropped\.docx.*w\.txt"
check "7: a bad .docx alerts with its name" "7 bad" "^alert: Cannot open .{1,3}bad\.docx"
check "7: the alert says why" "7 bad" "^alert: .*readable Word document"
check "7: a .doc is told to be saved as .docx" "7 bad" "^alert: .*Save it as \.docx first"
if [ "$(section "7 bad" | grep -c "^after close: slate exited$")" = 2 ]; then
  echo "ok   7: the window closes after the alert"
else
  echo "FAIL 7: the window closes after the alert"; fail=1
fi
if [ $fail = 0 ]; then echo "SLATE OK"; else
  echo "SLATE CHECK FAILED"; [ -n "${VERBOSE:-}" ] || echo "(VERBOSE=1 for the transcript)"
  exit 1
fi
