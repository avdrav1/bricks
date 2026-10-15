#!/usr/bin/env bash
# DEC-11 spike (throwaway), inside the cronkite-xwin container: bundle the cross-built apps
# with Fedora's MinGW GTK DLLs, then run each under Wine on Xvfb and screenshot it.
#   docker run --rm -v "$PWD":/src -v cronkite-xwin-target:/build -w /src cronkite-xwin \
#       spikes/dec-11/windows/wine_run.sh [OUT] [EXTRA_ENV...]
set -u
out=${1:-spikes/dec-11/windows/wine}
shift || true
mkdir -p "$out"
rel=/build/target/x86_64-pc-windows-gnu/release
b=/build/xbundle/cronkite
here=spikes/dec-11/windows
"$here/cross_bundle.sh" "$rel" /build/xbundle "$out/size.txt" >/dev/null

export DISPLAY=:99 WINEPREFIX=/build/wineprefix WINEDEBUG=-all
Xvfb :99 -screen 0 1280x800x24 >/dev/null 2>&1 &
sleep 2
[ -d "$WINEPREFIX" ] || wineboot -i >/dev/null 2>&1
wineserver -w
for kv in "$@"; do export "$kv"; done
export GSK_DEBUG=renderer

: >"$out/summary.txt"
fsprobe=/build/fsprobe/x86_64-pc-windows-gnu/release/fsprobe.exe
[ -f "$fsprobe" ] && wine "$fsprobe" 2>&1 | tr -d '\r' >"$out/fsprobe-wine.txt"
for app in brick slate frame etch; do
    echo "$app --version: $(timeout 60 wine "$b/bin/$app.exe" --version 2>&1 | tr -d '\r' | tail -1)" >>"$out/summary.txt"
done
run() {
    local app=$1 file=$2 tag=$3
    wine "$b/bin/$app.exe" ${file:+"$file"} >"$out/$tag.log" 2>&1 &
    local pid=$!
    sleep "${WAIT:-25}"
    if kill -0 $pid 2>/dev/null; then state=running; else wait $pid; state="exited $?"; fi
    import -window root "$out/$tag.png"
    local wins
    wins=$(xdotool search --onlyvisible --name . getwindowname %@ 2>/dev/null | tr '\n' '|')
    echo "$tag: $state windows='$wins'" >>"$out/summary.txt"
    wineserver -k 2>/dev/null
    sleep 2
}
run brick "Z:\\src\\$here\\samples\\sample.csv" brick-sample.csv
run slate 'Z:\src\README.md' slate-README.md
run slate 'Z:\src\apps\slate\crates\docx\tests\data\fixture.docx' slate-fixture.docx
run frame 'Z:\src\apps\frame\crates\pptx\tests\data\fixture.pptx' frame-fixture.pptx
run etch "Z:\\src\\$here\\samples\\sample.png" etch-sample.png
cat "$out/summary.txt" "$out/size.txt"
