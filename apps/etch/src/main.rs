//! Etch: a fast, lightweight Linux-native raster image editor.
//!
//! Usage: `etch [--version] [--bench-stroke FRAMES]`
//!
//! ET0 is a canvas prototype (docs/specs/etch.md, ADR 0010): each image has its own window
//! with a menu bar, the canvas, and a status bar. `etch` opens a blank white 1024×768
//! image; File → New Image (Ctrl+N) asks for another size. The canvas zooms and pans, the
//! pencil draws one-pixel black lines, and every stroke undoes and redoes.
//! `--bench-stroke` draws a stroke one segment a frame on a 7680×4320 image at 100% for
//! FRAMES frames and prints how long each took (ETCH-2's acceptance test).

mod canvas;
mod view;
mod window;

use gtk::prelude::*;
use gtk::{gio, glib};
use std::io::IsTerminal;
use suite_core::AppKind;

const APP: AppKind = AppKind::Etch;
/// The desktop entry and icon take this name (packaging/dev.cronkite.Etch.*), so docks
/// match the window to its entry.
const APP_ID: &str = "dev.cronkite.Etch";
const USAGE: &str = "usage: etch [--version] [--bench-stroke FRAMES]";

/// `--bench-stroke FRAMES`, if given.
fn parse_args() -> Result<Option<u32>, String> {
    let mut bench = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--version" => {
                println!("etch {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--bench-stroke" => {
                let v = it.next().ok_or("--bench-stroke needs a frame count")?;
                let n: u32 = v
                    .parse()
                    .map_err(|_| format!("bad number for --bench-stroke: {v:?}"))?;
                bench = Some(n.max(1));
            }
            _ => return Err(format!("unexpected argument {a:?}")),
        }
    }
    Ok(bench)
}

fn main() -> glib::ExitCode {
    let bench = match parse_args() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("etch: {e}\n{USAGE}");
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
        eprintln!("etch: {e}");
        return glib::ExitCode::FAILURE;
    }
    eprintln!("{}", suite_ui::startup_message(APP));

    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_startup(window::install_app_actions);
    app.connect_activate(move |app| {
        if app.windows().is_empty() {
            match bench {
                Some(frames) => window::open_bench(app, frames),
                None => window::open_blank(app, window::DEFAULT_SIZE),
            }
        }
    });
    app.run_with_args::<&str>(&[])
}
