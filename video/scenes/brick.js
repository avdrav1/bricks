// 0:00 Cold open and 0:30 Brick. Brick is a plain GTK 4 window: header bar,
// format bar, formula bar, its own grid widget (grid_view.rs), the find bar,
// sheet tabs, and the status bar. One renderer draws it from a state object.
"use strict";

(() => {
  const Z = 1.25; // a 125% desktop
  const ROW_H = 22, COL_W = 120;
  const POOL_ROWS = 26;
  const I = (name, size) => V.ui.icon(name, size);
  const esc = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;");
  const caret = (t, on = true) => `<span class="caret${on && V.blink(t) ? "" : " off"}"></span>`;
  const letters = (c) => {
    let s = "";
    for (let n = c + 1; n > 0; n = Math.floor((n - 1) / 26)) s = String.fromCharCode(65 + ((n - 1) % 26)) + s;
    return s;
  };
  // How a raw value aligns under General (formula::literal): numbers without a
  // leading zero and ISO dates right, true/false centred, the rest left.
  const autoAlign = (s) => {
    if (/^[+-]?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?$/.test(s) || /^\d{4}-\d\d-\d\d$/.test(s)) return "r";
    if (/^(true|false)$/i.test(s)) return "ctr";
    return "";
  };
  // Row-number gutter: the widest row number's digits plus 3 × PAD (grid_view.rs).
  const gutter = (widest) => String(widest).length * 9 + 12;

  // ---- camera ------------------------------------------------------------------
  // A push-in: scale k about a fixed focus point, in stage pixels.
  const camMap = (c, x, y) => [c.fx + (x - c.fx) * c.k, c.fy + (y - c.fy) * c.k];
  const focusFor = (k, [cx, cy], [sx, sy]) => ({ fx: (sx - k * cx) / (1 - k), fy: (sy - k * cy) / (1 - k) });
  const still = { k: 1, fx: 960, fy: 540 };
  function camera(el, c) {
    V.css(el, { transformOrigin: `${c.fx.toFixed(1)}px ${c.fy.toFixed(1)}px`, transform: c.k === 1 ? "none" : `scale(${c.k.toFixed(4)})` });
  }
  // The pointer along a path in unzoomed stage pixels, seen through the camera.
  const pointer = (t, c, path) => V.pointer(t, path.map(([at, x, y, o]) => [at, ...camMap(c, x, y), o]));

  // ---- Brick's window ------------------------------------------------------------
  function brickWindow(host, o) {
    const x = o.x ?? 110, y = o.y ?? 58, w = o.w ?? 1700, h = o.h ?? 820;
    const ncols = o.cols ?? 12;
    const colW = Array.from({ length: ncols }, (_, c) => o.colW?.[c] ?? COL_W);
    const colX = [];
    colW.reduce((acc, cw, c) => ((colX[c] = acc), acc + cw), 0);
    const flat = (name, cls = "") => `<div class="brick-flat ${cls}">${I(name)}</div>`;
    const flatText = (label, arrow) => `<div class="brick-flat text">${label}${arrow ? I("pan-down") : ""}</div>`;
    const sep = '<div class="sep-v"></div>';
    const body = `
      <div class="brick-fmt">
        <div class="dropdown brick-font"><span class="v">Calibri</span>${I("pan-down")}</div>
        <div class="brick-spin"><span class="v">11.0</span><b>−</b><b>+</b></div>
        ${sep}${flat("format-text-bold", "f-b")}${flat("format-text-italic", "f-i")}${flat("format-text-underline", "f-u")}${flat("format-text-strikethrough", "f-s")}
        ${sep}<div class="brick-colorbtn"><i class="f-tc" style="background:#000"></i></div><div class="brick-colorbtn"><i class="f-fc" style="background:#fff"></i></div>${flatText("Borders", true)}
        ${sep}${flat("format-justify-left", "f-l")}${flat("format-justify-center", "f-c")}${flat("format-justify-right", "f-r")}${flatText("Vertical", true)}${flatText("Wrap")}
        ${sep}<div class="dropdown brick-num"><span class="v">General</span>${I("pan-down")}</div>
        ${sep}${flatText("Merge")}${flatText("Unmerge")}${flat("edit-clear")}
      </div>
      <div class="brick-fbar"><div class="entry brick-name"><span class="v"></span></div><span class="dim-label brick-fx">ƒx</span><div class="entry brick-input"><span class="v"></span></div></div>
      <div class="brick-mid">
        <div class="brick-grid${o.xl ? " xl" : ""}">
          <div class="brick-body">
            <div class="brick-rows">${Array.from({ length: POOL_ROWS }, () => `<div class="brick-row"><div class="brick-rn"></div>${colW.map((cw) => `<div class="brick-c" style="width:${cw}px"></div>`).join("")}</div>`).join("")}</div>
            <div class="brick-range"></div><div class="brick-cursor"></div><div class="brick-editor"></div>
          </div>
          <div class="brick-head"><div class="brick-corner"></div>${colW.map((cw) => `<div class="brick-ch" style="width:${cw}px"><span class="ttl"><i></i><span></span></span><span class="tag"></span><span class="flt">▾</span></div>`).join("")}</div>
        </div>
        <div class="brick-vs"><i></i></div>
      </div>
      <div class="brick-hs"><i></i></div>
      <div class="brick-find"><div class="brick-findrow">
        <div class="entry brick-search">${I("edit-find")}<span class="v"></span></div>
        ${flat("go-up")}${flat("go-down")}${flat("edit-find-replace")}
        <span class="check">Match case</span>
        <div class="dropdown brick-scope">Whole file${I("pan-down")}</div>
        <span class="brick-note-l"></span>
        ${flat("window-close")}
      </div></div>
      <div class="brick-tabs"></div>
      <div class="brick-status"><span class="r"></span><span class="s"></span><span class="cancel">Cancel</span><span class="f dim-label"></span></div>`;
    const wrap = document.createElement("div");
    wrap.innerHTML = V.ui.win({
      id: o.id, x, y, w, h, cls: "brick-win",
      start: V.ui.btn("document-open") + V.ui.btn("pan-down") + V.ui.btn("document-new") + V.ui.btn("document-save-as"),
      end: `<button class="hb-btn text brick-hb-toggle">Header row</button><button class="hb-btn brick-hb-dd"><span>Auto</span>${I("pan-down")}</button>`,
      body,
    });
    const el = wrap.firstElementChild;
    host.appendChild(el);
    const menuEl = host.appendChild(V.html(`<div class="brick-menu"><div style="zoom:${Z}"></div></div>`));
    const q = (s) => el.querySelector(s);
    const parts = {
      title: q(".hb-title"), toggle: q(".brick-hb-toggle"), delim: q(".brick-hb-dd"), delimV: q(".brick-hb-dd span"),
      font: q(".brick-font .v"), size: q(".brick-spin .v"), num: q(".brick-num .v"),
      fb: q(".f-b"), fi: q(".f-i"), fu: q(".f-u"), fs: q(".f-s"), fl: q(".f-l"), fc: q(".f-c"), fr: q(".f-r"), tc: q(".f-tc"), fill: q(".f-fc"),
      name: q(".brick-name .v"), input: q(".brick-input .v"), inputEntry: q(".brick-input"),
      grid: q(".brick-grid"), rows: q(".brick-rows"), cursor: q(".brick-cursor"), range: q(".brick-range"), editor: q(".brick-editor"),
      corner: q(".brick-corner"), vs: q(".brick-vs i"), hs: q(".brick-hs i"), vsTrack: q(".brick-vs"),
      find: q(".brick-find"), findRow: q(".brick-findrow"), search: q(".brick-search .v"), searchEntry: q(".brick-search"), note: q(".brick-note-l"),
      tabs: q(".brick-tabs"), sr: q(".brick-status .r"), ss: q(".brick-status .s"), cancel: q(".brick-status .cancel"), sf: q(".brick-status .f"),
    };
    const rowEls = [...el.querySelectorAll(".brick-row")].map((row) => ({ row, rn: row.firstElementChild, cells: [...row.querySelectorAll(".brick-c")] }));
    const heads = [...el.querySelectorAll(".brick-ch")].map((ch) => ({ ch, letter: ch.querySelector("i"), title: ch.querySelector(".ttl span"), ttl: ch.querySelector(".ttl"), tag: ch.querySelector(".tag"), flt: ch.querySelector(".flt") }));
    heads.forEach((hd, c) => V.text(hd.letter, letters(c)));

    function setCell(cell, v) {
      if (v == null) v = "";
      const obj = typeof v === "object" ? v : { v };
      const s = obj.v ?? "";
      const align = obj.align ?? autoAlign(s);
      const cn = `brick-c${align ? " " + align : ""}${obj.b ? " b" : ""}${obj.dim ? " dimc" : ""}`;
      if (cell.__cn !== cn) cell.className = cell.__cn = cn;
      V.text(cell, s);
      V.css(cell, { backgroundColor: obj.fill || "", color: obj.color || "" });
    }

    function set(st) {
      V.text(parts.title, st.title);
      V.cls(parts.toggle, "toggled", !!st.header);
      V.show(parts.delim, !!st.delim);
      V.cls(parts.delim, "insensitive", !!st.delimOff);
      const f = st.fmt || {};
      V.text(parts.font, f.font || "Calibri");
      V.text(parts.size, f.size || "11.0");
      V.text(parts.num, f.number || "General");
      V.cls(parts.fb, "on", !!f.b);
      V.cls(parts.fi, "on", !!f.i);
      V.cls(parts.fu, "on", !!f.u);
      V.cls(parts.fs, "on", !!f.s);
      V.cls(parts.fl, "on", f.align === "l");
      V.cls(parts.fc, "on", f.align === "c");
      V.cls(parts.fr, "on", f.align === "r");
      V.css(parts.tc, { background: f.color || "#000000" });
      V.css(parts.fill, { background: f.fill || "#ffffff" });
      V.text(parts.name, st.name ?? "");
      V.markup(parts.input, st.input ?? "");
      V.cls(parts.inputEntry, "focus", !!st.inputFocus);

      // The grid.
      const gw = st.gw ?? 64;
      V.css(parts.grid, { "--gw": `${gw}px` });
      const top = st.top ?? 0;
      const base = Math.floor(top);
      V.css(parts.rows, { transform: `translateY(${(-(top - base) * ROW_H).toFixed(2)}px)` });
      const cur = st.cursor;
      const rowNum = st.rowNum ?? ((r) => String(r + (st.header ? 2 : 1)));
      rowEls.forEach((re, k) => {
        const r = base + k;
        V.cls(re.row, "odd", r % 2 === 1);
        V.text(re.rn, rowNum(r));
        V.cls(re.rn, "mark", !!cur && cur.r === r);
        re.cells.forEach((cell, c) => setCell(cell, st.cell ? st.cell(r, c) : ""));
      });
      V.text(parts.corner, st.header ? "1" : "");
      heads.forEach((hd, c) => {
        const title = st.titles?.[c] ?? "";
        const tag = st.types?.[c] ?? "";
        V.text(hd.title, title);
        V.text(hd.tag, tag);
        V.cls(hd.tag, "blue", !!st.typeSet?.[c]);
        V.cls(hd.flt, "blue", !!st.filtered?.[c]);
        V.css(hd.ttl, { right: `${tag ? 20 + tag.length * 8.6 + 4 : 20}px` });
        V.cls(hd.ch, "mark", !!cur && cur.c === c);
      });
      const box = (elx, r, c, on) => {
        V.show(elx, on);
        if (on) V.css(elx, { left: `${gw + colX[c]}px`, top: `${((r - top) * ROW_H).toFixed(2)}px`, width: `${colW[c]}px`, height: `${ROW_H}px` });
      };
      box(parts.cursor, cur?.r, cur?.c, !!cur && !st.editor);
      const ed = st.editor;
      box(parts.editor, ed?.r, ed?.c, !!ed);
      if (ed) V.markup(parts.editor, ed.html);
      const rg = st.range;
      V.show(parts.range, !!rg);
      if (rg) V.css(parts.range, { left: `${gw + colX[rg.c0]}px`, top: `${((rg.r0 - top) * ROW_H).toFixed(2)}px`, width: `${colX[rg.c1] + colW[rg.c1] - colX[rg.c0]}px`, height: `${(rg.r1 - rg.r0 + 1) * ROW_H}px` });

      // Scrollbars.
      const vs = st.vs || {};
      const vlen = vs.len ?? 0;
      V.css(parts.vs, { height: `max(40px, ${(vlen * 100).toFixed(2)}%)`, top: `calc((100% - max(40px, ${(vlen * 100).toFixed(2)}%)) * ${(vs.pos ?? 0).toFixed(5)})` });
      V.cls(parts.vs, "held", !!vs.held);
      const hs = st.hs || {};
      V.css(parts.hs, { width: `${((hs.len ?? 0.5) * 100).toFixed(2)}%`, left: "0" });

      // Find bar: a revealer sliding up under the grid.
      const fd = st.find || {};
      const open = fd.open ?? 0;
      V.css(parts.find, { height: `${(42 * open).toFixed(2)}px` });
      V.markup(parts.search, fd.html ?? '<span class="ph">Find</span>');
      V.cls(parts.searchEntry, "focus", !!fd.focus);
      V.text(parts.note, fd.note ?? "");

      // Sheet tabs, status bar.
      V.markup(parts.tabs, (st.tabs || []).map((n, i) => `<div class="brick-tab${i === st.tab ? " on" : ""}${i === st.tabHover && i !== st.tab ? " hover" : ""}">${esc(n)}</div>`).join("") + `<div class="brick-flat">${I("list-add")}</div>`);
      V.text(parts.sr, st.rows ?? "");
      V.text(parts.ss, st.save ?? "");
      V.show(parts.cancel, !!st.cancel);
      V.text(parts.sf, st.format ?? "");

      // The right-click menu, outside the window like a popover.
      const m = st.menu;
      V.show(menuEl, !!m);
      if (m) {
        V.css(menuEl, { left: `${m.x}px`, top: `${m.y}px` });
        V.markup(menuEl.firstElementChild, V.ui.menu(m.items));
      }
    }

    // Stage geometry, measured once laid out (the camera adds its own transform).
    let g0 = null;
    const origin = () => (g0 ??= V.ui.pos(parts.grid, 0, 0));
    const api = {
      el, set, parts, menuEl, colX, colW,
      // A cell's centre in stage pixels, with the grid scrolled to `top`.
      cell(r, c, top = 0, gw = 64, ax = 0.5) {
        const [gx, gy] = origin();
        return [gx + (gw + colX[c] + colW[c] * ax) * Z, gy + (24 + (r - top) * ROW_H + ROW_H / 2) * Z];
      },
      // The vertical scrollbar's thumb centre at `pos` (thumb 40 px long).
      thumb(pos) {
        const [x, y0] = V.ui.pos(parts.vsTrack, 0.5, 0);
        const [, y1] = V.ui.pos(parts.vsTrack, 0.5, 1);
        return [x, y0 + (y1 - y0 - 40 * Z) * pos + 20 * Z];
      },
      at: (sel, ax = 0.5, ay = 0.5) => V.ui.pos(el.querySelector(sel), ax, ay),
      // Visible rows in the grid body right now.
      visibleRows: () => Math.floor((parts.grid.getBoundingClientRect().height / Z - 24) / ROW_H),
    };
    return api;
  }

  // ---- big.csv: scripts/gen_corpus.py's columns, 1 GB, 19.1M rows ----------------
  const N = 19_138_572; // data rows; file rows 2..N+1 under the header
  const NAMES = ["Alice", "David", "Maya", "Omar", "Lena", "Yuki", "Priya", "Sam"];
  const CITIES = ["Boston", "Philly", "Chicago", "Austin", "Denver", "Portland", "Miami"];
  const BIG_TITLES = ["id", "name", "city", "revenue", "score", "date", "active", "code"];
  const BIG_TYPES = ["123", "abc", "abc", "123", "1.5", "date", "T/F", "abc"];
  const pad2 = (n) => String(n).padStart(2, "0");
  function big(i, c) {
    if (i < 0 || i >= N) return "";
    const h = (k) => V.hash(i, k);
    switch (c) {
      case 0: return String(i);
      case 1: return NAMES[Math.floor(h(1) * 8)];
      case 2: return CITIES[Math.floor(h(2) * 7)];
      case 3: return String(Math.floor(h(3) * 100000));
      case 4: return (h(4) * 1000).toFixed(2);
      case 5: return `2026-${pad2(1 + Math.floor(h(5) * 12))}-${pad2(1 + Math.floor(h(6) * 28))}`;
      case 6: return h(7) < 0.5 ? "true" : "false";
      case 7: return String(Math.floor(h(8) * 1000)).padStart(5, "0");
      default: return "";
    }
  }
  const BIG_GW = gutter(N + 2);
  const bigBase = () => ({
    title: "big.csv", header: true, delim: "Auto",
    name: "A2", input: "0",
    top: 0, cell: big, titles: BIG_TITLES, types: BIG_TYPES, gw: BIG_GW,
    cursor: { r: 0, c: 0 }, vs: { pos: 0, len: 0 }, hs: { len: 0.48 },
    find: { open: 0 }, tabs: ["big.csv"], tab: 0,
    rows: `${V.fmt(N)} rows`, save: "", format: "Comma (detected) · UTF-8",
  });
  const nameOf = (r, c, header = true) => `${letters(c)}${r + (header ? 2 : 1)}`;

  // The shot every Brick scene starts from: the desktop, a camera, a window.
  function stageWith(root, extra = "") {
    root.innerHTML = `<div class="brick-cam">${V.ui.desktop()}${extra}</div>`;
    return root.querySelector(".brick-cam");
  }

  // ==== Cold open, after the title card =================================================
  // The open runs in real time; the narrator names the time once the rows are up.
  V.scene({
    id: "cold-open",
    beats: [{ dur: 4.2 }, { say: "That was a one-gigabyte spreadsheet, open in about a third of a second.", min: 6 }],
    build(root, s) {
      const cam = stageWith(root, V.ui.terminal({ id: "brick-co-term", x: 150, y: 150, w: 1080, h: 560, title: "you@linux: ~" }));
      cam.querySelector("#brick-co-term").classList.add("brick-term");
      const w = brickWindow(cam, { id: "brick-co-win" });
      const stat = root.appendChild(V.html('<div class="brick-stat"><b>1 GB</b> CSV open in <b>314 ms</b></div>'));
      const termEl = cam.querySelector("#brick-co-term");
      const cmd = "brick big.csv";
      const typeAt = 1.5, enter = 2.9;
      const windowAt = enter + 0.267; // cold start to window, BENCHMARKS.md
      const rowsAt = enter + 0.314; // open to first rows visible, BENCHMARKS.md
      const indexed = rowsAt + 0.39; // full index
      const statAt = s.at(1, "one-gigabyte");
      return (t) => {
        const lines = [
          V.ui.prompt("ls -lh big.csv"),
          "-rw-r--r-- 1 you you 1.1G Oct 13 10:21 big.csv",
          V.ui.prompt(V.typed(cmd, t, typeAt, 11)),
        ];
        if (t >= enter) lines.push("");
        V.ui.term(termEl, lines, t, t < enter || t >= rowsAt + 3);
        V.keys(t, [[enter, "Enter"]]);
        V.ui.appear(w.el, t, windowAt, 0.12);
        const shown = t >= rowsAt;
        const k = V.p(t, rowsAt, indexed, V.ease.linear);
        const count = Math.floor(N * (0.004 + 0.996 * k));
        w.set({
          ...bigBase(),
          titles: shown ? BIG_TITLES : [],
          types: shown ? BIG_TYPES : [],
          cell: shown ? big : () => "",
          gw: gutter(shown ? count + 2 : 1),
          vs: { pos: 0, len: shown ? Math.min(1, 19 / count) : 1 },
          rows: shown ? `${V.fmt(count)} rows${k < 1 ? ` · indexing ${Math.floor(k * 100)}%` : ""}` : "0 rows",
          format: shown ? "Comma (detected) · UTF-8" : "",
          tabs: ["big.csv"],
        });
        const k2 = V.p(t, statAt, statAt + 0.4, V.ease.out);
        V.css(stat, { opacity: k2, transform: `translate(-50%, ${(1 - k2) * 16}px)` });
      };
    },
  });

  // ==== 0:30 Brick: scrolling 19 million rows ========================================
  V.scene({
    id: "brick-scroll",
    beats: [{ say: "Brick is the spreadsheet. It's built for the files other spreadsheets give up on. This one has nineteen million rows, and scrolling stays smooth.", min: 12.5 }],
    build(root, s) {
      const cam = stageWith(root);
      const w = brickWindow(cam, { id: "brick-sc-win" });
      w.set(bigBase());
      const vis = w.visibleRows();
      const maxTop = N + 2 - vis; // the last data row and the edge row past it in view
      const [tx0, ty0] = w.thumb(0);
      const [, ty1] = w.thumb(1);
      const grab = 5.8, drop = 6.6;
      const wheel0 = 8.3, wheel1 = 11.9;
      const wheelRows = 520;
      const zoom = { k: 1.22, fx: 115, fy: 868 };
      const rest = w.cell(9, 4);
      return (t) => {
        // Rows follow the thumb while it's dragged, then the wheel scrolls up.
        const dragK = V.p(t, grab + 0.1, drop, V.ease.inOut);
        let top = dragK * maxTop;
        if (t > wheel0) top = maxTop - wheelRows * V.p(t, wheel0, wheel1, V.ease.inOut);
        const base = bigBase();
        w.set({ ...base, top, vs: { pos: top / maxTop, len: 0, held: t >= grab && t < drop + 0.25 } });
        const c = { ...zoom, k: V.tw(t, 7.0, 7.9, 1, zoom.k) };
        camera(cam, c);
        pointer(t, c, [
          [0, rest[0], rest[1]],
          [4.9, rest[0], rest[1]],
          [5.65, tx0, ty0],
          [grab, tx0, ty0, { down: true }],
          [grab + 0.1, tx0, ty0],
          [drop, tx0, ty1],
          [drop + 0.25, tx0, ty1, { down: false }],
          [7.6, tx0 - 420, ty1 - 220],
          [wheel1 + 0.2, tx0 - 420, ty1 - 220],
        ]);
      };
    },
  });

  // ==== Sort: right-click, Sort Ascending, 0.82 s for 19.1M rows ======================
  const SORT_MENU = (hot) =>
    [
      ["Insert Rows Above"], ["Insert Rows Below"], ["Delete Rows"], "-",
      ["Insert Columns Left"], ["Insert Columns Right"], ["Delete Columns"], "-",
      ["Sort Ascending"], ["Sort Descending"], "-",
      ["Merge Cells"], ["Unmerge Cells"], "-",
      { label: "Format", sub: true },
    ].map((it) => (Array.isArray(it) ? { label: it[0], hot: it[0] === hot } : it));

  V.scene({
    id: "brick-sort",
    beats: [{ say: "Sorting all nineteen million rows takes under a second.", min: 6.5 }],
    build(root, s) {
      const cam = stageWith(root);
      const w = brickWindow(cam, { id: "brick-so-win" });
      const target = { r: 1, c: 0 }; // id, row 3
      const [cx, cy] = w.cell(target.r, target.c, 0, BIG_GW, 0.35);
      const menuAt = { x: cx, y: cy };
      w.set({ ...bigBase(), menu: { ...menuAt, items: SORT_MENU() } });
      const item = (label) => V.ui.pos(w.menuEl.querySelector(`[data-label="${label}"]`), 0.3, 0.5);
      const pick = item("Sort Descending");
      const delCols = item("Delete Columns");
      const rows = [...w.menuEl.querySelectorAll(".mi")].map((el) => [el.dataset.label, V.ui.pos(el, 0, 0)[1], V.ui.pos(el, 0, 1)[1]]);
      const rightClick = 0.9, click = 2.0;
      const sorted = click + 0.82; // BENCHMARKS.md: 0.82 s for 19.1M rows
      // Push in on the column and the status bar while it sorts.
      const zoom = { k: 1.1, fx: 960, fy: 872 };
      // Sorted by id, descending: the last row's id comes first, its other columns with it.
      const sortedCell = (r, c) => (r < 0 || r >= N ? "" : big(N - 1 - r, c));
      return (t) => {
        const base = bigBase();
        const p = [
          [0, 700, 640],
          [0.75, cx, cy],
          [rightClick, cx, cy, { click: true }],
          [1.35, delCols[0] + 20, delCols[1]],
          [1.8, pick[0], pick[1]],
          [click, pick[0], pick[1], { click: true }],
          [click + 1.2, pick[0] + 30, pick[1] + 40],
        ];
        const c = { ...zoom, k: t < sorted + 1.6 ? V.tw(t, click - 0.3, click + 0.5, 1, zoom.k) : V.tw(t, sorted + 1.6, sorted + 2.4, zoom.k, 1) };
        camera(cam, c);
        pointer(t, c, p);
        const open = t >= rightClick && t < click;
        let hot = null;
        if (open) {
          const py = V.track(t, p.map(([at, , y]) => [at, { y }])).y;
          const px = V.track(t, p.map(([at, x]) => [at, { x }])).x;
          if (px > menuAt.x) hot = rows.find(([, y0, y1]) => py >= y0 && py < y1)?.[0] ?? null;
        }
        const done = t >= sorted;
        const sorting = t >= click && !done;
        const pct = Math.floor(V.p(t, click, sorted, V.ease.linear) * 100);
        w.set({
          ...base,
          title: done ? "• big.csv" : "big.csv",
          delimOff: done,
          cursor: t >= rightClick ? target : base.cursor,
          name: t >= rightClick ? "A3" : base.name,
          input: t >= rightClick ? (done ? sortedCell(1, 0) : big(1, 0)) : base.input,
          cell: done ? sortedCell : big,
          save: sorting ? `Sorting ${pct}%` : done ? "1 unsaved edit" : "",
          menu: open ? { ...menuAt, items: SORT_MENU(hot) } : null,
        });
      };
    },
  });

  // ==== Find: Ctrl+F, portland, counted in the background =============================
  V.scene({
    id: "brick-find",
    beats: [{ say: "Search runs in the background, so the window never freezes.", min: 7 }],
    build(root, s) {
      const cam = stageWith(root);
      const w = brickWindow(cam, { id: "brick-fi-win" });
      w.set(bigBase());
      const vis = w.visibleRows() - 2; // the find bar takes about two rows
      // Portland cells from the top: each match is the city cell of such a row.
      const hits = [];
      for (let i = 0; hits.length < 3; i++) if (big(i, 2) === "Portland") hits.push(i);
      const TOTAL = 2_725_768; // BENCHMARKS.md: `portland`, 2,725,768 cells
      const ctrlF = 0.6, typeAt = 1.1, cps = 9;
      const query = "portland";
      const typedEnd = V.typedEnd(query, typeAt, cps);
      const searchAt = typedEnd + 0.15; // SearchEntry's search-changed delay
      const foundAt = searchAt + 0.224; // BENCHMARKS.md: 224 ms
      const enters = [3.7, 4.5];
      const zoom = { k: 1.1, fx: 960, fy: 872 };
      return (t) => {
        const base = bigBase();
        const open = V.p(t, ctrlF, ctrlF + 0.25, V.ease.out);
        const typed = V.typed(query, t, typeAt, cps);
        let note = "";
        let hit = -1;
        if (t >= searchAt && t < foundAt) {
          const k = V.p(t, searchAt, foundAt, V.ease.linear);
          note = `${V.fmt(Math.max(1, Math.floor(TOTAL * k * (0.85 + 0.15 * V.hash(Math.floor(t * 30))))))} so far…`;
        } else if (t >= foundAt) {
          hit = enters.filter((e) => t >= e).length;
          note = `${hit + 1} of ${V.fmt(TOTAL)}`;
        }
        const row = hit >= 0 ? hits[hit] : 0;
        const top = Math.max(0, row - vis + 1);
        w.set({
          ...base,
          top,
          vs: { pos: top / N, len: 0 },
          cursor: hit >= 0 ? { r: row, c: 2 } : base.cursor,
          name: hit >= 0 ? nameOf(row, 2) : base.name,
          input: hit >= 0 ? "Portland" : base.input,
          find: { open, focus: t >= ctrlF, html: t >= ctrlF ? (typed ? esc(typed) : "") + caret(t) : undefined, note },
        });
        V.keys(t, [[ctrlF, "Ctrl+F"], ...enters.map((e) => [e, "Enter"])]);
        camera(cam, { ...zoom, k: V.tw(t, 1.0, 2.2, 1, zoom.k) });
      };
    },
  });

  // ==== Raw values: 00123 stays 00123; an atomic save ===================================
  V.scene({
    id: "brick-raw",
    beats: [
      { say: "Your data stays your data. `00123` stays `00123`. Brick never quietly turns it into a number." },
      { say: "And every save is atomic: a crash can't leave you with half a file.", min: 7.5 },
    ],
    build(root, s) {
      const cam = stageWith(root);
      const w = brickWindow(cam, { id: "brick-ra-win" });
      w.set(bigBase());
      const code = { r: 4, c: 7 }; // H6
      const edit = { r: 2, c: 1 }; // B4
      const cell = (r, c) => (r === code.r && c === code.c ? "00123" : big(r, c));
      const [hx, hy] = w.cell(code.r, code.c);
      const [ex, ey] = w.cell(edit.r, edit.c);
      const K = 2.3;
      // Frame the cell and its column's header (the "abc" tag) in the middle of the screen.
      const zoomed = { k: K, ...focusFor(K, [hx, hy - 70], [960, 430]) };
      const clickCode = 0.5;
      const zoomIn = [0.9, 2.4], zoomOut = [s.b[1].start - 0.9, s.b[1].start + 0.1];
      const b1 = s.b[1].start;
      const clickEdit = b1 + 0.5, typeAt = b1 + 0.8, word = "Omar", enter = b1 + 1.75;
      const save = Math.max(b1 + 2.6, s.at(1, "atomic"));
      const saved = save + 0.77; // BENCHMARKS.md: a 1 GB save in the app, 0.77 s
      const checking = save + 0.6;
      return (t) => {
        const base = bigBase();
        const k = t < zoomOut[0] ? V.tw(t, zoomIn[0], zoomIn[1], 1, K) : V.tw(t, zoomOut[0], zoomOut[1], K, 1);
        const c = { ...zoomed, k };
        camera(cam, c);
        pointer(t, c, [
          [0, hx - 160, hy + 160],
          [clickCode - 0.05, hx, hy],
          [clickCode, hx, hy, { click: true }],
          [zoomIn[0], hx + 30, hy + 50],
          [b1 - 0.2, hx + 30, hy + 50],
          [clickEdit, ex, ey],
          [clickEdit + 0.01, ex, ey, { click: true }],
          [b1 + 1.6, ex + 40, ey + 60],
        ]);
        const editing = t >= typeAt && t < enter;
        const edited = t >= enter;
        const dirty = edited && t < saved;
        let cursor = base.cursor, name = base.name, input = base.input;
        if (t >= clickCode) (cursor = code), (name = "H6"), (input = "00123");
        if (t >= clickEdit) (cursor = edit), (name = "B4"), (input = editing ? esc(V.typed(word, t, typeAt, 8)) : big(edit.r, edit.c));
        if (edited) (cursor = { r: edit.r + 1, c: edit.c }), (name = "B5"), (input = big(edit.r + 1, edit.c));
        let status = "";
        if (dirty && t < save) status = "1 unsaved edit";
        else if (t >= save && t < checking) status = `Saving ${Math.floor(V.p(t, save, checking, V.ease.linear) * 100)}%`;
        else if (t >= checking && t < saved) status = "Checking the saved file…";
        else if (t >= saved) status = "Saved in 0.8 s";
        w.set({
          ...base,
          title: dirty ? "• big.csv" : "big.csv",
          delimOff: dirty,
          cell: (r, cc) => (edited && r === edit.r && cc === edit.c ? word : cell(r, cc)),
          cursor, name, input,
          editor: editing ? { ...edit, html: esc(V.typed(word, t, typeAt, 8)) + caret(t) } : null,
          save: status,
          cancel: t >= save && t < saved,
        });
        V.keys(t, [[enter, "Enter"], [save, "Ctrl+S"]]);
      };
    },
  });

  // ==== Past the data: a three-row CSV, E40 ==============================================
  V.scene({
    id: "brick-past-data",
    beats: [{ say: "Past your data, the sheet keeps going, like Excel and Calc. Type anywhere, and the rows are there.", min: 10 }],
    build(root, s) {
      const cam = stageWith(root);
      const w = brickWindow(cam, { id: "brick-pd-win" });
      const titles = ["name", "email", "city", "phone"];
      const data = [
        ["Ada Lovelace", "ada@acme.io", "London", "020 7946 0018"],
        ["Grace Hopper", "grace@acme.io", "Arlington", "703 555 0142"],
        ["Linus Torvalds", "linus@acme.io", "Portland", "503 555 0199"],
      ];
      const target = { r: 38, c: 4 }; // E40: file row 40 under the header
      const top1 = 22;
      const base = {
        title: "contacts.csv", header: true, delim: "Auto",
        titles, types: ["abc", "abc", "abc", "abc"], gw: gutter(99),
        vs: { pos: 0, len: 0.5 }, hs: { len: 0.48 }, find: { open: 0 }, tabs: ["contacts.csv"], tab: 0,
        rows: "3 rows", format: "Comma (detected) · UTF-8",
      };
      w.set(base);
      const [ex, ey] = w.cell(target.r, target.c, top1, base.gw);
      const scroll0 = 1.2, scroll1 = 2.8;
      const click = s.at(0, "Type anywhere") - 0.35, typeAt = click + 0.35, text = "Call back", enter = typeAt + 1.15;
      const undo = enter + 2.4;
      const zoom = { k: 1.2, fx: 115, fy: 868 };
      return (t) => {
        const top = V.tw(t, scroll0, scroll1, 0, top1, V.ease.inOut);
        const editing = t >= typeAt && t < enter;
        const added = t >= enter && t < undo;
        const typedText = V.typed(text, t, typeAt, 10);
        let cursor = { r: 0, c: 0 }, name = "A2", input = data[0][0];
        if (t >= click) (cursor = target), (name = "E40"), (input = editing ? esc(typedText) : "");
        if (t >= enter) (cursor = { r: target.r + 1, c: target.c }), (name = "E41"), (input = "");
        w.set({
          ...base,
          title: added ? "• contacts.csv" : "contacts.csv",
          delimOff: added,
          top,
          vs: { pos: top / (top1 + 19), len: added ? 0.45 : 0.5 },
          cell: (r, c) => (added && r === target.r && c === target.c ? text : data[r]?.[c] ?? ""),
          types: added ? ["abc", "abc", "abc", "abc", "abc"] : base.types,
          cursor, name, input,
          editor: editing ? { ...target, html: esc(typedText) + caret(t) } : null,
          rows: added ? "39 rows" : "3 rows",
          save: added ? "1 unsaved edit" : "",
        });
        const c = { ...zoom, k: V.tw(t, enter + 0.2, enter + 1.0, 1, zoom.k) * 1 };
        if (t > undo + 0.9) c.k = V.tw(t, undo + 0.9, undo + 1.6, zoom.k, 1);
        camera(cam, c);
        pointer(t, c, [
          [0, 760, 520],
          [scroll1 + 0.3, 760, 520],
          [click - 0.05, ex, ey],
          [click, ex, ey, { click: true }],
          [enter + 0.6, ex + 60, ey - 120],
        ]);
        V.keys(t, [[enter, "Enter"], [undo, "Ctrl+Z"]]);
      };
    },
  });

  // ==== budget.xlsx: sheets, styles, formulas ===========================================
  const BUDGET_ROWS = [
    ["Rent", 4500, 4500, 4500, 13500],
    ["Salaries", 18200, 18200, 18900, 55000],
    ["Software", 1240, 1310, 1295, 3900],
    ["Travel", 860, 1920, 1100, 4000],
    ["Marketing", 2100, 2750, 3400, 8000],
    ["Utilities", 410, 385, 362, 1200],
    ["Insurance", 640, 640, 640, 1920],
    ["Supplies", 295, 340, 218, 900],
  ];
  const BUDGET_HEAD = ["Category", "Jan", "Feb", "Mar", "Q1", "Budget"];
  const money = (n) => n.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  // The workbook's values with Travel's February set to `feb`.
  function budget(feb) {
    const rows = BUDGET_ROWS.map((r) => [...r]);
    rows[3][2] = feb;
    const q1 = rows.map((r) => r[1] + r[2] + r[3]);
    const tot = [1, 2, 3].map((m) => rows.reduce((a, r) => a + r[m], 0));
    return { rows, q1, tot, q1tot: q1.reduce((a, b) => a + b, 0), btot: rows.reduce((a, r) => a + r[4], 0) };
  }
  const HEAD_FILL = "#ddebf7", TOTAL_FILL = "#f2f2f2";

  V.scene({
    id: "brick-xlsx",
    beats: [{ say: "It opens Excel workbooks too, with formulas, multiple sheets, and cell styles.", min: 10.5 }],
    build(root, s) {
      const cam = stageWith(root);
      const colW = [150, 110, 110, 110, 110, 110];
      const w = brickWindow(cam, { id: "brick-xl-win", colW, xl: true });
      const base = {
        title: "budget.xlsx", header: false, delim: null,
        name: "A1", input: "Category", gw: gutter(99),
        vs: { pos: 0, len: 0.5 }, hs: { len: 0.5 }, find: { open: 0 },
        tabs: ["Budget", "Income", "Staff", "Notes"], tab: 0,
        rows: "10 rows", format: "Excel workbook",
      };
      // A cell's value, formula, and look.
      function cellAt(b, r, c) {
        if (r === 0) return c < 6 ? { v: BUDGET_HEAD[c], b: true, fill: HEAD_FILL, align: c ? "r" : "", f: BUDGET_HEAD[c], look: { b: true, fill: HEAD_FILL } } : null;
        if (r >= 1 && r <= 8) {
          const row = b.rows[r - 1];
          const n = r + 1;
          if (c === 0) return { v: row[0], f: row[0] };
          if (c >= 1 && c <= 3) return { v: money(row[c]), align: "r", f: String(row[c]), num: true };
          if (c === 4) return { v: money(b.q1[r - 1]), align: "r", f: `=SUM(B${n}:D${n})`, num: true };
          if (c === 5) return { v: money(row[4]), align: "r", f: String(row[4]), num: true };
        }
        if (r === 9) {
          const look = { b: true, fill: TOTAL_FILL };
          if (c === 0) return { v: "Total", ...look, f: "Total", look };
          if (c >= 1 && c <= 3) return { v: money(b.tot[c - 1]), align: "r", ...look, f: `=SUM(${letters(c)}2:${letters(c)}9)`, num: true, look };
          if (c === 4) return { v: money(b.q1tot), align: "r", ...look, f: "=SUM(E2:E9)", num: true, look };
          if (c === 5) return { v: money(b.btot), align: "r", ...look, f: "=SUM(F2:F9)", num: true, look };
        }
        return null;
      }
      w.set(base);
      const sumCell = { r: 2, c: 4 }, headCell = { r: 0, c: 0 }, edit = { r: 4, c: 2 };
      const P = (cl) => w.cell(cl.r, cl.c, 0, base.gw);
      const [sx, sy] = P(sumCell), [hx, hy] = P(headCell), [ex, ey] = P(edit);
      const tabIncome = V.ui.pos(w.parts.tabs.children[1]);
      const clickSum = s.at(0, "formulas") - 0.25;
      const hoverTabs = s.at(0, "multiple sheets") - 0.3;
      const clickHead = s.at(0, "cell styles") - 0.25;
      const clickEdit = clickHead + 1.5, typeAt = clickEdit + 0.35, value = "2400", enter = typeAt + 0.75;
      const save = enter + 1.8, saved = save + 0.12;
      const K = 1.4;
      const zoomed = { k: K, ...focusFor(K, [565, 395], [960, 430]) };
      return (t) => {
        const editing = t >= typeAt && t < enter;
        const b = budget(t >= enter ? 2400 : 1920);
        let cur = { r: 0, c: 0 };
        if (t >= clickSum) cur = sumCell;
        if (t >= clickHead) cur = headCell;
        if (t >= clickEdit) cur = edit;
        if (t >= enter) cur = { r: edit.r + 1, c: edit.c };
        const info = cellAt(b, cur.r, cur.c) || {};
        const look = info.look || {};
        const dirty = t >= enter && t < saved;
        let status = "";
        if (dirty && t < save) status = "1 unsaved edit";
        else if (t >= save && t < saved) status = `Saving ${Math.floor(V.p(t, save, saved, V.ease.linear) * 100)}%`;
        else if (t >= saved) status = "Saved in 0.1 s";
        const typedText = V.typed(value, t, typeAt, 9);
        w.set({
          ...base,
          title: dirty ? "• budget.xlsx" : "budget.xlsx",
          cell: (r, c) => cellAt(b, r, c),
          cursor: cur,
          name: nameOf(cur.r, cur.c, false),
          input: editing ? esc(typedText) : esc(info.f ?? ""),
          editor: editing ? { ...edit, html: esc(typedText) + caret(t) } : null,
          fmt: { b: !!look.b, fill: look.fill, number: info.num ? "Number" : "General" },
          tabHover: t >= hoverTabs + 0.4 && t < clickHead - 0.5 ? 1 : -1,
          save: status,
          cancel: t >= save && t < saved,
        });
        const k = t < save + 0.6 ? V.tw(t, clickEdit - 0.6, clickEdit + 0.2, 1, K) : V.tw(t, save + 0.6, save + 1.3, K, 1);
        const c = { ...zoomed, k };
        camera(cam, c);
        pointer(t, c, [
          [0, 900, 620],
          [clickSum - 0.05, sx, sy],
          [clickSum, sx, sy, { click: true }],
          [hoverTabs + 0.4, tabIncome[0], tabIncome[1]],
          [clickHead - 0.5, tabIncome[0], tabIncome[1]],
          [clickHead - 0.05, hx, hy],
          [clickHead, hx, hy, { click: true }],
          [clickEdit - 0.05, ex, ey],
          [clickEdit, ex, ey, { click: true }],
          [enter + 0.5, ex + 70, ey + 90],
        ]);
        V.keys(t, [[enter, "Enter"], [save, "Ctrl+S"]]);
      };
    },
  });

  // ==== The same workbook in LibreOffice Calc ===========================================
  function loIcon(kind) {
    const s = {
      new: '<path d="M4 2h6l3 3v9H4z" fill="#fff" stroke="#5a5a5a"/><path d="M10 2v3h3" fill="none" stroke="#5a5a5a"/>',
      open: '<path d="M1.5 4h5l1 1.5H14V13H1.5z" fill="#f4b740" stroke="#a4741c"/>',
      save: '<rect x="2" y="2" width="12" height="12" rx="1" fill="#3a76c4"/><rect x="4.5" y="2.5" width="7" height="4" fill="#fff"/>',
      pdf: '<path d="M4 2h6l3 3v9H4z" fill="#fff" stroke="#c9302c"/><text x="8.5" y="12" text-anchor="middle" font-size="5" font-family="Inter" font-weight="700" fill="#c9302c">PDF</text>',
      print: '<rect x="4" y="2" width="8" height="4" fill="#fff" stroke="#5a5a5a"/><rect x="2" y="6" width="12" height="6" rx="1" fill="#6d6d6d"/><rect x="4.5" y="10" width="7" height="4" fill="#fff" stroke="#5a5a5a"/>',
      cut: '<circle cx="5" cy="12" r="2" fill="none" stroke="#5a5a5a" stroke-width="1.4"/><circle cx="11" cy="12" r="2" fill="none" stroke="#5a5a5a" stroke-width="1.4"/><path d="M6 10.5L11 2M10 10.5L5 2" stroke="#5a5a5a" stroke-width="1.3"/>',
      copy: '<rect x="2" y="2" width="8" height="9" fill="#fff" stroke="#5a5a5a"/><rect x="6" y="5" width="8" height="9" fill="#fff" stroke="#5a5a5a"/>',
      paste: '<rect x="3" y="3" width="10" height="11" rx="1" fill="#c8a165" stroke="#8a6b3a"/><rect x="5" y="2" width="6" height="3" rx="1" fill="#5a5a5a"/>',
      undo: '<path d="M5 6h5a3 3 0 0 1 0 6H6" fill="none" stroke="#3a76c4" stroke-width="1.6"/><path d="M2 6l3.5-3v6z" fill="#3a76c4"/>',
      redo: '<path d="M11 6H6a3 3 0 0 0 0 6h4" fill="none" stroke="#3a76c4" stroke-width="1.6"/><path d="M14 6l-3.5-3v6z" fill="#3a76c4"/>',
      grid: '<rect x="2" y="2" width="12" height="12" fill="#fff" stroke="#5a5a5a"/><path d="M2 6h12M2 10h12M6 2v12M10 2v12" stroke="#5a5a5a" stroke-width=".8"/>',
      sortA: '<text x="2" y="8" font-size="6" font-family="Inter" font-weight="700" fill="#5a5a5a">A</text><text x="2" y="15" font-size="6" font-family="Inter" font-weight="700" fill="#5a5a5a">Z</text><path d="M11 2v11M8.5 10.5L11 13.5l2.5-3" fill="none" stroke="#3a76c4" stroke-width="1.4"/>',
      filter: '<path d="M2 3h12l-4.5 5v5l-3-1.5V8z" fill="#5a5a5a"/>',
      chart: '<rect x="2" y="8" width="3" height="6" fill="#004586"/><rect x="6.5" y="4" width="3" height="10" fill="#ff420e"/><rect x="11" y="6" width="3" height="8" fill="#ffd320"/>',
      pivot: '<rect x="2" y="2" width="12" height="12" fill="#fff" stroke="#5a5a5a"/><rect x="2" y="2" width="12" height="3" fill="#3a76c4"/><rect x="2" y="2" width="3" height="12" fill="#3a76c4"/>',
      omega: '<text x="8" y="13" text-anchor="middle" font-size="12" font-family="Caladea" fill="#5a5a5a">Ω</text>',
      link: '<path d="M6.5 9.5l3-3M5 8l-1.5 1.5a2.1 2.1 0 0 0 3 3L8 11M11 8l1.5-1.5a2.1 2.1 0 0 0-3-3L8 5" fill="none" stroke="#3a76c4" stroke-width="1.5"/>',
      note: '<path d="M2 2h12v9H7l-3 3v-3H2z" fill="#ffe17a" stroke="#a48a2c"/>',
      bold: '<text x="8" y="13" text-anchor="middle" font-size="12" font-family="Inter" font-weight="800" fill="#333">B</text>',
      italic: '<text x="8" y="13" text-anchor="middle" font-size="12" font-family="Caladea" font-style="italic" font-weight="700" fill="#333">I</text>',
      under: '<text x="8" y="11.5" text-anchor="middle" font-size="11" font-family="Inter" font-weight="600" fill="#333">U</text><path d="M3.5 14h9" stroke="#333"/>',
      fontc: '<text x="8" y="11" text-anchor="middle" font-size="11" font-family="Inter" font-weight="700" fill="#333">A</text><rect x="2" y="12.5" width="12" height="2.5" fill="#c9211e"/>',
      bg: '<path d="M4 3l6 6-3 3-5-5z" fill="#fff" stroke="#5a5a5a"/><rect x="2" y="12.5" width="12" height="2.5" fill="#ffff00"/>',
      left: '<path d="M2 3h12M2 6.3h8M2 9.6h12M2 13h8" stroke="#333" stroke-width="1.3"/>',
      center: '<path d="M2 3h12M4 6.3h8M2 9.6h12M4 13h8" stroke="#333" stroke-width="1.3"/>',
      right: '<path d="M2 3h12M6 6.3h8M2 9.6h12M6 13h8" stroke="#333" stroke-width="1.3"/>',
      cur: '<text x="8" y="12.5" text-anchor="middle" font-size="11" font-family="Inter" font-weight="700" fill="#2a7d2e">$</text>',
      pct: '<text x="8" y="12.5" text-anchor="middle" font-size="11" font-family="Inter" font-weight="700" fill="#333">%</text>',
      num: '<text x="8" y="12" text-anchor="middle" font-size="7.5" font-family="Inter" font-weight="700" fill="#333">0.0</text>',
      sum: '<text x="8" y="13" text-anchor="middle" font-size="13" font-family="Inter" fill="#333">Σ</text>',
      eq: '<text x="8" y="13" text-anchor="middle" font-size="13" font-family="Inter" fill="#333">=</text>',
    }[kind];
    return `<svg width="18" height="18" viewBox="0 0 16 16">${s}</svg>`;
  }

  V.scene({
    id: "brick-libreoffice",
    beats: [{ say: "When you save, Brick patches the original file rather than rewriting it, so charts, comments, and conditional formatting come through untouched.", min: 10 }],
    build(root, s) {
      const cam = stageWith(root);
      const X = 110, Y = 58, W = 1700, H = 820;
      const colW = [150, 110, 110, 110, 110, 110, 85, 85, 85, 85, 85, 85, 85, 85];
      const colX = [];
      colW.reduce((acc, cw, c) => ((colX[c] = acc), acc + cw), 0);
      const RH = 20, RHW = 40, CHH = 21;
      const b = budget(2400);
      const cells = [];
      const at = (r, c) => `left:${RHW + colX[c]}px;top:${CHH + r * RH}px;width:${colW[c]}px;height:${RH}px`;
      const put = (r, c, v, cls = "", style = "", extra = "") => cells.push(`<div class="brick-lo-cell ${cls}" style="${at(r, c)};${style}">${esc(v)}${extra}</div>`);
      for (let c = 0; c < colW.length; c++) for (let r = 0; r < 34; r++) if (!(r <= 9 && c <= 5)) put(r, c, "");
      BUDGET_HEAD.forEach((v, c) => put(0, c, v, `b${c ? " r" : ""}`, `background:${HEAD_FILL}`));
      const ind = '<span class="ind"></span>';
      b.rows.forEach((row, i) => {
        const r = i + 1;
        put(r, 0, row[0], "", "", r === 8 ? ind : "");
        for (let c = 1; c <= 3; c++) put(r, c, money(row[c]), "r", "", r === 4 && c === 2 ? ind : "");
        const over = b.q1[i] > row[4];
        // Conditional formatting: Q1 over budget uses the "Bad" cell style.
        put(r, 4, money(b.q1[i]), "r", over ? "background:#ffcccc;color:#cc0000" : "");
        put(r, 5, money(row[4]), "r");
      });
      put(9, 0, "Total", "b", `background:${TOTAL_FILL}`);
      [b.tot[0], b.tot[1], b.tot[2], b.q1tot, b.btot].forEach((n, i) => put(9, i + 1, money(n), "b r", `background:${TOTAL_FILL}`));
      const colHeads = colW.map((cw, c) => `<div class="brick-lo-colh${c === 0 ? " on" : ""}" style="left:${RHW + colX[c]}px;width:${cw}px">${letters(c)}</div>`).join("");
      const rowHeads = Array.from({ length: 34 }, (_, r) => `<div class="brick-lo-rowh${r === 0 ? " on" : ""}" style="top:${CHH + r * RH}px;height:${RH}px">${r + 1}</div>`).join("");
      // The chart: Q1 against budget for the discretionary lines (A4:A9).
      const cats = [2, 3, 4, 5, 6, 7];
      const cw = 560, ch = 330, px0 = 56, py0 = 50, pw = cw - px0 - 24, ph = ch - py0 - 70;
      const max = 10000;
      const bars = cats.map((i, k) => {
        const gx = px0 + (pw / cats.length) * k;
        const bw = pw / cats.length / 3.2;
        const q = (b.q1[i] / max) * ph, bu = (b.rows[i][4] / max) * ph;
        return `<rect x="${gx + bw * 0.6}" y="${py0 + ph - q}" width="${bw}" height="${q}" fill="#004586"/><rect x="${gx + bw * 1.6}" y="${py0 + ph - bu}" width="${bw}" height="${bu}" fill="#ff420e"/><text x="${gx + bw * 1.6}" y="${py0 + ph + 18}" text-anchor="middle" font-size="12" font-family="Carlito" fill="#333">${b.rows[i][0]}</text>`;
      }).join("");
      const ticks = [0, 2000, 4000, 6000, 8000, 10000].map((v) => `<path d="M${px0} ${py0 + ph - (v / max) * ph}H${px0 + pw}" stroke="#d9d9d9"/><text x="${px0 - 6}" y="${py0 + ph - (v / max) * ph + 4}" text-anchor="end" font-size="11" font-family="Carlito" fill="#555">${V.fmt(v)}</text>`).join("");
      const chart = `<div class="brick-lo-chart" style="left:${RHW + colX[6] + 30}px;top:${CHH + RH}px;width:${cw}px;height:${ch}px"><svg width="${cw}" height="${ch}">${ticks}${bars}
        <text x="${cw / 2}" y="28" text-anchor="middle" font-size="17" font-family="Carlito" font-weight="700" fill="#333">Discretionary spend, Q1</text>
        <rect x="${cw / 2 - 90}" y="${ch - 26}" width="10" height="10" fill="#004586"/><text x="${cw / 2 - 74}" y="${ch - 17}" font-size="12" font-family="Carlito" fill="#333">Q1</text>
        <rect x="${cw / 2 + 10}" y="${ch - 26}" width="10" height="10" fill="#ff420e"/><text x="${cw / 2 + 26}" y="${ch - 17}" font-size="12" font-family="Carlito" fill="#333">Budget</text></svg></div>`;
      const noteCell = { r: 8, c: 0 }; // A9; C5 has a comment too
      const nx = RHW + colX[noteCell.c] + colW[noteCell.c], ny = CHH + noteCell.r * RH;
      const note = `<div class="brick-lo-noteline" style="left:${nx}px;top:${ny}px;width:52px;transform:rotate(-24deg)"></div><div class="brick-lo-note" style="left:${nx + 46}px;top:${ny - 56}px"><b>Dana Whitfield</b>Paper and toner. New supplier from April, about 20% cheaper.</div>`;
      const menu = ["File", "Edit", "View", "Insert", "Format", "Styles", "Sheet", "Data", "Tools", "Window", "Help"].map((m) => `<span>${m}</span>`).join("");
      const ic = (k) => `<span class="ic">${loIcon(k)}</span>`;
      const sp = '<span class="sep"></span>';
      const tb1 = ["new", "open", "save", "|", "pdf", "print", "|", "cut", "copy", "paste", "|", "undo", "redo", "|", "grid", "sortA", "filter", "|", "chart", "pivot", "|", "omega", "link", "note"].map((k) => (k === "|" ? sp : ic(k))).join("");
      const tb2 = `<span class="combo" style="width:170px"><i>Calibri</i> ${I("pan-down")}</span><span class="combo" style="width:70px;margin-left:4px">11 pt ${I("pan-down")}</span>${sp}${ic("bold")}${ic("italic")}${ic("under")}${sp}${ic("fontc")}${ic("bg")}${sp}${ic("left")}${ic("center")}${ic("right")}${sp}${ic("cur")}${ic("pct")}${ic("num")}`;
      const lo = V.html(`<div class="brick-lo-pos" style="position:absolute;left:${X}px;top:${Y}px;width:${W}px;height:${H}px">
        <div class="brick-lo" style="zoom:${Z};width:${W / Z}px;height:${H / Z}px">
          <div class="brick-lo-title">budget.xlsx - LibreOffice Calc<span class="x">${I("window-close")}</span></div>
          <div class="brick-lo-menu">${menu}</div>
          <div class="brick-lo-tb">${tb1}</div>
          <div class="brick-lo-tb">${tb2}</div>
          <div class="brick-lo-fx"><span class="nb">A1</span>${I("pan-down")}<span class="fi">ƒx</span>${loIcon("sum")}${loIcon("eq")}<span class="in">Category</span></div>
          <div class="brick-lo-main">
            <div class="brick-lo-sheet">${cells.join("")}${colHeads}${rowHeads}<div class="brick-lo-colh" style="left:0;width:${RHW}px"></div>
              <div class="brick-lo-cur" style="${at(0, 0)}"></div>${chart}<div class="brick-lo-notewrap">${note}</div></div>
            <div class="brick-lo-side"><i></i><i></i><i></i><i></i><i></i></div>
          </div>
          <div class="brick-lo-tabs"><span class="nav">${I("go-up")}${I("go-down")}${I("list-add")}</span><span class="tab on">Budget</span><span class="tab">Income</span><span class="tab">Staff</span><span class="tab">Notes</span></div>
          <div class="brick-lo-status"><span>Sheet 1 of 4</span><span>Default</span><span>English (USA)</span><span class="grow"></span><span>Average: ; Sum: 0</span><span class="brick-lo-zoom"><b></b>100%</span></div>
        </div></div>`);
      cam.appendChild(lo);
      const noteWrap = lo.querySelector(".brick-lo-notewrap");
      const sheet = lo.querySelector(".brick-lo-sheet");
      const [sx0, sy0] = V.ui.pos(sheet, 0, 0);
      const cellXY = (r, c, ax = 0.5, ay = 0.5) => [sx0 + (RHW + colX[c] + colW[c] * ax) * Z, sy0 + (CHH + r * RH + RH * ay) * Z];
      const chartXY = V.ui.pos(lo.querySelector(".brick-lo-chart"), 0.55, 0.45);
      const [nxs, nys] = cellXY(noteCell.r, noteCell.c, 0.7);
      const tChart = s.at(0, "charts") - 0.5, tNote = s.at(0, "comments") - 0.35;
      return (t) => {
        V.ui.appear(lo, t, 0, 0.01);
        const path = [
          [0, 760, 640],
          [tChart, chartXY[0], chartXY[1]],
          [tNote, nxs, nys],
          [tNote + 4, nxs + 4, nys + 2],
        ];
        V.pointer(t, path);
        // Calc shows a comment while the pointer is over its cell.
        const hover = t >= tNote + 0.35;
        V.show(noteWrap, hover);
      };
    },
  });
})();
