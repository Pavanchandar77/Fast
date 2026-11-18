//! Errors from font loading, SVG icons, images, and the GPU renderer.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Font(String),
    Icon(String),
    Image(String),
    Gpu(String),
    NoAdapter,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Font(msg) => write!(f, "font error: {msg}"),
            Self::Icon(msg) => write!(f, "icon error: {msg}"),
            Self::Image(msg) => write!(f, "image error: {msg}"),
            Self::Gpu(msg) => write!(f, "gpu error: {msg}"),
            Self::NoAdapter => write!(f, "no wgpu adapter"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
