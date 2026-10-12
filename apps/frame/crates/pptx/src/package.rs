//! The OPC package: parts by name, each part's relationships, and parsed parts cached so
//! layouts, masters, and themes shared by many slides are read once.

use crate::xml::{self, Element};
use crate::Error;
use std::collections::HashMap;
use std::io::{Read, Seek};
use std::rc::Rc;
use std::sync::Arc;
use zip::ZipArchive;

pub(crate) trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

pub(crate) struct Package {
    zip: ZipArchive<Box<dyn ReadSeek>>,
    parts: HashMap<String, Rc<Part>>,
    media: HashMap<String, Arc<[u8]>>,
}

/// A parsed XML part, the bytes it was parsed from (its elements' spans index them), and
/// its relationships.
pub(crate) struct Part {
    pub root: Element,
    pub source: Vec<u8>,
    pub rels: Rels,
}

#[derive(Default)]
pub(crate) struct Rels(Vec<Rel>);

pub(crate) struct Rel {
    pub id: String,
    /// The relationship type's last segment: `slide`, `slideLayout`, `image`, ...
    pub kind: String,
    /// The target part's name in the package (resolved, no leading `/`), or the URL of an
    /// external target.
    pub target: String,
    pub external: bool,
}

impl Rels {
    pub fn get(&self, id: &str) -> Option<&Rel> {
        self.0.iter().find(|r| r.id == id)
    }

    /// The first internal relationship of type `kind`.
    pub fn of_kind(&self, kind: &str) -> Option<&Rel> {
        self.0.iter().find(|r| r.kind == kind && !r.external)
    }
}

impl Package {
    pub fn open(reader: Box<dyn ReadSeek>) -> Result<Self, Error> {
        let zip = ZipArchive::new(reader).map_err(|e| Error::NotPackage(e.to_string()))?;
        Ok(Package {
            zip,
            parts: HashMap::new(),
            media: HashMap::new(),
        })
    }

    pub fn has(&self, name: &str) -> bool {
        self.zip.index_for_name(name).is_some()
    }

    /// A media part's bytes (cached: slides share pictures).
    pub fn media(&mut self, name: &str) -> Result<Arc<[u8]>, Error> {
        if let Some(b) = self.media.get(name) {
            return Ok(b.clone());
        }
        let bytes: Arc<[u8]> = self.bytes(name)?.into();
        self.media.insert(name.to_owned(), bytes.clone());
        Ok(bytes)
    }

    /// A part's bytes.
    pub fn bytes(&mut self, name: &str) -> Result<Vec<u8>, Error> {
        let mut file = match self.zip.by_name(name) {
            Ok(f) => f,
            Err(zip::result::ZipError::FileNotFound) => {
                return Err(Error::Missing(name.to_owned()))
            }
            Err(e) => return Err(Error::damaged(name, e.to_string())),
        };
        let mut out = Vec::with_capacity(file.size().min(1 << 30) as usize);
        file.read_to_end(&mut out)
            .map_err(|e| Error::damaged(name, e.to_string()))?;
        Ok(out)
    }

    /// A part parsed, with its relationships (cached).
    pub fn part(&mut self, name: &str) -> Result<Rc<Part>, Error> {
        if let Some(p) = self.parts.get(name) {
            return Ok(p.clone());
        }
        let bytes = self.bytes(name)?;
        let root = xml::parse(&bytes).map_err(|e| Error::damaged(name, e))?;
        let rels = self.rels(name)?;
        let part = Rc::new(Part {
            root,
            source: bytes,
            rels,
        });
        self.parts.insert(name.to_owned(), part.clone());
        Ok(part)
    }

    /// The relationships of `part` (`""` for the package's own), empty when it has none.
    pub fn rels(&mut self, part: &str) -> Result<Rels, Error> {
        let (dir, file) = match part.rfind('/') {
            Some(i) => (&part[..i], &part[i + 1..]),
            None => ("", part),
        };
        let name = if dir.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{dir}/_rels/{file}.rels")
        };
        if !self.has(&name) {
            return Ok(Rels::default());
        }
        let bytes = self.bytes(&name)?;
        let root = xml::parse(&bytes).map_err(|e| Error::damaged(&name, e))?;
        let mut rels = Vec::new();
        for r in root.all("Relationship") {
            let (Some(id), Some(kind), Some(target)) =
                (r.attr("Id"), r.attr("Type"), r.attr("Target"))
            else {
                continue;
            };
            let external = r.attr("TargetMode") == Some("External");
            rels.push(Rel {
                id: id.to_owned(),
                kind: kind.rsplit('/').next().unwrap_or(kind).to_owned(),
                target: if external {
                    target.to_owned()
                } else {
                    resolve(dir, target)
                },
                external,
            });
        }
        Ok(Rels(rels))
    }
}

/// `target` relative to the directory `dir`, as a part name: `../media/a.png` from
/// `ppt/slides` is `ppt/media/a.png`; an absolute `/ppt/x.xml` is `ppt/x.xml`.
pub(crate) fn resolve(dir: &str, target: &str) -> String {
    let target = target.replace('\\', "/");
    let mut parts: Vec<&str> = if target.starts_with('/') || dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').collect()
    };
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn targets_resolve_against_the_part_directory() {
        assert_eq!(resolve("ppt/slides", "../media/a.png"), "ppt/media/a.png");
        assert_eq!(resolve("ppt", "slides/slide1.xml"), "ppt/slides/slide1.xml");
        assert_eq!(resolve("ppt/slides", "/ppt/x.xml"), "ppt/x.xml");
        assert_eq!(resolve("", "ppt/presentation.xml"), "ppt/presentation.xml");
    }
}
