//! Frame: a fast, lightweight Linux-native presentation app. "Frame" is a working name.
//!
//! Usage: `frame [FILE.pptx] [--version] [--bench-draw FRAMES]`
//!
//! FR0 is a viewer (docs/specs/frame.md, ADR 0004): each `.pptx` opens in its own window
//! with a slide list and a canvas, and F5 starts a full-screen slideshow. Without a file it
//! shows a start window; Ctrl+O picks a file, and files dropped on a window open. FR1
//! edits (ADR 0006): slide text on the canvas, with undo.
//! `--bench-draw` redraws the deck's densest slide for FRAMES frames and prints how long
//! each took (FRAME-4's acceptance test).

mod deck;
mod editing;
mod render;
mod slide_view;
mod window;

use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::io::IsTerminal;
use std::path::PathBuf;
use suite_core::AppKind;

const APP: AppKind = AppKind::Frame;
const APP_ID: &str = "dev.bricks.Frame";
const USAGE: &str = "usage: frame [FILE.pptx] [--version] [--bench-draw FRAMES]";

struct Args {
    file: Option<PathBuf>,
    /// `--bench-draw FRAMES`.
    bench: Option<u32>,
}

fn parse_args() -> Result<Args, String> {
    let (mut file, mut bench) = (None, None);
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--version" => {
                println!("frame {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--bench-draw" => {
                let v = it.next().ok_or("--bench-draw needs a frame count")?;
                let n: u32 = v
                    .parse()
                    .map_err(|_| format!("bad number for --bench-draw: {v:?}"))?;
                bench = Some(n.max(1));
            }
            _ if file.is_none() && !a.starts_with("--") => file = Some(PathBuf::from(a)),
            _ => return Err(format!("unexpected argument {a:?}")),
        }
    }
    if bench.is_some() && file.is_none() {
        return Err("--bench-draw needs a file".into());
    }
    Ok(Args { file, bench })
}

fn main() -> glib::ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("frame: {e}\n{USAGE}");
            return glib::ExitCode::from(2);
        }
    };
    // Logs and the startup line go to stderr: stdout carries the benchmark's report.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();
    tracing::info!("application starting");
    if let Err(e) = suite_platform::current() {
        eprintln!("frame: {e}");
        return glib::ExitCode::FAILURE;
    }
    eprintln!("{}", suite_ui::startup_message(APP));

    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_startup(|app| {
        let open = gio::ActionEntry::builder("open")
            .activate(|app: &gtk::Application, _, _| window::choose_and_open(app))
            .build();
        app.add_action_entries([open]);
        app.set_accels_for_action("app.open", &["<Control>o"]);
        app.set_accels_for_action("win.slideshow", &["F5"]);
        app.set_accels_for_action("win.slideshow-here", &["<Shift>F5"]);
    });
    let (file, bench) = (RefCell::new(args.file), args.bench);
    app.connect_activate(move |app| match file.take() {
        Some(path) => window::open(app, &path, bench),
        None if app.windows().is_empty() => window::start_window(app),
        None => {}
    });
    app.run_with_args::<&str>(&[])
}
