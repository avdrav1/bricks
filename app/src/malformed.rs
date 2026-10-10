//! Malformed files (APP-8). A binary file is refused when it opens; otherwise the file
//! opens as read (the parser is lenient), and once it is indexed a job checks every row.
//! If something is wrong, a dialog names the lines and shows their text, and can put the
//! cursor on the first one. Checked once per reading of the file (delimiter and encoding):
//! a save re-reads it, and unedited rows are saved as they were, so asking again would
//! only repeat it.

use crate::grid_view::GridView;
use crate::{status, Session};
use data_model::{CsvTable, Problem, ProblemKind, Problems};
use gtk::{glib, prelude::*};
use std::rc::Rc;

/// Problems listed in the dialog; the rest are counted.
const LISTED: usize = 3;

/// Refuse a binary file (a zero byte early on): it isn't CSV, and showing it would only
/// show noise.
pub fn refuse_binary(table: &CsvTable) -> std::io::Result<()> {
    match table.binary_at_start() {
        None => Ok(()),
        Some(p) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "It holds binary data, not text. Line {} has a zero byte: “{}”",
                status::group_digits(p.line),
                p.excerpt
            ),
        )),
    }
}

/// The status tick: once a new reading is indexed, check it on the job pool.
pub fn tick(window: &gtk::ApplicationWindow, grid: &GridView, session: &Rc<Session>) {
    if !grid.is_complete() || grid.busy() || session.path.borrow().is_none() {
        return;
    }
    let reading = (grid.delimiter(), grid.encoding());
    if session.checked.get() == Some(reading) {
        return;
    }
    session.checked.set(Some(reading));
    let Some(table) = grid.snapshot() else {
        return;
    };
    let generation = grid.generation();
    let job = jobs::spawn(move |cancel| table.check_file(cancel.flag()));
    let (window, grid, session) = (window.downgrade(), grid.downgrade(), session.clone());
    glib::spawn_future_local(async move {
        let problems = job.finished().await;
        let (Some(window), Some(grid)) = (window.upgrade(), grid.upgrade()) else {
            return;
        };
        match problems {
            Ok(Some(p)) if !p.is_empty() && grid.generation() == generation => {
                show(&window, &grid, &session, &p)
            }
            // Cancelled, or another reading came in meanwhile: check that one instead.
            Ok(None) => session.checked.set(None),
            _ => {}
        }
    });
}

fn what(kind: ProblemKind) -> &'static str {
    match kind {
        ProblemKind::UnclosedQuote => {
            "a quote opens a field and never closes, so the rest of the file reads as one cell"
        }
        ProblemKind::TextAfterQuote => "text follows a closing quote",
        ProblemKind::Binary => "binary data (a zero byte)",
    }
}

/// "Line 1,203: text follows a closing quote" and the line's text under it.
fn describe(p: &Problem) -> String {
    format!(
        "Line {}: {}:\n    {}",
        status::group_digits(p.line),
        what(p.kind),
        p.excerpt
    )
}

fn show(window: &gtk::ApplicationWindow, grid: &GridView, session: &Session, p: &Problems) {
    let mut detail: Vec<String> = p.first.iter().take(LISTED).map(describe).collect();
    let more = p.total - detail.len() as u64;
    if more > 0 {
        detail.push(format!(
            "…and {} more {}.",
            status::group_digits(more),
            if more == 1 { "problem" } else { "problems" }
        ));
    }
    detail.push("The file is shown as read. Rows you don't edit are saved unchanged.".into());
    let first = &p.first[0];
    let dialog = gtk::AlertDialog::builder()
        .message(if p.total == 1 {
            format!("“{}” has a malformed line", session.name())
        } else {
            format!("“{}” has malformed lines", session.name())
        })
        .detail(detail.join("\n\n"))
        .buttons([
            format!("Go to Line {}", status::group_digits(first.line)).as_str(),
            "Close",
        ])
        .cancel_button(1)
        .default_button(1)
        .modal(true)
        .build();
    let (window, grid, file_row) = (window.clone(), grid.clone(), first.file_row);
    glib::spawn_future_local(async move {
        if let Ok(0) = dialog.choose_future(Some(&window)).await {
            grid.go_to_file_row(file_row);
        }
    });
}
