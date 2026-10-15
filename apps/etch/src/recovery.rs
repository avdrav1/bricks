//! Crash recovery (ETCH-25). A window with unsaved changes keeps a journal of its image,
//! written [`WRITE_AFTER`] after the first change not yet in it (so at most that often while
//! drawing goes on), off the UI thread, in `$XDG_STATE_HOME/etch/recovery` (the suite's
//! [`Store`]: one journal per window, an owner lock per process). The journal goes once
//! nothing is unsaved (saved, or every change undone) and when the window closes, so one
//! still there after its process ended means a crash or a kill (`saving::Saver` keeps it).
//!
//! A journal is [`MAGIC`], a binary [`Header`] (when it was written, the image's size, the
//! file's size and modification time when it was read or last saved, its path), then the
//! image as a PNG (lossless, alpha and all, and quick to write with the encoder's fast
//! setting). After a crash, `etch` started without files opens each crashed image in its
//! own window, and opening such a file any other way does too; the window asks "Restore
//! unsaved edits?". Restoring puts the journal's pixels in place of the file's (or of a
//! new Untitled image of their size) as one undoable step, so the window shows them
//! unsaved ("• ") and the file stays as it was until saved. If the file changed after the
//! crash, or can't be opened, the pixels aren't put onto it: they're kept for later or
//! discarded.

use crate::canvas::Canvas;
use gtk::glib;
use image_io::Format;
use image_model::Image;
use std::io::{self, BufReader, Read, Write};
// DEC-11 SPIKE EXPERIMENT: was use std::os::unix::ffi::{OsStrExt, OsStringExt} (unix-only)
use std::path::{Path, PathBuf};
use std::time::Duration;
pub use suite_file::recovery::{identity, now_ms, Store};

/// How long the first change not in the journal waits before the journal is written.
pub const WRITE_AFTER: Duration = Duration::from_secs(5);

/// What a journal file starts with.
const MAGIC: &[u8] = b"ETCHJ1\n";
/// The longest path a header holds: more is a damaged journal.
const MAX_PATH: u32 = 64 * 1024;

/// What a journal says about its image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// The image's file; `None` for an image not saved yet.
    pub path: Option<PathBuf>,
    /// The file's size and modification time when it was read or last saved.
    pub file: Option<(u64, u64)>,
    /// When the journal was written, in milliseconds since the Unix epoch.
    pub saved_ms: u64,
    /// The journaled image's width and height.
    pub size: (u32, u32),
}

/// A journal left by an Etch process that is gone.
#[derive(Debug, Clone)]
pub struct Crashed {
    pub journal: PathBuf,
    pub header: Header,
}

/// The user's recovery directory: `$XDG_STATE_HOME/etch/recovery` (`~/.local/state/…`).
pub fn store() -> Store {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir);
    Store::at(state.join("etch").join("recovery"))
}

/// Write `image` and `header` as the journal `journal`, atomically; the journal's size in
/// bytes.
pub fn write(store: &Store, journal: &Path, header: &Header, image: &Image) -> io::Result<u64> {
    let png = image_io::encode(image, Format::Png).map_err(io::Error::other)?;
    let mut head = Vec::new();
    write_header(&mut head, header)?;
    store.write(journal, |f| {
        f.write_all(&head)?;
        f.write_all(&png)
    })?;
    Ok((head.len() + png.len()) as u64)
}

fn write_header(w: &mut impl Write, h: &Header) -> io::Result<()> {
    w.write_all(MAGIC)?;
    w.write_all(&h.saved_ms.to_le_bytes())?;
    w.write_all(&h.size.0.to_le_bytes())?;
    w.write_all(&h.size.1.to_le_bytes())?;
    match h.file {
        Some((len, mtime)) => {
            w.write_all(&[1])?;
            w.write_all(&len.to_le_bytes())?;
            w.write_all(&mtime.to_le_bytes())?;
        }
        None => w.write_all(&[0])?,
    }
    match &h.path {
        Some(p) => {
            let bytes = p.as_os_str().as_encoded_bytes();
            let len = u32::try_from(bytes.len())
                .ok()
                .filter(|&n| n <= MAX_PATH)
                .ok_or_else(|| io::Error::other("the path is too long"))?;
            w.write_all(&[1])?;
            w.write_all(&len.to_le_bytes())?;
            w.write_all(bytes)?;
        }
        None => w.write_all(&[0])?,
    }
    Ok(())
}

fn read_header_from(r: &mut impl Read) -> io::Result<Header> {
    let damaged = || io::Error::new(io::ErrorKind::InvalidData, "not an Etch recovery journal");
    let mut magic = [0; MAGIC.len()];
    r.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(damaged());
    }
    fn u64_of(r: &mut impl Read) -> io::Result<u64> {
        let mut b = [0; 8];
        r.read_exact(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }
    fn u32_of(r: &mut impl Read) -> io::Result<u32> {
        let mut b = [0; 4];
        r.read_exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }
    fn flag(r: &mut impl Read) -> io::Result<Option<bool>> {
        let mut b = [0; 1];
        r.read_exact(&mut b)?;
        Ok(match b[0] {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        })
    }
    let saved_ms = u64_of(r)?;
    let size = (u32_of(r)?, u32_of(r)?);
    let file = match flag(r)?.ok_or_else(damaged)? {
        true => Some((u64_of(r)?, u64_of(r)?)),
        false => None,
    };
    let path = match flag(r)?.ok_or_else(damaged)? {
        true => {
            let len = u32_of(r)?;
            if len > MAX_PATH {
                return Err(damaged());
            }
            let mut bytes = vec![0; len as usize];
            r.read_exact(&mut bytes)?;
            Some(PathBuf::from(String::from_utf8_lossy(&bytes).into_owned()))
        }
        false => None,
    };
    Ok(Header {
        path,
        file,
        saved_ms,
        size,
    })
}

/// The header of the journal at `path`, reading only its first bytes.
fn read_header(path: &Path) -> Option<Header> {
    let mut r = BufReader::new(std::fs::File::open(path).ok()?);
    read_header_from(&mut r).ok()
}

/// The image of the journal at `path`, in words for an alert when it can't be read.
pub fn read_image(path: &Path) -> Result<Image, String> {
    let mut r = BufReader::new(std::fs::File::open(path).map_err(|e| e.to_string())?);
    let header = read_header_from(&mut r).map_err(|e| e.to_string())?;
    let mut png = Vec::new();
    r.read_to_end(&mut png).map_err(|e| e.to_string())?;
    let image = image_io::decode(&png).map_err(|e| format!("The journal is damaged. {e}"))?;
    if (image.width(), image.height()) != header.size {
        return Err("The journal is damaged: its image isn't the size it says.".to_owned());
    }
    Ok(image)
}

/// The journals of Etch processes that are gone, newest first.
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
    crashed(&store()).into_iter().find(|c| {
        c.header
            .path
            .as_deref()
            .map(crate::window::canonical)
            .as_deref()
            == Some(path)
    })
}

/// A journal to offer back in a window being opened, read with its file.
pub struct Recovered {
    crashed: Crashed,
    /// The journal's image; `None` when the file changed after the crash, so it can't go
    /// onto it.
    image: Option<Result<Image, String>>,
}

impl Recovered {
    /// Read `crashed`'s image for a file just read whose size and modification time are
    /// `identity` (an Untitled image has no file); off the UI thread. `None` once the
    /// journal is gone (restored or discarded meanwhile, by another Etch).
    pub fn read(crashed: Crashed, identity: Option<(u64, u64)>) -> Option<Recovered> {
        if !crashed.journal.exists() {
            return None;
        }
        let image = if crashed.header.path.is_some() && crashed.header.file != identity {
            None
        } else {
            Some(read_image(&crashed.journal))
        };
        Some(Recovered { crashed, image })
    }

    /// Offer the pixels back in `window`, whose canvas shows the file as it is now (or a
    /// new Untitled image): ask first; say so when the file changed after the crash.
    pub fn offer(self, window: &gtk::ApplicationWindow, canvas: &Canvas) {
        let (window, canvas) = (window.clone(), canvas.clone());
        glib::spawn_future_local(async move {
            let Recovered { crashed, image } = self;
            let h = &crashed.header;
            let image = match image {
                None => {
                    changed(&window, &crashed).await;
                    return;
                }
                Some(Err(why)) => {
                    tracing::warn!("cannot restore unsaved changes: {why}");
                    keep_or_discard(&window, &crashed, "Cannot restore unsaved changes", &why)
                        .await;
                    return;
                }
                Some(Ok(image)) => image,
            };
            let dialog = gtk::AlertDialog::builder()
                .message("Restore unsaved edits?")
                .detail(format!(
                    "Etch closed unexpectedly at {} while {} had unsaved edits.",
                    when(h.saved_ms),
                    name(h)
                ))
                .buttons(["Discard", "Restore"])
                .default_button(1)
                .modal(true)
                .build();
            match dialog.choose_future(Some(&window)).await {
                Ok(1) => {
                    let (w, h) = (image.width(), image.height());
                    canvas.restore(image);
                    store().remove(&crashed.journal);
                    tracing::info!("restored {w}×{h} from {}", crashed.journal.display());
                }
                Ok(0) => store().remove(&crashed.journal),
                _ => {} // dismissed: offered again next time
            }
        });
    }
}

/// The journal's file changed after the crash, or can't be opened: its pixels can't go
/// onto it. Keep the journal for later, or discard it.
pub async fn changed(window: &gtk::ApplicationWindow, crashed: &Crashed) {
    let h = &crashed.header;
    let detail = format!(
        "Etch closed unexpectedly at {} with unsaved edits, but the file has changed \
         since, so they can't be put back onto it.",
        when(h.saved_ms)
    );
    let message = format!("{} changed after the crash", name(h));
    keep_or_discard(window, crashed, &message, &detail).await;
}

async fn keep_or_discard(
    window: &gtk::ApplicationWindow,
    crashed: &Crashed,
    message: &str,
    detail: &str,
) {
    let dialog = gtk::AlertDialog::builder()
        .message(message)
        .detail(detail)
        .buttons(["Keep for Later", "Discard"])
        .cancel_button(0)
        .default_button(0)
        .modal(true)
        .build();
    if let Ok(1) = dialog.choose_future(Some(window)).await {
        store().remove(&crashed.journal);
    }
}

/// `etch` without files after a crash: each crashed image opens in its own window, which
/// asks whether to restore it. Whether there were any.
pub fn reopen(app: &gtk::Application) -> bool {
    let crashed = crashed(&store());
    for c in crashed.iter().rev() {
        crate::window::reopen(app, c.clone());
    }
    !crashed.is_empty()
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

/// The image's name in a sentence: “photo.png”, or an untitled image.
fn name(h: &Header) -> String {
    h.path.as_deref().map_or_else(
        || "an untitled image".to_owned(),
        |p| format!("“{}”", suite_file::document_name(p)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image_model::Color;
    use std::time::Instant;

    /// A store of its own in the temp directory.
    fn scratch(name: &str) -> (PathBuf, Store) {
        let dir = std::env::temp_dir().join(format!("etch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (dir.clone(), Store::at(dir))
    }

    /// Opaque, see-through, and transparent pixels hiding a colour.
    fn pattern(w: u32, h: u32) -> Image {
        let data = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                let (r, g) = ((x * 13 % 256) as u8, (y * 7 % 256) as u8);
                match (x + y) % 3 {
                    0 => Color::rgb(r, g, 200),
                    1 => Color::rgba(r, g, 100, 128),
                    _ => Color::rgba(r, g, 50, 0),
                }
                .to_bytes()
            })
            .collect();
        Image::from_rgba(w, h, data).unwrap()
    }

    /// A journal reads back its header and its pixels exactly (transparent ones' colours
    /// too); listing reads only the headers, newest first; a file that isn't an Etch
    /// journal is left out.
    #[test]
    fn journals_round_trip_and_list_newest_first() {
        let (dir, store) = scratch("recovery");
        let image = pattern(70, 45);
        let older_header = Header {
            path: Some(PathBuf::from(std::ffi::OsString::from_vec(
                b"/tmp/caf\xe9 \"one\"\n.png".to_vec(),
            ))),
            file: Some((12, 34)),
            saved_ms: 10,
            size: (70, 45),
        };
        let older = dir.join("999997-0.journal");
        let bytes = write(&store, &older, &older_header, &image).unwrap();
        assert_eq!(bytes, std::fs::metadata(&older).unwrap().len());
        let newer_header = Header {
            path: None,
            file: None,
            saved_ms: 20,
            size: (3, 2),
        };
        let newer = dir.join("999998-0.journal");
        write(&store, &newer, &newer_header, &pattern(3, 2)).unwrap();
        store
            .write(&dir.join("999999-0.journal"), |f| {
                f.write_all(b"SLATEJ1 {}")
            })
            .unwrap();

        let listed: Vec<_> = crashed(&store)
            .into_iter()
            .map(|c| (c.journal, c.header))
            .collect();
        assert_eq!(
            listed,
            [(newer, newer_header), (older.clone(), older_header)]
        );
        assert_eq!(read_image(&older).unwrap(), image);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The journal's pixels go onto the file only while it has the size and time it had
    /// when the journal was written; an Untitled image's always do.
    #[test]
    fn a_journal_is_offered_only_onto_its_file_as_it_was() {
        let (dir, store) = scratch("recovered");
        let image = pattern(5, 4);
        let header = |path: Option<&str>| Header {
            path: path.map(PathBuf::from),
            file: path.map(|_| (100, 200)),
            saved_ms: 1,
            size: (5, 4),
        };
        let journal = dir.join("999996-0.journal");
        let crashed = |h: Header| Crashed {
            journal: journal.clone(),
            header: h,
        };
        write(&store, &journal, &header(Some("/tmp/a.png")), &image).unwrap();
        let same = Recovered::read(crashed(header(Some("/tmp/a.png"))), Some((100, 200)));
        assert_eq!(same.unwrap().image, Some(Ok(image.clone())));
        let changed = Recovered::read(crashed(header(Some("/tmp/a.png"))), Some((100, 201)));
        assert_eq!(changed.unwrap().image, None);
        let gone = Recovered::read(crashed(header(Some("/tmp/a.png"))), None);
        assert_eq!(gone.unwrap().image, None);
        let untitled = Recovered::read(crashed(header(None)), None);
        assert_eq!(untitled.unwrap().image, Some(Ok(image)));
        // A cut-short journal says why.
        let bytes = std::fs::read(&journal).unwrap();
        std::fs::write(&journal, &bytes[..bytes.len() / 2]).unwrap();
        let cut = Recovered::read(crashed(header(None)), None).unwrap().image;
        assert!(matches!(cut, Some(Err(why)) if why.starts_with("The journal is damaged")));
        store.remove(&journal);
        assert!(Recovered::read(crashed(header(None)), None).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An 8K photo-like journal: the copy made on the UI thread, then the write (PNG at
    /// the encoder's fast setting, synced) and read back off it. Timing is asserted only
    /// in optimized builds:
    /// `cargo test --release -p etch recovery -- --ignored --nocapture`
    #[test]
    #[ignore = "writes a large journal; run with --ignored"]
    fn an_8k_journal_writes_within_write_after() {
        let (w, h) = (7680u32, 4320u32);
        let mut data = Vec::with_capacity(w as usize * h as usize * 4);
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
                let sun = 1.0 - (((fx - 0.7).powi(2) + (fy - 0.3).powi(2)).sqrt() * 6.0).min(1.0);
                let hill = fy > 0.6 + 0.08 * (fx * 9.0).sin() + 0.04 * (fx * 23.0).cos();
                let (r, g, b) = if hill {
                    (60.0 + 80.0 * fy, 110.0 + 60.0 * (1.0 - fy), 40.0)
                } else {
                    (90.0 + 120.0 * fy, 140.0 + 80.0 * fy, 230.0 - 40.0 * fy)
                };
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let noise = (seed % 9) as f32 - 4.0;
                for c in [r + 120.0 * sun, g + 100.0 * sun, b - 60.0 * sun] {
                    data.push((c + noise).clamp(0.0, 255.0) as u8);
                }
                data.push(255);
            }
        }
        let image = Image::from_rgba(w, h, data).unwrap();
        let (dir, store) = scratch("journal-8k");
        let journal = dir.join("999995-0.journal");
        let header = Header {
            path: Some(PathBuf::from("/tmp/photo.png")),
            file: Some((1, 2)),
            saved_ms: now_ms(),
            size: (w, h),
        };
        let mut runs = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let copy = image.clone();
            let copy_ms = t.elapsed().as_secs_f64() * 1e3;
            let t = Instant::now();
            let bytes = write(&store, &journal, &header, &copy).unwrap();
            let write_ms = t.elapsed().as_secs_f64() * 1e3;
            let t = Instant::now();
            assert!(read_image(&journal).unwrap() == image);
            let read_ms = t.elapsed().as_secs_f64() * 1e3;
            println!(
                "8K journal: copy {copy_ms:.1} ms, write {write_ms:.0} ms, {:.1} MB \
                 ({:.0}% of the pixels), read {read_ms:.0} ms",
                bytes as f64 / 1e6,
                bytes as f64 * 100.0 / (w as f64 * h as f64 * 4.0)
            );
            runs.push((copy_ms, write_ms));
        }
        let _ = std::fs::remove_dir_all(&dir);
        runs.sort_by(|a, b| a.1.total_cmp(&b.1));
        let (copy_ms, write_ms) = runs[1];
        println!("median: copy {copy_ms:.1} ms, write {write_ms:.0} ms");
        if !cfg!(debug_assertions) {
            assert!(
                write_ms < WRITE_AFTER.as_secs_f64() * 1e3,
                "an 8K journal took {write_ms:.0} ms to write"
            );
        }
    }
}
