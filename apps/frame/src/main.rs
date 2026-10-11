//! Frame: a fast, lightweight Linux-native presentation app. "Frame" is a working name.
//!
//! A scaffold: it identifies itself and exits. The GUI toolkit is chosen in a later ADR.

use std::io::IsTerminal;
use std::process::ExitCode;
use suite_core::AppKind;

const APP: AppKind = AppKind::Frame;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();
    tracing::info!("application starting");
    if let Err(e) = suite_platform::current() {
        eprintln!("frame: {e}");
        return ExitCode::FAILURE;
    }
    println!("{}", suite_ui::startup_message(APP));
    ExitCode::SUCCESS
}
