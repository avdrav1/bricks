#!/bin/sh
# Makes the resize fixture and its references with ImageMagick 7 (`magick`, Q16-HDRI),
# from this directory: `sh make_references.sh`. Raw RGBA8 rows, top to bottom, straight
# alpha, no header: what tests/resize.rs reads.
#
# source.rgba, 48×36: colour gradients and stripes, the left eight columns fully
# transparent hiding bright green (so a filter that doesn't weight colour by alpha shows
# green at the edge), partial alpha rising to opaque across the middle, a hard-edged
# opaque block.
#
# Each reference is ImageMagick's Triangle filter (a tent one source pixel wide, widened
# to the shrink factor when shrinking), which weights each pixel's colour by its alpha
# and divides it back out (premultiplied filtering), in floating point:
#   magick -size 48x36 -depth 8 rgba:source.rgba -filter Triangle -resize WxH! -depth 8 rgba:OUT
set -eu
cd "$(dirname "$0")"

magick -size 48x36 xc:none -alpha set \
    -channel R -fx 'i < 8 ? 0 : ((i/6 + j/4) % 2 == 0 ? 0.95 : i/(w-1))' \
    -channel G -fx 'i < 8 ? 1 : 0.5 + 0.5*sin(j/3)' \
    -channel B -fx 'i < 8 ? 0 : (i >= 30 && i < 40 && j >= 10 && j < 22 ? 1 : ((i*j) % 7)/6)' \
    -channel A -fx 'i < 8 ? 0 : (i >= 24 ? 1 : (i-7)/17)' \
    +channel -depth 8 rgba:source.rgba

resize() {
    magick -size 48x36 -depth 8 rgba:source.rgba -filter Triangle -resize "$1!" -depth 8 "rgba:$2"
}
# Shrinking (by 48/19 and 36/13, not whole numbers), enlarging (by 125/48 and 97/36), and
# enlarging one way while shrinking the other.
resize 19x13 down.rgba
resize 125x97 up.rgba
resize 100x20 wide.rgba
