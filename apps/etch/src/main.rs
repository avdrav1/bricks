//! Etch: a fast, lightweight Linux-native raster image editor.
//!
//! Usage: `etch [FILE...] [--version] [--bench-stroke FRAMES] [--bench-open FILE]`
//!
//! Each image has its own window (docs/specs/etch.md, ADR 0010) with a menu bar, the
//! canvas, and a status bar. `etch` opens a blank white 1024×768 image; `etch FILE...`
//! opens each PNG, JPEG, or BMP file in a window of its own, read off the UI thread.
//! File → New Image (Ctrl+N) asks for a size and a background (white, transparent, or the
//! background colour), remembered across runs; File → Open (Ctrl+O) picks a file, and Save
//! (Ctrl+S), Save As (Ctrl+Shift+S), and Export (Ctrl+Shift+E) write PNG, JPEG, or BMP;
//! closing or quitting with changes not saved asks first. The canvas zooms and pans, the
//! pencil and eraser draw, and every stroke undoes and redoes.
//! `--bench-stroke` draws a stroke one segment a frame on a 7680×4320 image at 100% for
//! FRAMES frames and prints how long each took (ETCH-2's acceptance test); `--bench-open`
//! opens FILE and prints how long it took to show (ETCH-6's).

mod canvas;
mod colours;
mod saving;
mod settings;
mod text;
mod tools;
mod view;
mod window;

use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::Instant;
use suite_core::AppKind;

const APP: AppKind = AppKind::Etch;
/// The desktop entry and icon take this name (packaging/dev.cronkite.Etch.*), so docks
/// match the window to its entry.
const APP_ID: &str = "dev.cronkite.Etch";
const USAGE: &str = "usage: etch [FILE...] [--version] [--bench-stroke FRAMES] [--bench-open FILE]";

#[derive(Default)]
struct Args {
    files: Vec<PathBuf>,
    /// `--bench-stroke FRAMES`.
    stroke: Option<u32>,
    /// `--bench-open FILE`.
    open: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = std::env::args_os().skip(1);
    while let Some(a) = it.next() {
        match a.to_str() {
            Some("--version") => {
                println!("etch {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            Some("--bench-stroke") => {
                let v = it.next().ok_or("--bench-stroke needs a frame count")?;
                let n: u32 = v
                    .to_str()
                    .and_then(|v| v.parse().ok())
                    .ok_or_else(|| format!("bad number for --bench-stroke: {v:?}"))?;
                args.stroke = Some(n.max(1));
            }
            Some("--bench-open") => {
                let file = it.next().ok_or("--bench-open needs a file")?;
                args.open = Some(file.into());
            }
            Some("--") => args.files.extend(it.by_ref().map(PathBuf::from)),
            Some(s) if s.starts_with("--") => return Err(format!("unexpected argument {s:?}")),
            _ => args.files.push(a.into()),
        }
    }
    if (args.stroke.is_some() || args.open.is_some()) && !args.files.is_empty()
        || args.stroke.is_some() && args.open.is_some()
    {
        return Err("a benchmark runs alone".to_owned());
    }
    Ok(args)
}

fn main() -> glib::ExitCode {
    let start = Instant::now();
    let args = match parse_args() {
        Ok(a) => a,
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
    let (files, stroke, open) = (RefCell::new(args.files), args.stroke, args.open);
    app.connect_activate(move |app| {
        let files = files.take();
        if let Some(path) = &open {
            window::open(app, path, Some(start));
        } else if let Some(frames) = stroke {
            window::open_bench(app, frames);
        } else if !files.is_empty() {
            for path in &files {
                window::open(app, path, None);
            }
        } else if app.windows().is_empty() {
            window::open_blank(app, window::DEFAULT_SIZE);
        }
    });
    app.run_with_args::<&str>(&[])
}
