//! The fonts the apps draw text with (MAC-6). Call [`init`] first thing in `main`, before
//! any thread starts and before GTK.
//!
//! Linux: nothing changes. Pango draws through fontconfig with the system's fonts; the
//! metric-compatible fonts come from the distribution's packages (`docs/specs/slate.md`).
//!
//! macOS: Pango draws through fontconfig too, not CoreText (`PANGOCAIRO_BACKEND=fc`,
//! unless the environment already names a backend), so text lays out as on Linux:
//! - under CoreText the tops of Brick's grid text were clipped (DEC-11 C8), and
//!   Slate's embedded `.docx` fonts, which it adds to fontconfig, weren't used (C9);
//! - fontconfig reads the Mac's own fonts as well (`/System/Library/Fonts`,
//!   `/Library/Fonts`, `~/Library/Fonts`), and its metric aliases give Carlito for
//!   Calibri, Caladea for Cambria, and Liberation for Arial, Times New Roman, and Courier
//!   New where those aren't installed.
//!
//! The metric-compatible fonts ship with the app: Liberation, Carlito, and Caladea, with
//! their licences, in a `fonts` folder found from the executable ([`folder_for`]),
//! fetched by `scripts/fetch_fonts.py`. They're added as fontconfig application fonts of
//! this process, never installed for the user. GTK names the Mac's system font
//! `.AppleSystemUIFont`, which only CoreText knows; fontconfig is told it's the system
//! font's file (SF), so the app's own text keeps the Mac's look.

#[cfg(any(target_os = "macos", test))]
use std::path::{Path, PathBuf};

/// Choose how Pango finds fonts and add the bundled ones; see the module docs. Does
/// nothing on Linux.
pub fn init() {
    #[cfg(target_os = "macos")]
    mac::init();
}

/// The fonts folder for an executable at `exe`, if it's there:
/// - `Contents/Resources/fonts` for `Contents/MacOS/<app>` in an app bundle;
/// - else `fonts` beside the executable's folder: `target/fonts` for a build in
///   `target/debug` or `target/release` (`scripts/fetch_fonts.py` fills it).
#[cfg(any(target_os = "macos", test))]
fn folder_for(exe: &Path) -> Option<PathBuf> {
    let up = exe.parent()?.parent()?;
    [up.join("Resources").join("fonts"), up.join("fonts")]
        .into_iter()
        .find(|dir| dir.is_dir())
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod mac {
    use std::ffi::{c_char, c_int, c_void, CString};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;
    use std::time::Instant;

    /// `.AppleSystemUIFont` (GTK's `gtk-font-name` on macOS) as the system font's file.
    /// SF's family name differs between macOS releases, so the alias lists them in order.
    const SYSTEM_FONT: &str = r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <alias binding="same">
    <family>.AppleSystemUIFont</family>
    <prefer>
      <family>System Font</family>
      <family>.SF NS</family>
      <family>SF Pro</family>
      <family>Helvetica Neue</family>
    </prefer>
  </alias>
</fontconfig>
"#;

    #[link(name = "fontconfig")]
    extern "C" {
        fn FcConfigAppFontAddDir(config: *mut c_void, dir: *const c_char) -> c_int;
        fn FcConfigParseAndLoadFromMemory(
            config: *mut c_void,
            buffer: *const c_char,
            complain: c_int,
        ) -> c_int;
    }

    pub fn init() {
        if std::env::var_os("PANGOCAIRO_BACKEND").is_none() {
            // Before any other thread runs (the caller's contract), so nothing reads the
            // environment meanwhile.
            std::env::set_var("PANGOCAIRO_BACKEND", "fc");
        }
        let t0 = Instant::now();
        let alias = CString::new(SYSTEM_FONT).expect("no NUL in the alias");
        // SAFETY: a NUL-terminated buffer, null for fontconfig's current configuration
        // (which this loads: Pango's font map then uses it).
        if unsafe { FcConfigParseAndLoadFromMemory(std::ptr::null_mut(), alias.as_ptr(), 1) } == 0 {
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

    /// Add every font file in `dir` to fontconfig's current configuration as this
    /// process's application fonts. Whether fontconfig could.
    pub fn add_folder(dir: &Path) -> bool {
        let Ok(dir) = CString::new(dir.as_os_str().as_bytes()) else {
            return false;
        };
        // SAFETY: null for the current configuration and a NUL-terminated path;
        // fontconfig copies what it keeps.
        unsafe { FcConfigAppFontAddDir(std::ptr::null_mut(), dir.as_ptr()) != 0 }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::ffi::CStr;

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
                FcConfigSubstitute(std::ptr::null_mut(), pattern, 0);
                FcDefaultSubstitute(pattern);
                let mut result = 0;
                let font = FcFontMatch(std::ptr::null_mut(), pattern, &mut result);
                FcPatternDestroy(pattern);
                assert!(!font.is_null(), "no match");
                let mut s: *const c_char = std::ptr::null();
                assert_eq!(FcPatternGetString(font, c"family".as_ptr(), 0, &mut s), 0);
                let family = CStr::from_ptr(s).to_string_lossy().into_owned();
                FcPatternDestroy(font);
                family
            }
        }

        #[test]
        fn the_bundled_fonts_stand_in_for_office_fonts() {
            // `scripts/fetch_fonts.py` puts them in target/fonts (the macOS CI job does).
            let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fonts");
            assert!(
                dir.is_dir(),
                "{}: run scripts/fetch_fonts.py",
                dir.display()
            );
            assert_ne!(
                matched("Calibri"),
                "Carlito",
                "the Mac has no Carlito of its own"
            );
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
        }

        #[test]
        fn the_system_font_is_the_macs() {
            let alias = CString::new(SYSTEM_FONT).unwrap();
            // SAFETY: as in `init`.
            let loaded =
                unsafe { FcConfigParseAndLoadFromMemory(std::ptr::null_mut(), alias.as_ptr(), 1) };
            assert_eq!(loaded, 1);
            let family = matched(".AppleSystemUIFont");
            assert!(
                ["System Font", ".SF NS", "SF Pro", "Helvetica Neue"].contains(&family.as_str()),
                "{family}"
            );
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
        std::fs::create_dir_all(bundle.join("MacOS")).unwrap();
        std::fs::create_dir_all(target.join("release")).unwrap();
        let in_bundle = bundle.join("MacOS/slate");
        let in_target = target.join("release/slate");
        assert_eq!(folder_for(&in_bundle), None, "no folder yet");
        assert_eq!(folder_for(&in_target), None);
        std::fs::create_dir_all(bundle.join("Resources/fonts")).unwrap();
        std::fs::create_dir_all(target.join("fonts")).unwrap();
        assert_eq!(folder_for(&in_bundle), Some(bundle.join("Resources/fonts")));
        assert_eq!(folder_for(&in_target), Some(target.join("fonts")));
        // The bundle's folder comes first when both are there.
        std::fs::create_dir_all(bundle.join("fonts")).unwrap();
        assert_eq!(folder_for(&in_bundle), Some(bundle.join("Resources/fonts")));
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
