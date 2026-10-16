//! What Etch remembers across runs: New Image's last size and background, in `settings` in
//! Etch's config folder ([`suite_platform::dirs`]: `$XDG_CONFIG_HOME/etch` on Linux,
//! `~/Library/Application Support/dev.cronkite.Etch` on macOS, `%APPDATA%\Cronkite\Etch` on
//! Windows), a `key=value` file written atomically (as Slate's).
//!
//! A missing or unreadable file, or values Etch doesn't know, mean the defaults.

use image_model::{Color, MAX_SIDE};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const WIDTH_KEY: &str = "new-width";
const HEIGHT_KEY: &str = "new-height";
const BACKGROUND_KEY: &str = "new-background";

/// What a new image starts as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Background {
    #[default]
    White,
    Transparent,
    /// The background colour of the window New Image was asked from.
    Colour,
}

impl Background {
    pub const ALL: [Background; 3] = [Self::White, Self::Transparent, Self::Colour];

    /// The name in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            Self::White => "white",
            Self::Transparent => "transparent",
            Self::Colour => "background",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.id() == id)
    }

    /// The new image's colour, given the current background colour.
    pub fn color(self, background: Color) -> Color {
        match self {
            Self::White => Color::WHITE,
            Self::Transparent => Color::TRANSPARENT,
            Self::Colour => background,
        }
    }
}

/// What the settings file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// New Image's width and height: the last ones used.
    pub new_size: (u32, u32),
    /// New Image's background: the last one used.
    pub new_background: Background,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            new_size: (1024, 768),
            new_background: Background::White,
        }
    }
}

impl Settings {
    /// Reads a settings file's text; unknown keys, and values Etch can't use, are ignored.
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        let side = |v: &str| v.parse::<u32>().ok().filter(|n| (1..=MAX_SIDE).contains(n));
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                WIDTH_KEY => s.new_size.0 = side(value).unwrap_or(s.new_size.0),
                HEIGHT_KEY => s.new_size.1 = side(value).unwrap_or(s.new_size.1),
                BACKGROUND_KEY => {
                    s.new_background = Background::from_id(value).unwrap_or(s.new_background)
                }
                _ => {}
            }
        }
        s
    }

    pub fn serialize(&self) -> String {
        format!(
            "{WIDTH_KEY}={}\n{HEIGHT_KEY}={}\n{BACKGROUND_KEY}={}\n",
            self.new_size.0,
            self.new_size.1,
            self.new_background.id()
        )
    }

    /// The user's settings; the defaults when there are none or they can't be read.
    pub fn load() -> Settings {
        match std::fs::read_to_string(path()) {
            Ok(text) => Settings::parse(&text),
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("cannot read {}: {e}", path().display());
                }
                Settings::default()
            }
        }
    }
}

/// `settings` in Etch's config folder (`$XDG_CONFIG_HOME/etch/settings`).
pub fn path() -> PathBuf {
    suite_platform::dirs::config_dir(crate::APP).join("settings")
}

/// Writes `settings` to `path` atomically (a synced temporary file renamed over the old
/// one), making its directory first. Blocks: call it off the UI thread.
pub fn write(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = settings.serialize();
    suite_file::save_atomic(
        path,
        &AtomicBool::new(false),
        |w| w.write_all(text.as_bytes()),
        |_| Ok(()),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image_model::Image;

    #[test]
    fn settings_round_trip() {
        for new_background in Background::ALL {
            let s = Settings {
                new_size: (640, 480),
                new_background,
            };
            assert_eq!(Settings::parse(&s.serialize()), s);
        }
        assert_eq!(
            Settings::default().serialize(),
            "new-width=1024\nnew-height=768\nnew-background=white\n"
        );
    }

    #[test]
    fn unusable_values_keep_the_defaults() {
        let s = Settings::parse("new-width=0\nnew-height=99999\nnew-background=plaid\nx=1\nnoise");
        assert_eq!(s, Settings::default());
        let s = Settings::parse(" new-height = 300 \n");
        assert_eq!(s.new_size, (1024, 300));
    }

    #[test]
    fn written_settings_read_back() {
        let dir = std::env::temp_dir().join(format!("etch-settings-{}", std::process::id()));
        let path = dir.join("etch").join("settings");
        let s = Settings {
            new_size: (321, 123),
            new_background: Background::Transparent,
        };
        write(&path, &s).unwrap();
        assert_eq!(Settings::parse(&std::fs::read_to_string(&path).unwrap()), s);
        // Only the file: no temporary one left beside it.
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_transparent_new_image_is_all_alpha_0() {
        let red = Color::rgb(255, 0, 0);
        let image = Image::new(37, 29, Background::Transparent.color(red)).unwrap();
        assert_eq!(image.data().len(), 37 * 29 * 4);
        assert!(image.data().chunks(4).all(|p| p[3] == 0));
        let image = Image::new(3, 2, Background::Colour.color(red)).unwrap();
        assert!(image.data().chunks(4).all(|p| p == [255, 0, 0, 255]));
        let image = Image::new(3, 2, Background::White.color(red)).unwrap();
        assert!(image.data().chunks(4).all(|p| p == [255; 4]));
    }
}
