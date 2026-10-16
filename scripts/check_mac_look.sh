#!/usr/bin/env bash
# MAC-6's look check on a macOS runner (ci.yml, job macos-look): fonts and dark mode.
#
# Usage: scripts/check_mac_look.sh [OUTDIR]   (out by default; shots/ and logs/ inside)
#
# Needs a logged-in macOS session with screen capture and System Events allowed (GitHub's
# macOS runners), Homebrew's gtk4, the apps built in target/debug, and the fonts fetched
# into target/fonts (python3 scripts/fetch_fonts.py). Draws with GSK_RENDERER=cairo: GL
# drew black windows on the runner VMs (DEC-11).
#
#   1. Light appearance. Brick on a CSV of digits and Slate on office-fonts.docx
#      (scripts/office_fonts.py docx: Calibri, Cambria, Arial, Times New Roman, Courier
#      New), each with the suite's Pango backend (fontconfig) and, for comparison,
#      CoreText (PANGOCAIRO_BACKEND=coretext); Frame on its fixture deck. Each app's log
#      names the fonts folder it added.
#   2. Dark appearance, set as System Settings does (System Events): Brick, Slate, Frame,
#      and Etch draw dark.
#   3. Following changes: Slate starts light; the appearance turns dark, then light,
#      while it runs, and its window follows; Brick's window, grid included, turns dark.
# Dark or light is the mean brightness (0-255) of a strip across the top of the app's
# window (the header bar; Etch, with the Mac's own title bar, its window's middle): dark
# under 100, light over 150. The screenshots are for looking at: text in Carlito and
# Caladea, Brick's digits whole. Ends with "LOOK OK" or "LOOK FAILED".
set -uo pipefail
cd "$(dirname "$0")/.."
out=$(mkdir -p "${1:-out}" && cd "${1:-out}" && pwd)
shots=$out/shots logs=$out/logs work=$out/work
mkdir -p "$shots" "$logs" "$work"
bin=$PWD/target/debug
export GSK_RENDERER=cairo
failed=0

say() { echo "$*" | tee -a "$logs/summary.txt"; }
check() { # check NAME CONDITION...
  local name=$1; shift
  if "$@"; then say "ok: $name"; else say "FAILED: $name"; failed=1; fi
}

# The bounds of process $1's largest on-screen window, as "x y w h" in screen points.
cat >"$work/bounds.swift" <<'SWIFT'
import CoreGraphics
import Foundation
let pid = Int(CommandLine.arguments[1])!
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
var best: (Int, Int, Int, Int)? = nil
for w in list where (w[kCGWindowOwnerPID as String] as? Int) == pid && (w[kCGWindowLayer as String] as? Int) == 0 {
    let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
    let r = ((b["X"] as? Int) ?? 0, (b["Y"] as? Int) ?? 0, (b["Width"] as? Int) ?? 0, (b["Height"] as? Int) ?? 0)
    if best == nil || r.2 * r.3 > best!.2 * best!.3 { best = r }
}
if let b = best { print("\(b.0) \(b.1) \(b.2) \(b.3)") }
SWIFT
swiftc -O "$work/bounds.swift" -o "$work/bounds" >"$logs/swiftc.log" 2>&1 || { say "FAILED: swiftc"; exit 1; }

# Mean brightness of a window's part on screen, from a BMP screenshot: a strip across the
# top (rows 8 to 44, the middle half across) and the middle half; prints "TOP MIDDLE".
cat >"$work/brightness.py" <<'PY'
import struct, sys
bmp, x, y, w, h = sys.argv[1], *map(int, sys.argv[2:6])
data = open(bmp, "rb").read()
offset, = struct.unpack_from("<I", data, 10)
width, height, _, bpp = struct.unpack_from("<iiHH", data, 18)
bottom_up = height > 0
height = abs(height)
row = (width * bpp // 8 + 3) & ~3
x0, y0 = max(x, 0), max(y, 0)
x1, y1 = min(x + w, width), min(y + h, height)
def mean(xa, xb, ya, yb):
    total = count = 0
    for yy in range(ya, yb, 2):
        base = offset + ((height - 1 - yy) if bottom_up else yy) * row
        for xx in range(xa, xb, 2):
            b, g, r = data[base + xx * bpp // 8: base + xx * bpp // 8 + 3]
            total += (299 * r + 587 * g + 114 * b) // 1000
            count += 1
    return total // max(count, 1)
qx, qy = (x1 - x0) // 4, (y1 - y0) // 4
print(mean(x0 + qx, x1 - qx, y0 + 8, min(y0 + 44, y1)), mean(x0 + qx, x1 - qx, y0 + qy, y1 - qy))
PY

# Set the appearance as System Settings does, and wait until the preference says so.
appearance() { # appearance dark|light
  local want=false; [[ $1 == dark ]] && want=true
  osascript -e "tell application \"System Events\" to tell appearance preferences to set dark mode to $want" \
    >>"$logs/osascript.log" 2>&1 || say "osascript failed: dark mode $want"
  for _ in $(seq 20); do
    local now=light
    [[ $(defaults read -g AppleInterfaceStyle 2>/dev/null) == Dark ]] && now=dark
    [[ $now == "$1" ]] && { say "appearance: $1"; return 0; }
    sleep 0.5
  done
  say "FAILED: the appearance didn't turn $1"; failed=1
}

# Start an app in the background: start NAME WAIT CMD...; leaves its pid in $pid.
start() {
  local name=$1 wait=$2; shift 2
  "$@" >"$logs/$name.log" 2>&1 &
  pid=$!
  sleep "$wait"
  kill -0 "$pid" 2>/dev/null || { say "FAILED: $name exited early (see logs/$name.log)"; failed=1; }
}
stop() { kill "$pid" 2>/dev/null; sleep 1; kill -9 "$pid" 2>/dev/null; wait "$pid" 2>/dev/null; }
# Bring $pid to the front and screenshot the screen as shots/NAME.png; sets TOP and MIDDLE
# to the window's brightness there (-1 without a window).
shot() {
  osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $pid) to true" \
    >>"$logs/osascript.log" 2>&1
  sleep 2
  screencapture -x -t bmp "$work/$1.bmp"
  sips -s format png "$work/$1.bmp" --out "$shots/$1.png" >/dev/null
  local bounds; bounds=$("$work/bounds" "$pid")
  TOP=-1 MIDDLE=-1
  if [[ -z $bounds ]]; then say "FAILED: $1: no window"; failed=1; return; fi
  read -r TOP MIDDLE < <(python3 "$work/brightness.py" "$work/$1.bmp" $bounds)
  say "$1: window $bounds, brightness top $TOP, middle $MIDDLE"
}
dark() { (($1 >= 0 && $1 < 100)); }
light() { (($1 > 150)); }
added() { grep -q "fonts: added .*/target/fonts to fontconfig" "$logs/$1.log"; }

# Samples: a CSV of digits (leading zeros, decimals, dates) and a Word document in the
# fonts Office documents use most.
python3 - "$work/people.csv" <<'PY'
import random, sys
random.seed(11)
with open(sys.argv[1], "w") as f:
    f.write("id,name,city,zip,amount,date\n")
    for i in range(20000):
        f.write(f"{i},Person {i},{random.choice(['Paris', 'Austin', 'Kyoto', 'Lagos'])},"
                f"0{random.randint(1000, 9999)},{random.random() * 1000:.2f},"
                f"2026-{random.randint(1, 12):02d}-{random.randint(1, 28):02d}\n")
PY
python3 scripts/office_fonts.py docx "$work/office-fonts.docx"
cp apps/frame/crates/pptx/tests/data/fixture.pptx apps/slate/crates/document-model/tests/data/picture.png "$work/"
cp "$work/office-fonts.docx" "$shots/"

{
  sw_vers
  echo "gtk4 $(pkg-config --modversion gtk4), pango $(pkg-config --modversion pango), fontconfig $(pkg-config --modversion fontconfig)"
  echo "fonts folder: $(ls target/fonts | tr '\n' ' ')"
  echo "SFNS.ttf families: $(fc-scan --format '%{family}\n' /System/Library/Fonts/SFNS.ttf 2>&1 | sort -u | tr '\n' ' ')"
  for f in Calibri Cambria Arial 'Times New Roman' 'Courier New' sans-serif .AppleSystemUIFont; do
    echo "fc-match (system config, no bundled fonts) '$f': $(fc-match "$f")"
  done
} >"$logs/env.txt" 2>&1

# 1. Light.
appearance light
start brick-light 10 "$bin/brick" "$work/people.csv"
shot brick-light; check "brick draws light" light "$TOP"; stop
check "brick added the bundled fonts" added brick-light
start brick-coretext 10 env PANGOCAIRO_BACKEND=coretext "$bin/brick" "$work/people.csv"
shot brick-coretext; stop
start slate-light 20 "$bin/slate" "$work/office-fonts.docx"
shot slate-light; check "slate draws light" light "$TOP"; stop
check "slate added the bundled fonts" added slate-light
start slate-coretext 20 env PANGOCAIRO_BACKEND=coretext "$bin/slate" "$work/office-fonts.docx"
shot slate-coretext; stop
start frame-light 12 "$bin/frame" "$work/fixture.pptx"
shot frame-light; check "frame draws light" light "$TOP"; stop
check "frame added the bundled fonts" added frame-light
start frame-new 12 "$bin/frame"
shot frame-new; stop
start etch-light 10 "$bin/etch" "$work/picture.png"
shot etch-light; check "etch draws light" light "$MIDDLE"; stop

# 2. Dark.
appearance dark
start brick-dark 10 "$bin/brick" "$work/people.csv"
shot brick-dark; check "brick draws dark" dark "$TOP"; stop
start slate-dark 20 "$bin/slate" "$work/office-fonts.docx"
shot slate-dark; check "slate draws dark" dark "$TOP"; stop
start frame-dark 12 "$bin/frame" "$work/fixture.pptx"
shot frame-dark; check "frame draws dark" dark "$TOP"; stop
start etch-dark 10 "$bin/etch" "$work/picture.png"
shot etch-dark; check "etch draws dark" dark "$MIDDLE"; stop

# 3. Changes while running. The runner's virtual display ticks GTK's frame clock slowly
# (DEC-11: Brick saw 0.1 Hz), so a restyled window can take seconds to draw again: shoot
# until it shows the change, for up to about 30 s, and say how long it took.
until_shot() { # until_shot dark|light NAME DESCRIPTION
  local t0=$SECONDS
  for _ in $(seq 8); do
    shot "$2"
    "$1" "$TOP" && break
    sleep 1
  done
  say "$2: $1 after $((SECONDS - t0)) s"
  check "$3" "$1" "$TOP"
}
appearance light
start slate-live 20 "$bin/slate" "$work/office-fonts.docx"
shot slate-live-1-light; check "slate starts light" light "$TOP"
appearance dark
until_shot dark slate-live-2-dark "slate turns dark with the Mac"
appearance light
until_shot light slate-live-3-light "slate turns light again"
stop
start brick-live 10 "$bin/brick" "$work/people.csv"
shot brick-live-1-light; check "brick starts light" light "$TOP"
appearance dark
until_shot dark brick-live-2-dark "brick turns dark with the Mac"
stop
appearance light

if ((failed)); then say "LOOK FAILED"; exit 1; fi
say "LOOK OK"
