// The soundtrack: "Buildings" by avdrav (https://www.youtube.com/watch?v=cmKzUJzLp_s),
// fitted to the cut in an OfflineAudioContext, with the voiceover over it.
//
// - The whole edit plays the song from the title card. The surplus over the
//   video's length comes out of the song's main section (1:15-3:30) with a
//   crossfade, in whole two-bar units so the beat carries across, and the
//   song's own outro ends with the picture.
// - A cut (the shorts) starts the song so its drop hits as the cold open does.
//
// The song isn't in the repo: render.mjs says how to fetch it into .music/.
//
// V.soundtrack() -> Promise<AudioBuffer>; V.wav(buffer) -> 16-bit PCM WAV bytes.
"use strict";

(() => {
  const RATE = 48000;
  const SONG = {
    file: ".music/buildings.opus",
    url: "https://www.youtube.com/watch?v=cmKzUJzLp_s",
    end: 426.5, // where the outro has faded out
    drop: 259.41, // the drop's first hit (4:19.4)
    trimAt: 116, // the main section's surplus is cut from here (1:56)...
    trimMax: 94, // ...to at most 3:30, where the breakdown starts
    unit: (8 * 60) / 94, // two bars at 94 BPM
    gain: 0.45, // about -7 dB: the song is mastered at -11.5 LUFS
  };
  const XFADE = 2; // seconds, equal power
  const FADE = 2.5; // the picture's fade to black

  // Play `buffer` from `from` (seconds into the song) at `at` (into the video),
  // until `to`, with equal-power fades of `fadeIn` and `fadeOut` seconds.
  function play(ctx, buffer, dest, { at, from, to, fadeIn = 0, fadeOut = 0 }) {
    const src = ctx.createBufferSource();
    const g = ctx.createGain();
    src.buffer = buffer;
    src.connect(g).connect(dest);
    const steps = 64;
    const curve = (up) => Float32Array.from({ length: steps }, (_, i) => (up ? Math.sin : Math.cos)(((i / (steps - 1)) * Math.PI) / 2));
    if (fadeIn) g.gain.setValueCurveAtTime(curve(true), at, fadeIn);
    if (fadeOut) g.gain.setValueCurveAtTime(curve(false), at + (to - from) - fadeOut, fadeOut);
    src.start(at, from, to - from);
  }

  // The beat drifts a little from 94 BPM, so a trim of whole two-bar units is
  // nudged, within a third of a beat, to where the song's onsets before the cut
  // line up with those after it: the beat then carries across the crossfade.
  function inPhase(song, at, trim) {
    const rate = 200, hop = Math.round(song.sampleRate / rate), x = song.getChannelData(0);
    const span = 10 * rate, reach = Math.round(0.21 * rate);
    // Onset strength (rises in log energy) of `n` frames from `t` seconds.
    const onsets = (t, n) => {
      const start = Math.round(t * song.sampleRate), out = new Float64Array(n);
      let prev = 0;
      for (let k = 0; k < n; k++) {
        let e = 0;
        for (let i = start + k * hop, end = i + hop; i < end; i++) e += x[i] * x[i];
        const level = Math.log(e + 1e-9);
        out[k] = k ? Math.max(0, level - prev) : 0;
        prev = level;
      }
      return out;
    };
    const before = onsets(at - span / rate, span);
    const mean = before.reduce((a, b) => a + b, 0) / span;
    const after = onsets(at + trim - span / rate - reach / rate, span + 2 * reach);
    let best = -Infinity, lag = 0;
    for (let l = -reach; l <= reach; l++) {
      let s = 0;
      for (let i = 0; i < span; i++) s += (before[i] - mean) * after[i + reach + l];
      if (s > best) [best, lag] = [s, l];
    }
    return trim + lag / rate;
  }

  // Where each stretch of the song goes in this cut.
  function arrange(song, duration) {
    if (V.cutName) {
      const coldOpen = V.list().find((s) => s.id === "cold-open")?.start ?? 0;
      const from = SONG.drop - coldOpen;
      return [{ at: 0, from, to: Math.min(SONG.end, from + duration) }];
    }
    const surplus = SONG.end - duration;
    if (surplus <= 0) return [{ at: 0, from: 0, to: SONG.end }];
    const cut = SONG.trimAt;
    const trim = inPhase(song, cut, Math.min(SONG.trimMax, Math.ceil(surplus / SONG.unit) * SONG.unit));
    return [
      { at: 0, from: 0, to: cut + XFADE / 2, fadeOut: XFADE },
      // After the crossfade, video time t plays the song at t + trim.
      { at: cut - XFADE / 2, from: cut + trim - XFADE / 2, to: Math.min(SONG.end, duration + trim), fadeIn: XFADE },
    ];
  }

  // music: the song; voice: the voiceover clips from .voice/ (V.voiceTrack()).
  async function soundtrack({ music = true, voice = true } = {}) {
    const duration = V.duration;
    const ctx = new OfflineAudioContext(2, Math.ceil(duration * RATE), RATE);

    // ---- the mix --------------------------------------------------------
    // music -> duck (down while the narrator talks) -> limiter; voice -> limiter.
    // The limiter (Web Audio's compressor) lets the narrator's loudest syllables
    // over the song through near 0 dBFS, so the output sits 2 dB under it.
    const master = ctx.createGain();
    const duck = ctx.createGain();
    const limiter = ctx.createDynamicsCompressor();
    limiter.threshold.value = -2; limiter.knee.value = 0; limiter.ratio.value = 20; limiter.attack.value = 0.002; limiter.release.value = 0.12;
    const out = ctx.createGain();
    out.gain.value = 0.79;
    master.connect(duck).connect(limiter).connect(out).connect(ctx.destination);
    master.gain.value = SONG.gain;

    if (music) {
      const response = await fetch(SONG.file);
      if (!response.ok) throw new Error(`No song at video/${SONG.file}: yt-dlp -x --audio-format opus -o video/${SONG.file} '${SONG.url}'`);
      const song = await ctx.decodeAudioData(await response.arrayBuffer());
      for (const part of arrange(song, duration)) play(ctx, song, master, part);
      // Fade with the picture's fade to black.
      master.gain.setValueAtTime(SONG.gain, Math.max(0, duration - FADE));
      master.gain.linearRampToValueAtTime(0, duration);
    }

    // ---- the voiceover ---------------------------------------------------------
    // Each sentence's clip at its time; the music dips about 10 dB under each
    // stretch of talk, staying down through pauses under 3 s so it doesn't pump
    // between lines, and coming back up slowly in the longer ones.
    const clips = voice ? V.voiceTrack() : [];
    if (clips.length) {
      const VOICE = 1.15, DIP = 0.32;
      const vo = ctx.createGain();
      vo.gain.value = VOICE;
      vo.connect(limiter);
      const files = [...new Set(clips.map((c) => c.file))];
      const buffers = new Map(
        await Promise.all(files.map(async (f) => [f, await ctx.decodeAudioData(await (await fetch(`.voice/${f}`)).arrayBuffer())])),
      );
      // Every sentence at the same loudness, whoever speaks it: RMS to Connor's
      // average (-19.6 dBFS), within ±6 dB so a short word isn't blown up.
      const TARGET = 0.105;
      const level = (buffer) => {
        const x = buffer.getChannelData(0);
        let sum = 0;
        for (let i = 0; i < x.length; i++) sum += x[i] * x[i];
        return V.clamp(TARGET / Math.sqrt(sum / x.length || 1e-9), 0.5, 2);
      };
      const levels = new Map([...buffers].map(([f, b]) => [f, level(b)]));
      const talk = [];
      for (const c of clips) {
        const buffer = buffers.get(c.file);
        const src = ctx.createBufferSource();
        const gain = ctx.createGain();
        src.buffer = buffer;
        gain.gain.value = levels.get(c.file);
        src.connect(gain).connect(vo);
        src.start(c.start);
        const end = c.start + buffer.duration;
        const last = talk[talk.length - 1];
        if (last && c.start - last.end < 3) last.end = Math.max(last.end, end);
        else talk.push({ start: c.start, end });
      }
      for (const { start, end } of talk) {
        duck.gain.setTargetAtTime(DIP, Math.max(0, start - 0.3), 0.08);
        duck.gain.setTargetAtTime(1, end + 0.3, 0.6);
      }
    }

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
  V.song = { file: SONG.file, url: SONG.url };
})();
