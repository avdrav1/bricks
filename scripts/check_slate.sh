#!/usr/bin/env bash
# SLATE-4, SLATE-10, SLATE-11, SLATE-6, SLATE-12, and SLATE-13 acceptance: the window
# opens, shows, and refuses documents, its pageless column follows View → Text Width, its
# text selects and copies, it edits with undo, table cells included, and it composes
# through an input method.
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
#   8. col-a.md and col-b.md (a long word that wraps, a rule, a table) in two windows of one
#      slate process and col-c.md in another process, tabbed: each window measured in a
#      screenshot at Medium, then after F10 → Text Width → Narrow, Wide, and Full in col-a's
#      menu: the rule (the column's width exactly) and the widest line of text above it
#      measure 480, 624, 816, and 1232 px (the 1280 px screen less 24 px a side) in all
#      three windows, centred;
#   9. after Ctrl+Q the settings file says full, and slate relaunched shows a Full column;
#  10. Wide in a window resized to 640 px (floating) narrows the column to 592; Narrow
#      there stays 480;
#  11. `slate fixture.docx`: its page break is an accessible object named "Page break";
#  12. Ctrl+A, Ctrl+C in w.txt: the clipboard holds the file's lines as plain text;
#  13. in looks.md, a drag from left of the plain line to the right of the bold line below
#      it, then Ctrl+C: the clipboard holds those two lines;
#  14. in edit.md ("# Title", "First paragraph."), read through AT-SPI after each step:
#      Ctrl+End and " More", Enter and "Second" (typed through the input method), seven
#      Backspaces (the last joins the paragraphs), undo until nothing is left to undo, and
#      redo until nothing is left; then Home, End, Ctrl+Left, Left, Right, Ctrl+Right, Up,
#      and Down, each followed by a typed mark, put the marks where the keys say;
#  15. in fmt.md (three lines of HHHHHHHH and a Z), measured in screenshots with the caret
#      parked on the Z: Ctrl+1 on the first line draws it at least 1.5 times as tall,
#      Ctrl+Shift+8 on the second indents it as a bulleted item, Ctrl+B over the third
#      (selected by dragging) gives it at least 1.15 times the ink; three Ctrl+Z give back
#      the first screenshot's lines exactly and three Ctrl+Shift+Z the formatted ones;
#  16. in table.md ("Before", a 2x2 table a b / c d, "After"), each key followed by a
#      typed digit, read through AT-SPI: End and Down go into cell a, Tab to b and on to
#      c, Right twice from inside c into d, Tab past the last cell out to "After", Up back
#      into c, Shift+Tab back to b; Backspace in b deletes, a second one at the cell's
#      start does nothing, Enter splits b in two; undo to the end gives the opened text
#      and redo to the end the edited one;
#  17. an input method on sway's seat (scripts/ime_helpers.py, through zwp_input_method_v2
#      to the window's text input) composing in ime.md ("Hello world", caret at the end):
#      the preedit かな shows underlined right of the text, the document unchanged
#      (AT-SPI); commit replaces it with 仮名; the input method reads "Hello world仮名"
#      and the caret's byte offset 17 around the caret; deleting 3 bytes before the
#      caret removes 名. Sway sends a window text input only on a focus change, so a
#      second window takes focus and gives it back first;
#  18. selection and clipboard, read back with wl-paste and AT-SPI: in sel.md ("Alpha beta
#      gamma", "Before", a 2x2 table, "After"), Shift+End, Shift+Down, Shift+Down into the
#      table (the selection takes the whole table), and Shift+Ctrl+End each copy what they
#      select as plain text, cells one a line; a double click on "beta" copies the word and
#      a triple click the paragraph; Ctrl+X of the first line, then undo and redo; Ctrl+A,
#      Ctrl+X empties the page and Ctrl+V brings it back, table included (Slate's rich
#      clipboard), and undo empties it again; bold text copied in another Slate process
#      offers Slate's format and plain text, and pastes into target.md still bold (ink at
#      least 1.15 times the plain line's); text copied by another app (wl-copy) pastes as
#      paragraphs, and undo takes the paste back;
#  19. accessibility through Atspi in a11y.md ("Plain **bold** *italic* text", "Second
#      paragraph line"): the page's character count, the caret's offset after Ctrl+End,
#      the character, word, line, and paragraph at an offset, the run attributes of bold
#      (weight 700), italic (style italic), and plain text (weight 400); and the
#      text-changed events a listener receives while "X" is typed, deleted, Enter is
#      pressed, and the Enter undone;
#  20. saving: a Windows-1252 file with CRLF and no final newline, edited and saved with
#      Ctrl+S, keeps all three, is replaced by a rename (new inode, no temp file left),
#      and the title loses its "• " mark; a character Windows-1252 lacks is refused with
#      an alert and the file stays as it was; Save As to as.md writes Markdown in UTF-8;
#      a UTF-16 LE file saves in UTF-16 LE with its BOM; in a Markdown file with setext
#      headings, __bold__, "* " and "1)" lists, saving an edited paragraph changes only
#      its line and undoing it saves the original bytes; closing with changes asks to
#      save, and Enter saves them and closes;
#  21. saving a Word document: fixture.docx with " Edited" typed after its last paragraph
#      and Ctrl+S keeps every zip part but word/document.xml byte for byte, in the same
#      order, and its document.xml holds "The end. Edited"; reopened, the page shows it; a
#      relaunch after a kill leaves one package copy in the cache, the running one's.
#  22. a new document: `slate` alone shows the start window; Ctrl+N opens an Untitled
#      window in its place; a heading and a line typed mark it "• Untitled"; Ctrl+S asks
#      where, in the Documents folder, suggesting Untitled.docx; saved as plans.docx it is
#      a Word document with the heading in Heading 1 and the line, titled plans.docx;
#      Ctrl+N in a document window opens another Untitled window; reopened, plans.docx
#      shows the line.
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
  -v "$model:/model:ro" -v "$docx:/docx:ro" -v "$PWD/scripts/dnd_helpers.py:/dnd.py:ro" \
  -v "$PWD/scripts/ime_helpers.py:/ime.py:ro" -v "$PWD/scripts/protocols:/protocols:ro" "${shots[@]}" \
  archlinux:latest bash -c '
set -uo pipefail
pacman -Syu --noconfirm --needed sway wtype grim gtk4 ttf-dejavu wqy-zenhei dbus python-gobject \
  at-spi2-core python-pywayland wlr-protocols wayland pkgconf jq wl-clipboard >/dev/null 2>&1
mkdir -p /shots /tmp/rt /tmp/md /tmp/proto && chmod 700 /tmp/rt && touch /tmp/proto/__init__.py
python -m pywayland.scanner -i /usr/share/wayland/wayland.xml \
  /usr/share/wlr-protocols/unstable/wlr-virtual-pointer-unstable-v1.xml -o /tmp/proto/wlr_proto >/dev/null
python -m pywayland.scanner -i /usr/share/wayland/wayland.xml \
  /protocols/input-method-unstable-v2.xml -o /tmp/proto/im_proto >/dev/null 2>&1
cat > /tmp/atspi.py <<"PY"
import sys
import gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi

def page(name):
    d = Atspi.get_desktop(0)
    def walk(n, depth=0):
        if n is None or depth > 40:
            return
        yield n
        for i in range(n.get_child_count()):
            yield from walk(n.get_child_at_index(i), depth + 1)
    for i in range(d.get_child_count()):
        a = d.get_child_at_index(i)
        if a is not None and a.get_name() == "slate":
            for n in walk(a):
                if n.get_role_name() == "text" and n.get_name() == name:
                    return n

if sys.argv[1] == "read":
    t = page(sys.argv[2]).get_text_iface()
    print(f"count: {Atspi.Text.get_character_count(t)}")
    print(f"caret: {Atspi.Text.get_caret_offset(t)}")
    G = Atspi.TextGranularity
    for label, offset, g in (("char", 6, G.CHAR), ("word", 7, G.WORD), ("line", 30, G.LINE), ("paragraph", 3, G.PARAGRAPH)):
        r = Atspi.Text.get_string_at_offset(t, offset, g)
        print(f"{label}: {r.content!r} {r.start_offset}-{r.end_offset}")
    for offset in (1, 7, 12):
        attrs, start, end = Atspi.Text.get_attribute_run(t, offset, True)
        weight, style = attrs.get("weight"), attrs.get("style")
        print(f"attrs {offset}: {start}-{end} weight={weight} style={style}")
elif sys.argv[1] == "listen":
    def on_event(e):
        print(f"event: {e.type} {e.detail1} {e.detail2} {e.any_data!r}", flush=True)
    listener = Atspi.EventListener.new(on_event)
    listener.register("object:text-changed:insert")
    listener.register("object:text-changed:delete")
    print("listening", flush=True)
    Atspi.event_main()
PY
cat > /tmp/bytes.py <<"PY"
import sys
data = open(sys.argv[2], "rb").read()
print(f"{sys.argv[1]} bytes: {data!r}")
PY
# The lines that differ between two files ("LABEL-diff: - old", "LABEL-diff: + new") and how
# many, or "LABEL: same bytes" (the image has no diffutils).
cat > /tmp/compare.py <<"PY"
import difflib, sys
label, a, b = sys.argv[1], open(sys.argv[2], "rb").read(), open(sys.argv[3], "rb").read()
if a == b:
    print(f"{label}: same bytes")
else:
    lines = [l for l in difflib.ndiff(a.decode().splitlines(), b.decode().splitlines()) if l[:1] in "+-"]
    for l in lines:
        print(f"{label}-diff: {l}")
    print(f"{label} changed lines: {len(lines)}")
PY
# A saved package against the original: its part names in order, which parts changed, and
# how often TEXT is in its word/document.xml.
cat > /tmp/parts.py <<"PY"
import sys, zipfile
a, b, text = zipfile.ZipFile(sys.argv[1]), zipfile.ZipFile(sys.argv[2]), sys.argv[3]
print("parts: same order" if a.namelist() == b.namelist() else f"parts: {a.namelist()} != {b.namelist()}")
changed = [n for n in a.namelist() if n in b.namelist() and a.read(n) != b.read(n)]
print("changed parts: " + ",".join(changed))
found = b.read("word/document.xml").decode().count(text)
print(f"document.xml has the edit: {found}")
PY
cat > /tmp/words.py <<"PY"
import sys
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf
p = GdkPixbuf.Pixbuf.new_from_file(sys.argv[1])
w, h, rs, n, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
dark = lambda x, y: px[y * rs + x * n] + px[y * rs + x * n + 1] + px[y * rs + x * n + 2] < 300
rows = [y for y in range(60, h - 40) if any(dark(x, y) for x in range(w))]
y0 = rows[0]
y1 = y0
while y1 + 1 in rows:
    y1 += 1
cols = [x for x in range(w) if any(dark(x, y) for y in range(y0, y1 + 1))]
words, start, last = [], cols[0], cols[0]
for x in cols[1:]:
    if x - last > 6:
        words.append((start + last) // 2)
        start = x
    last = x
words.append((start + last) // 2)
print("words: " + str((y0 + y1) // 2) + " " + " ".join(map(str, words)))
PY
cat > /tmp/preedit.py <<"PY"
import sys
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf
def load(name):
    p = GdkPixbuf.Pixbuf.new_from_file(f"/shots/{name}.png")
    w, h, rs, n, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
    return lambda x, y: px[y * rs + x * n] + px[y * rs + x * n + 1] + px[y * rs + x * n + 2] < 450, w // 2, h
# The document window is the left half; its text lines are the bands of ink below the header.
dark, w, h = load("ime-before")
bands, cur = [], None
for y in range(60, h - 40):
    if any(dark(x, y) for x in range(w)):
        cur = [y, y] if cur is None else [cur[0], y]
    elif cur is not None:
        bands.append(cur)
        cur = None
y0, y1 = bands[-1]
right = max(x for y in range(y0, y1 + 1) for x in range(w) if dark(x, y))
dark, _, _ = load("ime-preedit")
ink = sum(1 for y in range(y0 - 4, y1 + 8) for x in range(right + 2, w) if dark(x, y))
longest = 0
for y in range(y0, y1 + 8):
    run = 0
    for x in range(right - 1, w):
        run = run + 1 if dark(x, y) else 0
        longest = max(longest, run)
print(f"preedit ink: {ink}")
print(f"underline: {longest}")
PY
export PYTHONPATH=/tmp/proto XDG_RUNTIME_DIR=/tmp/rt GDK_DEBUG=no-portals XDG_STATE_HOME=/tmp/state GDK_BACKEND=wayland
export XDG_CONFIG_HOME=/tmp/config
printf "output HEADLESS-1 resolution 1280x720 position 0 0\ndefault_border none\n" > /tmp/sway.conf
cp /model/windows1252-cr.txt /tmp/w.txt
cp /model/fixture.md /model/picture.png /tmp/md/
cp /docx/fixture.docx /tmp/fixture.docx; cp /docx/fixture.docx /tmp/dropped.docx
cp /docx/poi/IllustrativeCases.docx /docx/poi/Numbering.docx /tmp/
printf "not a zip at all\n" > /tmp/bad.docx
printf "\320\317\021\340\241\261\032\341old" > /tmp/old.doc
printf "# HHHHHHHH\n\nHHHHHHHH\n\n**HHHHHHHH**\n" > /tmp/looks.md
# A word too long for any line wraps at every width, so its lines are as wide as the column.
python3 -c "print(\"# Column\n\n\" + \"x\" * 800 + \"\n\n---\n\n| a | b |\n|---|---|\n| c | d |\")" > /tmp/col-a.md
cp /tmp/col-a.md /tmp/col-b.md; cp /tmp/col-a.md /tmp/col-c.md
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
        for part in json.loads(f[-1]).split("\n"):
            if part.strip():
                print(part)
PY
cat > /tmp/a11y.py <<"PY"
import sys
import gi
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi

def apps():
    d = Atspi.get_desktop(0)
    for i in range(d.get_child_count()):
        a = d.get_child_at_index(i)
        if a is not None and a.get_name() == "slate":
            yield a

def app():
    return next(apps(), None)

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
elif cmd == "page":
    # The page of the document named argv[2] (its accessible name), else the first.
    name = sys.argv[2] if len(sys.argv) > 2 else None
    pages = (n for a in apps() for n in walk(a) if n.get_role_name() == "text")
    for node in pages:
        ti = node.get_text_iface()
        if ti is not None and name in (None, node.get_name()):
            print("page: " + Atspi.Text.get_text(ti, 0, -1).replace("\n", "|"))
            break
PY
cat > /tmp/fmt.py <<"PY"
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf
def bands(name):
    p = GdkPixbuf.Pixbuf.new_from_file(f"/shots/{name}.png")
    w, h, rs, n, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
    out, cur = [], None
    for y in range(70, h - 50):
        xs = [x for x in range(w) if px[y * rs + x * n] + px[y * rs + x * n + 1] + px[y * rs + x * n + 2] < 300]
        if xs:
            if cur is None:
                cur = [y, y, 0, xs[-1]]
            cur[1], cur[2], cur[3] = y, cur[2] + len(xs), max(cur[3], xs[-1])
        elif cur is not None:
            out.append(cur)
            cur = None
    return out
before, after, undone, redone = (bands(n) for n in ("fmt-before", "fmt-after", "fmt-undone", "fmt-redone"))
print("bands: " + " ".join(f"{len(b)}" for b in (before, after, undone, redone)))
if len(before) >= 3 and len(after) >= 3:
    height = lambda b: b[1] - b[0] + 1
    print(f"heading height: {height(after[0]) * 100 // height(before[0])}")
    print(f"list shift: {after[1][3] - before[1][3]}")
    print(f"bold ink: {after[2][2] * 100 // before[2][2]}")
# The lines measured, not the Z line under them, where the caret is.
print("undo restores: " + ("yes" if undone[:3] == before[:3] else "no"))
print("redo restores: " + ("yes" if redone[:3] == after[:3] else "no"))
PY
cat > /tmp/px.py <<"PY"
import sys
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf
p = GdkPixbuf.Pixbuf.new_from_file(sys.argv[1] if len(sys.argv) > 1 else "/shots/looks.png")
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
    print(f"third ink: {bands[2][2] * 100 // bands[0][2]}")
    print("mids: " + " ".join(str((b[0] + b[1]) // 2) for b in bands[:3]))
PY
cat > /tmp/col.py <<"PY"
import sys
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf
p = GdkPixbuf.Pixbuf.new_from_file(sys.argv[1])
w, h, rs, n, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
# The rule: the longest run of its grey (c0bfbc) on one row; the column is as wide as it.
rule, rule_y, rule_x = 0, None, 0
for y in range(h):
    row = px[y * rs:y * rs + w * n]
    run = 0
    for x in range(w):
        if all(abs(row[x * n + i] - c) <= 6 for i, c in enumerate((0xC0, 0xBF, 0xBC))):
            run += 1
            if run > rule:
                rule, rule_y, rule_x = run, y, x - run + 1
        else:
            run = 0
# The wrapped word just above it: the widest extent of dark ink on a row (text, not the
# black desktop around a floating window).
text = 0
for y in range(max(0, (rule_y or 0) - 60), rule_y or 0):
    row = px[y * rs:y * rs + w * n]
    dark = [x for x in range(w) if 0 < sum(row[x * n:x * n + 3]) < 300]
    if dark:
        text = max(text, dark[-1] - dark[0] + 1)
print(f"rule {rule} text {text} off {rule_x + rule // 2 - w // 2}")
PY
cat > /tmp/copied.py <<"PY"
import subprocess, sys
raw = open(sys.argv[1], encoding=sys.argv[2], newline="").read()
got = subprocess.run(["wl-paste", "-n"], capture_output=True, text=True).stdout
print("copy equals the file" if got == "\n".join(raw.splitlines()) else f"copy differs: {got[:200]!r}")
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
  # Wait up to 20 s for a save to end: no window title marked "• " (changes not saved).
  saved() { for i in $(seq 40); do swaymsg -t get_tree | jq -r ".. | objects | .name? // empty" | grep -q "^• " || return 0; sleep 0.5; done; }
  # Screenshot each column window (focused in turn) and measure it: "LABEL col-X.md: rule ...".
  measure() {
    for t in col-a col-b col-c; do
      [ $# -gt 1 ] && [ $t != $2 ] && continue
      swaymsg "[title=$t] focus" >/dev/null; sleep 1; shot $1-$t
      echo "$1 $t.md: $(python3 /tmp/col.py /shots/$1-$t.png)"
    done
  }
  # F10 in col-a, Text Width (past New, Open, Save, Save As, Close Window), the Nth width
  # (1 Narrow, 2 Medium, 3 Wide, 4 Full).
  text_width() {
    swaymsg "[title=col-a] focus" >/dev/null; sleep 1
    keys -k F10; for i in 1 2 3 4 5; do keys -k Down; done; keys -k Return
    for i in $(seq $1); do keys -k Down; done
    keys -k Return; sleep 1
  }

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
  keys -M ctrl l -m ctrl; keys -d 40 /tmp/md/fixture.md; sleep 1; keys -k Return; sleep 4
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
  echo "== 8 width"
  slate /tmp/col-a.md /tmp/col-b.md >>/tmp/app.log 2>&1 & sleep 4
  slate /tmp/col-c.md >>/tmp/app.log 2>&1 & other=$!; sleep 4
  swaymsg "[title=col-a] focus, layout tabbed" >/dev/null; sleep 1
  titles; measure medium
  text_width 1; measure narrow
  text_width 3; measure wide
  text_width 4; measure full
  kill $other; sleep 1
  echo "== 9 restart"
  swaymsg "[title=col-a] focus" >/dev/null; sleep 1
  keys -M ctrl q -m ctrl; sleep 2
  pgrep -x slate >/dev/null && echo "after quit: slate still running" || echo "after quit: slate exited"
  echo "settings: $(cat /tmp/config/slate/settings 2>/dev/null)"
  slate /tmp/col-a.md >>/tmp/app.log 2>&1 & sleep 4
  measure restart col-a
  echo "== 10 narrow window"
  text_width 3; measure wide1280 col-a
  swaymsg "[title=col-a] floating enable, resize set width 640 px height 600 px, move position center" >/dev/null; sleep 1
  measure wide640 col-a
  text_width 1; measure narrow640 col-a
  quit
  echo "== 11 page break"
  slate /tmp/fixture.docx >>/tmp/app.log 2>&1 & sleep 4
  echo "page breaks: $(a11y dump | grep -c "^Page break$")"; quit
  echo "== 12 select all"
  slate /tmp/w.txt >>/tmp/app.log 2>&1 & sleep 4
  keys -M ctrl a -m ctrl; keys -M ctrl c -m ctrl; sleep 1; shot select-all
  python3 /tmp/copied.py /tmp/w.txt cp1252; quit
  echo "== 13 drag"
  slate /tmp/looks.md >>/tmp/app.log 2>&1 & sleep 4
  shot drag-before
  read -r _ plain bold <<<"$(python3 /tmp/px.py /shots/drag-before.png | sed -n "s/^mids: //p")"
  python /dnd.py drag 300 "$plain" 1000 "$bold"; keys -M ctrl c -m ctrl; sleep 1; shot drag
  echo "dragged: $(wl-paste -n | tr "\n" "|")"; quit
  echo "== 14 edit"
  printf "# Title\n\nFirst paragraph.\n" > /tmp/edit.md
  slate /tmp/edit.md >>/tmp/app.log 2>&1 & sleep 4
  # Every wtype starts with a pause: the first key of a new virtual keyboard can be lost
  # while the window takes its keymap. Repeated keys go in one wtype for the same reason.
  keys -M ctrl -k End -m ctrl; keys -d 60 " More"; keys -k Return; keys -d 60 "Second"; sleep 1
  a11y page | sed "s/^/typed /"
  keys $(for i in $(seq 7); do printf -- "-k BackSpace -s 120 "; done); sleep 1
  a11y page | sed "s/^/deleted /"
  keys -M ctrl $(for i in $(seq 15); do printf "z -s 120 "; done) -m ctrl; sleep 1
  a11y page | sed "s/^/undone /"
  keys -M ctrl -M shift $(for i in $(seq 15); do printf "z -s 120 "; done) -m shift -m ctrl; sleep 1
  a11y page | sed "s/^/redone /"
  keys -M ctrl -k End -m ctrl -s 200 -k Home -s 200 "[" -s 200 -k End -s 200 "]" -s 200 \
    -M ctrl -k Left -m ctrl -s 200 "|" -s 200 -k Left -s 200 -k Left -s 200 "_" -s 200 \
    -k Right -s 200 -k Right -s 200 -k Right -s 200 "^" -s 200 -M ctrl -k Right -m ctrl -s 200 "~" \
    -s 200 -k Up -s 200 -k End -s 200 "!" -s 200 -k Down -s 200 -k End -s 200 "?"; sleep 1
  a11y page | sed "s/^/moved /"; shot edit; quit
  echo "== 15 format"
  printf "HHHHHHHH\n\nHHHHHHHH\n\nHHHHHHHH\n\nZ\n" > /tmp/fmt.md
  slate /tmp/fmt.md >>/tmp/app.log 2>&1 & sleep 4
  keys -M ctrl -k End -m ctrl; shot fmt-before
  keys -M ctrl -k Home -m ctrl; keys -M ctrl 1 -m ctrl
  keys -k Down; keys -M ctrl -M shift 8 -m shift -m ctrl
  keys -M ctrl -k End -m ctrl; shot fmt-mid
  read -r _ _ third <<<"$(python3 /tmp/px.py /shots/fmt-mid.png | sed -n "s/^mids: //p")"
  python /dnd.py drag 300 "$third" 1000 "$third"; keys -M ctrl b -m ctrl
  keys -M ctrl -k End -m ctrl; shot fmt-after
  for i in 1 2 3; do keys -M ctrl z -m ctrl; done; keys -M ctrl -k End -m ctrl; shot fmt-undone
  for i in 1 2 3; do keys -M ctrl -M shift z -m shift -m ctrl; done
  keys -M ctrl -k End -m ctrl; shot fmt-redone
  python3 /tmp/fmt.py; quit
  echo "== 16 table"
  printf "Before\n\n| a | b |\n|---|---|\n| c | d |\n\nAfter\n" > /tmp/table.md
  slate /tmp/table.md >>/tmp/app.log 2>&1 & sleep 4
  a11y page | sed "s/^/opened /"
  keys -k End -s 200 -k Down -s 200 1 -s 200 -k Tab -s 200 2 -s 200 -k Tab -s 200 3 \
    -s 200 -k Right -s 200 -k Right -s 200 4 -s 200 -k Tab -s 200 5 -s 200 -k Up -s 200 6 \
    -s 200 -M shift -k Tab -m shift -s 200 7 -s 200 -k BackSpace -s 200 -k BackSpace -s 200 8 \
    -s 200 -k Return -s 200 9; sleep 1
  a11y page | sed "s/^/edited /"; shot table
  keys -M ctrl $(for i in $(seq 25); do printf "z -s 120 "; done) -m ctrl; sleep 1
  a11y page | sed "s/^/undone /"
  keys -M ctrl -M shift $(for i in $(seq 25); do printf "z -s 120 "; done) -m shift -m ctrl; sleep 1
  a11y page | sed "s/^/redone /"; quit
  echo "== 17 ime"
  printf "Hello world\n" > /tmp/ime.md
  mkfifo /tmp/ime.fifo
  ime() { timeout 5 sh -c "echo \"$1\" > /tmp/ime.fifo"; sleep 1; }
  slate /tmp/ime.md >>/tmp/app.log 2>&1 & sleep 4
  # A keyboard on the seat throughout, and an input method; then a focus change.
  wtype -s 120000 & board=$!; sleep 1
  python3 /ime.py serve /tmp/ime.fifo /tmp/ime.log 2>>/tmp/app.log & sleep 1
  slate >>/tmp/app.log 2>&1 & sleep 3
  swaymsg "[title=ime.md] focus" >/dev/null; sleep 1
  keys -M ctrl -k End -m ctrl; sleep 1; shot ime-before
  ime "preedit かな 6"; shot ime-preedit
  python3 /tmp/preedit.py
  a11y page | sed "s/^/composing /"
  ime "commit 仮名"; a11y page | sed "s/^/committed /"
  ime "surrounding"; grep "^surrounding:" /tmp/ime.log | tail -1
  ime "delete 3 0"; a11y page | sed "s/^/deleted /"; shot ime-after
  kill $board; quit
  echo "== 18 clipboard"
  printf "Alpha beta gamma\n\nBefore\n\n| a | b |\n|---|---|\n| c | d |\n\nAfter\n" > /tmp/sel.md
  printf "**HHHHHHHH**\n" > /tmp/bold.md
  printf "HHHHHHHH\n\nZ\n" > /tmp/target.md
  copied() { echo "$1: $(wl-paste -n 2>/dev/null | tr "\n" "|")"; }
  slate /tmp/sel.md >>/tmp/app.log 2>&1 & sleep 4
  keys -M ctrl -k Home -m ctrl -s 200 -M shift -k End -m shift -s 200 -M ctrl c -m ctrl; copied shift-end
  keys -M shift -k Down -m shift -s 200 -M ctrl c -m ctrl; copied shift-down
  keys -M shift -k Down -m shift -s 200 -M ctrl c -m ctrl; copied into-table
  keys -M ctrl -M shift -k End -m shift -m ctrl -s 200 -M ctrl c -m ctrl; copied to-end
  keys -M ctrl -k Home -m ctrl; sleep 1; shot sel
  read -r y _ beta _ <<<"$(python3 /tmp/words.py /shots/sel.png | sed -n "s/^words: //p")"
  python /dnd.py click "$beta" "$y" 2; keys -M ctrl c -m ctrl; copied double
  python /dnd.py click "$beta" "$y" 3; keys -M ctrl c -m ctrl; copied triple
  keys -M ctrl -k Home -m ctrl -s 200 -M shift -k End -m shift -s 200 -M ctrl x -m ctrl; sleep 1
  a11y page | sed "s/^/cut /"
  keys -M ctrl z -m ctrl; sleep 1; a11y page | sed "s/^/cut-undone /"
  keys -M ctrl -M shift z -m shift -m ctrl; sleep 1; a11y page | sed "s/^/cut-redone /"
  keys -M ctrl z -m ctrl -s 200 -M ctrl a -m ctrl -s 200 -M ctrl x -m ctrl; sleep 1
  a11y page | sed "s/^/all-cut /"
  keys -M ctrl v -m ctrl; sleep 2; a11y page | sed "s/^/all-pasted /"
  keys -M ctrl z -m ctrl; sleep 1; a11y page | sed "s/^/paste-undone /"; quit
  slate /tmp/bold.md >>/tmp/app.log 2>&1 & sleep 4
  keys -M ctrl a -m ctrl -s 200 -M ctrl c -m ctrl; sleep 1
  echo "offers: $(wl-paste -l 2>/dev/null | tr "\n" " ")"; copied bold-plain
  slate /tmp/target.md >>/tmp/app.log 2>&1 & sleep 4
  swaymsg "[title=target.md] focus, layout tabbed" >/dev/null; sleep 1
  keys -M ctrl -k End -m ctrl -s 200 -k Return -s 200 -M ctrl v -m ctrl; sleep 2
  a11y page target.md | sed "s/^/rich-pasted /"; shot rich
  python3 /tmp/px.py /shots/rich.png | sed -n "s/^third ink: /rich bold ink: /p"
  wl-copy "plain one
plain two" & sleep 1
  keys -M ctrl v -m ctrl; sleep 2; a11y page target.md | sed "s/^/plain-pasted /"
  keys -M ctrl z -m ctrl; sleep 1; a11y page target.md | sed "s/^/plain-undone /"; quit
  echo "== 19 accessibility"
  printf "Plain **bold** *italic* text\n\nSecond paragraph line\n" > /tmp/a11y.md
  slate /tmp/a11y.md >>/tmp/app.log 2>&1 & sleep 4
  keys -M ctrl -k End -m ctrl; sleep 1
  python3 /tmp/atspi.py read a11y.md 2>&1
  python3 -u /tmp/atspi.py listen > /tmp/events.log 2>&1 & listener=$!; sleep 2
  keys X -s 300 -k BackSpace -s 300 -k Return -s 300 -M ctrl z -m ctrl; sleep 2
  kill $listener; cat /tmp/events.log; quit
  echo "== 20 save"
  mkdir -p /tmp/save
  printf "caf\351 one\r\nsecond line" > /tmp/save/w.txt
  ino=$(stat -c %i /tmp/save/w.txt)
  slate /tmp/save/w.txt >>/tmp/app.log 2>&1 & sleep 4
  keys -M ctrl -k End -m ctrl; keys X; sleep 1; titles | sed "s/^/edited-/"
  keys -M ctrl s -m ctrl; saved; titles | sed "s/^/saved-/"
  python3 /tmp/bytes.py txt-saved /tmp/save/w.txt
  [ "$(stat -c %i /tmp/save/w.txt)" != "$ino" ] && echo "txt replaced by rename" || echo "txt written in place"
  keys 仮; sleep 1; keys -M ctrl s -m ctrl; sleep 2; shot refused
  a11y dump | grep -E "Cannot save|Windows-1252" | sed "s/^/alert: /"
  keys -k Return; sleep 1; python3 /tmp/bytes.py txt-refused /tmp/save/w.txt
  keys -M ctrl -M shift s -m shift -m ctrl; sleep 3; shot save-as
  keys -M ctrl a -m ctrl; keys as.md; keys -k Return; sleep 1; saved
  titles | sed "s/^/saved-as-/"; python3 /tmp/bytes.py as.md /tmp/save/as.md; quit
  printf "\377\376h\000i\000\n\000" > /tmp/save/u.txt
  slate /tmp/save/u.txt >>/tmp/app.log 2>&1 & sleep 4
  keys -M ctrl -k End -m ctrl; keys "!"; keys -M ctrl s -m ctrl; sleep 1; saved
  python3 /tmp/bytes.py utf16 /tmp/save/u.txt; quit
  printf "Setext Title\n============\n\nSome __bold__ and _italic_ text.\n\n* star item one\n* star item two\n\nEdit me here\n\n1) paren list\n2) second\n" > /tmp/save/m.md
  cp /tmp/save/m.md /tmp/m.orig
  slate /tmp/save/m.md >>/tmp/app.log 2>&1 & sleep 4
  keys -M ctrl -k Home -m ctrl; keys -k Down -k Down -k Down -k Down -k End; keys " now"; sleep 1
  keys -M ctrl s -m ctrl; saved
  python3 /tmp/compare.py md /tmp/m.orig /tmp/save/m.md
  keys -M ctrl z z z -m ctrl; keys -M ctrl s -m ctrl; sleep 1; saved
  python3 /tmp/compare.py md-undone /tmp/m.orig /tmp/save/m.md
  keys Z; sleep 1; titles | sed "s/^/md-edited-/"; keys -M ctrl w -m ctrl; sleep 2; shot close-prompt
  a11y dump | grep "Save changes" | sed "s/^/prompt: /"
  keys -k Return; sleep 1; for i in $(seq 40); do pgrep -x slate >/dev/null || break; sleep 0.5; done
  pgrep -x slate >/dev/null && echo "after prompt: slate still running" || echo "after prompt: slate exited"
  grep -c "^Edit me hereZ$" /tmp/save/m.md | sed "s/^/md saved on close: /"
  echo "temp files left: $(ls -a /tmp/save | grep -c cronkite-save)"; quit
  echo "== 21 docx save"
  cp /tmp/fixture.docx /tmp/save/f.docx
  slate /tmp/save/f.docx >>/tmp/app.log 2>&1 & sleep 4
  keys -M ctrl -k End -m ctrl; keys " Edited"; sleep 1; keys -M ctrl s -m ctrl; sleep 1; saved
  titles | sed "s/^/docx-saved-/"
  python3 /tmp/parts.py /tmp/fixture.docx /tmp/save/f.docx "The end. Edited"; quit
  slate /tmp/save/f.docx >>/tmp/app.log 2>&1 & sleep 4
  a11y page f.docx | grep -c "The end. Edited$" | sed "s/^/reopened with the edit: /"
  echo "package copies: $(ls ${XDG_CACHE_HOME:-$HOME/.cache}/slate | grep -c "^original-")"; quit
  echo "== 22 new document"
  mkdir -p /tmp/docs; echo "XDG_DOCUMENTS_DIR=\"/tmp/docs\"" > $XDG_CONFIG_HOME/user-dirs.dirs
  slate >>/tmp/app.log 2>&1 & sleep 3
  titles | sed "s/^/start-/"
  keys -M ctrl n -m ctrl; sleep 3; titles | sed "s/^/new-/"
  keys -M ctrl 1 -m ctrl; keys Plans; keys -k Return; keys "First line"; sleep 1
  titles | sed "s/^/typed-/"
  keys -M ctrl s -m ctrl; sleep 3; shot new-save-as
  keys -M ctrl a -m ctrl; keys plans.docx; keys -k Return; sleep 1; saved
  titles | sed "s/^/new-saved-/"
  echo "docs: $(ls /tmp/docs | tr "\n" " ")"
  python3 -c "import zipfile; x = zipfile.ZipFile(\"/tmp/docs/plans.docx\").read(\"word/document.xml\").decode()
print(\"heading 1:\", \"w:val=\\\"Heading1\\\"\" in x and \"Plans\" in x)
print(\"line:\", \"First line\" in x)"
  keys -M ctrl n -m ctrl; sleep 3; titles | sed "s/^/another-/"; quit
  slate /tmp/docs/plans.docx >>/tmp/app.log 2>&1 & sleep 4
  a11y page plans.docx | grep -c "First line$" | sed "s/^/reopened plans: /"; quit
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
column() { # WHAT SECTION LABEL WIDTH: the rule within 1 px, the text within 16 px, centred
  local m r t o
  m=$(section "$2" | sed -n "s/^$3: //p" | head -1)
  read -r _ r _ t _ o <<<"$m" || true
  if [ -n "$o" ] && [ "$r" -ge $(($4 - 1)) ] && [ "$r" -le $(($4 + 1)) ] && [ "$t" -le "$4" ] \
    && [ "$t" -ge $(($4 - 16)) ] && [ "${o#-}" -le 2 ]; then
    echo "ok   $1 (rule $r, text $t, off centre $o)"
  else
    echo "FAIL $1 (${m:-nothing}; want $4)"; fail=1
  fi
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
        n += sum(1 for p in json.loads(f[-1]).split("\n") if p.strip())
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
for w in medium:624 narrow:480 wide:816 full:1232; do
  for t in col-a col-b col-c; do
    column "8: ${w%%:*} in $t.md measures ${w#*:} px" "8 width" "${w%%:*} $t\.md" "${w#*:}"
  done
done
check "9: Ctrl+Q quits" "9 restart" "^after quit: slate exited$"
check "9: the settings file remembers Full" "9 restart" "^settings: text-width=full$"
column "9: relaunched, the column is still Full" "9 restart" "restart col-a\.md" 1232
column "10: Wide in a 1280 px window" "10 narrow window" "wide1280 col-a\.md" 816
column "10: Wide in a 640 px window narrows to 592" "10 narrow window" "wide640 col-a\.md" 592
column "10: Narrow in a 640 px window stays 480" "10 narrow window" "narrow640 col-a\.md" 480
check "11: the .docx page break is named Page break to screen readers" "11 page break" "^page breaks: 1$"
check "12: Ctrl+A, Ctrl+C copies the whole text as plain text" "12 select all" "^copy equals the file$"
check "13: dragging over two lines copies exactly them" "13 drag" "^dragged: HHHHHHHH\|HHHHHHHH$"
check "14: typing, Enter, and typing again edit the text" "14 edit" "^typed page: Title\|First paragraph\. More\|Second$"
check "14: Backspace deletes, and at a paragraph's start joins" "14 edit" "^deleted page: Title\|First paragraph\. More$"
check "14: undo gives back the opened text" "14 edit" "^undone page: Title\|First paragraph\.$"
check "14: redo gives back every edit" "14 edit" "^redone page: Title\|First paragraph\. More$"
check "14: Home, End, arrows, and word moves put the caret where they say" "14 edit" \
  "^moved page: Title!\|\[First paragraph\._ \|M\^ore~\]\?$"
at_least "15: Ctrl+1 makes a heading (percent of the line's height)" "15 format" "heading height" 150
at_least "15: Ctrl+Shift+8 makes a bulleted item (px its text moves right)" "15 format" "list shift" 20
at_least "15: Ctrl+B over a selection makes it bold (percent ink)" "15 format" "bold ink" 115
check "15: undo gives back the unformatted lines exactly" "15 format" "^undo restores: yes$"
check "15: redo gives back the formatted lines exactly" "15 format" "^redo restores: yes$"
check "16: the table reads in document order" "16 table" "^opened page: Before\|a\|b\|c\|d\|After$"
check "16: arrows and Tab move into, between, and out of cells; typing, Backspace, and Enter edit cells" \
  "16 table" "^edited page: Before\|a1\|8\|92b\|63c\|4d\|5After$"
check "16: undo gives back the opened table" "16 table" "^undone page: Before\|a\|b\|c\|d\|After$"
check "16: redo gives back the edited table" "16 table" "^redone page: Before\|a1\|8\|92b\|63c\|4d\|5After$"
at_least "17: the preedit shows right of the text (dark px)" "17 ime" "preedit ink" 40
at_least "17: the preedit is underlined (px of line)" "17 ime" "underline" 20
check "17: the preedit isn't in the document" "17 ime" "^composing page: Hello world$"
check "17: commit replaces the preedit with the text" "17 ime" "^committed page: Hello world仮名$"
check "17: the input method reads the text around the caret" "17 ime" "^surrounding: Hello world仮名\|17\|17$"
check "17: the input method deletes before the caret" "17 ime" "^deleted page: Hello world仮$"
check "18: Shift+End selects to the line's end" "18 clipboard" "^shift-end: Alpha beta gamma$"
check "18: Shift+Down extends to the next paragraph" "18 clipboard" "^shift-down: Alpha beta gamma\|Before$"
check "18: a selection into a table takes the whole table, cells one a line" "18 clipboard" \
  "^into-table: Alpha beta gamma\|Before\|a\|b\|c\|d$"
check "18: Shift+Ctrl+End selects to the document's end" "18 clipboard" "^to-end: Alpha beta gamma\|Before\|a\|b\|c\|d\|After$"
check "18: a double click selects the word" "18 clipboard" "^double: beta$"
check "18: a triple click selects the paragraph" "18 clipboard" "^triple: Alpha beta gamma$"
check "18: Ctrl+X cuts the selection" "18 clipboard" "^cut page: \|Before\|a\|b\|c\|d\|After$"
check "18: undo gives the cut text back" "18 clipboard" "^cut-undone page: Alpha beta gamma\|Before\|a\|b\|c\|d\|After$"
check "18: redo cuts it again" "18 clipboard" "^cut-redone page: \|Before\|a\|b\|c\|d\|After$"
check "18: cutting everything cuts through the table" "18 clipboard" "^all-cut page: $"
check "18: pasting it back brings the table back (Slate's rich clipboard)" "18 clipboard" \
  "^all-pasted page: Alpha beta gamma\|Before\|a\|b\|c\|d\|After$"
check "18: undo takes the paste back" "18 clipboard" "^paste-undone page: $"
check "18: a copy offers Slate's format and plain text" "18 clipboard" \
  "^offers: .*application/x-slate-fragment.*text/plain|^offers: .*text/plain.*application/x-slate-fragment"
check "18: other apps get plain text" "18 clipboard" "^bold-plain: HHHHHHHH$"
check "18: a copy pastes into another Slate process" "18 clipboard" "^rich-pasted page: HHHHHHHH\|Z\|HHHHHHHH$"
at_least "18: it pastes still bold (percent ink)" "18 clipboard" "rich bold ink" 115
check "18: text from another app pastes as paragraphs" "18 clipboard" \
  "^plain-pasted page: HHHHHHHH\|Z\|HHHHHHHHplain one\|plain two$"
check "18: undo takes that paste back" "18 clipboard" "^plain-undone page: HHHHHHHH\|Z\|HHHHHHHH$"
check "19: the page's text has its 44 characters" "19 accessibility" "^count: 44$"
check "19: the caret's offset follows Ctrl+End" "19 accessibility" "^caret: 44$"
check "19: the character at an offset" "19 accessibility" "^char: .b. 6-7$"
check "19: the word at an offset" "19 accessibility" "^word: .bold ?. 6-1[01]$"
check "19: the line at an offset" "19 accessibility" "^line: .Second paragraph line. 23-44$"
check "19: the paragraph at an offset" "19 accessibility" "^paragraph: .Plain bold italic text.n. 0-23$"
check "19: bold text's weight" "19 accessibility" "^attrs 7: 6-10 weight=700 "
check "19: italic text's style" "19 accessibility" "^attrs 12: 11-17 weight=400 style=italic$"
check "19: plain text's looks" "19 accessibility" "^attrs 1: 0-6 weight=400 style=normal$"
check "19: typing announces the inserted character" "19 accessibility" "^event: object:text-changed:insert 44 1 .X.$"
check "19: Backspace announces the deleted character" "19 accessibility" "^event: object:text-changed:delete 44 1 .X.$"
check "19: Enter announces a new line" "19 accessibility" "^event: object:text-changed:insert 44 1 ..n.$"
check "19: undoing it announces its removal" "19 accessibility" "^event: object:text-changed:delete 44 1 ..n.$"
check "20: an edit marks the title" "20 save" "^edited-titles: • w.txt,$"
check "20: Ctrl+S saves and clears the mark" "20 save" "^saved-titles: w.txt,$"
check "20: a .txt keeps Windows-1252, CRLF, and no final newline" "20 save" \
  "^txt-saved bytes: b.caf.xe9 one.r.nsecond lineX.$"
check "20: the save replaces the file by a rename" "20 save" "^txt replaced by rename$"
check "20: text the encoding lacks is refused, said in an alert" "20 save" "^alert: .*Windows-1252"
check "20: a refused save leaves the file as it was" "20 save" "^txt-refused bytes: b.caf.xe9 one.r.nsecond lineX.$"
check "20: Save As takes the new name" "20 save" "^saved-as-titles: as.md,$"
check "20: Save As writes Markdown, in UTF-8 when the encoding lacks a character" "20 save" \
  "^as.md bytes: b.caf.xc3.xa9 one.r.n.r.nsecond lineX.xe4.xbb.xae.$"
check "20: UTF-16 LE saves with its BOM" "20 save" "^utf16 bytes: b..xff.xfeh.x00i.x00!.x00.n.x00.$"
check "20: an edited .md changes only the edited line" "20 save" "^md changed lines: 2$"
check "20: that line has the edit" "20 save" "^md-diff: \+ Edit me here now$"
check "20: undone, the .md saves its original bytes" "20 save" "^md-undone: same bytes$"
check "20: closing with changes asks to save" "20 save" "^prompt: Save changes to .m.md. before closing\?$"
check "20: Enter saves and closes" "20 save" "^after prompt: slate exited$"
check "20: the change was saved on closing" "20 save" "^md saved on close: 1$"
check "20: no temp file is left" "20 save" "^temp files left: 0$"
check "21: Ctrl+S saves a Word document" "21 docx save" "^docx-saved-titles: f.docx,$"
check "21: the package keeps its parts in order" "21 docx save" "^parts: same order$"
check "21: only word/document.xml changes" "21 docx save" "^changed parts: word/document.xml$"
check "21: document.xml holds the edit" "21 docx save" "^document.xml has the edit: 1$"
check "21: reopened, the page shows the edit" "21 docx save" "^reopened with the edit: 1$"
check "21: copies left by killed processes are swept" "21 docx save" "^package copies: 1$"
check "22: slate alone shows the start window" "22 new document" "^start-titles: Slate,$"
check "22: Ctrl+N opens an Untitled document in its place" "22 new document" "^new-titles: Untitled,$"
check "22: typing marks it" "22 new document" "^typed-titles: • Untitled,$"
check "22: Ctrl+S asks where and saves it there" "22 new document" "^new-saved-titles: plans.docx,$"
check "22: in the Documents folder" "22 new document" "^docs: plans.docx $"
check "22: a Word document with the heading" "22 new document" "^heading 1: True$"
check "22: and the line" "22 new document" "^line: True$"
check "22: Ctrl+N in a document opens another" "22 new document" "^another-titles: Untitled,plans.docx,$"
check "22: reopened, it shows the line" "22 new document" "^reopened plans: 1$"
if [ $fail = 0 ]; then echo "SLATE OK"; else
  echo "SLATE CHECK FAILED"; [ -n "${VERBOSE:-}" ] || echo "(VERBOSE=1 for the transcript)"
  exit 1
fi
