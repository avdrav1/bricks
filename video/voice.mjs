// The voiceover: every narration sentence spoken, one WAV per sentence in
// .voice/, and .voice/manifest.json with each clip's length. The engine times
// the narration and captions from the manifest; soundtrack.js mixes the clips.
// Clips are cached by their text: only new or changed sentences are spoken.
//
// Two speakers, chosen by the voice's name:
// - Kokoro voices, "af_heart" (the default) and the like: Kokoro-82M, an
//   Apache-2.0 model run locally with ONNX in JavaScript (kokoro-js), so its
//   speech can be published. American and British voices only; af_heart is
//   the voice its authors grade highest. The model downloads once from
//   Hugging Face, then works offline.
// - Microsoft's neural voices, "en-IE-ConnorNeural" and the like, through the
//   read-aloud service built into Edge (msedge-tts). It needs no key but isn't
//   licensed for publishing, and it needs the network: for drafts, and for the
//   Hindi cut's narrator until it has a licensed one.
import { mkdir, readFile, writeFile, rm } from "node:fs/promises";
import { existsSync } from "node:fs";
import { spawn } from "node:child_process";
import { join } from "node:path";

export const DEFAULT_VOICE = "af_heart";
const isMicrosoft = (voice) => /^[a-z]{2}-[A-Z]{2}-\w+Neural$/.test(voice);
// About 165 words a minute, a relaxed pace; both speak at about 185 at 1.
export const defaultSpeed = (voice) => (isMicrosoft(voice) ? 0.9 : 0.85);

// lines: [{ key, text, voice, speed }] from V.voiceLines(); a line with no voice
// of its own (a cut in another language has one) is spoken by `voice`, at `speed`.
// Returns the manifest and whether it changed.
export async function makeVoice({ lines, dir, voice = DEFAULT_VOICE, speed = defaultSpeed(voice) }) {
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
    console.log(`Voiceover: speaking ${missing.length} of ${lines.length} sentences`);
    const speakers = new Map();
    const speakerFor = async (name, pace) => {
      const id = `${name}@${pace}`;
      if (!speakers.has(id)) speakers.set(id, isMicrosoft(name) ? await microsoft(name, pace) : await kokoro(name, pace));
      return speakers.get(id);
    };
    try {
      for (const [i, line] of missing.entries()) {
        const { samples, rate } = await (await (line.voice ? speakerFor(line.voice, line.speed ?? defaultSpeed(line.voice)) : speakerFor(voice, speed))).say(line.text);
        const clip = trim(samples, rate);
        const name = `${line.key}.wav`;
        await writeFile(join(dir, name), wav(clip, rate));
        manifest.clips[line.key] = { file: name, dur: +(clip.length / rate).toFixed(3), text: line.text, voice: line.voice ?? voice };
        // Save as we go: a dropped connection loses only the sentence in hand.
        await writeFile(file, JSON.stringify(manifest, null, 1));
        process.stdout.write(`\r  ${i + 1}/${missing.length}`);
      }
    } finally {
      for (const speaker of speakers.values()) speaker.close();
    }
    process.stdout.write("\n");
  }
  const changed = missing.length > 0 || stale.length > 0;
  if (changed) await writeFile(file, JSON.stringify(manifest, null, 1));
  return { manifest, changed };
}

async function kokoro(voice, speed) {
  const { KokoroTTS } = await import("kokoro-js");
  const tts = await KokoroTTS.from_pretrained("onnx-community/Kokoro-82M-v1.0-ONNX", { dtype: "fp32", device: "cpu" });
  return {
    async say(text) {
      const audio = await tts.generate(text, { voice, speed });
      return { samples: audio.audio, rate: audio.sampling_rate };
    },
    close() {},
  };
}

// The service sends MP3; ffmpeg decodes it to samples.
async function microsoft(voice, speed) {
  const { MsEdgeTTS, OUTPUT_FORMAT } = await import("msedge-tts");
  const tts = new MsEdgeTTS();
  await tts.setMetadata(voice, OUTPUT_FORMAT.AUDIO_24KHZ_96KBITRATE_MONO_MP3);
  const rate = `${Math.round((speed - 1) * 100)}%`;
  return {
    async say(text) {
      const { audioStream } = tts.toStream(text, { rate });
      const chunks = [];
      for await (const chunk of audioStream) chunks.push(chunk);
      return { samples: await decode(Buffer.concat(chunks), 24000), rate: 24000 };
    },
    close: () => tts.close(),
  };
}

function decode(mp3, rate) {
  return new Promise((ok, fail) => {
    const p = spawn("ffmpeg", ["-v", "error", "-f", "mp3", "-i", "-", "-f", "f32le", "-ac", "1", "-ar", String(rate), "-"]);
    const out = [];
    p.stdout.on("data", (d) => out.push(d));
    p.on("close", (code) => {
      if (code !== 0) return fail(new Error(`ffmpeg exited ${code} decoding speech`));
      const buf = Buffer.concat(out);
      ok(new Float32Array(buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.length)));
    });
    p.stdin.end(mp3);
  });
}

// Cut the silence the speaker leaves at either end, keeping 40 ms.
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
