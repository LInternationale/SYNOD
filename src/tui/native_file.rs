//! Native desktop attachment selection. Cancellation leaves the draft unchanged.
use crate::session::protocol::Attachment;
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

pub fn select() -> io::Result<Option<Attachment>> {
    pick(false)?.map(|path| read_attachment(&path)).transpose()
}

pub(super) fn directory() -> io::Result<Option<PathBuf>> {
    pick(true)
}

fn pick(folder: bool) -> io::Result<Option<PathBuf>> {
    #[cfg(target_os = "macos")]
    let result = Command::new("osascript")
        .args([
            "-e",
            "try",
            "-e",
            if folder {
                "return POSIX path of (choose folder)"
            } else {
                "return POSIX path of (choose file)"
            },
            "-e",
            "on error number -128",
            "-e",
            "return \"\"",
            "-e",
            "end try",
        ])
        .output()?;
    #[cfg(not(target_os = "macos"))]
    let result = {
        let mut command = Command::new("zenity");
        command.arg("--file-selection");
        if folder {
            command.args(["--directory", "--title=选择文件夹"]);
        } else {
            command.arg("--title=添加附件");
        }
        command.output()?
    };
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
    Ok(Some(PathBuf::from(path)))
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
