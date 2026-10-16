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
//!
//! Document text lays out with the fonts' own widths: GDK on Windows tells GTK to hint
//! font metrics (`gtk-hint-font-metrics`, `gdkwin32misc.c`), which rounds each glyph's
//! width to whole pixels, so Courier New and Cambria broke lines elsewhere than on Linux
//! (WIN-4's runs). Slate's page and Frame's slides take [`unhinted_metrics`], with
//! Linux's slight hinting, so full hinting doesn't snap glyphs placed between pixels
//! unevenly; the apps' own interface keeps GTK's.

use std::ffi::CString;
use std::path::Path;
#[cfg(any(target_os = "macos", windows, test))]
use std::path::PathBuf;

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

/// A path as fontconfig and FreeType take it, for `FcConfigAppFontAdd*`: its own bytes on
/// Unix. On Windows both open files through the system's ANSI functions
/// (`GetFileAttributesExA`, `CreateFileA`), so it's the path in the ANSI code page, or
/// else its short (8.3) name, whose letters that code page has: a folder under a user
/// named in another script (`C:\Users\文件\…` on a Western system) has no ANSI name of its
/// own. `None` if neither names it.
pub fn fontconfig_path(path: &Path) -> Option<CString> {
    #[cfg(unix)]
    return CString::new(std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str())).ok();
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        ansi::encode(&wide).or_else(|| ansi::encode(&ansi::short(&wide)?))
    }
}

/// Lay `context`'s text out with the fonts' own (unhinted) widths, unrounded glyph
/// positions, and slight hinting, as GTK does on Linux, so lines break there as on Linux
/// and in Word. Changes only Windows' contexts, where GTK hints font metrics.
pub fn unhinted_metrics(context: &gtk::pango::Context) {
    #[cfg(windows)]
    unhinted::set(context);
    #[cfg(not(windows))]
    let _ = context;
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod unhinted {
    use gtk::cairo::{self, FontOptions, HintMetrics, HintStyle};
    use gtk::glib::translate::ToGlibPtr;
    use gtk::pango;

    #[link(name = "pangocairo-1.0")]
    extern "C" {
        fn pango_cairo_context_get_font_options(
            context: *mut pango::ffi::PangoContext,
        ) -> *const cairo::ffi::cairo_font_options_t;
        fn pango_cairo_context_set_font_options(
            context: *mut pango::ffi::PangoContext,
            options: *const cairo::ffi::cairo_font_options_t,
        );
    }

    pub fn set(context: &pango::Context) {
        let ptr = context.to_glib_none().0;
        // SAFETY: a live context; the options it holds are copied before they change.
        let had = unsafe { pango_cairo_context_get_font_options(ptr) };
        let options = if had.is_null() {
            FontOptions::new()
        } else {
            // SAFETY: a copy the wrapper owns.
            Ok(unsafe { FontOptions::from_raw_full(cairo::ffi::cairo_font_options_copy(had)) })
        };
        let Ok(mut options) = options else {
            tracing::warn!("cairo has no font options: text keeps hinted widths");
            return;
        };
        options.set_hint_metrics(HintMetrics::Off);
        options.set_hint_style(HintStyle::Slight);
        // SAFETY: a live context and options, which Pango copies.
        unsafe { pango_cairo_context_set_font_options(ptr, options.to_raw_none()) };
        context.set_round_glyph_positions(false);
    }
}

/// The system's ANSI code page, which fontconfig and FreeType read file names in.
#[cfg(windows)]
#[allow(unsafe_code)]
mod ansi {
    use std::ffi::CString;
    use std::ptr::{null, null_mut};

    const CP_ACP: u32 = 0;
    const CP_UTF8: u32 = 65001;
    const WC_NO_BEST_FIT_CHARS: u32 = 0x400;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetACP() -> u32;
        fn WideCharToMultiByte(
            page: u32,
            flags: u32,
            wide: *const u16,
            wide_len: i32,
            out: *mut u8,
            out_len: i32,
            default: *const u8,
            used_default: *mut i32,
        ) -> i32;
        fn GetShortPathNameW(long: *const u16, short: *mut u16, len: u32) -> u32;
    }

    /// `wide` in the ANSI code page, or `None` if a character has no exact equivalent
    /// there (no best fit: `é` doesn't become `e`).
    pub fn encode(wide: &[u16]) -> Option<CString> {
        let len = i32::try_from(wide.len()).ok().filter(|&n| n > 0)?;
        // SAFETY: no arguments.
        let utf8 = unsafe { GetACP() } == CP_UTF8;
        // UTF-8 has every character, and refuses the flag and the out-parameter.
        let flags = if utf8 { 0 } else { WC_NO_BEST_FIT_CHARS };
        let mut used = 0;
        let used_ptr = if utf8 {
            null_mut()
        } else {
            &mut used as *mut i32
        };
        // SAFETY: `wide` holds `len` units; a null output asks for the length only.
        let n = unsafe {
            WideCharToMultiByte(
                CP_ACP,
                flags,
                wide.as_ptr(),
                len,
                null_mut(),
                0,
                null(),
                used_ptr,
            )
        };
        if n <= 0 || used != 0 {
            return None;
        }
        let mut out = vec![0u8; usize::try_from(n).ok()?];
        // SAFETY: `out` has room for the `n` bytes the first call asked for.
        let written = unsafe {
            WideCharToMultiByte(
                CP_ACP,
                flags,
                wide.as_ptr(),
                len,
                out.as_mut_ptr(),
                n,
                null(),
                null_mut(),
            )
        };
        (written == n).then_some(())?;
        CString::new(out).ok()
    }

    /// The short (8.3) name of the existing file or folder `wide`; the long name where
    /// the volume keeps no short names.
    pub fn short(wide: &[u16]) -> Option<Vec<u16>> {
        let long: Vec<u16> = wide.iter().copied().chain([0]).collect();
        // SAFETY: a NUL-terminated name; a null output asks for the length, NUL included.
        let n = unsafe { GetShortPathNameW(long.as_ptr(), null_mut(), 0) };
        if n == 0 {
            return None;
        }
        let mut out = vec![0u16; n as usize];
        // SAFETY: `out` has room for the `n` units the first call asked for.
        let written = unsafe { GetShortPathNameW(long.as_ptr(), out.as_mut_ptr(), n) };
        if written == 0 || written >= n {
            return None;
        }
        out.truncate(written as usize);
        Some(out)
    }
}

/// Pango on fontconfig, with the bundled fonts (macOS and Windows).
#[cfg(any(target_os = "macos", windows))]
#[allow(unsafe_code)]
mod fc {
    #[cfg(target_os = "macos")]
    use std::ffi::CString;
    use std::ffi::{c_char, c_int, c_void};
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
        // The executable's real place on macOS, where a symlink may start it; on Windows
        // its name as is (canonicalize gives a `\\?\` name).
        #[cfg(target_os = "macos")]
        let exe = std::env::current_exe().and_then(std::fs::canonicalize);
        #[cfg(windows)]
        let exe = std::env::current_exe();
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
        let Some(dir) = super::fontconfig_path(dir) else {
            return false;
        };
        // SAFETY: a live configuration and a NUL-terminated path; fontconfig copies what
        // it keeps.
        unsafe { FcConfigAppFontAddDir(current(), dir.as_ptr()) != 0 }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::ffi::{CStr, CString};
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

        // One test, as above. The bundled fonts are read from folders whose names aren't
        // ASCII, as a user's profile folder (where the apps install) may be: Liberation
        // from one the ANSI code page can name (é), Carlito and Caladea from one only its
        // short name can (文件, on a Western system). Both are in %LOCALAPPDATA%, on the
        // system drive, which keeps short names (MSYS2's temp folder is on D:, which
        // doesn't).
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
            assert_ne!(matched("Carlito"), "Carlito", "nor Carlito");
            let tmp = PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA"));
            let pid = std::process::id();
            let latin = tmp.join(format!("suite-ui fonts é {pid}"));
            let cjk = tmp.join(format!("suite-ui fonts 文件 {pid}"));
            for (folder, prefix) in [(&latin, "Liberation"), (&cjk, "Ca")] {
                let _ = std::fs::remove_dir_all(folder);
                std::fs::create_dir_all(folder).unwrap();
                for entry in std::fs::read_dir(&dir).unwrap() {
                    let path = entry.unwrap().path();
                    let name = path.file_name().unwrap().to_str().unwrap();
                    if name.starts_with(prefix) && name.ends_with(".ttf") {
                        std::fs::copy(&path, folder.join(name)).unwrap();
                    }
                }
                let named = crate::fonts::fontconfig_path(folder);
                eprintln!("{} reaches fontconfig as {named:?}", folder.display());
                assert!(named.is_some(), "{}", folder.display());
                assert!(add_folder(folder));
            }
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
            let _ = std::fs::remove_dir_all(&latin);
            let _ = std::fs::remove_dir_all(&cjk);
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
