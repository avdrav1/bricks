// 2:50 Frame: editing on the slide and a new deck, a picture into its placeholder and
// shrink to fit, smart guides and the inspector, the outline, and the reference task's
// count with a save and the slideshow.
//
// The Frame window follows apps/frame/src: window.rs (header bar, paned, status),
// slide_list.rs (numbers and 176px thumbnails), slide_view.rs and render.rs (24px canvas
// margin, muted dashed empty placeholders, accent selection with 7px handles, magenta
// guides), inspector.rs (240px, sections and "From the …" captions, 5-column palette),
// outline.rs (titles "N  " bold at 1.25, bullets indented 28px, grey prefixes).
"use strict";

(() => {
  const Z = 1.25; // GNOME at 125%
  const SW = 960, SH = 540; // slide units: points (12192000 × 6858000 EMU)
  const LW = 250, IW = 240, HB = 47, ST = 28; // list, inspector, header bar, status (with its separator)
  const TK = 176 / SW, TH = Math.round(SH * TK); // thumbnails 176 × 99
  const PITCH = 6 + TH + 6 + 2; // a list row and its gap
  const ACCENT = "#3584e4", GUIDE = "#e8308c", MUTED = "rgba(0,0,6,0.6)";
  // deck_edit::FITS: font scale and line reduction, largest first.
  const FITS = [[1, 0], [0.925, 0.1], [0.85, 0.1], [0.775, 0.1], [0.7, 0.2], [0.625, 0.2], [0.55, 0.2], [0.475, 0.2], [0.4, 0.2]];
  // inspector.rs PALETTE.
  const PALETTE = [
    ["Black", "000000"], ["Dark grey", "404040"], ["Grey", "808080"], ["Light grey", "BFBFBF"], ["White", "FFFFFF"],
    ["Red", "C00000"], ["Bright red", "FF0000"], ["Orange", "ED7D31"], ["Gold", "FFC000"], ["Yellow", "FFFF00"],
    ["Light green", "92D050"], ["Green", "00B050"], ["Teal", "008080"], ["Light blue", "00B0F0"], ["Blue", "0070C0"],
    ["Steel blue", "4472C4"], ["Navy", "002060"], ["Purple", "7030A0"], ["Pink", "FF66CC"], ["Brown", "833C0B"],
  ];

  const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;");
  let uidN = 0;
  const uid = () => `fr${++uidN}`;
  const measure = document.createElement("canvas").getContext("2d");
  const textW = (s, font) => ((measure.font = font), measure.measureText(s).width);

  // ---- deck.pptx: "Q3 Field Report" ---------------------------------------------
  const TEAL = "#0e4f55", ACC = "#127c7c", ORANGE = "#e07a3f";
  const foot = { id: "foot", layer: "m", cls: "frame-foot", x: 60, y: 502, w: 520, h: 22, paras: ["Q3 Field Report · Northwest region"] };
  const NEXT_BASE = ["Hire two field leads", "Open the Spokane depot in Q4", "Move the Salem fleet to electric vans before the spring routes", "Review fuel costs every month"];
  const bar = { id: "bar", layer: "m", kind: "rect", x: 60, y: 46, w: 6, h: 58, fill: ACC };
  const title = (t, x = 76) => ({ id: "title", ph: "title", cls: "frame-title", x, y: 40, w: 820, h: 70, paras: [t] });
  const body = (paras, y = 132, h = 356) => ({ id: "body", ph: "body", cls: "frame-body", bullets: true, x: 76, y, w: 810, h, paras });
  const box = (id, t, x, y) => ({ id, kind: "rect", round: true, cls: "frame-boxtx", x, y, w: 220, h: 110, fill: ACC, paras: [t] });
  const deckSlides = () => [
    {
      layout: "Title Slide", bg: TEAL, bgHex: "0E4F55",
      shapes: [
        { id: "ring", layer: "m", kind: "oval", x: 600, y: -150, w: 520, h: 520, fill: "rgba(255,255,255,0.06)" },
        { id: "ring2", layer: "m", kind: "oval", x: 760, y: 300, w: 320, h: 320, fill: "rgba(255,255,255,0.05)" },
        { id: "accent", layer: "m", kind: "rect", x: 74, y: 196, w: 96, h: 6, fill: ORANGE },
        { id: "title", ph: "title", cls: "frame-t1", x: 66, y: 208, w: 828, h: 92, paras: ["Q3 Field Report"] },
        { id: "sub", ph: "body", cls: "frame-s1", x: 66, y: 300, w: 828, h: 48, paras: ["Northwest region · October 2026"] },
      ],
    },
    {
      layout: "Title and Content", bg: "#fff", bgHex: "FFFFFF",
      shapes: [bar, foot, title("Highlights"), body(["Revenue up 12% on Q2", "Three new depots: Tacoma, Salem, and Boise", "Churn down to 4.1%", "Average response time 1.8 days"])],
    },
    {
      layout: "Picture with Caption", bg: "#fff", bgHex: "FFFFFF",
      shapes: [
        foot,
        { id: "title", ph: "title", cls: "frame-title frame-t3", x: 54, y: 60, w: 330, h: 124, paras: ["View from the Tacoma depot"] },
        { id: "pic", ph: "pic", kind: "pic", x: 420, y: 60, w: 480, h: 400 },
        { id: "cap", ph: "body", cls: "frame-cap", x: 54, y: 196, w: 330, h: 250, paras: ["Opened 14 August. The depot serves 1,200 sites from Olympia to Federal Way."] },
      ],
    },
    {
      layout: "Title Only", bg: "#fff", bgHex: "FFFFFF",
      shapes: [bar, foot, title("Where we work"), box("north", "North", 100, 200), box("coast", "Coast", 640, 200), box("inland", "Inland", 404, 342)],
    },
    {
      layout: "Title and Content", bg: "#fff", bgHex: "FFFFFF",
      shapes: [bar, foot, title("Next steps"), body(NEXT_BASE)],
    },
    // Made in the outline (frame-outline).
    { layout: "Title and Content", bg: "#fff", bgHex: "FFFFFF", shapes: [bar, foot, title(""), body([])] },
    { layout: "Title and Content", bg: "#fff", bgHex: "FFFFFF", shapes: [bar, foot, title(""), body([])] },
  ];
  // Frame's built-in blank deck (data/blank.pptx): a Title Slide with an empty centred
  // title and subtitle, in Calibri Light and Calibri.
  const blankSlides = () => [
    {
      layout: "Title Slide", bg: "#fff", bgHex: "FFFFFF",
      shapes: [
        { id: "title", ph: "title", cls: "frame-ctr", x: 120, y: 88.4, w: 720, h: 187.6, paras: [] },
        { id: "sub", ph: "body", cls: "frame-sub", x: 120, y: 283.6, w: 720, h: 130.4, paras: [] },
      ],
    },
  ];
  // Slide 5 before and after frame-picture's typing.
  const PICTURE_BULLETS = ["Roll out the route planner to every crew in the region", "Survey 200 customers in January", "Report back in March"];
  const NEXT_STEPS = [...NEXT_BASE, ...PICTURE_BULLETS];

  // ---- the photo: Mount Rainier over Commencement Bay at dusk ---------------------------
  function photo(fit = "slice") {
    const u = uid();
    return `<svg class="frame-photo" viewBox="0 0 400 300" preserveAspectRatio="xMidYMid ${fit}">
      <defs>
        <linearGradient id="${u}s" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#3f6ea3"/><stop offset=".45" stop-color="#a9a3c0"/><stop offset=".62" stop-color="#f0b48a"/><stop offset=".7" stop-color="#f7d4a0"/></linearGradient>
        <linearGradient id="${u}w" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#e2a985"/><stop offset=".35" stop-color="#8a7f98"/><stop offset="1" stop-color="#25415a"/></linearGradient>
        <linearGradient id="${u}m" x1="0" y1="0" x2="1" y2="0"><stop offset="0" stop-color="#b9a7c4"/><stop offset=".55" stop-color="#8c86ab"/><stop offset="1" stop-color="#6e6d92"/></linearGradient>
      </defs>
      <rect width="400" height="300" fill="url(#${u}s)"/>
      <circle cx="312" cy="196" r="13" fill="#fff1cf" opacity=".85"/>
      <path d="M40 206 L150 118 Q168 100 182 96 Q192 92 204 98 Q220 106 236 122 L360 206Z" fill="url(#${u}m)"/>
      <path d="M128 136 L150 118 Q168 100 182 96 Q192 92 204 98 Q220 106 236 122 L252 138 L238 134 L226 146 L214 130 L204 142 L192 126 L180 140 L168 128 L156 144 L146 132Z" fill="#f6f2f4"/>
      <path d="M0 214 Q50 196 104 206 T214 200 T318 198 T400 196 V224 H0Z" fill="#4c5f78"/>
      <path d="M0 222 L14 204 L20 214 L28 196 L36 214 L44 202 L52 216 L60 206 L72 220 L330 222 L340 208 L348 218 L356 200 L364 216 L374 204 L382 218 L392 206 L400 214 V228 H0Z" fill="#26323f"/>
      <rect y="226" width="400" height="74" fill="url(#${u}w)"/>
      <g stroke="#ffe7c2" stroke-linecap="round" opacity=".7"><path d="M296 236h32M302 244h20M290 252h40M306 262h12" stroke-width="2"/></g>
      <g stroke="#fff" opacity=".18"><path d="M20 250h60M120 268h80M40 284h50M230 278h60" stroke-width="1.5"/></g>
    </svg>`;
  }
  // Files' thumbnail of photo.jpg, as Nautilus draws an image: the picture with a frame.
  const photoThumb = (w = 64) => `<span class="frame-fthumb" style="width:${w}px;height:${(w * 3) / 4}px">${photo()}</span>`;

  // ---- slides ------------------------------------------------------------------------
  const caretHTML = (on) => `<span class="frame-caret${on ? "" : " off"}"></span>`;
  // Paragraphs: strings, or { t, lvl }. caret: { p, on } puts the caret at the end of
  // paragraph p.
  function parasHTML(sh, paras, caret) {
    if (!paras.length) return caret ? `<p class="frame-p">&#8203;${caretHTML(caret.on)}</p>` : "";
    return paras
      .map((para, j) => {
        const o = typeof para === "string" ? { t: para } : para;
        const lvl = o.lvl || 0;
        const bu = sh.bullets ? `<span class="frame-bu">•</span>` : "";
        const c = caret && caret.p === j ? caretHTML(caret.on) : "";
        return `<p class="frame-p l${lvl}">${bu}${esc(o.t) || "&#8203;"}${c}</p>`;
      })
      .join("");
  }
  function shapeHTML(sh) {
    const fill = sh.fill ? `background:${sh.fill};` : "";
    const kind = sh.kind || "text";
    const inner =
      kind === "pic"
        ? `<div class="frame-pic" style="display:none">${photo()}</div>`
        : sh.paras
          ? `<div class="frame-tx">${parasHTML(sh, sh.paras)}</div>`
          : "";
    return `<div class="frame-sh frame-k-${kind}${sh.round ? " round" : ""} ${sh.cls || ""}" data-id="${sh.id}" style="left:${sh.x}px;top:${sh.y}px;width:${sh.w}px;height:${sh.h}px;${fill}">${inner}</div>`;
  }
  const slideHTML = (sl) => `<div class="frame-slide" style="background:${sl.bg}">${sl.shapes.map(shapeHTML).join("")}</div>`;

  // ---- the inspector -----------------------------------------------------------------------
  const icon = (n) => V.ui.icon(n);
  const chip = (hex) => `<i class="frame-chip${hex ? "" : " none"}"${hex ? ` style="background:#${hex}"` : ""}></i>`;
  const I = {
    sec: (heading, rows) => `<div class="frame-sec"><div class="frame-h">${heading}</div>${rows.join("")}</div>`,
    row: (name, control, from = "") => `<div class="frame-irow"><div class="frame-lab">${name}</div>${control}${from ? `<div class="frame-from caption dim-label">${from}</div>` : ""}</div>`,
    drop: (label, pre = "") => `<div class="dropdown frame-dd">${pre}<span class="frame-ddl">${label}</span>${icon("pan-down")}</div>`,
    spin: (v, cls = "") => `<div class="frame-spin ${cls}"><span>${v}</span><b>−</b><b>+</b></div>`,
    swatch: (hex, label, cls = "") => `<div class="hb-btn frame-swatch ${cls}">${chip(hex)}<span>${label}</span></div>`,
    fact: (name, value) => `<div class="frame-fact"><span>${name}</span><span class="v">${value}</span></div>`,
  };
  const layoutThumb = `<span class="frame-lt"><i></i><i></i><i></i></span>`;
  const inspSlide = (sl) =>
    I.sec("Slide", [I.row("Layout", I.drop(sl.layout, layoutThumb)), I.row("Background", I.swatch(sl.bgHex, sl.bgHex), "From the master")]);
  function inspText({ font = "Calibri", size = "18.0", b = false, i = false, u = false, color = "1F2A37", align = 0, bullets = false, from = "" }) {
    const tg = (on, html) => `<button class="hb-btn${on ? " toggled" : ""}">${html}</button>`;
    const aligns = ["format-justify-left", "format-justify-center", "format-justify-right", "format-justify-fill"].map((n, k) => tg(k === align, icon(n))).join("");
    const f = (k) => (typeof from === "string" ? from : from[k] || "");
    return I.sec("Text", [
      I.row("Font", I.drop(font), f(0)),
      I.row("Size", I.spin(size), f(1)),
      I.row("Style", `<div class="linked frame-lk">${tg(b, "<b>B</b>")}${tg(i, "<i>I</i>")}${tg(u, "<u>U</u>")}</div>`, f(2)),
      I.row("Colour", I.swatch(color, color), f(3)),
      I.row("Paragraph", `<div class="frame-para"><div class="linked frame-lk">${aligns}</div>${tg(bullets, icon("view-list-bullet"))}</div>`, f(4)),
    ]);
  }
  function inspShape({ heading = "Shape", fill, fillFrom = "", fillCls = "", outline = null, width = "1.00", outlineFrom = "", facts = [] }) {
    const rows = [];
    if (fill !== undefined) rows.push(I.row("Fill", I.swatch(fill, fill || "No fill", `frame-fill ${fillCls}`), fillFrom));
    rows.push(I.row("Outline", `<div class="frame-oline">${I.swatch(outline, outline || "No outline")}${I.spin(outline ? width : "0.00", "narrow")}</div>`, outlineFrom));
    for (const [n, v] of facts) rows.push(I.fact(n, v));
    return I.sec(heading, rows);
  }
  // The swatch popover: "No fill", then the palette in five columns.
  const swatchPopover = (hot) =>
    `<div class="frame-swpop"><div class="frame-nofill">No fill</div><div class="frame-pal">${PALETTE.map(([n, hex]) => `<div class="frame-pb${n === hot ? " hot" : ""}" data-name="${n}">${chip(hex)}</div>`).join("")}</div><div class="frame-arrow"></div></div>`;

  // ---- the window ------------------------------------------------------------------------
  // Where things are, in the window's own (GTK) pixels.
  function geometry(w, h) {
    const gw = w / Z, gh = h / Z;
    const cw = gw - LW - 1 - 1 - IW, ch = gh - HB - ST;
    const k = Math.min((cw - 48) / SW, (ch - 48) / SH);
    return { gw, gh, cw, ch, k, ox: Math.round((cw - SW * k) / 2), oy: Math.round((ch - SH * k) / 2) };
  }

  function frameHTML({ id = "", x, y, w, h, title = "deck.pptx", slides = deckSlides(), n = 5, cur = 0, insp }) {
    const g = geometry(w, h);
    const rows = slides
      .map(
        (sl, i) =>
          `<div class="frame-row${i === cur ? " sel" : ""}"${i >= n ? ' style="display:none"' : ""}><span class="frame-num">${i + 1}</span><div class="frame-thumb"><div class="frame-scale" style="transform:scale(${TK})">${slideHTML(sl)}</div></div></div>`,
      )
      .join("");
    const canvas = slides
      .map((sl, i) => `<div class="frame-cs" style="transform:scale(${g.k});${i === cur ? "" : "display:none"}">${slideHTML(sl)}</div>`)
      .join("");
    const start =
      V.ui.btn("document-new") +
      V.ui.btn("document-open") +
      `<div class="linked">${V.ui.textBtn("New Slide")}${V.ui.btn("pan-down")}</div>` +
      `<button class="hb-btn text frame-insert">Insert ${icon("pan-down")}</button>` +
      V.ui.btn("document-save", "frame-save");
    const end = V.ui.textBtn("Outline", "frame-otog") + V.ui.btn("media-playback-start");
    const body = `
      <div class="frame-main">
        <div class="frame-list" style="width:${LW}px"><div class="frame-rows">${rows}</div></div>
        <div class="frame-panesep"></div>
        <div class="frame-page" style="width:${g.gw - LW - 1}px">
          <div class="frame-canvas" style="width:${g.cw}px">
            <div class="frame-stage" style="left:${g.ox}px;top:${g.oy}px;width:${SW * g.k}px;height:${SH * g.k}px">${canvas}</div>
            <svg class="frame-ov" width="${g.cw}" height="${g.ch}"></svg>
          </div>
          <div class="frame-isep"></div>
          <div class="frame-insp" style="width:${IW}px">${insp ?? inspSlide(slides[cur])}</div>
          <div class="frame-outline" style="display:none"><div class="frame-otext"></div></div>
        </div>
      </div>
      <div class="sep-h"></div>
      <div class="frame-status">Slide ${cur + 1} of ${n}</div>
      <div class="frame-pop"></div>`;
    return V.ui.win({ id, x, y, w, h, title, start, end, body, cls: "frame-win" });
  }

  // Drives a Frame window built by frameHTML: what the canvas, list, inspector, outline,
  // and status show at a time t. Every setter skips unchanged values.
  function bind(el, { x, y, w, h, slides }) {
    const g = geometry(w, h);
    const rows = [...el.querySelectorAll(".frame-row")];
    const cs = [...el.querySelectorAll(".frame-cs")];
    const inst = slides.map((sl, i) => [cs[i].firstElementChild, rows[i].querySelector(".frame-slide")]);
    const cache = new Map();
    const shapes = (i, id) => {
      const key = `${i}/${id}`;
      if (!cache.has(key)) cache.set(key, inst[i].map((s) => s.querySelector(`[data-id="${id}"]`)));
      return cache.get(key);
    };
    const spec = (i, id) => slides[i].shapes.find((s) => s.id === id);
    const q = (s) => el.querySelector(s);
    const ov = q(".frame-ov"), status = q(".frame-status"), insp = q(".frame-insp"), pop = q(".frame-pop");
    const rowsEl = q(".frame-rows"), outline = q(".frame-outline"), otext = q(".frame-otext");
    const page = [q(".frame-canvas"), q(".frame-isep"), insp];
    const live = slides.map((sl) => Object.fromEntries(sl.shapes.map((s) => [s.id, { paras: s.paras, filled: false, box: { x: s.x, y: s.y, w: s.w, h: s.h } }])));
    let cur = 0;
    const api = {
      el, g, slides, live,
      // Stage pixels of window pixels, of a slide point on the canvas, of thumbnail i.
      gpt: (gx, gy) => [x + gx * Z, y + gy * Z],
      pt: (u, v) => api.gpt(LW + 1 + g.ox + u * g.k, HB + g.oy + v * g.k),
      thumb: (i, scroll = 0) => api.gpt(6 + 4 + 27 + 8 + 88, HB + 6 + i * PITCH + 6 + TH / 2 - scroll),
      // Each frame: the slide shown and selected, the slide count, the list's scroll.
      go(i, n, scroll = 0) {
        cur = i;
        rows.forEach((r, j) => (V.cls(r, "sel", j === i), V.show(r, j < n)));
        cs.forEach((c, j) => V.show(c, j === i));
        V.css(rowsEl, { transform: `translateY(${-scroll}px)` });
        V.text(status, `Slide ${i + 1} of ${n}`);
      },
      status: (s) => V.text(status, s),
      // A shape's text, on the canvas (with the caret) and in its thumbnail.
      paras(i, id, paras, caret) {
        live[i][id].paras = paras;
        const [a, b] = shapes(i, id);
        const sh = spec(i, id);
        V.markup(a.firstElementChild, parasHTML(sh, paras, caret));
        V.markup(b.firstElementChild, parasHTML(sh, paras));
      },
      style(i, id, styles) {
        for (const s of shapes(i, id)) V.css(s, styles);
      },
      move(i, id, bx) {
        live[i][id].box = bx;
        api.style(i, id, { left: `${bx.x}px`, top: `${bx.y}px` });
      },
      fill(i, id, on) {
        live[i][id].filled = on;
        for (const s of shapes(i, id)) V.show(s.firstElementChild, on);
      },
      // Shrink text to fit its box (fit.rs): the largest of FITS that holds it.
      fit(i, id) {
        const [a] = shapes(i, id);
        const tx = a.firstElementChild;
        // A slide the canvas isn't showing has no layout: show it while measuring.
        const shown = cs[i].style.display;
        cs[i].style.display = "";
        let pick = FITS[FITS.length - 1];
        for (const f of FITS) {
          V.css(tx, { "--fit": f[0], "--lr": f[1] });
          if (tx.scrollHeight <= tx.clientHeight + 0.5) {
            pick = f;
            break;
          }
        }
        cs[i].style.display = shown;
        V.css(shapes(i, id)[1].firstElementChild, { "--fit": pick[0], "--lr": pick[1] });
        return pick[0];
      },
      // Stage pixels of the end of paragraph p's text on the canvas, measured (at build
      // time, before any camera moves).
      endOf(i, id, p) {
        const shown = cs[i].style.display;
        cs[i].style.display = "";
        const para = shapes(i, id)[0].querySelectorAll(".frame-p")[p];
        const range = document.createRange();
        range.selectNodeContents(para);
        const rects = [...range.getClientRects()].filter((r) => r.width > 0);
        const r = rects[rects.length - 1];
        const st = document.getElementById("stage").getBoundingClientRect();
        const s = st.width / V.W;
        cs[i].style.display = shown;
        return [(r.right - st.left) / s, (r.top + r.height / 2 - st.top) / s];
      },
      // The canvas's overlay: empty placeholders dashed and muted, the text being edited
      // dashed in the accent, the selection with its handles, and smart guides.
      overlay({ edit = null, sel = null, guides = [] } = {}) {
        const X = (u) => Math.round(g.ox + u * g.k) + 0.5, Y = (v) => Math.round(g.oy + v * g.k) + 0.5;
        const rect = (b, stroke, dash) => `<rect x="${X(b.x)}" y="${Y(b.y)}" width="${Math.round(b.w * g.k)}" height="${Math.round(b.h * g.k)}" fill="none" stroke="${stroke}" stroke-width="1"${dash ? ' stroke-dasharray="4 3"' : ""}/>`;
        let out = "";
        for (const sh of slides[cur].shapes) {
          const L = live[cur][sh.id];
          const empty = sh.ph && (sh.kind === "pic" ? !L.filled : !(L.paras || []).some((p) => (typeof p === "string" ? p : p.t)));
          if (empty && sh.id !== edit) out += rect(L.box, MUTED, true);
        }
        if (edit) out += rect(live[cur][edit].box, ACCENT, true);
        if (sel) {
          const b = live[cur][sel].box;
          out += rect(b, ACCENT, false);
          for (const [fx, fy] of [[0, 0], [0.5, 0], [1, 0], [0, 0.5], [1, 0.5], [0, 1], [0.5, 1], [1, 1]]) {
            const cx = Math.round(g.ox + (b.x + fx * b.w) * g.k), cy = Math.round(g.oy + (b.y + fy * b.h) * g.k);
            out += `<rect x="${cx - 3}" y="${cy - 3}" width="6" height="6" fill="#fff" stroke="${ACCENT}" stroke-width="1"/>`;
          }
        }
        for (const [x1, y1, x2, y2] of guides) {
          out += `<rect x="${Math.round(g.ox + Math.min(x1, x2) * g.k)}" y="${Math.round(g.oy + Math.min(y1, y2) * g.k)}" width="${Math.max(1, Math.round(Math.abs(x2 - x1) * g.k))}" height="${Math.max(1, Math.round(Math.abs(y2 - y1) * g.k))}" fill="${GUIDE}"/>`;
        }
        V.markup(ov, out);
      },
      insp: (html) => V.markup(insp, html),
      pop: (html, styles = {}) => (V.markup(pop, html), V.css(pop, styles)),
      // The outline in place of the canvas and inspector, or not.
      outlineMode(on) {
        for (const p of page) V.show(p, !on);
        V.show(outline, on);
        V.cls(q(".frame-otog"), "toggled", on);
      },
      outline: (html, scroll = 0) => (V.markup(otext, html), V.css(otext, { transform: `translateY(${-scroll}px)` })),
    };
    return api;
  }

  // The outline's lines (deck_edit::outline): titles, body paragraphs, a count of the rest.
  function outlineLines(f, n) {
    const out = [];
    for (let i = 0; i < n; i++) {
      const sl = f.slides[i];
      const titleSh = sl.shapes.find((s) => s.ph === "title");
      const bodySh = sl.shapes.find((s) => s.ph === "body");
      const t = titleSh ? (f.live[i].title.paras[0] ?? "") : "";
      out.push({ kind: "t", n: i + 1, text: typeof t === "string" ? t : t.t });
      if (bodySh) for (const p of f.live[i][bodySh.id].paras) out.push({ kind: "b", lvl: p.lvl || 0, text: typeof p === "string" ? p : p.t });
      const others = sl.shapes.filter((s) => !s.layer && s.id !== titleSh?.id && s.id !== bodySh?.id && !(s.kind === "pic" && !f.live[i][s.id].filled)).length;
      if (others) out.push({ kind: "o", text: others === 1 ? "1 other item" : `${others} other items` });
    }
    return out;
  }
  const OL_TOP = 16, OL_T = 12 + 26, OL_B = 2 + 20; // margins and line heights in the outline
  const olHeight = (lines, upto) => OL_TOP + lines.slice(0, upto + 1).reduce((a, l) => a + (l.kind === "t" ? OL_T : OL_B), 0);
  function outlineHTML(lines, caretLine, on) {
    return lines
      .map((l, j) => {
        const c = j === caretLine ? `<span class="frame-ocaret${on ? "" : " off"}"></span>` : "";
        if (l.kind === "t") return `<div class="frame-ol t"><span class="pre">${l.n}  </span>${esc(l.text)}${c}</div>`;
        if (l.kind === "o") return `<div class="frame-ol o">${l.text}${c}</div>`;
        return `<div class="frame-ol b" style="padding-left:${16 + 28 * (l.lvl + 1)}px"><span class="pre">${["• ", "◦ ", "▪ "][l.lvl % 3]}</span>${esc(l.text)}${c}</div>`;
      })
      .join("");
  }

  // ---- shared scene parts ---------------------------------------------------------------
  // A camera on a wrapper: keys [[t, {s, fx, fy}]] zoom by s about (fx, fy).
  function camera(keys) {
    const at = (t) => V.track(t, keys);
    const map = (t, [px, py]) => {
      const c = at(t);
      return [c.fx + c.s * (px - c.fx), c.fy + c.s * (py - c.fy)];
    };
    const apply = (el, t) => {
      const c = at(t);
      V.css(el, { transformOrigin: `${c.fx}px ${c.fy}px`, transform: `scale(${c.s.toFixed(4)})` });
    };
    // A pointer path in unzoomed stage pixels, through the camera at each key's time.
    const path = (list) => list.map(([tk, px, py, o]) => [tk, ...map(tk, [px, py]), o]);
    return { apply, map, path };
  }
  const textInsp = (o) => inspText(o);

  // Typing paragraphs: Enter, then the line, for each line. Returns the lines typed by t
  // and the Enter presses.
  function typing(lines, t0, cps = 14, gap = 0.35) {
    let at = t0;
    const ev = lines.map((s) => {
      const e = { enter: at, st: at + 0.25, s };
      at = e.st + s.length / cps + gap;
      return e;
    });
    return {
      end: at - gap,
      enters: ev.map((e) => e.enter),
      at: (t) => ev.filter((e) => t >= e.enter).map((e) => V.typed(e.s, t, e.st, cps)),
    };
  }

  // ---- the contract for install-open ---------------------------------------------------------
  V.frame = {
    window: ({ id, x, y, w, h, title = "deck.pptx" }) => {
      const slides = deckSlides().slice(0, 5);
      return frameHTML({ id, x, y, w, h, title, slides, n: 5, cur: 0 });
    },
  };

  // =======================================================================================
  // Open a deck and type into a title; Ctrl+N: a new deck.
  V.scene({
    id: "frame-edit",
    beats: [{ say: "Frame is for presentations. It opens PowerPoint decks, or starts a new one, and you edit right on the slide.", min: 9.6 }],
    build(root) {
      const A = { x: 160, y: 64, w: 1600, h: 860 }, B = { x: 230, y: 112, w: 1600, h: 860 };
      const slides = deckSlides(), blank = blankSlides();
      root.innerHTML = `<div class="frame-cam">${V.ui.desktop()}
        ${frameHTML({ id: "fe-a", ...A, slides, n: 5, cur: 0 })}
        ${frameHTML({ id: "fe-b", ...B, title: "Untitled", slides: blank, n: 1, cur: 0 })}</div>`;
      const cam = root.querySelector(".frame-cam");
      const a = bind(root.querySelector("#fe-a"), { ...A, slides });
      const b = bind(root.querySelector("#fe-b"), { ...B, slides: blank });
      const wb = root.querySelector("#fe-b");

      const tClickThumb = 1.0, tClickTitle = 1.9, tType = 2.3, add = " from the field", cps = 10;
      const tTyped = V.typedEnd(add, tType, cps);
      const tNew = 5.4, tType2 = 6.6, newTitle = "Planning day";
      const [tx, ty] = a.endOf(1, "title", 0);
      const focus = { fx: 560, fy: 300 };
      const cm = camera([[0, { s: 1, ...focus }], [1.5, { s: 1, ...focus }], [2.3, { s: 1.32, ...focus }], [4.5, { s: 1.32, ...focus }], [5.2, { s: 1, ...focus }]]);
      const path = cm.path([
        [0, 1180, 760],
        [0.75, ...a.thumb(1)],
        [tClickThumb, ...a.thumb(1), { click: true }],
        [1.6, tx + 2, ty],
        [tClickTitle, tx + 2, ty, { click: true }],
        [2.25, tx + 220, ty + 190],
        [4.6, tx + 220, ty + 190],
        [5.3, 1300, 820],
      ]);
      const textA = inspText({ size: "36.0", b: true, from: "From the master" });
      const textB = inspText({ font: "Calibri Light", size: "60.0", color: "000000", align: 1, from: ["From the master", "From the layout", "From the master", "From the master", "From the layout"] });

      return (t) => {
        cm.apply(cam, t);
        a.go(t >= tClickThumb ? 1 : 0, 5);
        const editing = t >= tClickTitle;
        a.paras(1, "title", ["Highlights" + V.typed(add, t, tType, cps)], editing && t < tNew ? { p: 0, on: t < tTyped + 0.2 || V.blink(t) } : null);
        a.overlay({ edit: editing ? "title" : null });
        a.insp(editing ? textA : inspSlide(slides[t >= tClickThumb ? 1 : 0]));

        V.ui.appear(wb, t, tNew + 0.08);
        const typed2 = V.typed(newTitle, t, tType2, 9);
        b.go(0, 1);
        b.paras(0, "title", typed2 ? [typed2] : [], { p: 0, on: (t > tType2 && t < V.typedEnd(newTitle, tType2, 9) + 0.2) || V.blink(t - tNew) });
        b.overlay({ edit: "title" });
        b.insp(textB);

        V.pointer(t, path);
        V.keys(t, [[tNew, "Ctrl+N"]]);
      };
    },
  });

  // =======================================================================================
  // Drag a photo from Files into the empty picture placeholder; type bullets until the text
  // shrinks to fit.
  V.scene({
    id: "frame-picture",
    beats: [
      { say: "Drop in a picture, and it lands where the layout wants it.", min: 6.4 },
      // Typing bullets, silent until the text box is about to fill.
      { dur: 5.3 },
      { say: "Too much text shrinks to fit.", min: 3.8 },
    ],
    build(root, s) {
      const F = { x: 280, y: 64, w: 1600, h: 860 };
      const slides = deckSlides();
      slides[1].shapes[2] = title("Highlights from the field");
      root.innerHTML = `<div class="frame-cam">${V.ui.desktop()}
        ${frameHTML({ id: "fp-frame", ...F, slides, n: 5, cur: 2 })}
        ${V.ui.files({ id: "fp-files", x: 40, y: 168, w: 640, h: 470, path: "Pictures", items: [{ name: "Screenshots", ext: "dir" }, { name: "photo.jpg", ext: "jpg" }, { name: "wallpaper.png", ext: "png" }] })}
        <div class="frame-ghost">${photoThumb(72)}</div></div>`;
      const cam = root.querySelector(".frame-cam");
      const f = bind(root.querySelector("#fp-frame"), { ...F, slides });
      const files = root.querySelector("#fp-files");
      const fitem = files.querySelector('.fitem[data-name="photo.jpg"]');
      fitem.querySelector("svg").outerHTML = photoThumb(64);
      const ghost = root.querySelector(".frame-ghost");
      const frameWin = root.querySelector("#fp-frame");
      const [px, py] = V.ui.pos(fitem.querySelector(".frame-fthumb"));

      const b1 = s.b[1].start;
      const tPress = 0.95, tMove = 1.3, tDrop = 3.0;
      const [dx, dy] = f.pt(660, 260);
      const tThumb = b1 - 1.4, tBullet = b1 - 0.6;
      const [lx, ly] = f.endOf(4, "body", NEXT_BASE.length - 1);
      const ty = typing(PICTURE_BULLETS, b1 - 0.1, 18);
      const cm = camera([[0, { s: 1, fx: 1100, fy: 420 }], [b1 - 0.6, { s: 1, fx: 1100, fy: 420 }], [b1 + 0.4, { s: 1.22, fx: 1100, fy: 420 }]]);
      const path = cm.path([
        [0, 1000, 820],
        [0.7, px, py],
        [tPress, px, py, { down: true }],
        [tMove, px + 10, py + 4],
        [tDrop - 0.25, dx, dy],
        [tDrop, dx, dy, { down: false }],
        [tThumb - 0.5, ...f.thumb(4)],
        [tThumb, ...f.thumb(4), { click: true }],
        [tBullet - 0.35, lx + 2, ly],
        [tBullet, lx + 2, ly, { click: true }],
        [tBullet + 0.4, lx + 200, ly + 150],
      ]);
      const picInsp = inspShape({ heading: "Picture", outline: null, facts: [["Format", "JPEG"], ["Pixels", "4032 × 3024"]] });
      const bodyInsp = inspText({ size: "32.0", color: "33414E", bullets: true, from: "From the master" });

      return (t) => {
        cm.apply(cam, t);
        const onFive = t >= tThumb;
        f.go(onFive ? 4 : 2, 5);
        V.css(frameWin, { zIndex: onFive ? 3 : 1 });
        V.css(files, { zIndex: 2 });
        V.ui.selectFile(files, t >= tPress ? "photo.jpg" : "");

        // The drag: the photo's icon follows the pointer from Files to the slide.
        const dragging = t >= tMove && t < tDrop;
        const pos = V.track(t, [[tMove, { x: px + 10, y: py + 4 }], [tDrop - 0.25, { x: dx, y: dy }]]);
        V.css(ghost, { display: dragging ? "" : "none", transform: `translate(${(pos.x - 30).toFixed(1)}px, ${(pos.y - 22).toFixed(1)}px)` });
        const dropped = t >= tDrop;
        f.fill(2, "pic", dropped);

        // Slide 5: Enter, a bullet, again, till the body holds no more at full size.
        const typedNow = t >= tBullet ? ty.at(t) : [];
        const paras = [...NEXT_BASE, ...typedNow];
        const typingNow = t >= tBullet;
        f.paras(4, "body", paras, typingNow ? { p: paras.length - 1, on: t < ty.end + 0.2 || V.blink(t) } : null);
        if (onFive) f.fit(4, "body");

        if (!onFive) {
          f.overlay({ sel: dropped ? "pic" : null });
          f.insp(dropped ? picInsp : inspSlide(slides[2]));
        } else {
          f.overlay({ edit: typingNow ? "body" : null });
          f.insp(typingNow ? bodyInsp : inspSlide(slides[4]));
        }
        V.pointer(t, path);
        V.keys(t, ty.enters.map((e) => [e, "Enter"]));
      };
    },
  });

  // =======================================================================================
  // Drag a shape: smart guides, a snap to the centre; then its fill in the inspector.
  V.scene({
    id: "frame-shapes",
    beats: [
      { say: "Shapes snap into line as you drag them.", min: 5.2 },
      { say: "Every formatting option lives in one inspector. There are no dialogs to dig through.", min: 6.8 },
    ],
    build(root, s) {
      const F = { x: 160, y: 64, w: 1600, h: 860 };
      const slides = deckSlides();
      slides[1].shapes[2] = title("Highlights from the field");
      slides[4].shapes[3] = body(NEXT_STEPS);
      root.innerHTML = `<div class="frame-cam">${V.ui.desktop()}${frameHTML({ id: "fs-frame", ...F, slides, n: 5, cur: 3 })}</div>`;
      const cam = root.querySelector(".frame-cam");
      const f = bind(root.querySelector("#fs-frame"), { ...F, slides });
      f.fit(4, "body");
      f.fill(2, "pic", true);
      const g = f.g;

      // The drag, in slide units: the pointer grabs Inland at its centre.
      const start = { x: 404, y: 342 }, end = { x: 370, y: 200 };
      const tPress = 0.8, tDrag = 1.0, tNear = 2.8, tRelease = 3.5;
      const grab = (bx) => f.pt(bx.x + 110, bx.y + 55);
      const tol = 6 / g.k; // six screen pixels
      // The unsnapped box under the pointer: past the snap, then eased onto it, a few
      // pixels off so the snap shows.
      const raw = (t) =>
        V.track(t, [
          [tDrag, { x: start.x, y: start.y }],
          [tNear - 0.5, { x: end.x + 26, y: end.y + 22 }],
          [tNear, { x: end.x + 4, y: end.y + 3 }],
          [tRelease - 0.15, { x: end.x + 3, y: end.y + 2 }],
        ]);
      const snap = (r) => {
        const guides = [];
        let { x, y } = r;
        if (Math.abs(x + 110 - SW / 2) < tol) {
          x = SW / 2 - 110;
          guides.push([SW / 2, 0, SW / 2, SH]);
        }
        if (Math.abs(y + 55 - 255) < tol) {
          y = 200;
          guides.push([100, 255, 860, 255]);
        }
        return { x, y, guides };
      };
      const b1 = s.b[1].start;
      const tOpen = b1 + 0.9, tHot = b1 + 1.7, tPick = b1 + 2.6;
      // The inspector in window pixels (fixed rows: heading 20, label 20 + 4 + control 34).
      const ix = g.gw - IW, sec2 = HB + 12 + 20 + 10 + 5 * 58 + 4 * 10 + 18;
      const fillY = sec2 + 20 + 10 + 24 + 17;
      const fillPt = f.gpt(ix + 120, fillY);
      // The popover above the Fill swatch: 6 padding, No fill 34, 6, 4 rows of 28 + 2.
      const popH = 6 + 34 + 6 + 4 * 28 + 3 * 2 + 6, popW = 6 + 5 * 38 + 4 * 2 + 6;
      const popTop = fillY - 17 - 10 - popH, popLeft = ix + 120 - popW / 2;
      const orange = f.gpt(popLeft + 6 + 2 * 40 + 19, popTop + 6 + 34 + 6 + 30 + 14);
      const cm = camera([
        [0, { s: 1, fx: 900, fy: 420 }],
        [0.3, { s: 1, fx: 900, fy: 420 }],
        [1.2, { s: 1.15, fx: 900, fy: 420 }],
        [b1 - 0.2, { s: 1.15, fx: 900, fy: 420 }],
        [b1 + 0.6, { s: 1.25, fx: 1700, fy: 520 }],
      ]);
      const path = cm.path([
        [0, 1250, 840],
        [0.6, ...grab(start)],
        [tPress, ...grab(start), { down: true }],
        [tDrag, ...grab(start)],
        ...[tDrag + 0.4, tDrag + 0.9, tNear - 0.5, tNear - 0.2, tNear, tRelease - 0.15].map((tk) => [tk, ...grab(raw(tk))]),
        [tRelease, ...grab(snap(raw(tRelease - 0.15))), { down: false }],
        [b1 + 0.1, ...grab(end)],
        [tOpen - 0.2, ...fillPt],
        [tOpen, ...fillPt, { click: true }],
        [tHot, ...orange],
        [tPick, ...orange, { click: true }],
        [tPick + 0.8, orange[0] - 40, orange[1] + 140],
      ]);
      const shapeInsp = (fill) =>
        inspText({ size: "20.0", b: true, color: "FFFFFF", align: 1 }) + inspShape({ fill, fillCls: "", outline: null, facts: [["Geometry", "roundRect"]] });

      return (t) => {
        cm.apply(cam, t);
        f.go(3, 5);
        const selected = t >= tPress;
        let bx = { x: start.x, y: start.y }, guides = [];
        if (t >= tDrag) {
          const sn = snap(raw(Math.min(t, tRelease - 0.15)));
          bx = { x: sn.x, y: sn.y };
          if (t < tRelease) guides = sn.guides;
        }
        f.move(3, "inland", { ...bx, w: 220, h: 110 });
        const fill = t >= tPick ? "ED7D31" : "127C7C";
        f.style(3, "inland", { background: `#${fill}` });
        f.overlay({ sel: selected ? "inland" : null, guides });
        f.insp(selected ? shapeInsp(fill) : inspSlide(slides[3]));
        V.cls(f.el.querySelector(".frame-fill"), "toggled", t >= tOpen && t < tPick);
        const open = t >= tOpen && t < tPick;
        const hot = t >= tHot ? "Orange" : "";
        f.pop(
          open ? swatchPopover(hot) + (t >= tHot + 0.5 ? `<div class="frame-tip" style="left:${6 + 2 * 40 - 24}px;top:${6 + 34 + 6 + 30 + 28 + 18}px">Orange ED7D31</div>` : "") : "",
          { left: `${popLeft}px`, top: `${popTop - HB}px` },
        );
        V.pointer(t, path);
      };
    },
  });

  // =======================================================================================
  // The outline: a new slide's title, Enter, Tab, three bullets.
  V.scene({
    id: "frame-outline",
    beats: [{ say: "Or skip the slides entirely and just type an outline. Frame builds the slides for you.", min: 12.2 }],
    build(root) {
      const F = { x: 160, y: 64, w: 1600, h: 860 };
      const slides = deckSlides();
      slides[1].shapes[2] = title("Highlights from the field");
      slides[4].shapes[3] = body(NEXT_STEPS);
      root.innerHTML = `<div class="frame-cam">${V.ui.desktop()}${frameHTML({ id: "fo-frame", ...F, slides, n: 5, cur: 4 })}</div>`;
      const f = bind(root.querySelector("#fo-frame"), { ...F, slides });
      f.fit(4, "body");
      f.fit(1, "body");
      f.fill(2, "pic", true);
      const tog = f.el.querySelector(".frame-otog");
      const [ogx, ogy] = V.ui.pos(tog);

      const tToggle = 0.9, tClick = 1.8, tNewSlide = 2.5, tTitle = 2.9, cps = 13;
      const T = "Q4 plan", B = ["Open the Spokane depot", "Hire two field leads", "Ship the route planner"];
      const tEnter = V.typedEnd(T, tTitle, cps) + 0.35;
      const tB1 = tEnter + 0.35, tTab = V.typedEnd(B[0], tB1, cps) + 0.4;
      const tE2 = tTab + 0.6, tB2 = tE2 + 0.3, tE3 = V.typedEnd(B[1], tB2, cps) + 0.35, tB3 = tE3 + 0.3;
      const gh = f.g.ch;
      // Where slide 5's last bullet ends, to click there.
      const base = outlineLines(f, 5);
      const lastY = olHeight(base, base.length - 1) - 10;
      const lastX = 16 + 28 + textW("• " + NEXT_STEPS[NEXT_STEPS.length - 1], "14.67px Inter");
      const [cx, cy] = f.gpt(LW + 1 + lastX + 4, HB + lastY);
      const cm = camera([[0, { s: 1, fx: 520, fy: 760 }], [tClick + 0.3, { s: 1, fx: 520, fy: 760 }], [tClick + 1.4, { s: 1.14, fx: 520, fy: 760 }]]);
      const cam = root.querySelector(".frame-cam");
      const path = cm.path([
        [0, 1250, 760],
        [0.6, ogx, ogy],
        [tToggle, ogx, ogy, { click: true }],
        [tClick - 0.4, cx, cy],
        [tClick, cx, cy, { click: true }],
        [tClick + 0.5, cx + 60, cy + 70],
      ]);

      return (t) => {
        cm.apply(cam, t);
        f.outlineMode(t >= tToggle);
        // Slide 6 (Ctrl+Enter) and 7 (Enter in 6's title), till Tab makes 7's title a bullet.
        let n = 5, cur = 4, caretSlide = 4;
        const title6 = V.typed(T, t, tTitle, cps);
        const b1 = V.typed(B[0], t, tB1, cps);
        let bullets6 = [];
        if (t >= tNewSlide) (n = 6), (cur = 5);
        if (t >= tEnter && t < tTab) (n = 7), (cur = 6);
        if (t >= tTab) {
          bullets6 = [b1];
          if (t >= tE2) bullets6.push(V.typed(B[1], t, tB2, cps));
          if (t >= tE3) bullets6.push(V.typed(B[2], t, tB3, cps));
        }
        caretSlide = cur;
        f.paras(5, "title", t >= tNewSlide ? [title6] : [""]);
        f.paras(5, "body", bullets6);
        f.paras(6, "title", t >= tEnter && t < tTab ? [b1] : [""]);
        const listH = f.g.ch, rowsH = 6 + n * PITCH;
        const scroll = Math.max(0, Math.min(rowsH - listH, 6 + (cur + 1) * PITCH - listH + 4));
        f.go(cur, n, scroll);

        // The outline's text and its caret.
        const lines = outlineLines(f, n);
        let caretLine = -1;
        if (t >= tClick) {
          if (t < tNewSlide) caretLine = lines.length - 1;
          else if (t < tTab) caretLine = lines.findIndex((l) => l.kind === "t" && l.n === caretSlide + 1);
          else caretLine = lines.length - 1;
        }
        const bottom = caretLine >= 0 ? olHeight(lines, caretLine) : 0;
        const oscroll = Math.max(0, bottom + 16 - gh);
        const typingNow = [[tTitle, T], [tB1, B[0]], [tB2, B[1]], [tB3, B[2]]].some(([t0, s]) => t >= t0 && t < V.typedEnd(s, t0, cps) + 0.2);
        f.outline(outlineHTML(lines, caretLine, typingNow || V.blink(t - tClick)), oscroll);
        V.pointer(t, path);
        V.keys(t, [[tNewSlide, "Ctrl+Enter"], [tEnter, "Enter"], [tTab, "Tab"], [tE2, "Enter"], [tE3, "Enter"]]);
      };
    },
  });

  // =======================================================================================
  // The count, then Ctrl+S and F5.
  V.scene({
    id: "frame-count",
    beats: [
      { say: "In our test, building the same four-slide deck took about a quarter fewer clicks and keystrokes than PowerPoint.", min: 8.4 },
      { say: "And like Slate, saving changes only what you edited.", min: 6.6 },
    ],
    build(root, s) {
      const F = { x: 160, y: 64, w: 1600, h: 860 };
      const slides = deckSlides();
      slides[1].shapes[2] = title("Highlights from the field");
      slides[3].shapes[5] = { ...slides[3].shapes[5], x: 370, y: 200, fill: "#ED7D31" };
      slides[4].shapes[3] = body(NEXT_STEPS);
      slides[5].shapes[2] = title("Q4 plan");
      slides[5].shapes[3] = body(["Open the Spokane depot", "Hire two field leads", "Ship the route planner"]);
      root.innerHTML = `
        <div class="frame-back">${V.ui.desktop()}${frameHTML({ id: "fc-frame", ...F, slides, n: 6, cur: 5 })}</div>
        <div class="frame-card">
          <p class="frame-card-label">the same four-slide deck</p>
          <p class="frame-card-big"><span class="frame-n">15</span> actions in Frame <span class="frame-vs">vs</span> <span class="frame-n pp">21</span> in PowerPoint</p>
          <div class="frame-bars">
            <div class="frame-barrow"><span>Frame</span><div class="frame-bar fr"><i></i></div><b>15</b></div>
            <div class="frame-barrow"><span>PowerPoint</span><div class="frame-bar pp"><i></i></div><b>21</b></div>
          </div>
        </div>
        <div class="frame-show"><div class="frame-showslides">${slides.slice(0, 2).map((sl) => `<div class="frame-ss">${slideHTML(sl)}</div>`).join("")}</div></div>`;
      const f = bind(root.querySelector("#fc-frame"), { ...F, slides });
      f.fit(4, "body");
      f.fit(1, "body");
      f.fill(2, "pic", true);
      const show = root.querySelector(".frame-show");
      const ss = [...root.querySelectorAll(".frame-ss")];
      for (const el of ss) {
        el.querySelector('[data-id="pic"]')?.firstElementChild?.style.removeProperty("display");
      }
      // Slide 2's body fits in the show as in the window.
      const fit2 = ss[1].querySelector('[data-id="body"] .frame-tx');
      const fitFrom = f.el.querySelectorAll(".frame-cs")[1].querySelector('[data-id="body"] .frame-tx');
      V.css(fit2, { "--fit": fitFrom.style.getPropertyValue("--fit") || 1, "--lr": fitFrom.style.getPropertyValue("--lr") || 0 });

      const card = root.querySelector(".frame-card");
      const [label, big] = root.querySelectorAll(".frame-card-label, .frame-card-big");
      const bars = [...root.querySelectorAll(".frame-barrow")];
      const fills = [...root.querySelectorAll(".frame-bar i")];
      const b1 = s.b[1].start;
      const tLabel = s.at(0, "the same"), tBig = s.at(0, "took about"), tBars = s.at(0, "a quarter");
      const tSave = b1 + 0.7, tShow = b1 + 2.5, tSpace = b1 + 5.0;
      return (t) => {
        const out = V.p(t, b1 - 0.4, b1 + 0.05, V.ease.linear);
        V.css(card, { opacity: (1 - out).toFixed(3), display: out >= 1 ? "none" : "" });
        V.css(label, { opacity: V.p(t, tLabel - 0.3, tLabel + 0.3), transform: `translateY(${(1 - V.p(t, tLabel - 0.3, tLabel + 0.3)) * 14}px)` });
        const kb = V.p(t, Math.min(tBig, 0.4) , Math.min(tBig, 0.4) + 0.6, V.ease.out);
        V.css(big, { opacity: kb, transform: `translateY(${(1 - kb) * 20}px)` });
        bars.forEach((el, i) => {
          const k = V.p(t, tBars - 0.2 + i * 0.25, tBars + 0.3 + i * 0.25, V.ease.out);
          V.css(el, { opacity: k });
          V.css(fills[i], { width: `${(100 * V.p(t, tBars + i * 0.25, tBars + 1.1 + i * 0.25, V.ease.out)).toFixed(2)}%` });
        });

        f.go(5, 6, 6 + 6 * PITCH - f.g.ch + 4);
        f.overlay({});
        f.insp(inspSlide(slides[5]));
        if (t >= tSave + 0.45) f.status("Saved “deck.pptx”");
        else if (t >= tSave) f.status("Saving “deck.pptx”…");

        const on = t >= tShow;
        V.css(show, { display: on ? "" : "none" });
        ss.forEach((el, i) => V.show(el, i === (t >= tSpace ? 1 : 0)));
        if (t >= b1 - 0.1 && !on) V.pointer(t, [[0, 1500, 780]]);
        V.keys(t, [[tSave, "Ctrl+S"], [tShow, "F5"], [tSpace, "Space"]]);
      };
    },
  });
})();
