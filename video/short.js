// The short cut: under a minute, from the same scenes. Play it with
// ?cut=short; render it with `node render.mjs --cut short`. Each clip is a
// window of a scene, in that scene's own seconds, under a line of its own.
V.cut("short", [
  { scene: "title", from: 0, to: 2.6, xfade: 0 },
  { scene: "cold-open", from: 1.2, to: 7.8, lead: 2.0, say: "A one-gigabyte spreadsheet, open in a third of a second." },
  { scene: "brick-sort", from: 1.0, to: 5.4, lead: 0.3, say: "Brick sorts nineteen million rows in under a second." },
  { scene: "slate-open", from: 0.6, to: 6.0, say: "Slate is a word processor with no pages, and Word documents look like Word." },
  { scene: "slate-format", from: 1.6, to: 7.4, say: "A slim format bar, not a ribbon." },
  { scene: "frame-shapes", from: 1.0, to: 8.6, say: "Frame edits PowerPoint decks right on the slide, with smart guides and one inspector." },
  { scene: "frame-outline", from: 1.0, to: 6.6, say: "Or just type an outline." },
  { scene: "install-terminal", from: 0.3, to: 5.0, say: "Each app installs with one command. No sudo, no account." },
  { scene: "close", from: 1.0, to: 12.16, say: "Cronkite. Fast, native, and open source. Get it at cronkite.cc." },
]);
