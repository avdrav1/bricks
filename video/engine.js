// The video's timeline. Every scene draws itself as a pure function of time,
// so any frame can be rendered on its own: the renderer seeks frame by frame
// in parallel browser pages, and the preview just seeks to the clock.
"use strict";

const V = (() => {
  const W = 1920, H = 1080, FPS = 30;
  // Narration pace: about 145 words a minute, plus a breath after each line.
  const WORDS_PER_SEC = 2.4, BREATH = 0.6;

  const scenes = [];
  const captions = [];
  let total = 0;
  let stage, captionEl, hudEl, pointerEl;

  // ---- maths -------------------------------------------------------------
  const clamp = (x, a = 0, b = 1) => Math.min(b, Math.max(a, x));
  const lerp = (a, b, k) => a + (b - a) * k;
  const ease = {
    linear: (k) => k,
    inOut: (k) => (k < 0.5 ? 4 * k * k * k : 1 - Math.pow(-2 * k + 2, 3) / 2),
    out: (k) => 1 - Math.pow(1 - k, 3),
    in: (k) => k * k * k,
    back: (k) => 1 + 2.2 * Math.pow(k - 1, 3) + 1.2 * Math.pow(k - 1, 2),
  };
  // Progress of t through [t0, t1], eased, clamped to 0..1.
  const p = (t, t0, t1, e = ease.inOut) => (t1 <= t0 ? (t >= t1 ? 1 : 0) : e(clamp((t - t0) / (t1 - t0))));
  // Tween a number from a to b over [t0, t1].
  const tw = (t, t0, t1, a, b, e) => lerp(a, b, p(t, t0, t1, e));
  // Keyframes: [[time, {x: 1, y: 2}], ...] -> the interpolated object at t.
  function track(t, keys, e = ease.inOut) {
    if (t <= keys[0][0]) return { ...keys[0][1] };
    for (let i = 1; i < keys.length; i++) {
      const [t1, b] = keys[i];
      if (t < t1) {
        const [t0, a] = keys[i - 1];
        const k = p(t, t0, t1, keys[i][2] || e);
        const out = {};
        for (const key in b) out[key] = typeof b[key] === "number" ? lerp(a[key] ?? b[key], b[key], k) : k < 1 ? a[key] : b[key];
        return out;
      }
    }
    return { ...keys[keys.length - 1][1] };
  }
  // Text typed from t0 at cps characters a second.
  const typed = (text, t, t0, cps = 16) => text.slice(0, Math.max(0, Math.floor((t - t0) * cps)));
  const typedEnd = (text, t0, cps = 16) => t0 + text.length / cps;
  // Deterministic noise for generated data: same input, same output.
  function hash(...xs) {
    let h = 2166136261 >>> 0;
    for (const x of xs) {
      h ^= x >>> 0;
      h = Math.imul(h, 16777619);
      h ^= h >>> 13;
      h = Math.imul(h, 0x5bd1e995);
      h ^= h >>> 15;
    }
    return (h >>> 0) / 4294967296;
  }
  // Is a caret's blink on at t?
  const blink = (t) => Math.floor(t * 1.9) % 2 === 0;
  const fmt = (n) => n.toLocaleString("en-US");

  // ---- DOM helpers -------------------------------------------------------
  const html = (s) => {
    const tpl = document.createElement("template");
    tpl.innerHTML = s.trim();
    return tpl.content.firstElementChild;
  };
  // Set a property only when it changes; the renderer seeks thousands of times.
  function css(el, styles) {
    if (!el) return;
    for (const k in styles) {
      const v = String(styles[k]);
      if (el.__css?.[k] !== v) {
        (el.__css ??= {})[k] = v;
        if (k.startsWith("--")) el.style.setProperty(k, v);
        else el.style[k] = v;
      }
    }
  }
  function text(el, s) {
    if (el && el.__text !== s) {
      el.__text = s;
      el.textContent = s;
    }
  }
  function markup(el, s) {
    if (el && el.__html !== s) {
      el.__html = s;
      el.innerHTML = s;
    }
  }
  function cls(el, name, on) {
    if (el && el.classList.contains(name) !== !!on) el.classList.toggle(name, !!on);
  }
  const show = (el, on) => css(el, { display: on ? "" : "none" });

  // ---- narration -------------------------------------------------------------
  // Each beat's line is spoken a sentence at a time. With the voiceover made
  // (voice.mjs fills .voice/ and index.html hands its manifest to setVoice),
  // a sentence lasts as long as its clip; without it, as long as it takes at
  // WORDS_PER_SEC. Pictures sync to words through s.at(), either way.
  const LEAD = 0.15, GAP = 0.18;
  const words = (s) => s.replace(/[*_`]/g, "").split(/\s+/).filter(Boolean).length;
  const speech = (s) => words(s) / WORDS_PER_SEC;
  // How the narrator says what the captions write.
  const SAY = [
    [/`/g, ""],
    [/\b00123\b/g, "zero zero one two three"],
    [/cronkite\.cc/g, "cronkite dot C C"],
    [/Ctrl\+N/g, "Control N"],
  ];
  const spoken = (text) => SAY.reduce((s, [re, to]) => s.replace(re, to), text);
  const sentences = (line) => line.split(/(?<=[.?!])\s+/);
  const clipKey = (text) => {
    let h = 2166136261 >>> 0;
    for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 16777619) >>> 0;
    return h.toString(16).padStart(8, "0");
  };
  let voice = null; // { voice, clips: { key: { file, dur } } }
  const setVoice = (manifest) => (voice = manifest);

  // scene({ id, beats: [{ say, min, dur }], xfade, build(root, ctx) -> update(t, ctx) })
  // A beat lasts its narration plus a breath, or `min` if the picture needs
  // longer, or exactly `dur`. ctx.b[i] has each beat's local start and end.
  // xfade: seconds this scene dissolves in over the one before (0: a cut).
  function scene(def) {
    scenes.push(def);
  }
  // The edit: scene ids in the order they play. Every scene must be in it once.
  let edit = null;
  function order(ids) {
    edit = ids;
  }
  // cut(name, clips): another, shorter edit of the same scenes, played with
  // ?cut=name. A clip { scene, from, to, say, lead, xfade } shows that scene
  // from `from` to `to` (seconds in its own time) under its own narration
  // `say`, which starts `lead` seconds in; it lasts the window, or longer if
  // the line needs it, holding the last frame.
  const cuts = {};
  const cut = (name, clips) => (cuts[name] = clips);
  // What plays: the scenes in the edit's order, or a cut's clips. Each shot:
  // { id, root, xfade, b, dur, start, end, update(localTime) }.
  let shots = [];
  // Work a scene's build starts but can't finish at once (an iframe loading,
  // an image decoding): rendering waits for every promise passed here.
  const pending = [];
  const defer = (promise) => (pending.push(promise), promise);
  const settled = () => Promise.all(pending);

  function layoutBeats(beats) {
    let local = 0;
    const b = beats.map((beat) => {
      // Each sentence: its text, where it starts in the line, its clip, its timing in the beat.
      let at = beat.lead ?? LEAD, from = 0;
      const lines = (beat.say ? sentences(beat.say) : []).map((text) => {
        const key = clipKey(spoken(text));
        const line = { text, key, off: beat.say.indexOf(text, from), dur: voice?.clips[key]?.dur ?? speech(text), start: at };
        from = line.off + text.length;
        at += line.dur + GAP;
        return line;
      });
      const talk = lines.length ? at - GAP : 0;
      const d = beat.dur ?? Math.max(beat.min ?? 0, beat.say ? talk + BREATH : 0);
      const out = { start: local, end: local + d, say: beat.say, lines };
      local += d;
      return out;
    });
    return { b, dur: local };
  }

  function layout(cutName) {
    for (const s of scenes) {
      Object.assign(s, layoutBeats(s.beats));
      s.at = (i, needle) => wordTime(s, i, needle);
    }
    const byId = new Map(scenes.map((s) => [s.id, s]));
    for (const [name, clips] of Object.entries(cuts))
      for (const c of clips) {
        if (!byId.has(c.scene)) console.error(`Cut ${name} uses a scene that isn't written: ${c.scene}`);
        Object.assign(c, layoutBeats([{ say: c.say, lead: c.lead, min: c.to - c.from }]));
      }
    if (cutName && !cuts[cutName]) console.error(`No cut called ${cutName}`);
    shots = cuts[cutName]
      ? cuts[cutName].map((c) => {
          const s = byId.get(c.scene);
          return { id: c.scene, root: s?.root, xfade: c.xfade, b: c.b, dur: c.dur, update: (t) => s?.update(clamp(c.from + t, c.from, c.to), s) };
        })
      : scenes.map((s) => ({ id: s.id, root: s.root, xfade: s.xfade, b: s.b, dur: s.dur, update: (t) => s.update(t, s) }));
    let t = 0;
    for (const shot of shots) {
      shot.start = t;
      shot.end = t += shot.dur;
      for (const b of shot.b) if (b.say) captions.push(...chunk(b, shot.start));
    }
    total = t;
  }

  // When the narrator reaches character `at` of a beat's line, in scene time:
  // through its sentence's clip, in proportion to the characters.
  function charTime(b, at) {
    const line = b.lines.findLast((l) => l.off <= at) ?? b.lines[0];
    return b.start + line.start + line.dur * clamp((at - line.off) / line.text.length);
  }

  // When the narrator reaches `needle` (a word or phrase) in beat i, in the
  // scene's time: for syncing the picture to the words.
  function wordTime(s, i, needle) {
    const b = s.b[i];
    const at = b.say.indexOf(needle);
    if (at < 0) throw new Error(`"${needle}" isn't in ${s.id}'s beat ${i}`);
    return charTime(b, at);
  }

  // The voiceover's clips: what to say (for voice.mjs: every scene's and every
  // cut's lines, so one voiceover serves them all) and when (for the mix).
  const voiceLines = () =>
    [...new Map([...scenes, ...Object.values(cuts).flat()].flatMap((s) => s.b.flatMap((b) => b.lines)).map((l) => [l.key, { key: l.key, text: spoken(l.text) }])).values()];
  const voiceTrack = () =>
    voice ? shots.flatMap((s) => s.b.flatMap((b) => b.lines.map((l) => ({ file: voice.clips[l.key]?.file, start: s.start + b.start + l.start })))).filter((c) => c.file) : [];

  // Split a beat's line into caption cards of at most two short lines, each
  // shown while the narrator says it. `start`: the scene's start.
  function chunk(b, start) {
    const MAX = 84;
    // Pack units into cards of at most MAX characters, in order.
    const pack = (units, sep = " ") => {
      const out = [];
      for (const u of units) {
        const last = out[out.length - 1];
        if (last !== undefined && (last + sep + u).length <= MAX) out[out.length - 1] = last + sep + u;
        else out.push(u);
      }
      return out;
    };
    // Words into n cards of about equal length.
    const balance = (s) => {
      const n = Math.ceil(s.length / MAX), target = s.length / n, out = [];
      let cur = "";
      for (const w of s.split(" ")) {
        if (cur && out.length < n - 1 && Math.abs(cur.length - target) < Math.abs(cur.length + 1 + w.length - target)) {
          out.push(cur);
          cur = w;
        } else cur = cur ? `${cur} ${w}` : w;
      }
      return [...out, cur];
    };
    // A sentence too long for one card breaks at commas, else evenly between words.
    const fit = (s) => {
      if (s.length <= MAX) return [s];
      const clauses = s.split(/(?<=,)\s+/);
      if (clauses.length > 1 && clauses.every((c) => c.length <= MAX)) return pack(clauses);
      return balance(s);
    };
    // Sentences end at . ? or ! followed by a space ("cronkite.cc" stays whole).
    const pieces = pack(sentences(b.say).flatMap(fit));
    let from = 0;
    const out = pieces.map((text) => {
      const at = b.say.indexOf(text, from);
      from = at + text.length;
      return { start: start + charTime(b, at), end: start + charTime(b, from), text };
    });
    // Each card stays up until the next; the last lingers a moment into the breath.
    out.forEach((c, i) => (c.end = out[i + 1]?.start ?? c.end + 0.25));
    return out;
  }

  // ---- shared overlays: captions, keys pressed, the pointer ----------------
  let hudKeys = [];
  let pointerState = null;

  // Keys pressed, shown as chips: [[time, "Ctrl+F"], ...] in the scene's time.
  function keys(t, list) {
    for (const [at, label] of list) if (t >= at && t < at + 1.4) hudKeys.push({ label, k: t - at });
  }
  // The mouse pointer along [[time, x, y, {click, drag, hand}], ...] in stage pixels.
  function pointer(t, path) {
    const pos = track(t, path.map(([at, x, y]) => [at, { x, y }]));
    let press = 0;
    let hand = false;
    for (const [at, , , opt] of path) {
      if (opt?.click && t >= at && t < at + 0.45) press = Math.max(press, 1 - (t - at) / 0.45);
      if (opt?.hand && t >= at) hand = opt.hand === true;
      if (opt?.hand === false && t >= at) hand = false;
    }
    let held = false;
    for (const [at, , , opt] of path) if (opt?.down !== undefined && t >= at) held = opt.down;
    pointerState = { ...pos, press, held, hand };
  }

  function drawOverlays(t) {
    const cap = captions.find((c) => t >= c.start && t < c.end);
    markup(captionEl, cap ? `<span>${cap.text.replace(/`([^`]+)`/g, "<code>$1</code>")}</span>` : "");
    css(captionEl, { opacity: cap ? 1 : 0 });

    markup(
      hudEl,
      hudKeys
        .map(({ label, k }) => {
          const o = clamp(k / 0.12) * clamp((1.4 - k) / 0.3);
          const s = 1 + 0.08 * clamp(1 - k / 0.15);
          return `<div class="chip" style="opacity:${o.toFixed(3)};transform:scale(${s.toFixed(3)})">${label
            .split("+")
            .map((key) => `<kbd>${key}</kbd>`)
            .join("<i>+</i>")}</div>`;
        })
        .join(""),
    );

    if (pointerState) {
      const { x, y, press, held, hand } = pointerState;
      css(pointerEl, { display: "", transform: `translate(${x.toFixed(1)}px, ${y.toFixed(1)}px)` });
      cls(pointerEl, "hand", hand);
      const ring = pointerEl.querySelector(".ring");
      css(ring, { opacity: (press * 0.9 + (held ? 0.45 : 0)).toFixed(3), transform: `scale(${(held ? 0.8 : 1.6 - press * 0.9).toFixed(3)})` });
    } else css(pointerEl, { display: "none" });
  }

  // ---- seeking -----------------------------------------------------------------
  function seek(t) {
    hudKeys = [];
    pointerState = null;
    // A shot plays until the next one has dissolved in over it. A cut can show
    // a scene's root in more than one shot, so roots are shown first, then drawn.
    const live = shots.filter((s, i) => {
      const next = shots[i + 1];
      const tail = next ? (next.xfade ?? 0.35) : 0;
      return t >= s.start - 1e-6 && (t < s.end + tail || (!next && t <= s.end + 1));
    });
    for (const s of scenes) show(s.root, live.some((shot) => shot.root === s.root));
    for (const [i, s] of live.entries()) {
      const next = shots[shots.indexOf(s) + 1];
      const tail = next ? (next.xfade ?? 0.35) : 0;
      const xf = s.xfade ?? 0.35;
      css(s.root, { zIndex: i, opacity: xf > 0 ? p(t, s.start, s.start + xf, ease.linear) : 1 });
      s.update(Math.min(t - s.start, s.dur + tail));
    }
    drawOverlays(t);
  }

  // cutName: play that cut instead of the whole edit.
  function init(cutName) {
    stage = document.getElementById("stage");
    if (edit) {
      const byId = new Map(scenes.map((s) => [s.id, s]));
      const missing = edit.filter((id) => !byId.has(id));
      const unplaced = scenes.filter((s) => !edit.includes(s.id)).map((s) => s.id);
      // The renderer refuses to render a page that logs errors.
      if (missing.length) console.error(`Scenes in the edit but not written: ${missing.join(", ")}`);
      if (unplaced.length) console.error(`Scenes written but not in the edit: ${unplaced.join(", ")}`);
      scenes.splice(0, scenes.length, ...edit.filter((id) => byId.has(id)).map((id) => byId.get(id)));
    }
    for (const s of scenes) {
      s.root = html(`<section class="scene" data-scene="${s.id}"></section>`);
      stage.appendChild(s.root);
    }
    layout(cutName);
    for (const s of scenes) s.update = s.build(s.root, s) || (() => {});
    captionEl = stage.appendChild(html(`<div id="caption"></div>`));
    hudEl = stage.appendChild(html(`<div id="hud"></div>`));
    pointerEl = stage.appendChild(
      html(`<div id="pointer"><div class="ring"></div>
        <svg class="arrow" width="34" height="40" viewBox="0 0 34 40"><path d="M3 2 L3 31 L10.5 24.5 L15.5 36 L21 33.6 L16 22.3 L26 22.3 Z" fill="#fff" stroke="#111" stroke-width="2.2" stroke-linejoin="round"/></svg>
        <svg class="handicon" width="34" height="40" viewBox="0 0 34 40"><path d="M12 4a2.6 2.6 0 0 1 5.2 0v12.5a2.5 2.5 0 0 1 5 0V18a2.5 2.5 0 0 1 5 0v2a2.5 2.5 0 0 1 5 0v8c0 6-4 10-10 10h-3c-4 0-6-2-8.5-5L5 24a2.6 2.6 0 0 1 4-3.3l3 3z" fill="#fff" stroke="#111" stroke-width="2" stroke-linejoin="round"/></svg></div>`),
    );
    return { duration: total, fps: FPS, width: W, height: H };
  }

  // WebVTT of the captions, for players that show them.
  function vtt() {
    const ts = (s) => {
      const ms = Math.round(s * 1000);
      const h = Math.floor(ms / 3600000), m = Math.floor(ms / 60000) % 60, sec = Math.floor(ms / 1000) % 60;
      return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}.${String(ms % 1000).padStart(3, "0")}`;
    };
    return "WEBVTT\n\n" + captions.map((c, i) => `${i + 1}\n${ts(c.start)} --> ${ts(c.end)}\n${c.text.replace(/`/g, "")}\n`).join("\n");
  }

  const list = () => shots.map((s) => ({ id: s.id, start: s.start, end: s.end }));

  return { W, H, FPS, clamp, lerp, ease, p, tw, track, typed, typedEnd, hash, blink, fmt, html, css, text, markup, cls, show, scene, order, cut, defer, settled, setVoice, voiceLines, voiceTrack, init, seek, keys, pointer, vtt, list, get duration() { return total; } };
})();
