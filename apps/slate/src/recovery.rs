//! Crash recovery (SLATE-17). A window with unsaved edits keeps a journal of its document,
//! written [`WRITE_AFTER`] after the first edit not yet in it (so at most that often while
//! editing goes on), off the UI thread, in `$XDG_STATE_HOME/slate/recovery` (the suite's
//! [`Store`]: one journal per window, an owner lock per process). The journal goes once
//! nothing is unsaved (saved, or every edit undone) and when the window closes, so one still
//! there after its process ended means a crash or a kill (`saving::Saver` keeps it).
//!
//! A journal holds a line of [`Header`] (the file's path, its size and modification time
//! when it was read or last saved, when the journal was written) and the document's blocks,
//! both JSON. After a crash, the start window lists the journals (Restore, Discard), and
//! opening such a file any other way asks "Restore unsaved edits?". Restoring opens the
//! file as it is and puts the journal's blocks in place of its own as one undoable step,
//! so the window shows them unsaved ("• ") and the file stays as it was until saved. If the
//! file changed after the crash, the edits aren't put onto it: they're kept for later or
//! discarded.

use crate::open::{self, Opened};
use document_model::Block;
use gtk::glib;
use gtk::prelude::*;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
pub use suite_file::recovery::{identity, now_ms, Store};

/// How long the first edit not in the journal waits before the journal is written.
pub const WRITE_AFTER: Duration = Duration::from_secs(5);

/// What a journal file starts with.
const MAGIC: &str = "SLATEJ1 ";

/// What a journal says about its document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// The document's file; `None` for a new document not saved yet.
    pub path: Option<PathBuf>,
    /// The file's size and modification time when it was read or last saved.
    pub file: Option<(u64, u64)>,
    /// When the journal was written, in milliseconds since the Unix epoch.
    pub saved_ms: u64,
}

/// A journal left by a Slate process that is gone.
#[derive(Debug, Clone)]
pub struct Crashed {
    pub journal: PathBuf,
    pub header: Header,
}

/// The user's recovery directory: `$XDG_STATE_HOME/slate/recovery` (`~/.local/state/…`).
pub fn store() -> Store {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
    Store::at(state.join("slate").join("recovery"))
}

/// Write `blocks` and `header` as the journal `journal`, atomically.
pub fn write(
    store: &Store,
    journal: &Path,
    header: &Header,
    blocks: &[Block],
) -> std::io::Result<()> {
    store.write(journal, |f| {
        let mut w = std::io::BufWriter::new(f);
        w.write_all(MAGIC.as_bytes())?;
        serde_json::to_writer(&mut w, header)?;
        w.write_all(b"\n")?;
        serde_json::to_writer(&mut w, blocks)?;
        w.flush()
    })
}

/// The header of the journal at `path`, reading only its first line.
fn read_header(path: &Path) -> Option<Header> {
    let mut line = String::new();
    BufReader::new(std::fs::File::open(path).ok()?)
        .read_line(&mut line)
        .ok()?;
    serde_json::from_str(line.strip_prefix(MAGIC)?).ok()
}

/// The blocks of the journal at `path`.
pub fn read_blocks(path: &Path) -> std::io::Result<Vec<Block>> {
    let mut r = BufReader::new(std::fs::File::open(path)?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    if !line.starts_with(MAGIC) {
        return Err(std::io::Error::other("not a Slate recovery journal"));
    }
    let mut rest = Vec::new();
    r.read_to_end(&mut rest)?;
    Ok(serde_json::from_slice(&rest)?)
}

/// The journals of Slate processes that are gone, newest first.
pub fn crashed(store: &Store) -> Vec<Crashed> {
    let mut found: Vec<Crashed> = store
        .crashed()
        .into_iter()
        .filter_map(|journal| {
            let header = read_header(&journal)?; // being written, or not ours
            Some(Crashed { journal, header })
        })
        .collect();
    found.sort_by_key(|c| std::cmp::Reverse(c.header.saved_ms));
    found
}

/// A crashed session's journal for the file at `path` (canonical), if there is one.
pub fn crashed_for(path: &Path) -> Option<Crashed> {
    crashed(&store())
        .into_iter()
        .find(|c| c.header.path.as_deref().map(crate::canonical).as_deref() == Some(path))
}

/// A journal to offer back in a window being opened, read with its file.
pub struct Recovered {
    crashed: Crashed,
    /// Restore without asking (chosen on the start window).
    ask: bool,
    /// The journal's blocks; `None` when the file changed after the crash, so they can't
    /// go onto it.
    blocks: Option<Result<Vec<Block>, String>>,
}

impl Recovered {
    /// Read `crashed`'s blocks for the file just `opened`, decoding their pictures into
    /// its textures; off the UI thread. `None` once the journal is gone (restored or
    /// discarded meanwhile, by another Slate).
    pub fn read(crashed: Crashed, ask: bool, opened: &mut Opened) -> Option<Recovered> {
        if !crashed.journal.exists() {
            return None;
        }
        let blocks = if crashed.header.path.is_some() && crashed.header.file != opened.identity {
            None
        } else {
            let blocks = read_blocks(&crashed.journal).map_err(|e| e.to_string());
            if let Ok(blocks) = &blocks {
                open::decode_pictures(blocks, &mut opened.textures);
            }
            Some(blocks)
        };
        Some(Recovered {
            crashed,
            ask,
            blocks,
        })
    }

    /// Offer the edits back in `window`, whose page shows the file as it is now: ask
    /// first unless chosen already; say so when the file changed after the crash.
    pub fn offer(self, window: &gtk::ApplicationWindow, page: &crate::view::Page) {
        let Recovered {
            crashed,
            ask,
            blocks,
        } = self;
        let (window, page) = (window.clone(), page.clone());
        glib::spawn_future_local(async move {
            let h = &crashed.header;
            let blocks = match blocks {
                None => {
                    let dialog = gtk::AlertDialog::builder()
                        .message(format!("{} changed after the crash", name(h)))
                        .detail(format!(
                            "Slate closed unexpectedly at {} with unsaved edits, but the file \
                             on disk has changed since, so they can't be put back onto it.",
                            when(h.saved_ms)
                        ))
                        .buttons(["Keep for Later", "Discard"])
                        .cancel_button(0)
                        .default_button(0)
                        .modal(true)
                        .build();
                    if let Ok(1) = dialog.choose_future(Some(&window)).await {
                        store().remove(&crashed.journal);
                    }
                    return;
                }
                Some(Err(why)) => {
                    tracing::warn!("cannot restore unsaved edits: {why}");
                    crate::alert(
                        Some(window.upcast_ref()),
                        "Cannot restore unsaved edits",
                        &why,
                    );
                    return;
                }
                Some(Ok(blocks)) => blocks,
            };
            if ask {
                let dialog = gtk::AlertDialog::builder()
                    .message("Restore unsaved edits?")
                    .detail(format!(
                        "Slate closed unexpectedly at {} while {} had unsaved edits.",
                        when(h.saved_ms),
                        name(h)
                    ))
                    .buttons(["Discard", "Restore"])
                    .default_button(1)
                    .modal(true)
                    .build();
                match dialog.choose_future(Some(&window)).await {
                    Ok(1) => {}
                    Ok(0) => {
                        store().remove(&crashed.journal);
                        return;
                    }
                    _ => return, // dismissed: offered again next time
                }
            }
            page.restore(blocks);
            store().remove(&crashed.journal);
        });
    }
}

/// "14:32" today, else "Oct 9, 14:32".
fn when(ms: u64) -> String {
    let at = glib::DateTime::from_unix_local((ms / 1000) as i64);
    let now = glib::DateTime::now_local();
    let format = match (&at, &now) {
        (Ok(a), Ok(n)) if a.ymd() == n.ymd() => "%H:%M",
        _ => "%b %-d, %H:%M",
    };
    at.and_then(|a| a.format(format))
        .map_or_else(|_| "an earlier session".into(), |s| s.to_string())
}

/// The document's name in a sentence: “notes.md”, or an untitled document.
fn name(h: &Header) -> String {
    h.path.as_deref().map_or_else(
        || "an untitled document".to_owned(),
        |p| format!("“{}”", suite_file::document_name(p)),
    )
}

/// The start window's list of crashed sessions, and its first Restore button (it takes
/// the focus: after a crash, putting the work back is the likely next step). The start
/// window stays open while the list has sessions left, so each can be restored in turn.
pub fn start_section(app: &gtk::Application) -> Option<(gtk::Box, gtk::Button)> {
    let crashed = crashed(&store());
    LEFT.set(crashed.len());
    if crashed.is_empty() {
        return None;
    }
    let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
    section.set_margin_top(12);
    let title = gtk::Label::new(Some("Unsaved edits from a crash"));
    title.add_css_class("heading");
    section.append(&title);
    let mut first = None;
    for c in crashed {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let h = &c.header;
        let label = h.path.as_deref().map_or_else(
            || crate::saving::UNTITLED.to_owned(),
            |p| suite_file::document_name(p).into_owned(),
        );
        row.append(&gtk::Label::new(Some(&label)));
        let detail = gtk::Label::new(Some(&format!("unsaved edits · {}", when(h.saved_ms))));
        detail.add_css_class("dim-label");
        detail.set_hexpand(true);
        detail.set_xalign(0.0);
        row.append(&detail);
        let restore = gtk::Button::with_label("Restore");
        restore.add_css_class("suggested-action");
        let discard = gtk::Button::with_label("Discard");
        discard.add_css_class("flat");
        row.append(&discard);
        row.append(&restore);
        if let Some(p) = &h.path {
            row.set_tooltip_text(Some(&p.display().to_string()));
        }
        restore.connect_clicked({
            let (app, c, row) = (app.clone(), c.clone(), row.downgrade());
            move |_| {
                if let Some(r) = row.upgrade() {
                    r.set_visible(false);
                }
                LEFT.set(LEFT.get().saturating_sub(1));
                let path = c.header.path.clone();
                crate::open_window(&app, path.as_deref(), None, Some(c.clone()));
            }
        });
        discard.connect_clicked({
            let (journal, row) = (c.journal.clone(), row.downgrade());
            move |_| {
                store().remove(&journal);
                LEFT.set(LEFT.get().saturating_sub(1));
                if let Some(r) = row.upgrade() {
                    r.set_visible(false);
                }
            }
        });
        first.get_or_insert(restore);
        section.append(&row);
    }
    Some((section, first?))
}

thread_local! {
    /// Crashed sessions the start window still lists.
    static LEFT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The start window lists crashed sessions not yet restored or discarded.
pub fn offers_left() -> bool {
    LEFT.get() > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use document_model::{
        Border, BorderOverrides, BorderStyle, LineSpacing, ParaOverrides, Paragraph, RunStyle,
        Shading,
    };

    /// A journal reads back its header and blocks; listing reads only the header, newest
    /// first; a file that isn't a journal is left out.
    #[test]
    fn journals_round_trip_and_list_newest_first() {
        let dir = std::env::temp_dir().join(format!("slate-recovery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::at(dir.clone());
        let mut p = Paragraph::default();
        p.push("Unsaved ", &RunStyle::default());
        p.push(
            "words",
            &RunStyle {
                bold: Some(true),
                ..RunStyle::default()
            },
        );
        p.own = ParaOverrides {
            space_before: Some(120),
            line: Some(LineSpacing::Exact(300)),
            indent_first: Some(-360),
            borders: BorderOverrides {
                bottom: Some(Border {
                    style: BorderStyle::Double,
                    width: 4,
                    space: 1,
                    color: Some([0xC0, 0, 0]),
                }),
                top: Some(Border::default()),
                ..BorderOverrides::default()
            },
            shading: Some(Shading { fill: None }),
            ..ParaOverrides::default()
        };
        let blocks = vec![Block::Paragraph(p), Block::PageBreak];
        let header = |path: Option<&str>, saved_ms| Header {
            path: path.map(PathBuf::from),
            file: path.map(|_| (12, 34)),
            saved_ms,
        };
        let older = dir.join("999997-0.journal");
        write(&store, &older, &header(Some("/tmp/a.md"), 10), &blocks).unwrap();
        let newer = dir.join("999998-0.journal");
        write(&store, &newer, &header(None, 20), &[]).unwrap();
        store
            .write(&dir.join("999999-0.journal"), |f| f.write_all(b"BRICKSJ1"))
            .unwrap();

        let found = crashed(&store);
        let listed: Vec<_> = found
            .iter()
            .map(|c| (c.journal.clone(), c.header.clone()))
            .collect();
        assert_eq!(
            listed,
            [
                (newer, header(None, 20)),
                (older.clone(), header(Some("/tmp/a.md"), 10))
            ]
        );
        assert_eq!(read_blocks(&older).unwrap(), blocks);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A journal written before paragraphs had their own looks reads them as none.
    #[test]
    fn journals_without_paragraph_looks_read() {
        let old = r#"[{"Paragraph":{"style":"Normal","align":"Start","indent":0,"runs":[]}}]"#;
        let blocks: Vec<Block> = serde_json::from_str(old).unwrap();
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("a paragraph");
        };
        assert!(p.own.is_empty() && p.tabs.is_empty());
        // And one without them is written as before.
        assert_eq!(serde_json::to_string(&blocks).unwrap(), old);
    }
}
