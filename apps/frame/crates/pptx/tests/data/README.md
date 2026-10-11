# PPTX test decks

Each `NAME.pptx` has `NAME-libreoffice.txt`: LibreOffice's reading of it, written by
`python3 scripts/lo_pptx.py dump NAME.pptx` (format in that script's docstring). Tests
compare Frame's reading with these dumps, so they need no LibreOffice.

- `fixture.pptx`: built by `python3 scripts/lo_pptx.py fixture fixture.pptx` (Impress,
  "Impress MS PowerPoint 2007 XML", then patched for theme colour transforms, a frameless
  title, and a scaled and rotated group).
- PowerPoint-authored samples from Apache POI's test data,
  <https://github.com/apache/poi/tree/trunk/test-data/slideshow>, licensed under the
  Apache License 2.0 (<https://www.apache.org/licenses/LICENSE-2.0>):
  - `shapes.pptx`: <https://github.com/apache/poi/blob/trunk/test-data/slideshow/shapes.pptx>
  - `layouts.pptx`: <https://github.com/apache/poi/blob/trunk/test-data/slideshow/layouts.pptx>
  - `SampleShow.pptx`: <https://github.com/apache/poi/blob/trunk/test-data/slideshow/SampleShow.pptx>
  - `themes.pptx`: <https://github.com/apache/poi/blob/trunk/test-data/slideshow/themes.pptx>
  - `backgrounds.pptx`: <https://github.com/apache/poi/blob/trunk/test-data/slideshow/backgrounds.pptx>
  - `bar-chart.pptx`: <https://github.com/apache/poi/blob/trunk/test-data/slideshow/bar-chart.pptx>

To re-record a dump: `python3 scripts/lo_pptx.py dump apps/frame/crates/pptx/tests/data/NAME.pptx > apps/frame/crates/pptx/tests/data/NAME-libreoffice.txt`.
