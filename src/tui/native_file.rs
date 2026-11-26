//! Native desktop attachment selection. Cancellation leaves the draft unchanged.
use crate::session::protocol::Attachment;
use std::{fs, io, path::Path, process::Command};

pub fn select() -> io::Result<Option<Attachment>> {
    #[cfg(target_os = "macos")]
    let result = Command::new("osascript")
        .args([
            "-e",
            "try",
            "-e",
            "return POSIX path of (choose file)",
            "-e",
            "on error number -128",
            "-e",
            "return \"\"",
            "-e",
            "end try",
        ])
        .output()?;
    #[cfg(not(target_os = "macos"))]
    let result = Command::new("zenity")
        .args(["--file-selection", "--title=添加附件"])
        .output()?;
    #[cfg(not(target_os = "macos"))]
    if result.status.code() == Some(1) {
        return Ok(None);
    }
    if !result.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&result.stderr).into_owned(),
        ));
    }
    let mut path = String::from_utf8(result.stdout).map_err(io::Error::other)?;
    if path.ends_with('\n') {
        path.pop();
    }
    if path.is_empty() {
        return Ok(None);
    }
    read_attachment(Path::new(&path)).map(Some)
}

fn read_attachment(path: &Path) -> io::Result<Attachment> {
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("Choose a regular file"));
    }
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| io::Error::other("Invalid attachment filename"))?
        .to_owned();
    use std::io::Read;
    let mut bytes = Vec::new();
    let mut file = file;
    file.read_to_end(&mut bytes)?;
    Ok(Attachment { filename, bytes })
}
