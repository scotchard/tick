//! Shared core for Tick: the Markdown to-do document, the file store, and
//! Omarchy theme colours. No dependencies; every frontend builds on this.

pub mod doc;
pub mod store;
pub mod theme;

use std::path::PathBuf;

pub use doc::{Doc, Item, List, Span};
pub use store::Store;
pub use theme::{Rgb, Theme, ThemeWatch};

/// Where captured to-dos go when no list is named.
pub const INBOX: &str = "Inbox";

/// The to-do file: `$TICK_FILE`, else `~/Documents/Tick/todo.md` (a folder of
/// its own, so it can be shared with Syncthing).
pub fn default_path() -> PathBuf {
    if let Some(p) = std::env::var_os("TICK_FILE").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join("Documents/Tick/todo.md")
}

/// `~` for the home directory, for showing paths in the UI.
pub fn display_path(path: &std::path::Path) -> String {
    if let Some(home) = std::env::var_os("HOME") {
        if let Ok(rest) = path.strip_prefix(&home) {
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}

/// Today's date via `date(1)`, formatted with a strftime pattern. Std has no
/// time zones, and shelling out once a minute costs nothing.
pub fn local_date(fmt: &str) -> String {
    std::process::Command::new("date")
        .arg(format!("+{fmt}"))
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}
