// 3:40 Etch: a drawing filled, brushed, and colour-picked; text and a rectangle; a selection
// copied, pasted, and moved; Image → Rotate and an undo; an 8K photo opened and zoomed to
// 3200%.
//
// The Etch window follows apps/etch/src: window.rs (the menu bar: File, Edit, View, Image;
// the tool column; the canvas; the colour row: the colours' square, Hex, Custom…, the
// palette, the tool's own controls, then the status: pointer, selection, zoom, size; and
// "Opening…" while a file is read), tools.rs (the ten tools in order, each icon a stroked
// path on 16 units drawn at 20 px, the column's 6 px margins and 4 px spacing), colours.rs
// (Paint's 20 colours), view.rs (zoom steps, fit with a 16 px margin, zoom about the
// pointer, the percentage), canvas.rs (the 35% black boundary, the 8 px checkerboard, the
// selection's dashes, the text's dashed frame and caret, the brush's ring), and
// image_menu.rs (the Image menu's sections and keys).
"use strict";

(() => {
  const Z = 1.25; // GNOME at 125%
  const HB = 47, MB = 26, TC = 53, ROW = 51; // header bar, menu bar, tool column and colour row with their separators
  const MARGIN = 16;
  const ZOOMS = [0.1, 0.125, 1 / 6, 0.25, 1 / 3, 0.5, 2 / 3, 1, 1.5, 2, 3, 4, 6, 8, 12, 16, 24, 32];
  const percent = (z) => {
    const p = Math.round(z * 1000) / 10;
    return Number.isInteger(p) ? `${p}%` : `${p.toFixed(1)}%`;
  };
  // colours.rs PALETTE: two rows of ten.
  const PALETTE = [
    ["Black", "000000"], ["Grey", "7F7F7F"], ["Dark Red", "880015"], ["Red", "ED1C24"], ["Orange", "FF7F27"],
    ["Yellow", "FFF200"], ["Green", "22B14C"], ["Turquoise", "00A2E8"], ["Indigo", "3F48CC"], ["Purple", "A349A4"],
    ["White", "FFFFFF"], ["Light Grey", "C3C3C3"], ["Brown", "B97A57"], ["Rose", "FFAEC9"], ["Gold", "FFC90E"],
    ["Light Yellow", "EFE4B0"], ["Lime", "B5E61D"], ["Light Turquoise", "99D9EA"], ["Blue Grey", "7092BE"], ["Lavender", "C8BFE7"],
  ];
  const hex = Object.fromEntries(PALETTE);
  // tools.rs: id, key, icon.
  const TOOLS = [
    ["pencil", "P", "M 2.5 13.5 L 3 10.5 L 11 2.5 L 13.5 5 L 5.5 13 Z M 9.5 4 L 12 6.5"],
    ["brush", "B", "M 14 2 L 9 7 M 7 6.5 L 9.5 9 M 8 8 C 5.5 7 3.5 9 3.5 11.5 C 3.5 12.8 3 13.5 2 14 C 5.5 14.5 9 12 8 8 Z"],
    ["eraser", "E", "M 2 9.5 L 8 3.5 L 13 8.5 L 8 13.5 L 6 13.5 Z M 5 6.5 L 10 11.5 M 8 13.5 L 14.5 13.5"],
    ["fill", "F", "M 2.5 8 L 7.5 3 L 12.5 8 L 8 12.5 L 7 12.5 Z M 2.5 8 L 12.5 8 M 7.5 3 L 5.5 1 M 13.5 10 C 12.5 11.5 12.3 13.5 13.5 13.5 C 14.7 13.5 14.5 11.5 13.5 10 Z"],
    ["picker", "I", "M 2 14 L 2.5 11.5 L 8.5 5.5 L 10.5 7.5 L 4.5 13.5 Z M 7.5 4.5 L 11.5 8.5 M 9.5 6.5 L 12 4 C 13 3 14.5 3 13.5 2.5 C 13 1.5 13 3 12 4"],
    ["text", "T", "M 3 5 L 3 2.5 L 13 2.5 L 13 5 M 8 2.5 L 8 13.5 M 5.5 13.5 L 10.5 13.5"],
    ["line", "L", "M 2.5 13.5 L 13.5 2.5"],
    ["rectangle", "R", "M 2 4 L 14 4 L 14 12 L 2 12 Z"],
    ["ellipse", "O", "M 1.5 8 C 1.5 5.5 4.4 3.5 8 3.5 C 11.6 3.5 14.5 5.5 14.5 8 C 14.5 10.5 11.6 12.5 8 12.5 C 4.4 12.5 1.5 10.5 1.5 8 Z"],
    ["selection", "S", "M 2 5.5 L 2 3.5 L 4 3.5 M 7 3.5 L 9 3.5 M 12 3.5 L 14 3.5 L 14 5.5 M 14 7 L 14 9 M 14 10.5 L 14 12.5 L 12 12.5 M 9 12.5 L 7 12.5 M 4 12.5 L 2 12.5 L 2 10.5 M 2 9 L 2 7"],
  ];
  // Each tool's own controls in the colour row (window.rs: sizing, tolerating, font, edge, style).
  const SIZED = new Set(["pencil", "brush", "eraser", "text", "line", "rectangle", "ellipse"]);

  let uidN = 0;
  const uid = () => `et${++uidN}`;
  const measure = document.createElement("canvas").getContext("2d");
  const textW = (s, font) => ((measure.font = font), measure.measureText(s).width);

  // ---- view.rs ----------------------------------------------------------------------------
  function clampView(v) {
    const axis = (origin, shown, size) => (shown <= size - 2 * MARGIN ? (size - shown) / 2 : Math.min(MARGIN, Math.max(size - MARGIN - shown, origin)));
    return { ...v, origin: [axis(v.origin[0], v.image[0] * v.zoom, v.size[0]), axis(v.origin[1], v.image[1] * v.zoom, v.size[1])] };
  }
  // View::new: 100% centred, or fitted when the image is larger than the canvas.
  function newView(image, size) {
    const fit = Math.min(32, Math.max(0.1, Math.min((size[0] - 2 * MARGIN) / image[0], (size[1] - 2 * MARGIN) / image[1])));
    const zoom = Math.min(fit, 1);
    return clampView({ zoom, image, size, origin: [(size[0] - image[0] * zoom) / 2, (size[1] - image[1] * zoom) / 2] });
  }
  function zoomAt(v, zoom, anchor) {
    const at = [(anchor[0] - v.origin[0]) / v.zoom, (anchor[1] - v.origin[1]) / v.zoom];
    return clampView({ ...v, zoom, origin: [anchor[0] - at[0] * zoom, anchor[1] - at[1] * zoom] });
  }
  const nextZoom = (z) => ZOOMS.find((s) => s > z + 1e-9) ?? z;

  // ---- drawing.png: a house, a tree, a sun, and birds; a level field ------------------------
  const IW = 1024, IH = 768;
  const SKY = "FFFFFF", CLOUD = [[108, 128], [150, 104], [196, 126], [242, 100], [290, 124], [336, 104], [384, 130]];
  const COPY = { x: 670, y: 288, w: 178, h: 328 }; // the selection around the tree
  const PASTED = { x: 40, y: 288 }; // where the copy lands, left of the house
  const WORDS = { x: 540, y: 648, size: 48, font: "Caladea", text: "Welcome home!" };
  const CHIMNEY = { x0: 440, y0: 236, x1: 472, y1: 304 };
  function artSVG(u) {
    const rays = Array.from({ length: 8 }, (_, i) => {
      const a = (i * Math.PI) / 4 + Math.PI / 8;
      const [c, s] = [Math.cos(a), Math.sin(a)];
      return `<line x1="${(880 + 100 * c).toFixed(1)}" y1="${(160 + 100 * s).toFixed(1)}" x2="${(880 + 128 * c).toFixed(1)}" y2="${(160 + 128 * s).toFixed(1)}"/>`;
    }).join("");
    return `<svg class="etch-art" viewBox="0 0 ${IW} ${IH}" preserveAspectRatio="none">
      <defs>
        <clipPath id="${u}-clip"><rect x="${COPY.x}" y="${COPY.y}" width="${COPY.w}" height="${COPY.h}"/></clipPath>
        <filter id="${u}-soft" x="-30%" y="-150%" width="160%" height="400%"><feGaussianBlur stdDeviation="8"/></filter>
      </defs>
      <g class="etch-turn">
        <g id="${u}-art">
          <rect class="etch-sky" width="${IW}" height="${IH}" fill="#${SKY}"/>
          <rect y="520" width="${IW}" height="${IH - 520}" fill="#B5E61D"/>
          <g stroke="#FF7F27" stroke-width="9" stroke-linecap="round">${rays}</g>
          <circle cx="880" cy="160" r="80" fill="#FFC90E"/>
          <g fill="none" stroke="#7F7F7F" stroke-width="5" stroke-linecap="round" stroke-linejoin="round"><polyline points="518,140 543,160 568,140"/><polyline points="600,190 622,208 644,190"/></g>
          <polyline class="etch-cloud" points="${CLOUD.map((p) => p.join(",")).join(" ")}" pathLength="1" stroke-dasharray="0 2" fill="none" stroke="#fff" stroke-width="44" stroke-linecap="round" stroke-linejoin="round" filter="url(#${u}-soft)"/>
          <rect x="250" y="330" width="250" height="215" fill="#ED1C24"/>
          <polygon points="230,336 375,230 520,336" fill="#880015"/>
          <rect class="etch-chimney" fill="#B97A57" width="0" height="0"/>
          <rect x="282" y="368" width="56" height="50" fill="#EFE4B0"/><rect x="412" y="368" width="56" height="50" fill="#EFE4B0"/>
          <rect x="350" y="450" width="50" height="95" fill="#B97A57"/>
          <rect x="745" y="420" width="22" height="170" fill="#B97A57"/>
          <circle cx="756" cy="365" r="72" fill="#22B14C"/>
          <text class="etch-words" x="${WORDS.x}" y="${WORDS.y}" font-family="${WORDS.font}" font-weight="700" font-size="${WORDS.size}" fill="#880015" dominant-baseline="text-before-edge"></text>
        </g>
        <g class="etch-paste" style="display:none"><g class="etch-pastemove"><g clip-path="url(#${u}-clip)"><use href="#${u}-art"/></g></g></g>
      </g>
    </svg>`;
  }

  // ---- mountain.png: an 8K photo, made a pixel at a time ------------------------------------
  const PW = 7680, PH = 4320;
  const vnoise = (x, seed) => {
    const i = Math.floor(x), f = x - i, s = f * f * (3 - 2 * f);
    const a = V.hash(i, seed), b = V.hash(i + 1, seed);
    return a + (b - a) * s;
  };
  const fbm = (x, seed, oct = 6) => {
    let v = 0, amp = 0.5, fr = 1;
    for (let o = 0; o < oct; o++, fr *= 2, amp *= 0.5) v += amp * vnoise(x * fr, seed + o);
    return v;
  };
  const mix = (a, b, k) => [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k];
  const ramp = (stops, k) => {
    if (k <= stops[0][0]) return stops[0][1];
    for (let i = 1; i < stops.length; i++) if (k <= stops[i][0]) return mix(stops[i - 1][1], stops[i][1], (k - stops[i - 1][0]) / (stops[i][0] - stops[i - 1][0]));
    return stops[stops.length - 1][1];
  };
  const SKYR = [[0, [46, 84, 140]], [0.22, [112, 128, 178]], [0.4, [214, 168, 168]], [0.52, [246, 192, 138]], [0.6, [252, 218, 168]]];
  const SUN = [0.67 * PW, 0.31 * PH, 0.028 * PW];
  // Per column: the far ridge, the near ridge, the treeline, the snow line, and the far
  // ridge's slope (which side of a peak faces the sun). The far ridge is ridged noise:
  // sharp peaks, rounded valleys.
  const ridged = (x, seed, oct = 7) => {
    let v = 0, amp = 0.5, fr = 1;
    for (let o = 0; o < oct; o++, fr *= 2, amp *= 0.5) v += amp * (1 - Math.abs(2 * vnoise(x * fr, seed + o) - 1));
    return v;
  };
  const far = (x) => PH * (0.66 - 0.48 * ridged(x / PW * 2.2, 11));
  const near = (x) => PH * (0.48 + 0.08 * fbm(x / PW * 5, 23));
  const trees = (x) => PH * (0.585 + 0.02 * fbm(x / PW * 90, 31, 4));
  const snowline = (x) => PH * (0.33 + 0.025 * fbm(x / PW * 40, 5, 3));
  const columns = (x) => [far(x), near(x), trees(x), snowline(x), far(x + 80) - far(x - 80)];
  function skyAt(x, y) {
    let c = ramp(SKYR, y / PH);
    const d = Math.hypot(x - SUN[0], y - SUN[1]);
    if (d < SUN[2]) return [255, 244, 214];
    return mix(c, [255, 236, 200], Math.max(0, 0.55 - (d - SUN[2]) / (PW * 0.12)));
  }
  function photoPixel(x, y, cols) {
    const [fy, ny, ty, sy, slope] = cols;
    let c;
    if (y < fy) c = skyAt(x, y);
    else if (y < ny) {
      // The far range: snow above the snow line, lit on the slopes facing the sun, then
      // rock fading into haze.
      const lit = Math.min(1, Math.max(0, 0.5 + slope / (PH * 0.03)));
      const depth = (y - fy) / PH;
      c = y < sy ? mix([170, 164, 204], [250, 243, 242], lit) : mix([150, 132, 170], [112, 104, 146], Math.min(1, depth * 9));
    } else if (y < ty) c = mix([66, 72, 104], [40, 48, 70], Math.min(1, (y - ny) / (PH * 0.08)));
    else if (y < PH * 0.6) c = [26, 36, 40];
    else {
      // The lake: the sky mirrored, darker with depth, with ripples.
      const m = 2 * PH * 0.6 - y;
      const r = mix(skyAt(x, Math.max(0, m)), [36, 58, 84], Math.min(1, (y - PH * 0.6) / (PH * 0.35)));
      c = mix(r, [255, 236, 200], 0.18 * Math.max(0, vnoise(x / 120, Math.floor(y / 6)) - 0.6) * 4);
    }
    const g = (V.hash(x, y, 77) - 0.5) * 14;
    return [c[0] + g, c[1] + g, c[2] + g];
  }
  // Pixels x0..x0+w, y0..y0+h of the photo, one in `step` a side, into a canvas.
  function photoCanvas(x0, y0, w, h, step = 1) {
    const cv = document.createElement("canvas");
    cv.width = w;
    cv.height = h;
    const ctx = cv.getContext("2d");
    const img = ctx.createImageData(w, h);
    const d = img.data;
    for (let i = 0; i < w; i++) {
      const x = x0 + i * step + (step >> 1);
      const cols = columns(x);
      for (let j = 0; j < h; j++) {
        const c = photoPixel(x, y0 + j * step + (step >> 1), cols);
        const o = (j * w + i) * 4;
        d[o] = c[0];
        d[o + 1] = c[1];
        d[o + 2] = c[2];
        d[o + 3] = 255;
      }
    }
    ctx.putImageData(img, 0, 0);
    return cv;
  }
  // The point the zoom goes to: on the far ridge's edge against the sky, by its highest peak.
  function ridgePoint() {
    let best = 0.5 * PW;
    for (let x = 0.5 * PW; x < 0.64 * PW; x += 8) if (far(x) < far(best)) best = x;
    const x = Math.round(best + 180);
    return [x, Math.floor(far(x))];
  }

  // ---- the window ---------------------------------------------------------------------------
  const toolIcon = (d) => `<svg width="20" height="20" viewBox="0 0 16 16"><path d="${d}" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" stroke-linecap="round"/></svg>`;
  const spin = (v, cls = "") => `<div class="etch-spin ${cls}"><span>${v}</span><b>−</b><b>+</b></div>`;
  const seg = (labels, cls) => `<div class="etch-seg ${cls}">${labels.map((l) => `<span data-v="${l}">${l}</span>`).join("")}</div>`;
  // Image menu (image_menu.rs): four sections, each command's key beside it.
  const IMAGE_MENU = [
    [["Crop to Selection", "Shift+Ctrl+X"]],
    [["Resize Image…", "Ctrl+R"], ["Resize Canvas…", "Shift+Ctrl+R"]],
    [["Rotate 90° Right", "Ctrl+H"], ["Rotate 90° Left", "Ctrl+G"], ["Rotate 180°", "Ctrl+J"]],
    [["Flip Horizontal"], ["Flip Vertical"]],
  ];
  const imageMenu = () =>
    `<div class="popover etch-menu">${IMAGE_MENU.map((sec) => sec.map(([l, a]) => `<div class="mi" data-label="${l}"><span class="mi-label">${l}</span>${a ? `<span class="accel">${a}</span>` : ""}</div>`).join("")).join('<div class="mi-sep"></div>')}</div>`;

  // body: the canvas's contents (the drawing's SVG, or a canvas for the photo).
  function windowHTML({ id = "", x, y, w, h, title, body }) {
    const gw = w / Z, gh = h / Z;
    const tools = TOOLS.map(([t, , d]) => `<div class="etch-tool" data-tool="${t}">${toolIcon(d)}</div>`).join("");
    const pal = PALETTE.map(([n, c]) => `<i data-c="${n}" style="background:#${c}"></i>`).join("");
    const content = `
      <div class="etch-menubar"><span>File</span><span>Edit</span><span>View</span><span data-m="image">Image</span></div>
      <div class="etch-main" style="height:${gh - HB - MB - ROW}px">
        <div class="etch-tools">${tools}</div>
        <div class="etch-canvas" style="width:${gw - TC}px">
          <div class="etch-img">${body}</div>
          <svg class="etch-ov" width="${gw - TC}" height="${gh - HB - MB - ROW}"></svg>
        </div>
        <div class="etch-opening"><svg class="etch-spinner" width="16" height="16" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.5" fill="none" stroke="currentColor" stroke-opacity=".2" stroke-width="2"/><path d="M8 1.5a6.5 6.5 0 0 1 6.5 6.5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg><span>Opening…</span></div>
      </div>
      <div class="etch-row">
        <div class="etch-ind"><i class="bg"></i><i class="fg"></i></div>
        <div class="etch-pair"><span>Hex</span><div class="etch-entry etch-hex"></div><div class="etch-button">Custom…</div></div>
        <div class="etch-pal">${pal}</div>
        <div class="etch-pair etch-sizing"><span>Size</span>${spin("1", "etch-size")}<span>px</span></div>
        <div class="etch-pair etch-tolerating"><span>Tolerance</span>${spin("0")}</div>
        <div class="etch-pair etch-font"><div class="etch-dd"><span>Sans</span>${V.ui.icon("pan-down")}</div><div class="etch-button etch-b"><b>B</b></div><div class="etch-button etch-i"><i>I</i></div></div>
        ${seg(["Hard", "Soft"], "etch-edge")}
        ${seg(["Outline", "Filled", "Both"], "etch-style")}
        <div class="etch-status"><span class="xy"></span><span class="grow"></span><span class="sel"></span><span class="zoom"></span><span class="size"></span></div>
      </div>
      <div class="etch-pop"></div>`;
    return V.ui.win({ id, x, y, w, h, title, body: content, cls: "etch-win" });
  }

  // Drives an Etch window built by windowHTML. image: the image's size; art: the drawing's
  // root (an SVG) or null for the photo.
  function bind(el, { x, y, w, h, image }) {
    const gw = w / Z, gh = h / Z;
    const cw = gw - TC, ch = gh - HB - MB - ROW;
    const q = (s) => el.querySelector(s);
    const tools = [...el.querySelectorAll(".etch-tool")];
    const img = q(".etch-img"), ov = q(".etch-ov"), pop = q(".etch-pop");
    const st = { xy: q(".etch-status .xy"), sel: q(".etch-status .sel"), zoom: q(".etch-status .zoom"), size: q(".etch-status .size") };
    const parts = { sizing: q(".etch-sizing"), tolerating: q(".etch-tolerating"), font: q(".etch-font"), edge: q(".etch-edge"), style: q(".etch-style") };
    const opening = q(".etch-opening"), canvasEl = q(".etch-canvas");
    const art = q(".etch-art");
    let v = newView(image, [cw, ch]);
    const api = {
      el, cw, ch,
      get view() { return v; },
      // Stage pixels of a point in the window (GTK pixels), and of an image point.
      gpt: (gx, gy) => [x + gx * Z, y + gy * Z],
      pt: (u) => api.gpt(TC + v.origin[0] + u[0] * v.zoom, HB + MB + v.origin[1] + u[1] * v.zoom),
      // An image point (fractional) under a stage point.
      toImage: (sx, sy) => [((sx - x) / Z - TC - v.origin[0]) / v.zoom, ((sy - y) / Z - HB - MB - v.origin[1]) / v.zoom],
      // The image pixel under a stage point, or null off the image.
      at(sx, sy) {
        const [u, w2] = api.toImage(sx, sy).map(Math.floor);
        return u >= 0 && w2 >= 0 && u < v.image[0] && w2 < v.image[1] ? [u, w2] : null;
      },
      // Where things in the window are, in stage pixels, measured.
      tool: (id) => V.ui.pos(el.querySelector(`[data-tool="${id}"]`)),
      swatch: (name) => V.ui.pos(el.querySelector(`[data-c="${name}"]`)),
      seg: (label) => V.ui.pos(el.querySelector(`[data-v="${label}"]`)),
      menubar: (m) => V.ui.pos(el.querySelector(`[data-m="${m}"]`)),
      // A view object (view.rs): the image's size, zoom, and origin. The drawing's SVG
      // takes its place and size; the photo's canvas draws itself.
      setView(nv) {
        v = nv;
        const [iw, ih] = v.image;
        if (art) {
          V.css(img, { left: `${v.origin[0]}px`, top: `${v.origin[1]}px`, width: `${iw * v.zoom}px`, height: `${ih * v.zoom}px` });
          art.setAttribute("viewBox", `0 0 ${iw} ${ih}`);
        }
        V.text(st.zoom, percent(v.zoom));
        V.text(st.size, `${iw}×${ih}`);
      },
      // The image's size changed (a rotation): the view resets as on opening.
      fresh: (size) => api.setView(newView(size, [cw, ch])),
      // The tool on, and its own controls in the colour row.
      state({ tool, fg = "000000", bg = "FFFFFF", size = 1, tolerance = 0, soft = true, style = "Outline", family = "Sans", bold = false }) {
        tools.forEach((b) => V.cls(b, "on", b.dataset.tool === tool));
        V.css(q(".etch-ind .fg"), { background: `#${fg}` });
        V.css(q(".etch-ind .bg"), { background: `#${bg}` });
        V.text(q(".etch-hex"), `#${fg}`);
        V.show(parts.sizing, SIZED.has(tool));
        V.text(q(".etch-size span"), String(size));
        V.show(parts.tolerating, tool === "fill");
        V.text(q(".etch-tolerating .etch-spin span"), String(tolerance));
        V.show(parts.font, tool === "text");
        V.text(q(".etch-dd span"), family);
        V.cls(q(".etch-b"), "on", bold);
        V.show(parts.edge, tool === "brush");
        for (const s of parts.edge.children) V.cls(s, "on", (s.dataset.v === "Soft") === soft);
        V.show(parts.style, tool === "rectangle" || tool === "ellipse");
        for (const s of parts.style.children) V.cls(s, "on", s.dataset.v === style);
      },
      status({ xy = null, sel = null } = {}) {
        V.text(st.xy, xy ? `x: ${xy[0]}  y: ${xy[1]}` : "");
        V.text(st.sel, sel ? `Selection: ${sel[0]}×${sel[1]}` : "");
      },
      // The canvas's overlay, in image pixels: selection rectangles (dashed black on
      // white), the text's frame and caret, the brush's ring.
      overlay({ sel = null, frame = null, ring = null } = {}) {
        const X = (u) => v.origin[0] + u * v.zoom, Y = (w2) => v.origin[1] + w2 * v.zoom;
        let out = "";
        if (sel) {
          const r = `x="${(Math.round(X(sel.x)) - 0.5).toFixed(1)}" y="${(Math.round(Y(sel.y)) - 0.5).toFixed(1)}" width="${Math.round(sel.w * v.zoom) + 1}" height="${Math.round(sel.h * v.zoom) + 1}" fill="none" stroke-width="1"`;
          out += `<rect ${r} stroke="#fff"/><rect ${r} stroke="#000" stroke-dasharray="4 4"/>`;
        }
        if (frame) {
          const fx = Math.round(X(frame.x)) - 4 + 0.5, fy = Math.round(Y(frame.y)) - 4 + 0.5;
          const fw = Math.round(frame.w * v.zoom) + 8, fh = Math.round(frame.h * v.zoom) + 8;
          out += `<rect x="${fx}" y="${fy}" width="${fw}" height="${fh}" fill="none" stroke="rgba(255,255,255,.7)"/><rect x="${fx}" y="${fy}" width="${fw}" height="${fh}" fill="none" stroke="rgba(0,0,0,.5)" stroke-dasharray="4 4"/>`;
          if (frame.caret) {
            const cx = Math.round(X(frame.x + frame.cx)) + 0.5;
            out += `<path d="M${cx + 1} ${Y(frame.y)}V${Y(frame.y + frame.h)}" stroke="rgba(255,255,255,.7)" stroke-width="1"/><path d="M${cx} ${Y(frame.y)}V${Y(frame.y + frame.h)}" stroke="#000" stroke-width="1"/>`;
          }
        }
        if (ring) {
          const r = (ring.size * v.zoom) / 2;
          out += `<circle cx="${X(ring.x)}" cy="${Y(ring.y)}" r="${r + 0.5}" fill="none" stroke="rgba(255,255,255,.7)"/><circle cx="${X(ring.x)}" cy="${Y(ring.y)}" r="${r - 0.5}" fill="none" stroke="rgba(0,0,0,.8)"/>`;
        }
        V.markup(ov, out);
      },
      // The Image menu, open with `hot` highlighted, or shut; Crop greyed without a selection.
      menu(open, { hot = "", crop = false } = {}) {
        V.cls(q('[data-m="image"]'), "on", open);
        if (!open) return V.markup(pop, "");
        V.markup(pop, imageMenu());
        for (const m of pop.querySelectorAll(".mi")) {
          V.cls(m, "hot", m.dataset.label === hot);
          V.cls(m, "off", m.dataset.label === "Crop to Selection" && !crop);
        }
      },
      menuItem(label) {
        api.menu(true);
        return V.ui.pos(pop.querySelector(`[data-label="${label}"]`));
      },
      // "Opening…" in place of the canvas and the colour row's tools, or the image.
      opening(on, t = 0) {
        V.show(opening, on);
        V.css(canvasEl, { visibility: on ? "hidden" : "visible" });
        V.css(q(".etch-tools"), { visibility: on ? "hidden" : "visible" });
        V.css(q(".etch-row"), { visibility: on ? "hidden" : "visible" });
        if (on) V.css(q(".etch-spinner"), { transform: `rotate(${(t * 360 * 1.2) % 360}deg)` });
      },
    };
    api.setView(v);
    api.menu(false);
    api.opening(false);
    // GTK's menu bar popover: just below the bar, a little in from its item's left edge.
    V.css(pop, { left: `${q('[data-m="image"]').offsetLeft + 4}px`, top: `${MB + 4}px` });
    return api;
  }

  // The drawing's parts a scene changes.
  function drawing(root) {
    const q = (s) => root.querySelector(s);
    const cloud = q(".etch-cloud"), chimney = q(".etch-chimney"), words = q(".etch-words"), paste = q(".etch-paste"), move = q(".etch-pastemove"), turn = q(".etch-turn");
    return {
      sky: (c) => q(".etch-sky").setAttribute("fill", `#${c}`),
      cloud: (k) => V.css(cloud, { strokeDasharray: `${k.toFixed(4)} 2` }),
      chimney(r) {
        const a = r ? { x: Math.min(r.x0, r.x1), y: Math.min(r.y0, r.y1), width: Math.abs(r.x1 - r.x0), height: Math.abs(r.y1 - r.y0) } : { x: 0, y: 0, width: 0, height: 0 };
        for (const k in a) if (chimney.getAttribute(k) !== String(a[k])) chimney.setAttribute(k, a[k]);
      },
      words: (s) => V.text(words, s),
      // The pasted copy at (px, py), or none.
      paste(at) {
        V.show(paste, !!at);
        if (at) {
          const tf = `translate(${(at[0] - COPY.x).toFixed(2)} ${(at[1] - COPY.y).toFixed(2)})`;
          if (move.getAttribute("transform") !== tf) move.setAttribute("transform", tf);
        }
      },
      turned(on) {
        const tf = on ? `translate(${IH} 0) rotate(90)` : "";
        if ((turn.getAttribute("transform") || "") !== tf) turn.setAttribute("transform", tf);
      },
    };
  }

  // Each scene's drawing as it starts: what the scenes before it did.
  const DONE = {
    paint: { sky: SKY, cloud: 0, words: "", chimney: null, paste: null },
    shapes: { sky: "99D9EA", cloud: 1, words: "", chimney: null, paste: null },
    select: { sky: "99D9EA", cloud: 1, words: WORDS.text, chimney: CHIMNEY, paste: null },
    end: { sky: "99D9EA", cloud: 1, words: WORDS.text, chimney: CHIMNEY, paste: [PASTED.x, PASTED.y] },
  };
  function paint(d, s) {
    d.sky(s.sky);
    d.cloud(s.cloud);
    d.words(s.words);
    d.chimney(s.chimney);
    d.paste(s.paste);
    d.turned(false);
  }

  const W = { x: 110, y: 56, w: 1700, h: 900 };
  function drawingWindow(root, { id, rect = W, title = "drawing.png" }) {
    const u = uid();
    root.insertAdjacentHTML("beforeend", windowHTML({ id, ...rect, title, body: artSVG(u) }));
    const el = root.querySelector(`#${id}`);
    return { el, e: bind(el, { ...rect, image: [IW, IH] }), d: drawing(el) };
  }

  // A pointer along an image-space path: [[t, [u, v]], ...] → [[t, sx, sy, opt]].
  const along = (pts, k) => {
    const lens = pts.slice(1).map((p, i) => Math.hypot(p[0] - pts[i][0], p[1] - pts[i][1]));
    const total = lens.reduce((a, b) => a + b, 0);
    let d = k * total;
    for (let i = 0; i < lens.length; i++) {
      if (d <= lens[i]) return [pts[i][0] + ((pts[i + 1][0] - pts[i][0]) * d) / lens[i], pts[i][1] + ((pts[i + 1][1] - pts[i][1]) * d) / lens[i]];
      d -= lens[i];
    }
    return pts[pts.length - 1];
  };
  // A drag's pointer path, sampled 60 times a second so it moves smoothly between keys:
  // [[t, sx, sy], ...] from at(t) for t through (t0, t1].
  const drag = (t0, t1, at) => {
    const n = Math.max(1, Math.ceil((t1 - t0) * 60));
    return Array.from({ length: n }, (_, i) => {
      const t = t0 + ((t1 - t0) * (i + 1)) / n;
      return [t, ...at(t)];
    });
  };

  // ---- the contract for install-open ---------------------------------------------------------
  V.etch = {
    window: ({ id, x, y, w, h, title = "drawing.png" }) => {
      const u = uid();
      return windowHTML({ id, x, y, w, h, title, body: artSVG(u) });
    },
    // Draws the finished drawing in a window made by V.etch.window.
    bind: (el, rect) => {
      const e = bind(el, { ...rect, image: [IW, IH] });
      paint(drawing(el), DONE.end);
      e.state({ tool: "pencil" });
      e.status();
      e.overlay();
      return e;
    },
  };

  // =======================================================================================
  // Fill the sky, brush a cloud, pick the roof's colour.
  V.scene({
    id: "etch-paint",
    beats: [{ say: "Etch is for images. It's closer to Paint than to Photoshop: a fill, a brush, a colour picker, and Paint's own keys.", min: 10.6 }],
    build(root) {
      root.innerHTML = V.ui.desktop();
      const { e, d } = drawingWindow(root, { id: "ep-etch" });
      paint(d, DONE.paint);
      const tF = 0.9, tTurq = 1.8, tSky = 2.75, tB = 3.4, tWhite = 4.15, tPress = 5.0, tRelease = 6.7, tI = 7.3, tPick = 8.3;
      const sky = [150, 230], roof = [372, 292];
      const brushAt = (t) => along(CLOUD, V.p(t, tPress, tRelease, V.ease.inOut));
      const path = [
        [0, ...e.pt([600, 600])], [tF + 0.3, ...e.pt([600, 600])],
        [tTurq - 0.45, ...e.swatch("Light Turquoise")], [tTurq, ...e.swatch("Light Turquoise"), { click: true }],
        [tSky - 0.5, ...e.pt(sky)], [tSky, ...e.pt(sky), { click: true }],
        [tB + 0.2, ...e.pt([200, 300])],
        [tWhite - 0.45, ...e.swatch("White")], [tWhite, ...e.swatch("White"), { click: true }],
        [tPress - 0.45, ...e.pt(CLOUD[0])], [tPress, ...e.pt(CLOUD[0]), { down: true }],
        ...drag(tPress, tRelease, (t) => e.pt(brushAt(t))),
        [tRelease + 0.01, ...e.pt(CLOUD[CLOUD.length - 1]), { down: false }],
        [tI + 0.2, ...e.pt([420, 250])],
        [tPick - 0.5, ...e.pt(roof)], [tPick, ...e.pt(roof), { click: true }],
        [tPick + 1.2, ...e.pt([470, 470])],
      ];
      return (t) => {
        const tool = t >= tPick ? "brush" : t >= tI ? "picker" : t >= tB ? "brush" : t >= tF ? "fill" : "pencil";
        const fg = t >= tPick ? hex["Dark Red"] : t >= tWhite ? "FFFFFF" : t >= tTurq ? hex["Light Turquoise"] : "000000";
        e.state({ tool, fg, size: tool === "brush" ? 44 : 1, soft: true });
        d.sky(t >= tSky ? hex["Light Turquoise"] : SKY);
        d.cloud(V.p(t, tPress, tRelease, V.ease.inOut));
        V.pointer(t, path);
        const pos = V.track(t, path.map(([at, px, py]) => [at, { x: px, y: py }]));
        const over = e.at(pos.x, pos.y);
        e.status({ xy: over });
        // The brush's tip, a ring around the pointer.
        const [ru, rv] = e.toImage(pos.x, pos.y);
        e.overlay({ ring: tool === "brush" && over ? { x: ru, y: rv, size: 44 } : null });
        V.keys(t, [[tF, "F"], [tB, "B"], [tI, "I"]]);
      };
    },
  });

  // =======================================================================================
  // Type text on the grass; draw a chimney with the rectangle, previewed while dragged.
  V.scene({
    id: "etch-shapes",
    beats: [{ say: "Type text in any font you have. Shapes show exactly where they'll land while you drag them.", min: 9.8 }],
    build(root, s) {
      root.innerHTML = V.ui.desktop();
      const { e, d } = drawingWindow(root, { id: "es-etch" });
      paint(d, DONE.shapes);
      const tT = 0.5, tClick = 1.3, tType = 1.65, cps = 11, tEnter = V.typedEnd(WORDS.text, tType, cps) + 0.5;
      const tR = Math.max(tEnter + 0.6, s.at(0, "Shapes") - 0.3);
      const tFilled = tR + 0.75, tBrown = tR + 1.45, tPress = tR + 2.1, tRelease = tR + 3.5;
      const font = `700 ${WORDS.size}px ${WORDS.font}`;
      const lineH = WORDS.size * 1.22;
      const corner = (t) => V.track(t, [[tPress, { x: CHIMNEY.x0, y: CHIMNEY.y0 }], [tPress + 0.5, { x: CHIMNEY.x0 + 12, y: CHIMNEY.y0 + 18 }], [tRelease - 0.2, { x: CHIMNEY.x1, y: CHIMNEY.y1 }]]);
      const path = [
        [0, ...e.pt([470, 470])], [tT + 0.2, ...e.pt([470, 520])],
        [tClick - 0.5, ...e.pt([WORDS.x, WORDS.y])], [tClick, ...e.pt([WORDS.x, WORDS.y]), { click: true }],
        [tClick + 0.6, ...e.pt([WORDS.x - 30, WORDS.y + 80])],
        [tFilled - 0.45, ...e.seg("Filled")], [tFilled, ...e.seg("Filled"), { click: true }],
        [tBrown - 0.45, ...e.swatch("Brown")], [tBrown, ...e.swatch("Brown"), { click: true }],
        [tPress - 0.5, ...e.pt([CHIMNEY.x0, CHIMNEY.y0])], [tPress, ...e.pt([CHIMNEY.x0, CHIMNEY.y0]), { down: true }],
        ...drag(tPress, tRelease - 0.2, (t) => {
          const c = corner(t);
          return e.pt([c.x, c.y]);
        }),
        [tRelease, ...e.pt([CHIMNEY.x1, CHIMNEY.y1]), { down: false }],
        [tRelease + 1.2, ...e.pt([560, 420])],
      ];
      return (t) => {
        const tool = t >= tR ? "rectangle" : t >= tT ? "text" : "brush";
        const fg = t >= tBrown ? hex.Brown : hex["Dark Red"];
        e.state({ tool, fg, size: tool === "text" ? 48 : tool === "brush" ? 44 : 1, family: WORDS.font, bold: true, style: t >= tFilled ? "Filled" : "Outline" });
        const typed = t >= tClick ? V.typed(WORDS.text, t, tType, cps) : "";
        d.words(typed);
        const typing = t >= tClick && t < tEnter;
        const tw = textW(typed, font);
        e.overlay({ frame: typing ? { x: WORDS.x, y: WORDS.y, w: Math.max(tw, 1), h: lineH, cx: tw, caret: t < V.typedEnd(WORDS.text, tType, cps) + 0.15 || V.blink(t) } : null });
        const c = corner(t);
        d.chimney(t >= tPress ? { x0: CHIMNEY.x0, y0: CHIMNEY.y0, x1: Math.round(c.x), y1: Math.round(c.y) } : null);
        V.pointer(t, path);
        const pos = V.track(t, path.map(([at, px, py]) => [at, { x: px, y: py }]));
        e.status({ xy: e.at(pos.x, pos.y) });
        V.keys(t, [[tT, "T"], [tEnter, "Enter"], [tR, "R"]]);
      };
    },
  });

  // =======================================================================================
  // Select the tree, copy, paste, move the copy; Image → Rotate 90° Right; Ctrl+Z.
  V.scene({
    id: "etch-select",
    beats: [{ say: "Select part of a picture to move it, or copy and paste it. Crop, resize, rotate, and flip are in the Image menu, and every step can be undone.", min: 13.5 }],
    build(root, s) {
      root.innerHTML = V.ui.desktop();
      const { e, d } = drawingWindow(root, { id: "ec-etch" });
      paint(d, DONE.select);
      const tS = 0.4, tPress = 1.1, tRelease = 2.3, tCopy = 2.8, tPaste = 3.4;
      const tGrab = 4.2, tDrop = 5.4, tAway = 6.3;
      const tMenu = Math.max(7.1, s.at(0, "Crop") + 0.2), tRotate = tMenu + 1.15;
      const tUndo = Math.max(tRotate + 1.6, s.at(0, "every step") + 0.2);
      const menuAt = e.menubar("image");
      const rotAt = e.menuItem("Rotate 90° Right");
      e.menu(false);
      const grabOff = [COPY.w / 2, COPY.h / 2];
      const floatAt = (t) => V.track(t, [[tGrab, { x: 0, y: 0 }], [tDrop - 0.1, { x: PASTED.x, y: PASTED.y }]]);
      const selEnd = (t) => V.track(t, [[tPress, { x: COPY.x, y: COPY.y }], [tRelease - 0.1, { x: COPY.x + COPY.w, y: COPY.y + COPY.h }]]);
      const path = [
        [0, ...e.pt([560, 420])], [tS + 0.2, ...e.pt([600, 300])],
        [tPress - 0.45, ...e.pt([COPY.x, COPY.y])], [tPress, ...e.pt([COPY.x, COPY.y]), { down: true }],
        [tRelease - 0.1, ...e.pt([COPY.x + COPY.w, COPY.y + COPY.h])], [tRelease, ...e.pt([COPY.x + COPY.w, COPY.y + COPY.h]), { down: false }],
        [tGrab - 0.5, ...e.pt(grabOff)], [tGrab, ...e.pt(grabOff), { down: true }],
        [tDrop - 0.1, ...e.pt([PASTED.x + grabOff[0], PASTED.y + grabOff[1]])], [tDrop, ...e.pt([PASTED.x + grabOff[0], PASTED.y + grabOff[1]]), { down: false }],
        [tAway - 0.45, ...e.pt([600, 700])], [tAway, ...e.pt([600, 700]), { click: true }],
        [tMenu - 0.5, ...menuAt], [tMenu, ...menuAt, { click: true }],
        [tRotate - 0.45, ...rotAt], [tRotate, ...rotAt, { click: true }],
        [tRotate + 1.0, rotAt[0] + 260, rotAt[1] + 380],
      ];
      return (t) => {
        e.state({ tool: t >= tS ? "selection" : "rectangle", fg: hex.Brown, size: 1, style: "Filled" });
        // The selection while dragged, then the pasted copy floating, then nothing.
        let sel = null;
        if (t >= tPress && t < tPaste) {
          const c = selEnd(t);
          sel = { x: COPY.x, y: COPY.y, w: Math.round(c.x) - COPY.x, h: Math.round(c.y) - COPY.y };
        }
        const floating = t >= tPaste && t < tAway;
        if (floating) {
          const f = floatAt(t);
          sel = { x: Math.round(f.x), y: Math.round(f.y), w: COPY.w, h: COPY.h };
        }
        d.paste(t >= tPaste ? (floating ? [sel.x, sel.y] : [PASTED.x, PASTED.y]) : null);
        const turned = t >= tRotate && t < tUndo;
        d.turned(turned);
        e.fresh(turned ? [IH, IW] : [IW, IH]);
        e.overlay({ sel: turned ? null : sel });
        e.menu(t >= tMenu && t < tRotate, { hot: t >= tRotate - 0.45 ? "Rotate 90° Right" : "" });
        V.pointer(t, path);
        const pos = V.track(t, path.map(([at, px, py]) => [at, { x: px, y: py }]));
        e.status({ xy: t >= tMenu && t < tRotate ? null : e.at(pos.x, pos.y), sel: sel ? [sel.w, sel.h] : null });
        V.keys(t, [[tS, "S"], [tCopy, "Ctrl+C"], [tPaste, "Ctrl+V"], [tUndo, "Ctrl+Z"]]);
      };
    },
  });

  // =======================================================================================
  // Double-click an 8K photo: open in under a second; zoom to 3200%.
  V.scene({
    id: "etch-big",
    beats: [{ say: "And it stays quick with big pictures: an 8K photo opens in under a second. Zoom all the way in, and every pixel is a crisp square.", min: 12 }],
    build(root, s) {
      root.innerHTML = `${V.ui.desktop()}${V.ui.files({
        id: "eb-files", x: 70, y: 120, w: 980, h: 600, path: "Pictures",
        items: [{ name: "Screenshots", ext: "dir" }, { name: "drawing.png", ext: "png" }, { name: "mountain.png", ext: "png" }],
      })}`;
      root.insertAdjacentHTML("beforeend", windowHTML({ id: "eb-etch", ...W, title: "mountain.png", body: `<canvas class="etch-photo"></canvas>` }));
      const stat = root.appendChild(V.html(`<div class="etch-stat">${V.tr("etch-stat", "<b>8K</b> PNG open in <b>659 ms</b>")}</div>`));
      const files = root.querySelector("#eb-files");
      const win = root.querySelector("#eb-etch");
      const e = bind(win, { ...W, image: [PW, PH] });
      const item = files.querySelector('[data-name="mountain.png"]');
      const [fx, fy] = V.ui.pos(item, 0.5, 0.4);

      // The photo: the whole of it at a quarter of its pixels, and every pixel of the part
      // the zoom goes to.
      const fit = e.view;
      const P = ridgePoint();
      // Each notch zooms about the pointer, but the first few are held in place by the
      // window's edges (view.rs's clamp), so the pointer is aimed until the last notch
      // lands on P.
      const zoomIn = (anchor) => {
        const out = [fit];
        while (out[out.length - 1].zoom < 32) out.push(zoomAt(out[out.length - 1], nextZoom(out[out.length - 1].zoom), anchor));
        return out;
      };
      const anchor = [fit.origin[0] + P[0] * fit.zoom, fit.origin[1] + P[1] * fit.zoom];
      let steps = zoomIn(anchor);
      for (let i = 0; i < 12; i++) {
        const last = steps[steps.length - 1];
        anchor[0] += (P[0] - (anchor[0] - last.origin[0]) / last.zoom) / 6;
        anchor[1] += (P[1] - (anchor[1] - last.origin[1]) / last.zoom) / 6;
        steps = zoomIn(anchor);
      }
      const at100 = steps.find((v) => v.zoom === 1);
      const rx = Math.floor(-at100.origin[0]) - 2, ry = Math.floor(-at100.origin[1]) - 2;
      const rw = Math.ceil(e.cw) + 4, rh = Math.ceil(e.ch) + 4;
      const lo = photoCanvas(0, 0, PW / 4, PH / 4, 4);
      const hi = photoCanvas(rx, ry, rw, rh, 1);
      const cv = win.querySelector(".etch-photo");
      cv.width = Math.round(e.cw * Z);
      cv.height = Math.round(e.ch * Z);
      const ctx = cv.getContext("2d");
      let drawn = null;
      const draw = (v) => {
        if (drawn === v) return;
        drawn = v;
        V.css(cv, { width: `${e.cw}px`, height: `${e.ch}px` });
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.fillStyle = "#f6f5f4";
        ctx.fillRect(0, 0, cv.width, cv.height);
        ctx.setTransform(Z, 0, 0, Z, 0, 0);
        if (v.zoom < 1) {
          ctx.imageSmoothingEnabled = true;
          ctx.imageSmoothingQuality = "high";
          ctx.drawImage(lo, v.origin[0], v.origin[1], PW * v.zoom, PH * v.zoom);
          ctx.strokeStyle = "rgba(0,0,0,.35)";
          ctx.lineWidth = 1 / Z;
          ctx.strokeRect(v.origin[0] - 0.5 / Z, v.origin[1] - 0.5 / Z, PW * v.zoom + 1 / Z, PH * v.zoom + 1 / Z);
        } else {
          ctx.imageSmoothingEnabled = false;
          ctx.drawImage(hi, v.origin[0] + rx * v.zoom, v.origin[1] + ry * v.zoom, rw * v.zoom, rh * v.zoom);
        }
      };
      // The photo's canvas fills the canvas area and draws the image's boundary itself.
      V.css(win.querySelector(".etch-img"), { left: "0px", top: "0px", width: `${e.cw}px`, height: `${e.ch}px`, boxShadow: "none" });

      const tDbl = 1.1, tShown = tDbl + 0.659;
      const statAt = s.at(0, "8K");
      const tZoom = Math.max(tShown + 2.4, s.at(0, "Zoom") - 0.2), notch = 0.16;
      const pAnchor = e.gpt(TC + anchor[0], HB + MB + anchor[1]);
      const path = [
        [0, 760, 640], [tDbl - 0.5, fx, fy], [tDbl - 0.15, fx, fy, { click: true }], [tDbl, fx, fy, { click: true }],
        [tShown + 0.4, fx + 120, fy + 60],
        [tZoom - 0.7, ...pAnchor], [tZoom + 4, ...pAnchor],
      ];
      return (t) => {
        V.ui.selectFile(files, t >= tDbl - 0.2 ? "mountain.png" : "");
        V.ui.appear(win, t, tDbl + 0.08);
        const shown = t >= tShown;
        e.opening(!shown, t);
        const n = Math.max(0, Math.min(steps.length - 1, Math.floor((t - tZoom) / notch) + 1));
        const v = t >= tZoom ? steps[n] : fit;
        if (shown) {
          draw(v);
          e.setView(v);
        }
        e.state({ tool: "pencil" });
        e.overlay();
        V.pointer(t, path);
        const pos = V.track(t, path.map(([at, px, py]) => [at, { x: px, y: py }]));
        e.status({ xy: shown ? e.at(pos.x, pos.y) : null });
        const k = V.p(t, statAt, statAt + 0.4, V.ease.out) * (1 - V.p(t, tZoom - 0.6, tZoom - 0.2));
        V.css(stat, { opacity: k, transform: `translate(-50%, ${(1 - k) * 16}px)` });
        V.keys(t, [[tZoom, "Ctrl+Scroll"]]);
      };
    },
  });
})();
