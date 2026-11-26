#![forbid(unsafe_code)]

pub mod adapters;
pub mod mail;
pub mod tui;

/// Initial client highlight color.
pub const DEFAULT_THEME_RGB: [u8; 3] = [0x46, 0x5d, 0x70];
pub mod application;
