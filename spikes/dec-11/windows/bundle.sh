#!/usr/bin/env bash
# DEC-11 spike (throwaway), MSYS2 UCRT64 only: a self-contained folder of the four apps,
# the DLLs they (and GDK-Pixbuf's loaders, and gdbus.exe) load from /ucrt64, and the data
# GTK reads at run time. Prints its size before and after stripping, and zipped.
set -u
out=${1:-bundle}
dest=$out/cronkite
rm -rf "$dest"
mkdir -p "$dest/bin" "$dest/lib" "$dest/share/glib-2.0/schemas" "$dest/share/icons"
for app in brick slate frame etch; do
    [ -f "target/release/$app.exe" ] && cp "target/release/$app.exe" "$dest/bin/"
done
cp -r /ucrt64/lib/gdk-pixbuf-2.0 "$dest/lib/"
cp /ucrt64/bin/gdbus.exe "$dest/bin/"
loaders=$(ls /ucrt64/lib/gdk-pixbuf-2.0/2.10.0/loaders/*.dll)
# shellcheck disable=SC2086
ldd "$dest"/bin/*.exe $loaders | awk '$3 ~ /^\/ucrt64\// {print $3}' | sort -u >"$out/dlls.txt"
xargs -a "$out/dlls.txt" cp -t "$dest/bin/"
cp /ucrt64/share/glib-2.0/schemas/gschemas.compiled "$dest/share/glib-2.0/schemas/"
cp -r /ucrt64/share/icons/Adwaita /ucrt64/share/icons/hicolor "$dest/share/icons/"
[ -d /ucrt64/etc/fonts ] && mkdir -p "$dest/etc" && cp -r /ucrt64/etc/fonts "$dest/etc/"

{
    echo "dlls: $(wc -l <"$out/dlls.txt")"
    echo "total (unstripped): $(du -sm "$dest" | cut -f1) MB"
    echo "  bin: $(du -sm "$dest/bin" | cut -f1) MB, of which exes:"
    for f in "$dest"/bin/{brick,slate,frame,etch}.exe; do [ -f "$f" ] && echo "    $(basename "$f") $(du -k "$f" | cut -f1) KB"; done
    echo "  share/icons: $(du -sm "$dest/share/icons" | cut -f1) MB"
    echo "  lib: $(du -sm "$dest/lib" | cut -f1) MB"
    echo "largest DLLs (KB):"
    du -k "$dest"/bin/*.dll | sort -rn | head -15
    strip "$dest"/bin/*.exe
    echo "total (exes stripped): $(du -sm "$dest" | cut -f1) MB"
    for f in "$dest"/bin/{brick,slate,frame,etch}.exe; do [ -f "$f" ] && echo "    $(basename "$f") stripped $(du -k "$f" | cut -f1) KB"; done
    (cd "$out" && zip -qr9 cronkite.zip cronkite)
    echo "zip -9: $(du -m "$out/cronkite.zip" | cut -f1) MB"
} | tee "$out/size.txt"
