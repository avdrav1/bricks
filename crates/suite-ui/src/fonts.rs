//! The fonts the apps draw text with (MAC-6, WIN-4). Call [`init`] first thing in `main`,
//! before any thread starts and before GTK.
//!
//! Linux: nothing changes. Pango draws through fontconfig with the system's fonts; the
//! metric-compatible fonts come from the distribution's packages (`docs/specs/slate.md`).
//!
//! macOS and Windows: Pango draws through fontconfig too, not CoreText or Win32
//! (`PANGOCAIRO_BACKEND=fc`, unless the environment already names a backend), so text lays
//! out as on Linux:
//! - under CoreText the tops of Brick's grid text were clipped (DEC-11 C8), and
//!   Slate's embedded `.docx` fonts, which it adds to fontconfig, weren't used (C9); on
//!   Windows Pango's own font map (DirectWrite) doesn't see fonts registered for the
//!   process either (`AddFontResourceExW` stopped working with Pango 1.50.11), so neither
//!   the bundled fonts nor Slate's embedded ones would draw (DEC-11 R8; WIN-4's runs);
//! - fontconfig reads the system's own fonts as well (`/System/Library/Fonts`,
//!   `/Library/Fonts`, `~/Library/Fonts` on the Mac; Windows' `Fonts` folders), and its
//!   metric aliases give Carlito for Calibri, Caladea for Cambria, and Liberation for
//!   Arial, Times New Roman, and Courier New where those aren't installed.
//!
//! The metric-compatible fonts ship with the app: Liberation, Carlito, and Caladea, with
//! their licences, in a `fonts` folder found from the executable ([`folder_for`]),
//! fetched by `scripts/fetch_fonts.py`. They're added as fontconfig application fonts of
//! this process, never installed for the user. GTK names the Mac's system font
//! `.AppleSystemUIFont`, which only CoreText knows; without an alias fontconfig draws the
//! apps' own text in Verdana. It gets Helvetica Neue, the Mac's system font before SF:
//! SF itself (`System Font`, `SFNS.ttf`) draws every `i` without its dot through
//! FreeType (MAC-6's runner screenshots). Windows' system font (`Segoe UI`, GTK's
//! `gtk-font-name` there) is a font file fontconfig reads like any other.

#[cfg(any(target_os = "macos", windows, test))]
use std::path::{Path, PathBuf};

/// Choose how Pango finds fonts and add the bundled ones; see the module docs. Does
/// nothing on Linux.
pub fn init() {
    #[cfg(any(target_os = "macos", windows))]
    fc::init();
}

/// The fonts folder for an executable at `exe`, if it's there:
/// - `Contents/Resources/fonts` for `Contents/MacOS/<app>` in an app bundle;
/// - else `fonts` beside the executable's folder: `target/fonts` for a build in
///   `target/debug` or `target/release` (`scripts/fetch_fonts.py` fills it), and
///   `<App>\fonts` for `<App>\bin\<app>.exe` in a Windows folder.
#[cfg(any(target_os = "macos", windows, test))]
fn folder_for(exe: &Path) -> Option<PathBuf> {
    let up = exe.parent()?.parent()?;
    [up.join("Resources").join("fonts"), up.join("fonts")]
        .into_iter()
        .find(|dir| dir.is_dir())
}

/// Pango on fontconfig, with the bundled fonts (macOS and Windows).
#[cfg(any(target_os = "macos", windows))]
#[allow(unsafe_code)]
mod fc {
    use std::ffi::{c_char, c_int, c_void, CString};
    use std::path::Path;
    use std::time::Instant;

    /// `.AppleSystemUIFont` (GTK's `gtk-font-name` on macOS) as Helvetica Neue.
    #[cfg(target_os = "macos")]
    const SYSTEM_FONT: &str = r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <alias binding="same">
    <family>.AppleSystemUIFont</family>
    <prefer><family>Helvetica Neue</family></prefer>
  </alias>
</fontconfig>
"#;

    #[link(name = "fontconfig")]
    extern "C" {
        fn FcConfigGetCurrent() -> *mut c_void;
        fn FcConfigAppFontAddDir(config: *mut c_void, dir: *const c_char) -> c_int;
        #[cfg(target_os = "macos")]
        fn FcConfigParseAndLoadFromMemory(
            config: *mut c_void,
            buffer: *const c_char,
            complain: c_int,
        ) -> c_int;
    }

    // The C library's own copy of the environment, which `getenv` reads (UCRT's: the
    // apps and MSYS2's DLLs share ucrtbase.dll).
    #[cfg(windows)]
    extern "C" {
        fn _putenv_s(name: *const c_char, value: *const c_char) -> c_int;
    }

    pub fn init() {
        choose_fontconfig();
        let t0 = Instant::now();
        #[cfg(target_os = "macos")]
        if !alias_system_font() {
            tracing::warn!("fontconfig refused the system font's alias");
        }
        let exe = std::env::current_exe().and_then(std::fs::canonicalize);
        match exe.ok().as_deref().and_then(super::folder_for) {
            Some(dir) if add_folder(&dir) => tracing::info!(
                "fonts: added {} to fontconfig in {} ms",
                dir.display(),
                t0.elapsed().as_millis()
            ),
            Some(dir) => tracing::warn!("fontconfig can't read the fonts in {}", dir.display()),
            None => tracing::warn!(
                "no bundled fonts folder: Word and PowerPoint fonts fall back to similar ones"
            ),
        }
    }

    /// Pango's backend is fontconfig unless the environment names one. Pango reads
    /// `PANGOCAIRO_BACKEND` with the C library's `getenv`, whose copy of the environment
    /// on Windows `SetEnvironmentVariableW` (Rust's `set_var`) doesn't change: there
    /// `_putenv_s` sets it too.
    pub fn choose_fontconfig() {
        if std::env::var_os("PANGOCAIRO_BACKEND").is_some() {
            return;
        }
        // Before any other thread runs (the caller's contract), so nothing reads the
        // environment meanwhile.
        std::env::set_var("PANGOCAIRO_BACKEND", "fc");
        // SAFETY: two NUL-terminated strings, which the C library copies.
        #[cfg(windows)]
        if unsafe { _putenv_s(c"PANGOCAIRO_BACKEND".as_ptr(), c"fc".as_ptr()) } != 0 {
            tracing::warn!("the C library's environment refused PANGOCAIRO_BACKEND");
        }
    }

    /// Fontconfig's current configuration, the one Pango's font map uses; the first call
    /// loads it with the system's fonts. Not thread-safe to change: one thread at a time.
    fn current() -> *mut c_void {
        // SAFETY: no arguments; the configuration is borrowed and lives while the process.
        unsafe { FcConfigGetCurrent() }
    }

    /// Add [`SYSTEM_FONT`]'s alias to the current configuration. Whether fontconfig could.
    #[cfg(target_os = "macos")]
    pub fn alias_system_font() -> bool {
        let alias = CString::new(SYSTEM_FONT).expect("no NUL in the alias");
        // SAFETY: a live configuration and a NUL-terminated buffer, parsed before return.
        unsafe { FcConfigParseAndLoadFromMemory(current(), alias.as_ptr(), 1) != 0 }
    }

    /// Add every font file in `dir` to the current configuration as this process's
    /// application fonts. Whether fontconfig could.
    pub fn add_folder(dir: &Path) -> bool {
        let Some(dir) = c_path(dir) else {
            return false;
        };
        // SAFETY: a live configuration and a NUL-terminated path; fontconfig copies what
        // it keeps.
        unsafe { FcConfigAppFontAddDir(current(), dir.as_ptr()) != 0 }
    }

    /// A path as fontconfig takes it: its own bytes on Unix, UTF-8 on Windows (where
    /// fontconfig and FreeType open files by UTF-8 names).
    fn c_path(path: &Path) -> Option<CString> {
        #[cfg(unix)]
        let bytes = std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str());
        #[cfg(windows)]
        let bytes = path.to_str()?.as_bytes();
        CString::new(bytes).ok()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::ffi::CStr;
        use std::path::PathBuf;

        #[link(name = "fontconfig")]
        extern "C" {
            fn FcNameParse(name: *const c_char) -> *mut c_void;
            fn FcConfigSubstitute(config: *mut c_void, pattern: *mut c_void, kind: c_int) -> c_int;
            fn FcDefaultSubstitute(pattern: *mut c_void);
            fn FcFontMatch(
                config: *mut c_void,
                pattern: *mut c_void,
                result: *mut c_int,
            ) -> *mut c_void;
            fn FcPatternGetString(
                pattern: *mut c_void,
                object: *const c_char,
                n: c_int,
                out: *mut *const c_char,
            ) -> c_int;
            fn FcPatternDestroy(pattern: *mut c_void);
        }

        /// The family fontconfig draws `name` in, as Pango asks for it.
        fn matched(name: &str) -> String {
            let name = CString::new(name).unwrap();
            // SAFETY: fontconfig's documented match sequence on the current configuration;
            // both patterns are destroyed after the family is copied out.
            unsafe {
                let pattern = FcNameParse(name.as_ptr());
                assert!(!pattern.is_null());
                FcConfigSubstitute(current(), pattern, 0);
                FcDefaultSubstitute(pattern);
                let mut result = 0;
                let font = FcFontMatch(current(), pattern, &mut result);
                FcPatternDestroy(pattern);
                assert!(!font.is_null(), "no match");
                let mut s: *const c_char = std::ptr::null();
                assert_eq!(FcPatternGetString(font, c"family".as_ptr(), 0, &mut s), 0);
                let family = CStr::from_ptr(s).to_string_lossy().into_owned();
                FcPatternDestroy(font);
                family
            }
        }

        /// `scripts/fetch_fonts.py` puts them in target/fonts (the macOS and Windows CI
        /// jobs do).
        fn fetched() -> PathBuf {
            let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fonts");
            assert!(
                dir.is_dir(),
                "{}: run scripts/fetch_fonts.py",
                dir.display()
            );
            dir
        }

        // One test: fontconfig's configuration is the process's, and changing it from two
        // test threads at once isn't safe.
        #[cfg(target_os = "macos")]
        #[test]
        fn the_bundled_fonts_stand_in_for_office_fonts_and_the_system_font_is_helvetica_neue() {
            let dir = fetched();
            assert_ne!(
                matched("Calibri"),
                "Carlito",
                "the Mac has no Carlito of its own"
            );
            assert!(alias_system_font());
            assert!(add_folder(&dir));
            for (asked, drawn) in [
                ("Calibri", "Carlito"),
                ("Cambria", "Caladea"),
                ("Carlito", "Carlito"),
                ("Caladea", "Caladea"),
                ("Liberation Sans", "Liberation Sans"),
                ("Liberation Serif", "Liberation Serif"),
                ("Liberation Mono", "Liberation Mono"),
                // The Mac has these, so they draw as themselves.
                ("Arial", "Arial"),
                ("Times New Roman", "Times New Roman"),
            ] {
                assert_eq!(matched(asked), drawn, "{asked}");
            }
            assert_eq!(matched(".AppleSystemUIFont"), "Helvetica Neue");
        }

        // One test, as above. The bundled fonts are read from a folder whose name isn't
        // ASCII, as a user's profile folder (where the apps install) may be.
        #[cfg(windows)]
        #[test]
        fn pango_takes_fontconfig_which_reads_windows_fonts_and_the_bundled_ones() {
            use gtk::glib::translate::FromGlibPtrFull;
            use gtk::prelude::*;
            #[link(name = "pangocairo-1.0")]
            extern "C" {
                fn pango_cairo_font_map_new() -> *mut gtk::pango::ffi::PangoFontMap;
            }

            let dir = fetched();
            choose_fontconfig();
            // SAFETY: a new font map, owned by the wrapper.
            let map: gtk::pango::FontMap =
                unsafe { FromGlibPtrFull::from_glib_full(pango_cairo_font_map_new()) };
            assert_eq!(map.type_().name(), "PangoCairoFcFontMap");

            assert_ne!(
                matched("Liberation Sans"),
                "Liberation Sans",
                "Windows has no Liberation of its own"
            );
            let copy =
                std::env::temp_dir().join(format!("suite-ui fonts ü 文件 {}", std::process::id()));
            let _ = std::fs::remove_dir_all(&copy);
            std::fs::create_dir_all(&copy).unwrap();
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|e| e == "ttf") {
                    std::fs::copy(&path, copy.join(path.file_name().unwrap())).unwrap();
                }
            }
            assert!(add_folder(&copy));
            for (asked, drawn) in [
                ("Carlito", "Carlito"),
                ("Caladea", "Caladea"),
                ("Liberation Sans", "Liberation Sans"),
                ("Liberation Serif", "Liberation Serif"),
                ("Liberation Mono", "Liberation Mono"),
                // Windows has these, so they draw as themselves.
                ("Segoe UI", "Segoe UI"),
                ("Arial", "Arial"),
                ("Times New Roman", "Times New Roman"),
                ("Courier New", "Courier New"),
            ] {
                assert_eq!(matched(asked), drawn, "{asked}");
            }
            // Windows' own when it has them (Office's fonts on most desktops), else the
            // bundled ones: the same widths either way.
            for (asked, alias) in [("Calibri", "Carlito"), ("Cambria", "Caladea")] {
                let drawn = matched(asked);
                assert!(drawn == asked || drawn == alias, "{asked}: {drawn}");
            }
            let _ = std::fs::remove_dir_all(&copy);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_is_in_the_bundle_or_beside_the_build() {
        let tmp = std::env::temp_dir().join(format!("suite-ui-fonts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let bundle = tmp.join("Slate.app/Contents");
        let target = tmp.join("target");
        let windows = tmp.join("Slate");
        std::fs::create_dir_all(bundle.join("MacOS")).unwrap();
        std::fs::create_dir_all(target.join("release")).unwrap();
        std::fs::create_dir_all(windows.join("bin")).unwrap();
        let in_bundle = bundle.join("MacOS/slate");
        let in_target = target.join("release/slate");
        let in_windows = windows.join("bin/slate.exe");
        assert_eq!(folder_for(&in_bundle), None, "no folder yet");
        assert_eq!(folder_for(&in_target), None);
        assert_eq!(folder_for(&in_windows), None);
        std::fs::create_dir_all(bundle.join("Resources/fonts")).unwrap();
        std::fs::create_dir_all(target.join("fonts")).unwrap();
        std::fs::create_dir_all(windows.join("fonts")).unwrap();
        assert_eq!(folder_for(&in_bundle), Some(bundle.join("Resources/fonts")));
        assert_eq!(folder_for(&in_target), Some(target.join("fonts")));
        assert_eq!(folder_for(&in_windows), Some(windows.join("fonts")));
        // The bundle's folder comes first when both are there.
        std::fs::create_dir_all(bundle.join("fonts")).unwrap();
        assert_eq!(folder_for(&in_bundle), Some(bundle.join("Resources/fonts")));
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
