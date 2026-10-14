// The soundtrack: techno at 125 BPM, synthesised with Web Audio in an
// OfflineAudioContext, arranged to the edit. It starts on the title card that
// opens the video (the cold open after it keeps the title's pads), changes
// part on the bar nearest each section's first scene, and ends with the picture.
//
// V.soundtrack() -> Promise<AudioBuffer>; V.wav(buffer) -> 16-bit PCM WAV bytes.
"use strict";

(() => {
  const BPM = 125;
  const BEAT = 60 / BPM, BAR = 4 * BEAT, STEP = BEAT / 4;
  const RATE = 48000;

  // What plays in each part of the edit. Scenes map to parts by id.
  const PARTS = {
    title:   { sub: 1, pad: 1 },
    intro:   { pad: 1, hats: 1, bass: 0.35, kick: 0 },
    groove:  { kick: 1, clap: 1, hats: 1, ohat: 1, bass: 1, pad: 0.8 },
    calm:    { kick: 1, hats: 1, bass: 0.5, pad: 1, arp: 0.5 },
    drive:   { kick: 1, clap: 1, hats: 1, ohat: 1, bass: 1, pad: 0.7, arp: 1 },
    break:   { pad: 1, arp: 0.8, hats: 0.5 },
    finale:  { kick: 1, clap: 1, hats: 1, ohat: 1, bass: 1, pad: 1, arp: 1 },
  };
  const partOf = (id) =>
    id === "title" ? "title"
    : id === "suite-icons" ? "intro"
    : id === "app-grid" ? "intro"
    : id.startsWith("brick-") ? "groove"
    : id.startsWith("slate-") ? "calm"
    : id.startsWith("frame-") ? "drive"
    : id.startsWith("install-") ? "break"
    : id === "close" ? "finale"
    : null;

  // A minor: Am7, Fmaj7, Dm7, Em7, two bars each. Bass roots and pad voicings in Hz.
  const hz = (midi) => 440 * Math.pow(2, (midi - 69) / 12);
  const CHORDS = [
    { root: 33, pad: [57, 60, 64, 67] }, // Am7
    { root: 29, pad: [57, 60, 64, 65] }, // Fmaj7
    { root: 26, pad: [57, 60, 62, 65] }, // Dm7
    { root: 28, pad: [55, 59, 62, 64] }, // Em7
  ];
  const chordAt = (bar) => CHORDS[Math.floor(bar / 2) % CHORDS.length];

  // Seeded noise, so every render is the same.
  function noiseBuffer(ctx, seconds, seed) {
    const buf = ctx.createBuffer(1, Math.ceil(seconds * ctx.sampleRate), ctx.sampleRate);
    const d = buf.getChannelData(0);
    let s = seed >>> 0 || 1;
    for (let i = 0; i < d.length; i++) {
      s ^= s << 13; s ^= s >>> 17; s ^= s << 5;
      d[i] = ((s >>> 0) / 4294967296) * 2 - 1;
    }
    return buf;
  }
  // A plate-ish reverb tail from decaying noise.
  function impulse(ctx, seconds) {
    const buf = ctx.createBuffer(2, Math.ceil(seconds * ctx.sampleRate), ctx.sampleRate);
    for (let c = 0; c < 2; c++) {
      const d = buf.getChannelData(c);
      let s = 0x9e3779b9 + c * 7919;
      for (let i = 0; i < d.length; i++) {
        s ^= s << 13; s ^= s >>> 17; s ^= s << 5;
        d[i] = (((s >>> 0) / 4294967296) * 2 - 1) * Math.pow(1 - i / d.length, 3.2);
      }
    }
    return buf;
  }

  async function soundtrack() {
    const scenes = V.list();
    const duration = V.duration;
    const ctx = new OfflineAudioContext(2, Math.ceil(duration * RATE), RATE);

    // ---- the mix --------------------------------------------------------
    const master = ctx.createGain();
    const comp = ctx.createDynamicsCompressor();
    comp.threshold.value = -14; comp.knee.value = 8; comp.ratio.value = 4; comp.attack.value = 0.005; comp.release.value = 0.18;
    master.connect(comp).connect(ctx.destination);
    master.gain.value = 0.62;

    const verb = ctx.createConvolver();
    verb.buffer = impulse(ctx, 2.6);
    const verbIn = ctx.createGain();
    verbIn.gain.value = 0.28;
    verbIn.connect(verb).connect(master);

    // A dotted-eighth echo for the arp and stabs.
    const delay = ctx.createDelay(2);
    delay.delayTime.value = STEP * 3;
    const fb = ctx.createGain();
    fb.gain.value = 0.38;
    const delayTone = ctx.createBiquadFilter();
    delayTone.type = "lowpass"; delayTone.frequency.value = 3200;
    delay.connect(delayTone).connect(fb).connect(delay);
    delayTone.connect(master);
    const delayIn = ctx.createGain();
    delayIn.gain.value = 0.3;
    delayIn.connect(delay);

    // Buses that duck under the kick (the "pump").
    const pumped = ctx.createGain();
    pumped.connect(master);
    pumped.connect(verbIn);
    const drums = ctx.createGain();
    drums.connect(master);

    const noise = noiseBuffer(ctx, 3, 12345);
    const playNoise = (t, dur, dest, offset = 0) => {
      const src = ctx.createBufferSource();
      src.buffer = noise;
      src.connect(dest);
      src.start(t, offset % 2, dur);
      return src;
    };

    // ---- instruments ---------------------------------------------------------
    function kick(t, v = 1) {
      const o = ctx.createOscillator(), g = ctx.createGain();
      o.frequency.setValueAtTime(155, t);
      o.frequency.exponentialRampToValueAtTime(48, t + 0.11);
      o.frequency.exponentialRampToValueAtTime(42, t + 0.4);
      g.gain.setValueAtTime(0.0001, t);
      g.gain.exponentialRampToValueAtTime(1.0 * v, t + 0.004);
      g.gain.exponentialRampToValueAtTime(0.0001, t + 0.42);
      o.connect(g).connect(drums);
      o.start(t); o.stop(t + 0.45);
      // the beater click
      const hp = ctx.createBiquadFilter(), cg = ctx.createGain();
      hp.type = "highpass"; hp.frequency.value = 2500;
      cg.gain.setValueAtTime(0.25 * v, t);
      cg.gain.exponentialRampToValueAtTime(0.0001, t + 0.012);
      playNoise(t, 0.02, hp, t * 7.3);
      hp.connect(cg).connect(drums);
      // duck the pumped bus
      pumped.gain.setValueAtTime(0.25, t);
      pumped.gain.linearRampToValueAtTime(1, t + BEAT * 0.82);
    }
    function hat(t, v, open = false) {
      const hp = ctx.createBiquadFilter(), g = ctx.createGain();
      hp.type = "highpass"; hp.frequency.value = open ? 7000 : 8500;
      const d = open ? 0.22 : 0.035;
      g.gain.setValueAtTime(v, t);
      g.gain.exponentialRampToValueAtTime(0.0001, t + d);
      playNoise(t, d + 0.02, hp, t * 3.1);
      hp.connect(g).connect(drums);
    }
    function clap(t, v = 1) {
      const bp = ctx.createBiquadFilter(), g = ctx.createGain();
      bp.type = "bandpass"; bp.frequency.value = 1400; bp.Q.value = 0.9;
      g.gain.setValueAtTime(0.0001, t);
      for (const [dt, a] of [[0, 0.5], [0.011, 0.45], [0.022, 0.6]]) {
        g.gain.setValueAtTime(a * v, t + dt);
        g.gain.exponentialRampToValueAtTime(0.05 * v, t + dt + 0.009);
      }
      g.gain.setValueAtTime(0.55 * v, t + 0.032);
      g.gain.exponentialRampToValueAtTime(0.0001, t + 0.2);
      playNoise(t, 0.24, bp, t * 5.7);
      bp.connect(g);
      g.connect(drums);
      const send = ctx.createGain();
      send.gain.value = 0.9;
      g.connect(send).connect(verbIn);
    }
    function bass(t, f, cutoff, v) {
      const o = ctx.createOscillator(), o2 = ctx.createOscillator(), lp = ctx.createBiquadFilter(), g = ctx.createGain();
      o.type = "sawtooth"; o.frequency.value = f;
      o2.type = "square"; o2.frequency.value = f / 2;
      lp.type = "lowpass"; lp.Q.value = 7;
      lp.frequency.setValueAtTime(cutoff * 4, t);
      lp.frequency.exponentialRampToValueAtTime(cutoff, t + 0.09);
      g.gain.setValueAtTime(0.0001, t);
      g.gain.exponentialRampToValueAtTime(0.32 * v, t + 0.005);
      g.gain.exponentialRampToValueAtTime(0.0001, t + STEP * 0.95);
      const mix = ctx.createGain(); mix.gain.value = 0.5;
      o2.connect(mix);
      o.connect(lp); mix.connect(lp);
      lp.connect(g).connect(pumped);
      o.start(t); o2.start(t); o.stop(t + STEP); o2.stop(t + STEP);
    }
    function sub(t, f, dur, v) {
      const o = ctx.createOscillator(), g = ctx.createGain();
      o.frequency.value = f;
      g.gain.setValueAtTime(0.0001, t);
      g.gain.exponentialRampToValueAtTime(0.5 * v, t + 0.02);
      g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
      o.connect(g).connect(master);
      o.start(t); o.stop(t + dur + 0.05);
    }
    // A slow-swelling chord of detuned saws.
    function pad(t, notes, dur, v, cutoff = 1400) {
      const lp = ctx.createBiquadFilter(), g = ctx.createGain();
      lp.type = "lowpass"; lp.Q.value = 1.5;
      lp.frequency.setValueAtTime(cutoff * 0.5, t);
      lp.frequency.linearRampToValueAtTime(cutoff, t + dur * 0.5);
      lp.frequency.linearRampToValueAtTime(cutoff * 0.6, t + dur);
      g.gain.setValueAtTime(0.0001, t);
      g.gain.linearRampToValueAtTime(0.05 * v, t + Math.min(0.8, dur * 0.3));
      g.gain.setValueAtTime(0.05 * v, t + dur - 0.3);
      g.gain.linearRampToValueAtTime(0.0001, t + dur + 0.25);
      for (const n of notes)
        for (const det of [-9, 0, 8]) {
          const o = ctx.createOscillator();
          o.type = "sawtooth"; o.frequency.value = hz(n); o.detune.value = det;
          o.connect(lp);
          o.start(t); o.stop(t + dur + 0.3);
        }
      lp.connect(g).connect(pumped);
    }
    function pluck(t, f, v) {
      const o = ctx.createOscillator(), lp = ctx.createBiquadFilter(), g = ctx.createGain();
      o.type = "square"; o.frequency.value = f;
      lp.type = "lowpass"; lp.Q.value = 4;
      lp.frequency.setValueAtTime(5000, t);
      lp.frequency.exponentialRampToValueAtTime(500, t + 0.15);
      g.gain.setValueAtTime(0.0001, t);
      g.gain.exponentialRampToValueAtTime(0.07 * v, t + 0.004);
      g.gain.exponentialRampToValueAtTime(0.0001, t + 0.22);
      o.connect(lp).connect(g);
      g.connect(pumped);
      g.connect(delayIn);
      o.start(t); o.stop(t + 0.25);
    }
    // A noise sweep into the next part, and a crash on its downbeat.
    function riser(t, dur) {
      const bp = ctx.createBiquadFilter(), g = ctx.createGain();
      bp.type = "bandpass"; bp.Q.value = 2.5;
      bp.frequency.setValueAtTime(300, t);
      bp.frequency.exponentialRampToValueAtTime(7000, t + dur);
      g.gain.setValueAtTime(0.0001, t);
      g.gain.exponentialRampToValueAtTime(0.16, t + dur);
      g.gain.linearRampToValueAtTime(0, t + dur + 0.02);
      const src = ctx.createBufferSource();
      src.buffer = noise; src.loop = true;
      src.connect(bp).connect(g).connect(master);
      src.start(t); src.stop(t + dur + 0.05);
    }
    function crash(t, v = 1) {
      const hp = ctx.createBiquadFilter(), g = ctx.createGain();
      hp.type = "highpass"; hp.frequency.value = 4500;
      g.gain.setValueAtTime(0.22 * v, t);
      g.gain.exponentialRampToValueAtTime(0.0001, t + 2.2);
      const src = ctx.createBufferSource();
      src.buffer = noise; src.loop = true;
      src.connect(hp).connect(g);
      g.connect(master); g.connect(verbIn);
      src.start(t); src.stop(t + 2.3);
    }

    // ---- the arrangement -----------------------------------------------------
    // The bar grid starts on the title card; each part starts on the bar
    // nearest its first scene.
    const title = scenes.find((s) => s.id === "title");
    const A = title.start;
    const bars = Math.ceil((duration - A) / BAR);
    const changes = [];
    for (const s of scenes) {
      const part = partOf(s.id);
      if (!part || (changes.length && changes[changes.length - 1].part === part)) continue;
      changes.push({ part, bar: Math.max(0, Math.round((s.start - A) / BAR)) });
    }
    const partAt = (bar) => changes.filter((c) => c.bar <= bar).pop()?.part ?? "title";
    const close = scenes.find((s) => s.id === "close");
    // The drums stop four bars into the close; the last chord rings to the end.
    const finalBar = Math.round((close.start - A) / BAR) + 4;

    // Every node in the graph costs time on every render quantum, started or
    // not, so the bars are scheduled a block at a time: the context suspends
    // just before each block, its notes are created, and it resumes.
    function scheduleBar(i) {
      const t0 = A + i * BAR;
      const name = partAt(i);
      const p = PARTS[name];
      const chord = chordAt(i);
      const ending = i >= finalBar;
      const next = partAt(i + 1);
      if (next !== name && i + 1 < bars) riser(t0, BAR);
      if (i > 0 && name !== partAt(i - 1)) crash(t0, 0.9);

      if (ending) {
        if (i === finalBar) {
          crash(t0, 1.1);
          kick(t0, 1);
          sub(t0, hz(33), 4, 0.9);
          pad(t0, CHORDS[0].pad, duration - t0, 1, 1800);
        }
        return;
      }
      // Pads: one chord every two bars.
      if (p.pad && i % 2 === 0) pad(t0, chord.pad, BAR * 2, p.pad, name === "calm" ? 1100 : 1600);
      if (p.sub && i % 2 === 0 && i > 0) sub(t0, hz(chord.root), BAR * 1.5, 0.6);

      for (let s = 0; s < 16; s++) {
        const t = t0 + s * STEP;
        const beat = s % 4 === 0;
        // Leave the last beat of a bar before a change empty for the drop.
        const gap = next !== name && s >= 12;
        if (p.kick && beat && !gap) kick(t, p.kick);
        if (p.clap && (s === 4 || s === 12) && !gap) clap(t, p.clap);
        if (p.hats) hat(t, (beat ? 0.05 : s % 2 ? 0.07 : 0.035) * p.hats);
        if (p.ohat && s % 4 === 2 && !gap) hat(t, 0.06 * p.ohat, true);
        // The rolling bass: three 16ths after each kick, an octave jump on the last.
        if (p.bass && !beat && !gap) {
          const up = s % 4 === 3 && (i + s) % 3 === 0;
          const cutoff = (name === "calm" ? 300 : 520) + 260 * Math.sin((i * 16 + s) * 0.05);
          bass(t, hz(chord.root + 12 + (up ? 12 : 0)), cutoff, p.bass);
        }
        // The arp walks the chord in 16ths.
        if (p.arp && s % 2 === 0) {
          const notes = chord.pad;
          const n = notes[(s / 2 + i) % notes.length] + (s % 8 === 6 ? 12 : 0);
          pluck(t, hz(n + 12), p.arp);
        }
      }
    }
    crash(A, 1.2);
    sub(A, hz(33), 3.5, 1);
    const BLOCK = 4;
    for (let b = 0; b * BLOCK < bars; b++) {
      const block = () => {
        for (let i = b * BLOCK; i < Math.min(bars, (b + 1) * BLOCK); i++) scheduleBar(i);
      };
      if (b === 0) block();
      else {
        // A render quantum (128 frames) before the block's first bar, at least.
        const at = Math.floor(((A + b * BLOCK * BAR - 0.05) * RATE) / 128) * 128 / RATE;
        ctx.suspend(at).then(() => {
          block();
          ctx.resume();
        });
      }
    }
    // Fade with the picture's fade to black.
    master.gain.setValueAtTime(0.62, Math.max(0, duration - 2.5));
    master.gain.linearRampToValueAtTime(0, duration);

    return ctx.startRendering();
  }

  // 16-bit PCM WAV.
  function wav(buffer) {
    const ch = buffer.numberOfChannels, n = buffer.length;
    const out = new DataView(new ArrayBuffer(44 + n * ch * 2));
    const str = (o, s) => [...s].forEach((c, i) => out.setUint8(o + i, c.charCodeAt(0)));
    str(0, "RIFF"); out.setUint32(4, 36 + n * ch * 2, true); str(8, "WAVE");
    str(12, "fmt "); out.setUint32(16, 16, true); out.setUint16(20, 1, true); out.setUint16(22, ch, true);
    out.setUint32(24, buffer.sampleRate, true); out.setUint32(28, buffer.sampleRate * ch * 2, true);
    out.setUint16(32, ch * 2, true); out.setUint16(34, 16, true);
    str(36, "data"); out.setUint32(40, n * ch * 2, true);
    const data = [...Array(ch)].map((_, c) => buffer.getChannelData(c));
    let o = 44;
    for (let i = 0; i < n; i++)
      for (let c = 0; c < ch; c++, o += 2) out.setInt16(o, Math.max(-1, Math.min(1, data[c][i])) * 32767, true);
    return new Uint8Array(out.buffer);
  }

  V.soundtrack = soundtrack;
  V.wav = wav;
})();
