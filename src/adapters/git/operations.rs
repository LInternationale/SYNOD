//! Access to the existing team's manifest without checking out user files.
use crate::application::setup::NewTeam;
use std::{io, path::Path, process::Command};

pub fn fetch_manifest(repo: &str, cache: &Path) -> io::Result<NewTeam> {
    let mut init = Command::new("git");
    init.args(["init", "--bare"]).arg(cache);
    run(init)?;
    let mut fetch = Command::new("git");
    fetch.arg("--git-dir").arg(cache).args([
        "-c",
        "protocol.allow=never",
        "-c",
        "protocol.https.allow=always",
        "fetch",
        "--no-tags",
        "--",
        repo,
        "refs/heads/main",
    ]);
    run(fetch)?;
    let mut show = Command::new("git");
    show.arg("--git-dir")
        .arg(cache)
        .args(["show", "FETCH_HEAD:team.json"]);
    let team: NewTeam = serde_json::from_slice(&run(show)?)?;
    team.validate()?;
    Ok(team)
}
fn run(mut cmd: Command) -> io::Result<Vec<u8>> {
    let out = cmd.env("GIT_TERMINAL_PROMPT", "0").output()?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(io::Error::other(
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ))
    }
}

/// Frozen initial team publication retained until remote confirmation.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct InitialCommit {
    pub actor: String,
    pub timestamp: String,
    pub operation: String,
}

pub fn freeze_initial(actor: &str) -> io::Result<InitialCommit> {
    let out = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()?;
    if !out.status.success() {
        return Err(io::Error::other("Unable to read UTC timestamp"));
    }
    let timestamp = String::from_utf8(out.stdout)
        .map_err(io::Error::other)?
        .trim()
        .to_owned();
    Ok(InitialCommit {
        actor: actor.into(),
        timestamp,
        operation: "team-initial-configuration".into(),
    })
}

fn git(dir: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    run(cmd)
}

fn verify_initial(dir: &Path, sha: &str, manifest: &[u8], message: &str) -> io::Result<()> {
    let parents = git(dir, &["rev-list", "--parents", "-1", sha])?;
    let tree = git(dir, &["ls-tree", "--name-only", sha])?;
    let object = git(dir, &["cat-file", "commit", sha])?;
    let body = object
        .windows(2)
        .position(|b| b == b"\n\n")
        .map(|i| &object[i + 2..]);
    if parents
        .split(|b| b.is_ascii_whitespace())
        .filter(|b| !b.is_empty())
        .count()
        != 1
        || tree != b"team.json\n"
        || git(dir, &["show", &format!("{sha}:team.json")])? != manifest
        || body != Some(message.as_bytes())
    {
        return Err(io::Error::other(
            "Initial team commit differs from the frozen setup",
        ));
    }
    Ok(())
}

pub fn publish_initial(
    root: &Path,
    remote: &str,
    team: &NewTeam,
    intent: &InitialCommit,
) -> io::Result<()> {
    use std::{fs, io::Write, process::Stdio};
    team.validate()?;
    let dir = root.join("teams").join(team.id()).join("worktrees");
    fs::create_dir_all(&dir)?;
    let mut attempt = 0;
    let work = loop {
        let path = dir.join(format!("initial-{attempt}"));
        match fs::create_dir(&path) {
            Ok(()) => break path,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => attempt += 1,
            Err(e) => return Err(e),
        }
    };
    let manifest = serde_json::to_vec_pretty(team)?;
    let message = format!(
        "kind = \"team.configured\"\n\nschema_version = 1\nteam_id = {}\noperation_id = {}\nactor_id = {}\noccurred_at = {}\n",
        toml::Value::String(team.id().into()),
        toml::Value::String(intent.operation.clone()),
        toml::Value::String(intent.actor.clone()),
        intent.timestamp
    );
    super::commit_message::decode(&message).map_err(|e| io::Error::other(format!("{e:?}")))?;
    git(&work, &["init", "-b", "main"])?;
    let listed = git(&work, &["ls-remote", "--", remote, "refs/heads/main"])?;
    if listed.is_empty() {
        fs::write(work.join("team.json"), &manifest)?;
        git(&work, &["add", "--", "team.json"])?;
        let mut child = Command::new("git")
            .arg("-C")
            .arg(&work)
            .args([
                "-c",
                &format!("user.name={}", intent.actor),
                "-c",
                "user.email=synod@localhost",
                "commit",
                "--cleanup=verbatim",
                "-F",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("Missing commit input"))?
            .write_all(message.as_bytes())?;
        let out = child.wait_with_output()?;
        if !out.status.success() {
            return Err(io::Error::other(
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ));
        }
        let oid = git(&work, &["rev-parse", "HEAD"])?;
        let sha = std::str::from_utf8(&oid).map_err(io::Error::other)?.trim();
        verify_initial(&work, sha, &manifest, &message)?;
        git(&work, &["push", "--", remote, "HEAD:refs/heads/main"])?;
    }
    git(
        &work,
        &["fetch", "--no-tags", "--", remote, "refs/heads/main"],
    )?;
    let roots = git(
        &work,
        &[
            "rev-list",
            "--first-parent",
            "--max-parents=0",
            "FETCH_HEAD",
        ],
    )?;
    let root = std::str::from_utf8(&roots)
        .map_err(io::Error::other)?
        .trim();
    verify_initial(&work, root, &manifest, &message)
}
