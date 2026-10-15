#!/usr/bin/env bash
# MAC-3 acceptance on macOS: Command keys drive each app.
#
#   scripts/check_mac_keys.sh [BIN_DIR]   (default target/release; macOS only)
#
# Runs on GitHub's macOS runner (ci.yml's macos job), which has a logged-in Aqua session
# where System Events may send keys and screencapture may record. Each app opens a copy of
# a sample file, drawn with GSK_RENDERER=cairo (GL draws black windows on the runner VMs),
# and gets real key events from System Events, one every 0.3 s (faster bursts garble GTK's
# input there, spike finding C13):
#   Brick: types zz Return into A2 (row 1 is the header row), ⌘S: the file's second row
#          starts with zz; types yy Return into A3, ⌘Z, ⌘S: the third row is as it was;
#   Slate: types Hello and a space at the start, ⌘S: the file starts with them; types X,
#          ⌘Z, ⌘S: no X;
#   Frame: New Slide (⇧⌘N), ⌘S: the deck has a slide more; New Slide, ⌘Z, ⌘S: no more;
#   Etch:  Rotate 90° Left (⌘G), ⌘S: the 64x32 PNG is 32x64; ⌘G, ⌘Z, ⌘S: still 32x64;
#   each:  ⌘O opens the open panel (a window more for the app), Escape closes it.
# Screenshots of every step land in $OUT/shots (default out/mac-keys); a failure exits 1
# after every app has run.
set -uo pipefail
bin=$(cd "${1:-target/release}" && pwd)
out=${OUT:-out/mac-keys}
mkdir -p "$out/shots" "$out/files" "$out/logs"
out=$(cd "$out" && pwd)
shots=$out/shots files=$out/files logs=$out/logs
root=$(cd "$(dirname "$0")/.." && pwd)
export GSK_RENDERER=cairo
failed=0

note() { echo "$*" | tee -a "$out/summary.txt"; }
pass() { note "ok   $*"; }
fail() { note "FAIL $*"; failed=1; }

# The windows on screen of process $1, one per line: layer, title, bounds.
cat >"$out/windows.swift" <<'EOF'
import CoreGraphics
import Foundation
let pid = Int(CommandLine.arguments[1]) ?? 0
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
for w in list where (w[kCGWindowOwnerPID as String] as? Int) == pid {
    let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
    print("\(w[kCGWindowLayer as String] ?? 0)\t\(w[kCGWindowName as String] ?? "")\t\(b["X"] ?? 0),\(b["Y"] ?? 0) \(b["Width"] ?? 0)x\(b["Height"] ?? 0)")
}
EOF
swiftc -O "$out/windows.swift" -o "$out/windows" >"$logs/swiftc.log" 2>&1 || {
    note "FAIL the window lister didn't build"; cat "$logs/swiftc.log"; exit 1
}
windows() { "$out/windows" "$1"; }

# osascript that can't hang the job on a permission prompt.
osa() {
    osascript -e "$1" >>"$logs/osascript.log" 2>&1 &
    local p=$! i
    for i in $(seq 40); do kill -0 "$p" 2>/dev/null || break; sleep 0.5; done
    if kill -0 "$p" 2>/dev/null; then kill "$p"; note "osascript timed out: $1"; return 1; fi
    wait "$p"
}
# A key with modifiers (`command down`, `{command down, shift down}`), or none.
key() { osa "tell application \"System Events\" to keystroke \"$1\"${2:+ using $2}"; sleep 0.3; }
# Characters one at a time.
type_slowly() {
    local s=$1 i
    for ((i = 0; i < ${#s}; i++)); do key "${s:i:1}"; done
}
code() { osa "tell application \"System Events\" to key code $1"; sleep 0.3; }
front() { osa "tell application \"System Events\" to set frontmost of (first process whose unix id is $1) to true"; sleep 1; }
shot() { sleep "${2:-1}"; screencapture -x "$shots/$1.png" || note "screencapture $1 failed"; }

# Start app $1 on file $2; sets $pid.
start() {
    "$bin/$1" "$2" >"$logs/$1.log" 2>&1 &
    pid=$!
    sleep 8
    if ! kill -0 "$pid" 2>/dev/null; then fail "$1 exited at start"; cat "$logs/$1.log"; return 1; fi
    front "$pid"
    windows "$pid" >"$logs/$1-windows-start.txt"
}
stop() {
    kill "$pid" 2>/dev/null; sleep 1; kill -9 "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
}

# ⌘O shows the open panel: one window more for the app; Escape closes it.
open_panel() {
    local app=$1 before after
    before=$(windows "$pid" | wc -l)
    key o "command down"
    shot "$app-4-cmd-o" 3
    windows "$pid" >"$logs/$app-windows-cmd-o.txt"
    after=$(wc -l <"$logs/$app-windows-cmd-o.txt")
    if ((after > before)); then pass "$app: ⌘O opened the open panel ($before -> $after windows)"
    else fail "$app: ⌘O opened nothing ($before -> $after windows)"; fi
    code 53
    shot "$app-5-escape" 2
}

# Brick: a CSV file.
brick() {
    local f=$files/people.csv
    printf 'id,name,city\n1,Ada,Paris\n2,Linus,Austin\n' >"$f"
    start brick "$f" || return
    shot brick-0-start
    type_slowly zz; code 36
    key s "command down"
    shot brick-1-cmd-s 3
    if [[ $(sed -n 2p "$f") == zz,Ada,Paris ]]; then pass "brick: ⌘S saved A2 = zz"
    else fail "brick: ⌘S didn't save: row 2 is $(sed -n 2p "$f")"; fi
    type_slowly yy; code 36
    shot brick-2-typed 1
    key z "command down"
    shot brick-3-cmd-z 1
    key s "command down"
    sleep 3
    if [[ $(sed -n 3p "$f") == 2,Linus,Austin ]]; then pass "brick: ⌘Z took yy back out of A3"
    else fail "brick: ⌘Z didn't undo: row 3 is $(sed -n 3p "$f")"; fi
    open_panel brick
    stop
}

# Slate: a plain-text file.
slate() {
    local f=$files/notes.txt
    printf 'Original line.\n' >"$f"
    start slate "$f" || return
    shot slate-0-start
    type_slowly "Hello "
    key s "command down"
    shot slate-1-cmd-s 3
    if [[ $(cat "$f") == "Hello Original line." ]]; then pass "slate: ⌘S saved the typing"
    else fail "slate: ⌘S didn't save: $(cat "$f")"; fi
    type_slowly X
    shot slate-2-typed 1
    key z "command down"
    shot slate-3-cmd-z 1
    key s "command down"
    sleep 3
    if [[ $(cat "$f") == "Hello Original line." ]]; then pass "slate: ⌘Z took X back out"
    else fail "slate: ⌘Z didn't undo: $(cat "$f")"; fi
    open_panel slate
    stop
}

slides() { unzip -l "$1" | grep -cE 'ppt/slides/slide[0-9]+\.xml$'; }

# Frame: a deck.
frame() {
    local f=$files/deck.pptx n0 n1 n2
    cp "$root/apps/frame/crates/pptx/tests/data/fixture.pptx" "$f"
    n0=$(slides "$f")
    start frame "$f" || return
    shot frame-0-start
    key n "{command down, shift down}"
    key s "command down"
    shot frame-1-cmd-s 3
    n1=$(slides "$f")
    if ((n1 == n0 + 1)); then pass "frame: ⌘S saved the new slide ($n0 -> $n1 slides)"
    else fail "frame: ⌘S didn't save a new slide ($n0 -> $n1 slides)"; fi
    key n "{command down, shift down}"
    shot frame-2-new-slide 1
    key z "command down"
    shot frame-3-cmd-z 1
    key s "command down"
    sleep 3
    n2=$(slides "$f")
    if ((n2 == n1)); then pass "frame: ⌘Z took the second new slide back ($n2 slides)"
    else fail "frame: ⌘Z didn't undo ($n1 -> $n2 slides)"; fi
    open_panel frame
    stop
}

size() { sips -g pixelWidth -g pixelHeight "$1" | awk '/pixel/ { printf "%s%s", sep, $2; sep = "x" }'; }

# Etch: a 64x32 PNG.
etch() {
    local f=$files/wide.png
    python3 - "$f" <<'EOF'
import struct, sys, zlib
w, h = 64, 32
raw = b"".join(b"\0" + bytes([200, 40, 40]) * (w // 2) + bytes([40, 40, 200]) * (w // 2) for _ in range(h))
def chunk(t, d):
    return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
EOF
    start etch "$f" || return
    shot etch-0-start
    key g "command down"
    key s "command down"
    shot etch-1-cmd-s 3
    if [[ $(size "$f") == 32x64 ]]; then pass "etch: ⌘S saved the rotated image (32x64)"
    else fail "etch: ⌘S didn't save the rotation: $(size "$f")"; fi
    key g "command down"
    shot etch-2-rotated 1
    key z "command down"
    shot etch-3-cmd-z 1
    key s "command down"
    sleep 3
    if [[ $(size "$f") == 32x64 ]]; then pass "etch: ⌘Z took the second rotation back"
    else fail "etch: ⌘Z didn't undo: $(size "$f")"; fi
    open_panel etch
    stop
}

for app in brick slate frame etch; do
    if [[ -x $bin/$app ]]; then "$app"; else fail "no $bin/$app"; fi
done
if ((failed)); then note "check_mac_keys: FAILED"; exit 1; fi
note "check_mac_keys: OK"
