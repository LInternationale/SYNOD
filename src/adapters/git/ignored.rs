//! Match workspace .gitignore files using Git's own pattern semantics.
use super::operations::git;
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) struct Ignored {
    ignored: BTreeSet<PathBuf>,
    tracked: BTreeSet<PathBuf>,
}
impl Ignored {
    pub(crate) fn read(work: &Path, repo: &Path, tracked: BTreeSet<PathBuf>) -> io::Result<Self> {
        if !repo.join("HEAD").is_file() {
            fs::create_dir_all(repo)?;
            git(repo, &["init", "--bare"])?;
        }
        let output = Command::new("git")
            .env("GIT_INDEX_FILE", repo.join("synod-ignore-empty-index"))
            .arg("-C")
            .arg(work)
            .arg("--git-dir")
            .arg(repo)
            .arg("--work-tree")
            .arg(work)
            .args([
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-per-directory=.gitignore",
                "--directory",
                "-z",
            ])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        let ignored = output
            .stdout
            .split(|b| *b == 0)
            .filter(|b| !b.is_empty())
            .map(|b| {
                std::str::from_utf8(b)
                    .map(|s| PathBuf::from(s.trim_end_matches('/')))
                    .map_err(io::Error::other)
            })
            .collect::<io::Result<_>>()?;
        Ok(Self { ignored, tracked })
    }
    pub(crate) fn excludes(&self, path: &Path) -> bool {
        path.ancestors().any(|a| self.ignored.contains(a))
            && !self
                .tracked
                .range(path.to_owned()..)
                .next()
                .is_some_and(|p| p.starts_with(path))
    }
    pub(crate) fn known(repo: &Path) -> io::Result<BTreeSet<PathBuf>> {
        let mut paths = BTreeSet::new();
        if repo.join("HEAD").is_file() {
            let commits = git(
                repo,
                &[
                    "for-each-ref",
                    "--format=%(objectname)",
                    "refs/synod/verified-main",
                    "refs/synod/personal",
                ],
            )?;
            for rev in std::str::from_utf8(&commits)
                .map_err(io::Error::other)?
                .lines()
                .collect::<BTreeSet<_>>()
            {
                paths.extend(Self::tracked(repo, rev)?);
            }
        }
        Ok(paths)
    }
    pub(crate) fn tracked(repo: &Path, rev: &str) -> io::Result<BTreeSet<PathBuf>> {
        let bytes = git(
            repo,
            &["ls-tree", "-rz", "--name-only", rev, "--", "workspace"],
        )?;
        bytes
            .split(|b| *b == 0)
            .filter(|b| !b.is_empty())
            .map(|b| {
                let path = std::str::from_utf8(b).map_err(io::Error::other)?;
                Ok(PathBuf::from(path.strip_prefix("workspace/").ok_or_else(
                    || io::Error::other("Invalid tracked workspace path"),
                )?))
            })
            .collect()
    }
}
