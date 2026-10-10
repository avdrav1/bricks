//! Status bar text (APP-2): row count with indexing progress, the file's format, and the
//! save state. Plain functions of what the window knows, so the wording is tested without
//! a display; `main.rs` puts the strings into labels every status tick.

use csv_engine::Encoding;
use std::time::Duration;

/// Where saving stands, as far as the status bar cares.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SaveView<'a> {
    Idle,
    /// Share of the file written so far (0..=1).
    Saving(f64),
    /// Written; reading it back to verify.
    Checking,
    Saved(Duration),
    Cancelled,
    Failed(&'a str),
}

/// The last copy or paste (CLIP-1, CLIP-2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClipView {
    Idle,
    /// Share of the rows copied so far (0..=1).
    Copying(f64),
    /// Just copied: cells, and whether it was too big for the HTML (spreadsheet) form.
    Copied {
        cells: u64,
        text_only: bool,
    },
    Pasted(u64),
    /// A paste refused for its size: its cells, and the most allowed.
    PasteTooLarge {
        cells: u64,
        max: u64,
    },
    /// A paste that needs rows added before indexing has finished.
    PasteNeedsIndex,
}

/// Everything the status bar shows.
#[derive(Debug, Clone, Copy)]
pub struct Facts<'a> {
    pub rows: u64,
    /// While filtered, the rows there are in all: "x of y rows" (FILT-1).
    pub of: Option<u64>,
    /// Share of the file indexed so far (0..=1); `None` once indexing is complete.
    pub indexing: Option<f64>,
    pub delimiter: u8,
    /// The delimiter was detected (Auto), not chosen.
    pub detected: bool,
    pub encoding: Encoding,
    pub edits: usize,
    /// A running sort or filter ("Sorting", "Filtering") and its share done (0..=1).
    pub job: Option<(&'a str, f64)>,
    pub save: SaveView<'a>,
    pub file_changed: bool,
    pub clip: ClipView,
}

/// The three parts of the status bar, left to right.
#[derive(Debug, PartialEq, Eq)]
pub struct Status {
    /// "1,898,604 rows" ("14,392 of 1,898,604 rows" while filtered), plus "indexing 43%"
    /// while the count still grows.
    pub rows: String,
    /// Unsaved edits, saving progress or result, and a changed-on-disk warning.
    pub save: String,
    /// Delimiter and encoding, e.g. "Comma (detected) · UTF-8".
    pub format: String,
}

pub fn status(f: &Facts) -> Status {
    let noun = |n: u64| if n == 1 { "row" } else { "rows" };
    let mut rows = match f.of {
        Some(all) => format!(
            "{} of {} {}",
            group_digits(f.rows),
            group_digits(all),
            noun(all)
        ),
        None => format!("{} {}", group_digits(f.rows), noun(f.rows)),
    };
    if let Some(done) = f.indexing {
        let pct = (done.clamp(0.0, 1.0) * 100.0).floor();
        rows.push_str(&format!(" · indexing {pct:.0}%"));
    }

    let mut save = Vec::new();
    if let Some((what, done)) = f.job {
        let pct = (done.clamp(0.0, 1.0) * 100.0).floor();
        save.push(format!("{what} {pct:.0}%"));
    }
    let cells = |n: u64| format!("{} cell{}", group_digits(n), if n == 1 { "" } else { "s" });
    match f.clip {
        ClipView::Idle => {}
        ClipView::Copying(done) => {
            let pct = (done.clamp(0.0, 1.0) * 100.0).floor();
            save.push(format!("Copying {pct:.0}%"));
        }
        ClipView::Copied {
            cells: n,
            text_only,
        } => save.push(format!(
            "Copied {}{}",
            cells(n),
            if text_only { " as plain text" } else { "" }
        )),
        ClipView::Pasted(n) => save.push(format!("Pasted {}", cells(n))),
        ClipView::PasteTooLarge { cells: n, max } => save.push(format!(
            "Paste too large: {} (at most {})",
            cells(n),
            group_digits(max)
        )),
        ClipView::PasteNeedsIndex => {
            save.push("Paste past the last row once indexing finishes".to_owned())
        }
    }
    let edits = || {
        format!(
            "{} unsaved edit{}",
            group_digits(f.edits as u64),
            if f.edits == 1 { "" } else { "s" }
        )
    };
    match f.save {
        SaveView::Saving(done) => {
            let pct = (done.clamp(0.0, 1.0) * 100.0).floor();
            save.push(format!("Saving {pct:.0}%"));
        }
        SaveView::Checking => save.push("Checking the saved file…".to_owned()),
        SaveView::Failed(e) => {
            save.push(format!("Save failed: {e}"));
            if f.edits > 0 {
                save.push(edits());
            }
        }
        SaveView::Cancelled => {
            save.push("Save cancelled".to_owned());
            if f.edits > 0 {
                save.push(edits());
            }
        }
        // A save result is old news once there are new edits.
        SaveView::Saved(took) if f.edits == 0 => {
            save.push(format!("Saved in {:.1} s", took.as_secs_f64()))
        }
        SaveView::Saved(_) | SaveView::Idle if f.edits > 0 => save.push(edits()),
        SaveView::Saved(_) | SaveView::Idle => {}
    }
    if f.file_changed {
        save.push("File changed on disk".to_owned());
    }

    let mut format = delimiter_name(f.delimiter).to_owned();
    if f.detected {
        format.push_str(" (detected)");
    }
    format.push_str(" · ");
    format.push_str(f.encoding.name());

    Status {
        rows,
        save: save.join(" · "),
        format,
    }
}

/// Window title: the file name, marked while there are unsaved edits.
pub fn title(name: &str, edits: usize) -> String {
    if edits > 0 {
        format!("• {name}")
    } else {
        name.to_owned()
    }
}

/// The find bar's note (SRCH-2, SRCH-3): the count streaming in while matches are
/// counted, then which match the cursor is on, or how a replace all went.
pub enum FindNote {
    Counting { found: u64 },
    Hit { ordinal: u64, total: u64 },
    Nothing,
    NotReady,
    Replaced { cells: u64 },
    TooMany { cells: u64 },
}

pub fn find_note(n: FindNote) -> String {
    let cells = |n: u64| {
        format!(
            "{} {}",
            group_digits(n),
            if n == 1 { "cell" } else { "cells" }
        )
    };
    match n {
        FindNote::Counting { found } => format!("{} so far…", group_digits(found)),
        FindNote::Hit { ordinal, total } => {
            format!("{} of {}", group_digits(ordinal), group_digits(total))
        }
        FindNote::Nothing => "No matches".into(),
        FindNote::NotReady => "Wait for indexing to finish".into(),
        FindNote::Replaced { cells: n } => format!("Replaced {}", cells(n)),
        FindNote::TooMany { cells: n } => format!(
            "Too many: {} (max {})",
            group_digits(n),
            group_digits(data_model::REPLACE_MAX_CELLS)
        ),
    }
}

fn delimiter_name(d: u8) -> &'static str {
    match d {
        b',' => "Comma",
        b'\t' => "Tab",
        b';' => "Semicolon",
        b'|' => "Pipe",
        _ => "Other delimiter",
    }
}

pub fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts<'static> {
        Facts {
            rows: 1_898_604,
            of: None,
            indexing: None,
            delimiter: b',',
            detected: true,
            encoding: Encoding::UTF8,
            edits: 0,
            job: None,
            save: SaveView::Idle,
            file_changed: false,
            clip: ClipView::Idle,
        }
    }

    #[test]
    fn rows_show_progress_until_indexing_completes() {
        let mut f = facts();
        f.indexing = Some(0.437);
        assert_eq!(status(&f).rows, "1,898,604 rows · indexing 43%");
        f.indexing = Some(1.0); // the last bytes are scanned but the build hasn't finished
        assert_eq!(status(&f).rows, "1,898,604 rows · indexing 100%");
        f.indexing = None;
        assert_eq!(status(&f).rows, "1,898,604 rows");
        f.rows = 1;
        assert_eq!(status(&f).rows, "1 row");
    }

    /// FILT-2 acceptance: while filters hide rows, the bar says how many are shown of all.
    #[test]
    fn filtered_rows_read_x_of_y() {
        let mut f = facts();
        f.rows = 14_392;
        f.of = Some(2_103_814);
        assert_eq!(
            status(&f).rows,
            "14,392 of 2,103,814 rows",
            "the spec's example"
        );
        f.rows = 0;
        f.of = Some(19_094_579);
        assert_eq!(status(&f).rows, "0 of 19,094,579 rows");
        f.rows = 1;
        f.of = Some(1);
        assert_eq!(status(&f).rows, "1 of 1 row");
        f.of = None;
        assert_eq!(status(&f).rows, "1 row", "no filter: just the count");
    }

    #[test]
    fn save_state_wording_and_precedence() {
        let mut f = facts();
        assert_eq!(status(&f).save, "");
        f.edits = 1;
        assert_eq!(status(&f).save, "1 unsaved edit");
        f.edits = 1_200;
        assert_eq!(status(&f).save, "1,200 unsaved edits");
        f.save = SaveView::Failed("disk full");
        assert_eq!(
            status(&f).save,
            "Save failed: disk full · 1,200 unsaved edits"
        );
        f.save = SaveView::Saved(Duration::from_millis(910));
        assert_eq!(
            status(&f).save,
            "1,200 unsaved edits",
            "new edits after a save"
        );
        f.edits = 0;
        assert_eq!(status(&f).save, "Saved in 0.9 s");
        f.file_changed = true;
        assert_eq!(status(&f).save, "Saved in 0.9 s · File changed on disk");
    }

    #[test]
    fn clipboard_progress_and_result_come_first() {
        let mut f = facts();
        f.edits = 2;
        f.clip = ClipView::Copying(0.456);
        assert_eq!(status(&f).save, "Copying 45% · 2 unsaved edits");
        f.job = Some(("Sorting", 0.5));
        assert_eq!(
            status(&f).save,
            "Sorting 50% · Copying 45% · 2 unsaved edits"
        );
        f.job = None;
        f.clip = ClipView::Copied {
            cells: 1,
            text_only: false,
        };
        assert_eq!(status(&f).save, "Copied 1 cell · 2 unsaved edits");
        f.clip = ClipView::Pasted(80_000);
        assert_eq!(status(&f).save, "Pasted 80,000 cells · 2 unsaved edits");
        f.edits = 0;
        f.clip = ClipView::Copied {
            cells: 19_094_579,
            text_only: true,
        };
        assert_eq!(status(&f).save, "Copied 19,094,579 cells as plain text");
        f.clip = ClipView::PasteTooLarge {
            cells: 3_000_000,
            max: 2_097_152,
        };
        assert_eq!(
            status(&f).save,
            "Paste too large: 3,000,000 cells (at most 2,097,152)"
        );
    }

    #[test]
    fn format_names_delimiter_and_encoding() {
        let mut f = facts();
        assert_eq!(status(&f).format, "Comma (detected) · UTF-8");
        f.delimiter = b'\t';
        f.detected = false;
        f.encoding = Encoding {
            bom: true,
            ..Encoding::UTF8
        };
        assert_eq!(status(&f).format, "Tab · UTF-8 with BOM");
    }

    #[test]
    fn title_marks_unsaved_edits() {
        assert_eq!(title("a.csv", 0), "a.csv");
        assert_eq!(title("a.csv", 3), "• a.csv");
    }
}
