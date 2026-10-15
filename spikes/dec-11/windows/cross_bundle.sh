#!/usr/bin/env bash
# DEC-11 spike (throwaway), Fedora MinGW cross build: a self-contained folder PARENT/cronkite
# with the exes from REL, every DLL they (and the pixbuf loaders, gdbus) import from the
# MinGW sysroot, and GTK's run-time data. Sizes go to SIZE (a file).
#   cross_bundle.sh REL PARENT SIZE
set -u
rel=$1 parent=$2 size=$3
sysroot=/usr/x86_64-w64-mingw32/sys-root/mingw
b=$parent/cronkite
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
    x86_64-w64-mingw32-strip "$b"/bin/*.exe
    echo "total (exes stripped): $(du -sm "$b" | cut -f1) MB"
    du -k "$b"/bin/*.exe
    (cd "$parent" && rm -f cronkite.zip && zip -qr9 cronkite.zip cronkite && echo "zip -9: $(du -m cronkite.zip | cut -f1) MB")
} >"$size" 2>&1
cat "$size"
