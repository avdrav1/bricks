//! A Word document's embedded fonts and family fallbacks (SLATE-25, ADR 0009).
//!
//! Opening a document (off the UI thread) writes each face its font table embeds once to
//! a private directory, `$XDG_RUNTIME_DIR/slate/fonts` (else `$XDG_CACHE_HOME/slate/fonts`;
//! mode 0700), named by its bytes' hash so the same face is one file. On the UI thread the
//! page adds them to fontconfig as application fonts of the configuration Pango's font
//! map uses (`FcConfigAppFontAddFile`, what Pango 1.56's `pango_font_map_add_font_file`
//! does too), and tells the font map (`pango_fc_font_map_config_changed`): the fonts are
//! this process's, never installed for the user, and only fontconfig and FreeType read
//! them. They stay loaded while the process runs; a face already added isn't added again.
//!
//! Each font of the table then gets the family Pango is given for it ([`Families`]):
//! - embedded: the face's own family, where it differs from the table's name;
//! - installed (a family the font map lists), or not in the table: its name as it is, so
//!   fontconfig's metric aliases (Calibri as Carlito, SLATE-20) still apply;
//! - otherwise `<name>,<generic>`, the generic family by its class (`w:family`: roman is
//!   serif, swiss sans-serif, modern monospace, any other serif). Fontconfig puts a
//!   name's aliases right after it, so an alias still comes before the generic family.
//!
//! Markdown and plain text have no font table and keep their families.

use document_model::{FontTable, Format};
use gtk::glib;
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
// DEC-11 SPIKE EXPERIMENT: was use std::os::unix::fs::{DirBuilderExt, PermissionsExt} (unix-only)
use std::path::{Path, PathBuf};

/// An embedded face written to the private font directory.
#[derive(Debug, Clone)]
pub struct FontFile {
    /// The font table's name for its font.
    pub name: String,
    pub path: PathBuf,
}

/// The family list Pango is given for each of a document's fonts that needs one other
/// than its name.
#[derive(Debug, Default)]
pub struct Families(HashMap<String, String>);

impl Families {
    /// What Pango is given for font `name`.
    pub fn get<'a>(&'a self, name: &'a str) -> &'a str {
        self.0.get(name).map_or(name, String::as_str)
    }
}

/// Write each face `table` embeds to the private font directory, once per distinct face
/// (a file of the same bytes is reused). Faces that can't be written are left out, and
/// draw in fallbacks.
pub fn write(table: &FontTable) -> Vec<FontFile> {
    if !table.has_embedded() {
        return Vec::new();
    }
    let dir = match private_dir() {
        Ok(dir) => dir,
        Err(e) => {
            tracing::warn!("cannot make a directory for embedded fonts: {e}");
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for font in &table.fonts {
        for face in &font.faces {
            match write_face(&dir, &face.bytes) {
                Ok(path) => out.push(FontFile {
                    name: font.name.clone(),
                    path,
                }),
                Err(e) => tracing::warn!("cannot write {}'s embedded face: {e}", font.name),
            }
        }
    }
    out
}

/// The private font directory, made (mode 0700) if need be.
fn private_dir() -> std::io::Result<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.is_dir())
        .unwrap_or_else(glib::user_cache_dir);
    let dir = base.join("slate").join("fonts");
    // DEC-11 SPIKE EXPERIMENT: mode 0700 (DirBuilderExt, PermissionsExt) dropped
    std::fs::DirBuilder::new().recursive(true).create(&dir)?;
    Ok(dir)
}

/// Write face `bytes` into `dir` as `<hash>-<length>.ttf` (`.otf` for CFF outlines),
/// unless a file there already holds them; its path.
fn write_face(dir: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let ext = if bytes.starts_with(b"OTTO") {
        "otf"
    } else {
        "ttf"
    };
    let path = dir.join(format!("{:016x}-{}.{ext}", fnv1a(bytes), bytes.len()));
    if std::fs::read(&path).is_ok_and(|have| have == bytes) {
        return Ok(path);
    }
    // Another Slate process may be writing the same face: write aside, then rename.
    let temp = dir.join(format!(
        ".{}-{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("face"),
        std::process::id()
    ));
    let mut file = std::fs::File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&temp, &path)?;
    Ok(path)
}

/// FNV-1a, 64 bits: a hash that's the same in every process, for file names.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

thread_local! {
    /// The faces added to fontconfig, by path: the families fontconfig read from each, or
    /// `None` when it couldn't read it.
    static LOADED: RefCell<HashMap<PathBuf, Option<Vec<String>>>> = RefCell::default();
    /// The families the font map lists, keyed by [`key`]; dropped when fonts are added.
    static INSTALLED: RefCell<Option<HashSet<String>>> = const { RefCell::new(None) };
}

/// A family name as fontconfig compares them: ignoring case and blanks.
fn key(family: &str) -> String {
    family
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Add `files` (faces of `format`'s fonts) to `map`'s fontconfig configuration, and the
/// family Pango is given for each of the document's fonts. On the UI thread.
pub fn families(format: &Format, files: &[FontFile], map: Option<&pango::FontMap>) -> Families {
    let Some(table) = format.fonts().filter(|t| !t.fonts.is_empty()) else {
        return Families::default();
    };
    if let Some(map) = map {
        load(files, map);
    }
    let installed = INSTALLED.with_borrow_mut(|cached| {
        cached
            .get_or_insert_with(|| {
                map.map(|m| m.list_families())
                    .unwrap_or_default()
                    .iter()
                    .map(|f| key(&f.name()))
                    .collect()
            })
            .clone()
    });
    LOADED.with_borrow(|loaded| resolve(table, files, loaded, &installed))
}

/// The families of `table`'s fonts given the faces `loaded` (by path, as `files` name
/// them) and the `installed` families (as [`key`]s).
fn resolve(
    table: &FontTable,
    files: &[FontFile],
    loaded: &HashMap<PathBuf, Option<Vec<String>>>,
    installed: &HashSet<String>,
) -> Families {
    let mut out = HashMap::new();
    for font in &table.fonts {
        let embedded = files
            .iter()
            .filter(|f| f.name == font.name)
            .find_map(|f| loaded.get(&f.path)?.as_ref().filter(|fs| !fs.is_empty()));
        if let Some(own) = embedded {
            if !own.iter().any(|f| key(f) == key(&font.name)) {
                out.insert(font.name.clone(), own[0].clone());
            }
        } else if !installed.contains(&key(&font.name)) {
            let list = format!("{},{}", font.name, font.family.generic());
            out.insert(font.name.clone(), list);
        }
    }
    Families(out)
}

/// Add the faces of `files` not added yet to the fontconfig configuration of `map` (when
/// it's fontconfig's), and tell it its fonts changed.
fn load(files: &[FontFile], map: &pango::FontMap) {
    let fresh: Vec<&FontFile> = LOADED.with_borrow(|loaded| {
        files
            .iter()
            .filter(|f| !loaded.contains_key(&f.path))
            .collect()
    });
    if fresh.is_empty() {
        return;
    }
    let Some(fc) = fc::FcMap::of(map) else {
        tracing::warn!("the font map isn't fontconfig's: embedded fonts aren't shown");
        return;
    };
    for file in fresh {
        let families = fc.add(&file.path);
        if families.is_none() {
            tracing::warn!("fontconfig can't read {}'s embedded face", file.name);
        }
        LOADED.with_borrow_mut(|loaded| loaded.insert(file.path.clone(), families));
    }
    fc.changed();
    INSTALLED.with_borrow_mut(|cached| *cached = None);
}

/// Fontconfig and Pango's fontconfig font map, through their C interfaces (libfontconfig
/// and libpangoft2, which GTK already loads).
#[allow(unsafe_code)]
mod fc {
    use gtk::glib;
    use gtk::glib::translate::{from_glib, ToGlibPtr};
    use gtk::prelude::*;
    use std::ffi::{c_char, c_int, c_void, CStr, CString};
    // DEC-11 SPIKE EXPERIMENT: was use std::os::unix::ffi::OsStrExt (unix-only)
    use std::path::Path;

    #[repr(C)]
    struct FcFontSet {
        nfont: c_int,
        sfont: c_int,
        fonts: *mut *mut c_void,
    }

    const FC_SET_APPLICATION: c_int = 1;
    const FC_RESULT_MATCH: c_int = 0;

    #[link(name = "fontconfig")]
    extern "C" {
        fn FcConfigAppFontAddFile(config: *mut c_void, file: *const c_char) -> c_int;
        fn FcConfigGetFonts(config: *mut c_void, set: c_int) -> *mut FcFontSet;
        fn FcPatternGetString(
            pattern: *mut c_void,
            object: *const c_char,
            n: c_int,
            out: *mut *const c_char,
        ) -> c_int;
    }

    #[link(name = "pangoft2-1.0")]
    extern "C" {
        fn pango_fc_font_map_get_type() -> glib::ffi::GType;
        fn pango_fc_font_map_get_config(map: *mut pango::ffi::PangoFontMap) -> *mut c_void;
        fn pango_fc_font_map_config_changed(map: *mut pango::ffi::PangoFontMap);
    }

    /// A Pango font map that's fontconfig's.
    pub struct FcMap<'a>(&'a pango::FontMap);

    impl<'a> FcMap<'a> {
        pub fn of(map: &'a pango::FontMap) -> Option<Self> {
            // SAFETY: a type getter with no arguments.
            let fc: glib::Type = unsafe { from_glib(pango_fc_font_map_get_type()) };
            map.type_().is_a(fc).then_some(FcMap(map))
        }

        /// The configuration the map uses: null for fontconfig's current one.
        fn config(&self) -> *mut c_void {
            // SAFETY: the map is a PangoFcFontMap (checked in `of`); the configuration is
            // borrowed, not kept past this call's users.
            unsafe { pango_fc_font_map_get_config(self.0.to_glib_none().0) }
        }

        /// Add the font file at `path` to the configuration's application fonts; the
        /// families fontconfig reads from it, or `None` if it can't.
        pub fn add(&self, path: &Path) -> Option<Vec<String>> {
            let file = CString::new(path.as_os_str().as_encoded_bytes()).ok()?;
            let config = self.config();
            // SAFETY: a valid configuration (or null for the current one) and a
            // NUL-terminated path; fontconfig copies what it keeps.
            if unsafe { FcConfigAppFontAddFile(config, file.as_ptr()) } == 0 {
                return None;
            }
            let mut families = Vec::new();
            // SAFETY: the application set belongs to the configuration and lives while it
            // does; patterns and strings are read, not kept.
            unsafe {
                let set = FcConfigGetFonts(config, FC_SET_APPLICATION);
                if set.is_null() {
                    return None;
                }
                let set = &*set;
                for i in 0..usize::try_from(set.nfont).unwrap_or(0) {
                    let pattern = *set.fonts.add(i);
                    let mut s: *const c_char = std::ptr::null();
                    if FcPatternGetString(pattern, c"file".as_ptr(), 0, &mut s) != FC_RESULT_MATCH
                        || CStr::from_ptr(s) != file.as_c_str()
                    {
                        continue;
                    }
                    let mut n = 0;
                    while FcPatternGetString(pattern, c"family".as_ptr(), n, &mut s)
                        == FC_RESULT_MATCH
                    {
                        let family = CStr::from_ptr(s).to_string_lossy().into_owned();
                        if !families.contains(&family) {
                            families.push(family);
                        }
                        n += 1;
                    }
                }
            }
            Some(families)
        }

        /// Tell the map its configuration's fonts changed: it drops its cached fonts and
        /// lists, and its contexts lay out again.
        pub fn changed(&self) {
            // SAFETY: the map is a PangoFcFontMap (checked in `of`).
            unsafe { pango_fc_font_map_config_changed(self.0.to_glib_none().0) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use document_model::{EmbeddedFace, FaceKind, Font, FontFamily, FontPitch};
    use std::sync::Arc;

    fn font(name: &str, family: FontFamily) -> Font {
        Font {
            name: name.into(),
            family,
            pitch: FontPitch::Variable,
            alt_name: None,
            faces: Vec::new(),
        }
    }

    fn file(name: &str, path: &str) -> FontFile {
        FontFile {
            name: name.into(),
            path: path.into(),
        }
    }

    #[test]
    fn fonts_neither_installed_nor_embedded_fall_back_by_class() {
        let table = FontTable {
            fonts: vec![
                font("Nosuchfont Serifish", FontFamily::Roman),
                font("Nosuchfont Sansish", FontFamily::Swiss),
                font("Nosuchfont Mono", FontFamily::Modern),
                font("Nosuchfont Hand", FontFamily::Script),
                font("Nosuchfont Fancy", FontFamily::Decorative),
                font("Nosuchfont Auto", FontFamily::Auto),
                font("Liberation Serif", FontFamily::Roman),
                font("Calibri", FontFamily::Swiss),
                font("Cardo", FontFamily::Roman),
                font("Odd Name", FontFamily::Roman),
                font("Broken", FontFamily::Swiss),
            ],
        };
        let files = [
            file("Cardo", "/f/cardo.ttf"),
            file("Odd Name", "/f/odd.ttf"),
            file("Broken", "/f/broken.ttf"),
        ];
        let loaded = HashMap::from([
            (
                PathBuf::from("/f/cardo.ttf"),
                Some(vec!["Cardo".to_owned()]),
            ),
            (
                PathBuf::from("/f/odd.ttf"),
                Some(vec![
                    "Real Family".to_owned(),
                    "Real Family Regular".to_owned(),
                ]),
            ),
            (PathBuf::from("/f/broken.ttf"), None),
        ]);
        let installed = HashSet::from([key("Liberation Serif"), key("Carlito")]);
        let families = resolve(&table, &files, &loaded, &installed);
        let got = |name| families.get(name);
        assert_eq!(got("Nosuchfont Serifish"), "Nosuchfont Serifish,serif");
        assert_eq!(got("Nosuchfont Sansish"), "Nosuchfont Sansish,sans-serif");
        assert_eq!(got("Nosuchfont Mono"), "Nosuchfont Mono,monospace");
        assert_eq!(got("Nosuchfont Hand"), "Nosuchfont Hand,serif");
        assert_eq!(got("Nosuchfont Fancy"), "Nosuchfont Fancy,serif");
        assert_eq!(got("Nosuchfont Auto"), "Nosuchfont Auto,serif");
        // Installed: as it is, whatever case or blanks the table uses.
        assert_eq!(got("Liberation Serif"), "Liberation Serif");
        // Not installed, but fontconfig's metric alias comes right after the name, before
        // the generic family.
        assert_eq!(got("Calibri"), "Calibri,sans-serif");
        // Embedded: its own family, which is the name; or another.
        assert_eq!(got("Cardo"), "Cardo");
        assert_eq!(got("Odd Name"), "Real Family");
        // A face fontconfig couldn't read is no face.
        assert_eq!(got("Broken"), "Broken,sans-serif");
        // Fonts the table doesn't list stay as they are.
        assert_eq!(got("Elsewhere"), "Elsewhere");
    }

    #[test]
    fn families_compare_ignoring_case_and_blanks() {
        assert_eq!(key("Liberation  Serif"), key("liberationserif"));
        assert_ne!(key("Liberation Serif"), key("Liberation Sans"));
    }

    #[test]
    fn faces_are_written_once_by_their_bytes() {
        let dir = std::env::temp_dir().join(format!("slate-fonts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ttf = b"\x00\x01\x00\x00 a face".to_vec();
        let a = write_face(&dir, &ttf).unwrap();
        let b = write_face(&dir, &ttf).unwrap();
        assert_eq!(a, b);
        assert_eq!(std::fs::read(&a).unwrap(), ttf);
        let otf = b"OTTO another face".to_vec();
        let c = write_face(&dir, &otf).unwrap();
        assert_ne!(a, c);
        assert_eq!(c.extension().and_then(|e| e.to_str()), Some("otf"));
        // A file of that name with other bytes is replaced.
        std::fs::write(&a, b"damaged").unwrap();
        assert_eq!(write_face(&dir, &ttf).unwrap(), a);
        assert_eq!(std::fs::read(&a).unwrap(), ttf);
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(names.len(), 2, "no temporary files left");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_table_without_faces_writes_nothing() {
        let mut table = FontTable {
            fonts: vec![font("Cardo", FontFamily::Roman)],
        };
        assert!(write(&table).is_empty());
        table.fonts[0].faces.push(EmbeddedFace {
            kind: FaceKind::Regular,
            subsetted: false,
            bytes: Arc::from(&b"\x00\x01\x00\x00"[..]),
        });
        assert!(table.has_embedded());
    }
}
