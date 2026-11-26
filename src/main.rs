#[cfg(unix)]
fn main() -> std::io::Result<()> {
    let config = synod::adapters::local::state::system_config_file()?;
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--session-host")) {
        return synod::session::transport::host(config);
    }
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("mcp")) {
        return synod::mcp::run(&config);
    }
    synod::tui::live::run(&config)
}
#[cfg(not(unix))]
fn main() -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Windows session transport is not implemented",
    ))
}
