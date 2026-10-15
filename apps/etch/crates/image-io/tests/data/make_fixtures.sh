#!/bin/sh
# Makes image-io's test fixtures and their references with ImageMagick 7 (`magick`), from
# this directory: `sh make_fixtures.sh`. Each FILE gets FILE.rgba, the pixels ImageMagick
# decodes from it (libpng, libjpeg-turbo, its own BMP reader), as raw RGBA8 rows top to
# bottom: the reference image-io's decoding is checked against, pixel by pixel.
set -eu
cd "$(dirname "$0")"

# 16×12, RGBA: colour gradients, the left four columns fully transparent, the right four
# opaque, partial alpha between.
magick -size 16x12 xc:none -alpha set \
    -channel R -fx 'i/(w-1)' \
    -channel G -fx 'j/(h-1)' \
    -channel B -fx '((i+j)%3)/2' \
    -channel A -fx 'i<4 ? 0 : (i>=12 ? 1 : (i-3)/9)' \
    +channel -strip PNG32:alpha.png

# 20×16 colour photo stand-in, quality 90: chroma not subsampled (photo.jpg) and
# subsampled 2×2 as cameras and most encoders do (photo420.jpg).
magick -size 20x16 xc: \
    -channel R -fx 'i/(w-1)' \
    -channel G -fx '0.5+0.5*sin(j/2)' \
    -channel B -fx '((i*j)%7)/6' \
    +channel -strip -quality 90 -sampling-factor 1x1 photo.jpg
magick photo.jpg -strip -quality 90 -sampling-factor 2x2 photo420.jpg

# 24-bit BMP (BITMAPINFOHEADER), 13×7: an odd width, so its rows carry padding.
magick -size 13x7 xc: \
    -channel R -fx 'i/(w-1)' \
    -channel G -fx '1-j/(h-1)' \
    -channel B -fx '((i+2*j)%5)/4' \
    +channel -strip -type TrueColor BMP3:rgb24.bmp

# 32-bit BMP with an alpha mask (BITMAPV5HEADER). Etch opens every BMP opaque, so its
# reference is the colour with alpha off.
magick alpha.png -resize 10x8! -strip -define bmp:format=bmp4 BMP:alpha32.bmp

# A GIF: a format Etch doesn't open.
magick -size 4x4 xc:red tiny.gif

for f in alpha.png photo.jpg photo420.jpg rgb24.bmp; do
    magick "$f" -depth 8 "rgba:$f.rgba"
done
magick alpha32.bmp -alpha off -depth 8 rgba:alpha32.bmp.rgba
