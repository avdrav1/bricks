#!/usr/bin/env bash
# MAC-7: cairo for the macOS bundles, built without lzo.
#
#   scripts/mac_cairo.sh PREFIX     # scripts/mac_bundle.sh uses target/mac-cairo
#
# Homebrew's cairo links its script interpreter (libcairo-script-interpreter, which GTK
# loads) to liblzo2, which is GPL-2.0-or-later. The bundles ship no GPL code (user
# decision, 2026-10-16), so they carry this cairo instead: the same upstream release as
# Homebrew's installed cairo, built with Homebrew's options (its formula's meson
# arguments) but `-Dlzo=disabled` (lzo only decompresses compressed cairo scripts) and
# `-Dtests=disabled`. Same source, same options otherwise, so the same ABI: Homebrew's GTK,
# Pango, and librsvg use it unchanged. It links Homebrew's pixman, freetype, fontconfig,
# libpng, glib, and X11 libraries, which the bundle carries anyway.
#
# Installs into PREFIX (lib/, include/), puts cairo's licence texts in
# PREFIX/share/licenses/cairo, and writes PREFIX/cronkite-build.txt (version, source,
# sha256, options), which scripts/mac_licenses.py names in THIRD-PARTY.txt. A PREFIX whose
# record matches this script's is reused (release-macos.yml caches it). It fails if
# Homebrew's cairo isn't this version (update the pin with it) or anything built links lzo.
set -euo pipefail
VERSION=1.18.4
SOURCE=https://cairographics.org/releases/cairo-$VERSION.tar.xz
SHA256=445ed8208a6e4823de1226a74ca319d3600e83f6369f99b14265006599c32ccb
# Homebrew's formula's arguments, then ours.
OPTIONS="-Dfontconfig=enabled -Dfreetype=enabled -Dpng=enabled -Dglib=enabled -Dxcb=enabled -Dxlib=enabled -Dzlib=enabled -Dquartz=enabled -Dlzo=disabled -Dtests=disabled"

prefix=${1:?usage: mac_cairo.sh PREFIX}
mkdir -p "$prefix"
prefix=$(cd "$prefix" && pwd)
record="version=$VERSION
source=$SOURCE
sha256=$SHA256
options=--buildtype=release --default-library=shared $OPTIONS
ldflags=-Wl,-headerpad_max_install_names"

installed=$(brew list --versions cairo | awk '{print $2}')
[[ ${installed%_*} == "$VERSION" ]] ||
  { echo "mac_cairo: Homebrew's cairo is ${installed:-not installed}, this builds $VERSION: update VERSION and SHA256"; exit 1; }
if [[ -f $prefix/cronkite-build.txt && $(cat "$prefix/cronkite-build.txt") == "$record" && -f $prefix/lib/libcairo.2.dylib ]]; then
  echo "mac_cairo: cairo $VERSION without lzo in $prefix (built before)"
  exit 0
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
curl -fsSL "$SOURCE" -o "$work/cairo.tar.xz"
got=$(shasum -a 256 "$work/cairo.tar.xz" | cut -d' ' -f1)
[[ $got == "$SHA256" ]] || { echo "mac_cairo: $SOURCE has sha256 $got, not $SHA256"; exit 1; }
tar -xf "$work/cairo.tar.xz" -C "$work"
src=$work/cairo-$VERSION

rm -rf "$prefix"
mkdir -p "$prefix"
# Room in the load commands for scripts/mac_bundle.sh's install_name_tool, as Homebrew's.
# shellcheck disable=SC2086 # the options are words
MACOSX_DEPLOYMENT_TARGET=14.0 LDFLAGS=-Wl,-headerpad_max_install_names meson setup "$work/build" "$src" \
  --prefix="$prefix" --libdir=lib --buildtype=release --wrap-mode=nofallback --default-library=shared $OPTIONS \
  >"$work/setup.log" 2>&1 || { cat "$work/setup.log"; exit 1; }
meson compile -C "$work/build" >"$work/compile.log" 2>&1 || { tail -50 "$work/compile.log"; exit 1; }
meson install -C "$work/build" >"$work/install.log" 2>&1 || { cat "$work/install.log"; exit 1; }

for lib in libcairo.2.dylib libcairo-gobject.2.dylib libcairo-script-interpreter.2.dylib; do
  [[ -f $prefix/lib/$lib ]] || { echo "mac_cairo: the build made no $lib"; exit 1; }
  if otool -L "$prefix/lib/$lib" | grep -qi lzo; then echo "mac_cairo: $lib links lzo"; exit 1; fi
done
mkdir -p "$prefix/share/licenses/cairo"
cp "$src"/COPYING* "$prefix/share/licenses/cairo/"
printf '%s\n' "$record" >"$prefix/cronkite-build.txt"
echo "mac_cairo: built cairo $VERSION without lzo into $prefix"
