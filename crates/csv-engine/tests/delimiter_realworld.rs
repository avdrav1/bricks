//! ENG-4 acceptance: the delimiter is detected correctly on 95%+ of a 50-file real-world
//! sample (comma, semicolon, tab, pipe; from public data portals).
//!
//! The sample is listed in `scripts/delimiter_sample.tsv` and downloaded, 256 KiB per file,
//! by `python3 scripts/fetch_delimiter_sample.py` into `corpus/realworld/`. Detection sees the
//! first 64 KiB of each file, the same amount the app reads on open.
//! Run: `cargo test -p csv-engine --test delimiter_realworld -- --ignored --nocapture`

use csv_engine::{detect_dialect, DETECT_SAMPLE_BYTES};
use std::path::Path;

#[test]
#[ignore = "needs the downloaded sample; run with --ignored"]
fn detects_95_percent_of_the_real_world_sample() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest = std::fs::read_to_string(root.join("scripts/delimiter_sample.tsv")).unwrap();
    let dir = root.join("corpus/realworld");
    let (mut total, mut correct, mut misses) = (0, 0, Vec::new());
    for line in manifest.lines().skip(1) {
        let mut cols = line.split('\t');
        let (name, label) = (cols.next().unwrap(), cols.next().unwrap());
        let want = match label {
            "comma" => b',',
            "semicolon" => b';',
            "tab" => b'\t',
            "pipe" => b'|',
            other => panic!("unknown label {other}"),
        };
        let bytes = std::fs::read(dir.join(name)).unwrap_or_else(|_| {
            panic!("missing {name}; run python3 scripts/fetch_delimiter_sample.py")
        });
        let sample = &bytes[..bytes.len().min(DETECT_SAMPLE_BYTES)];
        let got = detect_dialect(sample).delimiter;
        total += 1;
        if got == want {
            correct += 1;
        } else {
            misses.push(format!(
                "{name}: detected {:?}, expected {:?}",
                got as char, want as char
            ));
        }
    }
    eprintln!(
        "{correct} of {total} correct ({:.0}%)",
        100.0 * correct as f64 / total as f64
    );
    for m in &misses {
        eprintln!("  miss: {m}");
    }
    assert_eq!(total, 50, "the sample is 50 files");
    assert!(correct * 100 >= total * 95, "{correct}/{total}: {misses:?}");
}
