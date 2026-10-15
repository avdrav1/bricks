// 0:00 Title card, and What Cronkite is.
"use strict";

// Title card: the video opens on it, with the music's first hit.
V.scene({
  id: "title",
  xfade: 0,
  beats: [{ dur: 4.5 }],
  build(root) {
    root.innerHTML = `
      <div class="title-card">
        <h1 class="wordmark">Cronkite</h1>
        <p class="tagline">${V.tr("tagline", "A fast, native office suite for Linux.")}</p>
      </div>`;
    const [mark, tag] = root.querySelectorAll(".wordmark, .tagline");
    return (t) => {
      const a = V.p(t, 0.5, 1.3, V.ease.out);
      V.css(mark, { opacity: a, transform: `translateY(${(1 - a) * 24}px)` });
      const b = V.p(t, 1.0, 1.8, V.ease.out);
      V.css(tag, { opacity: b, transform: `translateY(${(1 - b) * 16}px)` });
    };
  },
});

// Four app icons slide in side by side, each named as the narrator says it.
V.scene({
  id: "suite-icons",
  beats: [{ say: "Cronkite is an office suite for Linux, built from scratch in Rust. It has four apps: Brick for spreadsheets, Slate for documents, Frame for presentations, and Etch for images." }],
  build(root, s) {
    const apps = [
      ["brick", "Brick", "Spreadsheets", "Brick for"],
      ["slate", "Slate", "Documents", "Slate for"],
      ["frame", "Frame", "Presentations", "Frame for"],
      ["etch", "Etch", "Images", "Etch for"],
    ];
    root.innerHTML = `
      <div class="paper">
        <p class="kicker">An office suite for Linux · built from scratch in Rust</p>
        <div class="suite-apps">${apps.map(([id, name, kind]) => `<div class="app-card" data-app="${id}">${V.ui.appIcon(id, 220)}<h2>${name}</h2><span>${kind}</span></div>`).join("")}</div>
      </div>`;
    const kicker = root.querySelector(".kicker");
    const cards = [...root.querySelectorAll(".app-card")];
    const named = apps.map(([, , , cue]) => s.at(0, cue));
    return (t) => {
      V.css(kicker, { opacity: V.p(t, 0.3, 1.0) });
      cards.forEach((card, i) => {
        const k = V.p(t, 0.5 + i * 0.25, 1.4 + i * 0.25, V.ease.out);
        const lit = V.p(t, named[i], named[i] + 0.35) * (1 - V.p(t, named[i] + 1.6, named[i] + 2.0));
        V.css(card, { opacity: k, transform: `translateX(${(1 - k) * 260}px) scale(${1 + 0.06 * lit})` });
        V.css(card.querySelector("span"), { opacity: 0.55 + 0.45 * V.p(t, named[i], named[i] + 0.4) });
        V.cls(card, "lit", lit > 0.5);
      });
    };
  },
});

// The icons in GNOME's app grid, next to Files and Settings.
V.scene({
  id: "app-grid",
  beats: [{ say: "They're native apps. No browser tab, no subscription, no account. Each one installs and runs on its own, so you take only what you need." }],
  build(root, s) {
    const grid = [
      ["files", "Files"], ["settings", "Settings"], ["terminal", "Console"], ["browser", "Web"], ["text", "Text Editor"], ["calendar", "Calendar"],
      ["software", "Software"], ["photos", "Image Viewer"], ["brick", "Brick"], ["slate", "Slate"], ["frame", "Frame"], ["etch", "Etch"],
    ];
    const ours = new Set(["brick", "slate", "frame", "etch"]);
    root.innerHTML = `
      ${V.ui.desktop({ dim: true })}
      <div class="appgrid-search">${V.ui.icon("edit-find", 18)}<span>Type to search</span></div>
      <div class="appgrid">${grid
        .map(([id, label]) => `<div class="ag-item${ours.has(id) ? " ours" : ""}" data-app="${id}">${ours.has(id) ? V.ui.appIcon(id, 112) : V.ui.otherIcon(id, 112)}<span>${label}</span></div>`)
        .join("")}</div>
      <div class="nopes">${["No browser tab", "No subscription", "No account"].map((x) => `<span>${x}</span>`).join("")}</div>`;
    const items = [...root.querySelectorAll(".ag-item")];
    const nopes = [...root.querySelectorAll(".nopes span")];
    const nopeAt = [s.at(0, "No browser"), s.at(0, "no subscription"), s.at(0, "no account")];
    const own = s.at(0, "Each one");
    return (t) => {
      items.forEach((el, i) => {
        const k = V.p(t, 0.1 + i * 0.04, 0.6 + i * 0.04, V.ease.out);
        V.css(el, { opacity: k, transform: `scale(${0.85 + 0.15 * k})` });
      });
      // While the narrator says each one installs on its own, the four
      // lift out one by one.
      items.filter((el) => ours.has(el.dataset.app)).forEach((el, i) => {
        const lift = V.p(t, own + i * 0.35, own + 0.4 + i * 0.35, V.ease.out);
        V.css(el.querySelector("svg"), { transform: `translateY(${-10 * lift}px)` });
        V.cls(el, "lit", lift > 0.5);
      });
      nopes.forEach((el, i) => {
        const k = V.p(t, nopeAt[i], nopeAt[i] + 0.35, V.ease.out);
        V.css(el, { opacity: k, transform: `translateY(${(1 - k) * 14}px)` });
      });
    };
  },
});
