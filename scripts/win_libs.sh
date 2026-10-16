#!/usr/bin/env bash
# WIN-5: the two MSYS2 libraries the Windows bundles carry rebuilt without GPL code.
#
#   scripts/win_libs.sh PREFIX     # in MSYS2's UCRT64 shell; win_bundle.sh uses target/win-libs
#
# The bundles ship no GPL code (user decision, 2026-10-16; MAC-7 did the same for macOS),
# and two of MSYS2's UCRT64 libraries that GTK reaches link a GPL one:
#   - cairo's script interpreter (libcairo-script-interpreter-2.dll, which GTK loads) links
#     liblzo2-2.dll, GPL-2.0-or-later; lzo only decompresses compressed cairo scripts;
#   - libtiff (libtiff-6.dll, gdk-pixbuf's TIFF loader) links libjbig-0.dll (jbigkit),
#     GPL-2.0; JBIG is a fax compression inside TIFF.
# So each is built again from MSYS2's own source package of the installed version (the
# upstream release and MSYS2's patches, applied as its PKGBUILD's prepare() does) with its
# PKGBUILD's options and makepkg's flags, but cairo with -Dlzo=disabled and libtiff with
# --disable-jbig (and neither its tests nor docs). Same source, same options otherwise, so
# the same ABI: MSYS2's GTK, Pango, librsvg, and gdk-pixbuf use them unchanged.
#
# PREFIX/cairo and PREFIX/libtiff are each an install prefix (bin\ has the DLLs) with the
# library's licence texts in share/licenses/<lib> and cronkite-build.txt (package,
# version, licence, what it's built without, options, source package and its sha256),
# which scripts/win_licenses.py names in THIRD-PARTY.txt. A prefix whose record matches is
# reused (release-windows.yml caches PREFIX). It fails if MSYS2's installed package isn't
# the pinned version (update the pin with it) or a built DLL still links lzo or jbig.
set -euo pipefail
prefix=${1:?usage: win_libs.sh PREFIX}
[[ ${MSYSTEM:-} == UCRT64 ]] || { echo "win_libs: run it in MSYS2's UCRT64 shell"; exit 1; }
mkdir -p "$prefix"
prefix=$(cd "$prefix" && pwd)
SOURCES=https://repo.msys2.org/mingw/sources
# makepkg's flags for UCRT64 (MSYS2's packages are built with them).
# shellcheck disable=SC1091
source /etc/makepkg_mingw.conf
export CFLAGS CXXFLAGS LDFLAGS

# PKG VERSION SHA256: MSYS2's source package for it, unpacked in $work/PKG; checks that
# the installed package is VERSION.
fetch_source() {
  local pkg=$1 version=$2 sha256=$3 installed got
  installed=$(pacman -Q "mingw-w64-ucrt-x86_64-$pkg" | cut -d' ' -f2)
  [[ $installed == "$version" ]] ||
    { echo "win_libs: MSYS2's $pkg is ${installed:-not installed}, this builds $version: update the pin"; exit 1; }
  curl -fsSL "$SOURCES/mingw-w64-$pkg-$version.src.tar.zst" -o "$work/$pkg.src.tar.zst"
  got=$(sha256sum "$work/$pkg.src.tar.zst" | cut -d' ' -f1)
  [[ $got == "$sha256" ]] || { echo "win_libs: mingw-w64-$pkg-$version.src.tar.zst has sha256 $got, not $sha256"; exit 1; }
  tar --zstd -xf "$work/$pkg.src.tar.zst" -C "$work"
}

# LIB RECORD: whether PREFIX/LIB was built from RECORD already.
built() { [[ -f $prefix/$1/cronkite-build.txt && $(cat "$prefix/$1/cronkite-build.txt") == "$2" ]]; }

# DLL PATTERN: fail if DLL imports a library matching PATTERN.
links_none() {
  if objdump -p "$1" | grep -i "DLL Name:.*$2"; then echo "win_libs: $(basename "$1") still links $2"; exit 1; fi
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# cairo: mingw-w64-cairo's PKGBUILD's meson options and CFLAGS (its shared build), then ours.
CAIRO=1.18.6-2
CAIRO_SHA256=ff7124c2155b1169fe102a1dcba1cd7a21f69dace3adf88f1dd8f85ef7ad1e19
CAIRO_OPTIONS="--buildtype=plain --wrap-mode=nofallback -Dauto_features=enabled -Dtests=disabled -Ddwrite=enabled -Dfontconfig=enabled -Dfreetype=enabled -Dpng=enabled -Dtee=enabled -Dzlib=enabled -Dxlib=disabled -Dxcb=disabled -Dspectre=disabled -Dsymbol-lookup=disabled --default-library=shared -Dlzo=disabled"
CAIRO_CFLAGS="-DGIO_STATIC_COMPILATION -DGLIB_STATIC_COMPILATION -DGMODULE_STATIC_COMPILATION -DGOBJECT_STATIC_COMPILATION -Wno-error=incompatible-pointer-types"
record="package=mingw-w64-ucrt-x86_64-cairo
version=$CAIRO
spdx=LGPL-2.1-or-later OR MPL-1.1
without=lzo (GPL-2.0-or-later)
options=meson $CAIRO_OPTIONS, CFLAGS $CFLAGS $CAIRO_CFLAGS, MSYS2's patches 0001-DWrite-Get-glyph-bitmap-with-D2D-in-selected-cases.patch and 673.patch
source=$SOURCES/mingw-w64-cairo-$CAIRO.src.tar.zst
sha256=$CAIRO_SHA256
recipe=scripts/win_libs.sh in https://github.com/avdrav1/cronkite"
if built cairo "$record"; then
  echo "win_libs: cairo $CAIRO without lzo in $prefix/cairo (built before)"
else
  fetch_source cairo "$CAIRO" "$CAIRO_SHA256"
  s=$work/mingw-w64-cairo
  tar -xf "$s/cairo-${CAIRO%-*}.tar.xz" -C "$s"
  src=$s/cairo-${CAIRO%-*}
  (cd "$src" && patch -p1 -s -i ../0001-DWrite-Get-glyph-bitmap-with-D2D-in-selected-cases.patch && patch -p1 -s -i ../673.patch)
  rm -rf "$prefix/cairo"
  # shellcheck disable=SC2086 # the options are words
  CFLAGS="$CFLAGS $CAIRO_CFLAGS" CXXFLAGS="$CXXFLAGS $CAIRO_CFLAGS" MSYS2_ARG_CONV_EXCL="--prefix=" \
    meson setup "$work/cairo-build" "$src" --prefix="$prefix/cairo" $CAIRO_OPTIONS >"$work/setup.log" 2>&1 ||
    { cat "$work/setup.log"; exit 1; }
  meson compile -C "$work/cairo-build" >"$work/compile.log" 2>&1 || { tail -50 "$work/compile.log"; exit 1; }
  meson install -C "$work/cairo-build" >"$work/install.log" 2>&1 || { cat "$work/install.log"; exit 1; }
  for dll in libcairo-2.dll libcairo-gobject-2.dll libcairo-script-interpreter-2.dll; do
    [[ -f $prefix/cairo/bin/$dll ]] || { echo "win_libs: the cairo build made no $dll"; exit 1; }
    links_none "$prefix/cairo/bin/$dll" lzo
  done
  mkdir -p "$prefix/cairo/share/licenses/cairo"
  cp "$src"/COPYING* "$prefix/cairo/share/licenses/cairo/"
  printf '%s\n' "$record" >"$prefix/cairo/cronkite-build.txt"
  echo "win_libs: built cairo $CAIRO without lzo into $prefix/cairo"
fi

# libtiff: mingw-w64-libtiff's PKGBUILD's configure options (its shared build), then ours.
# Its prepare() also runs autoreconf, for clang's libtool; the release's own configure
# serves gcc.
TIFF=4.7.2-2
TIFF_SHA256=a4d55d313764640aaa2ba7b4b377af855877d31fe6fa040d63dc3647cc45622d
TIFF_OPTIONS="--build=x86_64-w64-mingw32 --host=x86_64-w64-mingw32 --disable-static --enable-shared --enable-cxx --enable-lerc --enable-libdeflate --enable-webp --disable-docs --disable-jbig"
TIFF_CFLAGS="-fno-strict-aliasing"
record="package=mingw-w64-ucrt-x86_64-libtiff
version=$TIFF
spdx=libtiff
without=jbig (jbigkit, GPL-2.0)
options=configure $TIFF_OPTIONS, CFLAGS $CFLAGS $TIFF_CFLAGS, MSYS2's patch 0002-libtiff-install-headers.patch
source=$SOURCES/mingw-w64-libtiff-$TIFF.src.tar.zst
sha256=$TIFF_SHA256
recipe=scripts/win_libs.sh in https://github.com/avdrav1/cronkite"
if built libtiff "$record"; then
  echo "win_libs: libtiff $TIFF without jbig in $prefix/libtiff (built before)"
else
  fetch_source libtiff "$TIFF" "$TIFF_SHA256"
  s=$work/mingw-w64-libtiff
  tar -xzf "$s/tiff-${TIFF%-*}.tar.gz" -C "$s"
  src=$s/tiff-${TIFF%-*}
  (cd "$src" && patch -p1 -s -i ../0002-libtiff-install-headers.patch)
  rm -rf "$prefix/libtiff"
  mkdir -p "$work/tiff-build"
  # shellcheck disable=SC2086 # the options are words
  (cd "$work/tiff-build" && CFLAGS="$CFLAGS $TIFF_CFLAGS" CXXFLAGS="$CXXFLAGS $TIFF_CFLAGS" \
    "$src/configure" --prefix="$prefix/libtiff" $TIFF_OPTIONS >"$work/configure.log" 2>&1) ||
    { tail -50 "$work/configure.log"; exit 1; }
  make -C "$work/tiff-build/libtiff" -j"$(nproc)" install >"$work/make.log" 2>&1 || { tail -50 "$work/make.log"; exit 1; }
  [[ -f $prefix/libtiff/bin/libtiff-6.dll ]] || { echo "win_libs: the libtiff build made no libtiff-6.dll"; exit 1; }
  links_none "$prefix/libtiff/bin/libtiff-6.dll" jbig
  mkdir -p "$prefix/libtiff/share/licenses/libtiff"
  cp "$src/LICENSE.md" "$prefix/libtiff/share/licenses/libtiff/"
  printf '%s\n' "$record" >"$prefix/libtiff/cronkite-build.txt"
  echo "win_libs: built libtiff $TIFF without jbig into $prefix/libtiff"
fi
