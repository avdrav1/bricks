// Shared by both SLATE-5 spikes via `#[path = "../../common.rs"]`. Throwaway code.
//
// The document: `--words` words (default 150,000, about 300 pages) of deterministic prose in
// paragraphs of 40-180 words, a heading (level 1-3) every 8-16 paragraphs, a bullet list of
// 3-6 items every ~20 paragraphs, and about one word in ten bold or italic. Both spikes show
// exactly this content with the same looks: body 11pt sans, headings bold at 2.0/1.5/1.25x,
// bullets indented 28 px per level with a "•" marker, 12 px between paragraphs, the column
// centred at the chosen text width (ADR 0005: Narrow 480, Medium 624, Wide 816, Full =
// window less 24 px a side).
//
// Protocol on stdout (read by measure.py); all times CLOCK_MONOTONIC ns (`mono_ns`):
//   READY <win_w> <win_h> <column_w> <paragraphs> <chars>   first frame painted
//   K <ns> <keyval>                    key event delivered to the app (before handling)
//   E <ns> <revision>                  an edit landed in the document (revision += 1)
//   W <ns> <column_w>                  the text width changed (before any relayout)
//   F <ns> <revision> <column_w> <cpu_us> <top_y>
//        frame finished (frame clock after-paint); cpu_us = before-paint -> after-paint;
//        top_y = document y at the top of the viewport, in px
//   L <ns> <column_w> <content_h>      layout of the whole document is complete at this width
//                                      (every paragraph measured; the scroll range is final)
//   DONE
//
// Modes (--mode):
//   interactive  nothing automatic (measure.py injects keys: typing, width shortcuts)
//   scroll       scroll 37 px per frame from the cursor for --frames frames, then DONE
//   rewrap       every 90 frames switch the width Narrow <-> Full, --frames frames, then DONE
// Keys every spike handles in all modes: printable characters insert at the cursor
// (through GtkIMContext commit), Ctrl+1/2/3/4 set Narrow/Medium/Wide/Full.

use std::io::Write as _;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Interactive,
    Scroll,
    Rewrap,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextWidth {
    Narrow,
    Medium,
    Wide,
    Full,
}

impl TextWidth {
    /// The column width in px for a view `view_w` px wide (ADR 0005).
    pub fn column(self, view_w: i32) -> i32 {
        let fit = (view_w - 48).max(120);
        match self {
            TextWidth::Narrow => 480.min(fit),
            TextWidth::Medium => 624.min(fit),
            TextWidth::Wide => 816.min(fit),
            TextWidth::Full => fit,
        }
    }

    pub fn from_key(n: u32) -> Option<Self> {
        Some(match n {
            1 => TextWidth::Narrow,
            2 => TextWidth::Medium,
            3 => TextWidth::Wide,
            4 => TextWidth::Full,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Args {
    pub words: usize,
    pub width: TextWidth,
    pub mode: Mode,
    pub frames: u32,
    /// Where the cursor starts, as a fraction of the paragraphs (0.5 = the middle).
    pub at: f64,
}

impl Args {
    pub fn parse() -> Self {
        let mut a = Args {
            words: 150_000,
            width: TextWidth::Full,
            mode: Mode::Interactive,
            frames: 900,
            at: 0.5,
        };
        let mut it = std::env::args().skip(1);
        while let Some(k) = it.next() {
            let v = it.next().unwrap_or_else(|| panic!("missing value for {k}"));
            match k.as_str() {
                "--words" => a.words = v.parse().expect("--words"),
                "--frames" => a.frames = v.parse().expect("--frames"),
                "--at" => a.at = v.parse().expect("--at"),
                "--width" => {
                    a.width = match v.as_str() {
                        "narrow" => TextWidth::Narrow,
                        "medium" => TextWidth::Medium,
                        "wide" => TextWidth::Wide,
                        "full" => TextWidth::Full,
                        _ => panic!("--width narrow|medium|wide|full"),
                    }
                }
                "--mode" => {
                    a.mode = match v.as_str() {
                        "interactive" => Mode::Interactive,
                        "scroll" => Mode::Scroll,
                        "rewrap" => Mode::Rewrap,
                        _ => panic!("--mode interactive|scroll|rewrap"),
                    }
                }
                _ => panic!("unknown arg {k}"),
            }
        }
        a
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Body,
    /// 1..=3
    Heading(u8),
    /// 0..=1
    Bullet(u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    Bold,
    Italic,
}

/// One paragraph: its text and styled byte ranges (non-overlapping, sorted).
#[derive(Clone, Debug)]
pub struct Paragraph {
    pub kind: Kind,
    pub text: String,
    pub spans: Vec<(usize, usize, Look)>,
}

pub const PARAGRAPH_GAP: i32 = 12;
pub const LEVEL_INDENT: i32 = 28;
pub const BODY_PT: f64 = 11.0;

pub fn heading_scale(level: u8) -> f64 {
    match level {
        1 => 2.0,
        2 => 1.5,
        _ => 1.25,
    }
}

const VOCAB: [&str; 48] = [
    "the", "of", "and", "a", "to", "in", "is", "that", "for", "it", "as", "with", "was",
    "on", "be", "by", "this", "are", "from", "or", "which", "document", "editor", "column",
    "paragraph", "language", "between", "measure", "quickly", "through", "without",
    "although", "remarkable", "infrastructure", "considered", "throughout", "Valparaíso",
    "Zürich", "naïve", "café", "résumé", "fjord", "quartz", "rhythm", "jukebox", "whisky",
    "embarrassment", "uncharacteristically",
];

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

struct Rng(u64);

impl Rng {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = splitmix(self.0);
        self.0 % n
    }
}

fn sentence_words(rng: &mut Rng, words: usize, out: &mut Paragraph) {
    let mut start_of_sentence = true;
    for i in 0..words {
        if i > 0 {
            out.text.push(' ');
        }
        let w = VOCAB[rng.next(VOCAB.len() as u64) as usize];
        let look = match rng.next(20) {
            0 => Some(Look::Bold),
            1 => Some(Look::Italic),
            _ => None,
        };
        let start = out.text.len();
        if start_of_sentence {
            let mut c = w.chars();
            if let Some(f) = c.next() {
                out.text.extend(f.to_uppercase());
                out.text.push_str(c.as_str());
            }
        } else {
            out.text.push_str(w);
        }
        if let Some(look) = look {
            out.spans.push((start, out.text.len(), look));
        }
        start_of_sentence = rng.next(12) == 0 || i + 1 == words;
        if start_of_sentence {
            out.text.push('.');
        }
    }
}

/// The deterministic test document of about `words` words.
pub fn document(words: usize) -> Vec<Paragraph> {
    let mut rng = Rng(0x5EED);
    let mut doc = Vec::new();
    let mut total = 0;
    let mut until_heading = 0;
    let mut until_list = 10;
    while total < words {
        if until_heading == 0 {
            let n = 2 + rng.next(6) as usize;
            let mut p = Paragraph {
                kind: Kind::Heading(1 + rng.next(3) as u8),
                text: String::new(),
                spans: Vec::new(),
            };
            sentence_words(&mut rng, n, &mut p);
            p.spans.clear();
            p.text.pop(); // headings have no full stop
            total += n;
            doc.push(p);
            until_heading = 8 + rng.next(9) as usize;
            continue;
        }
        if until_list == 0 {
            for _ in 0..3 + rng.next(4) {
                let n = 4 + rng.next(14) as usize;
                let mut p = Paragraph {
                    kind: Kind::Bullet(rng.next(2) as u8),
                    text: String::new(),
                    spans: Vec::new(),
                };
                sentence_words(&mut rng, n, &mut p);
                total += n;
                doc.push(p);
            }
            until_list = 15 + rng.next(10) as usize;
            continue;
        }
        let n = 40 + rng.next(141) as usize;
        let mut p = Paragraph {
            kind: Kind::Body,
            text: String::new(),
            spans: Vec::new(),
        };
        sentence_words(&mut rng, n, &mut p);
        total += n;
        doc.push(p);
        until_heading -= 1;
        until_list -= 1;
    }
    doc
}

/// CLOCK_MONOTONIC in ns; same clock as Python's time.monotonic_ns().
pub fn mono_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: valid pointer to a timespec on the stack.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

pub fn emit(line: std::fmt::Arguments) {
    let mut so = std::io::stdout().lock();
    let _ = so.write_fmt(line);
    let _ = so.write_all(b"\n");
    let _ = so.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_is_deterministic_and_sized() {
        let a = document(150_000);
        let b = document(150_000);
        assert_eq!(a.len(), b.len());
        assert_eq!(a[a.len() / 2].text, b[b.len() / 2].text);
        let words: usize = a.iter().map(|p| p.text.split(' ').count()).sum();
        assert!((150_000..150_200).contains(&words), "{words}");
    }
}
