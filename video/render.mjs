#!/usr/bin/env node
// Renders index.html to an MP4: headless Chromium seeks every frame, ffmpeg
// encodes them. Pages render slices of the timeline in parallel; the slices
// are joined without re-encoding.
//
//   node render.mjs                       the whole video -> ../dist/video/cronkite-explainer.mp4
//   node render.mjs --scene brick-sort    one scene (ids: the preview's menu)
//   node render.mjs --from 30 --to 60     a stretch, in seconds
//   node render.mjs --still 31.5 [--still 40 ...]   PNG frames, for checking a look
//   node render.mjs --draft ...           render even though the page logs errors
//   node render.mjs --voice af_heart      the narrator's voice (default en-IE-ConnorNeural, Irish; see voice.mjs)
//   node render.mjs --speed 0.9           the narrator's pace, relative to the voice's default
//   node render.mjs --voice-only          just make the voiceover in .voice/ (for the preview)
//   node render.mjs --no-voice            no voiceover: captions only, timed by word count
//   node render.mjs --audio vo.wav        a voiceover recorded elsewhere instead, against the captions; the music ducks under it
//   node render.mjs --no-music            leave out the soundtrack (soundtrack.js)
//   node render.mjs --serve               the preview player at http://127.0.0.1:8123
//
// Writes the captions as WebVTT beside the MP4 too.
import { createServer } from "node:http";
import { readFile, mkdir, writeFile, rm } from "node:fs/promises";
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { cpus } from "node:os";
import { join, extname, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { makeVoice, DEFAULT_VOICE, defaultSpeed } from "./voice.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i < 0 ? fallback : args[i + 1];
};
// --still 31.5 (seconds into the video) or --still brick-sort@1.5 (into a scene)
const stills = args.flatMap((a, i) => (a === "--still" ? [args[i + 1]] : []));
// The spoken voiceover (voice.mjs), unless there's none or one comes from --audio.
const speak = !args.includes("--no-voice") && !opt("audio");

const TYPES = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".woff2": "font/woff2", ".svg": "image/svg+xml", ".png": "image/png", ".jpg": "image/jpeg" };
function serve(port) {
  const server = createServer(async (req, res) => {
    // /sites/... is the repo's web pages, shown in the browser shots as they are.
    const url = decodeURIComponent(new URL(req.url, "http://x").pathname).replace(/\/$/, "/index.html");
    const root = url.startsWith("/sites/") ? resolve(here, "..") : here;
    const path = join(root, url);
    // No voiceover made yet: the page times the narration by word count.
    if (url === "/.voice/manifest.json" && !existsSync(join(here, url))) return res.writeHead(200, { "content-type": "application/json" }).end("null");
    if (!path.startsWith(root) || !existsSync(path)) return res.writeHead(404).end();
    res.writeHead(200, { "content-type": TYPES[extname(path)] || "application/octet-stream", "cache-control": "no-store" });
    res.end(await readFile(path));
  });
  return new Promise((ok) => server.listen(port, "127.0.0.1", () => ok(server)));
}

if (args.includes("--serve")) {
  await serve(Number(opt("port", 8123)));
  console.log(`Preview: http://127.0.0.1:${opt("port", 8123)}/  (?t=SECONDS starts there)`);
} else {
  await render();
}

function findChromium() {
  for (const p of [process.env.CHROMIUM, "/usr/bin/chromium", "/usr/bin/chromium-browser", "/usr/bin/google-chrome"]) if (p && existsSync(p)) return p;
  return undefined; // playwright-core's own download, if there is one
}

// A video renders only from a page without errors; stills and --draft
// renders, for checking work in progress, just print them.
async function openPage(browser, url) {
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 });
  const problems = [];
  page.on("pageerror", (e) => problems.push(e.message));
  page.on("console", (m) => m.type() === "error" && problems.push(`${m.text()} ${m.location().url ?? ""}`));
  await page.goto(url);
  const info = await page.evaluate(() => window.ready);
  if (problems.length) {
    if (!stills.length && !args.includes("--draft")) throw new Error(`The page has errors:\n${problems.join("\n")}`);
    console.warn(`The page has errors:\n${problems.join("\n")}`);
  }
  page.on("pageerror", (e) => console.error(e.message));
  return { page, info };
}

function launch() {
  return chromium.launch({ executablePath: findChromium(), args: ["--font-render-hinting=none", "--disable-lcd-text", "--force-color-profile=srgb"] });
}

async function render() {
  const server = await serve(0);
  const url = `http://127.0.0.1:${server.address().port}/?render${speak ? "" : "&novoice"}`;
  const browser = await launch();
  const browsers = [browser];
  try {
    let first = await openPage(browser, url);
    // The voiceover first: its clips' lengths set the timeline. Stills use
    // whatever voiceover is already there.
    if (speak && !stills.length) {
      const lines = await first.page.evaluate(() => V.voiceLines());
      const { changed } = await makeVoice({ lines, dir: join(here, ".voice"), voice: opt("voice", DEFAULT_VOICE), speed: Number(opt("speed", defaultSpeed(opt("voice", DEFAULT_VOICE)))) });
      if (args.includes("--voice-only")) return;
      if (changed) {
        await first.page.close();
        first = await openPage(browser, url);
      }
    }
    const { duration, fps } = first.info;
    const outFile = resolve(opt("out", join(here, "..", "dist", "video", stills.length ? "still.png" : "cronkite-explainer.mp4")));
    await mkdir(dirname(outFile), { recursive: true });

    if (stills.length) {
      const scenes = await first.page.evaluate(() => V.list());
      for (const still of stills) {
        const [id, local] = still.includes("@") ? still.split("@") : [null, still];
        const scene = id && scenes.find((s) => s.id === id);
        if (id && !scene) throw new Error(`No scene ${id}`);
        const t = (scene ? scene.start : 0) + Number(local);
        await first.page.evaluate((t) => V.seek(t), t);
        const file = stills.length === 1 ? outFile : outFile.replace(/\.png$/, `-${still.replace("@", "-")}.png`);
        await first.page.screenshot({ path: file });
        console.log(`${file}  (${t.toFixed(2)} s)`);
      }
      return;
    }

    let from = Number(opt("from", 0)), to = Number(opt("to", duration));
    const sceneId = opt("scene");
    if (sceneId) {
      const s = (await first.page.evaluate(() => V.list())).find((s) => s.id === sceneId);
      if (!s) throw new Error(`No scene ${sceneId}`);
      [from, to] = [s.start, s.end];
    }
    const f0 = Math.round(from * fps), f1 = Math.round(Math.min(to, duration) * fps);
    // One browser per worker: pages in one browser share its compositor and
    // barely run in parallel.
    const workers = Math.max(1, Math.min(Number(opt("workers", Math.max(1, Math.floor(cpus().length / 3)))), Math.ceil((f1 - f0) / 60)));
    const per = Math.ceil((f1 - f0) / workers);
    const tmp = join(dirname(outFile), `.parts-${process.pid}`);
    await mkdir(tmp, { recursive: true });
    console.log(`${f1 - f0} frames (${((f1 - f0) / fps).toFixed(1)} s) at ${fps} fps in ${workers} browsers`);

    // The soundtrack renders in its own page while the frames do, and comes
    // back as a WAV in base64 slices (one string of it would be ~80 MB).
    const music = !args.includes("--no-music");
    const soundtrack = !music && !speak
      ? Promise.resolve(null)
      : (async () => {
          const { page } = await openPage(browser, url);
          const size = await page.evaluate(async (opts) => (window.__wav = V.wav(await V.soundtrack(opts))).length, { music, voice: speak });
          const chunks = [];
          for (let at = 0; at < size; at += 8 << 20)
            chunks.push(Buffer.from(await page.evaluate(([a, b]) => {
              let s = "";
              const bytes = window.__wav.subarray(a, b);
              for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
              return btoa(s);
            }, [at, at + (8 << 20)]), "base64"));
          const file = join(tmp, "soundtrack.wav");
          await writeFile(file, Buffer.concat(chunks));
          await page.close();
          return file;
        })();

    const started = Date.now();
    let doneFrames = 0;
    const progress = setInterval(() => {
      const k = doneFrames / (f1 - f0);
      process.stdout.write(`\r${(k * 100).toFixed(1)}%  ${doneFrames} frames  ${((Date.now() - started) / 1000).toFixed(0)} s`);
    }, 1000);

    const parts = [];
    await Promise.all(
      Array.from({ length: workers }, async (_, w) => {
        const a = f0 + w * per, b = Math.min(f1, a + per);
        if (a >= b) return;
        const part = join(tmp, `part-${String(w).padStart(3, "0")}.mp4`);
        parts[w] = part;
        let page = first.page;
        if (w > 0) {
          const own = await launch();
          browsers.push(own);
          page = (await openPage(own, url)).page;
        }
        // Lossless PNG straight from the compositor; Playwright's own
        // screenshot is four times slower.
        const cdp = await page.context().newCDPSession(page);
        const enc = spawn("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-f", "image2pipe", "-framerate", String(fps), "-c:v", "png", "-i", "-",
          "-c:v", "libx264", "-preset", "medium", "-crf", "16", "-pix_fmt", "yuv420p", "-tune", "animation", "-r", String(fps), part], { stdio: ["pipe", "inherit", "inherit"] });
        const closed = new Promise((ok, fail) => enc.on("close", (c) => (c === 0 ? ok() : fail(new Error(`ffmpeg exited ${c}`)))));
        for (let f = a; f < b; f++) {
          await page.evaluate((t) => V.seek(t), f / fps);
          const { data } = await cdp.send("Page.captureScreenshot", { format: "png", optimizeForSpeed: true });
          if (!enc.stdin.write(Buffer.from(data, "base64"))) await new Promise((ok) => enc.stdin.once("drain", ok));
          doneFrames++;
        }
        enc.stdin.end();
        await closed;
      }),
    );
    clearInterval(progress);
    process.stdout.write("\n");

    const list = join(tmp, "parts.txt");
    await writeFile(list, parts.filter(Boolean).map((p) => `file '${p}'`).join("\n"));
    // The soundtrack (music and spoken voiceover), and a voiceover from --audio.
    const voice = opt("audio") && resolve(opt("audio"));
    const mixed = await soundtrack;
    const inputs = ["-f", "concat", "-safe", "0", "-i", list];
    for (const file of [mixed, voice].filter(Boolean)) inputs.push("-ss", String(f0 / fps), "-i", file);
    // The music sits under the voice: a compressor keyed by the voice ducks it.
    const mix =
      mixed && voice ? ["-filter_complex", "[2:a]apad,asplit=2[vo][key];[1:a][key]sidechaincompress=threshold=0.03:ratio=6:attack=15:release=350[bed];[bed][vo]amix=inputs=2:normalize=0:duration=first[a]", "-map", "0:v", "-map", "[a]"]
      : mixed || voice ? ["-map", "0:v", "-map", "1:a", "-af", "apad"]
      : [];
    await run([...inputs, ...mix, ...(mixed || voice ? ["-c:a", "aac", "-b:a", "192k", "-shortest"] : []), "-c:v", "copy", "-movflags", "+faststart", outFile]);
    await rm(tmp, { recursive: true });
    if (f0 === 0 && f1 === Math.round(duration * fps)) await writeFile(outFile.replace(/\.mp4$/, ".vtt"), await first.page.evaluate(() => V.vtt()));
    console.log(`${outFile}  (${((Date.now() - started) / 1000).toFixed(0)} s)`);
  } finally {
    await Promise.all(browsers.map((b) => b.close()));
    server.close();
  }
}

function run(argv) {
  return new Promise((ok, fail) => {
    const p = spawn("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", ...argv], { stdio: "inherit" });
    p.on("close", (code) => (code === 0 ? ok() : fail(new Error(`ffmpeg exited ${code}`))));
  });
}
