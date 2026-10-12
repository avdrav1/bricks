# Cronkite: demo and explainer video script

About 3 minutes at a relaxed pace, about 420 words of voiceover. Screen recordings on a
GNOME desktop at 1920×1080. Each claim below has its source in brackets. Cut those notes
before recording.

**Tone:** calm, plain, a little dry. Show the apps working rather than describing them.

---

## 0:00 – Cold open (10 s)

| Picture | Voiceover |
| --- | --- |
| A terminal. The cursor blinks next to `bricks big.csv` (1 GB). Enter. The grid is full of rows almost at once. Hard cut to black. | *(none; the keyboard click and then silence)* |
| Title card: **Cronkite**. Beneath it: *A fast, native office suite for Linux.* | "That was a one-gigabyte spreadsheet, open in about a third of a second." |

[BENCHMARKS.md: 1 GB CSV open to first rows visible, 314 ms]

## 0:10 – What Cronkite is (20 s)

| Picture | Voiceover |
| --- | --- |
| Three app icons slide in side by side: Bricks, Slate, Frame. | "Cronkite is an office suite for Linux, built from scratch in Rust. It has three apps: Bricks for spreadsheets, Slate for documents, and Frame for presentations." |
| The icons sit in the GNOME app grid next to Files and Settings. | "They're native apps. No browser tab, no subscription, no account. Each one installs and runs on its own, so you take only what you need." |

## 0:30 – Bricks (55 s)

| Picture | Voiceover |
| --- | --- |
| Bricks with the 1 GB CSV: 19 million rows. Drag the scrollbar to the bottom; it's instantly at row 19,100,000. Scroll fast. | "Bricks is the spreadsheet. It's built for the files other spreadsheets give up on. This one has nineteen million rows, and scrolling stays smooth." |
| Click a numeric column header, then Sort. Done before the narrator finishes. | "Sorting all nineteen million rows takes under a second." |
| Ctrl+F, type `portland`. The match count streams in. | "Search runs in the background, so the window never freezes." |
| Zoom in on a cell holding `00123`. Edit another cell, press Ctrl+S, and the title bar's • clears. | "Your data stays your data. `00123` stays `00123`. Bricks never quietly turns it into a number. And every save is atomic: a crash can't leave you with half a file." |
| Open `budget.xlsx`: several sheet tabs, bold headers, fills, and `=SUM` formulas. Change a value; the totals update. | "It opens Excel workbooks too, with formulas, multiple sheets, and cell styles." |
| The same workbook open in LibreOffice: the chart and comments are still there. | "When you save, Bricks patches the original file rather than rewriting it, so charts, comments, and conditional formatting come through untouched." |

[BENCHMARKS.md: sort 0.82 s for 19.1M rows; find 224 ms; atomic save is CLAUDE.md invariant 6; XLSX patching is gate M5]

## 1:25 – Slate (50 s)

| Picture | Voiceover |
| --- | --- |
| Double-click `report.docx` in Files; Slate opens. Headings, a bulleted list, a table, a picture: one long column with no page edges. | "Slate is the word processor. You'll notice there are no pages. Slate gives you one endless page, so you write instead of fighting margins." |
| View → Text Width: Narrow, then Wide, then Full. The text reflows smoothly each time. | "Choose how wide you want the column. That's a setting for you; it doesn't change the document." |
| Type a sentence, press Ctrl+B and Ctrl+1, then undo and redo. | "Typing, headings, lists, tables, and copy and paste all work as you'd expect, with full undo." |
| Ctrl+S. Then open the same file in LibreOffice Writer: the edit is there, and so are the header, the footer, and the page layout Slate never showed. | "Here's the part we care most about. When Slate saves a Word document, it changes only what you edited. Everything else in the file, the parts Slate doesn't even show you, stays exactly as it was." |
| Open `notes.md`: it shows formatted, not as source. Edit one paragraph and save. A terminal `git diff` shows one changed line. | "Markdown and plain text too. Edit one paragraph of a Markdown file, and only that paragraph changes on disk. Your version control will thank you." |

[docs/specs/slate.md: pageless column, text widths; SLATE-8: LibreOffice reopens an edited .docx with the edits and everything else unchanged; SLATE-7: an edited .md changes only the edited blocks]

## 2:15 – Frame (35 s)

| Picture | Voiceover |
| --- | --- |
| Frame with a PowerPoint deck: slide list on the left, the slide in the middle, the inspector on the right. Click a title and type into it. | "Frame is for presentations, and it's coming next. It opens PowerPoint decks, and you edit right on the slide." |
| Drag a shape: smart guides snap it to the centre. Change the fill in the inspector. | "Shapes snap into line as you drag them. Every formatting option lives in one inspector. There are no dialogs to dig through." |
| Switch to the outline view and type a title, Enter, Tab, three bullets: new slides appear as you type. | "Or skip the slides entirely and just type an outline. Frame builds the slides for you." |
| On-screen text, with the narration: **15 actions in Frame vs 21 in PowerPoint**, under the label *the same four-slide deck*. Press F5: the deck plays full screen. | "In our test, building the same four-slide deck took about a quarter fewer clicks and keystrokes than PowerPoint." |

[docs/specs/frame.md; FRAME-15 notes: 15 actions on the canvas, 19 in the outline, PowerPoint's 21]

## 2:50 – Install (20 s)

| Picture | Voiceover |
| --- | --- |
| The browser at cronkite.cc. Click Copy next to the Slate command. | "Getting started takes one command per app." |
| A terminal: paste `curl -fsSL https://slate.cronkite.cc/install.sh \| sh`. It prints *Installed Slate 0.1.0*. | "It installs for your user only. No sudo, no package manager, and the download is checked before anything is installed." |
| Double-click a `.docx` in Files; Slate opens it. | "After that, your files just open in the right app." |

[sites/cronkite/index.html; SLATE-9 acceptance]

## 3:10 – Close (10 s)

| Picture | Voiceover |
| --- | --- |
| The three icons, then the URL **cronkite.cc**. Beneath it: *Bricks and Slate available now · Frame coming soon · Open source*. | "Cronkite. Fast, native, and open source. Bricks and Slate are available today, and Frame is coming soon, at cronkite.cc." |

---

## Production notes

- **Machine:** record the Bricks timings on the reference box (8-core, NVMe; see `apps/bricks/docs/BENCHMARKS.md`). Show them in real time; never speed them up.
- **Files you need:**
  - `big.csv` (`python3 scripts/gen_corpus.py`, the 1 GB file);
  - a styled `.xlsx` with a chart and comments;
  - a `.docx` with a header and footer, a table, and a picture (the Slate fixture works: `apps/slate/crates/docx/tests/data/fixture.docx`);
  - a short `notes.md` inside a git repository;
  - a `.pptx` with a few slides.
- **Frame:** Frame isn't released yet. Record it from a source build (`cargo run --release -p frame`) and keep the "coming next" wording until it ships. If Frame ships before the video is cut, change the close to "available today."
- **Claims to keep accurate:**
  - "about a third of a second" means 314 ms on the reference box;
  - "under a second" means 0.82 s for 19.1M rows;
  - "a quarter fewer" means 15 vs 21 actions;
  - "only what you edited" is the patched save. Inside an edited paragraph, bookmarks, comments, and fields don't survive, so don't demo the edit on a paragraph that has them.
- **Captions:** burn them in. Many viewers watch muted.
