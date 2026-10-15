# Cronkite: demo and explainer video script

About 4½ minutes at a relaxed pace, about 650 words of voiceover. Screen recordings on a
GNOME desktop at 1920×1080. Each claim below has its source in brackets. Cut those notes
before recording.

**Tone:** calm, plain, a little dry. Show the apps working rather than describing them.

---

## 0:00 – Title and cold open (15 s)

| Picture | Voiceover |
| --- | --- |
| Title card: **Cronkite**. Beneath it: *A fast, native office suite for Linux.* The music starts on it. | *(none)* |
| A terminal. The cursor blinks next to `brick big.csv` (1 GB). Enter. The grid is full of rows almost at once; then *1 GB CSV open in 314 ms*. | "That was a one-gigabyte spreadsheet, open in about a third of a second." |

[BENCHMARKS.md: 1 GB CSV open to first rows visible, 314 ms]

## 0:10 – What Cronkite is (20 s)

| Picture | Voiceover |
| --- | --- |
| Four app icons slide in side by side: Brick, Slate, Frame, Etch. | "Cronkite is an office suite for Linux, built from scratch in Rust. It has four apps: Brick for spreadsheets, Slate for documents, Frame for presentations, and Etch for images." |
| The icons sit in the GNOME app grid next to Files and Settings. | "They're native apps. No browser tab, no subscription, no account. Each one installs and runs on its own, so you take only what you need." |

## 0:30 – Brick (60 s)

| Picture | Voiceover |
| --- | --- |
| Brick with the 1 GB CSV: 19 million rows. Drag the scrollbar to the bottom; it's instantly at row 19,100,000. Scroll fast. | "Brick is the spreadsheet. It's built for the files other spreadsheets give up on. This one has nineteen million rows, and scrolling stays smooth." |
| Click a numeric column header, then Sort. Done before the narrator finishes. | "Sorting all nineteen million rows takes under a second." |
| Ctrl+F, type `portland`. The match count streams in. | "Search runs in the background, so the window never freezes." |
| Zoom in on a cell holding `00123`. Edit another cell, press Ctrl+S, and the title bar's • clears. | "Your data stays your data. `00123` stays `00123`. Brick never quietly turns it into a number. And every save is atomic: a crash can't leave you with half a file." |
| A three-row CSV: empty cells fill the window past the data. Click E40 and type; Ctrl+Z, and the rows are gone again. | "Past your data, the sheet keeps going, like Excel and Calc. Type anywhere, and the rows are there." |
| Open `budget.xlsx`: several sheet tabs, bold headers, fills, and `=SUM` formulas. Change a value; the totals update. | "It opens Excel workbooks too, with formulas, multiple sheets, and cell styles." |
| The same workbook open in LibreOffice: the chart and comments are still there. | "When you save, Brick patches the original file rather than rewriting it, so charts, comments, and conditional formatting come through untouched." |

[BENCHMARKS.md: sort 0.82 s for 19.1M rows; find 224 ms; atomic save is CLAUDE.md invariant 6; empty cells past the data are GRID-6 (v0.3.0); XLSX patching is gate M5]

## 1:30 – Slate (80 s)

| Picture | Voiceover |
| --- | --- |
| Double-click `report.docx` in Files; Slate opens. Headings, a bulleted list, a table, a picture: one long column with no page edges. The body is in Cambria and the headings in Calibri, as in Word. | "Slate is the word processor. You'll notice there are no pages. Slate gives you one endless page, so you write instead of fighting margins. And Word documents look like themselves, in their own fonts and styles." |
| A Word letter or invoice: an embedded font in its heading, a shaded heading with a border under it, tab stops with dot leaders and a right-aligned date, a paragraph with its own indents, Word's ● bullets. The column is the document's own page width. | "Word layouts come through too: the page's own width, tab stops with leaders, borders and shading, embedded fonts, and Word's own list markers." |
| View → Text Width, which starts at Page (the document's page width): Narrow, then Wide, then Full. The text reflows smoothly each time. | "Choose how wide you want the column. That's a setting for you; it doesn't change the document." |
| Type a sentence, press Ctrl+B and Ctrl+1, then undo and redo. | "Typing, headings, lists, tables, and copy and paste all work as you'd expect, with full undo." |
| Select a paragraph. The format bar over the page: open the style picker (each style drawn in its own look) and pick Quote; choose a font, a size, and a colour; Ctrl+E centres it. | "When you want more, a slim format bar holds the document's own styles, fonts, sizes, and colours. No ribbon." |
| Ctrl+F, type `budget`: matches highlight through the page and inside the table, with a count. Ctrl+H, type `forecast`, Replace All. Ctrl+Z puts every one back. | "Find and replace reaches into tables, and Replace All undoes in one step." |
| Ctrl+N: an Untitled window. Type a heading and a line; Ctrl+S suggests `Untitled.docx` in Documents. | "Ctrl+N starts a new Word document." |
| Unsaved edits in a window. A terminal: `kill -9` Slate. Launch it again: it offers the edits back, and the title shows •. | "And if Slate ever crashes, your unsaved edits are waiting when you come back. Brick does the same." |
| Ctrl+S. Then open the same file in LibreOffice Writer: the edit is there, and so are the header, the footer, and the page layout Slate never showed. | "Here's the part we care most about. When Slate saves a Word document, it changes only what you edited. Everything else in the file, the parts Slate doesn't even show you, stays exactly as it was." |
| Open `notes.md`: it shows formatted, not as source. Edit one paragraph and save. A terminal `git diff` shows one changed line. | "Markdown and plain text too. Edit one paragraph of a Markdown file, and only that paragraph changes on disk. Your version control will thank you." |

[docs/specs/slate.md: pageless column, text widths; SL3 fonts, styles, format bar (SLATE-19 to SLATE-22, ADR 0008); SL4 Word layouts: page width, tab stops, embedded fonts, a paragraph's own spacing and indents, borders and shading, list markers (SLATE-23 to SLATE-28, ADR 0009, Slate v0.3.0); SLATE-16 find and replace; SLATE-18 new document (Slate v0.1.1); SLATE-17 crash recovery; Brick's recovery is APP-7; SLATE-8: LibreOffice reopens an edited .docx with the edits and everything else unchanged; SLATE-7: an edited .md changes only the edited blocks]

## 2:50 – Frame (50 s)

| Picture | Voiceover |
| --- | --- |
| Frame with a PowerPoint deck: slide list on the left, the slide in the middle, the inspector on the right. Click a title and type into it. Then Ctrl+N: a new 16:9 deck with a title slide. | "Frame is for presentations. It opens PowerPoint decks, or starts a new one, and you edit right on the slide." |
| Drag a photo from Files onto a slide: it fills the empty picture placeholder. Keep typing bullets until the text box is full; the text shrinks to fit. | "Drop in a picture, and it lands where the layout wants it. Too much text shrinks to fit." |
| Drag a shape: smart guides snap it to the centre. Change the fill in the inspector. | "Shapes snap into line as you drag them. Every formatting option lives in one inspector. There are no dialogs to dig through." |
| Switch to the outline view and type a title, Enter, Tab, three bullets: new slides appear as you type. | "Or skip the slides entirely and just type an outline. Frame builds the slides for you." |
| On-screen text, with the narration: **15 actions in Frame vs 21 in PowerPoint**, under the label *the same four-slide deck*. Ctrl+S, then press F5: the deck plays full screen. | "In our test, building the same four-slide deck took about a quarter fewer clicks and keystrokes than PowerPoint. And like Slate, saving changes only what you edited." |

[docs/specs/frame.md; FRAME-14 new deck; FRAME-12 pictures into placeholders and shrink to fit; FRAME-15 notes: 15 actions on the canvas, 19 in the outline, PowerPoint's 21; FRAME-8 patched save]

## 3:40 – Etch (40 s)

| Picture | Voiceover |
| --- | --- |
| Etch with a blank `poster.png` (1024×768): the menu bar (File, Edit, View, Image), the tool column, the canvas, and the colour row with the palette. Alt+H, type `#10392E`, Enter; press F and click the image: it fills deep green. Alt+H, `#F2B134`; press O, choose Filled, and drag an amber sun off the top right corner. Press L, Alt+S, 4, and drag a thin road across the bottom. Alt+H, `#F4F1EA`; press O, choose Outline, Alt+S, 14, and drag the bike's two wheels onto the road. | "Etch is for images. It's closer to Paint than to Photoshop: a fill, lines and shapes, any colour by its hex code, and Paint's own keys." |
| The bike now has its frame, saddle, and handlebar. Press T, open the font list and pick Adwaita Sans, press Bold, click the top left, and type *Spring*, Shift+Enter, *Ride*: it shows in its dashed frame, 128 px, as it will land. Enter. Press R (still Filled), Alt+H, `#E4572E`, and drag the coral tag under the headline: the rectangle follows the pointer until the button comes up. | "Type text in any font you have. Shapes show exactly where they'll land while you drag them." |
| The finished poster: the date and place in amber, *Free entry* on the tag, a quiet line on the road. Press S and drag a box around the tag: *Selection: 162×60*. Ctrl+C, Ctrl+V: the copy floats at the image's top-left; drag it beside the tag, then Ctrl+Z drops it. Image → Rotate 90° Right: the poster turns, 768×1024. Ctrl+Z turns it back. | "Select part of a picture to move it, or copy and paste it. Crop, resize, rotate, and flip are in the Image menu, and every step can be undone." |
| Double-click `mountain.png` (7680×4320) in Files: Etch says *Opening…*, then shows it fitted. On-screen: *8K PNG open in 659 ms*. Ctrl+scroll over a ridge, notch by notch to 3200%: the pixels are sharp squares. | "And it stays quick with big pictures: an 8K photo opens in under a second. Zoom all the way in, and every pixel is a crisp square." |

[docs/specs/etch.md (Principles, Tools, Shortcuts); ETCH-9 and ETCH-14 fill with tolerance, ETCH-11 shapes previewed while dragged (outline, filled, or both), ETCH-12 text in every installed font through the input method, Enter puts it on the image; the hex field (Alt+H) and the size (Alt+S); ETCH-15 selection, ETCH-16 cut, copy, and paste (paste floats at the visible image's top-left; undo while pixels float drops them); ETCH-17 to ETCH-20 crop, resize image and canvas, rotate, flip, each one undo step; ETCH-24 Paint's keys; ETCH-6: an 8K photo-like PNG (7680×4320) shows in 659 ms, median of 3, on a Ryzen 9 5900 (target 1 s); ET0: zoom 10% to 3200%, pixels square from 100%. The poster is cronkite.cc's, painted in Etch by `scripts/etch_poster.py`.]

## 4:20 – Install (20 s)

| Picture | Voiceover |
| --- | --- |
| The browser at cronkite.cc: four install commands, one per app. Click Copy next to the Slate command. | "Getting started takes one command per app." |
| A terminal: paste `curl -fsSL https://slate.cronkite.cc/install.sh \| sh`. It prints *Installed Slate 0.3.0*. | "It installs for your user only. No sudo, no package manager, and the download is checked before anything is installed." |
| Double-click a `.docx` in Files; Slate opens it. Then a `.pptx`; Frame opens it. Then a `.png`; Etch opens it. | "After that, your files just open in the right app." |

[sites/cronkite/index.html; SLATE-9, FRAME-9, and ETCH-6 acceptance: installing makes each app the default for its files]

## 4:40 – Close (10 s)

| Picture | Voiceover |
| --- | --- |
| The four icons, then the URL **cronkite.cc**. Beneath it: *Brick, Slate, Frame, and Etch · Available now · Open source*. | "Cronkite. Fast, native, and open source. Brick, Slate, Frame, and Etch are all available today, at cronkite.cc." |

---

## Production notes

- **Machine:** record the Brick timings on the reference box (8-core, NVMe; see `apps/brick/docs/BENCHMARKS.md`), and Etch's 8K open on a machine like the Ryzen 9 5900 it was measured on. Show them in real time; never speed them up.
- **Files you need:**
  - `big.csv` (`python3 scripts/gen_corpus.py`, the 1 GB file);
  - a three-row `.csv` for the empty cells past the data;
  - a styled `.xlsx` with a chart and comments;
  - a `.docx` letter or invoice with an embedded font, a shaded and bordered heading, tab stops with leaders, and Word bullets;
  - a `.docx` with a header and footer, a table, and a picture, its body in Cambria and its headings in Calibri, with the word `budget` in a paragraph and in the table (start from the Slate fixture, `apps/slate/crates/docx/tests/data/fixture.docx`);
  - a short `notes.md` inside a git repository;
  - a `.pptx` with a few slides, one of them with an empty picture placeholder, and a photo to drop on it;
  - `poster.png`, 1024×768, blank, to paint the poster of `scripts/etch_poster.py` on (the steps above; the bike's frame and the date, the quiet line, and the tag's words between the shots);
  - `mountain.png`, a 7680×4320 photo.
- **Versions:** Brick v0.3.0, Slate v0.3.0, Frame v0.1.0, and Etch v0.2.0 are released. Make the installer shot print the version that's current on the day.
- **Fonts:** the Word-like look needs the metric-compatible fonts installed (Carlito for Calibri, Caladea for Cambria; `docs/specs/slate.md` lists the packages). Install them on the recording machine.
- **Claims to keep accurate:**
  - "about a third of a second" means 314 ms on the reference box;
  - Brick's "under a second" means 0.82 s for 19.1M rows;
  - "a quarter fewer" means 15 vs 21 actions;
  - "only what you edited" is the patched save. Inside an edited paragraph, bookmarks, comments, and fields don't survive, so don't demo the edit on a paragraph that has them;
  - Etch's "under a second" means an 8K PNG shown 659 ms after starting (ETCH-6, a Ryzen 9 5900; the spec's target is a second).
- **Captions:** burn them in. Many viewers watch muted.
- **The web-made video** (`video/`, `node render.mjs`) follows this script with the apps recreated in HTML, avdrav's song "Buildings" as the soundtrack (the long cut trims its main section so its outro ends with the picture; the shorts start on its drop), and a voiceover by Kokoro's `af_heart` voice: Kokoro-82M, an Apache-2.0 model run locally (kokoro-js), so the narration can be published. `--voice` picks another: Kokoro's `bm_george` and the like, or, for drafts only, Microsoft's voices through the read-aloud service built into Edge, which isn't licensed for publishing. Its narration is these lines, verbatim; it runs 6:23. Its short cut (`video/short.js`, `node render.mjs --cut short`) runs 55 s: windows of the same shots under eight shorter lines of its own, Etch's among them. Its Hindi version (`--cut short-hi`, 60 s) has the same shots but Frame's outline in place of Etch's, with the lines in Hindi, narrated by Microsoft's `hi-IN-MadhurNeural` through Edge (before it's published, use an Azure Speech key for that voice or a recorded narrator), and Hindi captions and on-screen text; the apps' interfaces stay in English. Have a native speaker check the Hindi before it's published.
