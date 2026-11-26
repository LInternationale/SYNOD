#![forbid(unsafe_code)]

pub mod adapters;
pub mod mail;
pub mod tui;

/// Initial client highlight color.
pub const DEFAULT_THEME_RGB: [u8; 3] = [0x9a, 0xb0, 0xc2];
pub mod application;
pub mod session;

#[cfg(unix)]
pub mod mcp;
