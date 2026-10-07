//! Text encodings (ENG-6, SP-4): UTF-8 (with or without BOM), UTF-16 LE/BE, and
//! Windows-1252 (what files called "Latin-1" almost always are; it decodes every ISO-8859-1
//! letter the same way and adds €, “ ” and friends in 0x80–0x9F).
//!
//! UTF-8 files are read straight from the mapping. Other encodings are decoded once at open
//! into a UTF-8 copy in the cache directory, which is mapped and then unlinked (so nothing
//! is left behind, even after a crash); the rest of the engine only ever sees UTF-8. On save,
//! [`EncodeWriter`] turns the UTF-8 back into the original encoding and BOM. Decoding and
//! re-encoding are lossless for well-formed input, so untouched rows still come out byte for
//! byte (invariant 1). Malformed UTF-16 (lone surrogates) is shown and saved as U+FFFD.

use crate::Source;
use encoding_rs::{EncoderResult, UTF_16BE, UTF_16LE, WINDOWS_1252};
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Charset {
    Utf8,
    Utf16Le,
    Utf16Be,
    Windows1252,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Encoding {
    pub charset: Charset,
    /// The file starts with a byte-order mark (kept on save).
    pub bom: bool,
}

impl Encoding {
    pub const UTF8: Encoding = Encoding {
        charset: Charset::Utf8,
        bom: false,
    };

    pub fn name(&self) -> &'static str {
        match (self.charset, self.bom) {
            (Charset::Utf8, false) => "UTF-8",
            (Charset::Utf8, true) => "UTF-8 with BOM",
            (Charset::Utf16Le, _) => "UTF-16 LE",
            (Charset::Utf16Be, _) => "UTF-16 BE",
            (Charset::Windows1252, _) => "Windows-1252",
        }
    }
}

/// Bytes read from the start of a file to detect its encoding.
const DETECT_BYTES: usize = 64 * 1024;

/// Guess a file's encoding from its first bytes: a BOM if there is one; else UTF-16 if every
/// other byte is mostly zero; else UTF-8 if the sample is valid UTF-8; else Windows-1252.
pub fn detect_encoding(sample: &[u8]) -> Encoding {
    let with_bom = |charset| Encoding { charset, bom: true };
    if sample.starts_with(b"\xEF\xBB\xBF") {
        return with_bom(Charset::Utf8);
    }
    if sample.starts_with(b"\xFF\xFE") {
        return with_bom(Charset::Utf16Le);
    }
    if sample.starts_with(b"\xFE\xFF") {
        return with_bom(Charset::Utf16Be);
    }
    let pairs = sample.len().min(4096) / 2;
    if pairs >= 2 {
        let zeros = |offset: usize| (0..pairs).filter(|i| sample[2 * i + offset] == 0).count();
        let (even, odd) = (zeros(0), zeros(1));
        if odd * 10 >= pairs * 4 && even * 20 < pairs {
            return Encoding {
                charset: Charset::Utf16Le,
                bom: false,
            };
        }
        if even * 10 >= pairs * 4 && odd * 20 < pairs {
            return Encoding {
                charset: Charset::Utf16Be,
                bom: false,
            };
        }
    }
    let valid = match std::str::from_utf8(sample) {
        Ok(_) => true,
        // A multi-byte character cut by the end of the sample is fine.
        Err(e) => e.error_len().is_none() && sample.len() - e.valid_up_to() < 4,
    };
    if valid {
        Encoding::UTF8
    } else {
        Encoding {
            charset: Charset::Windows1252,
            bom: false,
        }
    }
}

/// Open `path` as UTF-8 text: detect its encoding and, if it is not UTF-8, map a decoded copy.
pub fn open_text(path: &Path) -> io::Result<(Source, Encoding)> {
    let mut head = Vec::with_capacity(DETECT_BYTES);
    File::open(path)?
        .take(DETECT_BYTES as u64)
        .read_to_end(&mut head)?;
    let encoding = detect_encoding(&head);
    Ok((open_text_as(path, encoding)?, encoding))
}

/// Open `path` as UTF-8 text, decoding from `encoding`.
pub fn open_text_as(path: &Path, encoding: Encoding) -> io::Result<Source> {
    if encoding.charset == Charset::Utf8 {
        return Source::open(path);
    }
    let copy = decoded_copy_path()?;
    let result = transcode(path, &copy, encoding).and_then(|()| Source::open(&copy));
    let _ = std::fs::remove_file(&copy); // the mapping stays valid after unlink
    result
}

fn decoded_copy_path() -> io::Result<PathBuf> {
    static N: AtomicU64 = AtomicU64::new(0);
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("bricks");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(format!(
        "decoded-{}-{}.utf8",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    )))
}

fn transcode(from: &Path, to: &Path, encoding: Encoding) -> io::Result<()> {
    let mut decoder = match encoding.charset {
        Charset::Utf16Le => UTF_16LE.new_decoder_with_bom_removal(),
        Charset::Utf16Be => UTF_16BE.new_decoder_with_bom_removal(),
        Charset::Windows1252 => WINDOWS_1252.new_decoder_without_bom_handling(),
        Charset::Utf8 => unreachable!("UTF-8 is mapped directly"),
    };
    let mut input = File::open(from)?;
    let mut output = io::BufWriter::with_capacity(1 << 20, File::create(to)?);
    let (mut buf, mut out) = (vec![0u8; 1 << 20], String::new());
    loop {
        let n = input.read(&mut buf)?;
        let last = n == 0;
        let mut src = &buf[..n];
        loop {
            out.clear();
            out.reserve(
                decoder
                    .max_utf8_buffer_length(src.len())
                    .unwrap_or(src.len() * 3 + 16),
            );
            let (result, read, _) = decoder.decode_to_string(src, &mut out, last);
            output.write_all(out.as_bytes())?;
            src = &src[read..];
            if result == encoding_rs::CoderResult::InputEmpty {
                break;
            }
        }
        if last {
            break;
        }
    }
    output.flush()
}

/// Writes UTF-8 into `inner` in `encoding`, starting with its BOM if it has one. Call
/// [`EncodeWriter::finish`] at the end. Text the encoding cannot hold is an error, never a
/// silent substitution.
pub struct EncodeWriter<'a> {
    inner: &'a mut dyn Write,
    encoding: Encoding,
    encoder: Option<encoding_rs::Encoder>,
    pending: Vec<u8>,
    out: Vec<u8>,
    started: bool,
}

/// Input taken per `write` call, so whole-file writes don't buffer the whole file.
const WRITE_CHUNK: usize = 1 << 20;

impl<'a> EncodeWriter<'a> {
    pub fn new(inner: &'a mut dyn Write, encoding: Encoding) -> Self {
        let encoder =
            (encoding.charset == Charset::Windows1252).then(|| WINDOWS_1252.new_encoder());
        Self {
            inner,
            encoding,
            encoder,
            pending: Vec::new(),
            out: Vec::new(),
            started: false,
        }
    }

    /// Encode everything still buffered and flush.
    pub fn finish(mut self) -> io::Result<()> {
        self.drain(true)?;
        self.inner.flush()
    }

    fn drain(&mut self, last: bool) -> io::Result<()> {
        self.out.clear();
        if !self.started {
            self.started = true;
            if self.encoding.bom {
                self.out.extend_from_slice(match self.encoding.charset {
                    Charset::Utf8 => b"\xEF\xBB\xBF",
                    Charset::Utf16Le => b"\xFF\xFE",
                    Charset::Utf16Be => b"\xFE\xFF",
                    Charset::Windows1252 => b"",
                });
            }
        }
        let valid = match std::str::from_utf8(&self.pending) {
            Ok(s) => s.len(),
            Err(e) if e.error_len().is_none() && !last => e.valid_up_to(),
            Err(_) => {
                return Err(io::Error::other(
                    "internal error: invalid UTF-8 reached the encoder",
                ))
            }
        };
        let text = std::str::from_utf8(&self.pending[..valid]).expect("validated above");
        match self.encoding.charset {
            Charset::Utf8 => self.out.extend_from_slice(text.as_bytes()),
            Charset::Utf16Le => text
                .encode_utf16()
                .for_each(|u| self.out.extend_from_slice(&u.to_le_bytes())),
            Charset::Utf16Be => text
                .encode_utf16()
                .for_each(|u| self.out.extend_from_slice(&u.to_be_bytes())),
            Charset::Windows1252 => {
                let encoder = self.encoder.as_mut().expect("Windows-1252 has an encoder");
                let mut src = text;
                loop {
                    let need = encoder
                        .max_buffer_length_from_utf8_without_replacement(src.len())
                        .unwrap_or(src.len());
                    self.out.reserve(need);
                    let (result, read) = encoder.encode_from_utf8_to_vec_without_replacement(
                        src,
                        &mut self.out,
                        last,
                    );
                    src = &src[read..];
                    match result {
                        EncoderResult::InputEmpty => break,
                        EncoderResult::OutputFull => continue,
                        EncoderResult::Unmappable(c) => {
                            return Err(io::Error::other(format!(
                                "“{c}” cannot be saved in {}; this file's encoding has no such character",
                                self.encoding.name()
                            )));
                        }
                    }
                }
            }
        }
        self.inner.write_all(&self.out)?;
        self.pending.drain(..valid);
        Ok(())
    }
}

impl Write for EncodeWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = buf.len().min(WRITE_CHUNK);
        self.pending.extend_from_slice(&buf[..n]);
        self.drain(false)?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(charset: Charset, bom: bool) -> Encoding {
        Encoding { charset, bom }
    }

    #[test]
    fn detects_boms_utf16_utf8_and_windows_1252() {
        assert_eq!(
            detect_encoding(b"\xEF\xBB\xBFa,b\n"),
            enc(Charset::Utf8, true)
        );
        assert_eq!(
            detect_encoding(b"\xFF\xFEa\0,\0b\0"),
            enc(Charset::Utf16Le, true)
        );
        assert_eq!(
            detect_encoding(b"\xFE\xFF\0a\0,\0b"),
            enc(Charset::Utf16Be, true)
        );
        assert_eq!(
            detect_encoding(b"a\0,\0b\0\n\x001\0,\x002\0\n\0"),
            enc(Charset::Utf16Le, false)
        );
        assert_eq!(
            detect_encoding(b"\0a\0,\0b\0\n\x001\0,\x002\0\n"),
            enc(Charset::Utf16Be, false)
        );
        assert_eq!(detect_encoding("name\nJosé\n".as_bytes()), Encoding::UTF8);
        assert_eq!(detect_encoding(b"plain ascii\n"), Encoding::UTF8);
        assert_eq!(
            detect_encoding(b"name\nJos\xe9\nM\xfcller\n"),
            enc(Charset::Windows1252, false)
        );
        // A UTF-8 character cut off by the end of the sample still means UTF-8.
        assert_eq!(detect_encoding(&"Zoë".as_bytes()[..3]), Encoding::UTF8);
        assert_eq!(detect_encoding(b""), Encoding::UTF8);
    }

    fn encode_all(text: &str, encoding: Encoding, chunk: usize) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut w = EncodeWriter::new(&mut out, encoding);
        for piece in text.as_bytes().chunks(chunk) {
            w.write_all(piece)?;
        }
        w.finish()?;
        Ok(out)
    }

    #[test]
    fn encodes_utf8_split_mid_character() {
        let text = "Zoë,€5\n東京\n";
        for chunk in [1, 2, 3, 1000] {
            let le = encode_all(text, enc(Charset::Utf16Le, true), chunk).unwrap();
            assert_eq!(&le[..2], b"\xFF\xFE");
            assert_eq!(UTF_16LE.decode_with_bom_removal(&le[2..]).0, text);
            let be = encode_all(text, enc(Charset::Utf16Be, false), chunk).unwrap();
            assert_eq!(UTF_16BE.decode_without_bom_handling(&be).0, text);
            let cp = encode_all("Zoë,€5\n", enc(Charset::Windows1252, false), chunk).unwrap();
            assert_eq!(cp, b"Zo\xeb,\x805\n");
        }
    }

    #[test]
    fn unmappable_text_is_an_error_naming_the_character() {
        let err = encode_all("ok\n東\n", enc(Charset::Windows1252, false), 1000).unwrap_err();
        assert!(err.to_string().contains('東'), "{err}");
    }
}
