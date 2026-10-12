//! What kind of file a path holds, by its first bytes: Brick opens text (CSV) and XLSX,
//! and refuses other spreadsheet formats with a message saying what to do.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// Compound File Binary header: old `.xls` files, and encrypted OOXML files.
const CFB: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
const ZIP: [u8; 4] = *b"PK\x03\x04";
const ODS_MIME: &[u8] = b"application/vnd.oasis.opendocument.spreadsheet";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An Office Open XML workbook (.xlsx, .xlsm).
    Xlsx,
    /// An Excel 97-2003 binary workbook.
    Xls,
    /// A password-protected workbook (a CFB container not named `.xls`).
    Encrypted,
    /// An OpenDocument spreadsheet.
    Ods,
    /// Anything else: opened as delimited text.
    Text,
}

/// The kind of file at `path`.
pub fn kind(path: &Path) -> io::Result<Kind> {
    let mut file = File::open(path)?;
    let mut head = [0u8; 8];
    let n = read_up_to(&mut file, &mut head)?;
    if n == 8 && head == CFB {
        let xls = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xls"));
        return Ok(if xls { Kind::Xls } else { Kind::Encrypted });
    }
    if n < 4 || head[..4] != ZIP {
        return Ok(Kind::Text);
    }
    file.seek(SeekFrom::Start(0))?;
    // A file that starts like a zip but isn't one is left to the text path.
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return Ok(Kind::Text);
    };
    if zip.index_for_name("xl/workbook.xml").is_some() {
        return Ok(Kind::Xlsx);
    }
    if let Ok(mut mimetype) = zip.by_name("mimetype") {
        let mut mime = [0u8; ODS_MIME.len()];
        if read_up_to(&mut mimetype, &mut mime)? == mime.len() && mime == ODS_MIME {
            return Ok(Kind::Ods);
        }
    }
    Ok(Kind::Text)
}

/// Why Brick won't open a file of this kind, or `None` if it will.
pub fn refusal(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::Xls => Some(
            "Old Excel .xls files can't be opened. Save it as .xlsx in Excel or LibreOffice first.",
        ),
        Kind::Encrypted => Some(
            "This workbook is password-protected. Remove the password in Excel or LibreOffice first.",
        ),
        Kind::Ods => {
            Some("OpenDocument spreadsheets (.ods) can't be opened yet. Save it as .xlsx first.")
        }
        Kind::Xlsx | Kind::Text => None,
    }
}

fn read_up_to(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}
