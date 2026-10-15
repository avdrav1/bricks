// 3:40 Install, and 4:00 Close.
"use strict";

// The browser at cronkite.cc, the real page from sites/cronkite: scroll to
// the install commands and copy Slate's.
V.scene({
  id: "install-site",
  beats: [{ say: "Getting started takes one command per app.", min: 6.5 }],
  build(root) {
    const zoom = 1.25;
    root.innerHTML = `${V.ui.desktop()}${V.ui.win({
      id: "web", x: 150, y: 74, w: 1620, h: 850, zoom, cls: "web",
      start: V.ui.btn("go-up", "web-back") + V.ui.btn("go-down", "web-fwd"),
      title: `<div class="urlbar"><span class="lock"></span>cronkite.cc</div>`,
      end: V.ui.btn("open-menu"),
      body: `<iframe class="site" title="cronkite.cc"></iframe>`,
    })}`;
    const frame = root.querySelector("iframe");
    // The page as published, minus its web-font links: the video's own
    // copy of League Spartan stands in, so rendering needs no network. Its
    // screenshots' URLs are made absolute: Chromium's preload scanner fetches
    // them before it reads the <base>, against this page's own address.
    V.defer(
      fetch("/sites/cronkite/index.html")
        .then((r) => r.text())
        .then(
          (page) =>
            new Promise((ok) => {
              frame.onload = () => frame.contentDocument.fonts.ready.then(ok);
              frame.srcdoc = page
                .replace(/<link[^>]+fonts\.(googleapis|gstatic)\.com[^>]*>\s*/g, "")
                .replace(/(["\s])shots\//g, "$1/sites/cronkite/shots/")
                .replace("<head>", '<head><base href="/sites/cronkite/"><link rel="stylesheet" href="/fonts.css"><style>html{scrollbar-width:none}</style>');
            }),
        ),
    );
    const scrollAt = 1.0, scrolled = 2.4, click = 4.0;
    let target = 0;
    return (t) => {
      const doc = frame.contentDocument;
      if (!doc?.getElementById("install-slate-cmd")) return;
      // Scroll so Slate's command sits in the middle of the window.
      if (!target) {
        const cmd = doc.getElementById("install-slate-cmd").getBoundingClientRect().top + frame.contentWindow.scrollY;
        target = Math.round(cmd - frame.clientHeight * 0.45);
      }
      const y = Math.round(V.tw(t, scrollAt, scrolled, 0, target));
      if (frame.contentWindow.scrollY !== y) frame.contentWindow.scrollTo(0, y);

      const copy = doc.querySelector('button[data-copy="install-slate-cmd"]');
      V.text(copy, t >= click ? "Copied" : "Copy");
      copy.style.background = t >= scrolled + 0.9 && t < click + 0.3 ? "var(--accent-tint)" : "";
      const r = copy.getBoundingClientRect();
      const [fx, fy] = V.ui.pos(frame, 0, 0);
      const [bx, by] = [fx + (r.left + r.width / 2) * zoom, fy + (r.top + r.height / 2) * zoom];
      V.pointer(t, [[0, 1320, 760], [scrolled, 1320, 760], [scrolled + 0.9, bx, by], [click, bx, by, { click: true }], [click + 2, bx + 40, by + 70]]);
    };
  },
});

// A terminal: paste the Slate install command, and it installs.
V.scene({
  id: "install-terminal",
  beats: [{ say: "It installs for your user only. No sudo, no package manager, and the download is checked before anything is installed.", min: 9 }],
  build(root, s) {
    const cmd = "curl -fsSL https://slate.cronkite.cc/install.sh | sh";
    root.innerHTML = `${V.ui.desktop()}${V.ui.terminal({ id: "t", x: 260, y: 130, w: 1400, h: 600, title: "you@linux: ~" })}
      <div class="notes">${V.tr("install-notes", ["Just for you: ~/.local", "No sudo", "No package manager", "Checked against its sha256"]).map((x) => `<span>${x}</span>`).join("")}</div>`;
    const term = root.querySelector("#t");
    const notes = [...root.querySelectorAll(".notes span")];
    const noteAt = [s.at(0, "for your user"), s.at(0, "No sudo"), s.at(0, "no package"), s.at(0, "the download is checked")];
    const paste = 0.7, enter = 1.5, downloaded = 1.9, installed = 3.1;
    return (t) => {
      const lines = [V.ui.prompt(t >= paste ? cmd : "")];
      if (t >= downloaded) lines.push("Downloading Slate 0.3.0");
      if (t >= installed) lines.push("Installed Slate 0.3.0", "  /home/you/.local/bin/slate; Word documents (.docx) and Markdown (.md) open with it");
      if (t >= installed + 0.1) lines.push(V.ui.prompt());
      V.ui.term(term, lines, t, t < enter || t >= installed + 0.1);
      V.keys(t, [[paste - 0.25, "Ctrl+Shift+V"], [enter, "Enter"]]);
      notes.forEach((el, i) => {
        const k = V.p(t, noteAt[i], noteAt[i] + 0.35, V.ease.out);
        V.css(el, { opacity: k, transform: `translateY(${(1 - k) * 14}px)` });
      });
    };
  },
});

// Double-click a .docx: Slate opens it. Then a .pptx: Frame does. Then a .png: Etch does.
V.scene({
  id: "install-open",
  beats: [{ say: "After that, your files just open in the right app.", min: 10.5 }],
  build(root) {
    const items = [
      { name: "report.docx", ext: "docx" }, { name: "deck.pptx", ext: "pptx" }, { name: "notes.md", ext: "md" },
      { name: "budget.xlsx", ext: "xlsx" }, { name: "big.csv", ext: "csv" }, { name: "poster.png", ext: "png" },
    ];
    root.innerHTML = `${V.ui.desktop()}${V.ui.files({ id: "files", x: 120, y: 110, w: 1000, h: 640, path: "Documents", items })}`;
    // Slate's, Frame's, and Etch's windows come from their scene files.
    if (!V.slate || !V.frame || !V.etch) return console.error("install-open needs V.slate.window, V.frame.window, and V.etch.window");
    const etchRect = { x: 470, y: 120, w: 1360, h: 800 };
    root.insertAdjacentHTML(
      "beforeend",
      `${V.slate.window({ id: "slate", x: 560, y: 90, w: 1180, h: 840 })}${V.frame.window({ id: "frame", x: 330, y: 150, w: 1460, h: 820 })}${V.etch.window({ id: "etch", ...etchRect })}`,
    );
    const files = root.querySelector("#files");
    const slate = root.querySelector("#slate");
    const frame = root.querySelector("#frame");
    const etch = root.querySelector("#etch");
    V.etch.bind(etch, etchRect);
    const docx = files.querySelector('[data-name="report.docx"]');
    const pptx = files.querySelector('[data-name="deck.pptx"]');
    const png = files.querySelector('[data-name="poster.png"]');
    const open1 = 1.3, back = 3.6, open2 = 4.6, back2 = 6.6, open3 = 7.6;
    return (t) => {
      const [dx, dy] = V.ui.pos(docx, 0.5, 0.4);
      const [px, py] = V.ui.pos(pptx, 0.5, 0.4);
      const [gx, gy] = V.ui.pos(png, 0.5, 0.4);
      V.ui.selectFile(files, t >= open3 - 0.2 ? "poster.png" : t >= open2 - 0.2 ? "deck.pptx" : t >= open1 - 0.2 ? "report.docx" : "");
      // Back to Files for each next file: the apps opened stay open behind it.
      V.css(files, { zIndex: t >= back2 ? 5 : t >= back ? 3 : 1 });
      V.css(slate, { zIndex: 2 });
      V.css(frame, { zIndex: 4 });
      V.css(etch, { zIndex: 6 });
      V.ui.appear(slate, t, open1 + 0.35);
      V.ui.appear(frame, t, open2 + 0.35);
      V.ui.appear(etch, t, open3 + 0.35);
      V.pointer(t, [
        [0, 900, 650], [open1 - 0.5, dx, dy], [open1 - 0.15, dx, dy, { click: true }], [open1, dx, dy, { click: true }],
        [back - 0.4, dx + 60, dy + 380], [back, 300, 700, { click: true }], [open2 - 0.5, px, py], [open2 - 0.15, px, py, { click: true }], [open2, px, py, { click: true }],
        [back2 - 0.4, 260, 620], [back2, 230, 640, { click: true }], [open3 - 0.5, gx, gy], [open3 - 0.15, gx, gy, { click: true }], [open3, gx, gy, { click: true }],
        [open3 + 2, gx + 420, gy + 260],
      ]);
    };
  },
});

// The four icons, then the URL.
V.scene({
  id: "close",
  xfade: 0.6,
  beats: [{ say: "Cronkite. Fast, native, and open source. Brick, Slate, Frame, and Etch are all available today, at cronkite.cc." }, { dur: 2.5 }],
  build(root, s) {
    root.innerHTML = `
      <div class="paper close">
        <div class="close-icons">${["brick", "slate", "frame", "etch"].map((a) => V.ui.appIcon(a, 168)).join("")}</div>
        <h1 class="close-url">cronkite.cc</h1>
        <p class="close-line">${V.tr("close-line", "Brick, Slate, Frame, and Etch · Available now · Open source")}</p>
        <div class="close-black"></div>
      </div>`;
    const icons = [...root.querySelectorAll(".close-icons svg")];
    const url = root.querySelector(".close-url");
    const line = root.querySelector(".close-line");
    const black = root.querySelector(".close-black");
    const urlAt = s.at(0, "Brick, Slate");
    return (t) => {
      icons.forEach((el, i) => {
        const k = V.p(t, 0.3 + i * 0.2, 1.0 + i * 0.2, V.ease.back);
        V.css(el, { opacity: V.p(t, 0.3 + i * 0.2, 0.7 + i * 0.2), transform: `translateY(${(1 - k) * 40}px)` });
      });
      const u = V.p(t, urlAt - 0.6, urlAt + 0.2, V.ease.out);
      V.css(url, { opacity: u, transform: `translateY(${(1 - u) * 20}px)` });
      V.css(line, { opacity: V.p(t, urlAt, urlAt + 0.6) });
      V.css(black, { opacity: V.p(t, s.dur - 1.0, s.dur, V.ease.linear) });
    };
  },
});
