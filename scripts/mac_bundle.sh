#!/usr/bin/env bash
# MAC-7: an app's self-contained, ad-hoc-signed macOS app bundle, its DMG, and its tarball.
#
#   scripts/mac_bundle.sh APP [BIN_DIR [OUT]]   # BIN_DIR: target/release; OUT: dist
#
# On an arm64 Mac (release-macos.yml's macos-14 runner, or `scripts/ship.sh APP build
# --macos` on a Mac) with Homebrew's gtk4, adwaita-icon-theme, librsvg (rsvg-convert and
# gdk-pixbuf's SVG loader), and dylibbundler. Makes in OUT, for APP's version V:
#   APP-V-arm64-macos/<Name>.app        the bundle (suite_platform::bundle has its layout)
#   APP-V-arm64-macos.tar.gz            <Name>.app alone, for the curl installer (sites/APP/install.sh)
#   APP-V-arm64-macos.dmg               <Name>.app and a link to /Applications
#   APP-V-arm64-macos.{tar.gz,dmg}.sha256
# The bundle runs without Homebrew: every dylib it loads is in Contents/Frameworks, linked
# by @executable_path, and GTK's data is in Contents/Resources (the app points the
# libraries there when it starts, suite_platform::bundle). It fails if any Mach-O still
# names a library outside the bundle and the system.
set -euo pipefail
app=${1:?usage: mac_bundle.sh APP [BIN_DIR [OUT]]}
root=$(cd "$(dirname "$0")/.." && pwd)
bin=$(cd "${2:-$root/target/release}" && pwd)
out=${3:-$root/dist}
mkdir -p "$out"
out=$(cd "$out" && pwd)
[[ $(uname -s) == Darwin && $(uname -m) == arm64 ]] || { echo "mac_bundle: needs an arm64 Mac"; exit 1; }
for tool in brew dylibbundler rsvg-convert glib-compile-schemas iconutil hdiutil codesign; do
  command -v "$tool" >/dev/null || { echo "mac_bundle: $tool not found (brew install gtk4 adwaita-icon-theme librsvg dylibbundler)"; exit 1; }
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
# hicolor with the app's icon as PNGs, which GTK decodes itself (the About dialog's).
icons=$R/share/icons
mkdir -p "$icons/Adwaita" "$icons/hicolor"
cp "$brew/share/icons/Adwaita/index.theme" "$icons/Adwaita/"
cp -R "$brew/share/icons/Adwaita/symbolic" "$icons/Adwaita/"
cp "$brew/share/icons/hicolor/index.theme" "$icons/hicolor/"
for s in 16 32 48 64 128 256 512; do
  mkdir -p "$icons/hicolor/${s}x${s}/apps"
  rsvg-convert -w $s -h $s "$svg" -o "$icons/hicolor/${s}x${s}/apps/$id.png"
done

# gdk-pixbuf's loaders (SVG from librsvg, GIF, ICO, ...) and their list, whose module paths
# are relative to Contents/Resources; the app writes them out absolute at start.
pixbuf=lib/gdk-pixbuf-2.0/2.10.0
loaders=$R/$pixbuf/loaders
mkdir -p "$loaders"
GDK_PIXBUF_MODULEDIR=$brew/$pixbuf/loaders gdk-pixbuf-query-loaders >"$work/loaders.cache"
python3 - "$work/loaders.cache" "$loaders" "$pixbuf/loaders" >"$R/$pixbuf/loaders.cache" <<'EOF'
import os, shutil, sys
cache, dest, rel = sys.argv[1:]
for line in open(cache).read().splitlines():
    s = line.strip()
    if s.startswith('"/') and s.endswith('.so"') and s.count('"') == 2:
        src = s[1:-1]
        base = os.path.basename(src)
        shutil.copy2(os.path.realpath(src), os.path.join(dest, base))
        os.chmod(os.path.join(dest, base), 0o755)
        line = f'"{rel}/{base}"'
    elif line.startswith("# LoaderDir"):
        line = f"# LoaderDir = {rel}"
    print(line)
EOF

# fontconfig: Homebrew's fonts.conf (the Mac's font folders) without Homebrew's own paths,
# its cache first in the app's Caches folder, and conf.d (the metric aliases, MAC-6).
fc=$R/etc/fonts
mkdir -p "$fc"
python3 - "$brew" "$id" <"$brew/etc/fonts/fonts.conf" >"$fc/fonts.conf" <<'EOF'
import sys
brew, app_id = sys.argv[1:]
lines = sys.stdin.read().splitlines()
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
for so in "$loaders"/*.so; do xs+=(-x "$so"); done
dylibbundler -of -b "${xs[@]}" -d "$C/Frameworks" -p @executable_path/../Frameworks/ -s "$brew/lib" </dev/null >"$work/dylibbundler.log" 2>&1 ||
  { cat "$work/dylibbundler.log"; exit 1; }

# Nothing may point outside the bundle but the system's libraries.
bad=0
while IFS= read -r f; do
  if otool -L "$f" | tail -n +2 | grep -vE '^[[:space:]]+(/usr/lib/|/System/|@executable_path/\.\./Frameworks/)' | grep -q .; then
    echo "mac_bundle: $f links outside the bundle:"; otool -L "$f" | tail -n +2 | grep -vE '^[[:space:]]+(/usr/lib/|/System/|@executable_path/\.\./Frameworks/)'
    bad=1
  fi
done < <(find "$C" -type f \( -name '*.dylib' -o -name '*.so' -o -path "*/MacOS/*" \))
((bad == 0)) || exit 1

# Ad-hoc signatures: each library and loader, then the app (arm64 runs no unsigned code).
find "$C/Frameworks" "$loaders" -type f \( -name '*.dylib' -o -name '*.so' \) -exec codesign --force -s - {} + 2>"$work/codesign.log" ||
  { cat "$work/codesign.log"; exit 1; }
codesign --force -s - "$bundle"
codesign --verify --deep --strict "$bundle"

# The tarball for the installer and the DMG for the browser.
COPYFILE_DISABLE=1 tar -C "$stage" -czf "$out/$name.tar.gz" "$title.app"
mkdir -p "$work/dmg"
ditto "$bundle" "$work/dmg/$title.app"
ln -s /Applications "$work/dmg/Applications"
for try in 1 2 3; do
  hdiutil create -quiet -volname "$title" -srcfolder "$work/dmg" -fs HFS+ -format UDZO -ov "$out/$name.dmg" && break
  ((try < 3)) || exit 1
  sleep 5 # hdiutil now and then finds the image busy on CI
done
(cd "$out" && shasum -a 256 "$name.tar.gz" >"$name.tar.gz.sha256" && shasum -a 256 "$name.dmg" >"$name.dmg.sha256")

echo "$title.app $version: $(du -sh "$bundle" | cut -f1) ($(ls "$C/Frameworks" | wc -l | tr -d ' ') dylibs in $(du -sh "$C/Frameworks" | cut -f1), loaders: $(cd "$loaders" && ls | tr '\n' ' '))"
echo "  $out/$name.tar.gz: $(stat -f %z "$out/$name.tar.gz") bytes, sha256 $(cut -d' ' -f1 "$out/$name.tar.gz.sha256")"
echo "  $out/$name.dmg: $(stat -f %z "$out/$name.dmg") bytes, sha256 $(cut -d' ' -f1 "$out/$name.dmg.sha256")"
