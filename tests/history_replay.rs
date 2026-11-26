//! Exercise the checked-in team history as actual Git objects.
use serde::Deserialize;
use std::{
    fs,
    io::{self, Write},
    path::Path,
    process::{Command, Stdio},
};
use synod::{adapters::git::operations, application::setup::NewTeam};

#[derive(Deserialize)]
struct History {
    step: Vec<Step>,
}
#[derive(Deserialize)]
struct Step {
    branch: String,
    write: Vec<String>,
    message: String,
    merge_from: Option<String>,
}

fn git(dir: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output()?;
    if !out.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&out.stderr).into_owned(),
        ));
    }
    Ok(out.stdout)
}

#[test]
fn first_parent_replay_verifies_fixture_and_rejects_forged_actor() -> io::Result<()> {
    let temp = std::env::temp_dir().join(format!("synod-history-fixture-{}", std::process::id()));
    fs::create_dir(&temp)?;
    let repo = temp.join("repo");
    fs::create_dir(&repo)?;
    git(&repo, &["init", "-b", "main"])?;
    git(&repo, &["config", "user.name", "Fixture"])?;
    git(&repo, &["config", "user.email", "fixture@localhost"])?;
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1/team");
    let history: History =
        toml::from_str(include_str!("fixtures/v1/history.toml")).map_err(io::Error::other)?;
    for (index, step) in history.step.into_iter().enumerate() {
        if step.branch.starts_with("drafts/") {
            git(&repo, &["switch", "-c", &step.branch, "main"])?;
        } else if index > 0 {
            git(&repo, &["switch", "main"])?;
        }
        if let Some(branch) = step.merge_from {
            git(&repo, &["merge", "--no-ff", "--no-commit", &branch])?;
        }
        for path in step.write {
            let dest = repo.join(&path);
            fs::create_dir_all(dest.parent().expect("fixture parent"))?;
            fs::copy(fixture.join(&path), dest)?;
            git(&repo, &["add", "--", &path])?;
        }
        let mut proc = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["commit", "--cleanup=verbatim", "-F", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()?;
        proc.stdin
            .take()
            .unwrap()
            .write_all(step.message.as_bytes())?;
        assert!(proc.wait()?.success());
    }
    let team: NewTeam = serde_json::from_slice(&fs::read(fixture.join("team.json"))?)?;
    let remote = repo.to_str().expect("fixture path");
    let state = temp.join("state");
    let letters = operations::published_letters(&state, &team, remote)?;
    assert_eq!(letters.len(), 2);
    assert!(letters.iter().all(|l| l.completed));
    let figure = letters.iter().find(|l| l.id == "letter-042").unwrap();
    assert_eq!(figure.cc, ["member-bob"]);
    assert_eq!(figure.dependencies, ["letter-041"]);
    assert_eq!(figure.events[1].actor_id, "member-alice");
    let p = repo.join("letters/letter-042/events/evt-018.json");
    let data = fs::read_to_string(&p)?.replace("member-alice", "member-bob");
    fs::write(p, data)?;
    git(&repo, &["add", "--all"])?;
    git(&repo, &["commit", "-m", "malformed history"])?;
    assert!(operations::published_letters(&state, &team, remote).is_err());
    fs::remove_dir_all(temp)
}
