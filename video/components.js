// Pieces every scene draws with: the GNOME desktop, GTK 4 windows with a
// header bar (the apps are plain GTK 4 on the Adwaita theme), symbolic icons,
// the app icons, a terminal, the Files app, popover menus.
"use strict";

V.ui = (() => {
  // Symbolic icons, 16px, drawn in currentColor.
  const ICONS = {
    "document-open": '<path d="M1.5 3.5h5l1.5 1.5h6.5v8.5h-13z" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/>',
    "document-new": '<path d="M3.5 1.5h6l3 3v10h-9z" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><path d="M8 7v5M5.5 9.5h5" stroke="currentColor" stroke-width="1.5"/>',
    "document-save": '<path d="M2 2h9.5L14 4.5V14H2z" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><rect x="4.5" y="2.5" width="6" height="3.5" fill="currentColor"/><rect x="4.5" y="9" width="7" height="4" fill="currentColor" opacity=".55"/>',
    "document-save-as": '<path d="M2 2h9.5L14 4.5V9M9 14H2V2" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><rect x="4.5" y="2.5" width="6" height="3.5" fill="currentColor"/><path d="M10.5 15l4-4-1.5-1.5-4 4V15z" fill="currentColor"/>',
    "open-menu": '<path d="M2 4h12M2 8h12M2 12h12" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>',
    "pan-down": '<path d="M4 6l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>',
    "window-close": '<path d="M4.5 4.5l7 7M11.5 4.5l-7 7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>',
    "go-up": '<path d="M3.5 10l4.5-4.5 4.5 4.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>',
    "go-down": '<path d="M3.5 6l4.5 4.5 4.5-4.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>',
    "go-next": '<path d="M6 3.5l4.5 4.5L6 12.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>',
    "edit-find": '<circle cx="6.5" cy="6.5" r="4.2" fill="none" stroke="currentColor" stroke-width="1.6"/><path d="M9.7 9.7l4.3 4.3" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>',
    "edit-find-replace": '<circle cx="6" cy="6" r="3.6" fill="none" stroke="currentColor" stroke-width="1.5"/><path d="M8.7 8.7L11 11" stroke="currentColor" stroke-width="1.6"/><path d="M10 14.5h5M13 12.5l2 2-2 2" fill="none" stroke="currentColor" stroke-width="1.3"/>',
    "edit-clear": '<path d="M6 3h8v10H6L2 8z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/><path d="M8 6l4 4M12 6l-4 4" stroke="currentColor" stroke-width="1.4"/>',
    "media-playback-start": '<path d="M4.5 2.5v11l9-5.5z" fill="currentColor"/>',
    "list-add": '<path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>',
    "view-list-bullet": '<circle cx="3" cy="4" r="1.3" fill="currentColor"/><circle cx="3" cy="8" r="1.3" fill="currentColor"/><circle cx="3" cy="12" r="1.3" fill="currentColor"/><path d="M6.5 4h7.5M6.5 8h7.5M6.5 12h7.5" stroke="currentColor" stroke-width="1.5"/>',
    "format-justify-left": '<path d="M2 3h12M2 6.3h8M2 9.6h12M2 13h8" stroke="currentColor" stroke-width="1.5"/>',
    "format-justify-center": '<path d="M2 3h12M4 6.3h8M2 9.6h12M4 13h8" stroke="currentColor" stroke-width="1.5"/>',
    "format-justify-right": '<path d="M2 3h12M6 6.3h8M2 9.6h12M6 13h8" stroke="currentColor" stroke-width="1.5"/>',
    "format-justify-fill": '<path d="M2 3h12M2 6.3h12M2 9.6h12M2 13h12" stroke="currentColor" stroke-width="1.5"/>',
    "format-text-bold": '<text x="8" y="13" text-anchor="middle" font-family="Inter" font-weight="800" font-size="13" fill="currentColor">B</text>',
    "format-text-italic": '<text x="8.5" y="13" text-anchor="middle" font-family="Caladea" font-style="italic" font-weight="700" font-size="14" fill="currentColor">I</text>',
    "format-text-underline": '<text x="8" y="11.5" text-anchor="middle" font-family="Inter" font-weight="600" font-size="12" fill="currentColor">U</text><path d="M3.5 14.5h9" stroke="currentColor" stroke-width="1.4"/>',
    "format-text-strikethrough": '<text x="8" y="13" text-anchor="middle" font-family="Inter" font-weight="600" font-size="12" fill="currentColor">S</text><path d="M2.5 8.6h11" stroke="currentColor" stroke-width="1.4"/>',
    "folder": '<path d="M1.5 3.5h5l1.5 1.5h6.5v8.5h-13z" fill="currentColor"/>',
    "go-home": '<path d="M2 8l6-5.5L14 8M4 7v6.5h8V7" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/>',
    "edit-copy": '<rect x="5" y="5" width="9" height="9.5" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.5"/><path d="M2.5 11V2.5H10" fill="none" stroke="currentColor" stroke-width="1.5"/>',
  };
  const icon = (name, size = 16) =>
    `<svg class="icon" width="${size}" height="${size}" viewBox="0 0 16 16" aria-hidden="true">${ICONS[name] || ""}</svg>`;

  // The apps' icons, from packaging/.
  const APP_ICONS = {
    brick: '<rect x="12" y="16" width="104" height="96" rx="10" fill="#ffffff" stroke="#2f6f4e" stroke-width="4"/><path d="M14 26a8 8 0 0 1 8-8h84a8 8 0 0 1 8 8v14H14z" fill="#3a8a5f"/><g stroke="#9cc5ad" stroke-width="3"><path d="M14 64h100M14 88h100"/><path d="M46 40v70M80 40v70"/></g><rect x="47.5" y="65.5" width="31" height="21" fill="none" stroke="#1f5fbf" stroke-width="4"/>',
    slate: '<path d="M28 10h52l24 24v80a6 6 0 0 1-6 6H28a6 6 0 0 1-6-6V16a6 6 0 0 1 6-6z" fill="#ffffff" stroke="#34465a" stroke-width="4" stroke-linejoin="round"/><path d="M80 10v18a6 6 0 0 0 6 6h18" fill="#dfe6ee" stroke="#34465a" stroke-width="4" stroke-linejoin="round"/><rect x="36" y="44" width="40" height="8" rx="2" fill="#3f5a78"/><g stroke="#9fb1c4" stroke-width="5" stroke-linecap="round"><path d="M38 66h52M38 80h52M38 94h34"/></g><path d="M80 92v12" stroke="#1f5fbf" stroke-width="4" stroke-linecap="round"/>',
    frame: '<rect x="12" y="20" width="104" height="72" rx="8" fill="#ffffff" stroke="#b5532d" stroke-width="4"/><path d="M14 28a6 6 0 0 1 6-6h88a6 6 0 0 1 6 6v8H14z" fill="#d9683a"/><rect x="26" y="46" width="44" height="7" rx="3.5" fill="#b5532d"/><g fill="#e9b39c"><rect x="26" y="60" width="36" height="5" rx="2.5"/><rect x="26" y="71" width="28" height="5" rx="2.5"/></g><rect x="76" y="46" width="26" height="30" rx="3" fill="#f3d2c3"/><path d="M64 92v14M44 112l20-8 20 8" fill="none" stroke="#b5532d" stroke-width="4" stroke-linecap="round" stroke-linejoin="round"/>',
    // The checker square in the top-left corner is cut to the corner's curve, in place of
    // the packaged icon's clip path (its id would repeat on the page).
    etch: '<rect x="14" y="14" width="100" height="100" rx="10" fill="#ffffff"/><g fill="#d9d5f3"><path d="M14 30v-6a10 10 0 0 1 10-10h6v16z"/><rect x="46" y="14" width="16" height="16"/><rect x="30" y="30" width="16" height="16"/><rect x="14" y="46" width="16" height="16"/></g><rect x="14" y="14" width="100" height="100" rx="10" fill="none" stroke="#4a3fb8" stroke-width="4"/><path d="M28 90c10-22 22-30 30-18s18 6 26-14" fill="none" stroke="#e8483c" stroke-width="7" stroke-linecap="round"/><path d="M74 98l8-20 30-30 12 12-30 30z" fill="#f2b33d" stroke="#4a3fb8" stroke-width="4" stroke-linejoin="round"/><path d="M74 98l8-20 12 12z" fill="#fbe3b5" stroke="#4a3fb8" stroke-width="4" stroke-linejoin="round"/><path d="M74 98l3-8 5 5z" fill="#4a3fb8"/><path d="M104 56l12 12" stroke="#4a3fb8" stroke-width="4"/>',
  };
  const appIcon = (app, size = 128) => `<svg class="appicon" width="${size}" height="${size}" viewBox="0 0 128 128">${APP_ICONS[app]}</svg>`;

  // Other apps' icons in the app grid and the dock: simple stand-ins.
  const otherIcon = (kind, size = 128) => {
    const shapes = {
      files: '<path d="M14 30a8 8 0 0 1 8-8h30l10 10h44a8 8 0 0 1 8 8v60a8 8 0 0 1-8 8H22a8 8 0 0 1-8-8z" fill="#438de6"/><path d="M14 46h100v54a8 8 0 0 1-8 8H22a8 8 0 0 1-8-8z" fill="#62a0ea"/>',
      settings: '<circle cx="64" cy="64" r="48" fill="#77767b"/><circle cx="64" cy="64" r="34" fill="#9a9996"/><circle cx="64" cy="64" r="14" fill="#deddda"/>',
      terminal: '<rect x="12" y="18" width="104" height="92" rx="12" fill="#241f31"/><path d="M32 50l16 14-16 14" fill="none" stroke="#fff" stroke-width="7" stroke-linecap="round" stroke-linejoin="round"/><path d="M58 80h28" stroke="#fff" stroke-width="7" stroke-linecap="round"/>',
      browser: '<circle cx="64" cy="64" r="50" fill="#3584e4"/><path d="M14 64h100M64 14c-18 16-18 84 0 100M64 14c18 16 18 84 0 100" fill="none" stroke="#99c1f1" stroke-width="5"/>',
      text: '<rect x="24" y="12" width="80" height="104" rx="8" fill="#f6f5f4" stroke="#c0bfbc" stroke-width="3"/><path d="M40 40h48M40 56h48M40 72h32" stroke="#5e5c64" stroke-width="6" stroke-linecap="round"/>',
      calendar: '<rect x="16" y="20" width="96" height="92" rx="10" fill="#fff" stroke="#c0bfbc" stroke-width="3"/><path d="M16 30a10 10 0 0 1 10-10h76a10 10 0 0 1 10 10v14H16z" fill="#e01b24"/><text x="64" y="96" text-anchor="middle" font-family="Inter" font-weight="700" font-size="44" fill="#3d3846">13</text>',
      software: '<rect x="18" y="28" width="92" height="80" rx="12" fill="#f6d32d"/><path d="M42 28a22 22 0 0 1 44 0" fill="none" stroke="#e5a50a" stroke-width="7"/>',
      photos: '<rect x="14" y="22" width="100" height="84" rx="10" fill="#33d17a"/><circle cx="44" cy="50" r="10" fill="#fff"/><path d="M14 92l32-26 22 18 18-14 28 22v4a10 10 0 0 1-10 10H24a10 10 0 0 1-10-10z" fill="#26a269"/>',
      office: '<rect x="20" y="12" width="88" height="104" rx="6" fill="#fff" stroke="#9a9996" stroke-width="3"/><path d="M20 18a6 6 0 0 1 6-6h76a6 6 0 0 1 6 6v18H20z" fill="#18a303"/>',
      writer: '<rect x="20" y="12" width="88" height="104" rx="6" fill="#fff" stroke="#9a9996" stroke-width="3"/><path d="M20 18a6 6 0 0 1 6-6h76a6 6 0 0 1 6 6v18H20z" fill="#2a6ad8"/><path d="M36 56h56M36 70h56M36 84h40" stroke="#9a9996" stroke-width="5" stroke-linecap="round"/>',
    };
    return `<svg class="appicon" width="${size}" height="${size}" viewBox="0 0 128 128">${shapes[kind]}</svg>`;
  };

  // A document icon for the Files app, by extension.
  function fileIcon(ext, size = 64) {
    const colour = { csv: "#26a269", xlsx: "#26a269", docx: "#1c71d8", md: "#5e5c64", txt: "#5e5c64", pptx: "#e66100", jpg: "#9141ac", png: "#9141ac" }[ext] || "#77767b";
    const pic = ext === "jpg" || ext === "png";
    return `<svg class="fileicon" width="${size}" height="${size}" viewBox="0 0 64 64">
      <path d="M14 4h26l12 12v42a2 2 0 0 1-2 2H14a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z" fill="#fff" stroke="#c0bfbc" stroke-width="1.5"/>
      <path d="M40 4v10a2 2 0 0 0 2 2h10" fill="#deddda" stroke="#c0bfbc" stroke-width="1.5"/>
      ${pic ? `<rect x="17" y="22" width="30" height="24" rx="2" fill="${colour}" opacity=".85"/><circle cx="25" cy="30" r="3.5" fill="#fff"/><path d="M17 44l10-8 7 5 6-4 7 5v3H17z" fill="#fff" opacity=".7"/>` : `<rect x="17" y="40" width="30" height="12" rx="2" fill="${colour}"/><text x="32" y="49.5" text-anchor="middle" font-family="Inter" font-weight="700" font-size="9" fill="#fff">${ext.toUpperCase()}</text><path d="M19 22h26M19 28h26M19 34h18" stroke="#c0bfbc" stroke-width="2"/>`}
    </svg>`;
  }

  // The desktop: wallpaper and GNOME's top bar.
  const desktop = ({ clock = "Tue Oct 13  10:24", dim = false } = {}) => `
    <div class="desktop${dim ? " dim" : ""}">
      <div class="wallpaper"></div>
      <div class="topbar">
        <div class="ws"><i class="on"></i><i></i><i></i></div>
        <div class="clock">${clock}</div>
        <div class="tray"><b></b><b></b><b class="batt"></b></div>
      </div>
    </div>`;

  // A GTK 4 window with a header bar. x, y, w, h in stage pixels; the
  // contents are laid out at 1/zoom of that, like a 125% scaled desktop.
  function win({ id = "", x, y, w, h, title = "", start = "", end = "", body = "", zoom = 1.25, cls = "" }) {
    return `
      <div class="winpos ${cls}" ${id ? `id="${id}"` : ""} style="left:${x}px;top:${y}px;width:${w}px;height:${h}px">
        <div class="win" style="zoom:${zoom};width:${w / zoom}px;height:${h / zoom}px">
          <header class="headerbar">
            <div class="hb-start">${start}</div>
            <div class="hb-title">${title}</div>
            <div class="hb-end">${end}<button class="hb-btn close">${icon("window-close")}</button></div>
          </header>
          <div class="win-body">${body}</div>
        </div>
      </div>`;
  }
  const btn = (iconName, extra = "") => `<button class="hb-btn ${extra}">${icon(iconName)}</button>`;
  const textBtn = (label, extra = "") => `<button class="hb-btn text ${extra}">${label}</button>`;

  // A window that opens: fades and grows in from t0.
  function appear(el, t, t0, d = 0.22) {
    const k = V.p(t, t0, t0 + d, V.ease.out);
    V.css(el, { opacity: k.toFixed(3), transform: `scale(${(0.94 + 0.06 * k).toFixed(4)})`, display: t >= t0 ? "" : "none" });
  }

  // A popover menu: items are [label, accel] pairs, "-" for a separator,
  // or { label, accel, check, sub, hot } objects. `hot` highlights the row.
  function menu(items, { cls = "" } = {}) {
    const rows = items
      .map((it) => {
        if (it === "-") return '<div class="mi-sep"></div>';
        const o = Array.isArray(it) ? { label: it[0], accel: it[1] } : it;
        if (o.heading) return `<div class="mi-head">${o.heading}</div>`;
        return `<div class="mi${o.hot ? " hot" : ""}" data-label="${o.label}">${o.radio !== undefined ? `<span class="radio${o.radio ? " on" : ""}"></span>` : ""}<span class="mi-label">${o.label}</span>${o.accel ? `<span class="accel">${o.accel}</span>` : ""}${o.sub ? `<span class="sub">${icon("go-next")}</span>` : ""}</div>`;
      })
      .join("");
    return `<div class="popover ${cls}">${rows}</div>`;
  }

  // A terminal (GNOME Console look). Render lines into `.term-body` with term().
  const terminal = ({ id = "", x, y, w, h, title = "~", zoom = 1.25 }) =>
    win({ id, x, y, w, h, zoom, title: `<span>${title}</span>`, cls: "terminal", body: '<pre class="term-body"></pre>' });
  // lines: strings of trusted markup; the caret shows after the last line.
  function term(el, lines, t, caret = true) {
    const body = el.querySelector(".term-body");
    V.markup(body, lines.join("\n") + (caret ? `<span class="tcaret${V.blink(t) ? "" : " off"}"> </span>` : ""));
  }
  const prompt = (cmd = "", dir = "~") => `<span class="pr">you@linux</span>:<span class="pd">${dir}</span>$ ${cmd}`;

  // The Files app at a folder. items: [{ name, ext, size }]; one row per file.
  function files({ id = "", x, y, w, h, path = "Documents", items, zoom = 1.25 }) {
    const grid = items
      .map(
        (f) => `<div class="fitem" data-name="${f.name}">${f.ext === "dir" ? `<span class="foldericon">${icon("folder", 56)}</span>` : fileIcon(f.ext)}<span class="fname">${f.name}</span></div>`,
      )
      .join("");
    const side = ["Recent", "Starred", "Home", "Documents", "Downloads", "Music", "Pictures", "Videos", "Trash"]
      .map((s) => `<div class="side-row${s === path.split("/")[0] ? " on" : ""}">${icon(s === "Home" ? "go-home" : "folder")}<span>${s}</span></div>`)
      .join("");
    return win({
      id, x, y, w, h, zoom, cls: "files",
      start: btn("go-up", "back"),
      title: `<div class="pathbar">${icon("go-home")}<span>/</span>${path.split("/").map((s) => `<span class="crumb">${s}</span>`).join("<span>/</span>")}</div>`,
      end: btn("edit-find") + btn("open-menu"),
      body: `<div class="files-side">${side}</div><div class="files-grid">${grid}</div>`,
    });
  }
  // Highlight a file in a Files window.
  function selectFile(el, name) {
    for (const f of el.querySelectorAll(".fitem")) V.cls(f, "sel", f.dataset.name === name);
  }

  // Centre of an element in stage pixels (for pointing at it).
  function pos(el, ax = 0.5, ay = 0.5) {
    const stage = document.getElementById("stage");
    const s = stage.getBoundingClientRect();
    const k = s.width / V.W;
    const r = el.getBoundingClientRect();
    return [(r.left - s.left + r.width * ax) / k, (r.top - s.top + r.height * ay) / k];
  }

  return { icon, appIcon, otherIcon, fileIcon, desktop, win, btn, textBtn, appear, menu, terminal, term, prompt, files, selectFile, pos };
})();
