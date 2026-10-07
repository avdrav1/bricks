//! GRID-3 acceptance: keyboard navigation matches LibreOffice on the side-by-side checklist
//! (docs/navigation-checklist.md). Every scenario recorded from real LibreOffice Calc by
//! `scripts/lo_navigation.py` is replayed here and must end on the same cursor cell and
//! selection.

use grid::{Bounds, Cell, Key, Mods, Selection};

const DATA: &str = include_str!("data/lo_navigation.tsv");
/// The recording's sheet: 200 rows x 6 columns of data.
const ROWS: u64 = 200;
const COLS: u32 = 6;

fn parse_cell(s: &str) -> Cell {
    let letters: String = s.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let col = letters
        .bytes()
        .fold(0u32, |acc, b| acc * 26 + u32::from(b - b'A' + 1))
        - 1;
    let row: u64 = s[letters.len()..].parse().unwrap();
    Cell { row: row - 1, col }
}

fn name(c: Cell) -> String {
    let mut col = c.col + 1;
    let mut letters = String::new();
    while col > 0 {
        let r = (col - 1) % 26;
        letters.insert(0, (b'A' + r as u8) as char);
        col = (col - 1) / 26;
    }
    format!("{letters}{}", c.row + 1)
}

fn parse_key(spec: &str) -> (Key, Mods) {
    let mut mods = Mods::default();
    let mut key = None;
    for part in spec.split('+') {
        match part {
            "SHIFT" => mods.shift = true,
            "CTRL" => mods.ctrl = true,
            "UP" => key = Some(Key::Up),
            "DOWN" => key = Some(Key::Down),
            "LEFT" => key = Some(Key::Left),
            "RIGHT" => key = Some(Key::Right),
            "TAB" => key = Some(Key::Tab),
            "RETURN" => key = Some(Key::Enter),
            "HOME" => key = Some(Key::Home),
            "END" => key = Some(Key::End),
            "PAGEUP" => key = Some(Key::PageUp),
            "PAGEDOWN" => key = Some(Key::PageDown),
            other => panic!("unknown key part {other}"),
        }
    }
    (key.unwrap_or_else(|| panic!("no key in {spec}")), mods)
}

#[test]
fn navigation_matches_libreoffice() {
    let rows: Vec<Vec<&str>> = DATA
        .lines()
        .filter(|l| !l.starts_with('#'))
        .skip(1)
        .map(|l| l.split('\t').collect())
        .collect();
    // LibreOffice's screen height: how far one Page Down moved from A1.
    let page = rows
        .iter()
        .find(|r| r[0] == "page-down")
        .map(|r| parse_cell(r[3]).row)
        .unwrap();
    let bounds = Bounds {
        rows: ROWS,
        cols: COLS,
        page_rows: page,
    };

    let mut failures = Vec::new();
    for r in &rows {
        let (id, start, keys, want_cursor, want_range) = (r[0], r[1], r[2], r[3], r[4]);
        let mut sel = Selection::at(parse_cell(start));
        for k in keys.split(' ') {
            let (key, mods) = parse_key(k);
            sel.press(key, mods, bounds);
        }
        let (top_left, bottom_right) = sel.range();
        let got = (
            name(sel.cursor()),
            format!("{}:{}", name(top_left), name(bottom_right)),
        );
        if got != (want_cursor.to_owned(), want_range.to_owned()) {
            failures.push(format!(
                "{id}: {start} {keys} -> {got:?}, LibreOffice ({want_cursor}, {want_range})"
            ));
        }
    }
    assert!(rows.len() >= 39, "checklist has {} rows", rows.len());
    assert!(
        failures.is_empty(),
        "differs from LibreOffice:\n{}",
        failures.join("\n")
    );
}
