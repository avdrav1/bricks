//! SLATE-11 acceptance: on a 150,000-word document at Full in a 1600x1000 window, the
//! visible text re-wraps within one frame, the whole document in under 500 ms, and
//! scrolling keeps frame CPU p99 under 16 ms.
//!
//! Writes ADR 0007's spike document as Markdown (headings, two bullet levels, bold and
//! italic words) and runs `slate FILE --bench-flow` on it. Opens a real window, so it needs
//! a graphical session; on a tiling desktop make the window float at 1600x1000 or the
//! numbers are for another size (the report says which). Timing is asserted only in
//! optimized builds:
//! `cargo test --release -p slate --test flow_perf -- --ignored --nocapture`

use std::process::Command;

const WORDS: usize = 150_000;

const VOCAB: [&str; 48] = [
    "the",
    "of",
    "and",
    "a",
    "to",
    "in",
    "is",
    "that",
    "for",
    "it",
    "as",
    "with",
    "was",
    "on",
    "be",
    "by",
    "this",
    "are",
    "from",
    "or",
    "which",
    "document",
    "editor",
    "column",
    "paragraph",
    "language",
    "between",
    "measure",
    "quickly",
    "through",
    "without",
    "although",
    "remarkable",
    "infrastructure",
    "considered",
    "throughout",
    "Valparaíso",
    "Zürich",
    "naïve",
    "café",
    "résumé",
    "fjord",
    "quartz",
    "rhythm",
    "jukebox",
    "whisky",
    "embarrassment",
    "uncharacteristically",
];

struct Rng(u64);

impl Rng {
    fn next(&mut self, n: u64) -> u64 {
        let mut x = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        self.0 = x ^ (x >> 31);
        self.0 % n
    }
}

/// `words` words of sentences, one word in ten bold or italic.
fn sentences(rng: &mut Rng, words: usize, out: &mut String) {
    let mut start = true;
    for i in 0..words {
        if i > 0 {
            out.push(' ');
        }
        let w = VOCAB[rng.next(VOCAB.len() as u64) as usize];
        let mark = match rng.next(20) {
            0 => "**",
            1 => "*",
            _ => "",
        };
        out.push_str(mark);
        if start {
            let mut c = w.chars();
            out.extend(c.next().into_iter().flat_map(char::to_uppercase));
            out.push_str(c.as_str());
        } else {
            out.push_str(w);
        }
        out.push_str(mark);
        start = rng.next(12) == 0 || i + 1 == words;
        if start {
            out.push('.');
        }
    }
}

/// The spike's document (spikes/slate-5/common.rs) as Markdown.
fn document() -> String {
    let mut rng = Rng(0x5EED);
    let mut md = String::new();
    let (mut total, mut until_heading, mut until_list) = (0, 0, 10);
    while total < WORDS {
        if until_heading == 0 {
            let n = 2 + rng.next(6) as usize;
            md.push_str(&"#".repeat(1 + rng.next(3) as usize));
            md.push(' ');
            let mut heading = String::new();
            sentences(&mut rng, n, &mut heading);
            md.push_str(heading.replace('*', "").trim_end_matches('.'));
            md.push_str("\n\n");
            total += n;
            until_heading = 8 + rng.next(9) as usize;
            continue;
        }
        if until_list == 0 {
            for i in 0..3 + rng.next(4) {
                let n = 4 + rng.next(14) as usize;
                let level = rng.next(2);
                md.push_str(if level == 1 && i > 0 { "  - " } else { "- " });
                sentences(&mut rng, n, &mut md);
                md.push('\n');
                total += n;
            }
            md.push('\n');
            until_list = 15 + rng.next(10) as usize;
            continue;
        }
        let n = 40 + rng.next(141) as usize;
        sentences(&mut rng, n, &mut md);
        md.push_str("\n\n");
        total += n;
        until_heading -= 1;
        until_list -= 1;
    }
    md
}

/// The number at `path` (keys separated by `/`) in a flat JSON report.
fn field(json: &str, path: &str) -> f64 {
    let mut rest = json;
    for key in path.split('/') {
        let at = rest
            .find(&format!("\"{key}\":"))
            .unwrap_or_else(|| panic!("no {path} in {json}"));
        rest = &rest[at + key.len() + 3..];
    }
    let end = rest.find([',', '}', ']']).unwrap();
    rest[..end].parse().unwrap()
}

#[test]
#[ignore = "opens a window; run with --release --ignored"]
fn the_flow_rewraps_and_scrolls_a_150000_word_document_in_time() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        panic!("needs a graphical session");
    }
    let dir = std::env::temp_dir().join(format!("slate-flow-perf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("long.md");
    std::fs::write(&path, document()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_slate"))
        .arg(&path)
        .arg("--bench-flow")
        .output()
        .expect("run slate");
    std::fs::remove_dir_all(&dir).ok();
    let line = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    assert!(
        out.status.success() && line.starts_with('{'),
        "{line}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    eprintln!("{line}");
    assert!(field(&line, "words") >= WORDS as f64, "{line}");
    assert_eq!(field(&line, "switches"), 10.0, "{line}");
    assert_eq!(field(&line, "unfinished"), 0.0, "{line}");
    // The frame after a width change shows the new column.
    assert_eq!(field(&line, "first_frame/frames_max"), 1.0, "{line}");
    if cfg!(debug_assertions) {
        eprintln!("debug build: timing not checked; use --release");
        return;
    }
    let first = field(&line, "first_frame/cpu_ms/max");
    let whole = field(&line, "whole_ms/max");
    let scroll = field(&line, "scroll/frame_cpu_ms/p99");
    eprintln!(
        "re-wrap: visible text in one frame of at most {first:.1} ms, whole document at most \
         {whole:.0} ms (target < 500); scrolling frame CPU p99 {scroll:.1} ms (target < 16)"
    );
    assert!(
        first < 1000.0 / 60.0,
        "the re-wrapped frame took {first:.1} ms"
    );
    assert!(whole < 500.0, "re-wrapping the document took {whole:.0} ms");
    assert!(scroll < 16.0, "scrolling frames took {scroll:.1} ms p99");
}
