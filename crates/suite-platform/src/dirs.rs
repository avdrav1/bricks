//! Per-user folders (ADR 0011, MAC-2): where each app keeps its settings, its state (recent
//! files, crash recovery journals), and its caches (private package copies, scratch files).
//! Linux keeps the XDG base directories; macOS uses the folders Apple's guidelines name,
//! under the app's bundle identifier; Windows uses the roaming and local application data
//! folders, under `Cronkite\<App>`.
//!
//! | Folder | Linux | macOS | Windows |
//! |---|---|---|---|
//! | [`config_dir`] | `$XDG_CONFIG_HOME/<command>`, else `~/.config/<command>` | `~/Library/Application Support/<app id>` | `%APPDATA%\Cronkite\<App>` |
//! | [`state_dir`] | `$XDG_STATE_HOME/<command>`, else `~/.local/state/<command>` | `~/Library/Application Support/<app id>` | `%LOCALAPPDATA%\Cronkite\<App>` |
//! | [`cache_dir`] | `$XDG_CACHE_HOME/<command>`, else `~/.cache/<command>` | `~/Library/Caches/<app id>` | `%LOCALAPPDATA%\Cronkite\<App>` |
//! | [`runtime_dir`] | `$XDG_RUNTIME_DIR/<command>`, when that variable names a folder | none | none |
//!
//! `<command>` is the app's command, `brick` ([`AppKind::command`]); `<app id>` its bundle
//! identifier, `dev.cronkite.Brick` ([`AppKind::app_id`]), as Apple asks of an app's own
//! folders; `<App>` its name, `Brick` ([`AppKind::display_name`]). On macOS settings and
//! state share one folder, and on Windows state and caches do (ADR 0011): their files have
//! different names (`settings`, `recent`, `recovery/`, the caches' own). An XDG variable
//! counts only when it's an absolute path, as the XDG spec says; macOS and Windows ignore
//! them. The home folder is `$HOME`, else the user's entry in the password database, else
//! the temporary folder. On Windows `%APPDATA%` and `%LOCALAPPDATA%` count when they're
//! absolute, else they're `AppData\Roaming` and `AppData\Local` in the user's profile
//! folder (`std::env::home_dir`; `HOME` is a Unix shell's, not Windows'). Nothing here
//! makes a folder; whoever writes into one does.

use crate::Platform;
use std::ffi::OsString;
use std::path::PathBuf;
use suite_core::AppKind;

/// Where `app` keeps its settings.
pub fn config_dir(app: AppKind) -> PathBuf {
    folder(PLATFORM, Folder::Config, app, &env)
}

/// Where `app` keeps what it remembers between runs: recent files, crash recovery.
pub fn state_dir(app: AppKind) -> PathBuf {
    folder(PLATFORM, Folder::State, app, &env)
}

/// Where `app` keeps files it can make again: private copies, scratch files.
pub fn cache_dir(app: AppKind) -> PathBuf {
    folder(PLATFORM, Folder::Cache, app, &env)
}

/// The user's private folder for `app`'s files that last only while they're logged in,
/// when the system has one (Linux's `$XDG_RUNTIME_DIR`).
pub fn runtime_dir(app: AppKind) -> Option<PathBuf> {
    runtime(PLATFORM, app, &env)
}

#[derive(Clone, Copy, Debug)]
enum Folder {
    Config,
    State,
    Cache,
}

/// The platform these folders are for: the one built for ([`crate::current`] refuses
/// others before any folder is asked for).
const PLATFORM: Platform = if cfg!(target_os = "macos") {
    Platform::MacOs
} else if cfg!(windows) {
    Platform::Windows
} else {
    Platform::Linux
};

/// An environment variable.
type Env<'a> = &'a dyn Fn(&str) -> Option<OsString>;

fn env(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// `folder`'s path for `app` on `platform`, reading the environment through `env`.
fn folder(platform: Platform, folder: Folder, app: AppKind, env: Env) -> PathBuf {
    match platform {
        Platform::Linux => {
            let (var, default) = match folder {
                Folder::Config => ("XDG_CONFIG_HOME", ".config"),
                Folder::State => ("XDG_STATE_HOME", ".local/state"),
                Folder::Cache => ("XDG_CACHE_HOME", ".cache"),
            };
            absolute(env(var))
                .unwrap_or_else(|| home(env).join(default))
                .join(app.command())
        }
        Platform::MacOs => {
            let library = home(env).join("Library");
            match folder {
                Folder::Config | Folder::State => library.join("Application Support"),
                Folder::Cache => library.join("Caches"),
            }
            .join(app.app_id())
        }
        Platform::Windows => {
            let (var, default) = match folder {
                Folder::Config => ("APPDATA", "Roaming"),
                Folder::State | Folder::Cache => ("LOCALAPPDATA", "Local"),
            };
            absolute(env(var))
                .unwrap_or_else(|| profile(env).join("AppData").join(default))
                .join("Cronkite")
                .join(app.display_name())
        }
    }
}

fn runtime(platform: Platform, app: AppKind, env: Env) -> Option<PathBuf> {
    match platform {
        Platform::Linux => absolute(env("XDG_RUNTIME_DIR"))
            .filter(|p| p.is_dir())
            .map(|p| p.join(app.command())),
        Platform::MacOs | Platform::Windows => None,
    }
}

fn absolute(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|p| p.is_absolute())
}

fn home(env: Env) -> PathBuf {
    absolute(env("HOME"))
        .or_else(std::env::home_dir)
        .unwrap_or_else(std::env::temp_dir)
}

/// The Windows user's profile folder: `%USERPROFILE%`, else what Windows says it is, else
/// the temporary folder.
fn profile(env: Env) -> PathBuf {
    absolute(env("USERPROFILE"))
        .or_else(std::env::home_dir)
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    const XDG: [&str; 4] = [
        "XDG_CONFIG_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
        "XDG_RUNTIME_DIR",
    ];

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let pairs: Vec<(String, OsString)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    fn all(platform: Platform, app: AppKind, env: Env) -> [PathBuf; 3] {
        [Folder::Config, Folder::State, Folder::Cache].map(|f| folder(platform, f, app, env))
    }

    // Linux's and macOS's folders are tested with Unix paths (`/home/me`), which Windows
    // doesn't take as absolute: those tests run on Unix, Windows' with paths absolute on
    // the system running them.
    #[cfg(unix)]
    #[test]
    fn linux_without_xdg_variables_uses_the_home_folder() {
        let env = vars(&[("HOME", "/home/me")]);
        assert_eq!(
            all(Platform::Linux, AppKind::Brick, &env),
            [
                "/home/me/.config/brick",
                "/home/me/.local/state/brick",
                "/home/me/.cache/brick"
            ]
            .map(PathBuf::from)
        );
        assert_eq!(runtime(Platform::Linux, AppKind::Brick, &env), None);
    }

    #[cfg(unix)]
    #[test]
    fn linux_follows_the_xdg_variables() {
        let env = vars(&[
            ("HOME", "/home/me"),
            ("XDG_CONFIG_HOME", "/cfg"),
            ("XDG_STATE_HOME", "/st"),
            ("XDG_CACHE_HOME", "/ca"),
        ]);
        assert_eq!(
            all(Platform::Linux, AppKind::Etch, &env),
            ["/cfg/etch", "/st/etch", "/ca/etch"].map(PathBuf::from)
        );
    }

    #[cfg(unix)]
    #[test]
    fn relative_or_empty_xdg_variables_are_ignored() {
        let env = vars(&[
            ("HOME", "/home/me"),
            ("XDG_CONFIG_HOME", ""),
            ("XDG_STATE_HOME", "state"),
            ("XDG_CACHE_HOME", "./cache"),
            ("XDG_RUNTIME_DIR", "run"),
        ]);
        assert_eq!(
            all(Platform::Linux, AppKind::Slate, &env),
            [
                "/home/me/.config/slate",
                "/home/me/.local/state/slate",
                "/home/me/.cache/slate"
            ]
            .map(PathBuf::from)
        );
        assert_eq!(runtime(Platform::Linux, AppKind::Slate, &env), None);
    }

    #[test]
    fn linux_runtime_folder_must_exist() {
        let here = std::env::temp_dir();
        let env = vars(&[("XDG_RUNTIME_DIR", here.to_str().unwrap())]);
        assert_eq!(
            runtime(Platform::Linux, AppKind::Slate, &env),
            Some(here.join("slate"))
        );
        let env = vars(&[("XDG_RUNTIME_DIR", "/no/such/folder")]);
        assert_eq!(runtime(Platform::Linux, AppKind::Slate, &env), None);
    }

    #[cfg(unix)]
    #[test]
    fn macos_uses_the_library_by_bundle_identifier() {
        let env = vars(&[("HOME", "/Users/me"), ("XDG_STATE_HOME", "/st")]);
        assert_eq!(
            all(Platform::MacOs, AppKind::Frame, &env),
            [
                "/Users/me/Library/Application Support/dev.cronkite.Frame",
                "/Users/me/Library/Application Support/dev.cronkite.Frame",
                "/Users/me/Library/Caches/dev.cronkite.Frame"
            ]
            .map(PathBuf::from)
        );
        let here = std::env::temp_dir();
        let env = vars(&[("XDG_RUNTIME_DIR", here.to_str().unwrap())]);
        assert_eq!(runtime(Platform::MacOs, AppKind::Frame, &env), None);
    }

    #[test]
    fn windows_uses_the_application_data_folders_by_app_name() {
        // Paths absolute on whichever system runs the test.
        let base = std::env::temp_dir();
        let (roaming, local) = (base.join("Roaming"), base.join("Local"));
        let env = vars(&[
            ("APPDATA", roaming.to_str().unwrap()),
            ("LOCALAPPDATA", local.to_str().unwrap()),
            ("XDG_STATE_HOME", "/st"),
        ]);
        let cronkite = |dir: &Path| dir.join("Cronkite").join("Frame");
        assert_eq!(
            all(Platform::Windows, AppKind::Frame, &env),
            [cronkite(&roaming), cronkite(&local), cronkite(&local)]
        );
        let env = vars(&[("XDG_RUNTIME_DIR", base.to_str().unwrap())]);
        assert_eq!(runtime(Platform::Windows, AppKind::Frame, &env), None);
    }

    #[test]
    fn windows_without_the_variables_uses_the_profile_folder() {
        let profile = std::env::temp_dir().join("me");
        let env = vars(&[
            ("USERPROFILE", profile.to_str().unwrap()),
            ("APPDATA", "relative"),
        ]);
        let app_data = profile.join("AppData");
        let cronkite = |dir: &str| app_data.join(dir).join("Cronkite").join("Etch");
        assert_eq!(
            all(Platform::Windows, AppKind::Etch, &env),
            [cronkite("Roaming"), cronkite("Local"), cronkite("Local")]
        );
    }

    /// Run in a child process by [`this_platforms_folders`]: prints every app's folders.
    #[test]
    fn print_folders_for_parent() {
        if std::env::var_os("SUITE_PLATFORM_DIRS_CHILD").is_none() {
            return;
        }
        for app in AppKind::ALL {
            let runtime = runtime_dir(app).map_or("-".into(), |p| p.display().to_string());
            println!(
                "folders\t{}\t{}\t{}\t{}\t{runtime}",
                app.command(),
                config_dir(app).display(),
                state_dir(app).display(),
                cache_dir(app).display(),
            );
        }
    }

    /// Every app's folders, `[config, state, cache, runtime]`, as a process with `HOME` at
    /// `home`, `%APPDATA%` and `%LOCALAPPDATA%` at its `Roaming` and `Local`, and the XDG
    /// variables under `xdg` (or none) finds them.
    fn folders_in_a_process(home: &Path, xdg: Option<&Path>) -> Vec<Vec<String>> {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "dirs::tests::print_folders_for_parent",
            "--nocapture",
        ])
        .env("SUITE_PLATFORM_DIRS_CHILD", "1")
        .env("HOME", home)
        .env("APPDATA", home.join("Roaming"))
        .env("LOCALAPPDATA", home.join("Local"));
        for var in XDG {
            match xdg {
                Some(base) => cmd.env(var, base.join(var)),
                None => cmd.env_remove(var),
            };
        }
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .filter_map(|l| l.strip_prefix("folders\t"))
            .map(|l| l.split('\t').skip(1).map(str::to_owned).collect())
            .collect()
    }

    /// The real folders of this build's platform, with `HOME` (and Windows' application
    /// data folders) set in a temporary folder, without and with the XDG variables.
    #[test]
    fn this_platforms_folders() {
        let base = std::env::temp_dir().join(format!("suite-platform-dirs-{}", std::process::id()));
        let (home, xdg) = (base.join("home"), base.join("xdg"));
        std::fs::create_dir_all(xdg.join("XDG_RUNTIME_DIR")).unwrap();
        let p = |p: PathBuf| p.display().to_string();
        for with_xdg in [false, true] {
            let found = folders_in_a_process(&home, with_xdg.then_some(xdg.as_path()));
            let expected: Vec<Vec<String>> = AppKind::ALL
                .iter()
                .map(|app| {
                    if cfg!(windows) {
                        let roaming = home.join("Roaming").join("Cronkite");
                        let local = home.join("Local").join("Cronkite");
                        let name = app.display_name();
                        vec![
                            p(roaming.join(name)),
                            p(local.join(name)),
                            p(local.join(name)),
                            "-".into(),
                        ]
                    } else if cfg!(target_os = "macos") {
                        let support = home.join("Library/Application Support").join(app.app_id());
                        let caches = home.join("Library/Caches").join(app.app_id());
                        vec![p(support.clone()), p(support), p(caches), "-".into()]
                    } else if with_xdg {
                        let cmd = app.command();
                        vec![
                            p(xdg.join("XDG_CONFIG_HOME").join(cmd)),
                            p(xdg.join("XDG_STATE_HOME").join(cmd)),
                            p(xdg.join("XDG_CACHE_HOME").join(cmd)),
                            p(xdg.join("XDG_RUNTIME_DIR").join(cmd)),
                        ]
                    } else {
                        let cmd = app.command();
                        vec![
                            p(home.join(".config").join(cmd)),
                            p(home.join(".local/state").join(cmd)),
                            p(home.join(".cache").join(cmd)),
                            "-".into(),
                        ]
                    }
                })
                .collect();
            assert_eq!(found, expected, "with the XDG variables: {with_xdg}");
        }
        let _ = std::fs::remove_dir_all(&base);
    }
}
