#!/usr/bin/env bash
# MAC-7: an app's self-contained, ad-hoc-signed macOS app bundle, its DMG, and its tarball.
#
#   scripts/mac_bundle.sh APP [BIN_DIR [OUT]]   # BIN_DIR: target/release; OUT: dist
#
# On an arm64 Mac (release-macos.yml's macos-14 runner, or `scripts/ship.sh APP build
# --macos` on a Mac) with Homebrew's gtk4, adwaita-icon-theme, librsvg (rsvg-convert and
# gdk-pixbuf's SVG loader), dylibbundler, meson, and ninja (cairo without lzo:
# scripts/mac_cairo.sh, into $CAIRO_PREFIX, default target/mac-cairo). Makes in OUT, for
# APP's version V:
#   APP-V-arm64-macos/<Name>.app        the bundle (suite_platform::bundle has its layout)
#   APP-V-arm64-macos.tar.gz            <Name>.app alone, for the curl installer (sites/APP/install.sh)
#   APP-V-arm64-macos.dmg               <Name>.app and a link to /Applications
#   APP-V-arm64-macos.{tar.gz,dmg}.sha256
# The bundle runs without Homebrew: every dylib it loads is in Contents/Frameworks, linked
# by @executable_path, and GTK's data is in Contents/Resources (the app points the
# libraries there when it starts, suite_platform::bundle). Contents/Resources/licenses has
# every bundled formula's licence files and THIRD-PARTY.txt (scripts/mac_licenses.py). It
# fails if any Mach-O still names a library outside the bundle and the system, or a
# bundled library has no formula or licence.
set -euo pipefail
app=${1:?usage: mac_bundle.sh APP [BIN_DIR [OUT]]}
root=$(cd "$(dirname "$0")/.." && pwd)
bin=$(cd "${2:-$root/target/release}" && pwd)
out=${3:-$root/dist}
mkdir -p "$out"
out=$(cd "$out" && pwd)
[[ $(uname -s) == Darwin && $(uname -m) == arm64 ]] || { echo "mac_bundle: needs an arm64 Mac"; exit 1; }
for tool in brew dylibbundler rsvg-convert glib-compile-schemas iconutil hdiutil codesign meson ninja; do
  command -v "$tool" >/dev/null || { echo "mac_bundle: $tool not found (brew install gtk4 adwaita-icon-theme librsvg dylibbundler meson ninja)"; exit 1; }
done
brew=$(brew --prefix)
[[ -x $bin/$app ]] || { echo "mac_bundle: no $bin/$app (cargo build --release -p $app)"; exit 1; }

version=$(python3 -c 'import sys; sys.path.insert(0, sys.argv[1]); import mac_app; print(mac_app.version(sys.argv[2]))' "$root/scripts" "$app")
name=$app-$version-arm64-macos
stage=$out/$name
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
rm -rf "$stage"
mkdir -p "$stage"

# The skeleton: Info.plist, the binary, the bundled fonts (MAC-6; Brick, Slate, Frame).
[[ $app == etch ]] || python3 "$root/scripts/fetch_fonts.py" "$root/target/fonts" >/dev/null
bundle=$(python3 "$root/scripts/mac_app.py" "$app" "$bin" "$stage")
C=$bundle/Contents
R=$C/Resources
title=$(basename "$bundle" .app)
id=$(plutil -extract CFBundleIdentifier raw "$C/Info.plist")
strip -x "$C/MacOS/$app"
mkdir -p "$R" "$C/Frameworks"
cp "$root"/LICENSE* "$R/"

# The icon: the Linux SVG at every size Finder and the Dock ask for.
svg=$root/packaging/$id.svg
set_dir=$work/$app.iconset
mkdir -p "$set_dir"
for s in 16 32 128 256 512; do
  rsvg-convert -w $s -h $s "$svg" -o "$set_dir/icon_${s}x${s}.png"
  rsvg-convert -w $((s * 2)) -h $((s * 2)) "$svg" -o "$set_dir/icon_${s}x${s}@2x.png"
done
iconutil -c icns "$set_dir" -o "$R/$app.icns"

# GTK's GSettings schemas (the file chooser's, colour and emoji choosers', inspector's).
schemas=$R/share/glib-2.0/schemas
mkdir -p "$schemas"
cp "$brew"/share/glib-2.0/schemas/org.gtk.gtk4.*.gschema.xml "$schemas/"
glib-compile-schemas "$schemas"

# Icons: Adwaita's symbolic icons (GTK's widgets draw them; GTK renders them itself) and
# hicolor with the app's icon as PNGs, which GTK decodes itself (the About dialog's). The
# hicolor index is written here for those folders alone: hicolor-icon-theme's is GPL-2.0.
icons=$R/share/icons
mkdir -p "$icons/Adwaita" "$icons/hicolor"
cp "$brew/share/icons/Adwaita/index.theme" "$icons/Adwaita/"
# Dereferenced: the theme's aliases are links, some to folders this copy leaves out.
cp -RL "$brew/share/icons/Adwaita/symbolic" "$icons/Adwaita/"
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

# gdk-pixbuf's loaders (SVG from librsvg, GIF, ICO, ...) and their list, whose module paths
# are relative to Contents/Resources; the app writes them out absolute at start.
pixbuf=lib/gdk-pixbuf-2.0/2.10.0
loaders=$R/$pixbuf/loaders
mkdir -p "$loaders"
GDK_PIXBUF_MODULEDIR=$brew/$pixbuf/loaders gdk-pixbuf-query-loaders >"$work/loaders.cache"
python3 - "$work/loaders.cache" "$loaders" "$pixbuf/loaders" "$work/loader-sources.tsv" >"$R/$pixbuf/loaders.cache" <<'EOF'
import os, shutil, sys
cache, dest, rel, sources = sys.argv[1:]
with open(sources, "w") as tsv:
    for line in open(cache).read().splitlines():
        s = line.strip()
        if s.startswith('"/') and s.endswith('.so"') and s.count('"') == 2:
            src = s[1:-1]
            base = os.path.basename(src)
            shutil.copy2(os.path.realpath(src), os.path.join(dest, base))
            os.chmod(os.path.join(dest, base), 0o755)
            tsv.write(f"{base}\t{os.path.realpath(src)}\n")
            line = f'"{rel}/{base}"'
        elif line.startswith("# LoaderDir"):
            line = f"# LoaderDir = {rel}"
        print(line)
EOF

# fontconfig: Homebrew's fonts.conf (the Mac's font folders) without Homebrew's own paths,
# its cache first in the app's Caches folder, and conf.d (the metric aliases, MAC-6).
fc=$R/etc/fonts
mkdir -p "$fc"
python3 - "$brew" "$id" "$brew/etc/fonts/fonts.conf" >"$fc/fonts.conf" <<'EOF'
import sys
brew, app_id, conf = sys.argv[1:]
lines = open(conf).read().splitlines()
has_conf_d = any(brew not in l and "conf.d</include>" in l for l in lines)
for line in lines:
    if brew in line:
        print(f"mac_bundle: fonts.conf drops: {line.strip()}", file=sys.stderr)
        continue
    print(line)
    if line.strip().startswith("<fontconfig>"):
        print(f"  <cachedir>~/Library/Caches/{app_id}/fontconfig</cachedir>")
        if not has_conf_d:
            print('  <include ignore_missing="yes">conf.d</include>')
EOF
grep -q "Caches/$id/fontconfig" "$fc/fonts.conf" || { echo "mac_bundle: no <fontconfig> in $brew/etc/fonts/fonts.conf"; exit 1; }
cp -RL "$brew/etc/fonts/conf.d" "$fc/conf.d"

# The libraries: every Homebrew dylib the app and the loaders need, copied into Frameworks
# and relinked to @executable_path/../Frameworks.
xs=(-x "$C/MacOS/$app")
for so in "$loaders"/*.so; do
  xs+=(-x "$so")
  # librsvg builds its loader as a dylib named .so, still named by its Homebrew path.
  if [[ -n $(otool -D "$so" | tail -n +2) ]]; then
    install_name_tool -id "@executable_path/../Resources/$pixbuf/loaders/$(basename "$so")" "$so" 2>>"$work/install_name_tool.log"
  fi
done
dylibbundler -of -b "${xs[@]}" -d "$C/Frameworks" -p @executable_path/../Frameworks/ -s "$brew/lib" </dev/null >"$work/dylibbundler.log" 2>&1 ||
  { cat "$work/dylibbundler.log"; exit 1; }

# cairo without lzo (GPL): scripts/mac_cairo.sh's build of Homebrew's cairo release takes
# the place of Homebrew's cairo libraries, under the same names, so everything linking them
# (GTK, Pango, librsvg) loads ours. Then Frameworks keeps only what the app and the loaders
# reach, which drops liblzo2.
cairo=${CAIRO_PREFIX:-$root/target/mac-cairo}
"$root/scripts/mac_cairo.sh" "$cairo"
python3 - "$C" "$cairo/lib" "$app" "$loaders" <<'EOF'
import os, shutil, subprocess, sys
contents, cairo_lib, app, loaders = sys.argv[1:]
fw = os.path.join(contents, "Frameworks")
REF = "@executable_path/../Frameworks/"

def otool(flag, f):
    out = subprocess.run(["otool", flag, f], capture_output=True, text=True, check=True).stdout
    return [l.strip().split(" (")[0] for l in out.splitlines()[1:] if l.strip()]

def deps(f):
    own = otool("-D", f)
    return [d for d in otool("-L", f) if d not in own]

def tool(*args):
    subprocess.run(["install_name_tool", *args], check=True)

for base in ("libcairo.2.dylib", "libcairo-gobject.2.dylib", "libcairo-script-interpreter.2.dylib"):
    dest = os.path.join(fw, base)
    if not os.path.exists(dest):
        continue
    os.remove(dest)
    shutil.copy2(os.path.realpath(os.path.join(cairo_lib, base)), dest)
    os.chmod(dest, 0o755)
    tool("-id", REF + base, dest)
    for d in deps(dest):
        if d.startswith(("/usr/lib/", "/System/", REF)):
            continue
        name = os.path.basename(d)
        if not os.path.exists(os.path.join(fw, name)):
            sys.exit(f"mac_bundle: our {base} needs {d}, which the bundle lacks")
        tool("-change", d, REF + name, dest)
    print(f"mac_bundle: {base} is cairo built without lzo")

keep, todo = set(), [os.path.join(contents, "MacOS", app)]
todo += [os.path.join(loaders, s) for s in os.listdir(loaders)]
while todo:
    for d in deps(todo.pop()):
        if d.startswith(REF) and d[len(REF):] not in keep:
            keep.add(d[len(REF):])
            todo.append(os.path.join(fw, d[len(REF):]))
for base in sorted(os.listdir(fw)):
    if base not in keep:
        os.remove(os.path.join(fw, base))
        print(f"mac_bundle: dropped {base}, which nothing loads now")
for base in sorted(keep):
    if "lzo" in base.lower() or any("lzo" in d.lower() for d in deps(os.path.join(fw, base))):
        sys.exit(f"mac_bundle: {base} still brings lzo")
EOF

# Nothing may point outside the bundle but the system's libraries.
bad=0
while IFS= read -r f; do
  if otool -L "$f" | tail -n +2 | grep -vE '^[[:space:]]+(/usr/lib/|/System/|@executable_path/\.\./(Frameworks|Resources)/)' | grep -q .; then
    echo "mac_bundle: $f links outside the bundle:"; otool -L "$f" | tail -n +2 | grep -vE '^[[:space:]]+(/usr/lib/|/System/|@executable_path/\.\./(Frameworks|Resources)/)'
    bad=1
  fi
done < <(find "$C" -type f \( -name '*.dylib' -o -name '*.so' -o -path "*/MacOS/*" \))
((bad == 0)) || exit 1

# Third-party licence notices: each bundled library's and data's Homebrew formula, its
# licence files, version, SPDX licence, and source (Resources/licenses/THIRD-PARTY.txt).
python3 "$root/scripts/mac_licenses.py" "$C" "$work/loader-sources.tsv" "$cairo" >"$work/licenses.txt" ||
  { cat "$work/licenses.txt"; exit 1; }
cat "$work/licenses.txt"

# Ad-hoc signatures: each library and loader, then the app (arm64 runs no unsigned code).
broken=$(find "$C" -type l ! -exec test -e {} \; -print)
[[ -z $broken ]] || { echo "mac_bundle: broken links in the bundle: $broken"; exit 1; }
find "$C/Frameworks" "$loaders" -type f \( -name '*.dylib' -o -name '*.so' \) -exec codesign --force -s - {} + 2>"$work/codesign.log" ||
  { cat "$work/codesign.log"; exit 1; }
codesign --force -s - "$bundle"
codesign --verify --deep --strict --verbose=2 "$bundle"

# The tarball for the installer and the DMG for the browser.
COPYFILE_DISABLE=1 tar -C "$stage" -czf "$out/$name.tar.gz" "$title.app"
mkdir -p "$work/dmg"
ditto "$bundle" "$work/dmg/$title.app"
ln -s /Applications "$work/dmg/Applications"
for try in 1 2 3; do
  # ULMO (LZMA): the smallest of hdiutil's formats; macOS 10.15 and later read it.
  hdiutil create -quiet -volname "$title" -srcfolder "$work/dmg" -fs HFS+ -format ULMO -ov "$out/$name.dmg" && break
  ((try < 3)) || exit 1
  sleep 5 # hdiutil now and then finds the image busy on CI
done
(cd "$out" && shasum -a 256 "$name.tar.gz" >"$name.tar.gz.sha256" && shasum -a 256 "$name.dmg" >"$name.dmg.sha256")

echo "$title.app $version: $(du -sh "$bundle" | cut -f1) ($(ls "$C/Frameworks" | wc -l | tr -d ' ') dylibs in $(du -sh "$C/Frameworks" | cut -f1), loaders: $(cd "$loaders" && ls | tr '\n' ' '))"
echo "  $out/$name.tar.gz: $(stat -f %z "$out/$name.tar.gz") bytes, sha256 $(cut -d' ' -f1 "$out/$name.tar.gz.sha256")"
echo "  $out/$name.dmg: $(stat -f %z "$out/$name.dmg") bytes, sha256 $(cut -d' ' -f1 "$out/$name.dmg.sha256")"
