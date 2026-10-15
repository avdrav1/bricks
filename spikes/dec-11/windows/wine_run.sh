#!/usr/bin/env bash
# DEC-11 spike (throwaway), inside the cronkite-xwin container: bundle the cross-built apps
# with Fedora's MinGW GTK DLLs, then run each under Wine on Xvfb and screenshot it.
#   docker run --rm -v "$PWD":/src -v cronkite-xwin-target:/build -w /src cronkite-xwin \
#       spikes/dec-11/windows/wine_run.sh [OUT] [EXTRA_ENV...]
set -u
out=${1:-spikes/dec-11/windows/wine}
shift || true
mkdir -p "$out"
sysroot=/usr/x86_64-w64-mingw32/sys-root/mingw
rel=/build/target/x86_64-pc-windows-gnu/release
b=/build/xbundle/cronkite
here=spikes/dec-11/windows

# The bundle: exes, every DLL they (and the pixbuf loaders, gdbus) import from the sysroot.
rm -rf "$b" && mkdir -p "$b/bin" "$b/lib" "$b/share/glib-2.0/schemas" "$b/share/icons"
cp "$rel"/{brick,slate,frame,etch}.exe "$b/bin/" 2>/dev/null
cp "$sysroot/bin/gdbus.exe" "$b/bin/"
cp -r "$sysroot/lib/gdk-pixbuf-2.0" "$b/lib/"
queue=("$b"/bin/*.exe "$sysroot"/lib/gdk-pixbuf-2.0/2.10.0/loaders/*.dll)
declare -A seen
while [ ${#queue[@]} -gt 0 ]; do
    f=${queue[0]}; queue=("${queue[@]:1}")
    for dll in $(x86_64-w64-mingw32-objdump -p "$f" | awk '/DLL Name:/ {print $3}'); do
        [ -n "${seen[$dll]:-}" ] && continue
        seen[$dll]=1
        if [ -f "$sysroot/bin/$dll" ]; then
            cp "$sysroot/bin/$dll" "$b/bin/"
            queue+=("$sysroot/bin/$dll")
        fi
    done
done
glib-compile-schemas --targetdir="$b/share/glib-2.0/schemas" "$sysroot/share/glib-2.0/schemas" 2>/dev/null ||
    cp "$sysroot/share/glib-2.0/schemas/gschemas.compiled" "$b/share/glib-2.0/schemas/" 2>/dev/null
cp -r "$sysroot/share/icons/Adwaita" "$sysroot/share/icons/hicolor" "$b/share/icons/" 2>/dev/null
{
    echo "dlls: $(ls "$b"/bin/*.dll | wc -l)"
    echo "total: $(du -sm "$b" | cut -f1) MB (bin $(du -sm "$b/bin" | cut -f1) MB, icons $(du -sm "$b/share/icons" | cut -f1) MB)"
    du -k "$b"/bin/*.exe
    (cd /build/xbundle && rm -f c.zip && zip -qr9 c.zip cronkite && echo "zip -9: $(du -m c.zip | cut -f1) MB")
} >"$out/size.txt" 2>&1

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
