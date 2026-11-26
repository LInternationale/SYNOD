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

pub(super) fn git(dir: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).arg("--literal-pathspecs").args(args);
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
        "kind = \"team.configured\"\n\nschema_version = 2\nteam_id = {}\noperation_id = {}\nactor_id = {}\noccurred_at = {}\n",
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

/// Read published originals at one fetched main revision. Reject incomplete or mismatched originals.
pub fn published_letters(
    root: &Path,
    team: &NewTeam,
    remote: &str,
) -> io::Result<Vec<crate::application::letters::PublishedLetter>> {
    published_snapshot(root, team, remote).map(|(_, letters)| letters)
}
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct VerifiedBoard {
    pub remote: String,
    pub revision: String,
    pub team: NewTeam,
    pub letters: Vec<crate::application::letters::PublishedLetter>,
}
pub(crate) fn cached_board(
    root: &Path,
    team: &NewTeam,
    remote: &str,
) -> io::Result<Option<VerifiedBoard>> {
    let path = root
        .join("teams")
        .join(team.id())
        .join("verified-board.json");
    match std::fs::File::open(path) {
        Ok(f) => Ok(serde_json::from_reader::<_, VerifiedBoard>(f)
            .ok()
            .filter(|b| b.remote == remote && b.team.id() == team.id())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn published_snapshot(
    root: &Path,
    team: &NewTeam,
    remote: &str,
) -> io::Result<(NewTeam, Vec<crate::application::letters::PublishedLetter>)> {
    use crate::application::letters::PublishedLetter;
    #[derive(serde::Deserialize)]
    struct Envelope {
        letter_id: String,
        title: String,
        from: String,
        to: String,
        #[serde(default)]
        cc: Vec<String>,
        #[serde(default)]
        references: Vec<String>,
        #[serde(default)]
        initial_dependencies: Vec<String>,
    }
    let cache = root.join("teams").join(team.id()).join("repo.git");
    std::fs::create_dir_all(&cache)?;
    git(&cache, &["init", "--bare"])?;
    git(
        &cache,
        &["fetch", "--no-tags", "--", remote, "refs/heads/main"],
    )?;
    let revision = String::from_utf8(git(&cache, &["rev-parse", "FETCH_HEAD"])?)
        .map_err(io::Error::other)?
        .trim()
        .to_owned();
    if let Some(board) = cached_board(root, team, remote)?
        && board.revision == revision
    {
        return Ok((board.team, board.letters));
    }
    let actual: NewTeam = serde_json::from_slice(&git(&cache, &["show", "FETCH_HEAD:team.json"])?)?;
    actual.validate()?;
    if actual.id() != team.id() {
        return Err(io::Error::other("Repository team identity has changed"));
    }
    let mut events = super::history::replay(&cache, team.id())?;
    let paths = git(
        &cache,
        &[
            "ls-tree",
            "-r",
            "--name-only",
            "-z",
            "FETCH_HEAD",
            "--",
            "letters/",
        ],
    )?;
    let mut result = Vec::new();
    for path in paths.split(|b| *b == 0) {
        let path = std::str::from_utf8(path).map_err(io::Error::other)?;
        let Some(id) = path
            .strip_prefix("letters/")
            .and_then(|s| s.strip_suffix("/envelope.json"))
        else {
            continue;
        };
        if id.is_empty()
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err(io::Error::other("Invalid published letter path"));
        }
        let env: Envelope =
            serde_json::from_slice(&git(&cache, &["show", &format!("FETCH_HEAD:{path}")])?)?;
        if env.letter_id != id {
            return Err(io::Error::other("Envelope and letter path disagree"));
        }
        let body = String::from_utf8(git(
            &cache,
            &["show", &format!("FETCH_HEAD:letters/{id}/body.md")],
        )?)
        .map_err(io::Error::other)?;
        let facts = events
            .remove(id)
            .ok_or_else(|| io::Error::other("Published letter has no verified send event"))?;
        let mut dependencies = env.initial_dependencies;
        let mut completed = false;
        let mut checklist = crate::application::letters::checklist(&body);
        for ev in &facts {
            match ev.kind.as_str() {
                "ChecklistSet" => {
                    let change = ev
                        .checklist_change
                        .as_ref()
                        .ok_or_else(|| io::Error::other("Missing checklist change"))?;
                    checklist
                        .iter_mut()
                        .find(|item| item.line == change.line)
                        .ok_or_else(|| io::Error::other("Unknown checklist line"))?
                        .checked = change.checked;
                }
                "MarkedCompleted" => completed = true,
                "MarkedIncomplete" => completed = false,
                "SubtasksSet" => {
                    dependencies = ev
                        .subtasks
                        .clone()
                        .ok_or_else(|| io::Error::other("Missing subtasks"))?
                }
                "DependencyAdded" => {
                    let dep = ev
                        .dependency
                        .clone()
                        .ok_or_else(|| io::Error::other("Missing dependency"))?;
                    if !dependencies.contains(&dep) {
                        dependencies.push(dep)
                    }
                }
                "DependencyRemoved" => dependencies.retain(|d| Some(d) != ev.dependency.as_ref()),
                _ => {}
            }
        }
        result.push(PublishedLetter {
            checklist,
            events: facts,
            to: env.to,
            cc: env.cc,
            references: env.references,
            dependencies,
            unread: false,
            progress: 0,
            progress_done: 0,
            progress_total: 0,
            completed,
            id: env.letter_id,
            title: env.title,
            author: env.from,
            body,
        });
    }
    crate::application::letters::project_progress(&mut result)?;
    let board = VerifiedBoard {
        remote: remote.into(),
        revision,
        team: actual,
        letters: result,
    };
    git(
        &cache,
        &["update-ref", "refs/synod/verified-main", &board.revision],
    )?;
    let dir = root.join("teams").join(team.id());
    let temp = dir.join("verified-board.tmp");
    let file = std::fs::File::create(&temp)?;
    serde_json::to_writer(&file, &board)?;
    file.sync_all()?;
    std::fs::rename(temp, dir.join("verified-board.json"))?;
    Ok((board.team, board.letters))
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct FrozenSend {
    pub key: String,
    pub timestamp: String,
    pub actor: String,
    pub team: String,
    pub draft: crate::application::letters::Draft,
}

impl FrozenSend {
    pub(super) fn letter(&self) -> String {
        self.draft
            .cloud
            .as_ref()
            .map(|c| c.letter.clone())
            .unwrap_or_else(|| format!("letter-{}", self.key))
    }

    pub fn new(
        actor: &str,
        team: &str,
        draft: crate::application::letters::Draft,
    ) -> io::Result<Self> {
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?;
        Ok(Self {
            key: format!("{}-{}", time.as_nanos(), std::process::id()),
            timestamp: freeze_initial(actor)?.timestamp,
            actor: actor.into(),
            team: team.into(),
            draft,
        })
    }
}

pub(super) fn commit_message(kind: &str, send: &FrozenSend, target: &str) -> io::Result<String> {
    let quoted = |s: &str| toml::Value::String(s.to_owned()).to_string();
    let msg = format!(
        "kind = {}\n\nschema_version = 2\nteam_id = {}\noperation_id = {}\nactor_id = {}\noccurred_at = {}\n\n[target]\n{target}\n",
        quoted(kind),
        quoted(&send.team),
        quoted(&format!("{kind}-{}", send.key)),
        quoted(&send.actor),
        send.timestamp
    );
    super::commit_message::decode(&msg).map_err(|e| io::Error::other(format!("{e:?}")))?;
    Ok(msg)
}
pub(super) fn commit(dir: &Path, msg: &str) -> io::Result<()> {
    use std::{io::Write, process::Stdio};
    let mut process = Command::new("git")
        .arg("-C")
        .arg(dir)
        .arg("commit")
        .args(
            msg.starts_with("kind = \"letter.event\"")
                .then_some("--allow-empty"),
        )
        .args(["--cleanup=verbatim", "-F", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    process
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("Missing commit input"))?
        .write_all(msg.as_bytes())?;
    let out = process.wait_with_output()?;
    if !out.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ));
    }
    let object = git(dir, &["cat-file", "commit", "HEAD"])?;
    if !object
        .windows(2)
        .position(|b| b == b"\n\n")
        .is_some_and(|p| &object[p + 2..] == msg.as_bytes())
    {
        return Err(io::Error::other("Commit message was modified"));
    }
    Ok(())
}
pub(super) fn originals(s: &FrozenSend) -> io::Result<std::collections::BTreeMap<String, Vec<u8>>> {
    use std::collections::BTreeMap;
    let mut files = BTreeMap::new();
    let mut attachments = Vec::new();
    for (index, a) in s.draft.attachments.iter().enumerate() {
        let id = format!("asset-{}", index + 1);
        attachments.push(serde_json::json!({"attachment_id":id,"filename":a.filename}));
        files.insert(format!("attachments/{id}"), a.bytes.clone());
    }
    let cc: Vec<_> = s.draft.cc.split(',').filter(|v| !v.is_empty()).collect();
    let env = serde_json::json!({"letter_id":s.letter(),"from":s.actor,"to":s.draft.to,"cc":cc,"title":s.draft.title,"references":s.draft.references,"initial_dependencies":s.draft.dependencies,"attachments":attachments});
    files.insert("envelope.json".into(), serde_json::to_vec_pretty(&env)?);
    files.insert("body.md".into(), s.draft.body.as_bytes().to_vec());
    Ok(files)
}

/// Publish the frozen draft using only ordinary fast-forward ref updates.
pub fn send_letter(root: &Path, team: &NewTeam, remote: &str, s: &FrozenSend) -> io::Result<()> {
    use std::fs;
    team.validate()?;
    if team.id() != s.team
        || !(team.human(&s.actor) || team.assistant(&s.actor))
        || !s.key.bytes().all(|b| b.is_ascii_digit() || b == b'-')
    {
        return Err(io::Error::other("Invalid frozen send identity"));
    }
    let known = |id: &str| team.human(id) || team.assistant(id);
    if s.draft.title.trim().is_empty()
        || !known(&s.draft.to)
        || !s.draft.cc.split(',').filter(|v| !v.is_empty()).all(known)
    {
        return Err(io::Error::other("Select a recipient and enter a title"));
    }
    let letter = s.letter();
    if let Some(c) = &s.draft.cloud {
        super::drafts::validate(c)?;
    }
    let event = format!("evt-{}", s.key);
    let prefix = format!("letters/{letter}");
    let copy = s
        .draft
        .cloud
        .as_ref()
        .map_or("primary", |c| c.copy.as_str());
    let branch = format!("refs/heads/drafts/{}/{letter}/{copy}", s.actor);
    let files = originals(s)?;
    let draft_msg = commit_message(
        "draft.saved",
        s,
        &format!("letter_id = \"{letter}\"\ncopy_id = \"{copy}\""),
    )?;
    let sent_msg = commit_message(
        "letter.sent",
        s,
        &format!("letter_id = \"{letter}\"\nevent_id = \"{event}\""),
    )?;
    for round in 0..3 {
        let board = published_letters(root, team, remote)?;
        let cache = root.join("teams").join(team.id()).join("repo.git");
        if let Some(existing) = board.iter().find(|l| l.id == letter) {
            if !existing.events.iter().any(|e| {
                e.kind == "LetterSent"
                    && e.event_id == event
                    && e.actor_id == s.actor
                    && e.occurred_at == s.timestamp
            }) {
                return Err(io::Error::other(
                    "Published operation differs from frozen send",
                ));
            }
            for (path, bytes) in &files {
                if git(&cache, &["show", &format!("FETCH_HEAD:{prefix}/{path}")])? != *bytes {
                    return Err(io::Error::other(
                        "Published content differs from frozen send",
                    ));
                }
            }
            return Ok(());
        }
        if board.iter().any(|l| l.title.trim() == s.draft.title.trim()) {
            return Err(io::Error::other("队伍公告板已有同名信件，草稿已保留"));
        }
        if !s
            .draft
            .references
            .iter()
            .chain(&s.draft.dependencies)
            .all(|id| board.iter().any(|l| l.id == *id))
        {
            return Err(io::Error::other("Referenced letter is missing from main"));
        }
        let base = git(&cache, &["rev-parse", "FETCH_HEAD"])?;
        let base = std::str::from_utf8(&base).map_err(io::Error::other)?.trim();
        let dir = root
            .join("teams")
            .join(team.id())
            .join("worktrees")
            .join(format!(
                "send-{}-{}-{round}",
                s.key,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(io::Error::other)?
                    .as_nanos()
            ));
        fs::create_dir_all(&dir)?;
        git(&dir, &["init", "-b", "candidate"])?;
        git(&dir, &["config", "user.name", &s.actor])?;
        git(&dir, &["config", "user.email", "synod@localhost"])?;
        git(
            &dir,
            &["fetch", "--no-tags", "--", remote, "refs/heads/main"],
        )?;
        if git(&dir, &["rev-parse", "FETCH_HEAD"])? != format!("{base}\n").as_bytes() {
            continue;
        }
        git(&dir, &["switch", "--detach", base])?;
        let draft_oid = if git(&dir, &["ls-remote", "--", remote, &branch])?.is_empty() {
            for (path, bytes) in &files {
                let dest = dir.join(&prefix).join(path);
                fs::create_dir_all(
                    dest.parent()
                        .ok_or_else(|| io::Error::other("Missing parent"))?,
                )?;
                fs::write(dest, bytes)?
            }
            git(&dir, &["add", "--", &prefix])?;
            commit(&dir, &draft_msg)?;
            git(&dir, &["push", "--", remote, &format!("HEAD:{branch}")])?;
            git(&dir, &["rev-parse", "HEAD"])?
        } else {
            git(&dir, &["fetch", "--no-tags", "--", remote, &branch])?;
            let observed = s
                .draft
                .cloud
                .as_ref()
                .map_or("FETCH_HEAD", |c| c.revision.as_str());
            for (path, bytes) in &files {
                if git(&dir, &["show", &format!("{observed}:{prefix}/{path}")])? != *bytes {
                    return Err(io::Error::other("Cloud draft differs from frozen send"));
                }
            }
            git(&dir, &["rev-parse", observed])?
        };
        let draft_oid = std::str::from_utf8(&draft_oid)
            .map_err(io::Error::other)?
            .trim();
        git(&dir, &["switch", "--detach", base])?;
        git(&dir, &["merge", "--no-ff", "--no-commit", draft_oid])?;
        let changes = git(&dir, &["diff", "--cached", "--name-only", "-z", base])?;
        let changed: std::collections::BTreeSet<_> = changes
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        let expected = files.keys().map(|p| format!("{prefix}/{p}")).collect();
        if changed != expected {
            return Err(io::Error::other("Unexpected candidate paths"));
        }
        commit(&dir, &sent_msg)?;
        let parents = git(&dir, &["show", "-s", "--format=%P", "HEAD"])?;
        if parents != format!("{base} {draft_oid}\n").as_bytes() {
            return Err(io::Error::other("Invalid publication parents"));
        }
        match git(&dir, &["push", "--", remote, "HEAD:refs/heads/main"]) {
            Ok(_) => {
                let confirmed = published_letters(root, team, remote)?;
                if confirmed.iter().any(|l| l.id == letter) {
                    return Ok(());
                }
            }
            Err(e) if round == 2 => return Err(e),
            Err(_) => {}
        }
    }
    Err(io::Error::other("Remote branch advanced; retry this send"))
}

pub(super) fn copy_workspace(from: &Path, to: &Path) -> io::Result<()> {
    use std::fs;
    if !fs::symlink_metadata(from)?.is_dir() {
        return Err(io::Error::other("Select an ordinary workspace directory"));
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().eq_ignore_ascii_case(".git")
            || name.to_string_lossy().starts_with(".synod-sync-")
        {
            return Err(io::Error::other("Nested Git metadata in workspace"));
        }
        let target = to.join(name);
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_workspace(&entry.path(), &target)?
        } else if ty.is_file() {
            fs::copy(entry.path(), target)?;
        } else {
            return Err(io::Error::other(
                "Workspace contains a symbolic link or special file",
            ));
        }
    }
    Ok(())
}
fn work_msg(kind: &str, team: &str, actor: &str, space: &str) -> io::Result<String> {
    let timestamp = freeze_initial(actor)?.timestamp;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let space = toml::Value::String(space.into());
    let msg = format!(
        "kind = \"{kind}\"\n\nschema_version = 2\nteam_id = \"{team}\"\noperation_id = \"work-{unique}-{}\"\nactor_id = \"{actor}\"\noccurred_at = {timestamp}\n\n[target]\nspace_id = {space}\n",
        std::process::id()
    );
    super::commit_message::decode(&msg).map_err(|e| io::Error::other(format!("{e:?}")))?;
    Ok(msg)
}
fn check_staged_scope(dir: &Path, space: &str) -> io::Result<()> {
    let diff = git(
        dir,
        &[
            "diff",
            "--cached",
            "--no-renames",
            "--name-only",
            "-z",
            "HEAD",
        ],
    )?;
    let prefix = format!("workspace/{space}/");
    if !diff
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .all(|p| p.starts_with(prefix.as_bytes()))
    {
        return Err(io::Error::other(
            "Candidate changes outside selected workspace",
        ));
    }
    Ok(())
}

pub fn workspace_action(
    state: &Path,
    work: &Path,
    team: &NewTeam,
    remote: &str,
    actor: &str,
    space: &str,
    action: crate::session::protocol::WorkAction,
) -> io::Result<crate::session::protocol::WorkResult> {
    use crate::session::protocol::{WorkAction as A, WorkResult as R};
    use std::fs;
    team.validate()?;
    if !team.human(actor) || !crate::adapters::local::tags::load(state, team)?.contains_key(space) {
        return Err(io::Error::other(
            "Select a human identity and a team workspace",
        ));
    }
    let base = state.join("teams").join(team.id());
    let key = crate::adapters::local::tags::key(space);
    let local = crate::adapters::local::tags::directory(work, space)?;
    let area = base.join("spaces").join(&key).join(actor);
    fs::create_dir_all(&area)?;
    let marker = area.join("merge-attempt");
    let prefix = format!("workspace/{space}");
    let personal = format!("refs/heads/work/{actor}/{key}");
    let attempts = base.join("worktrees");
    let publish = |dir: &Path| -> io::Result<R> {
        check_staged_scope(dir, space)?;
        if !git(dir, &["ls-files", "-u", "-z"])?.is_empty() {
            return Ok(R::Conflict);
        }
        commit(
            dir,
            &work_msg("workspace.published", team.id(), actor, space)?,
        )?;
        git(dir, &["push", "--", remote, "HEAD:refs/heads/main"])?;
        Ok(R::Published)
    };
    if matches!(action, A::Defer) {
        if marker.exists() {
            fs::remove_file(marker)?
        }
        return Ok(R::Deferred);
    }
    if matches!(action, A::Resolve | A::ResolveOnTerminal(_)) {
        let attempt = fs::read_to_string(&marker)?;
        if !attempt.starts_with("work-")
            || !attempt
                .bytes()
                .all(|b| b.is_ascii_digit() || b"work-".contains(&b))
        {
            return Err(io::Error::other("Invalid merge attempt"));
        }
        let dir = attempts.join(attempt);
        if git(&dir, &["config", "--get", "merge.tool"]).is_err() {
            return Err(io::Error::other(
                "Configure Git merge.tool before resolving conflicts",
            ));
        }
        if let A::ResolveOnTerminal(path) = action {
            #[cfg(unix)]
            {
                use std::{io::IsTerminal, os::unix::fs::MetadataExt};
                let path = Path::new(&path).canonicalize()?;
                if !path.starts_with("/dev")
                    || fs::metadata(&path)?.uid() != fs::metadata(&marker)?.uid()
                {
                    return Err(io::Error::other("Invalid terminal device"));
                }
                let tty = fs::OpenOptions::new().read(true).write(true).open(path)?;
                if !tty.is_terminal() {
                    return Err(io::Error::other("Expected a terminal device"));
                }
                let status = Command::new("git")
                    .arg("-C")
                    .arg(&dir)
                    .args(["mergetool", "--no-prompt", "--", &prefix])
                    .stdin(tty.try_clone()?)
                    .stdout(tty.try_clone()?)
                    .stderr(tty)
                    .status()?;
                if !status.success() {
                    return Err(io::Error::other(
                        "Merge tool exited without resolving the conflict",
                    ));
                }
            }
            #[cfg(not(unix))]
            return Err(io::Error::other("Terminal handoff unavailable"));
        } else {
            git(&dir, &["mergetool", "--no-prompt", "--", &prefix])?;
        }
        let result = publish(&dir)?;
        if result == R::Published {
            fs::remove_file(marker)?
        }
        return Ok(result);
    }
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let attempt = format!("work-{unique}-{}", std::process::id());
    let dir = attempts.join(&attempt);
    fs::create_dir_all(&dir)?;
    git(&dir, &["init"])?;
    git(&dir, &["config", "user.name", actor])?;
    git(&dir, &["config", "user.email", "synod@localhost"])?;
    git(
        &dir,
        &[
            "fetch",
            "--no-tags",
            "--",
            remote,
            "refs/heads/main:refs/heads/published",
        ],
    )?;
    let manifest: NewTeam = serde_json::from_slice(&git(&dir, &["show", "published:team.json"])?)?;
    manifest.validate()?;
    if manifest.id() != team.id() {
        return Err(io::Error::other("Remote team does not match this client"));
    }
    let has_personal = !git(&dir, &["ls-remote", "--", remote, &personal])?.is_empty();
    if has_personal {
        git(
            &dir,
            &[
                "fetch",
                "--no-tags",
                "--",
                remote,
                &format!("{personal}:refs/heads/personal"),
            ],
        )?;
    }
    git(
        &dir,
        &[
            "switch",
            "--detach",
            if has_personal {
                "personal"
            } else {
                "published"
            },
        ],
    )?;
    let sub = crate::adapters::local::tags::directory(&dir, &prefix)?;
    let parent = dir.join("workspace");
    if parent.exists() && !fs::symlink_metadata(&parent)?.is_dir() {
        return Err(io::Error::other("Invalid remote workspace root"));
    }
    if let Ok(meta) = fs::symlink_metadata(&sub) {
        if !meta.is_dir() {
            return Err(io::Error::other("Invalid remote subworkspace"));
        }
        fs::remove_dir_all(&sub)?;
    }
    copy_workspace(&local, &sub)?;
    if !git(&dir, &["ls-files", "-z", "--", &prefix])?.is_empty()
        || fs::read_dir(&sub)?.next().is_some()
    {
        git(&dir, &["add", "--all", "--force", "--", &prefix])?;
    }
    check_staged_scope(&dir, space)?;
    let dirty = !git(&dir, &["diff", "--cached", "--name-only", "-z", "HEAD"])?.is_empty();
    if matches!(action, A::Submit) && (dirty || !has_personal) {
        return Ok(R::Dirty);
    }
    if dirty {
        commit(&dir, &work_msg("workspace.saved", team.id(), actor, space)?)?;
        git(&dir, &["push", "--", remote, &format!("HEAD:{personal}")])?;
    } else if !has_personal && matches!(action, A::Save | A::SaveAndSubmit) {
        git(&dir, &["push", "--", remote, &format!("HEAD:{personal}")])?;
    }
    if matches!(action, A::Save) {
        return Ok(R::Saved);
    }
    let oid = git(&dir, &["rev-parse", "HEAD"])?;
    let oid = std::str::from_utf8(&oid).map_err(io::Error::other)?.trim();
    git(&dir, &["switch", "--detach", "published"])?;
    if !git(&dir, &["branch", "--contains", oid, "--list", "published"])?.is_empty() {
        return Ok(R::Published);
    }
    match git(&dir, &["merge", "--no-ff", "--no-commit", oid]) {
        Ok(_) => publish(&dir),
        Err(_) if !git(&dir, &["ls-files", "-u", "-z"])?.is_empty() => {
            fs::write(marker, attempt)?;
            Ok(R::Conflict)
        }
        Err(e) => Err(e),
    }
}

/// Merge a roster addition into the latest remote manifest. Repeat attempts retain the member IDs.
pub fn register_members(
    root: &Path,
    remote: &str,
    team: &NewTeam,
    intent: &InitialCommit,
    human: &str,
    assistant: &str,
) -> io::Result<NewTeam> {
    use std::fs;
    team.validate()?;
    let dir = root
        .join("teams")
        .join(team.id())
        .join("worktrees")
        .join(format!("registration-{}", intent.operation));
    fs::create_dir_all(&dir)?;
    git(&dir, &["init"])?;
    git(&dir, &["config", "user.name", &intent.actor])?;
    git(&dir, &["config", "user.email", "synod@localhost"])?;
    for round in 0..3 {
        git(
            &dir,
            &["fetch", "--no-tags", "--", remote, "refs/heads/main"],
        )?;
        git(
            &dir,
            &["switch", "--discard-changes", "--detach", "FETCH_HEAD"],
        )?;
        let mut actual: NewTeam = serde_json::from_slice(&git(&dir, &["show", "HEAD:team.json"])?)?;
        actual.validate()?;
        if actual.id() != team.id() {
            return Err(io::Error::other("Remote team identity changed"));
        }
        let count = actual.roster().len();
        actual.register(&intent.operation, human, assistant)?;
        if actual.roster().len() == count {
            return Ok(actual);
        }
        fs::write(dir.join("team.json"), serde_json::to_vec_pretty(&actual)?)?;
        let q = |s: &str| toml::Value::String(s.into()).to_string();
        let message = format!(
            "kind = \"team.configured\"\n\nschema_version = 2\nteam_id = {}\noperation_id = {}\nactor_id = {}\noccurred_at = {}\n",
            q(team.id()),
            q(&intent.operation),
            q(&intent.actor),
            intent.timestamp
        );
        super::commit_message::decode(&message).map_err(|e| io::Error::other(format!("{e:?}")))?;
        git(&dir, &["add", "--", "team.json"])?;
        commit(&dir, &message)?;
        if git(&dir, &["diff", "--name-only", "HEAD^", "HEAD"])?.as_slice() != b"team.json\n" {
            return Err(io::Error::other("Unexpected registration paths"));
        }
        match git(&dir, &["push", "--", remote, "HEAD:refs/heads/main"]) {
            Ok(_) => return Ok(actual),
            Err(e) if round == 2 => return Err(e),
            Err(_) => {}
        }
    }
    Err(io::Error::other("Concurrent roster update; retry"))
}

/// Publish an immutable task change against the most recent team state.
pub fn change_task(
    root: &Path,
    team: &NewTeam,
    remote: &str,
    target: &str,
    intent: &InitialCommit,
    change: &crate::application::letters::TaskChange,
) -> io::Result<()> {
    use crate::application::letters::{LetterEvent, TaskChange};
    use std::fs;
    fn safe(s: &str) -> bool {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    }
    if !safe(target)
        || !safe(&intent.operation)
        || !(team.human(&intent.actor) || team.assistant(&intent.actor))
    {
        return Err(io::Error::other("Invalid task change"));
    }
    let event_id = format!("evt-{}", intent.operation);

    let (kind, subtasks) = match change {
        TaskChange::Checklist(_) => ("ChecklistSet", None),
        TaskChange::Completion(true) => ("MarkedCompleted", None),
        TaskChange::Completion(false) => ("MarkedIncomplete", None),
        TaskChange::Subtasks { selected, .. } => ("SubtasksSet", Some(selected.clone())),
    };
    let fact = LetterEvent {
        schema_version: 1,
        event_id: event_id.clone(),
        letter_id: target.into(),
        actor_id: intent.actor.clone(),
        occurred_at: intent.timestamp.clone(),
        kind: kind.into(),
        checklist_change: match change {
            TaskChange::Checklist(item) => Some(item.clone()),
            _ => None,
        },
        percent: None,
        dependency: None,
        subtasks,
    };
    let bytes = serde_json::to_vec_pretty(&fact)?;
    let mut msg = format!(
        "kind = \"letter.event\"\n\nschema_version = 2\nteam_id = \"{}\"\noperation_id = \"{}\"\nactor_id = \"{}\"\noccurred_at = {}\n\n[target]\nletter_id = \"{target}\"\nevent_id = \"{event_id}\"\nevent_kind = \"{kind}\"\n",
        team.id(),
        intent.operation,
        intent.actor,
        intent.timestamp
    );
    if let Some(items) = &fact.subtasks {
        msg.push_str(&format!(
            "subtasks = {}\n",
            toml::Value::try_from(items).map_err(io::Error::other)?
        ));
    }
    if let Some(item) = &fact.checklist_change {
        msg.push_str(&format!(
            "[target.checklist_change]\nline = {}\nchecked = {}\n",
            item.line, item.checked
        ));
    }
    for _ in 0..3 {
        let board = published_letters(root, team, remote)?;
        let letter = board
            .iter()
            .find(|l| l.id == target)
            .ok_or_else(|| io::Error::other("Unknown letter"))?;
        if let Some(existing) = letter.events.iter().find(|e| e.event_id == event_id) {
            if serde_json::to_vec_pretty(existing)? == bytes {
                return Ok(());
            }
            return Err(io::Error::other("Task operation identity mismatch"));
        }
        match change {
            TaskChange::Checklist(item) => {
                let current = letter
                    .checklist
                    .iter()
                    .find(|i| i.line == item.line)
                    .ok_or_else(|| io::Error::other("Unknown checklist line"))?;
                if current.checked == item.checked {
                    return Ok(());
                }
            }
            TaskChange::Completion(value) if *value == letter.completed => return Ok(()),
            TaskChange::Completion(_) => {}
            TaskChange::Subtasks { selected, observed } => {
                if *selected == letter.dependencies {
                    return Ok(());
                }
                if *observed != letter.dependencies {
                    return Err(io::Error::other(
                        "子任务已被其他成员更新，请重新打开进度设置",
                    ));
                }
                let unique: std::collections::HashSet<_> = selected.iter().collect();
                if unique.len() != selected.len() {
                    return Err(io::Error::other("Duplicate subtask"));
                }
                let mut pending = selected.iter().map(String::as_str).collect::<Vec<_>>();
                let mut seen = std::collections::HashSet::new();
                while let Some(node) = pending.pop() {
                    if node == target {
                        return Err(io::Error::other("这些任务会形成循环"));
                    }
                    if seen.insert(node) {
                        let entry = board
                            .iter()
                            .find(|l| l.id == node)
                            .ok_or_else(|| io::Error::other("Unknown subtask"))?;
                        pending.extend(entry.dependencies.iter().map(String::as_str))
                    }
                }
            }
        }
        let cache = root.join("teams").join(team.id()).join("repo.git");
        let base = git(&cache, &["rev-parse", "FETCH_HEAD"])?;
        let dir = root
            .join("teams")
            .join(team.id())
            .join("worktrees")
            .join(format!("event-{}", intent.operation));
        fs::create_dir_all(&dir)?;
        git(&dir, &["init"])?;
        git(&dir, &["config", "user.name", &intent.actor])?;
        git(&dir, &["config", "user.email", "synod@localhost"])?;
        git(
            &dir,
            &["fetch", "--no-tags", "--", remote, "refs/heads/main"],
        )?;
        if git(&dir, &["rev-parse", "FETCH_HEAD"])? != base {
            continue;
        }
        git(
            &dir,
            &["switch", "--discard-changes", "--detach", "FETCH_HEAD"],
        )?;
        commit(&dir, &msg)?;
        if !git(&dir, &["diff", "--name-only", "HEAD^", "HEAD"])?.is_empty() {
            return Err(io::Error::other("Unexpected task event paths"));
        }
        if git(&dir, &["push", "--", remote, "HEAD:refs/heads/main"]).is_ok() {
            let confirmed = published_letters(root, team, remote)?;
            if confirmed
                .iter()
                .any(|l| l.id == target && l.events.iter().any(|e| e.event_id == event_id))
            {
                return Ok(());
            }
        }
    }
    Err(io::Error::other("Task publication not confirmed"))
}
