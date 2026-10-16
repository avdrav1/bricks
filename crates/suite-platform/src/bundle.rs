//! Running from a self-contained macOS app bundle (MAC-7). Call [`init`] first thing in
//! `main`, before any thread starts and before GTK, fontconfig, or GLib read their
//! settings.
//!
//! A release bundle (`scripts/mac_bundle.sh`) carries GTK and everything it reads at run
//! time, so it needs no Homebrew:
//!
//! | In `Contents/` | What |
//! |---|---|
//! | `MacOS/<app>` | the app, linked to `@executable_path/../Frameworks` |
//! | `Frameworks/*.dylib` | GTK, GLib, Pango, cairo, fontconfig, gdk-pixbuf, librsvg, and their libraries |
//! | `Resources/share/glib-2.0/schemas` | GTK's GSettings schemas, compiled |
//! | `Resources/share/icons` | Adwaita's symbolic icons, hicolor with the app's icon |
//! | `Resources/lib/gdk-pixbuf-2.0/2.10.0` | gdk-pixbuf's loaders (SVG among them) and `loaders.cache`, whose module paths are relative to `Resources` |
//! | `Resources/etc/fonts` | fontconfig's `fonts.conf` and `conf.d` (the metric aliases) |
//! | `Resources/fonts` | the bundled fonts (`suite_ui::fonts`) |
//! | `Resources/licenses` | each bundled Homebrew formula's licence files and `THIRD-PARTY.txt` |
//!
//! Those libraries were built to look in Homebrew's prefix, so [`init`] points them at the
//! bundle through their environment variables: `XDG_DATA_DIRS` (schemas, icons),
//! `GSETTINGS_SCHEMA_DIR`, `GTK_DATA_PREFIX` and `GTK_EXE_PREFIX` (GTK's own data and
//! modules), `GIO_MODULE_DIR` (none: a Mac with Homebrew's GLib must not load its
//! modules into the bundle's GLib), `FONTCONFIG_FILE` and `FONTCONFIG_PATH`, and
//! `GDK_PIXBUF_MODULE_FILE`. gdk-pixbuf wants absolute module paths and the bundle can be
//! anywhere, so the loader list is written with this bundle's paths into the app's cache
//! folder (`~/Library/Caches/<app id>/gdk-pixbuf-loaders.cache`) when it differs.
//!
//! Elsewhere (Linux, a build in `target/`, MAC-5's minimal bundles without `Frameworks`)
//! nothing changes.

#[cfg(any(target_os = "macos", test))]
use std::ffi::OsString;
#[cfg(any(target_os = "macos", test))]
use std::path::Path;
use std::path::PathBuf;
use suite_core::AppKind;

/// gdk-pixbuf's folder in `Contents/Resources`: the loaders and their list.
#[cfg(any(target_os = "macos", test))]
const PIXBUF: &str = "lib/gdk-pixbuf-2.0/2.10.0";

/// Point GTK and its libraries at the bundle's files when the app runs from a
/// self-contained bundle; see the module docs. `Ok(Some(resources))` names the bundle's
/// `Contents/Resources`, `Ok(None)` means not a bundle (nothing set). An error says the
/// loader list couldn't be written: everything else is set, and pictures gdk-pixbuf
/// decodes (SVG) won't show.
#[cfg(target_os = "macos")]
pub fn init(app: AppKind) -> Result<Option<PathBuf>, String> {
    mac::init(app)
}

/// Not a macOS bundle: nothing to set.
#[cfg(not(target_os = "macos"))]
pub fn init(_app: AppKind) -> Result<Option<PathBuf>, String> {
    Ok(None)
}

/// The bundle's `Contents` folder for an executable at `exe`, when that's a self-contained
/// bundle's: `<Name>.app/Contents/MacOS/<app>` beside `Contents/Frameworks`.
#[cfg(any(target_os = "macos", test))]
fn contents_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundled = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && contents.join("Frameworks").is_dir();
    bundled.then(|| contents.to_path_buf())
}

/// The variables for the bundle at `contents`, given the loader list's path (when written)
/// and the environment's `XDG_DATA_DIRS`, which the bundle's `share` goes before.
#[cfg(any(target_os = "macos", test))]
fn variables(
    contents: &Path,
    loaders: Option<&Path>,
    data_dirs: Option<OsString>,
) -> Vec<(&'static str, OsString)> {
    let res = contents.join("Resources");
    let share = res.join("share");
    let mut dirs = share.clone().into_os_string();
    if let Some(rest) = data_dirs.filter(|d| !d.is_empty()) {
        dirs.push(":");
        dirs.push(rest);
    }
    let mut vars = vec![
        ("XDG_DATA_DIRS", dirs),
        (
            "GSETTINGS_SCHEMA_DIR",
            share.join("glib-2.0/schemas").into(),
        ),
        ("GTK_DATA_PREFIX", res.clone().into()),
        ("GTK_EXE_PREFIX", res.clone().into()),
        ("GIO_MODULE_DIR", res.join("lib/gio/modules").into()),
        ("FONTCONFIG_FILE", res.join("etc/fonts/fonts.conf").into()),
        ("FONTCONFIG_PATH", res.join("etc/fonts").into()),
    ];
    if let Some(list) = loaders {
        vars.push(("GDK_PIXBUF_MODULE_FILE", list.into()));
        vars.push((
            "GDK_PIXBUF_MODULEDIR",
            res.join(PIXBUF).join("loaders").into(),
        ));
    }
    vars
}

/// gdk-pixbuf's loader list `template`, whose module paths are relative to the bundle's
/// `Resources`, with those paths made absolute under `res`. A module's path is the line
/// holding nothing but one quoted relative path ending in `.so`; every other line stays.
#[cfg(any(target_os = "macos", test))]
fn loaders_list(template: &str, res: &Path) -> String {
    let mut out = String::with_capacity(template.len() + 64 * 16);
    for line in template.lines() {
        let path = line
            .strip_prefix('"')
            .and_then(|l| l.strip_suffix('"'))
            .filter(|p| !p.contains('"') && !p.starts_with('/') && p.ends_with(".so"));
        match path {
            Some(p) => {
                out.push('"');
                out.push_str(&res.join(p).to_string_lossy());
                out.push('"');
            }
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{contents_of, loaders_list, variables, PIXBUF};
    use std::path::{Path, PathBuf};
    use suite_core::AppKind;

    pub fn init(app: AppKind) -> Result<Option<PathBuf>, String> {
        let exe = std::env::current_exe().and_then(std::fs::canonicalize);
        let Some(contents) = exe.ok().as_deref().and_then(contents_of) else {
            return Ok(None);
        };
        let res = contents.join("Resources");
        let list = write_loaders(&res, &crate::dirs::cache_dir(app));
        let data_dirs = std::env::var_os("XDG_DATA_DIRS");
        for (name, value) in variables(
            &contents,
            list.as_ref().ok().map(PathBuf::as_path),
            data_dirs,
        ) {
            // Before any other thread runs (the caller's contract), so nothing reads the
            // environment meanwhile.
            std::env::set_var(name, value);
        }
        list.map(|_| Some(res))
    }

    /// Write the bundle's loader list with absolute paths into `cache`, unless it's there
    /// already; its path.
    fn write_loaders(res: &Path, cache: &Path) -> Result<PathBuf, String> {
        let template = res.join(PIXBUF).join("loaders.cache");
        let text = std::fs::read_to_string(&template)
            .map_err(|e| format!("can't read {}: {e}", template.display()))?;
        let list = loaders_list(&text, res);
        let path = cache.join("gdk-pixbuf-loaders.cache");
        if std::fs::read_to_string(&path).is_ok_and(|old| old == list) {
            return Ok(path);
        }
        // Another copy of the app may be writing it too: each writes its own file and
        // renames it into place.
        let tmp = cache.join(format!(".gdk-pixbuf-loaders.cache.{}", std::process::id()));
        std::fs::create_dir_all(cache)
            .and_then(|()| std::fs::write(&tmp, &list))
            .and_then(|()| std::fs::rename(&tmp, &path))
            .map_err(|e| format!("can't write {}: {e}", path.display()))?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_bundle_with_frameworks_counts() {
        let tmp =
            std::env::temp_dir().join(format!("suite-platform-bundle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let contents = tmp.join("Brick.app/Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        std::fs::create_dir_all(tmp.join("target/release")).unwrap();
        let exe = contents.join("MacOS/brick");
        assert_eq!(
            contents_of(&exe),
            None,
            "MAC-5's minimal bundle: no Frameworks"
        );
        std::fs::create_dir_all(contents.join("Frameworks")).unwrap();
        assert_eq!(contents_of(&exe), Some(contents.clone()));
        assert_eq!(contents_of(&tmp.join("target/release/brick")), None);
        assert_eq!(contents_of(Path::new("/brick")), None);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn the_variables_point_into_the_bundle() {
        let contents = Path::new("/Users/me/Applications/Slate.app/Contents");
        let list =
            Path::new("/Users/me/Library/Caches/dev.cronkite.Slate/gdk-pixbuf-loaders.cache");
        let res = "/Users/me/Applications/Slate.app/Contents/Resources";
        let vars = variables(contents, Some(list), None);
        let get = |name: &str| {
            vars.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.to_string_lossy().into_owned())
        };
        assert_eq!(get("XDG_DATA_DIRS").unwrap(), format!("{res}/share"));
        assert_eq!(
            get("GSETTINGS_SCHEMA_DIR").unwrap(),
            format!("{res}/share/glib-2.0/schemas")
        );
        assert_eq!(get("GTK_DATA_PREFIX").unwrap(), res);
        assert_eq!(get("GTK_EXE_PREFIX").unwrap(), res);
        assert_eq!(
            get("GIO_MODULE_DIR").unwrap(),
            format!("{res}/lib/gio/modules")
        );
        assert_eq!(
            get("FONTCONFIG_FILE").unwrap(),
            format!("{res}/etc/fonts/fonts.conf")
        );
        assert_eq!(get("FONTCONFIG_PATH").unwrap(), format!("{res}/etc/fonts"));
        assert_eq!(
            get("GDK_PIXBUF_MODULE_FILE").unwrap(),
            list.to_str().unwrap()
        );
        assert_eq!(
            get("GDK_PIXBUF_MODULEDIR").unwrap(),
            format!("{res}/lib/gdk-pixbuf-2.0/2.10.0/loaders")
        );

        // The environment's data folders stay, after the bundle's; no list, no loaders.
        let vars = variables(contents, None, Some("/usr/local/share:/usr/share".into()));
        let dirs = vars.iter().find(|(n, _)| *n == "XDG_DATA_DIRS").unwrap();
        assert_eq!(
            dirs.1,
            OsString::from(format!("{res}/share:/usr/local/share:/usr/share"))
        );
        assert!(!vars.iter().any(|(n, _)| n.starts_with("GDK_PIXBUF")));
        let vars = variables(contents, None, Some("".into()));
        let dirs = vars.iter().find(|(n, _)| *n == "XDG_DATA_DIRS").unwrap();
        assert_eq!(dirs.1, OsString::from(format!("{res}/share")));
    }

    #[test]
    fn the_loader_list_gets_the_bundles_paths() {
        // As scripts/mac_bundle.sh writes it, from gdk-pixbuf-query-loaders' output.
        let template = "\
# GdkPixbuf Image Loader Modules file
# LoaderDir = lib/gdk-pixbuf-2.0/2.10.0/loaders
#
\"lib/gdk-pixbuf-2.0/2.10.0/loaders/libpixbufloader_svg.so\"
\"svg\" 6 \"gdk-pixbuf\" \"Scalable Vector Graphics\" \"LGPL\"
\"image/svg+xml\" \"image/svg\" \"\"
\"svg\" \"svgz\" \"\"
\" <svg\" \"*    \" 100

\"lib/gdk-pixbuf-2.0/2.10.0/loaders/libpixbufloader-gif.so\"
\"gif\" 4 \"gdk-pixbuf\" \"GIF\" \"LGPL\"
";
        let res = Path::new("/Applications/Frame.app/Contents/Resources");
        let list = loaders_list(template, res);
        let lines: Vec<&str> = list.lines().collect();
        assert_eq!(lines.len(), template.lines().count());
        assert_eq!(
            lines[3],
            "\"/Applications/Frame.app/Contents/Resources/lib/gdk-pixbuf-2.0/2.10.0/loaders/libpixbufloader_svg.so\""
        );
        assert_eq!(
            lines[9],
            "\"/Applications/Frame.app/Contents/Resources/lib/gdk-pixbuf-2.0/2.10.0/loaders/libpixbufloader-gif.so\""
        );
        // Everything else is as it was.
        for i in [0, 1, 2, 4, 5, 6, 7, 8, 10] {
            assert_eq!(lines[i], template.lines().nth(i).unwrap(), "line {i}");
        }
        // An absolute path stays.
        assert_eq!(loaders_list("\"/opt/x.so\"\n", res), "\"/opt/x.so\"\n");
    }
}
