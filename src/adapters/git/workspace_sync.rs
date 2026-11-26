//! Per-space comparison and recoverable installation from a verified Git revision.
use super::operations::git;
use crate::application::setup::NewTeam;
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Mode {
    Manual,
    WhenClean,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Preview {
    pub revision: String,
    pub paths: Vec<String>,
    pub conflicts: Vec<String>,
    pub dirty: bool,
    pub mode: Mode,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Blob {
    bytes: Vec<u8>,
    executable: bool,
}
type Files = BTreeMap<String, Blob>;

fn invalid() -> io::Error {
    io::Error::other("Unsupported path or file type in workspace")
}
fn relative(name: &str) -> io::Result<()> {
    if name.split('/').any(|part| part.starts_with(".synod-sync-"))
        || name.is_empty()
        || name.contains('\\')
        || name
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == ".." || s.eq_ignore_ascii_case(".git"))
        || !Path::new(name)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(invalid());
    }
    Ok(())
}
fn disk(dir: &Path) -> io::Result<Files> {
    fn visit(root: &Path, path: &Path, files: &mut Files) -> io::Result<()> {
        if !fs::symlink_metadata(path)?.is_dir() {
            return Err(invalid());
        }
        for e in fs::read_dir(path)? {
            let e = e?;
            let p = e.path();
            let key = p
                .strip_prefix(root)
                .map_err(io::Error::other)?
                .to_str()
                .ok_or_else(invalid)?;
            relative(key)?;
            if e.file_type()?.is_dir() {
                visit(root, &p, files)?
            } else if e.file_type()?.is_file() {
                #[cfg(unix)]
                let executable = e.metadata()?.permissions().mode() & 0o111 != 0;
                #[cfg(not(unix))]
                let executable = false;
                files.insert(
                    key.to_owned(),
                    Blob {
                        bytes: fs::read(p)?,
                        executable,
                    },
                );
            } else {
                return Err(invalid());
            }
        }
        Ok(())
    }
    let mut files = Files::new();
    match fs::symlink_metadata(dir) {
        Ok(_) => visit(dir, dir, &mut files)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    Ok(files)
}
fn tree(repo: &Path, rev: &str, space: &str) -> io::Result<Files> {
    let prefix = format!("workspace/{space}/");
    let mut files = Files::new();
    for row in git(repo, &["ls-tree", "-rz", rev, "--", &prefix])?
        .split(|b| *b == 0)
        .filter(|r| !r.is_empty())
    {
        let (header, path) = std::str::from_utf8(row)
            .map_err(io::Error::other)?
            .split_once('\t')
            .ok_or_else(invalid)?;
        let path = path.strip_prefix(&prefix).ok_or_else(invalid)?;
        relative(path)?;
        let mut fields = header.split_whitespace();
        let executable = match fields.next() {
            Some("100644") => false,
            Some("100755") => true,
            _ => return Err(invalid()),
        };
        if fields.next() != Some("blob") {
            return Err(invalid());
        }
        let id = fields.next().ok_or_else(invalid)?;
        files.insert(
            path.into(),
            Blob {
                bytes: git(repo, &["cat-file", "blob", id])?,
                executable,
            },
        );
    }
    Ok(files)
}
fn compare(base: &Files, local: &Files, remote: &Files, rev: String, mode: Mode) -> Preview {
    let mut paths = Vec::new();
    let mut conflicts = Vec::new();
    for name in base.keys().chain(remote.keys()).collect::<BTreeSet<_>>() {
        if base.get(name) == remote.get(name) {
            continue;
        }
        paths.push(name.clone());
        if local.get(name) != base.get(name) && local.get(name) != remote.get(name) {
            conflicts.push(name.clone())
        }
    }
    Preview {
        revision: rev,
        paths,
        conflicts,
        dirty: base != local,
        mode,
    }
}
fn atomic(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let temp = path.with_extension("tmp");
    let f = fs::File::create(&temp)?;
    serde_json::to_writer(&f, value)?;
    f.sync_all()?;
    fs::rename(temp, path)
}
#[derive(Serialize, Deserialize)]
struct Journal {
    revision: String,
    nonce: u128,
    edits: Vec<Edit>,
    next: usize,
}
#[derive(Serialize, Deserialize)]
struct Edit {
    name: String,
    before: Option<Blob>,
    after: Option<Blob>,
}
pub struct Space {
    repo: PathBuf,
    area: PathBuf,
    local: PathBuf,
    name: String,
}
impl Space {
    pub fn open(state: &Path, work: &Path, team: &NewTeam, space: &str) -> io::Result<Self> {
        team.validate()?;
        if !crate::adapters::local::tags::load(state, team)?.contains_key(space) {
            return Err(invalid());
        }
        let team_dir = state.join("teams").join(team.id());
        let area = team_dir
            .join("spaces")
            .join(crate::adapters::local::tags::key(space))
            .join("sync");
        fs::create_dir_all(&area)?;
        Ok(Self {
            repo: team_dir.join("repo.git"),
            area,
            local: crate::adapters::local::tags::directory(work, space)?,
            name: space.into(),
        })
    }
    pub fn mode(&self) -> io::Result<Mode> {
        match fs::File::open(self.area.join("mode.json")) {
            Ok(f) => Ok(serde_json::from_reader(f)?),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Mode::Manual),
            Err(e) => Err(e),
        }
    }
    pub fn mark_saved(&self) -> io::Result<()> {
        fs::write(self.area.join("pending-personal"), [])
    }
    pub fn mark_published(&self) -> io::Result<()> {
        match fs::remove_file(self.area.join("pending-personal")) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }
    pub fn set_mode(&self, mode: Mode) -> io::Result<()> {
        atomic(&self.area.join("mode.json"), &mode)
    }
    fn baseline(&self) -> String {
        format!(
            "refs/synod/spaces/{}/applied",
            crate::adapters::local::tags::key(&self.name)
        )
    }
    fn base(&self) -> io::Result<Files> {
        let refs = git(
            &self.repo,
            &["for-each-ref", "--format=%(refname)", &self.baseline()],
        )?;
        if refs.is_empty() {
            Ok(Files::new())
        } else {
            tree(&self.repo, &self.baseline(), &self.name)
        }
    }
    fn revision(&self) -> io::Result<String> {
        Ok(
            String::from_utf8(git(&self.repo, &["rev-parse", "refs/synod/verified-main"])?)
                .map_err(io::Error::other)?
                .trim()
                .into(),
        )
    }
    pub fn preview(&self) -> io::Result<Preview> {
        let rev = self.revision()?;
        let mut plan = compare(
            &self.base()?,
            &disk(&self.local)?,
            &tree(&self.repo, &rev, &self.name)?,
            rev,
            self.mode()?,
        );
        plan.dirty |= self.area.join("pending-personal").exists();
        Ok(plan)
    }
    fn advance(&self, rev: &str) -> io::Result<()> {
        git(&self.repo, &["update-ref", &self.baseline(), rev])?;
        Ok(())
    }

    pub fn synchronize(&self, manual: Option<&Preview>) -> io::Result<Preview> {
        self.recover()?;
        let rev = self.revision()?;
        let local = disk(&self.local)?;
        let remote = tree(&self.repo, &rev, &self.name)?;
        let mut plan = compare(&self.base()?, &local, &remote, rev, self.mode()?);
        plan.dirty |= self.area.join("pending-personal").exists();
        if let Some(observed) = manual
            && observed != &plan
        {
            return Err(io::Error::other("文件已变化，请重新预览"));
        }
        if local == remote && !plan.paths.is_empty() {
            self.advance(&plan.revision)?;
            return self.preview();
        }
        if plan.paths.is_empty()
            || !plan.conflicts.is_empty()
            || (manual.is_none()
                && (plan.mode == Mode::Manual
                    || plan.dirty
                    || self.area.join("pending-personal").exists()))
        {
            return Ok(plan);
        }
        let mut edits: Vec<_> = plan
            .paths
            .iter()
            .filter(|p| local.get(*p) != remote.get(*p))
            .map(|p| Edit {
                name: p.clone(),
                before: local.get(p).cloned(),
                after: remote.get(p).cloned(),
            })
            .collect();
        edits.sort_by_key(|e| e.after.is_some());
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let journal = Journal {
            revision: plan.revision,
            nonce,
            edits,
            next: 0,
        };
        atomic(&self.area.join("applying.json"), &journal)?;
        self.recover()?;
        self.preview()
    }
    fn recover(&self) -> io::Result<()> {
        let path = self.area.join("applying.json");
        let mut j: Journal = match fs::File::open(&path) {
            Ok(f) => serde_json::from_reader(f)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        fs::create_dir_all(&self.local)?;
        while let Some(edit) = j.edits.get(j.next) {
            relative(&edit.name)?;
            let dest = self.local.join(&edit.name);
            let parent = dest.parent().ok_or_else(invalid)?;
            let mut dir = self.local.clone();
            for c in parent
                .strip_prefix(&self.local)
                .map_err(io::Error::other)?
                .components()
            {
                dir.push(c);
                if !dir.try_exists()? {
                    fs::create_dir(&dir)?;
                }
                if !fs::symlink_metadata(&dir)?.is_dir() {
                    return Err(invalid());
                }
            }
            let tmp = parent.join(format!(".synod-sync-{}-{}-new", j.nonce, j.next));
            let old = parent.join(format!(".synod-sync-{}-{}-old", j.nonce, j.next));
            apply_file(edit, &dest, &tmp, &old)?;
            j.next += 1;
            atomic(&path, &j)?;
        }
        self.advance(&j.revision)?;
        fs::remove_file(path)
    }
}
fn read_file(path: &Path) -> io::Result<Option<Blob>> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => {
            #[cfg(unix)]
            let executable = m.permissions().mode() & 0o111 != 0;
            #[cfg(not(unix))]
            let executable = false;
            Ok(Some(Blob {
                bytes: fs::read(path)?,
                executable,
            }))
        }
        Ok(_) => Err(invalid()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
fn apply_file(edit: &Edit, dest: &Path, tmp: &Path, old: &Path) -> io::Result<()> {
    use std::io::Write;
    let collision = || io::Error::other(format!("本机文件在更新期间发生变化：{}", edit.name));
    let current = read_file(dest)?;
    if current != edit.after {
        if current.is_some() {
            if current != edit.before || old.try_exists()? {
                return Err(collision());
            }
            fs::rename(dest, old)?;
        } else if edit.before.is_some() && !old.try_exists()? {
            return Err(collision());
        }
        if let Some(previous) = read_file(old)?
            && Some(previous) != edit.before
        {
            if !dest.try_exists()? {
                fs::hard_link(old, dest)?;
            }
            return Err(collision());
        }
        if let Some(blob) = &edit.after {
            if tmp.try_exists()? {
                fs::remove_file(tmp)?;
            }
            let mut f = fs::File::create_new(tmp)?;
            f.write_all(&blob.bytes)?;
            #[cfg(unix)]
            {
                let mode = if old.is_file() {
                    fs::metadata(old)?.permissions().mode() & 0o777
                } else {
                    f.metadata()?.permissions().mode() & 0o777
                };
                f.set_permissions(fs::Permissions::from_mode(if blob.executable {
                    mode | 0o100
                } else {
                    mode & !0o111
                }))?;
            }
            f.sync_all()?;
            fs::hard_link(tmp, dest)?;
        }
    }
    if tmp.try_exists()? {
        fs::remove_file(tmp)?
    }
    if let Some(previous) = read_file(old)? {
        if Some(previous) != edit.before {
            return Err(collision());
        }
        fs::remove_file(old)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resumes_after_original_moved_and_preserves_a_concurrent_edit() -> io::Result<()> {
        let dir = std::env::temp_dir().join(format!("synod-file-recovery-{}", std::process::id()));
        fs::create_dir(&dir)?;
        let dest = dir.join("notes");
        let old = dir.join("old");
        let temp = dir.join("new");
        let blob = |text: &str| Blob {
            bytes: text.as_bytes().into(),
            executable: false,
        };
        let edit = Edit {
            name: "notes".into(),
            before: Some(blob("before")),
            after: Some(blob("after")),
        };
        fs::write(&old, "before")?;
        apply_file(&edit, &dest, &temp, &old)?;
        apply_file(&edit, &dest, &temp, &old)?;
        assert_eq!(fs::read(&dest)?, b"after");
        assert!(!temp.exists() && !old.exists());
        fs::write(&dest, "edited")?;
        assert!(apply_file(&edit, &dest, &temp, &old).is_err());
        assert_eq!(fs::read(&dest)?, b"edited");
        fs::remove_dir_all(dir)
    }
}
