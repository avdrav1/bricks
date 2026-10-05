//! Application shell: windows, menus, dialogs, OS integration.
//! The UI toolkit is chosen in DEC-1; until then this is a stub.

fn main() {
    let path = std::env::args().nth(1);
    println!(
        "spreadsheet {} (UI not built yet; see BACKLOG.md). File: {}",
        env!("CARGO_PKG_VERSION"),
        path.as_deref().unwrap_or("none")
    );
}
