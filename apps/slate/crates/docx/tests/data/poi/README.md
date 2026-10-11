# Word-authored samples

These `.docx` files come unchanged from Apache POI's test data,
https://github.com/apache/poi/tree/trunk/test-data/document (branch `trunk`, fetched
2026-10-11), and are used under the Apache License 2.0
(https://www.apache.org/licenses/LICENSE-2.0). Copyright The Apache Software Foundation.

| File | Source | What it exercises |
| --- | --- | --- |
| `ComplexNumberedLists.docx` | https://github.com/apache/poi/blob/trunk/test-data/document/ComplexNumberedLists.docx | numbered lists: nesting, restarts, a list continued after normal text |
| `Numbering.docx` | https://github.com/apache/poi/blob/trunk/test-data/document/Numbering.docx | bullets on four levels, letter, roman, legal (`1.1.1.`), and custom labels |
| `IllustrativeCases.docx` | https://github.com/apache/poi/blob/trunk/test-data/document/IllustrativeCases.docx | a real Word report: Title style, many tables, line breaks in cells |
| `WithGIF.docx` | https://github.com/apache/poi/blob/trunk/test-data/document/WithGIF.docx | an inline GIF picture |
| `SampleDoc.docx` | https://github.com/apache/poi/blob/trunk/test-data/document/SampleDoc.docx | a page break in a paragraph of its own, fonts and colours |
| `footnotes.docx` | https://github.com/apache/poi/blob/trunk/test-data/document/footnotes.docx | a footnote reference mark |

Each `X-libreoffice.txt` is LibreOffice's reading of `X.docx`:
`python3 scripts/lo_docx.py dump X.docx > X-libreoffice.txt`.
