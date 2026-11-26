//! Cloud draft branches in a team's data repository.
use super::operations::{FrozenSend, commit, commit_message, git, originals};
use crate::application::{
    letters::{Attachment, CloudDraft, Draft},
    setup::NewTeam,
};
use serde::Deserialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

fn safe(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
pub(super) fn validate(c: &CloudDraft) -> io::Result<()> {
    if !safe(&c.letter)
        || !safe(&c.copy)
        || !matches!(c.revision.len(), 40 | 64)
        || !c.revision.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(io::Error::other("Invalid cloud draft coordinates"));
    }
    Ok(())
}
fn decode(bytes: Vec<u8>) -> io::Result<String> {
    String::from_utf8(bytes).map_err(io::Error::other)
}
fn repository(root: &Path, team: &NewTeam) -> io::Result<PathBuf> {
    team.validate()?;
    let cache = root.join("teams").join(team.id()).join("repo.git");
    fs::create_dir_all(&cache)?;
    git(&cache, &["init", "--bare"])?;
    Ok(cache)
}
fn branch(actor: &str, letter: &str, copy: &str) -> String {
    format!("refs/heads/drafts/{actor}/{letter}/{copy}")
}
fn tip(dir: &Path, remote: &str, reference: &str) -> io::Result<Option<String>> {
    Ok(
        decode(git(dir, &["ls-remote", "--refs", "--", remote, reference])?)?
            .split_whitespace()
            .next()
            .map(str::to_owned),
    )
}

fn load(dir: &Path, actor: &str, c: CloudDraft) -> io::Result<Draft> {
    #[derive(Deserialize)]
    struct Asset {
        attachment_id: String,
        filename: String,
    }
    #[derive(Deserialize)]
    struct Envelope {
        letter_id: String,
        from: String,
        to: String,
        title: String,
        #[serde(default)]
        cc: Vec<String>,
        #[serde(default)]
        references: Vec<String>,
        #[serde(default)]
        initial_dependencies: Vec<String>,
        attachments: Vec<Asset>,
    }
    validate(&c)?;
    let prefix = format!("{}:letters/{}/", c.revision, c.letter);
    let read = |file: &str| git(dir, &["show", &format!("{prefix}{file}")]);
    let e: Envelope = serde_json::from_slice(&read("envelope.json")?)?;
    if e.letter_id != c.letter || e.from != actor {
        return Err(io::Error::other("Cloud draft identity mismatch"));
    }
    let mut attachments = Vec::new();
    for a in e.attachments {
        if !safe(&a.attachment_id) {
            return Err(io::Error::other("Invalid cloud attachment ID"));
        }
        attachments.push(Attachment {
            filename: a.filename,
            bytes: read(&format!("attachments/{}", a.attachment_id))?,
        })
    }
    Ok(Draft {
        reference: String::new(),
        cloud: Some(c),
        to: e.to,
        title: e.title,
        cc: e.cc.join(","),
        body: decode(read("body.md")?)?,
        references: e.references,
        dependencies: e.initial_dependencies,
        attachments,
    })
}

pub fn discover(
    root: &Path,
    team: &NewTeam,
    remote: &str,
    actor: &str,
) -> io::Result<(Vec<Draft>, std::collections::BTreeSet<(String, String)>)> {
    if !team.human(actor) && !team.assistant(actor) {
        return Err(io::Error::other("Unknown draft author"));
    }
    let dir = repository(root, team)?;
    git(
        &dir,
        &["fetch", "--no-tags", "--", remote, "refs/heads/main"],
    )?;
    let manifest: NewTeam = serde_json::from_slice(&git(&dir, &["show", "FETCH_HEAD:team.json"])?)?;
    if manifest.id() != team.id() {
        return Err(io::Error::other("Remote team mismatch"));
    }
    let main = decode(git(&dir, &["rev-parse", "FETCH_HEAD"])?)?
        .trim()
        .to_owned();
    let mut sealed = std::collections::BTreeSet::new();
    let published = decode(
        git(
            &dir,
            &["ls-tree", "-d", "--name-only", "FETCH_HEAD:letters"],
        )
        .or_else(|e| {
            if git(&dir, &["ls-tree", "FETCH_HEAD", "letters"])?.is_empty() {
                Ok(vec![])
            } else {
                Err(e)
            }
        })?,
    )?;
    let prefix = format!("refs/heads/drafts/{actor}/");
    let refs = decode(git(
        &dir,
        &["ls-remote", "--refs", "--", remote, &format!("{prefix}*")],
    )?)?;
    let mut drafts = Vec::new();
    for line in refs.lines() {
        let (_, reference) = line
            .split_once('\t')
            .ok_or_else(|| io::Error::other("Invalid remote reference"))?;
        let (letter, copy) = reference
            .strip_prefix(&prefix)
            .and_then(|s| s.split_once('/'))
            .ok_or_else(|| io::Error::other("Unexpected draft reference"))?;
        if !safe(letter) || !safe(copy) {
            return Err(io::Error::other("Invalid draft reference"));
        }

        git(&dir, &["fetch", "--no-tags", "--", remote, reference])?;
        let revision = decode(git(&dir, &["rev-parse", "FETCH_HEAD"])?)?
            .trim()
            .to_owned();
        if published.lines().any(|id| id == letter) {
            let prefix = format!("letters/{letter}/");
            let difference = git(
                &dir,
                &[
                    "diff",
                    "--name-only",
                    "-z",
                    &main,
                    &revision,
                    "--",
                    &format!("{prefix}envelope.json"),
                    &format!("{prefix}body.md"),
                    &format!("{prefix}attachments"),
                ],
            )?;
            if difference.is_empty() {
                sealed.insert((letter.to_owned(), copy.to_owned()));
                continue;
            }
        }
        drafts.push(load(
            &dir,
            actor,
            CloudDraft {
                letter: letter.into(),
                copy: copy.into(),
                revision,
            },
        )?);
    }
    Ok((drafts, sealed))
}

pub fn save(root: &Path, team: &NewTeam, remote: &str, s: &FrozenSend) -> io::Result<CloudDraft> {
    team.validate()?;
    if team.id() != s.team || !safe(&s.key) || (!team.human(&s.actor) && !team.assistant(&s.actor))
    {
        return Err(io::Error::other("Invalid draft save identity"));
    }
    if let Some(c) = &s.draft.cloud {
        validate(c)?
    }
    let letter = s.letter();
    let original = s.draft.cloud.as_ref();
    let primary = original.map_or("primary", |c| c.copy.as_str());
    let fork = format!("copy-{}", s.key);
    let dir = root
        .join("teams")
        .join(team.id())
        .join("worktrees")
        .join(format!("save-{}", s.key));
    fs::create_dir_all(&dir)?;
    git(&dir, &["init"])?;
    git(&dir, &["config", "user.name", &s.actor])?;
    git(&dir, &["config", "user.email", "synod@localhost"])?;
    git(
        &dir,
        &["fetch", "--no-tags", "--", remote, "refs/heads/main"],
    )?;
    let main = decode(git(&dir, &["rev-parse", "FETCH_HEAD"])?)?
        .trim()
        .to_owned();
    let manifest: NewTeam = serde_json::from_slice(&git(&dir, &["show", "FETCH_HEAD:team.json"])?)?;
    if manifest.id() != team.id() {
        return Err(io::Error::other("Remote team mismatch"));
    }
    let files = originals(s)?;
    for copy in [primary, fork.as_str()] {
        let reference = branch(&s.actor, &letter, copy);
        let expected = if copy == primary {
            original.map(|c| c.revision.as_str())
        } else {
            None
        };
        let mut observed = tip(&dir, remote, &reference)?;
        let msg = commit_message(
            "draft.saved",
            s,
            &format!("letter_id = \"{letter}\"\ncopy_id = \"{copy}\""),
        )?;
        if observed.is_some() {
            git(&dir, &["fetch", "--no-tags", "--", remote, &reference])?;
            let oid = decode(git(&dir, &["rev-parse", "FETCH_HEAD"])?)?
                .trim()
                .to_owned();
            let c = CloudDraft {
                letter: letter.clone(),
                copy: copy.into(),
                revision: oid.clone(),
            };
            let mut existing = load(&dir, &s.actor, c.clone())?;
            existing.reference = s.draft.reference.clone();
            existing.cloud = s.draft.cloud.clone();
            if existing == s.draft {
                return Ok(c);
            }
            observed = Some(oid);
        }
        if observed.as_deref() != expected {
            continue;
        }
        let base = original.map(|c| c.revision.as_str()).unwrap_or(&main);
        if git(&dir, &["cat-file", "-e", base]).is_err() {
            git(&dir, &["fetch", "--no-tags", "--", remote, base])?;
        }
        git(&dir, &["switch", "--discard-changes", "--detach", base])?;
        let parent = dir.join("letters");
        if parent.symlink_metadata().is_ok_and(|m| !m.is_dir()) {
            return Err(io::Error::other("Invalid letters directory"));
        }
        let path = parent.join(&letter);
        if let Ok(m) = path.symlink_metadata() {
            if !m.is_dir() {
                return Err(io::Error::other("Invalid draft directory"));
            }
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(path.join("attachments"))?;
        for (name, bytes) in &files {
            fs::write(path.join(name), bytes)?
        }
        git(
            &dir,
            &[
                "add",
                "--all",
                "--force",
                "--",
                &format!("letters/{letter}"),
            ],
        )?;
        if !git(&dir, &["diff", "--cached", "--name-only", "HEAD"])?.is_empty() {
            commit(&dir, &msg)?;
        }
        let oid = decode(git(&dir, &["rev-parse", "HEAD"])?)?
            .trim()
            .to_owned();
        match git(&dir, &["push", "--", remote, &format!("HEAD:{reference}")]) {
            Ok(_) => {
                if tip(&dir, remote, &reference)?.as_deref() == Some(&oid) {
                    return Ok(CloudDraft {
                        letter,
                        copy: copy.into(),
                        revision: oid,
                    });
                }
                return Err(io::Error::other("Draft confirmation changed; retry"));
            }
            Err(e) if tip(&dir, remote, &reference)? == observed => return Err(e),
            Err(_) => continue,
        }
    }
    Err(io::Error::other("Conflicting cloud draft revisions"))
}
