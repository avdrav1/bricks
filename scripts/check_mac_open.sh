#!/usr/bin/env bash
# MAC-5 acceptance on macOS: Launch Services opens files in each app, which draws them with
# the renderer the apps choose there (cairo, `suite_ui::renderer`).
#
#   scripts/check_mac_open.sh [BIN_DIR]   (default target/release; macOS only)
#
# Runs on GitHub's macOS runner (ci.yml's macos job). Each app's release binary is wrapped
# in a minimal bundle (scripts/mac_app.py: identifier dev.cronkite.<App>, its file types)
# and registered with Launch Services, then for each app:
#   1. `open -a <App>.app FILE` starts it: one process, one window, titled with the file;
#   2. `open -a <App>.app OTHER` while it runs: the same process, a second window, titled
#      with the other file;
#   3. after it quits, `open -a <App>.app` with no file: one window, the app's no-file
#      state (Brick's and Slate's start windows, Frame's and Etch's new Untitled document);
#   and GTK reports the cairo renderer (GSK_DEBUG=renderer) with GSK_RENDERER unset.
# Screenshots of every step land in $OUT/shots (default out/mac-open), the apps' logs in
# $OUT/logs; a failure exits 1 after every app has run.
set -uo pipefail
bin=$(cd "${1:-target/release}" && pwd)
out=${OUT:-out/mac-open}
mkdir -p "$out/shots" "$out/files" "$out/logs" "$out/apps"
out=$(cd "$out" && pwd)
shots=$out/shots files=$out/files logs=$out/logs
root=$(cd "$(dirname "$0")/.." && pwd)
lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
# shellcheck source=scripts/mac_helpers.sh
source "$root/scripts/mac_helpers.sh"
# The apps choose the renderer on macOS themselves (suite_ui::renderer): that's under test.
unset GSK_RENDERER

# The app's document windows (layer 0).
documents() { windows "$1" | awk -F'\t' '$1 == 0'; }

# Wait up to 20 s for process $1 to show $2 windows; print the list.
wait_windows() {
    local i
    for i in $(seq 40); do
        (($(documents "$1" | wc -l) >= $2)) && break
        sleep 0.5
    done
    sleep 2
    documents "$1"
}

# The pids running bundle $1's executable.
pids() { pgrep -f "^$1/Contents/MacOS/" || true; }

# Apps Launch Services starts get launchd's environment, not this shell's.
launchctl unsetenv GSK_RENDERER
launchctl setenv GSK_DEBUG renderer

# Check app $1 (bundle name $2) with files $3 and $4, whose no-file window is titled $5.
check() {
    local app=$1 name=$2 one=$3 two=$4 untitled=$5 bundle pid list n
    bundle=$(python3 "$root/scripts/mac_app.py" "$app" "$bin" "$out/apps") || { fail "$app: no bundle"; return; }
    $lsregister -f "$bundle" || note "$app: lsregister failed"

    # 1. A file, the app not running.
    open --stderr "$logs/$app-1.err" -a "$bundle" "$one"
    sleep 1
    pid=$(pids "$bundle" | head -1)
    if [[ -z $pid ]]; then fail "$app: open -a started nothing"; return; fi
    list=$(wait_windows "$pid" 1)
    shot "$app-1-open-a-file"
    echo "$list" >"$logs/$app-1-windows.txt"
    n=$(grep -c . <<<"$list")
    if ((n == 1)) && grep -qF "$(basename "$one")" <<<"$list"; then
        pass "$app: open -a $(basename "$one") showed it in one window"
    else fail "$app: open -a $(basename "$one"): $n windows: $(tr '\n' '|' <<<"$list")"; fi

    # 2. Another file, the app running.
    open -a "$bundle" "$two"
    list=$(wait_windows "$pid" 2)
    shot "$app-2-second-open"
    echo "$list" >"$logs/$app-2-windows.txt"
    n=$(grep -c . <<<"$list")
    if [[ $(pids "$bundle" | wc -l | tr -d ' ') != 1 ]]; then
        fail "$app: the second open started another process: $(pids "$bundle" | tr '\n' ' ')"
    elif ((n == 2)) && grep -qF "$(basename "$two")" <<<"$list"; then
        pass "$app: a second open -a $(basename "$two") opened a second window in the running app"
    else fail "$app: second open -a $(basename "$two"): $n windows: $(tr '\n' '|' <<<"$list")"; fi
    if grep -q "Using renderer 'GskCairoRenderer'" "$logs/$app-1.err"; then
        pass "$app: GTK draws with cairo"
    else fail "$app: not the cairo renderer: $(grep -m1 "Using renderer" "$logs/$app-1.err")"; fi
    kill "$pid"; sleep 2; kill -9 "$pid" 2>/dev/null

    # 3. No file.
    open --stderr "$logs/$app-3.err" -a "$bundle"
    sleep 1
    pid=$(pids "$bundle" | head -1)
    if [[ -z $pid ]]; then fail "$app: open -a without a file started nothing"; return; fi
    list=$(wait_windows "$pid" 1)
    shot "$app-3-open-a-no-file"
    echo "$list" >"$logs/$app-3-windows.txt"
    n=$(grep -c . <<<"$list")
    if ((n == 1)) && grep -qF "$untitled" <<<"$list"; then
        pass "$app: open -a without a file showed $untitled"
    else fail "$app: open -a without a file: $n windows: $(tr '\n' '|' <<<"$list")"; fi
    kill "$pid"; sleep 2; kill -9 "$pid" 2>/dev/null
}

f=$files
printf 'id,name,city\n1,Ada,Paris\n2,Linus,Austin\n' >"$f/people.csv"
cp "$root/apps/brick/crates/xlsx/tests/data/fixture.xlsx" "$f/book.xlsx"
printf '# Read me\n\nOpened by **Finder**.\n' >"$f/readme.md"
cp "$root/apps/slate/crates/docx/tests/data/fixture.docx" "$f/letter.docx"
cp "$root/apps/frame/crates/pptx/tests/data/fixture.pptx" "$f/deck.pptx"
cp "$root/apps/frame/crates/pptx/tests/data/shapes.pptx" "$f/shapes.pptx"
python3 - "$f/wide.png" "$f/tall.png" <<'EOF'
import struct, sys, zlib
def png(path, w, h, colour):
    raw = b"".join(b"\0" + bytes(colour) * w for _ in range(h))
    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
    open(path, "wb").write(data)
png(sys.argv[1], 320, 160, (200, 40, 40))
png(sys.argv[2], 160, 320, (40, 40, 200))
EOF

check brick Brick "$f/people.csv" "$f/book.xlsx" Brick
check slate Slate "$f/readme.md" "$f/letter.docx" Slate
check frame Frame "$f/deck.pptx" "$f/shapes.pptx" Untitled
check etch Etch "$f/wide.png" "$f/tall.png" Untitled

launchctl unsetenv GSK_DEBUG
if ((failed)); then note "check_mac_open: FAILED"; exit 1; fi
note "check_mac_open: OK"
