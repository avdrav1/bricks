// 1:30 Slate. A faithful mockup of Slate's GTK 4 window (apps/slate/src: main.rs,
// format_bar.rs, find_bar.rs, saving.rs, view.rs): header bar with Open and Menu, the
// find bar, the format bar, the white pageless column, and the status line. The
// document, report.docx, is a Word document: headings in Calibri (Carlito), body in
// Cambria (Caladea), as Slate draws it with the metric-compatible fonts.
"use strict";

(() => {
  const { icon, btn, textBtn } = V.ui;
  const Z = 1.25; // the desktop's scale
  // settings.rs: Page (the default) is a Word document's page width less its margins,
  // then Narrow 480, Medium 624, Wide 816, Full = width - 2×24. report.docx is Letter
  // with 1″ margins, so Page gives it 624 px.
  const PAGE = 624;
  const TEXT_WIDTHS = ["Page", "Narrow", "Medium", "Wide", "Full"];
  const WIDTHS = { Page: PAGE, Narrow: 480, Medium: 624, Wide: 816 };
  // Where the main window sits: about 1600 px wide, clear of the captions' band.
  const SW = { x: 160, y: 62, w: 1600, h: 880 };

  // ---- report.docx ---------------------------------------------------------
  // Seven "budget"s: four in the body, two in the table, one in the caption.
  const REPORT = [
    { k: "title", h: "Field Office Report" },
    { k: "subtitle", h: "Third quarter 2026 · Northfield Community Trust" },
    { k: "h1", h: "Summary" },
    { k: "p", h: "The third quarter closed ahead of plan. Field visits rose by a fifth, and the new intake process cut the wait for a first appointment from nine days to four. We stayed inside the budget for the quarter while opening two new sites." },
    { k: "p", h: "This report covers what changed, what it cost, and what we plan for the fourth quarter. The figures come from the regional returns filed by 5 October." },
    { k: "h2", h: "Highlights" },
    { k: "li", h: "Two new sites opened, in Ashford and in Leigh." },
    { k: "li", h: "The average wait for a first visit fell to four days." },
    { k: "li", h: "Volunteer hours rose 18% on the second quarter." },
    { k: "li", h: "Travel came in 6% under budget." },
    { k: "p", h: "Our thanks go to the volunteers who gave up their evenings to get both new sites ready on time." },
    { k: "h1", h: "Spending" },
    { k: "p", h: "Spending by area is set out below. Staff costs rose with the new sites; everything else held close to the budget agreed in March." },
    {
      k: "table",
      rows: [
        ["Area", "Budget", "Spent", "Left"],
        ["Staff", "£412,000", "£398,450", "£13,550"],
        ["Sites", "£96,000", "£101,200", "−£5,200"],
        ["Travel", "£38,000", "£35,720", "£2,280"],
        ["Training", "£24,000", "£19,880", "£4,120"],
        ["Reserve budget", "£30,000", "£8,500", "£21,500"],
      ],
    },
    { k: "pic" },
    { k: "caption", h: "Figure 1. Monthly spending against budget, July to September." },
    { k: "h1", h: "Next quarter" },
    { k: "p", h: "The fourth-quarter budget assumes both new sites run at full strength from November. We will report against it in January." },
    { k: "p", h: "Questions about this report go to the regional office." },
  ];
  const B = { p1: 3, p2: 4, highlights: 5, thanks: 10, spending: 11, p3: 12 };

  // The picture: a chart, as a .docx embeds a PNG of one.
  const CHART = `<svg viewBox="0 0 480 250" width="480" height="250" aria-hidden="true">
    <rect width="480" height="250" fill="#fff"/>
    <text x="240" y="24" text-anchor="middle" font-family="Carlito" font-size="17" font-weight="700" fill="#404040">Monthly spending (£ thousands)</text>
    <g stroke="#d9d9d9" stroke-width="1">${[0, 1, 2, 3, 4].map((i) => `<path d="M56 ${206 - i * 38}h400"/>`).join("")}</g>
    <g font-family="Carlito" font-size="13" fill="#595959" text-anchor="end">${[0, 50, 100, 150, 200].map((v, i) => `<text x="48" y="${210 - i * 38}">${v}</text>`).join("")}</g>
    ${[["July", 168], ["August", 181], ["September", 176]].map(([m, v], i) => {
      const x = 92 + i * 132, hgt = (v / 200) * 152;
      return `<rect x="${x}" y="${206 - hgt}" width="64" height="${hgt}" fill="#4472c4"/><text x="${x + 32}" y="${200 - hgt}" text-anchor="middle" font-family="Carlito" font-size="13" fill="#404040">${v}</text><text x="${x + 32}" y="226" text-anchor="middle" font-family="Carlito" font-size="14" fill="#595959">${m}</text>`;
    }).join("")}
    <path d="M70 69.5H446" stroke="#ed7d31" stroke-width="2.5" stroke-dasharray="7 5"/>
    <g font-family="Carlito" font-size="13" fill="#595959"><rect x="150" y="236" width="11" height="11" fill="#4472c4"/><text x="166" y="246">Spent</text><path d="M240 241.5h18" stroke="#ed7d31" stroke-width="2.5" stroke-dasharray="5 3"/><text x="263" y="246">Plan (180)</text></g>
  </svg>`;

  // notes.md, as Slate shows it (its source is in the git diff).
  const NOTES = [
    { k: "mh1", h: "Release notes" },
    { k: "p", h: "What goes into the next release, and who checks it." },
    { k: "mh2", h: "Before you tag" },
    { k: "li", h: "Run the full test suite." },
    { k: "li", h: "Check the installer on Arch, Ubuntu, and Fedora." },
    { k: "li", h: "Update the changelog." },
    { k: "code", h: "cargo test --workspace" },
    { k: "p", h: "Ship on Thursday, after the last review." },
    { k: "mh2", h: "After the release" },
    { k: "p", h: "Post the notes on the website and close the milestone." },
  ];

  // invoice.docx, for SL4's Word fidelity (ADR 0009): A4 with Word's narrow 0.5″ margins,
  // so Text Width → Page gives (11906 − 2 × 720) / 15 ≈ 698 px. The letterhead is in an
  // embedded font (League Spartan, carried in word/fonts/), the section headings are
  // shaded with a bottom border, lines use a right stop at the text width, a left stop
  // at 1.25″, and dot leaders; one paragraph has its own indents; bullets are the
  // numbering's "●" at Word's 0.25″/0.5″ indents.
  const INVOICE_PAGE = 698;
  const tabs = (...cells) => cells.join('<span class="slate-tab"></span>');
  const lead = (item, amount) => `<span>${item}</span><span class="slate-dots">${".".repeat(160)}</span><span>${amount}</span>`;
  const stop = (label, text) => `<span class="slate-stop">${label}</span><span>${text}</span>`;
  const INVOICE = [
    { k: "lhead", h: '<span class="slate-lhead-t">HARBOUR &amp; STONE</span>' },
    { k: "lsub", h: tabs("Architects · 14 Quay Street, Bristol BS1 4DB", "0117 496 0321") },
    { k: "tabrow", h: tabs("<b>Invoice 2026-114</b>", "14 October 2026") },
    { k: "stoprow", h: stop("Billed to", "Northfield Community Trust") },
    { k: "stoprow", h: stop("", "2 Mill Lane, Ashford TN23 1HP") },
    { k: "shade", h: "SERVICES" },
    { k: "leadrow", h: lead("Site survey, Ashford", "£1,200.00") },
    { k: "leadrow", h: lead("Planning drawings, two sites", "£3,450.00") },
    { k: "leadrow", h: lead("Structural consultation", "£860.00") },
    { k: "leadrow slate-total", h: lead("<b>Total due</b>", "<b>£5,510.00</b>") },
    { k: "shade", h: "PAYMENT" },
    { k: "indent", h: "Please pay within 30 days of the invoice date, quoting the invoice number. Payments received late may be charged interest at 4% above the Bank of England base rate." },
    { k: "wli", h: '<span class="slate-wmark">●</span>Harbour Mutual · sort code 20-45-77' },
    { k: "wli", h: '<span class="slate-wmark">●</span>Account 4401 8823 · Harbour &amp; Stone Ltd' },
    { k: "wli", h: '<span class="slate-wmark">●</span>Questions to accounts@harbourstone.co.uk' },
    { k: "thanks", h: "Thank you for your business." },
  ];
  const INVOICE_BODY = { style: "Normal", font: "Calibri", size: "11", align: "left" };
  const INVOICE_HEAD = { style: "Title", font: "League Spartan", size: "26", b: true, align: "left" };

  // ---- the document as HTML ------------------------------------------------
  const CHAR_RE = /[.*+?^${}()|[\]\\]/g;
  function blockHTML(b, i) {
    const st = b.st ? ` style="${b.st}"` : "";
    switch (b.k) {
      case "li":
        return `<div class="slate-b slate-li" data-b="${i}"${st}><span class="slate-dot">•</span>${b.h}</div>`;
      case "table":
        return `<table class="slate-b slate-tbl" data-b="${i}">${b.rows.map((r, j) => `<tr>${r.map((c, k) => `<td class="${j === 0 ? "slate-th" : ""}${k > 0 && j > 0 ? " slate-num" : ""}">${c}</td>`).join("")}</tr>`).join("")}</table>`;
      case "pic":
        return `<div class="slate-b slate-pic" data-b="${i}">${CHART}</div>`;
      case "code":
        return `<div class="slate-b slate-code" data-b="${i}">${b.h}</div>`;
      default:
        return `<div class="slate-b slate-${b.k}" data-b="${i}"${st}>${b.h || "&#8203;"}</div>`;
    }
  }
  // Highlight `q` (case-folded, as Find without Match Case) in text, never in tags.
  function mark(html, q, from) {
    if (!q) return [html, 0];
    const re = new RegExp(q.replace(CHAR_RE, "\\$&"), "gi");
    let n = 0;
    const out = html.replace(/(<[^>]*>)|([^<]+)/g, (m, tag) => (tag ? m : m.replace(re, (w) => `<span class="slate-hit" data-n="${from + n++}">${w}</span>`)));
    return [out, n];
  }
  // The column's HTML; with a search, every match marked and the current one (the
  // first from the caret's block on, wrapping) stronger.
  function docHTML(blocks, { q = "", caretBlock = 0 } = {}) {
    let n = 0, before = 0;
    const parts = blocks.map((b, i) => {
      if (i === caretBlock) before = n;
      const html = blockHTML(b, i);
      if (b.k === "pic") return html; // pictures aren't searched
      const [out, k] = mark(html, q, n);
      n += k;
      return out;
    });
    let html = parts.join("");
    const cur = n ? (before < n ? before : 0) : -1;
    if (cur >= 0) html = html.replace(`class="slate-hit" data-n="${cur}"`, `class="slate-hit slate-cur" data-n="${cur}"`);
    return { html, n, cur };
  }
  const plain = (h) => h.replace(/<[^>]*>/g, " ").replace(/&#8203;/g, "");
  const words = (blocks) => blocks.reduce((n, b) => n + (b.rows ? b.rows.flat().join(" ") : plain(b.h || "")).split(/\s+/).filter((w) => /\w/.test(w)).length, 0);
  const statusText = (kind, blocks) => `${kind === "md" ? "Markdown" : "Word document"} · ${V.fmt(words(blocks))} word${words(blocks) === 1 ? "" : "s"}`;
  const caret = (t, on = true) => `<span class="slate-caret${on && V.blink(t) ? "" : " off"}"></span>`;
  const sel = (h) => `<span class="slate-sel">${h}</span>`;
  // Put the caret before the first `word` of a block's text.
  const caretBefore = (h, word, t) => h.replace(word, caret(t) + word);
  // The blink clock restarts with each key: solid while typing in [t0, t1].
  const keyClock = (t, t0, t1) => (t < t0 ? t : t < t1 ? 0 : t - t1);
  const withBlock = (blocks, i, patch) => blocks.map((b, j) => (j === i ? { ...b, ...patch } : b));
  const insertBlock = (blocks, i, b) => [...blocks.slice(0, i), b, ...blocks.slice(i)];

  // ---- the window ------------------------------------------------------------
  const minus = '<svg class="icon" width="16" height="16" viewBox="0 0 16 16"><path d="M3 8h10" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/></svg>';
  const back = '<svg class="icon" width="16" height="16" viewBox="0 0 16 16"><path d="M10 3.5L5.5 8l4.5 4.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  const check = '<svg class="icon" width="16" height="16" viewBox="0 0 16 16"><path d="M3 8.5l3.2 3.2L13 4.8" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>';

  // format_bar.rs's order: style | font, size | B I U S | colour, reset | alignment.
  const fmtBarHTML = () => `
    <div class="slate-fmt">
      <div class="dropdown slate-dd slate-style"><span class="v"></span>${icon("pan-down")}</div>
      <div class="sep-v"></div>
      <div class="dropdown slate-dd slate-font"><span class="v"></span>${icon("pan-down")}</div>
      <div class="slate-spin"><span class="v"></span><button class="slate-spinb slate-minus">${minus}</button><button class="slate-spinb slate-plus">${icon("list-add")}</button></div>
      <div class="sep-v"></div>
      ${["bold", "italic", "underline", "strikethrough"].map((n) => `<button class="hb-btn slate-flat slate-${n}">${icon(`format-text-${n}`)}</button>`).join("")}
      <div class="sep-v"></div>
      <button class="hb-btn slate-color"><span class="slate-swatch"></span></button>
      <button class="hb-btn slate-flat">${icon("edit-clear")}</button>
      <div class="sep-v"></div>
      <div class="linked">${["left", "center", "right", "fill"].map((a) => `<button class="hb-btn slate-al" data-a="${a}">${icon(`format-justify-${a}`)}</button>`).join("")}</div>
    </div>
    <div class="sep-h"></div>`;

  // find_bar.rs: a Revealer sliding down above the format bar; rows 12/6 margins.
  const findBarHTML = () => `
    <div class="slate-findwrap"><div class="slate-findin">
      <div class="slate-frows">
        <div class="slate-frow">
          <div class="entry slate-fentry">${icon("edit-find")}<span class="slate-etext"></span></div>
          <span class="check">Match Case</span>
          <span class="slate-count dim-label"></span>
          ${btn("go-up")}${btn("go-down")}${btn("window-close")}
        </div>
        <div class="slate-frow slate-rrow">
          <div class="entry slate-rentry"><span class="slate-etext"></span></div>
          ${textBtn("Replace")}${textBtn("Replace All", "slate-rall")}
        </div>
      </div>
      <div class="sep-h"></div>
    </div></div>`;

  const REPORT_TOP = { style: "Title", font: "Calibri", size: "28", align: "left" };

  function windowHTML({ id = "", x, y, w, h, title = "report.docx", kind = "docx", width = PAGE, blocks = REPORT, fmt = REPORT_TOP, find = false, cls = "" }) {
    const colW = width === "full" ? w / Z - 48 : width;
    const body = `
      ${find ? findBarHTML() : ""}
      ${fmtBarHTML()}
      <div class="slate-page"><div class="slate-scroll"><div class="slate-col slate-${kind} ${cls}" style="width:${colW}px">${docHTML(blocks).html}</div></div><div class="slate-sb"></div></div>
      <div class="sep-h"></div>
      <div class="slate-status caption">${statusText(kind, blocks)}</div>`;
    const html = V.ui.win({ id, x, y, w, h, cls: "slate-win", start: btn("document-open", "slate-open"), title: `<span class="slate-t">${title}</span>`, end: btn("open-menu", "slate-menu"), body });
    // The format bar's first state, so a static window is complete too.
    const el = V.html(html);
    setFmt(bind(el, w), fmt);
    return el.outerHTML;
  }

  // The window's parts, for update().
  function bind(el, w) {
    const q = (s) => el.querySelector(s);
    return {
      el,
      pageW: w / Z,
      title: q(".slate-t"),
      menuBtn: q(".slate-menu"),
      page: q(".slate-page"),
      scroll: q(".slate-scroll"),
      col: q(".slate-col"),
      sb: q(".slate-sb"),
      status: q(".slate-status"),
      styleDD: q(".slate-style"),
      fontDD: q(".slate-font"),
      style: q(".slate-style .v"),
      font: q(".slate-font .v"),
      size: q(".slate-spin .v"),
      plus: q(".slate-plus"),
      colorBtn: q(".slate-color"),
      swatch: q(".slate-swatch"),
      looks: { b: q(".slate-bold"), i: q(".slate-italic"), u: q(".slate-underline"), s: q(".slate-strikethrough") },
      aligns: [...el.querySelectorAll(".slate-al")],
      findWrap: q(".slate-findwrap"),
      findIn: q(".slate-findin"),
      rrow: q(".slate-rrow"),
      fentry: q(".slate-fentry"),
      ftext: q(".slate-fentry .slate-etext"),
      rentry: q(".slate-rentry"),
      rtext: q(".slate-rentry .slate-etext"),
      count: q(".slate-count"),
      rall: q(".slate-rall"),
    };
  }
  function setFmt(w, f) {
    V.text(w.style, f.style);
    V.text(w.font, f.font);
    V.text(w.size, f.size);
    for (const k of ["b", "i", "u", "s"]) V.cls(w.looks[k], "on", !!f[k]);
    V.css(w.swatch, { background: f.color || "#000000" });
    for (const a of w.aligns) V.cls(a, "on", a.dataset.a === (f.align || "left"));
  }
  // Draw a window's state: { title, blocks, width, scroll, fmt, find }.
  function draw(w, s) {
    if (s.title !== undefined) V.text(w.title, s.title);
    if (s.blocks) {
      const d = docHTML(s.blocks, s.find || {});
      V.markup(w.col, d.html);
      if (s.status !== false) V.text(w.status, statusText(s.kind || "docx", s.words || s.blocks));
      w.found = d;
    }
    if (s.width !== undefined) V.css(w.col, { width: `${(s.width === "full" ? w.pageW - 48 : s.width).toFixed(2)}px` });
    if (s.scroll !== undefined) V.css(w.scroll, { transform: `translateY(${-s.scroll.toFixed(1)}px)` });
    if (s.fmt) setFmt(w, s.fmt);
  }
  // The find bar, revealed by k (0..1); `rows` 1 or 2.
  function drawFind(w, k, rows) {
    const full = rows === 2 ? 87 : 47;
    V.show(w.rrow, rows === 2);
    V.css(w.findWrap, { height: `${(k * full).toFixed(2)}px` });
    V.css(w.findIn, { transform: `translateY(${((k - 1) * full).toFixed(2)}px)` });
  }
  const entryText = (text, ph, focused, t) => (text ? text.replace(/</g, "&lt;") : `<span class="ph">${ph}</span>`) + (focused ? caret(t) : "");

  // ---- stage helpers -----------------------------------------------------------
  const stageK = () => document.getElementById("stage").getBoundingClientRect().width / V.W;
  const at = (el, ax = 0.5, ay = 0.5) => V.ui.pos(el, ax, ay);
  // The camera: { x, y } of the stage stays at the screen's centre, scaled by s.
  function camera(el, c) {
    const s = c.s;
    const x = V.clamp(c.x, V.W / 2 / s, V.W - V.W / 2 / s);
    const y = V.clamp(c.y, V.H / 2 / s, V.H - V.H / 2 / s);
    V.css(el, { transform: `translate(${(V.W / 2 - x * s).toFixed(2)}px, ${(V.H / 2 - y * s).toFixed(2)}px) scale(${s.toFixed(4)})` });
  }
  // A popover in stage coordinates, centred under (x, y), drawn at the desktop's scale.
  const popHTML = (cls, inner) => `<div class="slate-popwrap ${cls}"><div class="slate-zoom"><div class="slate-arrow"></div>${inner}</div></div>`;
  // A popover centred under (x, y), a point on screen in stage pixels (from at()). It
  // lives in the scene or the camera, so the point is mapped into its parent's space.
  function placePop(wrap, open, x, y) {
    V.css(wrap, { opacity: open ? 1 : 0 });
    const k = stageK();
    const parent = wrap.parentElement.getBoundingClientRect();
    const stage = document.getElementById("stage").getBoundingClientRect();
    const sc = parent.width / k / V.W;
    const width = wrap.firstElementChild.getBoundingClientRect().width / k / sc;
    const lx = (x - (parent.left - stage.left) / k) / sc, ly = (y - (parent.top - stage.top) / k) / sc;
    V.css(wrap, { left: `${(lx - width / 2).toFixed(1)}px`, top: `${(ly + 4 / sc).toFixed(1)}px` });
  }
  const findLabel = (root, cls, label) => [...root.querySelectorAll(`.${cls} .mi`)].find((m) => m.dataset.label === label);
  // A typing span of keys pressed: text typed from t0 at cps.
  const typing = (text, t, t0, cps = 14) => V.typed(text, t, t0, cps);
  // Double and triple clicks for V.pointer.
  const clicks = (t0, x, y, n, gap = 0.16) => Array.from({ length: n }, (_, i) => [t0 + i * gap, x, y, { click: true }]);

  // ---- main menu (main.rs header()) ------------------------------------------------
  const MAIN_MENU = [
    ["New", "Ctrl+N"], ["Open…", ""], ["Save", "Ctrl+S"], ["Save As…", "Shift+Ctrl+S"], ["Close Window", ""],
    "-", { heading: "View" }, { label: "Text Width", sub: true },
    "-", { heading: "Edit" }, ["Undo", "Ctrl+Z"], ["Redo", "Shift+Ctrl+Z"], ["Cut", "Ctrl+X"], ["Copy", "Ctrl+C"], ["Paste", "Ctrl+V"], ["Select All", "Ctrl+A"], ["Find…", "Ctrl+F"], ["Replace…", "Ctrl+H"],
    "-", { heading: "Format" }, ["Bold", "Ctrl+B"], ["Italic", "Ctrl+I"], ["Underline", "Ctrl+U"], ["Strikethrough", ""], { label: "Alignment", sub: true }, { label: "Paragraph Style", sub: true }, ["Bulleted List", "Shift+Ctrl+8"], ["Numbered List", "Shift+Ctrl+7"],
    "-", ["Quit", "Ctrl+Q"],
  ];
  const widthMenu = (current) =>
    V.ui.menu(TEXT_WIDTHS.map((l) => ({ label: l, radio: l === current })), { cls: "slate-submenu" }).replace(
      '<div class="popover slate-submenu">',
      `<div class="popover slate-submenu"><div class="mi slate-subhead"><span class="slate-back">${back}</span><span class="mi-label">Text Width</span></div><div class="mi-sep"></div>`,
    );

  // The styles of report.docx, each drawn in its own font, size (capped at 16 pt), and
  // weight, as the style drop-down lists them (format_bar.rs style_label).
  const STYLES = [
    ["Normal", "Caladea", 11], ["Title", "Carlito", 16], ["Subtitle", "Carlito", 11], ["Heading 1", "Carlito", 16, "b"], ["Heading 2", "Carlito", 13, "b"],
    ["Quote", "Caladea", 11, "i"], ["Caption", "Caladea", 9, "i"], ["List Paragraph", "Caladea", 11],
  ];
  const FONTS = ["Caladea", "Calibri", "Cambria", "Cantarell", "Carlito", "DejaVu Sans", "DejaVu Serif", "Inter", "JetBrains Mono", "Liberation Mono", "Liberation Sans", "Liberation Serif", "Noto Sans", "Noto Serif"];

  // ---- dialogs ---------------------------------------------------------------------
  // GTK's colour chooser: nine hues of five shades, then Custom.
  const PALETTE = [
    ["#99c1f1", "#62a0ea", "#3584e4", "#1c71d8", "#1a5fb4"], ["#8ff0a4", "#57e389", "#33d17a", "#2ec27e", "#26a269"], ["#f9f06b", "#f8e45c", "#f6d32d", "#f5c211", "#e5a50a"],
    ["#ffbe6f", "#ffa348", "#ff7800", "#e66100", "#c64600"], ["#f66151", "#ed333b", "#e01b24", "#c01c28", "#a51d2d"], ["#dc8add", "#c061cb", "#9141ac", "#813d9c", "#613583"],
    ["#cdab8f", "#b5835a", "#986a44", "#865e3c", "#63452c"], ["#ffffff", "#f6f5f4", "#deddda", "#c0bfbc", "#9a9996"], ["#77767b", "#5e5c64", "#3d3846", "#241f31", "#000000"],
  ];
  const colourDialogHTML = (id, x, y) =>
    V.ui.win({
      id, x, y, w: 560, h: 440, cls: "slate-dialog",
      start: textBtn("Cancel"), title: "Select a Colour", end: textBtn("Select", "suggested slate-select"),
      body: `<div class="slate-palette">${PALETTE.map((col) => `<div class="slate-pcol">${col.map((c) => `<div class="slate-sw" data-c="${c}" style="background:${c}">${check}</div>`).join("")}</div>`).join("")}</div>
        <div class="slate-custom"><div class="slate-chead">Custom</div><div class="slate-addc">${icon("list-add")}</div></div>`,
    });

  const DOCS = [
    { name: "budget.xlsx", size: "48.2 kB", when: "Yesterday" },
    { name: "deck.pptx", size: "1.2 MB", when: "Oct 9" },
    { name: "notes.md", size: "612 bytes", when: "10:02" },
    { name: "photo.jpg", size: "2.4 MB", when: "Oct 2" },
    { name: "report.docx", size: "38.4 kB", when: "10:20" },
  ];

  // The window `slate` shows without a file (main.rs start_window), with the sessions a
  // crash left (recovery.rs start_section): name, "unsaved edits · 10:23", Discard, Restore.
  const startWindowHTML = (id, { x, y, w, h }, crashed) =>
    V.ui.win({
      id, x, y, w, h, cls: "slate-win slate-startwin",
      start: btn("document-open"), title: "Slate", end: btn("open-menu"),
      body: `<div class="slate-start">
        <div class="slate-title2">Open a document</div>
        <div class="dim-label">Slate shows .txt, .md, and .docx files. Drop one here, choose Open, or start a new one.</div>
        <div class="slate-startbtns">${textBtn("Open…", "suggested")}${textBtn("New Document")}</div>
        <div class="slate-crashed"><div class="slate-heading">Unsaved edits from a crash</div>
          ${crashed.map((c) => `<div class="slate-crow"><span>${c.name}</span><span class="dim-label slate-grow">unsaved edits · ${c.when}</span>${textBtn("Discard", "slate-flatbtn")}${textBtn("Restore", "suggested slate-restore")}</div>`).join("")}
        </div>
      </div>`,
    });
  const saveDialogHTML = (id, x, y) =>
    V.ui.win({
      id, x, y, w: 1180, h: 740, cls: "slate-dialog slate-filedlg",
      start: textBtn("Cancel"), title: "Save As", end: textBtn("Save", "suggested"),
      body: `<div class="slate-namerow"><span>Name</span><div class="entry focus slate-name"><span><span class="slate-sel2">Untitled</span>.docx</span></div></div>
        <div class="slate-fcmain">
          <div class="slate-fcside">${[["Recent", "folder"], ["Starred", "folder"], ["Home", "go-home"], ["Desktop", "folder"], ["Documents", "folder"], ["Downloads", "folder"], ["Music", "folder"], ["Pictures", "folder"], ["Videos", "folder"]].map(([n, i]) => `<div class="side-row${n === "Documents" ? " on" : ""}">${icon(i)}<span>${n}</span></div>`).join("")}</div>
          <div class="slate-fclist">
            <div class="slate-fcpath"><span class="slate-crumb">${icon("go-home")}you</span><span class="slate-crumb on">Documents</span></div>
            <div class="slate-fchead"><span>Name</span><span>Size</span><span>Modified</span></div>
            ${[{ name: "Projects", size: "", when: "Oct 11", dir: true }, ...DOCS].map((f) => `<div class="slate-fcrow"><span class="slate-fcn">${f.dir ? `<span class="foldericon">${icon("folder")}</span>` : V.ui.fileIcon(f.name.split(".").pop(), 22)}${f.name}</span><span>${f.size}</span><span>${f.when}</span></div>`).join("")}
            <div class="slate-fcfilter"><div class="dropdown">Word document${icon("pan-down")}</div></div>
          </div>
        </div>`,
    });

  // ---- LibreOffice Writer --------------------------------------------------------
  // Colibre-like toolbar glyphs: small, mostly grey with a coloured accent.
  const g = (inner) => `<svg class="slate-lo-ic" width="20" height="20" viewBox="0 0 20 20">${inner}</svg>`;
  const LO_STD = [
    g('<path d="M5 2h7l4 4v12H5z" fill="#fff" stroke="#555" stroke-width="1.2"/><path d="M12 2v4h4" fill="none" stroke="#555" stroke-width="1.2"/>'),
    g('<path d="M2 5h6l2 2h8v10H2z" fill="#f5c211" stroke="#8a6d00" stroke-width="1"/>'),
    g('<path d="M3 3h12l2 2v12H3z" fill="#3584e4" stroke="#1a5fb4" stroke-width="1"/><rect x="6" y="4" width="7" height="4" fill="#fff"/>'),
    "|",
    g('<rect x="4" y="2" width="12" height="16" rx="1" fill="#fff" stroke="#555" stroke-width="1.2"/><rect x="6" y="11" width="8" height="4" fill="#e01b24"/><text x="10" y="9" text-anchor="middle" font-size="5" font-family="Inter" font-weight="700" fill="#e01b24">PDF</text>'),
    g('<rect x="3" y="8" width="14" height="7" rx="1" fill="#777"/><rect x="6" y="3" width="8" height="5" fill="#fff" stroke="#555"/><rect x="6" y="13" width="8" height="5" fill="#fff" stroke="#555"/>'),
    "|",
    g('<circle cx="6" cy="14" r="2.5" fill="none" stroke="#555" stroke-width="1.3"/><circle cx="14" cy="14" r="2.5" fill="none" stroke="#555" stroke-width="1.3"/><path d="M7.5 12L14 3M12.5 12L6 3" stroke="#555" stroke-width="1.3"/>'),
    g('<rect x="3" y="5" width="9" height="12" fill="#fff" stroke="#555" stroke-width="1.2"/><rect x="8" y="2" width="9" height="12" fill="#fff" stroke="#555" stroke-width="1.2"/>'),
    g('<rect x="4" y="4" width="12" height="14" rx="1" fill="#c8a165" stroke="#7d5c2d"/><rect x="7" y="2" width="6" height="4" rx="1" fill="#fff" stroke="#555"/><rect x="6" y="8" width="8" height="8" fill="#fff"/>'),
    g('<path d="M4 16l7-11 3 2-7 11H4z" fill="#f5c211" stroke="#8a6d00"/>'),
    "|",
    g('<path d="M8 4L3 9l5 5" fill="none" stroke="#1a5fb4" stroke-width="2"/><path d="M3 9h9a5 5 0 0 1 0 10" fill="none" stroke="#1a5fb4" stroke-width="2"/>'),
    g('<path d="M12 4l5 5-5 5" fill="none" stroke="#999" stroke-width="2"/><path d="M17 9H8a5 5 0 0 0 0 10" fill="none" stroke="#999" stroke-width="2"/>'),
    "|",
    g('<circle cx="8" cy="8" r="5" fill="none" stroke="#555" stroke-width="1.6"/><path d="M12 12l5 5" stroke="#555" stroke-width="2"/>'),
    g('<text x="2" y="10" font-size="9" font-family="Inter" font-weight="700" fill="#555">abc</text><path d="M4 15l3 3 8-8" fill="none" stroke="#26a269" stroke-width="2"/>'),
    g('<text x="4" y="16" font-size="15" font-family="Caladea" fill="#555">¶</text>'),
    "|",
    g('<rect x="2" y="3" width="16" height="14" fill="#fff" stroke="#555"/><path d="M2 8h16M2 12.5h16M7.5 3v14M13 3v14" stroke="#555"/>'),
    g('<rect x="2" y="3" width="16" height="14" fill="#fff" stroke="#555"/><path d="M3 15l5-6 4 4 2-2 3 4z" fill="#26a269"/><circle cx="13" cy="7" r="2" fill="#f5c211"/>'),
    g('<rect x="3" y="10" width="3" height="7" fill="#3584e4"/><rect x="8.5" y="5" width="3" height="12" fill="#e66100"/><rect x="14" y="8" width="3" height="9" fill="#26a269"/>'),
    g('<rect x="2" y="4" width="16" height="12" fill="#fff" stroke="#555"/><text x="10" y="13.5" text-anchor="middle" font-size="9" font-family="Caladea" font-weight="700" fill="#555">A</text>'),
    g('<path d="M5 2h10v6H5zM5 12h10v6H5z" fill="#fff" stroke="#555"/><path d="M2 10h16" stroke="#3584e4" stroke-dasharray="2 2" stroke-width="1.5"/>'),
    g('<rect x="2" y="5" width="16" height="10" rx="2" fill="#deddda" stroke="#777"/><text x="10" y="13" text-anchor="middle" font-size="8" font-family="Inter" fill="#333">{x}</text>'),
    g('<text x="10" y="16" text-anchor="middle" font-size="15" font-family="Caladea" fill="#555">Ω</text>'),
    g('<rect x="2" y="7" width="9" height="6" rx="3" fill="none" stroke="#1a5fb4" stroke-width="1.8" transform="rotate(-35 6.5 10)"/><rect x="9" y="7" width="9" height="6" rx="3" fill="none" stroke="#1a5fb4" stroke-width="1.8" transform="rotate(-35 13.5 10)"/>'),
    g('<path d="M3 4h14v10H9l-4 3v-3H3z" fill="#f9f06b" stroke="#8a6d00"/>'),
  ];
  const LO_FMT_ICONS = [
    "B", "I", "U", "S", "|", "x²", "x₂", "|",
    g('<text x="10" y="13" text-anchor="middle" font-size="12" font-family="Caladea" font-weight="700" fill="#333">A</text><rect x="3" y="15" width="14" height="3" fill="#e01b24"/>'),
    g('<path d="M4 15l7-11 4 3-7 11z" fill="#fff" stroke="#555"/><rect x="3" y="15" width="14" height="3" fill="#f6d32d"/>'),
    "|",
    g('<path d="M3 4h14M3 8h9M3 12h14M3 16h9" stroke="#555" stroke-width="1.6"/>'),
    g('<path d="M3 4h14M5.5 8h9M3 12h14M5.5 16h9" stroke="#555" stroke-width="1.6"/>'),
    g('<path d="M3 4h14M8 8h9M3 12h14M8 16h9" stroke="#555" stroke-width="1.6"/>'),
    g('<path d="M3 4h14M3 8h14M3 12h14M3 16h14" stroke="#555" stroke-width="1.6"/>'),
    "|",
    g('<circle cx="4" cy="5" r="1.6" fill="#555"/><circle cx="4" cy="10" r="1.6" fill="#555"/><circle cx="4" cy="15" r="1.6" fill="#555"/><path d="M8 5h9M8 10h9M8 15h9" stroke="#555" stroke-width="1.6"/>'),
    g('<text x="1" y="7" font-size="6" font-family="Inter" fill="#555">1</text><text x="1" y="12" font-size="6" font-family="Inter" fill="#555">2</text><text x="1" y="17" font-size="6" font-family="Inter" fill="#555">3</text><path d="M8 5h9M8 10h9M8 15h9" stroke="#555" stroke-width="1.6"/>'),
  ];
  const loTools = (list) => list.map((x) => (x === "|" ? '<span class="slate-lo-sep"></span>' : x.startsWith("<svg") ? `<span class="slate-lo-tb">${x}</span>` : `<span class="slate-lo-tb slate-lo-txt">${x}</span>`)).join("");
  // A Word page at 100%: Letter, 1 in margins, the header and footer in the top and bottom margins.
  const PAGE_W = 816, PAGE_H = 1056, MARGIN = 96;
  const pageHTML = (n, of, docHtml) => `
    <div class="slate-lo-page" data-page="${n}">
      <div class="slate-lo-header"><span>Northfield Community Trust</span><span>Field Office Report · Q3 2026</span></div>
      <div class="slate-lo-body"><div class="slate-lo-flow slate-col slate-docx">${docHtml}</div></div>
      <div class="slate-lo-footer"><span>For the steering group · Not for circulation</span><span>Page ${n} of ${of}</span></div>
    </div>`;
  function writerHTML({ id, x, y, w, h, docHtml, count, zoom = 0.44 }) {
    const menu = ["File", "Edit", "View", "Insert", "Format", "Styles", "Table", "Form", "Tools", "Window", "Help"].map((m) => `<span>${m}</span>`).join("");
    const ruler = Array.from({ length: 34 }, (_, i) => `<i style="left:${i * 24}px" class="${i % 4 === 0 ? "l" : ""}"></i>`).join("");
    const sidebar = ["⚙", "A", "▦", "◎", "☰"].map((c) => `<span>${c}</span>`).join("");
    return V.ui.win({
      id, x, y, w, h, cls: "slate-lo",
      title: "report.docx - LibreOffice Writer",
      body: `
        <div class="slate-lo-menubar">${menu}</div>
        <div class="slate-lo-bar">${loTools(LO_STD)}</div>
        <div class="slate-lo-bar">
          <span class="slate-lo-combo" style="width:180px">Default Paragraph Style</span>
          <span class="slate-lo-combo" style="width:150px">Cambria</span>
          <span class="slate-lo-combo" style="width:64px">11 pt</span>
          <span class="slate-lo-sep"></span>
          ${loTools(LO_FMT_ICONS)}
        </div>
        <div class="slate-lo-main">
          <div class="slate-lo-docarea">
            <div class="slate-lo-ruler"><div class="slate-lo-rulerin" style="width:${(PAGE_W * zoom).toFixed(1)}px">${ruler}</div></div>
            <div class="slate-lo-desk"><div class="slate-lo-pages" style="--z:${zoom}">${pageHTML(1, 2, docHtml)}${pageHTML(2, 2, docHtml)}</div></div>
          </div>
          <div class="slate-lo-side">${sidebar}</div>
        </div>
        <div class="slate-lo-status"><span>Page 1 of 2</span><span>${V.fmt(count)} words, 1,412 characters</span><span>Default Page Style</span><span>English (UK)</span><span class="slate-lo-grow"></span><span>${Math.round(zoom * 100)}%</span></div>`,
    });
  }

  // ---- the shared contract ------------------------------------------------------------
  // A Slate window showing report.docx at its top, at Medium width (for install-open).
  V.slate = {
    window: ({ id, x, y, w, h, title = "report.docx" }) => windowHTML({ id, x, y, w, h, title }),
  };

  // The column's scroll position that puts block i near the page's top.
  function scrollTo(w, i, pad = 16) {
    const el = w.col.querySelector(`[data-b="${i}"]`);
    return el ? Math.max(0, el.offsetTop + 40 - pad) : 0;
  }
  function maxScroll(w) {
    return Math.max(0, w.col.offsetHeight + 104 - w.page.clientHeight);
  }

  const FMT = {
    title: REPORT_TOP,
    normal: { style: "Normal", font: "Cambria", size: "11", align: "left" },
    h1: { style: "Heading 1", font: "Calibri", size: "16", b: true, align: "left" },
    li: { style: "List Paragraph", font: "Cambria", size: "11", align: "left" },
  };

  // =====================================================================================
  // Double-click report.docx in Files; Slate opens; a slow scroll down the one column.
  V.scene({
    id: "slate-open",
    beats: [
      { say: "Slate is the word processor.", min: 3.4 },
      { say: "You'll notice there are no pages. Slate gives you one endless page, so you write instead of fighting margins." },
      { say: "And Word documents look like themselves, in their own fonts and styles.", min: 6 },
    ],
    build(root, s) {
      root.innerHTML = `<div class="slate-cam">${V.ui.desktop()}
        ${V.ui.files({ id: "slo-files", x: 300, y: 150, w: 1240, h: 680, path: "Documents", items: [{ name: "Projects", ext: "dir" }, { name: "budget.xlsx", ext: "xlsx" }, { name: "deck.pptx", ext: "pptx" }, { name: "notes.md", ext: "md" }, { name: "photo.jpg", ext: "jpg" }, { name: "report.docx", ext: "docx" }] })}
        ${windowHTML({ id: "slo-win", ...SW })}</div>`;
      const cam = root.querySelector(".slate-cam");
      const files = root.querySelector("#slo-files");
      const winEl = root.querySelector("#slo-win");
      const w = bind(winEl, SW.w);
      const icon = files.querySelector('[data-name="report.docx"]');
      const open = 1.85;
      const scrollEnd = scrollTo(w, B.spending, 26);
      const b1 = s.b[1], b2 = s.b[2];
      const clickBody = b2.start + 0.5, clickHead = s.at(2, "fonts");
      return (t) => {
        V.ui.selectFile(files, t >= 1.25 ? "report.docx" : "");
        V.ui.appear(winEl, t, open);
        // A slow scroll through the column, then a click in the body, then a heading.
        const y = V.tw(t, b1.start + 0.2, b1.end + 0.4, 0, scrollEnd, V.ease.inOut);
        const scrolling = t > b1.start + 0.1 && t < b1.end + 1.2;
        V.css(w.sb, { opacity: scrolling ? 1 : 0, top: `${(6 + (y / (maxScroll(w) || 1)) * (w.page.clientHeight - 140)).toFixed(1)}px` });
        let blocks = REPORT, fmt = FMT.title;
        if (t >= clickHead) {
          blocks = withBlock(REPORT, B.spending, { h: caretBefore(REPORT[B.spending].h, "ding", t) });
          fmt = FMT.h1;
        } else if (t >= clickBody) {
          blocks = withBlock(REPORT, B.p3, { h: caretBefore(REPORT[B.p3].h, "rose", t) });
          fmt = FMT.normal;
        } else if (t >= open) blocks = withBlock(REPORT, 0, { h: caret(t) + REPORT[0].h });
        draw(w, { blocks, scroll: y, fmt, words: REPORT });
        camera(cam, V.track(t, [[0, { x: 960, y: 540, s: 1 }], [b2.start, { x: 960, y: 540, s: 1 }], [b2.start + 1.6, { x: 760, y: 420, s: 1.3 }], [b2.end, { x: 780, y: 420, s: 1.34 }]]));
        // The pointer: double-click the file, then click in the body and on a heading.
        const [ix, iy] = at(icon, 0.5, 0.4);
        const path = [[0, 1480, 860], [1.0, ix, iy], ...clicks(1.25, ix, iy, 2, 0.2), [2.6, 1500, 760]];
        if (t >= open + 0.2) {
          const rose = w.col.querySelector(`[data-b="${B.p3}"]`);
          const head = w.col.querySelector(`[data-b="${B.spending}"]`);
          const [px, py] = at(rose, 0.42, 0.3);
          const [hx, hy] = at(head, 0.06, 0.55);
          path.push([b2.start, 1500, 760], [clickBody - 0.1, px, py], [clickBody, px, py, { click: true }], [clickHead - 0.15, hx, hy], [clickHead, hx, hy, { click: true }]);
        }
        V.pointer(t, path);
      };
    },
  });

  // =====================================================================================
  // A Word invoice at Text Width → Page (ADR 0009): the page's text width, tab stops
  // with leaders, a bordered and shaded heading, an embedded font, Word's "●" bullets.
  V.scene({
    id: "slate-fidelity",
    beats: [{ say: "Word layouts come through too: the page's own width, tab stops with leaders, borders and shading, embedded fonts, and Word's own list markers.", min: 12.5 }],
    build(root, s) {
      const IW = { x: 160, y: 62, w: 1600, h: 880 };
      root.innerHTML = `<div class="slate-cam">${V.ui.desktop()}${windowHTML({ id: "slfi-win", ...IW, title: "invoice.docx", width: INVOICE_PAGE, blocks: INVOICE, fmt: INVOICE_BODY, cls: "slate-inv" })}
        ${popHTML("slfi-main", V.ui.menu(MAIN_MENU, { cls: "slate-mainmenu" }))}${popHTML("slfi-sub", widthMenu("Page"))}</div>`;
      const cam = root.querySelector(".slate-cam");
      const winEl = root.querySelector("#slfi-win");
      const w = bind(winEl, IW.w);
      const main = root.querySelector(".slfi-main");
      const sub = root.querySelector(".slfi-sub");
      const q = (sel) => w.col.querySelector(sel);
      const T = { appear: 0.1, open: 0.6, sub: 1.3 };
      T.close = Math.max(2.4, s.at(0, "the page's own width") + 0.6);
      T.tabs = Math.max(T.close + 0.3, s.at(0, "tab stops"));
      T.borders = Math.max(T.tabs + 1.1, s.at(0, "borders"));
      T.fonts = Math.max(T.borders + 1.1, s.at(0, "embedded fonts"));
      T.markers = Math.max(T.fonts + 1.1, s.at(0, "list markers"));
      // Where each detail is, measured now, before anything moves.
      const pt = (el, ax, ay) => {
        const [x, y] = at(el, ax, ay);
        return { x, y };
      };
      const tabRow = pt(q(".slate-tabrow"), 0.5, 0.5);
      const leaders = pt(q(".slate-leadrow"), 0.5, 0.5);
      const services = pt(q(".slate-shade"), 0.5, 0.5);
      const head = pt(q(".slate-lhead"), 0.5, 0.5);
      const bullets = pt(q(".slate-wli"), 0.3, 0.5);
      const keys = [
        [0, { x: 960, y: 540, s: 1 }],
        [T.close + 0.1, { x: 960, y: 540, s: 1 }],
        // Tabs, leaders, and the shaded heading share one view; the camera drifts
        // toward the heading as the narrator reaches borders and shading.
        [T.tabs + 0.7, { x: leaders.x, y: (tabRow.y + leaders.y) / 2, s: 1.5 }],
        [T.borders, { x: leaders.x, y: (tabRow.y + leaders.y) / 2 + 6, s: 1.52 }],
        [T.borders + 0.8, { x: services.x, y: services.y + 20, s: 1.62 }],
        [T.fonts - 0.1, { x: services.x, y: services.y + 22, s: 1.63 }],
        [T.fonts + 0.6, { x: head.x - 60, y: head.y - 30, s: 1.6 }],
        [T.markers - 0.1, { x: head.x - 60, y: head.y - 30, s: 1.61 }],
        [T.markers + 0.7, { x: bullets.x + 80, y: bullets.y - 60, s: 1.7 }],
        [s.dur, { x: bullets.x + 80, y: bullets.y - 56, s: 1.74 }],
      ];
      return (t) => {
        camera(cam, V.track(t, keys));
        V.ui.appear(winEl, t, T.appear);
        // The caret starts on the invoice line; a click in the letterhead shows its font.
        const inHead = t >= T.fonts + 0.25;
        const blocks = inHead
          ? withBlock(INVOICE, 0, { h: INVOICE[0].h.replace("STONE", `STO${caret(t - T.fonts)}NE`) })
          : withBlock(INVOICE, 2, { h: caret(t, t >= T.appear + 0.3) + INVOICE[2].h });
        draw(w, { blocks, fmt: inHead ? INVOICE_HEAD : INVOICE_BODY, words: INVOICE });
        // Menu → Text Width: Page is the default; picking it again closes the menu.
        const [mx, my] = at(w.menuBtn, 0.5, 1);
        V.cls(w.menuBtn, "on", t >= T.open && t < T.close);
        placePop(main, t >= T.open && t < T.sub, mx, my);
        placePop(sub, t >= T.sub && t < T.close, mx, my);
        const tw = findLabel(root, "slfi-main", "Text Width");
        const page = findLabel(root, "slfi-sub", "Page");
        V.cls(tw, "hot", t >= T.sub - 0.35 && t < T.sub);
        V.cls(page, "hot", t >= T.close - 0.35 && t < T.close);
        const [tx, ty] = at(tw, 0.35, 0.5);
        const [px, py] = at(page, 0.35, 0.5);
        const [hx, hy] = at(q(".slate-lhead-t"), 0.84, 0.55);
        V.pointer(t, [
          [0, 1500, 760], [T.open - 0.4, mx, my - 18], [T.open, mx, my - 18, { click: true }],
          [T.sub - 0.3, tx, ty], [T.sub, tx, ty, { click: true }],
          [T.close - 0.35, px, py], [T.close, px, py, { click: true }],
          [T.close + 0.8, 1700, 640], [T.fonts - 0.6, 1700, 640], [T.fonts + 0.15, hx, hy], [T.fonts + 0.25, hx, hy, { click: true }],
          [T.markers + 0.4, 1760, 620],
        ]);
      };
    },
  });

  // =====================================================================================
  // Menu → Text Width: from Page (the default) to Narrow, then Wide, then Full; the
  // column reflows each time.
  V.scene({
    id: "slate-width",
    beats: [{ say: "Choose how wide you want the column. That's a setting for you; it doesn't change the document.", min: 11 }],
    build(root, s) {
      root.innerHTML = `${V.ui.desktop()}${windowHTML({ id: "slw-win", ...SW })}
        ${popHTML("slw-main", V.ui.menu(MAIN_MENU, { cls: "slate-mainmenu" }))}
        ${TEXT_WIDTHS.map((l) => popHTML(`slw-sub slw-sub-${l}`, widthMenu(l))).join("")}`;
      const w = bind(root.querySelector("#slw-win"), SW.w);
      const main = root.querySelector(".slw-main");
      const subs = Object.fromEntries(TEXT_WIDTHS.map((l) => [l, root.querySelector(`.slw-sub-${l}`)]));
      // Three rounds: open the menu, Text Width, pick a width.
      const rounds = [["Narrow", 0.5], ["Wide", 3.7], ["Full", 6.9]].map(([pick, t0]) => ({ pick, open: t0 + 0.4, sub: t0 + 1.3, choose: t0 + 2.2 }));
      const widthAt = (t) => {
        let wd = PAGE;
        for (const [i, r] of rounds.entries()) {
          const from = i === 0 ? PAGE : rounds[i - 1].pick === "Full" ? w.pageW - 48 : WIDTHS[rounds[i - 1].pick];
          const to = r.pick === "Full" ? w.pageW - 48 : WIDTHS[r.pick];
          if (t >= r.choose) wd = V.tw(t, r.choose + 0.05, r.choose + 0.45, from, to, V.ease.inOut);
        }
        return wd;
      };
      const current = (t) => {
        let c = "Page";
        for (const r of rounds) if (t >= r.choose) c = r.pick;
        return c;
      };
      return (t) => {
        draw(w, { blocks: withBlock(REPORT, 0, { h: caret(t, false) + REPORT[0].h }), width: widthAt(t), scroll: 0, fmt: FMT.title });
        V.cls(w.menuBtn, "on", rounds.some((r) => t >= r.open && t < r.choose));
        const [mx, my] = at(w.menuBtn, 0.5, 1);
        const round = rounds.find((r) => t >= r.open && t < r.choose);
        const onMain = round && t < round.sub;
        placePop(main, onMain, mx, my);
        const cur = current(t);
        for (const [l, el] of Object.entries(subs)) placePop(el, round && !onMain && l === cur, mx, my);
        // Hover highlights follow the pointer.
        const tw = findLabel(root, "slw-main", "Text Width");
        V.cls(tw, "hot", !!round && onMain && t >= round.sub - 0.35);
        for (const r of rounds) {
          const it = findLabel(root, `slw-sub-${cur}`, r.pick);
          if (it) V.cls(it, "hot", !!round && round.pick === r.pick && t >= r.choose - 0.35);
        }
        const path = [[0, 1300, 620]];
        for (const r of rounds) {
          const [tx, ty] = at(tw, 0.35, 0.5);
          const pickEl = findLabel(root, `slw-sub-${cur}`, r.pick);
          const [px, py] = pickEl ? at(pickEl, 0.35, 0.5) : [tx, ty + 80];
          path.push([r.open - 0.35, mx, my - 18], [r.open, mx, my - 18, { click: true }], [r.sub - 0.3, tx, ty], [r.sub, tx, ty, { click: true }], [r.choose - 0.35, px, py], [r.choose, px, py, { click: true }]);
        }
        path.push([rounds[2].choose + 1.2, 1420, 700]);
        V.pointer(t, path);
      };
    },
  });

  // =====================================================================================
  // Type a sentence, Ctrl+B, Ctrl+1; undo, redo.
  V.scene({
    id: "slate-typing",
    beats: [{ say: "Typing, headings, lists, tables, and copy and paste all work as you'd expect, with full undo.", min: 10.6 }],
    build(root) {
      root.innerHTML = `<div class="slate-cam">${V.ui.desktop()}${windowHTML({ id: "slt-win", ...SW })}</div>`;
      const cam = root.querySelector(".slate-cam");
      const w = bind(root.querySelector("#slt-win"), SW.w);
      const text = "Every site now files its returns online.";
      const T = { enter: 0.7, type: 1.0, home: 4.4, bold: 5.3, h1: 6.3, undo: 7.6, redo: 8.9 };
      return (t) => {
        let blocks = REPORT, fmt = FMT.normal;
        if (t < T.enter) blocks = withBlock(REPORT, B.p2, { h: REPORT[B.p2].h + caret(t) });
        else {
          const typedText = typing(text, t, T.type);
          const bold = t >= T.bold;
          const heading = (t >= T.h1 && t < T.undo) || t >= T.redo;
          let h = t >= T.home ? sel(bold ? `<b>${text}</b>` : text) : typedText + caret(keyClock(t, T.type, V.typedEnd(text, T.type, 14)));
          if (t >= T.home) h = caret(t) + h;
          blocks = insertBlock(REPORT, B.p2 + 1, { k: heading ? "h1" : "p", h });
          fmt = heading ? { ...FMT.h1, b: true } : { ...FMT.normal, b: bold };
        }
        draw(w, { blocks, scroll: 0, fmt, title: t >= T.type ? "• report.docx" : "report.docx" });
        V.keys(t, [[T.enter, "Enter"], [T.home, "Shift+Home"], [T.bold, "Ctrl+B"], [T.h1, "Ctrl+1"], [T.undo, "Ctrl+Z"], [T.redo, "Ctrl+Shift+Z"]]);
        camera(cam, V.track(t, [[0, { x: 960, y: 540, s: 1 }], [1.2, { x: 760, y: 440, s: 1.3 }], [10.6, { x: 770, y: 450, s: 1.34 }]]));
      };
    },
  });

  // =====================================================================================
  // Select a paragraph; style Quote; a font, a size, a colour; Ctrl+E centres it.
  V.scene({
    id: "slate-format",
    beats: [{ say: "When you want more, a slim format bar holds the document's own styles, fonts, sizes, and colours. No ribbon.", min: 12.4 }],
    build(root) {
      const styleMenu = `<div class="popover slate-list">${STYLES.map(([n, f, pt, look]) => `<div class="mi" data-label="${n}"><span class="mi-label" style="font-family:${f};font-size:${(pt * 4) / 3}px;${look === "b" ? "font-weight:700;" : ""}${look === "i" ? "font-style:italic;" : ""}">${n}</span><span class="slate-tick">${check}</span></div>`).join("")}</div>`;
      const fontMenu = `<div class="popover slate-list slate-fontlist"><div class="entry focus slate-fsearch">${icon("edit-find")}<span class="slate-etext"></span></div><div class="slate-fl">${FONTS.map((f) => `<div class="mi" data-label="${f}"><span class="mi-label">${f}</span><span class="slate-tick">${check}</span></div>`).join("")}</div></div>`;
      root.innerHTML = `<div class="slate-cam">${V.ui.desktop()}${windowHTML({ id: "slf-win", ...SW })}
        ${colourDialogHTML("slf-colour", SW.x + (SW.w - 560) / 2, SW.y + 50)}
        ${popHTML("slf-styles", styleMenu)}${popHTML("slf-fonts", fontMenu)}</div>`;
      const cam = root.querySelector(".slate-cam");
      const w = bind(root.querySelector("#slf-win"), SW.w);
      const stylesPop = root.querySelector(".slf-styles");
      const fontsPop = root.querySelector(".slf-fonts");
      const fsearch = fontsPop.querySelector(".slate-etext");
      const dlg = root.querySelector("#slf-colour");
      const swatch = dlg.querySelector('[data-c="#1a5fb4"]');
      const select = dlg.querySelector(".slate-select");
      const T = { sel: 1.0, styleOpen: 2.0, quote: 3.0, fontOpen: 3.8, search: 4.2, font: 5.2, plus: [5.9, 6.25, 6.6], colour: 7.3, swatch: 8.1, select: 8.7, centre: 9.6 };
      const p = REPORT[B.thanks].h;
      const scroll = 150;
      return (t) => {
        camera(cam, V.track(t, [[0, { x: 960, y: 540, s: 1 }], [1.6, { x: 860, y: 400, s: 1.22 }], [12.4, { x: 870, y: 410, s: 1.26 }]]));
        const quote = t >= T.quote, calibri = t >= T.font, blue = t >= T.select, centred = t >= T.centre;
        const size = 11 + T.plus.filter((x) => t >= x).length;
        const st = [calibri ? "font-family:Carlito" : "", size > 11 ? `font-size:${((size * 4) / 3).toFixed(2)}px` : "", blue ? "color:#1a5fb4" : "", centred ? "text-align:center" : ""].filter(Boolean).join(";");
        const h = t >= T.sel ? sel(p) : p;
        const blocks = withBlock(REPORT, B.thanks, { k: quote ? "quote" : "p", h: t < T.sel ? caretBefore(p, "volunteers", t) : h, st });
        const fmt = { style: quote ? "Quote" : "Normal", font: calibri ? "Calibri" : "Cambria", size: String(size), i: quote, color: blue ? "#1a5fb4" : "#000000", align: centred ? "center" : "left" };
        draw(w, { blocks, scroll, fmt, title: t >= T.quote ? "• report.docx" : "report.docx" });
        // The drop-downs' lists.
        const [sx, sy] = at(w.styleDD, 0.5, 1);
        const stylesOpen = t >= T.styleOpen && t < T.quote;
        placePop(stylesPop, stylesOpen, sx, sy);
        V.cls(w.styleDD, "on", stylesOpen);
        for (const m of stylesPop.querySelectorAll(".mi")) {
          V.cls(m, "hot", m.dataset.label === "Quote" && t >= T.quote - 0.35);
          V.cls(m, "picked", m.dataset.label === "Normal");
        }
        const [fx, fy] = at(w.fontDD, 0.5, 1);
        const fontsOpen = t >= T.fontOpen && t < T.font;
        const q = V.typed("calib", t, T.search, 10);
        V.markup(fsearch, entryText(q, "", true, t));
        for (const m of fontsPop.querySelectorAll(".mi")) {
          V.show(m, m.dataset.label.toLowerCase().includes(q));
          V.cls(m, "picked", m.dataset.label === "Cambria");
          V.cls(m, "hot", m.dataset.label === "Calibri" && t >= T.font - 0.35);
        }
        placePop(fontsPop, fontsOpen, fx, fy);
        V.cls(w.fontDD, "on", fontsOpen);
        // The colour dialog.
        V.ui.appear(dlg, t, T.colour + 0.1, 0.18);
        if (t >= T.select) V.css(dlg, { display: "none" });
        V.cls(swatch, "on", t >= T.swatch);
        // The pointer: triple-click the paragraph, then the bar's controls.
        const para = w.col.querySelector(`[data-b="${B.thanks}"]`);
        const [ax, ay] = at(para, 0.3, 0.3);
        const quoteEl = findLabel(root, "slf-styles", "Quote");
        const [qx, qy] = at(quoteEl, 0.3, 0.5);
        const calEl = findLabel(root, "slf-fonts", "Calibri");
        const [cx, cy] = calEl.offsetParent ? at(calEl, 0.3, 0.5) : [fx, fy + 90];
        const [plx, ply] = at(w.plus);
        const [kx, ky] = at(w.colorBtn);
        const [wx, wy] = t >= T.colour ? at(swatch) : [kx, ky + 200];
        const [ex, ey] = t >= T.colour ? at(select) : [kx, ky + 100];
        V.pointer(t, [
          [0, 1350, 760], [0.85, ax, ay], ...clicks(T.sel - 0.3, ax, ay, 3, 0.15),
          [T.styleOpen - 0.4, sx, sy - 16], [T.styleOpen, sx, sy - 16, { click: true }],
          [T.quote - 0.35, qx, qy], [T.quote, qx, qy, { click: true }],
          [T.fontOpen - 0.4, fx, fy - 16], [T.fontOpen, fx, fy - 16, { click: true }],
          [T.font - 0.35, cx, cy], [T.font, cx, cy, { click: true }],
          [T.plus[0] - 0.4, plx, ply], ...T.plus.map((x) => [x, plx, ply, { click: true }]),
          [T.colour - 0.4, kx, ky], [T.colour, kx, ky, { click: true }],
          [T.swatch - 0.4, wx, wy], [T.swatch, wx, wy, { click: true }],
          [T.select - 0.4, ex, ey], [T.select, ex, ey, { click: true }],
          [T.select + 0.8, 1400, 720],
        ]);
        V.keys(t, [[T.centre, "Ctrl+E"]]);
      };
    },
  });

  // =====================================================================================
  // Ctrl+F budget: matches in the body and the table; Ctrl+H forecast, Replace All; Ctrl+Z.
  V.scene({
    id: "slate-find",
    beats: [{ say: "Find and replace reaches into tables, and Replace All undoes in one step.", min: 10.4 }],
    build(root) {
      root.innerHTML = `<div class="slate-cam">${V.ui.desktop()}${windowHTML({ id: "slx-win", ...SW, find: true })}</div>`;
      const cam = root.querySelector(".slate-cam");
      const w = bind(root.querySelector("#slx-win"), SW.w);
      const T = { find: 0.5, type: 1.0, replace: 3.0, click: 3.6, type2: 3.9, all: 5.7, undo: 7.5 };
      const scroll = scrollTo(w, B.highlights, 12);
      const replaced = REPORT.map((b) => (b.k === "pic" ? b : b.rows ? { ...b, rows: b.rows.map((r) => r.map((c) => c.replace(/budget/gi, "forecast"))) } : { ...b, h: b.h.replace(/budget/gi, "forecast") }));
      return (t) => {
        camera(cam, V.track(t, [[0, { x: 960, y: 540, s: 1 }], [1.4, { x: 960, y: 470, s: 1.14 }], [10.4, { x: 960, y: 480, s: 1.16 }]]));
        const q = V.typed("budget", t, T.type, 8);
        const swapped = t >= T.all && t < T.undo;
        const doc = swapped ? replaced : REPORT;
        const blocks = withBlock(doc, B.highlights, { h: caret(t, false) + doc[B.highlights].h });
        draw(w, { blocks, scroll, fmt: { style: "Heading 2", font: "Calibri", size: "13", b: true, align: "left" }, find: { q, caretBlock: B.highlights }, title: swapped ? "• report.docx" : "report.docx" });
        const { n, cur } = w.found;
        V.text(w.count, !q ? "" : n === 0 ? "No matches" : `${cur + 1} of ${n}`);
        drawFind(w, V.p(t, T.find, T.find + 0.25, V.ease.out), t >= T.replace ? 2 : 1);
        const inReplace = t >= T.click;
        V.markup(w.ftext, entryText(q, "Find", !inReplace, t));
        V.markup(w.rtext, entryText(V.typed("forecast", t, T.type2, 10), "Replace with", inReplace, t));
        V.cls(w.fentry, "focus", !inReplace);
        V.cls(w.rentry, "focus", inReplace);
        V.keys(t, [[T.find, "Ctrl+F"], [T.replace, "Ctrl+H"], [T.undo, "Ctrl+Z"]]);
        const [rx, ry] = t >= T.replace ? at(w.rentry, 0.3, 0.5) : [1200, 300];
        const [ax, ay] = t >= T.replace ? at(w.rall) : [1500, 300];
        V.pointer(t, [[0, 1460, 780], [T.click - 0.45, rx, ry], [T.click, rx, ry, { click: true }], [T.all - 0.5, ax, ay], [T.all, ax, ay, { click: true }], [T.all + 0.9, 1480, 760]]);
      };
    },
  });

  // =====================================================================================
  // Ctrl+N: Untitled; a heading and a line; Ctrl+S asks where, suggesting Untitled.docx.
  V.scene({
    id: "slate-new",
    beats: [{ say: "Ctrl+N starts a new Word document.", min: 9.6 }],
    build(root) {
      const NW = { x: 260, y: 104, w: 1520, h: 820 };
      root.innerHTML = `<div class="slate-cam">${V.ui.desktop()}${windowHTML({ id: "sln-report", ...SW })}
        ${windowHTML({ id: "sln-win", ...NW, title: "Untitled", blocks: [{ k: "p", h: "" }], fmt: { style: "Normal", font: "Calibri", size: "11", align: "left" } })}
        ${saveDialogHTML("sln-save", NW.x + (NW.w - 1180) / 2, NW.y + 46)}</div>`;
      const cam = root.querySelector(".slate-cam");
      const nwEl = root.querySelector("#sln-win");
      const w = bind(nwEl, NW.w);
      const dlg = root.querySelector("#sln-save");
      const T = { n: 0.4, open: 0.6, h1: 1.3, type: 1.6, enter: 2.6, type2: 2.9, save: 5.8 };
      const head = "Trip notes", line = "Leaving Thursday, back on Monday.";
      return (t) => {
        V.ui.appear(nwEl, t, T.open);
        V.ui.appear(dlg, t, T.save + 0.2);
        const c = t < T.save ? caret(t < T.enter ? keyClock(t, T.type, V.typedEnd(head, T.type, 14)) : keyClock(t, T.type2, V.typedEnd(line, T.type2, 14))) : "";
        let blocks;
        if (t < T.enter) blocks = [{ k: t >= T.h1 ? "bh1" : "bp", h: typing(head, t, T.type) + c }];
        else blocks = [{ k: "bh1", h: head }, { k: "bp", h: typing(line, t, T.type2) + c }];
        const inHead = t < T.enter && t >= T.h1;
        draw(w, { blocks, scroll: 0, title: t >= T.type ? "• Untitled" : "Untitled", fmt: inHead ? { style: "Heading 1", font: "Calibri", size: "16", b: true, align: "left" } : { style: "Normal", font: "Calibri", size: "11", align: "left" } });
        V.keys(t, [[T.n, "Ctrl+N"], [T.h1, "Ctrl+1"], [T.enter, "Enter"], [T.save, "Ctrl+S"]]);
        camera(cam, V.track(t, [[0, { x: 960, y: 540, s: 1 }], [T.open + 0.3, { x: 960, y: 540, s: 1 }], [T.h1 + 0.4, { x: 820, y: 380, s: 1.4 }], [T.save - 0.1, { x: 830, y: 385, s: 1.42 }], [T.save + 0.6, { x: 960, y: 540, s: 1 }]]));
      };
    },
  });

  // =====================================================================================
  // Unsaved edits; kill -9 in a Console; slate again: the start window offers them back
  // (recovery.rs start_section); Restore: • report.docx with the edit.
  V.scene({
    id: "slate-crash",
    beats: [
      { say: "And if Slate ever crashes, your unsaved edits are waiting when you come back.", min: 7.2 },
      { say: "Brick does the same.", min: 3.6 },
    ],
    build(root, s) {
      const edit = { k: "p", h: "Both new sites passed their first fire inspection in September." };
      const edited = insertBlock(REPORT, B.p1 + 1, edit);
      const TW = { x: 1210, y: 470, w: 680, h: 420 };
      const STW = { x: 270, y: 190, w: 900, h: 650 };
      root.innerHTML = `<div class="slate-cam">${V.ui.desktop()}
        ${windowHTML({ id: "slc-win", ...SW, title: "• report.docx", blocks: edited, fmt: FMT.normal })}
        ${V.ui.terminal({ id: "slc-term", ...TW, title: "you@linux: ~" })}
        ${startWindowHTML("slc-start", STW, [{ name: "report.docx", when: "10:23" }])}
        ${windowHTML({ id: "slc-back", ...SW, title: "• report.docx", blocks: edited, fmt: FMT.normal })}</div>`;
      const cam = root.querySelector(".slate-cam");
      const winEl = root.querySelector("#slc-win");
      const w = bind(winEl, SW.w);
      const term = root.querySelector("#slc-term");
      const start = root.querySelector("#slc-start");
      const backEl = root.querySelector("#slc-back");
      const back = bind(backEl, SW.w);
      const restore = start.querySelector(".slate-restore");
      const cmd = "kill -9 $(pgrep -x slate)";
      const T = { type: 0.6 };
      T.enter = Math.max(V.typedEnd(cmd, T.type, 20) + 0.25, s.at(0, "crashes"));
      T.type2 = T.enter + 0.8;
      T.enter2 = V.typedEnd("slate", T.type2, 12) + 0.3;
      T.start = T.enter2 + 0.55;
      T.click = Math.max(T.start + 1.6, s.at(0, "when you come back"));
      T.back = T.click + 0.3;
      const editAt = edited.indexOf(edit);
      // The restored window sits where the killed one was: aim at its edit now, while
      // nothing is scaled or hidden.
      const [ex, ey] = at(w.col.querySelector(`[data-b="${editAt}"]`), 0.4, 0.5);
      const keys = [[0, { x: 960, y: 540, s: 1 }], [T.back + 0.4, { x: 960, y: 540, s: 1 }], [T.back + 1.6, { x: ex, y: ey - 150, s: 1.45 }], [s.dur, { x: ex, y: ey - 146, s: 1.48 }]];
      const [rx, ry] = at(restore);
      return (t) => {
        // The Slate process: its window until the kill, the start window once relaunched,
        // and the restored document after Restore (the start window then closes).
        V.show(winEl, t < T.enter + 0.12);
        V.ui.appear(start, t, T.start);
        if (t >= T.back) V.css(start, { display: "none" });
        V.ui.appear(backEl, t, T.back);
        const withCaret = withBlock(edited, editAt, { h: edit.h + caret(t) });
        draw(w, { blocks: withCaret });
        draw(back, { blocks: t >= T.back ? withCaret : edited });
        const lines = [V.ui.prompt(V.typed(cmd, t, T.type, 20))];
        if (t >= T.enter) lines.push(V.ui.prompt(V.typed("slate", t, T.type2, 12)));
        if (t >= T.enter2) lines.push("");
        V.ui.term(term, lines, t);
        V.keys(t, [[T.enter, "Enter"], [T.enter2, "Enter"]]);
        camera(cam, V.track(t, keys));
        V.pointer(t, [[0, 1560, 860], [T.start + 0.4, 1560, 860], [T.click - 0.15, rx, ry], [T.click, rx, ry, { click: true }], [T.back + 1.0, 1640, 860]]);
      };
    },
  });

  // =====================================================================================
  // An edit, Ctrl+S; then LibreOffice Writer: the edit, plus a header, footer, and pages.
  V.scene({
    id: "slate-docx-save",
    beats: [
      { say: "Here's the part we care most about.", min: 3.6 },
      { say: "When Slate saves a Word document, it changes only what you edited." },
      { say: "Everything else in the file, the parts Slate doesn't even show you, stays exactly as it was.", min: 6.4 },
    ],
    build(root, s) {
      const edit = ", both on time";
      const editedP1 = REPORT[B.p1].h.replace("two new sites.", `two new sites${edit}.`);
      const edited = withBlock(REPORT, B.p1, { h: editedP1 });
      const LW = { x: 90, y: 54, w: 1740, h: 900 };
      root.innerHTML = `<div class="slate-cam">${V.ui.desktop()}${windowHTML({ id: "sld-win", ...SW })}
        ${writerHTML({ id: "sld-lo", ...LW, count: words(edited), docHtml: docHTML(withBlock(edited, B.p1, { h: REPORT[B.p1].h.replace("two new sites.", `two new sites<span class="slate-lo-edit">${edit}</span>.`) })).html })}</div>`;
      const cam = root.querySelector(".slate-cam");
      const w = bind(root.querySelector("#sld-win"), SW.w);
      const lo = root.querySelector("#sld-lo");
      // Split the flow into pages at a block boundary.
      const flows = [...lo.querySelectorAll(".slate-lo-flow")];
      const bodyH = PAGE_H - 2 * MARGIN;
      let cut = bodyH;
      for (const b of flows[0].children) {
        if (b.offsetTop + b.offsetHeight > bodyH) {
          cut = b.offsetTop;
          break;
        }
      }
      V.css(flows[0].parentElement, { height: `${cut}px` });
      V.css(flows[1], { transform: `translateY(${-cut}px)` });
      const typeAt = 0.3, save = 2.0, cutAt = s.b[1].start;
      const editEl = lo.querySelector(".slate-lo-edit");
      const header = lo.querySelector(".slate-lo-header");
      const footer = lo.querySelector(".slate-lo-footer");
      const pt = (el, ax = 0.5, ay = 0.5) => {
        // The point in the camera's unscaled space: stage point, undone by the camera.
        const r = el.getBoundingClientRect(), c = cam.getBoundingClientRect(), k = stageK();
        const sc = c.width / k / V.W;
        return { x: ((r.left - c.left) / k + (r.width / k) * ax) / sc, y: ((r.top - c.top) / k + (r.height / k) * ay) / sc };
      };
      let keys = null;
      return (t) => {
        const typedEnd = V.typedEnd(edit, typeAt, 14);
        const blocks = withBlock(edited, B.p1, { h: REPORT[B.p1].h.replace("two new sites.", `two new sites${typing(edit, t, typeAt)}${caret(keyClock(t, typeAt, typedEnd))}.`) });
        draw(w, { blocks, scroll: 0, fmt: FMT.normal, title: t >= typeAt && t < save + 0.15 ? "• report.docx" : "report.docx" });
        V.keys(t, [[save, "Ctrl+S"]]);
        V.show(lo, t >= cutAt);
        if (t >= cutAt && !keys) {
          // Measure once, with the camera at rest.
          camera(cam, { x: 960, y: 540, s: 1 });
          const e = pt(editEl), hd = pt(header, 0.5, 0.5), ft = pt(footer, 0.5, 0.5);
          keys = [
            [cutAt, { x: 960, y: 540, s: 1 }],
            [cutAt + 0.7, { x: 960, y: 540, s: 1 }],
            [s.at(1, "it changes") , { x: e.x - 40, y: e.y + 10, s: 2.5 }],
            [s.b[2].start + 0.2, { x: e.x - 30, y: e.y + 12, s: 2.6 }],
            [s.at(2, "the parts"), { x: hd.x, y: hd.y + 40, s: 2.2 }],
            [s.at(2, "stays"), { x: hd.x, y: hd.y + 40, s: 2.2 }],
            [s.at(2, "stays") + 1.1, { x: ft.x, y: ft.y - 40, s: 2.2 }],
            [s.b[2].end - 1.0, { x: 960, y: 540, s: 1.02 }],
          ];
        }
        camera(cam, t >= cutAt && keys ? V.track(t, keys) : V.track(t, [[0, { x: 900, y: 360, s: 1.12 }], [3.6, { x: 900, y: 360, s: 1.18 }]]));
      };
    },
  });

  // =====================================================================================
  // notes.md, formatted; one paragraph edited and saved; git diff shows one line.
  V.scene({
    id: "slate-markdown",
    beats: [
      { say: "Markdown and plain text too.", min: 3.2 },
      { say: "Edit one paragraph of a Markdown file, and only that paragraph changes on disk.", min: 7.6 },
      { say: "Your version control will thank you.", min: 4.4 },
    ],
    build(root, s) {
      const MW = { x: 200, y: 70, w: 1480, h: 860 };
      const mdTop = { style: "Heading 1", font: "Liberation Sans", size: "22", b: true, align: "left" };
      root.innerHTML = `<div class="slate-cam">${V.ui.desktop()}${windowHTML({ id: "slm-win", ...MW, title: "notes.md", kind: "md", blocks: NOTES, fmt: mdTop })}
        ${V.ui.terminal({ id: "slm-term", x: 470, y: 330, w: 1280, h: 560, title: "you@linux: ~/notes" })}</div>`;
      const cam = root.querySelector(".slate-cam");
      const winEl = root.querySelector("#slm-win");
      const w = bind(winEl, MW.w);
      const term = root.querySelector("#slm-term");
      const b1 = s.b[1].start;
      const T = { open: 0.15, dbl: b1 + 0.5, type: b1 + 1.1, save: b1 + 2.3, term: b1 + 3.1, cmd: b1 + 3.5, enter: b1 + 4.3 };
      const SHIP = 7;
      const diff = [
        "<b>diff --git a/notes.md b/notes.md</b>",
        "<b>index 4b1d2e7..a93c0f5 100644</b>",
        "<b>--- a/notes.md</b>",
        "<b>+++ b/notes.md</b>",
        '<span class="hunk">@@ -12,7 +12,7 @@</span> What goes into the next release, and who checks it.',
        " cargo test --workspace",
        " ```",
        " ",
        '<span class="del">-Ship on Thursday, after the last review.</span>',
        '<span class="add">+Ship on Friday, after the last review.</span>',
        " ",
        " ## After the release",
        " ",
      ];
      return (t) => {
        camera(cam, V.track(t, [[0, { x: 960, y: 540, s: 1 }], [b1 - 0.4, { x: 960, y: 540, s: 1 }], [T.dbl - 0.1, { x: 820, y: 420, s: 1.38 }], [T.save + 0.3, { x: 820, y: 420, s: 1.4 }], [T.term, { x: 960, y: 540, s: 1 }]]));
        V.ui.appear(winEl, t, T.open);
        const typed = typing("Friday", t, T.type, 10);
        let h = NOTES[SHIP].h;
        const word = (x) => `<span class="slm-word">${x}</span>`;
        if (t < T.dbl) h = caretBefore(h, "Ship", t).replace("Thursday", word("Thursday"));
        else if (t < T.type) h = h.replace("Thursday", word(sel("Thursday")));
        else h = h.replace("Thursday", word(typed) + caret(keyClock(t, T.type, V.typedEnd("Friday", T.type, 10))));
        const blocks = withBlock(NOTES, SHIP, { h });
        draw(w, { blocks, kind: "md", scroll: 0, fmt: t < T.dbl ? mdTop : { style: "Normal", font: "Liberation Sans", size: "11", align: "left" }, title: t >= T.type && t < T.save + 0.15 ? "• notes.md" : "notes.md" });
        V.keys(t, [[T.save, "Ctrl+S"], [T.enter, "Enter"]]);
        V.ui.appear(term, t, T.term);
        const lines = [V.ui.prompt(V.typed("git diff", t, T.cmd, 12), "~/notes")];
        if (t >= T.enter + 0.15) lines.push(...diff, V.ui.prompt("", "~/notes"));
        V.ui.term(term, lines, t);
        const target = w.col.querySelector(".slm-word");
        const [px, py] = t >= T.open ? at(target, 0.5, 0.55) : [900, 500];
        V.pointer(t, [[0, 1300, 760], [T.dbl - 0.5, px, py], ...clicks(T.dbl - 0.16, px, py, 2), [T.dbl + 0.8, px + 60, py + 120], [T.term - 0.2, 1780, 240]]);
      };
    },
  });
})();
