//! The user's settings (ADR 0005): the text width, remembered in
//! `$XDG_CONFIG_HOME/slate/settings`, a `key=value` file written atomically.
//!
//! A missing or unreadable file, or one without a value Slate knows, means the defaults.
//! Writes run on a thread of their own, in order, so the UI thread never waits on the disk;
//! [`Saver::finish`] waits for the last one when the application shuts down.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;

type Result<T, E = std::io::Error> = std::result::Result<T, E>;

const TEXT_WIDTH_KEY: &str = "text-width";

/// How wide the text column is (View → Text Width).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextWidth {
    Narrow,
    #[default]
    Medium,
    Wide,
    /// The window's width less the margins.
    Full,
}

impl TextWidth {
    pub const ALL: [TextWidth; 4] = [Self::Narrow, Self::Medium, Self::Wide, Self::Full];

    /// The name in the settings file and the `app.text-width` action's state.
    pub fn id(self) -> &'static str {
        match self {
            Self::Narrow => "narrow",
            Self::Medium => "medium",
            Self::Wide => "wide",
            Self::Full => "full",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|w| w.id() == id)
    }

    /// The menu's name for it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Narrow => "Narrow",
            Self::Medium => "Medium",
            Self::Wide => "Wide",
            Self::Full => "Full",
        }
    }

    /// The column's width in pixels; `None` for Full, which follows the window.
    pub fn pixels(self) -> Option<i32> {
        match self {
            Self::Narrow => Some(480),
            Self::Medium => Some(624),
            Self::Wide => Some(816),
            Self::Full => None,
        }
    }
}

/// What the settings file holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub text_width: TextWidth,
}

impl Settings {
    /// Read a settings file's text; unknown keys and values are ignored.
    pub fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim() == TEXT_WIDTH_KEY {
                if let Some(w) = TextWidth::from_id(value.trim()) {
                    settings.text_width = w;
                }
            }
        }
        settings
    }

    pub fn serialize(&self) -> String {
        format!("{TEXT_WIDTH_KEY}={}\n", self.text_width.id())
    }

    /// The user's settings; the defaults when there are none or they can't be read.
    pub fn load() -> Self {
        match std::fs::read_to_string(path()) {
            Ok(text) => Self::parse(&text),
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("cannot read {}: {e}", path().display());
                }
                Self::default()
            }
        }
    }
}

/// `$XDG_CONFIG_HOME/slate/settings`.
pub fn path() -> PathBuf {
    gtk::glib::user_config_dir().join("slate").join("settings")
}

/// Write `settings` to `path`: a temporary file in the same directory, synced, then renamed
/// over the old one, so a crash leaves the old file or the new one, never half of one.
pub fn write(path: &Path, settings: &Settings) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let written = (|| {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(settings.serialize().as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

/// Saves settings off the UI thread, one write after another.
#[derive(Default)]
pub struct Saver {
    queue: Option<(mpsc::Sender<Settings>, JoinHandle<()>)>,
}

impl Saver {
    pub fn save(&mut self, settings: Settings) {
        let (sender, _) = self.queue.get_or_insert_with(|| {
            let (sender, receiver) = mpsc::channel::<Settings>();
            let thread = std::thread::spawn(move || {
                while let Ok(mut settings) = receiver.recv() {
                    // Only the newest of the waiting changes needs writing.
                    while let Ok(newer) = receiver.try_recv() {
                        settings = newer;
                    }
                    let path = path();
                    if let Err(e) = write(&path, &settings) {
                        tracing::warn!("cannot save {}: {e}", path.display());
                    }
                }
            });
            (sender, thread)
        });
        let _ = sender.send(settings);
    }

    /// Wait until every change is written.
    pub fn finish(&mut self) {
        if let Some((sender, thread)) = self.queue.take() {
            drop(sender);
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_width_round_trips() {
        for w in TextWidth::ALL {
            let settings = Settings { text_width: w };
            assert_eq!(Settings::parse(&settings.serialize()), settings);
        }
        assert_eq!(
            Settings {
                text_width: TextWidth::Wide
            }
            .serialize(),
            "text-width=wide\n"
        );
    }

    #[test]
    fn missing_or_bad_values_mean_medium() {
        for text in [
            "",
            "text-width",
            "text-width=huge\n",
            "colour=blue\n",
            "\u{FFFD}\0garbage",
        ] {
            assert_eq!(
                Settings::parse(text).text_width,
                TextWidth::Medium,
                "{text:?}"
            );
        }
    }

    #[test]
    fn parse_tolerates_spaces_other_keys_and_crlf() {
        let text = "# Slate\r\ncolour=blue\r\n text-width = full \r\n";
        assert_eq!(Settings::parse(text).text_width, TextWidth::Full);
    }

    #[test]
    fn widths_are_the_adrs() {
        let px: Vec<_> = TextWidth::ALL.iter().map(|w| w.pixels()).collect();
        assert_eq!(px, [Some(480), Some(624), Some(816), None]);
        assert_eq!(TextWidth::default(), TextWidth::Medium);
        for w in TextWidth::ALL {
            assert_eq!(TextWidth::from_id(w.id()), Some(w));
        }
    }

    #[test]
    fn write_replaces_the_file_and_leaves_no_temporary() {
        let dir = std::env::temp_dir().join(format!("slate-settings-{}", std::process::id()));
        let path = dir.join("slate").join("settings");
        for w in [TextWidth::Narrow, TextWidth::Full] {
            write(&path, &Settings { text_width: w }).unwrap();
            let text = std::fs::read_to_string(&path).unwrap();
            assert_eq!(Settings::parse(&text).text_width, w);
        }
        let files: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(files, ["settings"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
