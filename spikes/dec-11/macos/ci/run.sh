#!/usr/bin/env bash
# DEC-11 spike: one phase of the macOS probe (spike-macos.yml runs them in order). Every
# phase keeps going after a failure and appends its result to out/logs/summary.txt; logs
# and screenshots land in out/ for the workflow's artifact.
set -uo pipefail
phase=$1
root=$(pwd)
out=$root/out
logs=$out/logs
shots=$out/shots
mkdir -p "$logs" "$shots"
here=$root/spikes/dec-11/macos/ci
rel=$root/target/release
apps=(brick slate frame etch)

note() { echo "$*" | tee -a "$logs/summary.txt"; }

# Run "$@" to a log, recording its exit status and time.
step() {
    local name=$1; shift
    local t0=$SECONDS
    "$@" >"$logs/$name.log" 2>&1
    local rc=$?
    note "$name: exit $rc in $((SECONDS - t0)) s ($*)"
    return 0
}

# Start a command for a few seconds, list windows, screenshot, stop it.
# usage: shoot NAME WAIT CMD...
shoot() {
    local name=$1 wait=$2; shift 2
    if [[ $1 != env && ! -x $1 ]]; then note "run-$name: no binary $1"; return 0; fi
    "$@" >"$logs/run-$name.log" 2>&1 &
    local pid=$!
    sleep "$wait"
    if kill -0 "$pid" 2>/dev/null; then
        note "run-$name: still running after ${wait} s (pid $pid)"
    else
        wait "$pid"
        note "run-$name: exited with $? within ${wait} s"
    fi
    "$out/windows" >"$logs/windows-$name.txt" 2>&1
    screencapture -x "$shots/$name.png" 2>>"$logs/run-$name.log" || note "screencapture $name failed"
    if [[ -n ${KEEP:-} ]]; then echo "$pid" >"$out/pid"; return 0; fi
    kill "$pid" 2>/dev/null
    sleep 1
    kill -9 "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
    return 0
}

# osascript that can't hang the job on a permission prompt.
osa() {
    osascript -e "$1" >>"$logs/osascript.log" 2>&1 &
    local p=$! i
    for i in $(seq 20); do kill -0 "$p" 2>/dev/null || break; sleep 0.5; done
    if kill -0 "$p" 2>/dev/null; then
        kill "$p"
        note "osascript timed out: $1"
    else
        wait "$p"
        note "osascript exit $?: $1"
    fi
}

# usage: timed NAME SECONDS CMD... (a benchmark mode: must exit on its own)
timed() {
    local name=$1 limit=$2; shift 2
    local t0=$SECONDS
    "$@" >"$logs/$name.log" 2>&1 &
    local pid=$!
    while kill -0 "$pid" 2>/dev/null && ((SECONDS - t0 < limit)); do sleep 1; done
    if kill -0 "$pid" 2>/dev/null; then
        "$out/windows" >"$logs/windows-$name.txt" 2>&1
        screencapture -x "$shots/$name.png" 2>/dev/null
        kill -9 "$pid"
        note "$name: killed after $limit s ($*)"
    else
        wait "$pid"
        note "$name: exit $? in $((SECONDS - t0)) s ($*)"
    fi
}

samples() {
    local s=$out/samples
    mkdir -p "$s"
    python3 - "$s/people.csv" <<'EOF'
import sys, random
random.seed(11)
with open(sys.argv[1], "w") as f:
    f.write("id,name,city,zip,amount,date\n")
    for i in range(20000):
        f.write(f"{i},Person {i},{random.choice(['Paris','Austin','Kyoto','Lagos'])},0{random.randint(1000,9999)},{random.random()*1000:.2f},2026-{random.randint(1,12):02d}-{random.randint(1,28):02d}\n")
EOF
    cp README.md "$s/readme.md"
    cp apps/slate/crates/docx/tests/data/fixture.docx "$s/fixture.docx"
    cp apps/frame/crates/pptx/tests/data/fixture.pptx "$s/fixture.pptx"
    cp apps/slate/crates/document-model/tests/data/picture.png "$s/picture.png"
    printf 'Typed on macOS: ' >"$s/typing.txt"
}

case $phase in
env)
    {
        sw_vers; uname -a; sysctl -n machdep.cpu.brand_string hw.ncpu hw.memsize
        rustc -Vv; cargo -V
        brew --version; brew list --versions gtk4 glib pango cairo harfbuzz fontconfig freetype gdk-pixbuf graphene libepoxy adwaita-icon-theme hicolor-icon-theme pkgconf pkg-config
        pkg-config --modversion gtk4 pango cairo fontconfig
        system_profiler SPDisplaysDataType
        defaults read -g AppleInterfaceStyle 2>&1
        echo "fc-list families: $(fc-list : family | sort -u | wc -l)"
        for f in 'Liberation Sans' 'Liberation Serif' 'Liberation Mono' Carlito Caladea 'DejaVu Serif' Calibri Arial 'Helvetica Neue' sans-serif serif monospace; do
            echo "fc-match '$f': $(fc-match "$f")"
        done
        echo "--- system font families (CoreText, first 40)"
        fc-list : family | sort -u | head -40
    } >"$logs/env.txt" 2>&1
    swiftc -O "$here/windows.swift" -o "$out/windows" >"$logs/swiftc.log" 2>&1 && note "windows helper built" || note "windows helper failed"
    "$out/windows" >"$logs/windows-empty.txt" 2>&1
    screencapture -x "$shots/00-desktop.png" 2>"$logs/screencapture-desktop.log" && note "desktop screenshot ok" || note "desktop screenshot failed"
    ;;
check)
    step check cargo check --workspace
    step check-all-targets cargo check --workspace --all-targets
    step clippy cargo clippy --workspace --all-targets --all-features -- -D warnings
    ;;
test)
    step test cargo test --workspace --no-fail-fast
    grep -E '^test result|^error|FAILED|panicked' "$logs/test.log" >"$logs/test-summary.txt"
    ;;
build)
    for a in "${apps[@]}"; do step "build-$a" cargo build --release -p "$a"; done
    for a in "${apps[@]}"; do
        b=$rel/$a
        [[ -x $b ]] || { note "$a: no binary"; continue; }
        note "$a binary: $(stat -f %z "$b") bytes; $(lipo -archs "$b")"
        otool -L "$b" >"$logs/otool-$a.txt"
        codesign -dv "$b" >"$logs/codesign-$a.txt" 2>&1
        note "$a --version: $("$b" --version 2>&1 | head -3)"
    done
    ;;
run)
    samples
    s=$out/samples
    shoot brick 10 "$rel/brick" "$s/people.csv"
    shoot brick-xlsx 10 "$rel/brick" apps/brick/crates/xlsx/tests/data/fixture.xlsx
    shoot slate 10 "$rel/slate" "$s/readme.md"
    shoot frame 10 "$rel/frame" "$s/fixture.pptx"
    shoot etch 10 "$rel/etch" "$s/picture.png"
    ;;
patched)
    # One-line experiment: let suite-platform accept macOS so Slate, Frame, and Etch start.
    git apply "$here/allow-macos.patch" && note "allow-macos.patch applied"
    for a in slate frame etch; do step "build-patched-$a" cargo build --release -p "$a"; done
    s=$out/samples
    shoot p-slate 10 "$rel/slate" "$s/readme.md"
    shoot p-slate-docx 10 "$rel/slate" "$s/fixture.docx"
    shoot p-slate-start 8 "$rel/slate"
    shoot p-frame 10 "$rel/frame" "$s/fixture.pptx"
    shoot p-frame-blank 8 "$rel/frame"
    shoot p-etch 10 "$rel/etch" "$s/picture.png"
    shoot p-etch-shot 10 "$rel/etch" "$shots/brick.png"
    for g in cairo gl ngl vulkan; do shoot "p-frame-gsk-$g" 8 env GSK_RENDERER=$g "$rel/frame" "$s/fixture.pptx"; done
    # Benchmark modes: they open a window, measure, print, and quit.
    timed bench-brick-scroll 120 "$rel/brick" "$s/people.csv" --bench-scroll 120
    timed bench-slate-flow 180 "$rel/slate" "$s/fixture.docx" --bench-flow
    timed bench-frame-draw 120 "$rel/frame" "$s/fixture.pptx" --bench-draw 60
    timed bench-etch-stroke 120 "$rel/etch" --bench-stroke 120
    ;;
input)
    # Keyboard: type into Slate, then try Cmd+Z and Ctrl+Z (the apps bind <Control>).
    s=$out/samples
    KEEP=1 shoot k-slate-0 10 "$rel/slate" "$s/typing.txt"
    pid=$(cat "$out/pid")
    osa "tell application \"System Events\" to set frontmost of (first process whose unix id is $pid) to true"
    osa 'tell application "System Events" to keystroke "hello world"'
    sleep 2; screencapture -x "$shots/k-slate-1-typed.png"
    osa 'tell application "System Events" to keystroke "z" using command down'
    sleep 2; screencapture -x "$shots/k-slate-2-cmd-z.png"
    osa 'tell application "System Events" to keystroke "z" using control down'
    sleep 2; screencapture -x "$shots/k-slate-3-ctrl-z.png"
    osa 'tell application "System Events" to keystroke "o" using command down'
    sleep 3; screencapture -x "$shots/k-slate-4-cmd-o.png"
    osa 'tell application "System Events" to key code 53'
    osa 'tell application "System Events" to keystroke "o" using control down'
    sleep 3; screencapture -x "$shots/k-slate-5-ctrl-o.png"
    "$out/windows" >"$logs/windows-k-slate.txt" 2>&1
    osa 'tell application "System Events" to key code 53'
    kill "$pid"; sleep 1; kill -9 "$pid" 2>/dev/null
    ;;
dark)
    defaults write -g AppleInterfaceStyle -string Dark && note "dark mode set"
    s=$out/samples
    shoot d-brick 10 "$rel/brick" "$s/people.csv"
    shoot d-slate 10 "$rel/slate" "$s/readme.md"
    defaults delete -g AppleInterfaceStyle
    ;;
bundle)
    # What a self-contained .app would carry: the binary's Homebrew dylib closure and the
    # data GTK reads at run time.
    python3 - "$rel/etch" >"$logs/bundle-closure.txt" 2>&1 <<'EOF'
import os, subprocess, sys
seen, todo = set(), [os.path.realpath(sys.argv[1])]
while todo:
    f = todo.pop()
    out = subprocess.run(["otool", "-L", f], capture_output=True, text=True).stdout.splitlines()[1:]
    for line in out:
        p = line.strip().split(" (")[0]
        if p.startswith("@rpath") or p.startswith("/usr/lib") or p.startswith("/System"):
            continue
        r = os.path.realpath(p)
        if r not in seen and os.path.exists(r):
            seen.add(r); todo.append(r)
total = sum(os.path.getsize(p) for p in seen)
for p in sorted(seen): print(os.path.getsize(p), p)
print(f"dylibs: {len(seen)}, {total/1e6:.1f} MB")
EOF
    tail -1 "$logs/bundle-closure.txt" | sed 's/^/etch closure: /' >>"$logs/summary.txt"
    p=$(brew --prefix)
    du -sh "$p/share/icons/Adwaita" "$p/share/icons/hicolor" "$p/share/glib-2.0/schemas" "$p/lib/gdk-pixbuf-2.0" "$p/share/locale" >"$logs/bundle-data.txt" 2>&1
    note "bundle data: $(tr '\n' ' ' <"$logs/bundle-data.txt")"
    ;;
*) echo "unknown phase $phase"; exit 2 ;;
esac
