//! Virtualized views for file browsing: Details, List, Grid (Icons), and Miller Columns.

pub mod columns;
pub mod details;
pub mod grid;
pub mod list;

use crate::icons::{ICON_CODE, ICON_DOCUMENT, ICON_FOLDER, ICON_IMAGE};
use nx_fs::DirEntry;

pub fn icon_for_entry(entry: &DirEntry) -> &'static str {
    if entry.kind.is_dir() {
        ICON_FOLDER
    } else {
        let name = &entry.display_name;
        if name.ends_with(".rs")
            || name.ends_with(".toml")
            || name.ends_with(".json")
            || name.ends_with(".js")
            || name.ends_with(".ts")
            || name.ends_with(".py")
            || name.ends_with(".c")
            || name.ends_with(".cpp")
            || name.ends_with(".h")
            || name.ends_with(".wgsl")
        {
            ICON_CODE
        } else if name.ends_with(".png")
            || name.ends_with(".jpg")
            || name.ends_with(".jpeg")
            || name.ends_with(".gif")
            || name.ends_with(".webp")
            || name.ends_with(".bmp")
            || name.ends_with(".svg")
        {
            ICON_IMAGE
        } else {
            ICON_DOCUMENT
        }
    }
}

pub fn kind_for_entry(entry: &DirEntry) -> &'static str {
    if entry.kind.is_dir() {
        "Folder"
    } else {
        let name = &entry.display_name;
        if name.ends_with(".rs") {
            "Rust Source"
        } else if name.ends_with(".toml") {
            "TOML Document"
        } else if name.ends_with(".json") {
            "JSON Document"
        } else if name.ends_with(".md") {
            "Markdown"
        } else if name.ends_with(".png") {
            "PNG Image"
        } else if name.ends_with(".jpg") || name.ends_with(".jpeg") {
            "JPEG Image"
        } else if name.ends_with(".txt") {
            "Plain Text"
        } else {
            "Document"
        }
    }
}
