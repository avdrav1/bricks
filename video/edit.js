// The edit: the order the scenes play in, following docs/video/explainer-script.md.
// Each scene's narration (its captions) and timing live with the scene.
// Left out: the crash recovery shot (SLATE-17 hasn't shipped), per the script's
// production notes. Unlike the script, the video opens on the title card, where
// the music starts, and the cold open follows it.
V.order([
  // 0:00 Title, then the cold open
  "title", "cold-open",
  // What Cronkite is
  "suite-icons", "app-grid",
  // Brick
  "brick-scroll", "brick-sort", "brick-find", "brick-raw", "brick-past-data", "brick-xlsx", "brick-libreoffice",
  // Slate
  "slate-open", "slate-width", "slate-typing", "slate-format", "slate-find", "slate-new", "slate-docx-save", "slate-markdown",
  // Frame
  "frame-edit", "frame-picture", "frame-shapes", "frame-outline", "frame-count",
  // Install
  "install-site", "install-terminal", "install-open",
  // Close
  "close",
]);
