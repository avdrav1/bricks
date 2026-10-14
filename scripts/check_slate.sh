#!/usr/bin/env bash
# SLATE-4, SLATE-10, SLATE-11, SLATE-6, SLATE-12, SLATE-13, SLATE-20, SLATE-21, SLATE-22,
# SLATE-23, SLATE-24, and SLATE-25 acceptance: the window opens, shows, and refuses
# documents, its pageless column follows View → Text Width and a Word document's page, its
# text selects and copies, it edits with undo, table cells included, it composes through an
# input method, it draws a Word document's fonts (embedded ones and fallbacks by class),
# paragraph looks, and tab stops, its format bar sets them, and they save.
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
#      screenshot at the default, Page (Markdown has no page: Medium's 624), then after
#      F10 → Text Width → Narrow, Medium, Wide, and Full in col-a's menu: the rule (the
#      column's width exactly) and the widest line of text above it measure 624, 480, 624,
#      816, and 1232 px (the 1280 px screen less 24 px a side) in all three windows,
#      centred;
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
#  23. fonts.docx (written by /tmp/fonts.py with Liberation, Carlito, and Caladea
#      installed): the same line in paragraph styles of Liberation Serif, Liberation Sans,
#      Liberation Mono, and Calibri at 11 pt draws as wide (ink, in a screenshot) as Pango
#      measures it in that font at 96 dpi, within 2 px, Calibri as Carlito, and the three
#      Liberation widths differ; then paragraphs of "Hamburg" in Liberation Sans: one with
#      space before 450 and after 300 twips between two plain ones, one indented 900, one
#      indented 900 with a 450 hanging first line, and one with lines exactly 600 apart;
#      their ink tops and left edges are 30 and 20 px further apart than plain lines, 60 px
#      in, the first line 30 px in and the next 60, and lines 40 px apart, each within 2 px.
#  24. the format bar (screenshots measure text below it, from row TOP): fixture.docx
#      shows it; in bar.docx (written by /tmp/bar.py: "Alpha beta gamma", "plain " and a
#      bold 14 pt "bold", a paragraph in the custom style Fancy, "Z"), with "beta"
#      selected, the bar picks Liberation Mono from the font list (searched), types 20 in
#      the size field, presses Underline, picks Red in the colour dialog, presses Centre,
#      and picks Fancy from the style list: AT-SPI reads each on the word (family, size,
#      underline, fg-color), the line's ink centres in the window, and the bar shows the
#      style, font, size, and toggles; one Ctrl+Z per change undoes just it, newest first;
#      Ctrl+U, Ctrl+] twice, and Ctrl+E underline the word, make it 13 pt, and centre the
#      line, and Ctrl+[ makes it 12 pt; a selection of plain 11 pt and bold 14 pt text
#      shows Bold off and the size blank, the bold word alone Bold on and 14; a .txt
#      window has no format bar (no control named Font).
#  25. saving the format bar's looks: in a copy of bar.docx, "beta" given Liberation Mono,
#      20 pt, underline, Red, centred, and Fancy through the bar as in 24, then Ctrl+S:
#      the saved document.xml gives the paragraph w:pStyle Fancy and w:jc center and the
#      word w:rFonts Liberation Mono, w:sz 40, w:u single, and w:color E01B24, and keeps
#      the other paragraphs as they were; reopened, AT-SPI reads the word's looks and the
#      rest of the line's (Fancy's) as before the save, the line's ink centres, and the bar
#      shows Fancy, the font, the size, Underline, and Centre.
#  26. find and replace in find.md ("A cat sat.", "The Cat and the cat.", a table with a
#      "cat" cell, "Last cat."), the bar's count read through AT-SPI: Ctrl+F and "cat"
#      count 1 of 5 from the caret; Enter selects it, then steps on into the table cell;
#      Escape and a typed X replace the cell's match; Ctrl+F again counts 4 of 4 from the
#      caret, Shift+Enter steps back twice; Match Case drops "Cat" (of 3); Ctrl+H, "dog"
#      in the replace row, Enter twice replaces one match, Replace All the rest, leaving
#      "Cat" and No matches; one undo takes back Replace All and the next Replace;
#      closing the bar clears the count.
#  27. crash recovery: r.txt, r.md, and r.docx edited in one process have no journal right
#      after the edit and one each 7 s later; kill -9 leaves the files and the journals.
#      `slate` alone lists the three on its start window; Restore on r.txt opens it marked
#      "• " with the edit, the list still showing; `slate r.md r.docx` asks in each window,
#      and Restore puts their edits back, marked "• ", the files unchanged and the crashed
#      journals gone; the restored windows journal again. One undo in r.txt gives back the
#      file; Ctrl+S in r.md, then closing every window (Discard for r.docx) ends Slate and
#      leaves no journal. An edit crashed and discarded leaves the file and no journal.
#      Typing a letter every half second for 20 s, the journal shows up about 5 s after the
#      first and is rewritten at least 5 s apart.
#  28. Page width, without a settings file (the default, Page): page-75.docx (written by
#      /tmp/page.py: Letter with 0.5″ left and right margins, a long word that wraps, and a
#      one-cell table, whose border is the column's width exactly) and page-none.docx (the
#      same without w:sectPr), tabbed, measure as col.py measures in 8: 720 px (7.5″ at 96
#      dpi) and 624 (Letter with 1″ margins); Medium from the menu makes page-75 624, and
#      Page again 720; the settings file says page, and col-a.md opened then measures 624.
#  29. tab stops: tabs.docx (written by /tmp/tabs.py: Letter with 0.5″ margins, Liberation
#      Serif 11 pt, w:defaultTabStop 720, and a one-cell table whose border is the column)
#      measured in a screenshot: "NAVA", a tab, and "Remote" with a right stop at 10800
#      twips ends Remote's ink at the column's right edge; "Lead Product Manager" and three
#      default tabs start "X" at the third multiple of 48 px past the first text's ink; a
#      centre stop at 5400 centres "Middle" 360 px in; and a right stop at 7200 with a dot
#      leader ends "42" 480 px in, with dots along the gap after "Contents" on the baseline
#      and no ink above it; each within 2 px. tab.md, a Markdown line with a tab, opens and
#      reads as written.
#  30. embedded fonts: embed-plain.docx and embed-obfuscated.docx (written by /tmp/embed.py)
#      embed Cardo (OFL, a Latin subset, apps/slate/crates/docx/tests/data/fonts; not
#      installed in the container), one as a plain .ttf with a zero w:fontKey, the other as
#      Word's obfuscated .odttf with a key, and run the same line in Cardo, in "Nosuchfont
#      Serifish" (font table: roman), and in "Nosuchfont Sansish" (swiss): in a screenshot
#      the Cardo line's ink is as wide as Pango measures it in Cardo loaded from the file,
#      the roman one as in the system serif, and the swiss one as in sans, each within 2 px
#      (and the three differ); a typed character and Ctrl+S keep fontTable.xml, its
#      relationships, and the font part byte for byte. The face is written once to
#      $XDG_RUNTIME_DIR/slate/fonts (mode 700) and never installed: fc-list doesn't list
#      Cardo afterwards and the user's font directories stay empty.
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
  at-spi2-core python-pywayland wlr-protocols wayland pkgconf jq wl-clipboard \
  ttf-liberation ttf-carlito ttf-caladea python-cairo >/dev/null 2>&1
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
import os, sys
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf
p = GdkPixbuf.Pixbuf.new_from_file(sys.argv[1])
w, h, rs, n, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
dark = lambda x, y: px[y * rs + x * n] + px[y * rs + x * n + 1] + px[y * rs + x * n + 2] < 300
rows = [y for y in range(int(os.environ["TOP"]), h - 40) if any(dark(x, y) for x in range(w))]
y0 = rows[0]
y1 = y0
while y1 + 1 in rows:
    y1 += 1
cols = [x for x in range(w) if any(dark(x, y) for y in range(y0, y1 + 1))]
words, start, last = [], cols[0], cols[0]
for x in cols[1:]:
    if x - last > 4:
        words.append((start + last) // 2)
        start = x
    last = x
words.append((start + last) // 2)
print("words: " + str((y0 + y1) // 2) + " " + " ".join(map(str, words)))
PY
cat > /tmp/preedit.py <<"PY"
import os, sys
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf
def load(name):
    p = GdkPixbuf.Pixbuf.new_from_file(f"/shots/{name}.png")
    w, h, rs, n, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
    return lambda x, y: px[y * rs + x * n] + px[y * rs + x * n + 1] + px[y * rs + x * n + 2] < 450, w // 2, h
# The document window is the left half; its text lines are the bands of ink below the
# header and the format bar.
dark, w, h = load("ime-before")
bands, cur = [], None
for y in range(int(os.environ["TOP"]), h - 40):
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
# Screenshots measure text from this row down: below the header bar and the format bar
# (whose bottom edge is at 90).
export TOP=96
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

def named(name):
    # Widgets named NAME in every slate process, depth first; a drop-down is named by the
    # item it shows, so one described as NAME too.
    return [n for a in apps() for n in walk(a) if name in (n.get_name(), n.get_description())]

def shown(node):
    # What a drop-down shows: the label inside it that shows (with nothing chosen none
    # does, though the drop-down may still be named by the last item).
    for n in walk(node):
        if n is not node and n.get_role_name() == "label":
            if n.get_state_set().contains(Atspi.StateType.SHOWING):
                return n.get_name() or ""
    return ""

def on(node):
    s = node.get_state_set()
    return "on" if s.contains(Atspi.StateType.CHECKED) or s.contains(Atspi.StateType.PRESSED) else "off"

if cmd == "press":
    # Activate the widget named argv[2] (its first action): a button, a toggle button, a
    # colour swatch.
    for n in named(sys.argv[2]):
        if n.get_action_iface() and Atspi.Action.get_n_actions(n.get_action_iface()) > 0:
            Atspi.Action.do_action(n.get_action_iface(), 0)
            sys.exit(0)
    sys.exit(f"no button {sys.argv[2]!r}")
elif cmd == "where":
    # A point to click on the first widget named argv[2] that shows (of role argv[4], if
    # given): its middle, or at fraction argv[3] of its width.
    fx = float(sys.argv[3]) if len(sys.argv) > 3 else 0.5
    role = sys.argv[4] if len(sys.argv) > 4 else None
    for n in named(sys.argv[2]):
        if role is not None and n.get_role_name() != role:
            continue
        c = n.get_component_iface()
        e = c.get_extents(Atspi.CoordType.WINDOW) if c else None
        if e is not None and e.width > 0:
            print(e.x + int(e.width * fx), e.y + e.height // 2)
            sys.exit(0)
    sys.exit(f"nothing named {sys.argv[2]!r}")
elif cmd == "bar":
    # The format bar as it shows: the style and font named, the size field, the toggles.
    out = ["fonts=%d" % len(named("Font"))]
    for label, name in (("style", "Style"), ("font", "Font")):
        nodes = named(name)
        out.append(label + "=" + (shown(nodes[0]) if nodes else "-"))
    sizes = named("Size")
    size = "-"
    for n in (walk(sizes[0]) if sizes else ()):
        ti = n.get_text_iface()
        if ti is not None:
            size = Atspi.Text.get_text(ti, 0, -1)
            break
    out.append(f"size={size}")
    for label, name in (("bold", "Bold"), ("italic", "Italic"), ("underline", "Underline"),
                        ("strike", "Strikethrough"), ("left", "Align Left"), ("centre", "Centre"),
                        ("right", "Align Right"), ("justify", "Justify")):
        nodes = [n for n in named(name) if n.get_role() == Atspi.Role.TOGGLE_BUTTON]
        out.append(label + "=" + (on(nodes[0]) if nodes else "-"))
    print("bar: " + " ".join(out))
    sys.exit(0)
elif cmd == "attrs":
    # The run attributes at character argv[3] of the page named argv[2].
    pages = (n for a in apps() for n in walk(a) if n.get_role_name() == "text" and n.get_name() == sys.argv[2])
    t = next(pages).get_text_iface()
    attrs, start, end = Atspi.Text.get_attribute_run(t, int(sys.argv[3]), True)
    keys = ("family-name", "size", "weight", "style", "underline", "strikethrough", "fg-color")
    print(f"attrs {start}-{end}: " + " ".join(f"{k}={attrs.get(k)}" for k in keys))
    sys.exit(0)
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
elif cmd == "choose":
    # Press each showing button named argv[2]; with argv[3] "dialog", only those in a
    # dialog (each window has its own), else only the one in the row showing argv[3].
    def in_dialog(node):
        while node is not None:
            if node.get_role_name() in ("alert", "dialog"):
                return True
            node = node.get_parent()
        return False
    def in_row(node, text):
        row = node.get_parent()
        return any(row.get_child_at_index(i).get_name() == text for i in range(row.get_child_count()))
    scope = sys.argv[3] if len(sys.argv) > 3 else None
    pressed = 0
    for node in [n for a in apps() for n in walk(a)]:
        if node.get_role_name() != "button" or node.get_name() != sys.argv[2] \
                or not node.get_state_set().contains(Atspi.StateType.SHOWING):
            continue
        if scope == "dialog" and not in_dialog(node) or scope not in (None, "dialog") and not in_row(node, scope):
            continue
        Atspi.Action.do_action(node.get_action_iface(), 0)
        pressed += 1
    print(f"pressed {pressed}")
elif cmd == "count":
    # How many accessible objects of every Slate process are named argv[2].
    print(sum(1 for a in apps() for n in walk(a) if n.get_name() == sys.argv[2]))
PY
cat > /tmp/fmt.py <<"PY"
import os
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf
def bands(name):
    p = GdkPixbuf.Pixbuf.new_from_file(f"/shots/{name}.png")
    w, h, rs, n, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
    out, cur = [], None
    for y in range(int(os.environ["TOP"]), h - 50):
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
import os, sys
import gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf
p = GdkPixbuf.Pixbuf.new_from_file(sys.argv[1] if len(sys.argv) > 1 else "/shots/looks.png")
w, h, rs, n, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
bands, cur = [], None
for y in range(int(os.environ["TOP"]), h - 50):
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
# SLATE-23: `page.py PATH [MARGIN]` writes a Word document of a long word that wraps (its
# lines as wide as the column) and a one-cell table (its border the column exactly), on
# Letter with MARGIN twips left and right; without MARGIN it has no w:sectPr.
cat > /tmp/page.py <<"PY"
import sys
import zipfile

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG = "http://schemas.openxmlformats.org/package/2006/relationships"
CT = "http://schemas.openxmlformats.org/package/2006/content-types"
OOXML = "application/vnd.openxmlformats-officedocument.wordprocessingml"
HEAD = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"""

path, word = sys.argv[1], "x" * 800
body = ("""<w:p><w:r><w:t>Page</w:t></w:r></w:p>"""
        f"""<w:p><w:r><w:t>{word}</w:t></w:r></w:p>"""
        """<w:tbl><w:tr><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/>""")
if len(sys.argv) > 2:
    m = sys.argv[2]
    body += (f"""<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="{m}" w:right="{m}" """
             f"""w:bottom="{m}" w:left="{m}" w:header="0" w:footer="0" w:gutter="0"/></w:sectPr>""")
with zipfile.ZipFile(path, "w") as z:
    z.writestr("[Content_Types].xml", HEAD + f"""<Types xmlns="{CT}">"""
               """<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"""
               """<Default Extension="xml" ContentType="application/xml"/>"""
               f"""<Override PartName="/word/document.xml" ContentType="{OOXML}.document.main+xml"/></Types>""")
    z.writestr("_rels/.rels", HEAD + f"""<Relationships xmlns="{PKG}"><Relationship Id="rId1" """
               f"""Type="{REL}/officeDocument" Target="word/document.xml"/></Relationships>""")
    z.writestr("word/document.xml", HEAD + f"""<w:document xmlns:w="{W}"><w:body>{body}</w:body></w:document>""")
PY
# SLATE-24: `make PATH` writes tabs.docx: Letter with 0.5″ margins (a 720 px column),
# Liberation Serif 11 pt, w:defaultTabStop 720 in its settings, four tabbed lines (see
# section 29), a one-cell table (its border the column exactly), and "Z"; `measure PNG`
# measures the lines above the table in a screenshot.
cat > /tmp/tabs.py <<"PY"
import os
import sys
import zipfile

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG = "http://schemas.openxmlformats.org/package/2006/relationships"
CT = "http://schemas.openxmlformats.org/package/2006/content-types"
OOXML = "application/vnd.openxmlformats-officedocument.wordprocessingml"
HEAD = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"""
RIGHT, CENTRE, LEADER, DEFAULT = 10800, 5400, 7200, 720

def para(texts, tabs=""):
    runs = "<w:r><w:tab/></w:r>".join(f"""<w:r><w:t xml:space="preserve">{t}</w:t></w:r>""" for t in texts)
    ppr = f"<w:pPr><w:tabs>{tabs}</w:tabs></w:pPr>" if tabs else ""
    return f"<w:p>{ppr}{runs}</w:p>"

def make(path):
    body = (para(["NAVA", "Remote"], f"""<w:tab w:val="right" w:pos="{RIGHT}"/>""")
            + para(["Lead Product Manager", "", "", "X"])
            + para(["A", "Middle"], f"""<w:tab w:val="center" w:pos="{CENTRE}"/>""")
            + para(["Contents", "42"], f"""<w:tab w:val="right" w:leader="dot" w:pos="{LEADER}"/>""")
            + """<w:tbl><w:tr><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"""
            + para(["Z"])
            + """<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="720" w:right="720" """
            """w:bottom="720" w:left="720" w:header="0" w:footer="0" w:gutter="0"/></w:sectPr>""")
    styles = ("""<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Liberation Serif" """
              """w:hAnsi="Liberation Serif"/><w:sz w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr>"""
              """<w:spacing w:before="0" w:after="120"/></w:pPr></w:pPrDefault></w:docDefaults>"""
              """<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>""")
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("[Content_Types].xml", HEAD + f"""<Types xmlns="{CT}">"""
                   """<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"""
                   """<Default Extension="xml" ContentType="application/xml"/>"""
                   f"""<Override PartName="/word/document.xml" ContentType="{OOXML}.document.main+xml"/>"""
                   f"""<Override PartName="/word/styles.xml" ContentType="{OOXML}.styles+xml"/>"""
                   f"""<Override PartName="/word/settings.xml" ContentType="{OOXML}.settings+xml"/></Types>""")
        z.writestr("_rels/.rels", HEAD + f"""<Relationships xmlns="{PKG}"><Relationship Id="rId1" """
                   f"""Type="{REL}/officeDocument" Target="word/document.xml"/></Relationships>""")
        z.writestr("word/_rels/document.xml.rels", HEAD + f"""<Relationships xmlns="{PKG}">"""
                   f"""<Relationship Id="rId1" Type="{REL}/styles" Target="styles.xml"/>"""
                   f"""<Relationship Id="rId2" Type="{REL}/settings" Target="settings.xml"/></Relationships>""")
        z.writestr("word/styles.xml", HEAD + f"""<w:styles xmlns:w="{W}">{styles}</w:styles>""")
        z.writestr("word/settings.xml", HEAD + f"""<w:settings xmlns:w="{W}">"""
                   f"""<w:defaultTabStop w:val="{DEFAULT}"/></w:settings>""")
        z.writestr("word/document.xml", HEAD + f"""<w:document xmlns:w="{W}"><w:body>{body}</w:body></w:document>""")

def measure(png):
    import gi
    gi.require_version("GdkPixbuf", "2.0")
    from gi.repository import GdkPixbuf
    p = GdkPixbuf.Pixbuf.new_from_file(png)
    w, h, rs, n, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
    # The border of the table, as col.py finds it: the longest run of its grey on a row.
    rule, rule_y, left = 0, h, 0
    for y in range(h):
        row = px[y * rs:y * rs + w * n]
        run = 0
        for x in range(w):
            if all(abs(row[x * n + i] - c) <= 6 for i, c in enumerate((0xC0, 0xBF, 0xBC))):
                run += 1
                if run > rule:
                    rule, rule_y, left = run, y, x - run + 1
            else:
                run = 0
    print(f"column: {left} {rule}")
    dark = lambda x, y: sum(px[y * rs + x * n:y * rs + x * n + 3]) < 300
    # Bands of rows with ink above the table, each with its ink by column.
    bands, cur = [], None
    for y in range(int(os.environ["TOP"]), rule_y - 2):
        xs = [x for x in range(left, left + rule) if dark(x, y)]
        if xs:
            if cur is None:
                cur = {"top": y, "rows": {}}
            cur["bottom"] = y
            cur["rows"][y] = xs
        elif cur is not None:
            bands.append(cur)
            cur = None
    print(f"lines: {len(bands)}")
    if len(bands) != 4:
        return
    def clusters(xs, gap=8):
        out = []
        for x in sorted(set(xs)):
            if out and x - out[-1][1] <= gap:
                out[-1][1] = x
            else:
                out.append([x, x])
        return out
    def ink(band, rows=None):
        return [x for y, xs in band["rows"].items() if rows is None or y in rows for x in xs]
    def report(label, got, want):
        print(f"{label}: {got} (want {want}) " + ("ok" if abs(got - want) <= 2 else "off"))
    a, b, c, d = bands
    # Ink ends one past its last dark column; the right edge of the column is left + rule.
    words = clusters(ink(a))
    print("remote words: " + " ".join(f"{l}-{r}" for l, r in words))
    report("remote right edge", words[-1][1] + 1 - left, rule)
    words = clusters(ink(b))
    first = words[0][1] + 1 - left
    stop = (first // 48 + 3) * 48
    print(f"manager ends at {first}")
    report("x after three default tabs", words[-1][0] - left, stop)
    words = clusters(ink(c))
    report("middle centre", (words[-1][0] + words[-1][1] + 1) / 2 - left, CENTRE // 15)
    # Letters and digits reach above the 4 rows of the baseline; leader dots do not.
    bottom = d["bottom"]
    tall = clusters(ink(d, range(d["top"], bottom - 3)))
    print(f"leader line tall clusters: {len(tall)}")
    if len(tall) != 2:
        return
    report("42 right edge", tall[1][1] + 1 - left, LEADER // 15)
    gap = range(tall[0][1] + 5, tall[1][0] - 4)
    low = set(ink(d, range(bottom - 3, bottom + 1)))
    dots = [x for x in gap if x in low]
    holes, run = 0, 0
    for x in gap:
        run = 0 if x in low else run + 1
        holes = max(holes, run)
    print(f"leader gap: {len(gap)} px, dotted {len(dots)} px, widest hole {holes} px")
    print("leader fills the gap on the baseline: " + ("yes" if len(gap) > 100 and len(dots) * 4 >= len(gap) and holes <= 6 else "no"))

make(sys.argv[2]) if sys.argv[1] == "make" else measure(sys.argv[2])
PY
# SLATE-25: `make PATH MODE` writes a document embedding Cardo (MODE plain: a .ttf with a
# zero key; obfuscated: an .odttf with KEY); `measure PNG` measures the lines of its screenshot
# against Pango at 96 dpi (Cardo added from its file for this process); `kept ORIGINAL
# SAVED` compares the font parts.
cat > /tmp/embed.py <<"PY"
import os
import sys
import zipfile

# DejaVu Serif and Sans, the container serif and sans, differ by 17 px on this line.
TEXT = "Embedded: illicit fillip, little lilies, jilting fiji"
FONT = "/docx/fonts/Cardo-Regular.ttf"
KEY = "{6C2F5D8A-91E3-4B07-A2C4-3F1E9D8B7A65}"
ZERO = "{00000000-0000-0000-0000-000000000000}"
# Each line: its name, its run font, and the family Pango measures it in.
LINES = [("embedded", "Cardo", "Cardo"), ("roman", "Nosuchfont Serifish", "serif"),
         ("swiss", "Nosuchfont Sansish", "sans-serif")]
FONT_PARTS = ("word/fontTable.xml", "word/_rels/fontTable.xml.rels")
W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG = "http://schemas.openxmlformats.org/package/2006/relationships"
CT = "http://schemas.openxmlformats.org/package/2006/content-types"
OOXML = "application/vnd.openxmlformats-officedocument.wordprocessingml"
HEAD = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"""

def obfuscate(data, key):
    # ECMA-376 Part 1, 17.8.1: the GUID bytes, last hex pair first, XOR the first 32 bytes.
    hexes = key.strip("{}").replace("-", "")
    k = bytes.fromhex(hexes)[::-1]
    head = bytes(b ^ k[i % 16] for i, b in enumerate(data[:32]))
    return head + data[32:]

def para(font, text):
    rpr = f"""<w:rPr><w:rFonts w:ascii="{font}" w:hAnsi="{font}"/></w:rPr>""" if font else ""
    return f"""<w:p><w:r>{rpr}<w:t xml:space="preserve">{text}</w:t></w:r></w:p>"""

def make(path, mode):
    font = open(FONT, "rb").read()
    if mode == "plain":
        part, key, data = "fonts/font1.ttf", ZERO, font
    else:
        part, key, data = "fonts/font1.odttf", KEY, obfuscate(font, KEY)
        assert data[:4] != font[:4]
    table = (f"""<w:fonts xmlns:w="{W}" xmlns:r="{REL}">"""
             """<w:font w:name="Liberation Sans"><w:family w:val="swiss"/></w:font>"""
             """<w:font w:name="Cardo"><w:family w:val="roman"/><w:pitch w:val="variable"/>"""
             f"""<w:embedRegular w:fontKey="{key}" r:id="rId1" w:subsetted="0"/></w:font>"""
             """<w:font w:name="Nosuchfont Serifish"><w:family w:val="roman"/><w:pitch w:val="variable"/></w:font>"""
             """<w:font w:name="Nosuchfont Sansish"><w:family w:val="swiss"/><w:pitch w:val="variable"/></w:font>"""
             "</w:fonts>")
    styles = (f"""<w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr>"""
              """<w:rFonts w:ascii="Liberation Sans" w:hAnsi="Liberation Sans"/><w:sz w:val="22"/>"""
              """</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before="0" w:after="0" """
              """w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>"""
              """<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/>"""
              """</w:style></w:styles>""")
    body = "".join(para(f, TEXT) for _, f, _ in LINES) + para(None, "Z")
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", HEAD + f"""<Types xmlns="{CT}">"""
                   """<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"""
                   """<Default Extension="xml" ContentType="application/xml"/>"""
                   """<Default Extension="ttf" ContentType="application/x-font-ttf"/>"""
                   """<Default Extension="odttf" ContentType="application/vnd.openxmlformats-officedocument.obfuscatedFont"/>"""
                   f"""<Override PartName="/word/document.xml" ContentType="{OOXML}.document.main+xml"/>"""
                   f"""<Override PartName="/word/styles.xml" ContentType="{OOXML}.styles+xml"/>"""
                   f"""<Override PartName="/word/fontTable.xml" ContentType="{OOXML}.fontTable+xml"/></Types>""")
        z.writestr("_rels/.rels", HEAD + f"""<Relationships xmlns="{PKG}"><Relationship Id="rId1" """
                   f"""Type="{REL}/officeDocument" Target="word/document.xml"/></Relationships>""")
        z.writestr("word/_rels/document.xml.rels", HEAD + f"""<Relationships xmlns="{PKG}">"""
                   f"""<Relationship Id="rId1" Type="{REL}/styles" Target="styles.xml"/>"""
                   f"""<Relationship Id="rId2" Type="{REL}/fontTable" Target="fontTable.xml"/></Relationships>""")
        z.writestr("word/_rels/fontTable.xml.rels", HEAD + f"""<Relationships xmlns="{PKG}">"""
                   f"""<Relationship Id="rId1" Type="{REL}/font" Target="{part}"/></Relationships>""")
        z.writestr("word/" + part, data)
        z.writestr("word/fontTable.xml", HEAD + table)
        z.writestr("word/styles.xml", HEAD + styles)
        z.writestr("word/document.xml", HEAD + f"""<w:document xmlns:w="{W}"><w:body>"""
                   + body + "</w:body></w:document>")

def pango_widths():
    import cairo
    import gi
    gi.require_version("Pango", "1.0")
    gi.require_version("PangoCairo", "1.0")
    from gi.repository import Pango, PangoCairo
    fm = PangoCairo.FontMap.get_default()
    # Cardo for this process only, as Slate loads it (Pango 1.56).
    fm.add_font_file(FONT)
    out = {}
    for name, _, family in LINES:
        ctx = fm.create_context()
        PangoCairo.context_set_resolution(ctx, 96)
        options = cairo.FontOptions()
        options.set_hint_metrics(cairo.HINT_METRICS_OFF)
        PangoCairo.context_set_font_options(ctx, options)
        ctx.set_round_glyph_positions(False)
        desc = Pango.FontDescription.from_string(family + " 11")
        layout = Pango.Layout.new(ctx)
        layout.set_font_description(desc)
        layout.set_text(TEXT, -1)
        face = fm.load_font(ctx, desc).describe().get_family()
        out[name] = (layout.get_pixel_extents()[0].width, face)
    return out

def measure(png):
    import gi
    gi.require_version("GdkPixbuf", "2.0")
    from gi.repository import GdkPixbuf
    p = GdkPixbuf.Pixbuf.new_from_file(png)
    w, h, rs, n, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
    bands, cur = [], None
    for y in range(int(os.environ["TOP"]), h - 10):
        row = px[y * rs:y * rs + w * n]
        xs = [x for x in range(40, w - 40) if row[x * n] + row[x * n + 1] + row[x * n + 2] < 600]
        if xs:
            if cur is None:
                cur = [y, y, xs[0], xs[-1]]
            cur[1], cur[2], cur[3] = y, min(cur[2], xs[0]), max(cur[3], xs[-1])
        elif cur is not None:
            bands.append(cur)
            cur = None
    print("bands: " + " ".join(f"{b[0]}-{b[1]}@{b[2]}-{b[3]}" for b in bands))
    if len(bands) < 4:
        print("too few lines")
        return
    want = pango_widths()
    shots = {}
    for (name, font, family), b in zip(LINES, bands):
        shot = b[3] - b[2] + 1
        width, face = want[name]
        shots[name] = shot
        verdict = "ok" if abs(shot - width) <= 2 else "off"
        print(f"{name} {font}: shot {shot} pango {family} {width} face {face} {verdict}")
    e, r, s = shots["embedded"], shots["roman"], shots["swiss"]
    print("embedded differs from serif: " + ("yes" if abs(e - r) > 2 else "no"))
    print("serif differs from sans: " + ("yes" if abs(r - s) > 2 else "no"))

def kept(original, saved):
    a, b = zipfile.ZipFile(original), zipfile.ZipFile(saved)
    names = [n for n in a.namelist() if n.startswith("word/fonts/") or n in FONT_PARTS]
    same = all(n in b.namelist() and a.read(n) == b.read(n) for n in names)
    edited = a.read("word/document.xml") != b.read("word/document.xml")
    yes = {True: "yes", False: "no"}
    print(f"font parts kept: {yes[same]} {len(names)}, document edited: {yes[edited]}")

if sys.argv[1] == "make":
    make(sys.argv[2], sys.argv[3])
elif sys.argv[1] == "measure":
    measure(sys.argv[2])
else:
    kept(sys.argv[2], sys.argv[3])
PY
# SLATE-20: `make PATH` writes fonts.docx; `measure PNG` measures its screenshot: each
# band of ink (a line of text) and its left and right edges, against Pango at 96 dpi and
# the twips of the styles.
cat > /tmp/fonts.py <<"PY"
import os
import sys
import zipfile

TEXT = "Sphinx of black quartz, judge my vow quickly."
FONTS = [("serif", "Liberation Serif"), ("sans", "Liberation Sans"),
         ("mono", "Liberation Mono"), ("calibri", "Calibri")]
BEFORE, AFTER, LEFT, HANG, EXACT = 450, 300, 900, 450, 600
W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG = "http://schemas.openxmlformats.org/package/2006/relationships"
CT = "http://schemas.openxmlformats.org/package/2006/content-types"
OOXML = "application/vnd.openxmlformats-officedocument.wordprocessingml"
HEAD = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"""

def rfonts(font):
    return f"""<w:rFonts w:ascii="{font}" w:hAnsi="{font}"/>"""

def style(sid, ppr="", rpr=""):
    return (f"""<w:style w:type="paragraph" w:styleId="{sid}"><w:name w:val="{sid}"/>"""
            f"""<w:basedOn w:val="Normal"/><w:pPr>{ppr}</w:pPr><w:rPr>{rpr}</w:rPr></w:style>""")

def para(sid, text):
    return (f"""<w:p><w:pPr><w:pStyle w:val="{sid}"/></w:pPr>"""
            f"""<w:r><w:t xml:space="preserve">{text}</w:t></w:r></w:p>""")

def make(path):
    styles = [
        f"""<w:docDefaults><w:rPrDefault><w:rPr>{rfonts("Liberation Sans")}<w:sz w:val="22"/>"""
        """</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before="0" w:after="0" """
        """w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>""",
        """<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>""",
    ]
    styles += [style(name.title(), rpr=rfonts(font)) for name, font in FONTS]
    styles += [
        style("Spaced", ppr=f"""<w:spacing w:before="{BEFORE}" w:after="{AFTER}"/>"""),
        style("Indented", ppr=f"""<w:ind w:left="{LEFT}"/>"""),
        style("Hanging", ppr=f"""<w:ind w:left="{LEFT}" w:hanging="{HANG}"/>"""),
        style("Exact", ppr=f"""<w:spacing w:line="{EXACT}" w:lineRule="exact"/>"""),
    ]
    paras = [para(name.title(), TEXT) for name, _ in FONTS]
    paras += [para("Normal", "Hamburg one"), para("Spaced", "Hamburg two"),
              para("Normal", "Hamburg three"), para("Indented", "Hamburg four"),
              para("Hanging", " ".join(["Hamburg"] * 20)), para("Exact", " ".join(["Hamburg"] * 30)),
              para("Normal", "Z")]
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("[Content_Types].xml", HEAD + f"""<Types xmlns="{CT}">"""
                   """<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"""
                   """<Default Extension="xml" ContentType="application/xml"/>"""
                   f"""<Override PartName="/word/document.xml" ContentType="{OOXML}.document.main+xml"/>"""
                   f"""<Override PartName="/word/styles.xml" ContentType="{OOXML}.styles+xml"/></Types>""")
        z.writestr("_rels/.rels", HEAD + f"""<Relationships xmlns="{PKG}"><Relationship Id="rId1" """
                   f"""Type="{REL}/officeDocument" Target="word/document.xml"/></Relationships>""")
        z.writestr("word/_rels/document.xml.rels", HEAD + f"""<Relationships xmlns="{PKG}">"""
                   f"""<Relationship Id="rId1" Type="{REL}/styles" Target="styles.xml"/></Relationships>""")
        z.writestr("word/styles.xml", HEAD + f"""<w:styles xmlns:w="{W}">""" + "".join(styles) + "</w:styles>")
        z.writestr("word/document.xml", HEAD + f"""<w:document xmlns:w="{W}"><w:body>"""
                   + "".join(paras) + "</w:body></w:document>")

def measure(png):
    import cairo
    import gi
    gi.require_version("GdkPixbuf", "2.0")
    gi.require_version("Pango", "1.0")
    gi.require_version("PangoCairo", "1.0")
    from gi.repository import GdkPixbuf, Pango, PangoCairo
    p = GdkPixbuf.Pixbuf.new_from_file(png)
    w, h, rs, n, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
    # Bands of rows with ink (anything darker than the paper), each [top, bottom, left, right].
    bands, cur = [], None
    for y in range(int(os.environ["TOP"]), h - 10):
        row = px[y * rs:y * rs + w * n]
        xs = [x for x in range(40, w - 40) if row[x * n] + row[x * n + 1] + row[x * n + 2] < 600]
        if xs:
            if cur is None:
                cur = [y, y, xs[0], xs[-1]]
            cur[1], cur[2], cur[3] = y, min(cur[2], xs[0]), max(cur[3], xs[-1])
        elif cur is not None:
            bands.append(cur)
            cur = None
    print("bands: " + " ".join(f"{b[0]}-{b[1]}@{b[2]}-{b[3]}" for b in bands))
    if len(bands) < 13:
        print("too few lines")
        return
    fm = PangoCairo.FontMap.get_default()
    # As GTK lays text out: metrics unhinted, glyphs at fractional positions (rounded is
    # printed too, for comparison).
    def pango(font, rounded):
        ctx = fm.create_context()
        PangoCairo.context_set_resolution(ctx, 96)
        options = cairo.FontOptions()
        options.set_hint_metrics(cairo.HINT_METRICS_OFF)
        PangoCairo.context_set_font_options(ctx, options)
        ctx.set_round_glyph_positions(rounded)
        desc = Pango.FontDescription.from_string(font + " 11")
        layout = Pango.Layout.new(ctx)
        layout.set_font_description(desc)
        layout.set_text(TEXT, -1)
        face = fm.load_font(ctx, desc).describe().get_family()
        return layout.get_pixel_extents()[0].width, face
    widths, match = {}, True
    for (name, font), b in zip(FONTS, bands):
        shot = b[3] - b[2] + 1
        want, face = pango(font, False)
        rounded, _ = pango(font, True)
        widths[name] = shot
        match = match and abs(shot - want) <= 2
        print(f"font {name}: shot {shot} pango {want} (rounded positions {rounded}) face {face}")
        if name == "calibri":
            print(f"calibri face: {face}")
    print("font widths match pango: " + ("yes" if match else "no"))
    a, b, c = widths["serif"], widths["sans"], widths["mono"]
    print("font widths differ: " + ("yes" if min(abs(a - b), abs(b - c), abs(a - c)) > 2 else "no"))
    one, two, three, four = bands[4:8]
    # Lines of the hanging paragraph are indented; the exact paragraph (and Z, last) not.
    rest, left0, hanging = bands[8:-1], three[2], []
    while rest and rest[0][2] - left0 > 15:
        hanging.append(rest.pop(0))
    line = four[0] - three[0]
    print(f"plain line: {line}")
    def report(label, got, want):
        verdict = "ok" if abs(got - want) <= 2 else "off"
        print(f"{label}: {got} (want {want}) {verdict}")
    report("space before", two[0] - one[0] - line, BEFORE // 15)
    report("space after", three[0] - two[0] - line, AFTER // 15)
    report("left indent", four[2] - left0, LEFT // 15)
    report("hanging first line", hanging[0][2] - left0 if hanging else -1, (LEFT - HANG) // 15)
    report("hanging next line", hanging[1][2] - left0 if len(hanging) > 1 else -1, LEFT // 15)
    pitches = [b2[0] - b1[0] for b1, b2 in zip(rest, rest[1:])]
    print("exact pitches: " + " ".join(map(str, pitches)))
    worst = max(pitches, key=lambda p: abs(p - EXACT // 15)) if pitches else -1
    report("exact line", worst, EXACT // 15)

make(sys.argv[2]) if sys.argv[1] == "make" else measure(sys.argv[2])
PY
# SLATE-21: `make PATH` writes bar.docx (styles Normal, Heading 2, and a custom Fancy);
# `line PNG` measures the first line of text below the format bar in a screenshot: the
# left and right edges of its ink and how far its middle is from the middle of the window.
# SLATE-22: `saved PATH ORIGINAL` reads the looks a saved bar.docx gives "beta".
cat > /tmp/bar.py <<"PY"
import os
import sys
import zipfile

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG = "http://schemas.openxmlformats.org/package/2006/relationships"
CT = "http://schemas.openxmlformats.org/package/2006/content-types"
OOXML = "application/vnd.openxmlformats-officedocument.wordprocessingml"
HEAD = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"""

def run(text, rpr=""):
    return f"""<w:r><w:rPr>{rpr}</w:rPr><w:t xml:space="preserve">{text}</w:t></w:r>"""

def para(sid, *runs):
    return f"""<w:p><w:pPr><w:pStyle w:val="{sid}"/></w:pPr>{"".join(runs)}</w:p>"""

def make(path):
    styles = (
        """<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Liberation Sans" w:hAnsi="Liberation Sans"/>"""
        """<w:sz w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before="0" w:after="120"/>"""
        """</w:pPr></w:pPrDefault></w:docDefaults>"""
        """<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>"""
        """<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/>"""
        """<w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style>"""
        """<w:style w:type="paragraph" w:customStyle="1" w:styleId="Fancy"><w:name w:val="Fancy"/>"""
        """<w:basedOn w:val="Normal"/><w:rPr><w:rFonts w:ascii="Liberation Serif" w:hAnsi="Liberation Serif"/>"""
        """<w:i/><w:sz w:val="28"/></w:rPr></w:style>"""
    )
    paras = [
        para("Normal", run("Alpha beta gamma")),
        para("Normal", run("plain "), run("bold", """<w:b/><w:sz w:val="28"/>""")),
        para("Fancy", run("Fancy words")),
        para("Normal", run("Z")),
    ]
    with zipfile.ZipFile(path, "w") as z:
        z.writestr("[Content_Types].xml", HEAD + f"""<Types xmlns="{CT}">"""
                   """<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"""
                   """<Default Extension="xml" ContentType="application/xml"/>"""
                   f"""<Override PartName="/word/document.xml" ContentType="{OOXML}.document.main+xml"/>"""
                   f"""<Override PartName="/word/styles.xml" ContentType="{OOXML}.styles+xml"/></Types>""")
        z.writestr("_rels/.rels", HEAD + f"""<Relationships xmlns="{PKG}"><Relationship Id="rId1" """
                   f"""Type="{REL}/officeDocument" Target="word/document.xml"/></Relationships>""")
        z.writestr("word/_rels/document.xml.rels", HEAD + f"""<Relationships xmlns="{PKG}">"""
                   f"""<Relationship Id="rId1" Type="{REL}/styles" Target="styles.xml"/></Relationships>""")
        z.writestr("word/styles.xml", HEAD + f"""<w:styles xmlns:w="{W}">{styles}</w:styles>""")
        z.writestr("word/document.xml", HEAD + f"""<w:document xmlns:w="{W}"><w:body>"""
                   + "".join(paras) + "</w:body></w:document>")

def line(png):
    import gi
    gi.require_version("GdkPixbuf", "2.0")
    from gi.repository import GdkPixbuf
    p = GdkPixbuf.Pixbuf.new_from_file(png)
    w, h, rs, n, px = p.get_width(), p.get_height(), p.get_rowstride(), p.get_n_channels(), p.get_pixels()
    top = int(os.environ["TOP"])
    left, right = None, None
    for y in range(top, h - 50):
        row = px[y * rs:y * rs + w * n]
        xs = [x for x in range(w) if row[x * n] + row[x * n + 1] + row[x * n + 2] < 450]
        if xs:
            left = xs[0] if left is None else min(left, xs[0])
            right = xs[-1] if right is None else max(right, xs[-1])
        elif left is not None:
            break
    if left is None:
        print("line none")
    else:
        print(f"line {left}-{right} off centre {(left + right) // 2 - w // 2}")

def saved(path, original):
    """SLATE-22: the looks of the saved paragraph holding "beta" and of the word, and
    whether the other paragraphs are as they were."""
    import re
    def paras(p):
        x = zipfile.ZipFile(p).read("word/document.xml").decode()
        return re.findall(r"<w:p[ >].*?</w:p>", x, re.S)
    def val(xml, tag, attr="val"):
        m = re.search(r"<w:" + tag + r"\b[^>]*?w:" + attr + r"=\"([^\"]*)\"", xml)
        return m.group(1) if m else "-"
    new, old = paras(path), paras(original)
    p = next(p for p in new if ">beta<" in p)
    r = next(r for r in re.findall(r"<w:r>.*?</w:r>", p, re.S) if ">beta<" in r)
    style, jc = val(p, "pStyle"), val(p, "jc")
    print(f"saved paragraph: style={style} jc={jc}")
    font, size, u, colour = val(r, "rFonts", "ascii"), val(r, "sz"), val(r, "u"), val(r, "color")
    print(f"saved beta: font={font} size={size} underline={u} colour={colour}")
    same = [q for q in new if q is not p] == [q for q in old if "Alpha beta gamma" not in q]
    print("saved others as they were: " + ("yes" if same else "no"))

if sys.argv[1] == "make":
    make(sys.argv[2])
elif sys.argv[1] == "saved":
    saved(sys.argv[2], sys.argv[3])
else:
    line(sys.argv[2])
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
  # SIGTERM ends Slate without closing its windows, which leaves crash journals of their
  # unsaved edits (SLATE-17): later sections start without them.
  quit() { pkill -x slate; sleep 1; rm -rf /tmp/state/slate/recovery; }
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
  # F10 in window TITLE (default col-a), Text Width (past New, Open, Save, Save As, Close
  # Window), the Nth width (1 Page, 2 Narrow, 3 Medium, 4 Wide, 5 Full).
  text_width() {
    swaymsg "[title=${2:-col-a}] focus" >/dev/null; sleep 1
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
  titles; measure page
  text_width 2; measure narrow
  text_width 3; measure medium
  text_width 4; measure wide
  text_width 5; measure full
  kill $other; sleep 1
  echo "== 9 restart"
  swaymsg "[title=col-a] focus" >/dev/null; sleep 1
  keys -M ctrl q -m ctrl; sleep 2
  pgrep -x slate >/dev/null && echo "after quit: slate still running" || echo "after quit: slate exited"
  echo "settings: $(cat /tmp/config/slate/settings 2>/dev/null)"
  slate /tmp/col-a.md >>/tmp/app.log 2>&1 & sleep 4
  measure restart col-a
  echo "== 10 narrow window"
  text_width 4; measure wide1280 col-a
  swaymsg "[title=col-a] floating enable, resize set width 640 px height 600 px, move position center" >/dev/null; sleep 1
  measure wide640 col-a
  text_width 2; measure narrow640 col-a
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
  # Tabbed: the tab bar of sway moves the window content 25 px down.
  TOP=$((TOP + 25)) python3 /tmp/px.py /shots/rich.png | sed -n "s/^third ink: /rich bold ink: /p"
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
  echo "== 23 fonts"
  echo "fc-match calibri: $(fc-match -f "%{family}" Calibri)"
  python3 /tmp/fonts.py make /tmp/fonts.docx
  slate /tmp/fonts.docx >>/tmp/app.log 2>&1 & sleep 4
  # The caret on the last line, Z, which the measurements leave out.
  keys -M ctrl -k End -m ctrl; sleep 1; shot fonts
  python3 /tmp/fonts.py measure /shots/fonts.png 2>&1; quit
  echo "== 24 format bar"
  python3 /tmp/bar.py make /tmp/bar.docx
  bar() { python3 /tmp/a11y.py bar 2>&1 | sed "s/^/$1 /"; }
  attrs() { python3 /tmp/a11y.py attrs bar.docx ${2:-7} 2>&1 | sed "s/^/$1 /"; }
  # A click at the widget named $1 (of role $3, if given), at fraction $2 of its width.
  click() { read -r x y <<<"$(python3 /tmp/a11y.py where "$1" ${2:-0.5} ${3:-})"; python /dnd.py click "$x" "$y"; sleep 1; }
  # Select "beta" in "Alpha beta gamma".
  beta() {
    keys -M ctrl -k Home -m ctrl -s 200 -M ctrl -k Right -m ctrl -s 200 -k Right -s 200 \
      -M shift -M ctrl -k Right -m ctrl -m shift; sleep 1
  }
  slate /tmp/fixture.docx >>/tmp/app.log 2>&1 & sleep 4; bar fixture; shot bar-fixture; quit
  slate /tmp/bar.docx >>/tmp/app.log 2>&1 & sleep 4
  keys -M ctrl -k End -m ctrl; sleep 1; bar caret; shot bar-start
  python3 /tmp/bar.py line /shots/bar-start.png | sed "s/^/start /"
  beta; bar word; attrs plain
  click Font; keys "Liberation Mono"; sleep 1; shot font-list; click "Liberation Mono" 0.5 label
  bar font; attrs font
  click Size 0.2; keys -M ctrl a -m ctrl "20" -k Return; sleep 1; bar size; attrs size
  python3 /tmp/a11y.py press Underline; sleep 1; bar underline; attrs underline
  # The dialog is a window of its own: its swatch and button are pressed through AT-SPI.
  click Colour; sleep 2; shot colour-dialog; python3 /tmp/a11y.py press Red; sleep 1
  python3 /tmp/a11y.py press Select 2>/dev/null; sleep 1
  bar colour; attrs colour
  python3 /tmp/a11y.py press Centre; sleep 1; bar centre; shot bar-centred
  python3 /tmp/bar.py line /shots/bar-centred.png | sed "s/^/centred /"
  click Style; sleep 1; shot style-list; click Fancy 0.5 label; bar style; attrs style; attrs style-rest 1
  shot bar-styled
  # One Ctrl+Z for each change, newest first.
  for step in style centre colour underline size font; do
    keys -M ctrl z -m ctrl; sleep 1; bar undo-$step; attrs undo-$step
  done
  shot bar-undone; python3 /tmp/bar.py line /shots/bar-undone.png | sed "s/^/undone /"
  # The keys: Ctrl+U, Ctrl+] (twice), and Ctrl+E on "beta".
  beta; keys -M ctrl u -m ctrl -s 200 -M ctrl "]]" -m ctrl -s 200 -M ctrl e -m ctrl; sleep 1
  bar keys; attrs keys; shot bar-keys; python3 /tmp/bar.py line /shots/bar-keys.png | sed "s/^/keys /"
  keys -M ctrl "[" -m ctrl; sleep 1; attrs smaller
  # A selection of plain text and bold text at 14 pt; then the bold word alone.
  keys -M ctrl -k Home -m ctrl -s 200 -k Down -s 200 -k Home -s 200 -M shift -k End -m shift; sleep 1
  bar mixed
  keys -k End -s 200 -M shift -M ctrl -k Left -m ctrl -m shift; sleep 1; bar bold-word
  # Everything: two styles, two fonts.
  keys -M ctrl -k Home -m ctrl -s 200 -M shift -M ctrl -k End -m ctrl -m shift; sleep 1
  bar all; shot bar-mixed
  # Normal, the item the style list still holds though it shows none, makes all Normal.
  click Style; sleep 1; click Normal 0.5 label; sleep 1; bar restyled; attrs restyled 29; quit
  # Plain text has no format bar.
  slate /tmp/w.txt >>/tmp/app.log 2>&1 & sleep 4; bar txt; shot bar-txt; quit
  echo "== 25 save looks"
  cp /tmp/bar.docx /tmp/save/bar.docx
  slate /tmp/save/bar.docx >>/tmp/app.log 2>&1 & sleep 4
  # The bar edits of section 24 on "beta": font, size, underline, colour, centre, Fancy.
  beta; click Font; keys "Liberation Mono"; sleep 1; click "Liberation Mono" 0.5 label
  click Size 0.2; keys -M ctrl a -m ctrl "20" -k Return; sleep 1
  python3 /tmp/a11y.py press Underline; sleep 1
  click Colour; sleep 2; python3 /tmp/a11y.py press Red; sleep 1
  python3 /tmp/a11y.py press Select 2>/dev/null; sleep 1
  python3 /tmp/a11y.py press Centre; sleep 1
  click Style; sleep 1; click Fancy 0.5 label; attrs edited
  keys -M ctrl s -m ctrl; sleep 1; saved; titles | sed "s/^/looks-saved-/"
  python3 /tmp/bar.py saved /tmp/save/bar.docx /tmp/bar.docx; quit
  slate /tmp/save/bar.docx >>/tmp/app.log 2>&1 & sleep 4
  attrs reopened; attrs reopened-rest 1
  keys -M ctrl -k End -m ctrl; sleep 1; shot looks-reopened
  python3 /tmp/bar.py line /shots/looks-reopened.png | sed "s/^/reopened /"
  beta; bar reopened; quit
  echo "== 26 find"
  printf "A cat sat.\n\nThe Cat and the cat.\n\n| x | y |\n|---|---|\n| cat | dog |\n\nLast cat.\n" > /tmp/find.md
  slate /tmp/find.md >>/tmp/app.log 2>&1 & sleep 4
  count() { a11y dump | grep -E "^([0-9]+ of [0-9]+|No matches)$" | head -1 | sed "s/^/$1: /"; }
  keys -M ctrl f -m ctrl; sleep 1; keys cat; sleep 1; count typed; shot find
  keys -k Return; sleep 0.5; count enter-1
  keys -k Return; sleep 0.5; count enter-2
  keys -k Return -s 300 -k Return; sleep 0.5; count enter-4
  keys -k Escape; sleep 0.5; keys X; sleep 1; a11y page find.md | sed "s/^/into-cell /"
  keys -M ctrl f -m ctrl; sleep 1; count again
  keys -M shift -k Return -m shift; sleep 0.5; count back-1
  keys -M shift -k Return -m shift; sleep 0.5; count back-2
  keys -k Tab -k space; sleep 1; count case
  keys -M ctrl h -m ctrl; sleep 1; keys -k Tab -k Tab -k Tab -k Tab -k Tab; keys dog
  keys -k Return; sleep 0.5; keys -k Return; sleep 1; a11y page find.md | sed "s/^/replaced-one /"
  keys -k Tab -k Tab -k space; sleep 1; a11y page find.md | sed "s/^/replaced-all /"
  count after-all; shot replace
  keys -k Escape; sleep 0.5; keys -M ctrl z -m ctrl; sleep 1; a11y page find.md | sed "s/^/undo-all /"
  keys -M ctrl z -m ctrl; sleep 1; a11y page find.md | sed "s/^/undo-one /"
  a11y dump | grep -cE "^([0-9]+ of [0-9]+|No matches)$" | sed "s/^/counts after closing: /"; quit
  echo "== 27 recovery"
  J=/tmp/state/slate/recovery; rm -rf $J /tmp/rec; mkdir -p /tmp/rec
  journals() { ls $J 2>/dev/null | grep -c "\.journal$"; }
  printf "plain line\n" > /tmp/rec/r.txt; printf "# Notes\n\nmd line\n" > /tmp/rec/r.md
  cp /tmp/fixture.docx /tmp/rec/r.docx; sha256sum /tmp/rec/r.* > /tmp/rec.sums
  slate /tmp/rec/r.txt /tmp/rec/r.md /tmp/rec/r.docx >>/tmp/app.log 2>&1 & sleep 5
  for f in r.txt r.md r.docx; do
    swaymsg "[title=$f] focus" >/dev/null; sleep 0.5; keys -M ctrl -k End -m ctrl; keys " Unsaved"
    [ $f = r.txt ] && echo "journals right after typing: $(journals)"
  done
  sleep 7; echo "journals: $(journals)"; titles | sed "s/^/edited-/"
  old=$(pgrep -x slate); pkill -9 -x slate; sleep 1
  sha256sum -c --quiet /tmp/rec.sums && echo "files unchanged after the kill"
  echo "journals after the kill: $(journals)"
  slate >>/tmp/app.log 2>&1 & sleep 4; shot recovery-start
  a11y dump | grep -E "^r\.(docx|md|txt)$" | sort -u | tr "\n" "," | sed "s/^/offered: /"; echo
  a11y choose Restore r.txt; sleep 3; titles | sed "s/^/from-start-/"
  a11y page r.txt | sed "s/^/restored-txt /"
  slate /tmp/rec/r.md /tmp/rec/r.docx >>/tmp/app.log 2>&1 & sleep 5; shot recovery-ask
  echo "asked: $(a11y count "Restore unsaved edits?")"
  a11y choose Restore dialog; sleep 3; titles | sed "s/^/restored-/"
  a11y page r.md | sed "s/^/restored-md /"; a11y page r.docx | grep -c "Unsaved$" | sed "s/^/restored-docx: /"
  sha256sum -c --quiet /tmp/rec.sums && echo "files unchanged after restoring"
  echo "old journals left: $(ls $J | grep -c "^$old-")"
  sleep 6; echo "journals while restored: $(journals)"
  swaymsg "[title=r.txt] focus" >/dev/null; sleep 0.5; keys -M ctrl z -m ctrl; sleep 1; titles | sed "s/^/undone-/"
  swaymsg "[title=r.md] focus" >/dev/null; sleep 0.5; keys -M ctrl s -m ctrl; sleep 2; saved
  for f in r.txt r.md r.docx Slate; do swaymsg "[title=$f] focus" >/dev/null; sleep 0.5; keys -M ctrl w -m ctrl; sleep 1; done
  a11y choose Discard dialog; sleep 2
  pgrep -x slate >/dev/null && echo "after closing: slate still running" || echo "after closing: slate exited"
  echo "journals after a clean close: $(journals)"
  grep -c "^md line Unsaved$" /tmp/rec/r.md | sed "s/^/md saved: /"
  grep -v r.md /tmp/rec.sums | sha256sum -c --quiet && echo "txt and docx unchanged"
  pkill -9 -x slate; rm -rf $J; sleep 1
  slate /tmp/rec/r.txt >>/tmp/app.log 2>&1 & sleep 4
  keys -M ctrl -k End -m ctrl; keys " Again"; sleep 7; pkill -9 -x slate; sleep 1
  slate /tmp/rec/r.txt >>/tmp/app.log 2>&1 & sleep 4; a11y choose Discard dialog; sleep 1
  a11y page r.txt | sed "s/^/discarded-txt /"; echo "journals after discarding: $(journals)"; titles | sed "s/^/discarded-/"
  pid=$(pgrep -x slate | head -1)
  # Type a letter every half second for 20 s; note when the journal first shows up and
  # each time it is rewritten (its modification time).
  keys -M ctrl -k End -m ctrl; t0=$(date +%s%3N); first=""; : > /tmp/rec.times
  while [ $(( $(date +%s%3N) - t0 )) -lt 20000 ]; do
    wtype a; f=$(ls $J/$pid-*.journal 2>/dev/null | head -1)
    if [ -n "$f" ]; then stat -c %.3Y "$f" >> /tmp/rec.times; [ -z "$first" ] && first=$(( $(date +%s%3N) - t0 )); fi
    sleep 0.5
  done
  python3 -c "
t = sorted({float(l) for l in open(\"/tmp/rec.times\")})
gap = min((b - a for a, b in zip(t, t[1:])), default=0)
print(f\"typing for 20 s: first journal after ${first:-0} ms, writes {len(t)}, shortest gap {gap * 1000:.0f} ms\")"
  quit
  echo "== 28 page width"
  rm -f /tmp/config/slate/settings
  python3 /tmp/page.py /tmp/page-75.docx 720; python3 /tmp/page.py /tmp/page-none.docx
  slate /tmp/page-75.docx /tmp/page-none.docx >>/tmp/app.log 2>&1 & sleep 5
  swaymsg "[title=page-75] focus, layout tabbed" >/dev/null; sleep 1
  titles
  # Screenshot each window named (focused in turn) and measure it: "LABEL NAME: rule ...".
  pages() {
    local label=$1; shift
    for t in "$@"; do
      swaymsg "[title=$t] focus" >/dev/null; sleep 1; shot $label-$t
      echo "$label $t.docx: $(python3 /tmp/col.py /shots/$label-$t.png)"
    done
  }
  pages page page-75 page-none
  text_width 3 page-75; pages medium page-75 page-none
  text_width 1 page-75; pages page-again page-75
  echo "settings: $(cat /tmp/config/slate/settings 2>/dev/null)"
  slate /tmp/col-a.md >>/tmp/app.log 2>&1 & sleep 4
  measure pagemd col-a
  quit
  echo "== 29 tab stops"
  python3 /tmp/tabs.py make /tmp/tabs.docx
  printf "Tabbed\tline\n" > /tmp/tab.md
  slate /tmp/tabs.docx >>/tmp/app.log 2>&1 & sleep 4
  titles
  # The caret on the last line, Z, below the table the measurements stop at.
  keys -M ctrl -k End -m ctrl; sleep 1; shot tabs
  python3 /tmp/tabs.py measure /shots/tabs.png 2>&1; quit
  slate /tmp/tab.md >>/tmp/app.log 2>&1 & sleep 4
  titles | sed "s/^/md-/"; a11y page tab.md | sed "s/^/md-tab /"; shot tab-md; quit
  echo "== 30 embedded fonts"
  echo "cardo installed: $(fc-list | grep -ci cardo)"
  for mode in plain obfuscated; do
    python3 /tmp/embed.py make /tmp/embed-$mode.docx $mode
    cp /tmp/embed-$mode.docx /tmp/save/embed-$mode.docx
    slate /tmp/save/embed-$mode.docx >>/tmp/app.log 2>&1 & sleep 4
    titles | sed "s/^/$mode-/"
    # The caret on the last line, Z, which the measurements leave out.
    keys -M ctrl -k End -m ctrl; sleep 1; shot embed-$mode
    python3 /tmp/embed.py measure /shots/embed-$mode.png 2>&1 | sed "s/^/$mode /"
    keys "!"; keys -M ctrl s -m ctrl; sleep 1; saved
    python3 /tmp/embed.py kept /tmp/embed-$mode.docx /tmp/save/embed-$mode.docx 2>&1 | sed "s/^/$mode /"
    quit
  done
  echo "private fonts: $(ls /tmp/rt/slate/fonts | wc -l) mode $(stat -c %a /tmp/rt/slate/fonts)"
  echo "cardo installed after: $(fc-list | grep -ci cardo)"
  echo "user font files: $(find ~/.local/share/fonts ~/.fonts -type f 2>/dev/null | wc -l)"
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
for w in page:624 narrow:480 medium:624 wide:816 full:1232; do
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
check "23: Calibri is Carlito (fontconfig)" "23 fonts" "^fc-match calibri: Carlito$"
check "23: each font's line is as wide as Pango measures it, within 2 px" "23 fonts" \
  "^font widths match pango: yes$"
check "23: Liberation Serif, Sans, and Mono draw at different widths" "23 fonts" "^font widths differ: yes$"
check "23: a Calibri paragraph draws in Carlito" "23 fonts" "^calibri face: Carlito$"
check "23: space before adds to the line above's" "23 fonts" "^space before: .* ok$"
check "23: space after adds to the line below's" "23 fonts" "^space after: .* ok$"
check "23: a left indent moves the text in" "23 fonts" "^left indent: .* ok$"
check "23: a hanging first line starts left of the indent" "23 fonts" "^hanging first line: .* ok$"
check "23: the hanging paragraph's next line starts at the indent" "23 fonts" "^hanging next line: .* ok$"
check "23: exact line spacing sets every line's pitch" "23 fonts" "^exact line: .* ok$"
s24="24 format bar"
word="family-name=Liberation"
check "24: a .docx window has the format bar" "$s24" "^fixture bar: fonts=[1-9]"
check "24: the bar shows the caret's style, font, size, and looks" "$s24" \
  "^caret bar: fonts=[0-9]+ style=Normal font=Liberation Sans size=11 bold=off italic=off underline=off strike=off left=on "
check "24: the font list picks Liberation Mono for the word" "$s24" \
  "^font attrs 6-10: $word Mono size=11 weight=400 style=normal underline=none strikethrough=false fg-color=30,30,30$"
check "24: ... and the bar shows it" "$s24" "^font bar: .* font=Liberation Mono size=11 "
check "24: the size field sets 20 pt" "$s24" "^size attrs 6-10: $word Mono size=20 .* underline=none "
check "24: ... and shows it" "$s24" "^size bar: .* size=20 "
check "24: Underline underlines the word" "$s24" "^underline attrs 6-10: $word Mono size=20 .* underline=single "
check "24: ... and shows on" "$s24" "^underline bar: .* underline=on "
check "24: the colour dialog colours the word red" "$s24" \
  "^colour attrs 6-10: $word Mono size=20 .* underline=single strikethrough=false fg-color=224,27,36$"
check "24: Centre centres the line" "$s24" "^centred line .* off centre -?[0-2]$"
check "24: ... which was at the left" "$s24" "^start line .* off centre -[0-9]{3}$"
check "24: ... and shows on" "$s24" "^centre bar: .* left=off centre=on "
check "24: the style list gives the paragraph the custom style Fancy" "$s24" \
  "^style-rest attrs 0-6: $word Serif size=14 weight=400 style=italic "
check "24: ... the word keeps its own looks" "$s24" \
  "^style attrs 6-10: $word Mono size=20 weight=400 style=italic underline=single strikethrough=false fg-color=224,27,36$"
check "24: ... and the bar shows Fancy" "$s24" "^style bar: fonts=[0-9]+ style=Fancy "
check "24: Ctrl+Z undoes the style alone" "$s24" "^undo-style bar: fonts=[0-9]+ style=Normal .* centre=on "
check "24: ... the word as it was" "$s24" \
  "^undo-style attrs 6-10: $word Mono size=20 weight=400 style=normal underline=single strikethrough=false fg-color=224,27,36$"
check "24: the next undoes the centring alone" "$s24" "^undo-centre bar: .* left=on centre=off "
check "24: ... the word as it was" "$s24" "^undo-centre attrs 6-10: $word Mono size=20 .* fg-color=224,27,36$"
check "24: the next undoes the colour alone" "$s24" \
  "^undo-colour attrs 6-10: $word Mono size=20 .* underline=single strikethrough=false fg-color=30,30,30$"
check "24: the next undoes the underline alone" "$s24" "^undo-underline attrs 6-10: $word Mono size=20 .* underline=none "
check "24: the next undoes the size alone" "$s24" "^undo-size attrs 6-10: $word Mono size=11 "
check "24: the next undoes the font: the line is as opened" "$s24" \
  "^undo-font attrs 0-16: $word Sans size=11 weight=400 style=normal underline=none strikethrough=false fg-color=30,30,30$"
check "24: ... at the left" "$s24" "^undone line .* off centre -[0-9]{3}$"
check "24: Ctrl+U underlines and Ctrl+] twice makes 13 pt" "$s24" "^keys attrs 6-10: $word Sans size=13 .* underline=single "
check "24: Ctrl+E centres" "$s24" "^keys line .* off centre -?[0-2]$"
check "24: ... and the bar shows them" "$s24" "^keys bar: .* size=13 .* underline=on .* centre=on "
check "24: Ctrl+[ makes 12 pt" "$s24" "^smaller attrs 6-10: $word Sans size=12 "
check "24: plain and bold text at two sizes shows Bold off and the size blank" "$s24" "^mixed bar: .* size= bold=off "
check "24: the bold word alone shows Bold on and its size" "$s24" "^bold-word bar: .* size=14 bold=on "
check "24: a selection over two styles and fonts shows the style, font, and size blank" "$s24" \
  "^all bar: fonts=[0-9]+ style= font= size= "
check "24: Normal picked there, the item the list still held, makes everything Normal" "$s24" \
  "^restyled attrs [0-9]+-[0-9]+: $word Sans size=11 weight=400 style=normal "
check "24: ... and the bar shows Normal" "$s24" "^restyled bar: fonts=[0-9]+ style=Normal "
check "24: a .txt window has no format bar" "$s24" "^txt bar: fonts=0 "
s25="25 save looks"
check "25: the bar's looks on the word before saving" "$s25" \
  "^edited attrs 6-10: $word Mono size=20 weight=400 style=italic underline=single strikethrough=false fg-color=224,27,36$"
check "25: Ctrl+S saves bar.docx" "$s25" "^looks-saved-titles: bar.docx,$"
check "25: the saved paragraph is in Fancy, centred" "$s25" "^saved paragraph: style=Fancy jc=center$"
check "25: the saved word is Liberation Mono 20 pt, underlined, red" "$s25" \
  "^saved beta: font=Liberation Mono size=40 underline=single colour=E01B24$"
check "25: the other paragraphs are saved as they were" "$s25" "^saved others as they were: yes$"
check "25: reopened, the word shows its looks" "$s25" \
  "^reopened attrs 6-10: $word Mono size=20 weight=400 style=italic underline=single strikethrough=false fg-color=224,27,36$"
check "25: ... the rest of the line Fancy's" "$s25" "^reopened-rest attrs 0-6: $word Serif size=14 weight=400 style=italic "
check "25: ... the line centred" "$s25" "^reopened line .* off centre -?[0-2]$"
check "25: ... and the bar shows them" "$s25" \
  "^reopened bar: fonts=[0-9]+ style=Fancy font=Liberation Mono size=20 .* underline=on .* centre=on "
check "26: Ctrl+F and typing count the matches, from the caret" "26 find" "^typed: 1 of 5$"
check "26: Enter selects the current match first" "26 find" "^enter-1: 1 of 5$"
check "26: Enter goes to the next match" "26 find" "^enter-2: 2 of 5$"
check "26: and on into a table cell" "26 find" "^enter-4: 4 of 5$"
check "26: Escape leaves the cell match selected on the page" "26 find" \
  "^into-cell page: A cat sat\.\|The Cat and the cat\.\|x\|y\|X\|dog\|Last cat\.$"
check "26: Ctrl+F again counts from the caret" "26 find" "^again: 4 of 4$"
check "26: Shift+Enter goes to the match before the caret" "26 find" "^back-1: 3 of 4$"
check "26: and the one before that" "26 find" "^back-2: 2 of 4$"
check "26: Match Case leaves out Cat" "26 find" "^case: [0-9] of 3$"
check "26: Replace selects the match, then replaces it" "26 find" \
  "^replaced-one page: A cat sat\.\|The Cat and the dog\.\|x\|y\|X\|dog\|Last cat\.$"
check "26: Replace All replaces the rest, keeping Cat" "26 find" \
  "^replaced-all page: A dog sat\.\|The Cat and the dog\.\|x\|y\|X\|dog\|Last dog\.$"
check "26: then nothing is left to find" "26 find" "^after-all: No matches$"
check "26: one undo takes back Replace All" "26 find" \
  "^undo-all page: A cat sat\.\|The Cat and the dog\.\|x\|y\|X\|dog\|Last cat\.$"
check "26: the next takes back Replace" "26 find" \
  "^undo-one page: A cat sat\.\|The Cat and the cat\.\|x\|y\|X\|dog\|Last cat\.$"
check "26: closing the bar clears the count" "26 find" "^counts after closing: 0$"
check "27: a journal isn't written as soon as an edit is made" "27 recovery" "^journals right after typing: 0$"
check "27: within seconds each edited window has one" "27 recovery" "^journals: 3$"
check "27: the windows show unsaved edits" "27 recovery" "^edited-titles: • r\.docx,• r\.md,• r\.txt,$"
check "27: kill -9 leaves the files as they were" "27 recovery" "^files unchanged after the kill$"
check "27: and the journals" "27 recovery" "^journals after the kill: 3$"
check "27: the start window offers each" "27 recovery" "^offered: r\.docx,r\.md,r\.txt,$"
check "27: Restore there opens the file marked unsaved, the list still showing" "27 recovery" \
  "^from-start-titles: Slate,• r\.txt,$"
check "27: with the edit" "27 recovery" "^restored-txt page: plain line Unsaved$"
check "27: opening the others asks in each" "27 recovery" "^asked: 2$"
check "27: Restore puts their edits back, marked unsaved" "27 recovery" \
  "^restored-titles: Slate,• r\.docx,• r\.md,• r\.txt,$"
check "27: the Markdown edit" "27 recovery" "^restored-md page: Notes\|md line Unsaved$"
check "27: the Word document edit" "27 recovery" "^restored-docx: 1$"
check "27: the files stay as they were until saved" "27 recovery" "^files unchanged after restoring$"
check "27: restored journals are gone" "27 recovery" "^old journals left: 0$"
check "27: the restored windows journal their unsaved edits again" "27 recovery" "^journals while restored: 3$"
check "27: one undo gives back the file (no longer unsaved)" "27 recovery" "^undone-titles: Slate,r\.txt,• r\.docx,• r\.md,$"
check "27: saving, undoing, and discarding, then closing, ends Slate" "27 recovery" "^after closing: slate exited$"
check "27: and leaves no recovery data" "27 recovery" "^journals after a clean close: 0$"
check "27: the saved file has the edit" "27 recovery" "^md saved: 1$"
check "27: the others are as they were" "27 recovery" "^txt and docx unchanged$"
check "27: Discard after a crash keeps the file as it is" "27 recovery" "^discarded-txt page: plain line$"
check "27: and leaves no recovery data either" "27 recovery" "^journals after discarding: 0$"
check "27: typing, the journal is written 5 s after the first edit, then at most every 5 s" \
  "27 recovery" "^typing for 20 s: first journal after (4[5-9]|[5-8][0-9])[0-9]{2} ms, writes [234], shortest gap ([5-9][0-9]{3}|[0-9]{5}) ms$"
check "28: both Word documents open in their own windows" "28 page width" "^titles: .*page-75\.docx.*page-none\.docx"
column "28: under Page, the default, Letter with 0.5″ margins measures 720 px" "28 page width" "page page-75\.docx" 720
column "28: under Page, a .docx without w:sectPr measures 624 px (Letter, 1″ margins)" "28 page width" \
  "page page-none\.docx" 624
column "28: Medium makes the 7.5″ page 624 px" "28 page width" "medium page-75\.docx" 624
column "28: Medium leaves the 6.5″ page 624 px" "28 page width" "medium page-none\.docx" 624
column "28: Page again restores 720 px" "28 page width" "page-again page-75\.docx" 720
check "28: the settings file says page" "28 page width" "^settings: text-width=page$"
column "28: a Markdown window under Page measures Medium's 624 px" "28 page width" "pagemd col-a\.md" 624
check "29: tabs.docx opens" "29 tab stops" "^titles: .*tabs\.docx"
check "29: the column is the page's 720 px" "29 tab stops" "^column: [0-9]+ (719|720|721)$"
check "29: a right stop at the text width ends Remote at the column's right edge" "29 tab stops" \
  "^remote right edge: .* ok$"
check "29: text after three default tabs starts at a multiple of the default stop" "29 tab stops" \
  "^x after three default tabs: .* ok$"
check "29: a centre stop centres its text on it" "29 tab stops" "^middle centre: .* ok$"
check "29: a right stop with a leader ends its text at the stop" "29 tab stops" "^42 right edge: .* ok$"
check "29: the dot leader fills the gap on the baseline, nothing above it" "29 tab stops" \
  "^leader fills the gap on the baseline: yes$"
check "29: a Markdown file with a tab opens" "29 tab stops" "^md-titles: .*tab\.md"
check "29: and reads as written" "29 tab stops" "^md-tab page: Tabbed.line$"
check "30: Cardo isn't installed in the container" "30 embedded fonts" "^cardo installed: 0$"
for mode in plain obfuscated; do
  check "30: ($mode) the document opens" "30 embedded fonts" "^$mode-titles: .*embed-$mode\.docx"
  check "30: ($mode) the embedded Cardo line measures as Pango measures Cardo, within 2 px" \
    "30 embedded fonts" "^$mode embedded Cardo: .* face Cardo ok$"
  check "30: ($mode) ... unlike the system serif" "30 embedded fonts" "^$mode embedded differs from serif: yes$"
  check "30: ($mode) a roman font neither installed nor embedded draws in the system serif" \
    "30 embedded fonts" "^$mode roman Nosuchfont Serifish: .* ok$"
  check "30: ($mode) a swiss one in the system sans" "30 embedded fonts" "^$mode swiss Nosuchfont Sansish: .* ok$"
  check "30: ($mode) ... which differ" "30 embedded fonts" "^$mode serif differs from sans: yes$"
  check "30: ($mode) saving an edit keeps fontTable.xml, its relationships, and the font part" \
    "30 embedded fonts" "^$mode font parts kept: yes 3, document edited: yes$"
done
check "30: the face is written once, to a private directory" "30 embedded fonts" "^private fonts: 1 mode 700$"
check "30: and isn't installed for the user" "30 embedded fonts" "^cardo installed after: 0$"
check "30: nothing goes in the user's font directories" "30 embedded fonts" "^user font files: 0$"
if [ $fail = 0 ]; then echo "SLATE OK"; else
  echo "SLATE CHECK FAILED"; [ -n "${VERBOSE:-}" ] || echo "(VERBOSE=1 for the transcript)"
  exit 1
fi
