//! What kind of file a path holds, so formats Frame doesn't open are refused with what to
//! do instead (spec: `.ppt`, `.odp`, and `.key` say "Save it as .pptx first").

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A PowerPoint Open XML package (`.pptx`, `.pptm`).
    Pptx,
    /// Old binary PowerPoint (`.ppt`).
    Ppt,
    /// A password-protected Office file (a compound file that isn't `.ppt`).
    Encrypted,
    /// OpenDocument presentation (`.odp`).
    Odp,
    /// Apple Keynote (`.key`).
    Keynote,
    /// Anything else.
    Other,
}

const CFB: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// The kind of file at `path`, from its first bytes and, for ZIP packages, their parts.
pub fn kind(path: &Path) -> io::Result<Kind> {
    let mut head = [0u8; 8];
    let mut file = File::open(path)?;
    let n = read_up_to(&mut file, &mut head)?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if n == 8 && head == CFB {
        return Ok(if ext == "ppt" || ext == "pps" || ext == "pot" {
            Kind::Ppt
        } else {
            Kind::Encrypted
        });
    }
    if n >= 4 && head[..4] == *b"PK\x03\x04" {
        let Ok(mut zip) = zip::ZipArchive::new(File::open(path)?) else {
            return Ok(Kind::Other);
        };
        if zip.index_for_name("ppt/presentation.xml").is_some() {
            return Ok(Kind::Pptx);
        }
        if let Ok(mut m) = zip.by_name("mimetype") {
            let mut mime = String::new();
            let _ = m.by_ref().take(200).read_to_string(&mut mime);
            if mime.starts_with("application/vnd.oasis.opendocument.presentation") {
                return Ok(Kind::Odp);
            }
        }
        if zip.index_for_name("Index/Document.iwa").is_some()
            || zip.index_for_name("index.apxl").is_some()
            || ext == "key"
        {
            return Ok(Kind::Keynote);
        }
        // A package without ppt/presentation.xml at the usual name may still name it in
        // its relationships; let the reader decide.
        if zip.index_for_name("_rels/.rels").is_some() {
            return Ok(Kind::Pptx);
        }
        return Ok(Kind::Other);
    }
    Ok(if ext == "key" {
        Kind::Keynote
    } else {
        Kind::Other
    })
}

/// What to tell the user about a kind Frame won't open; `None` for [`Kind::Pptx`].
pub fn refusal(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::Pptx => None,
        Kind::Ppt => Some("Old PowerPoint .ppt files can't be opened. Save it as .pptx first."),
        Kind::Encrypted => Some(
            "This presentation is password-protected. Remove the password in PowerPoint or LibreOffice, and save it as .pptx first.",
        ),
        Kind::Odp => {
            Some("OpenDocument presentations (.odp) can't be opened. Save it as .pptx first.")
        }
        Kind::Keynote => Some("Keynote presentations can't be opened. Save it as .pptx first."),
        Kind::Other => Some("This isn't a PowerPoint presentation. Frame opens .pptx files."),
    }
}

fn read_up_to(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn temp(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("frame-detect-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn zip_with(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, body) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn formats_frame_refuses_say_so() {
        let mut cfb = CFB.to_vec();
        cfb.extend([0; 504]);
        assert_eq!(kind(&temp("old.ppt", &cfb)).unwrap(), Kind::Ppt);
        assert_eq!(kind(&temp("locked.pptx", &cfb)).unwrap(), Kind::Encrypted);
        let odp = zip_with(&[(
            "mimetype",
            "application/vnd.oasis.opendocument.presentation",
        )]);
        assert_eq!(kind(&temp("talk.odp", &odp)).unwrap(), Kind::Odp);
        let key = zip_with(&[("Index/Document.iwa", "")]);
        assert_eq!(kind(&temp("talk.key", &key)).unwrap(), Kind::Keynote);
        assert_eq!(kind(&temp("notes.txt", b"hello")).unwrap(), Kind::Other);
        for k in [Kind::Ppt, Kind::Odp, Kind::Keynote] {
            assert!(refusal(k).unwrap().ends_with("Save it as .pptx first."));
        }
        let pptx = zip_with(&[("ppt/presentation.xml", "<p:presentation/>")]);
        assert_eq!(kind(&temp("deck.pptx", &pptx)).unwrap(), Kind::Pptx);
        assert_eq!(refusal(Kind::Pptx), None);
    }
}
