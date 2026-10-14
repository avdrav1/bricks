// The voiceover: every narration sentence spoken by Kokoro (an 82M-parameter
// TTS model run with ONNX in JavaScript, kokoro-js), one WAV per sentence in
// .voice/, and .voice/manifest.json with each clip's length. The engine times
// the narration and captions from the manifest; soundtrack.js mixes the clips.
// Clips are cached by their text: only new or changed sentences are spoken.
import { mkdir, readFile, writeFile, rm } from "node:fs/promises";
import { existsSync } from "node:fs";
import { join } from "node:path";

export const VOICES = ["af_heart", "af_bella", "am_michael", "bm_george", "bf_emma"];
const MODEL = "onnx-community/Kokoro-82M-v1.0-ONNX";

// lines: [{ key, text }] from V.voiceLines(). Returns the manifest and whether it changed.
// speed 0.85: about 165 words a minute, a relaxed pace (1 is ~185).
export async function makeVoice({ lines, dir, voice = "af_heart", speed = 0.85 }) {
  await mkdir(dir, { recursive: true });
  const file = join(dir, "manifest.json");
  let manifest = existsSync(file) ? JSON.parse(await readFile(file, "utf8")) : null;
  if (!manifest || manifest.voice !== voice || manifest.speed !== speed) {
    if (manifest) await rm(dir, { recursive: true }).then(() => mkdir(dir, { recursive: true }));
    manifest = { voice, speed, clips: {} };
  }
  const wanted = new Set(lines.map((l) => l.key));
  const missing = lines.filter((l) => !manifest.clips[l.key] || !existsSync(join(dir, manifest.clips[l.key].file)));
  const stale = Object.keys(manifest.clips).filter((k) => !wanted.has(k));
  for (const k of stale) {
    await rm(join(dir, manifest.clips[k].file), { force: true });
    delete manifest.clips[k];
  }
  if (missing.length) {
    console.log(`Voiceover: speaking ${missing.length} of ${lines.length} sentences (${voice})`);
    const { KokoroTTS } = await import("kokoro-js");
    const tts = await KokoroTTS.from_pretrained(MODEL, { dtype: "fp32", device: "cpu" });
    for (const [i, line] of missing.entries()) {
      const audio = await tts.generate(line.text, { voice, speed });
      const samples = trim(audio.audio, audio.sampling_rate);
      const name = `${line.key}.wav`;
      await writeFile(join(dir, name), wav(samples, audio.sampling_rate));
      manifest.clips[line.key] = { file: name, dur: +(samples.length / audio.sampling_rate).toFixed(3), text: line.text };
      process.stdout.write(`\r  ${i + 1}/${missing.length}`);
    }
    process.stdout.write("\n");
  }
  const changed = missing.length > 0 || stale.length > 0;
  if (changed) await writeFile(file, JSON.stringify(manifest, null, 1));
  return { manifest, changed };
}

// Cut the silence the model leaves at either end, keeping 40 ms.
function trim(x, rate) {
  const loud = (v) => Math.abs(v) > 0.01;
  let a = 0, b = x.length;
  while (a < b && !loud(x[a])) a++;
  while (b > a && !loud(x[b - 1])) b--;
  const pad = Math.round(0.04 * rate);
  return x.subarray(Math.max(0, a - pad), Math.min(x.length, b + pad));
}

function wav(x, rate) {
  const out = Buffer.alloc(44 + x.length * 2);
  out.write("RIFF", 0); out.writeUInt32LE(36 + x.length * 2, 4); out.write("WAVE", 8);
  out.write("fmt ", 12); out.writeUInt32LE(16, 16); out.writeUInt16LE(1, 20); out.writeUInt16LE(1, 22);
  out.writeUInt32LE(rate, 24); out.writeUInt32LE(rate * 2, 28); out.writeUInt16LE(2, 32); out.writeUInt16LE(16, 34);
  out.write("data", 36); out.writeUInt32LE(x.length * 2, 40);
  for (let i = 0; i < x.length; i++) out.writeInt16LE(Math.round(Math.max(-1, Math.min(1, x[i])) * 32767), 44 + i * 2);
  return out;
}
