#!/usr/bin/env bash
# WIN-5: an app's self-contained Windows folder and its per-user Inno Setup installer.
#
#   scripts/win_bundle.sh APP [BIN_DIR [OUT]]   # BIN_DIR: target/release; OUT: dist
#
# In MSYS2's UCRT64 shell on Windows (release-windows.yml's windows-latest runner) with
# MSYS2's gtk4, librsvg (rsvg-convert), python, meson, and ninja, and Inno Setup 6
# (ISCC.exe; $ISCC, else on PATH, else in Program Files). The cairo and libtiff it carries
# are built without GPL code by scripts/win_libs.sh into $WIN_LIBS (default
# target/win-libs). Makes in OUT, for APP's version V:
#   APP-V-x86_64-windows/<Name>\                 the folder Setup installs (layout:
#                                                suite_platform::bundle's Windows section)
#   APP-V-x86_64-windows-setup.exe              the installer (packaging/windows/setup.iss)
#   APP-V-x86_64-windows-setup.exe.sha256
# The folder runs without MSYS2: <Name>\bin has the stripped app, every DLL it, GLib's
# gdbus.exe, and gdk-pixbuf's loaders import (recursively, delay-loaded ones too) from
# MSYS2 or the rebuilt libraries, and GTK's data is beside it in MSYS2's prefix layout,
# where GLib, GTK, gdk-pixbuf, and fontconfig look relative to their DLLs. <Name>\licenses
# has every bundled MSYS2 package's licence files and THIRD-PARTY.txt
# (scripts/win_licenses.py), and THIRD-PARTY-RUST.txt, the Rust crates compiled into the
# app for x86_64-pc-windows-gnu (scripts/rust_licenses.py). It fails if a PE file imports
# a DLL that is neither in bin nor Windows' own, or a bundled library or crate has no
# licence text or one the release can't carry (no GPL code).
set -euo pipefail
app=${1:?usage: win_bundle.sh APP [BIN_DIR [OUT]]}
[[ ${MSYSTEM:-} == UCRT64 ]] || { echo "win_bundle: run it in MSYS2's UCRT64 shell"; exit 1; }
root=$(cd "$(dirname "$0")/.." && pwd)
bin=$(cd "${2:-$root/target/release}" && pwd)
out=${3:-$root/dist}
mkdir -p "$out"
out=$(cd "$out" && pwd)
for tool in python3 objdump strip glib-compile-schemas rsvg-convert cygpath; do
  command -v "$tool" >/dev/null || { echo "win_bundle: $tool not found (pacman -S mingw-w64-ucrt-x86_64-{gtk4,librsvg,python})"; exit 1; }
done
iscc=${ISCC:-$(command -v iscc || command -v ISCC || true)}
[[ -n $iscc ]] || iscc="/c/Program Files (x86)/Inno Setup 6/ISCC.exe"
[[ -x $iscc ]] || { echo "win_bundle: no Inno Setup 6 ($iscc; choco install innosetup)"; exit 1; }
[[ -f $bin/$app.exe ]] || { echo "win_bundle: no $bin/$app.exe (cargo build --release -p $app)"; exit 1; }
win() { cygpath -m "$1"; }

name=$(python3 "$root/scripts/win_app.py" name "$app")
version=$(python3 "$root/scripts/win_app.py" version "$app")
id=dev.cronkite.$name
stem=$app-$version-x86_64-windows
stage=$out/$stem
B=$stage/$name
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
rm -rf "$stage" "$out/$stem-setup.exe" "$out/$stem-setup.exe.sha256"
mkdir -p "$B/bin"
libs=${WIN_LIBS:-$root/target/win-libs}
"$root/scripts/win_libs.sh" "$libs"

# The app, GLib's session bus (GApplication starts it, DEC-11 R13), the app's licences, and
# the bundled fonts (WIN-4; Brick, Slate, and Frame: Etch uses Windows' fonts), which the
# app finds in ..\fonts beside bin (suite_ui::fonts).
cp "$bin/$app.exe" "$B/bin/"
strip "$B/bin/$app.exe"
cp /ucrt64/bin/gdbus.exe "$B/bin/"
cp "$root"/LICENSE* "$B/"
if [[ $app != etch ]]; then
  python3 "$root/scripts/fetch_fonts.py" "$root/target/fonts" >/dev/null
  cp -r "$root/target/fonts" "$B/fonts"
fi
printf '%s\t%s\n' "bin/gdbus.exe" /ucrt64/bin/gdbus.exe >"$work/sources.tsv"

# The icon: the Linux SVG as a .ico for Setup, the Start menu, and Settings > Apps.
svg=$root/packaging/$id.svg
pngs=()
for s in 16 20 24 32 40 48 64 256; do
  rsvg-convert -w $s -h $s "$svg" -o "$work/icon-$s.png"
  pngs+=("$(win "$work/icon-$s.png")")
done
python3 "$root/scripts/win_app.py" ico "$(win "$B/$app.ico")" "${pngs[@]}"

# GTK's GSettings schemas (the file chooser's, colour and emoji choosers', inspector's).
schemas=$B/share/glib-2.0/schemas
mkdir -p "$schemas"
cp /ucrt64/share/glib-2.0/schemas/org.gtk.gtk4.*.gschema.xml "$schemas/"
glib-compile-schemas "$schemas"

# Icons: Adwaita's symbolic icons (GTK's widgets draw them) and hicolor with the app's
# icon as PNGs (the window's and About's icon). The hicolor index is written here for
# those folders alone: hicolor-icon-theme's is GPL-2.0.
icons=$B/share/icons
mkdir -p "$icons/Adwaita" "$icons/hicolor"
cp /ucrt64/share/icons/Adwaita/index.theme "$icons/Adwaita/"
cp -rL /ucrt64/share/icons/Adwaita/symbolic "$icons/Adwaita/"
sizes="16 32 48 64 128 256 512"
{
  printf '[Icon Theme]\nName=Hicolor\nComment=Fallback icon theme\nHidden=true\nDirectories='
  for s in $sizes; do printf '%s' "${s}x${s}/apps,"; done
  printf '\n'
  for s in $sizes; do printf '\n[%sx%s/apps]\nSize=%s\nContext=Applications\nType=Threshold\n' "$s" "$s" "$s"; done
} >"$icons/hicolor/index.theme"
for s in $sizes; do
  mkdir -p "$icons/hicolor/${s}x${s}/apps"
  rsvg-convert -w $s -h $s "$svg" -o "$icons/hicolor/${s}x${s}/apps/$id.png"
done

# gdk-pixbuf's loaders (SVG from librsvg, GIF, ICO, TIFF, ...) and MSYS2's list of them,
# whose module paths are relative to the prefix (gdk-pixbuf finds them from its DLL).
pixbuf=lib/gdk-pixbuf-2.0/2.10.0
mkdir -p "$B/$pixbuf/loaders"
for dll in /ucrt64/$pixbuf/loaders/*.dll; do
  cp "$dll" "$B/$pixbuf/loaders/"
  printf '%s\t%s\n' "$pixbuf/loaders/$(basename "$dll")" "$dll" >>"$work/sources.tsv"
done
cp "/ucrt64/$pixbuf/loaders.cache" "$B/$pixbuf/"
python3 - "$(win "$B")" "$pixbuf/loaders.cache" <<'EOF'
import os, re, sys
root, cache = sys.argv[1:]
listed = re.findall(r'^"([^"]+\.dll)"$', open(os.path.join(root, cache)).read(), re.M)
paths = {os.path.normcase(os.path.join(root, p.replace("\\\\", "/"))) for p in listed}
present = {os.path.normcase(os.path.join(root, os.path.dirname(cache), "loaders", f))
           for f in os.listdir(os.path.join(root, os.path.dirname(cache), "loaders"))}
if not listed or any(os.path.isabs(p.replace("\\\\", "/")) for p in listed) or paths != present:
    sys.exit(f"win_bundle: loaders.cache lists {sorted(listed)}, the bundle has {sorted(present)}")
print(f"win_bundle: {len(listed)} gdk-pixbuf loaders")
EOF

# fontconfig: MSYS2's fonts.conf with Windows' font folders alone and its cache in the
# app's cache folder (<cachedir prefix="xdg">: suite_platform::bundle points XDG_CACHE_HOME
# at %LOCALAPPDATA%\Cronkite\<Name>), and conf.d (the metric aliases, WIN-4).
mkdir -p "$B/etc/fonts"
python3 - "$(win /ucrt64/etc/fonts/fonts.conf)" >"$B/etc/fonts/fonts.conf" <<'EOF'
import re, sys
lines = open(sys.argv[1], encoding="utf-8").read().splitlines()
keep_dirs = ("WINDOWSFONTDIR", "WINDOWSUSERFONTDIR")
dirs = 0
for line in lines:
    s = line.strip()
    if re.fullmatch(r"<cachedir\b.*</cachedir>", s) or (
            re.fullmatch(r"<dir\b.*</dir>", s) and not any(k in s for k in keep_dirs)):
        print("win_bundle: fonts.conf drops " + s.replace("<", "[").replace(">", "]"), file=sys.stderr)
        continue
    dirs += bool(re.fullmatch(r"<dir\b.*</dir>", s))
    print(line)
    if s.startswith("<fontconfig>"):
        print('  <cachedir prefix="xdg">fontconfig</cachedir>')
if dirs == 0:
    sys.exit("win_bundle: MSYS2's fonts.conf names no Windows font folder")
EOF
grep -q 'cachedir prefix="xdg">fontconfig<' "$B/etc/fonts/fonts.conf" || { echo "win_bundle: no <fontconfig> in MSYS2's fonts.conf"; exit 1; }
grep -q 'conf.d</include>' "$B/etc/fonts/fonts.conf" || { echo "win_bundle: MSYS2's fonts.conf includes no conf.d"; exit 1; }
cp -rL /ucrt64/etc/fonts/conf.d "$B/etc/fonts/conf.d"

# The DLLs: everything the PE files in the bundle import, by name, recursively: the rebuilt
# cairo and libtiff first, then MSYS2's. Windows' own DLLs (System32, api-ms-win-*) stay.
python3 - "$(win "$B")" "$(win "$libs")" "$(win /ucrt64/bin)" "$(win "$work/sources.tsv")" <<'EOF'
import os, re, shutil, subprocess, sys
root, libs, msys_bin, sources = sys.argv[1:]
system = os.path.join(os.environ.get("SystemRoot", "C:/Windows"), "System32")
bin_dir = os.path.join(root, "bin")
rebuilt = {lib: os.path.join(libs, lib, "bin") for lib in ("cairo", "libtiff")}
def imports(path):
    out = subprocess.run(["objdump", "-p", path], capture_output=True, text=True, check=True).stdout
    return re.findall(r"^\s*DLL Name: (\S+)", out, re.M)
pes = [os.path.join(d, f) for d, _, fs in os.walk(root) for f in fs if f.lower().endswith((".exe", ".dll"))]
have = {f.lower() for f in os.listdir(bin_dir)}
missing = []
with open(sources, "a") as tsv:
    while pes:
        pe = pes.pop()
        for dll in imports(pe):
            key = dll.lower()
            if key in have or key.startswith(("api-ms-win-", "ext-ms-")):
                continue
            if os.path.exists(os.path.join(system, dll)):
                have.add(key)
                continue
            found = [(lib, os.path.join(d, dll)) for lib, d in rebuilt.items() if os.path.exists(os.path.join(d, dll))]
            if found:
                lib, src = found[0]
                source = f"rebuilt:{lib}"
            elif os.path.exists(os.path.join(msys_bin, dll)):
                src = os.path.join(msys_bin, dll)
                source = f"/ucrt64/bin/{os.path.basename(src)}"
            else:
                missing.append(f"{dll} (imported by {os.path.relpath(pe, root)})")
                have.add(key)
                continue
            dest = os.path.join(bin_dir, os.path.basename(src))
            shutil.copy2(src, dest)
            have.add(key)
            pes.append(dest)
            tsv.write(f"bin/{os.path.basename(src)}\t{source}\n")
if missing:
    sys.exit("win_bundle: DLLs found neither in MSYS2 nor in Windows:\n  " + "\n  ".join(missing))
EOF
dlls=$(find "$B/bin" -iname '*.dll' | wc -l)
# The rebuilt libraries, stripped as MSYS2 strips its packages'.
awk -F'\t' '$2 ~ /^rebuilt:/ {print $1}' "$work/sources.tsv" | while IFS= read -r f; do strip --strip-unneeded "$B/$f"; done
if find "$B" -iname '*lzo*' -o -iname '*jbig*' | grep .; then echo "win_bundle: the bundle carries lzo or jbig"; exit 1; fi

# Third-party licence notices: each bundled MSYS2 package's licence files, version,
# licence, and source (licenses\THIRD-PARTY.txt); fails on the GPL.
python3 "$root/scripts/win_licenses.py" "$(win "$B")" "$(win "$work/sources.tsv")" "$(win /)" "$(win "$libs")" >"$work/licenses.txt" ||
  { cat "$work/licenses.txt"; exit 1; }
cat "$work/licenses.txt"
# The Rust crates compiled into the app (PKG-6), after win_licenses.py, which writes the
# licenses folder afresh.
python3 "$root/scripts/rust_licenses.py" "$app" x86_64-pc-windows-gnu "$(win "$B/licenses/THIRD-PARTY-RUST.txt")"

# The installer.
# MSYS2 would take Inno Setup's /D options for paths and convert them.
MSYS2_ARG_CONV_EXCL="*" "$iscc" /Q "/DAppDefs=$(cygpath -w "$work/app.iss")" "/DSource=$(cygpath -w "$B")" \
  "/DOutputDir=$(cygpath -w "$out")" "$(cygpath -w "$root/packaging/windows/setup.iss")" >"$work/iscc.log" 2>&1 ||
  { cat "$work/iscc.log"; exit 1; }
setup=$out/$stem-setup.exe
[[ -f $setup ]] || { cat "$work/iscc.log"; echo "win_bundle: Inno Setup made no $setup"; exit 1; }
(cd "$out" && sha256sum "$stem-setup.exe" >"$stem-setup.exe.sha256")

echo "$name $version: $(du -sh "$B" | cut -f1) ($dlls DLLs in $(du -sh "$B/bin" | cut -f1) with the app, loaders: $(cd "$B/$pixbuf/loaders" && ls | tr '\n' ' '))"
echo "  $setup: $(stat -c %s "$setup") bytes, sha256 $(cut -d' ' -f1 "$setup.sha256")"
