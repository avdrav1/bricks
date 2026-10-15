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

# shoot with GTK's default renderer, then again with cairo (software).
both() {
    local name=$1 wait=$2; shift 2
    shoot "$name" "$wait" "$@"
    [[ -x $1 ]] && shoot "$name-cairo" "$wait" env GSK_RENDERER=cairo "$@"
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
    step check-all-targets cargo check --workspace --all-targets --keep-going
    grep -E '^error: could not compile' "$logs/check-all-targets.log" | sort -u | tee -a "$logs/summary.txt"
    step clippy cargo clippy --workspace --all-targets --all-features --keep-going -- -D warnings
    grep -E '^error: could not compile' "$logs/clippy.log" | sort -u | tee -a "$logs/summary.txt"
    exit 0
    ;;
test)
    # Target by target, so a test file that doesn't compile doesn't stop the rest.
    cargo metadata --no-deps --format-version 1 | python3 -c '
import json, sys
for p in json.load(sys.stdin)["packages"]:
    for t in p["targets"]:
        k = t["kind"][0]
        if k == "lib": print(p["name"], "lib", t["name"]); print(p["name"], "doc", t["name"])
        elif k in ("bin", "test"): print(p["name"], k, t["name"])
' >"$out/targets.txt"
    : >"$logs/test-matrix.txt"
    while read -r pkg kind name; do
        case $kind in
        lib) sel=(--lib) ;; doc) sel=(--doc) ;; bin) sel=(--bin "$name") ;; test) sel=(--test "$name") ;;
        esac
        log=$logs/test/$pkg-$kind-$name.log
        mkdir -p "$logs/test"
        cargo test -p "$pkg" "${sel[@]}" --no-fail-fast </dev/null >"$log" 2>&1
        rc=$?
        res=$(grep -E '^test result' "$log" | tail -1 | sed 's/ finished in.*//')
        grep -q 'could not compile' "$log" && res="COMPILE ERROR: $(grep -m1 -E '^error(\[|:)' "$log")"
        fails=$(grep -E '^test .* FAILED$' "$log" | sed 's/^test //; s/ \.\.\. FAILED//' | tr '\n' ' ')
        echo "$pkg $kind $name: exit $rc; $res ${fails:+; failed: $fails}" >>"$logs/test-matrix.txt"
    done <"$out/targets.txt"
    note "test targets: $(wc -l <"$out/targets.txt"), non-zero exit: $(grep -vc 'exit 0;' "$logs/test-matrix.txt")"
    grep -v 'exit 0;' "$logs/test-matrix.txt" | tee -a "$logs/summary.txt"
    exit 0
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
    both brick 10 "$rel/brick" "$s/people.csv"
    both brick-xlsx 10 "$rel/brick" apps/brick/crates/xlsx/tests/data/fixture.xlsx
    shoot slate 10 "$rel/slate" "$s/readme.md"
    shoot frame 10 "$rel/frame" "$s/fixture.pptx"
    shoot etch 10 "$rel/etch" "$s/picture.png"
    exit 0
    ;;
patched)
    # One-line experiment: let suite-platform accept macOS so Slate, Frame, and Etch start.
    git apply "$here/allow-macos.patch" && note "allow-macos.patch applied"
    for a in slate frame etch; do step "build-patched-$a" cargo build --release -p "$a"; done
    s=$out/samples
    both p-slate 10 "$rel/slate" "$s/readme.md"
    both p-slate-docx 10 "$rel/slate" "$s/fixture.docx"
    both p-slate-start 8 "$rel/slate"
    both p-frame 10 "$rel/frame" "$s/fixture.pptx"
    both p-frame-blank 8 "$rel/frame"
    both p-etch 10 "$rel/etch" "$s/picture.png"
    both p-etch-shot 10 "$rel/etch" "$shots/brick-cairo.png"
    for g in gl ngl vulkan; do shoot "p-frame-gsk-$g" 8 env GSK_RENDERER=$g "$rel/frame" "$s/fixture.pptx"; done
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
minos)
    # Which macOS a Homebrew-built bundle would need: each Mach-O's minimum OS.
    p=$(brew --prefix)
    for f in "$rel/brick" "$p/opt/gtk4/lib/libgtk-4.1.dylib" "$p/opt/glib/lib/libglib-2.0.0.dylib" "$p/opt/pango/lib/libpango-1.0.0.dylib"; do
        echo "$f: $(otool -l "$f" | awk '/LC_BUILD_VERSION/{b=1} b&&/minos/{print $2; exit}')"
    done >"$logs/minos.txt" 2>&1
    note "minos: $(tr '\n' ' ' <"$logs/minos.txt")"
    ;;
sigbus)
    # F2: csv-engine's SIGBUS guard (a file truncated while mapped) on macOS.
    step sigbus cargo test -p csv-engine --lib truncat -- --nocapture
    grep -E '^test |test result' "$logs/sigbus.log" | tee -a "$logs/summary.txt"
    ;;
sweep)
    # F13: Slate's Original::sweep looks for /proc/<pid>; macOS has no /proc.
    s=$out/samples
    cache=$HOME/.cache/slate
    KEEP=1 shoot sw-a 10 "$rel/slate" "$s/fixture.docx"
    a=$(cat "$out/pid")
    note "sweep: slate A pid $a running; cache: $(ls "$cache" | tr '\n' ' ')"
    KEEP=1 shoot sw-b 10 "$rel/slate" "$s/readme.md"
    b=$(cat "$out/pid")
    note "sweep: slate B pid $b started; A alive: $(kill -0 "$a" 2>/dev/null && echo yes || echo no); cache now: $(ls "$cache" | tr '\n' ' ')"
    kill "$a" "$b"; sleep 1; kill -9 "$a" "$b" 2>/dev/null
    ;;
app)
    # F34 and bundling: a minimal Slate.app (binary + Info.plist, Homebrew's dylibs) opened
    # through LaunchServices, then a self-contained Brick.app (dylibbundler) and its DMG.
    s=$out/samples
    lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
    app=$out/app/Slate.app
    mkdir -p "$app/Contents/MacOS"
    cp "$rel/slate" "$app/Contents/MacOS/slate"
    cp "$here/Slate.Info.plist" "$app/Contents/Info.plist"
    $lsregister -f "$app" && note "lsregister Slate.app ok"
    # Apps LaunchServices starts get launchd's environment, not this shell's.
    [[ -n ${GSK_RENDERER:-} ]] && launchctl setenv GSK_RENDERER "$GSK_RENDERER"
    open --stdout "$logs/app-slate.out" --stderr "$logs/app-slate.err" -a "$app" "$s/readme.md"
    note "open -a Slate.app readme.md: exit $?"
    sleep 10
    "$out/windows" >"$logs/windows-app-1.txt" 2>&1
    screencapture -x "$shots/app-1-open-a.png"
    open -a "$app" "$s/fixture.docx"
    note "open -a Slate.app fixture.docx (second, app running): exit $?"
    sleep 8
    "$out/windows" >"$logs/windows-app-2.txt" 2>&1
    screencapture -x "$shots/app-2-second-open.png"
    pkill -f "$app/Contents/MacOS/slate"; sleep 1
    open -n -a "$app" --args "$s/readme.md"
    note "open -n -a Slate.app --args readme.md: exit $?"
    sleep 8
    "$out/windows" >"$logs/windows-app-3.txt" 2>&1
    screencapture -x "$shots/app-3-open-args.png"
    pkill -f "$app/Contents/MacOS/slate"
    note "pgrep slate after: $(pgrep -fl slate | tr '\n' ' ')"

    brew install dylibbundler >"$logs/brew-dylibbundler.log" 2>&1
    bapp=$out/app/Brick.app
    mkdir -p "$bapp/Contents/MacOS" "$bapp/Contents/Frameworks"
    cp "$rel/brick" "$bapp/Contents/MacOS/brick"
    sed -e 's/Slate/Brick/g; s/slate/brick/g' "$here/Slate.Info.plist" >"$bapp/Contents/Info.plist"
    step dylibbundler dylibbundler -of -b -x "$bapp/Contents/MacOS/brick" -d "$bapp/Contents/Frameworks" -p @executable_path/../Frameworks/
    note "Brick.app: $(du -sh "$bapp" | cut -f1), $(ls "$bapp/Contents/Frameworks" | wc -l) dylibs"
    step codesign-verify codesign --verify --deep --strict -v "$bapp"
    shoot app-brick-bundled 10 "$bapp/Contents/MacOS/brick" "$s/people.csv"
    step codesign-adhoc codesign --force --deep -s - "$bapp"
    shoot app-brick-bundled-signed 10 "$bapp/Contents/MacOS/brick" "$s/people.csv"
    step dmg hdiutil create -volname Brick -srcfolder "$bapp" -format UDZO -ov "$out/app/Brick.dmg"
    note "Brick.dmg: $(stat -f %z "$out/app/Brick.dmg" 2>/dev/null) bytes"
    step spctl spctl --assess --type execute -vv "$bapp"
    rm -rf "$out/app"
    ;;
*) echo "unknown phase $phase"; exit 2 ;;
esac
exit 0
