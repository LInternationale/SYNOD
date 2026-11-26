//! Local, team-scoped workspace folder labels.
use crate::application::setup::NewTeam;
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

pub fn valid(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path.split('/').all(|p| {
            !matches!(p, "" | "." | "..")
                && !p.eq_ignore_ascii_case(".git")
                && !p.starts_with(".synod-sync-")
        })
}
pub fn key(path: &str) -> String {
    if path
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return path.into();
    }
    let mut id = String::from("p.");
    use std::fmt::Write;
    for b in path.bytes() {
        let _ = write!(id, "{b:02x}");
    }
    id
}
pub fn directory(root: &Path, name: &str) -> io::Result<PathBuf> {
    if !valid(name) {
        return Err(io::Error::other("Invalid workspace folder"));
    }
    let mut path = root.to_owned();
    if !fs::symlink_metadata(root)?.is_dir() {
        return Err(io::Error::other("Invalid workspace root"));
    }
    for part in name.split('/') {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(meta) if !meta.is_dir() => {
                return Err(io::Error::other("Expected an ordinary directory"));
            }
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(path)
}
pub type Tags = BTreeMap<String, String>;
pub fn load(state: &Path, team: &NewTeam) -> io::Result<Tags> {
    team.validate()?;
    let dir = state.join("teams").join(team.id());
    let path = dir.join("workspace-tags.json");
    match fs::File::open(path) {
        Ok(f) => {
            let tags: Tags = serde_json::from_reader(f)?;
            if tags.keys().any(|p| {
                !valid(p)
                    || Path::new(p)
                        .ancestors()
                        .skip(1)
                        .any(|a| tags.contains_key(a.to_str().unwrap_or("")))
            }) {
                return Err(io::Error::other("Invalid local workspace tags"));
            }
            Ok(tags)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let tags = team
                .spaces()
                .into_iter()
                .map(|(id, label)| (id.to_owned(), label.to_owned()))
                .collect();
            save(&dir, &tags)?;
            Ok(tags)
        }
        Err(e) => Err(e),
    }
}
fn save(dir: &Path, tags: &Tags) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let temp = dir.join("workspace-tags.tmp");
    let file = fs::File::create(&temp)?;
    serde_json::to_writer(&file, tags)?;
    file.sync_all()?;
    fs::rename(temp, dir.join("workspace-tags.json"))
}
pub fn toggle(state: &Path, work: &Path, team: &NewTeam, name: &str) -> io::Result<Tags> {
    let mut tags = load(state, team)?;
    if tags.remove(name).is_none() {
        if !directory(work, name)?.is_dir() {
            return Err(io::Error::other("请选择已有文件夹"));
        }
        let path = Path::new(name);
        if tags
            .keys()
            .any(|s| path.starts_with(s) || Path::new(s).starts_with(path))
        {
            return Err(io::Error::other("子工作区之间不能嵌套"));
        }
        tags.insert(name.into(), name.into());
    }
    save(&state.join("teams").join(team.id()), &tags)?;
    Ok(tags)
}
