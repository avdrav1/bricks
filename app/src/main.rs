//! Application shell: windows, menus, dialogs, OS integration.
//! Toolkit: GTK4 (ADR 0001). The window arrives with GRID-1/APP-1; until then this is a stub.

fn main() {
    let path = std::env::args().nth(1);
    println!(
        "spreadsheet {} (UI not built yet; see BACKLOG.md). File: {}",
        env!("CARGO_PKG_VERSION"),
        path.as_deref().unwrap_or("none")
    );
}
