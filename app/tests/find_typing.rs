//! SRCH-2 acceptance: a new keystroke cancels the running search. `--bench-find portland`
//! types "portland" into the find bar one character every 200 ms on the 1 GB file and
//! prints each keystroke, search start, and result (see `bench_find` in main.rs). Every
//! search still running when a key comes must come back cancelled, within 200 ms of the
//! key, and the last search must land on the first Portland, "1 of 2,725,768".
//!
//! Opens a real window, so it needs a graphical session (Broadway works) and the corpus:
//! `cargo test --release -p spreadsheet --test find_typing -- --ignored --nocapture`

use std::path::Path;
use std::process::Command;

/// Cells containing "portland" in corpus/rows_1024mb.csv (the city column).
const PORTLAND: u64 = 2_725_768;

#[derive(Debug)]
enum Event {
    Key { cancelled: bool },
    Start(String),
    Done { result: String, text: String },
}

fn parse(line: &str) -> Option<(f64, Event)> {
    let mut f = line.strip_prefix("FIND ")?.splitn(4, ' ');
    let ms: f64 = f.next()?.parse().ok()?;
    let event = match f.next()? {
        "key" => Event::Key {
            cancelled: f.next()? == "1",
        },
        "start" => Event::Start(f.collect::<Vec<_>>().join(" ")),
        "done" => Event::Done {
            result: f.next()?.to_owned(),
            text: f.next().unwrap_or("").to_owned(),
        },
        _ => return None,
    };
    Some((ms, event))
}

#[test]
#[ignore = "opens a window and needs corpus/; run with --release --ignored"]
fn a_new_keystroke_cancels_the_running_search() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        panic!("needs a graphical session");
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../corpus/rows_1024mb.csv");
    assert!(
        path.exists(),
        "missing {}; run python3 scripts/gen_corpus.py",
        path.display()
    );
    let out = Command::new(env!("CARGO_BIN_EXE_spreadsheet"))
        .arg(&path)
        .args(["--bench-find", "portland"])
        .output()
        .expect("run spreadsheet");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    eprintln!("{stdout}");
    let events: Vec<(f64, Event)> = stdout.lines().filter_map(parse).collect();

    let mut interrupted = 0;
    for (i, (_, e)) in events.iter().enumerate() {
        let Event::Start(text) = e else { continue };
        let (done, result) = events[i + 1..]
            .iter()
            .find_map(|(t, e)| match e {
                Event::Done { result, text: d } if d == text => Some((*t, result)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the search for {text:?} never came back"));
        let key = events[i + 1..]
            .iter()
            .find(|(t, e)| *t <= done && matches!(e, Event::Key { .. }));
        match key {
            Some((at, Event::Key { cancelled })) => {
                interrupted += 1;
                assert!(cancelled, "the key at {at} ms didn't cancel {text:?}");
                assert_eq!(result, "cancelled", "{text:?} ran past a keystroke");
                let late = done - at;
                eprintln!("{text:?} cancelled {late:.1} ms after the next key");
                assert!(late < 200.0, "{text:?}: {late:.1} ms after the key");
            }
            _ => assert_ne!(result, "cancelled", "{text:?} cancelled with no key"),
        }
    }
    assert!(
        interrupted > 0,
        "no search was running at a keystroke: nothing tested"
    );
    let last = events.iter().rev().find_map(|(_, e)| match e {
        Event::Done { result, text } if text == "portland" => Some(result.as_str()),
        _ => None,
    });
    assert_eq!(last, Some(format!("hit:1/{PORTLAND}").as_str()));
}
