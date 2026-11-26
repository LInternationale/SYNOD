#![cfg(unix)]
use std::{fs, io, path::Path, process::Command};
use synod::{
    adapters::{git::operations, local::state::ClientConfig},
    application::setup::NewTeam,
    session::{bootstrap, protocol::Profile},
};

#[test]
fn initial_publication_can_recover_after_lost_confirmation() -> io::Result<()> {
    let root = std::env::temp_dir().join(format!("synod-initial-{}", std::process::id()));
    fs::create_dir(&root)?;
    let bare = root.join("remote.git");
    let init = Command::new("git")
        .args(["init", "--bare"])
        .arg(&bare)
        .output()?;
    assert!(init.status.success());
    let names = || ["one", "two", "three"].map(str::to_owned);
    let team = NewTeam::from_wizard("team-test", names(), names(), vec!["paper".into()])
        .expect("test manifest");
    let intent = operations::InitialCommit {
        actor: "member-2".into(),
        timestamp: "2026-11-26T08:00:00Z".into(),
        operation: "setup-one".into(),
    };
    let remote = bare.to_str().expect("temporary path");
    operations::publish_initial(&root, remote, &team, &intent)?;
    operations::publish_initial(&root, remote, &team, &intent)?;
    let count = Command::new("git")
        .arg("--git-dir")
        .arg(bare)
        .args(["rev-list", "--count", "main"])
        .output()?;
    assert_eq!(count.stdout, b"1\n");
    fs::remove_dir_all(root)
}

/// PTY harness entry point. The test process uses only the supplied temporary configuration.
#[test]
fn isolated_terminal() {
    let Some(root) = std::env::var_os("SYNOD_TEST_TERMINAL_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let config = root.join("client.json");
    let roots =
        bootstrap::initialize(&config, &root.join("state"), &root.join("work")).expect("roots");
    if std::env::var_os("SYNOD_TEST_FIRST_RUN").is_none() {
        let mut c = ClientConfig::read(&config).expect("settings");
        let team = serde_json::from_str(include_str!("fixtures/v1/team/team.json")).expect("team");
        let bare = root.join("remote.git");
        assert!(
            Command::new("git")
                .args(["init", "--bare"])
                .arg(&bare)
                .output()
                .expect("init")
                .status
                .success()
        );
        let remote = bare.to_str().expect("path");
        operations::publish_initial(
            &roots.state,
            remote,
            &team,
            &operations::freeze_initial("member-alice").expect("timestamp"),
        )
        .expect("publish");
        let profile = Profile {
            team,
            human: "member-alice".into(),
            assistant: if std::env::var_os("SYNOD_TEST_MCP").is_some() {
                "assistant-alice".into()
            } else {
                String::new()
            },
            remote: remote.into(),
        };
        c.fields.insert(
            "profile".into(),
            serde_json::to_value(profile).expect("profile"),
        );
        c.save(&config).expect("settings");
    }
    let file = config.clone();
    let host = std::thread::spawn(move || synod::session::transport::host(file));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !roots.state.join("runtime/endpoint.json").exists() {
        assert!(std::time::Instant::now() < until);
        std::thread::yield_now();
    }
    if std::env::var_os("SYNOD_TEST_MCP").is_some() {
        synod::mcp::run(&config).expect("MCP");
    } else {
        synod::tui::live::run(&config).expect("terminal");
    }
    host.join().expect("host thread").expect("host");
}

#[test]
fn sends_and_replies_with_frozen_attachments_and_independent_references() -> io::Result<()> {
    use synod::session::protocol::{Attachment, Draft};
    let root = std::env::temp_dir().join(format!("synod-send-{}", std::process::id()));
    fs::create_dir(&root)?;
    let bare = root.join("team.git");
    assert!(
        Command::new("git")
            .args(["init", "--bare"])
            .arg(&bare)
            .output()?
            .status
            .success()
    );
    let remote = bare.to_str().expect("fixture path");
    let team: NewTeam = serde_json::from_str(include_str!("fixtures/v1/team/team.json"))?;
    operations::publish_initial(
        &root,
        remote,
        &team,
        &operations::freeze_initial("member-alice")?,
    )?;
    let draft = Draft {
        title: "Diagram".into(),
        to: "member-chen".into(),
        cc: "member-bob,assistant-chen".into(),
        body: "Deliver figure\n- [ ] verify".into(),
        attachments: vec![Attachment {
            filename: "图.png".into(),
            bytes: vec![0, 255],
        }],
        ..Default::default()
    };
    let first = operations::FrozenSend::new("member-alice", team.id(), draft)?;
    operations::send_letter(&root, &team, remote, &first)?;
    operations::send_letter(&root, &team, remote, &first)?;
    let board = operations::published_letters(&root, &team, remote)?;
    assert_eq!(board.len(), 1);
    let reply = Draft::replying_to(&board[0], &board);
    assert_eq!(reply.to, "member-alice");
    assert!(reply.dependencies.is_empty());
    let second = operations::FrozenSend::new("member-chen", team.id(), reply)?;
    operations::send_letter(&root, &team, remote, &second)?;
    let board = operations::published_letters(&root, &team, remote)?;
    let original = board
        .iter()
        .find(|l| l.title == "Diagram")
        .expect("original");
    assert_eq!(
        Draft::replying_to(original, &board).title,
        "Re: Diagram - 2"
    );
    let replied = board
        .iter()
        .find(|l| l.title == "Re: Diagram")
        .expect("reply");
    assert_eq!(Draft::replying_to(replied, &board).title, "Re: Re: Diagram");
    let read = |path: String| {
        Command::new("git")
            .arg("--git-dir")
            .arg(&bare)
            .args(["show", &format!("main:{path}")])
            .output()
    };
    let envelope: serde_json::Value =
        serde_json::from_slice(&read(format!("letters/{}/envelope.json", replied.id))?.stdout)?;
    assert_eq!(envelope["references"][0], original.id);
    assert_eq!(envelope["initial_dependencies"], serde_json::json!([]));
    assert_eq!(
        read(format!("letters/{}/attachments/asset-1", original.id))?.stdout,
        vec![0, 255]
    );
    use synod::application::letters::TaskChange;
    let task = |id: &str, operation: &str, change: TaskChange| {
        operations::change_task(
            &root,
            &team,
            remote,
            id,
            &operations::InitialCommit {
                actor: "member-chen".into(),
                timestamp: "2026-11-26T09:00:00Z".into(),
                operation: operation.into(),
            },
            &change,
        )
    };
    task(
        &original.id,
        "link",
        TaskChange::Subtasks {
            selected: vec![replied.id.clone()],
            observed: vec![],
        },
    )?;
    assert!(
        task(
            &replied.id,
            "cycle",
            TaskChange::Subtasks {
                selected: vec![original.id.clone()],
                observed: vec![]
            }
        )
        .is_err()
    );
    task(&replied.id, "done", TaskChange::Completion(true))?;
    task(&replied.id, "done", TaskChange::Completion(true))?;
    task(&replied.id, "also-done", TaskChange::Completion(true))?;
    let updated = operations::published_letters(&root, &team, remote)?;
    assert_eq!(
        updated
            .iter()
            .find(|l| l.id == original.id)
            .unwrap()
            .progress,
        33
    );
    assert_eq!(
        updated
            .iter()
            .find(|l| l.id == replied.id)
            .unwrap()
            .events
            .len(),
        2
    );
    task(&replied.id, "reopen", TaskChange::Completion(false))?;
    assert_eq!(
        operations::published_letters(&root, &team, remote)?
            .iter()
            .find(|l| l.id == original.id)
            .unwrap()
            .progress,
        0
    );
    use synod::application::letters::ChecklistChange;
    for checked in [true, false] {
        task(
            &original.id,
            if checked { "tick" } else { "untick" },
            TaskChange::Checklist(ChecklistChange { line: 1, checked }),
        )?;
        let clean = root.join(if checked {
            "fresh-checked"
        } else {
            "fresh-unchecked"
        });
        let projected = operations::published_letters(&clean, &team, remote)?;
        let letter = projected.iter().find(|l| l.id == original.id).unwrap();
        assert_eq!(letter.checklist[0].checked, checked);
        assert_eq!(letter.progress_done, usize::from(checked));
        assert_eq!(letter.body, "Deliver figure\n- [ ] verify");
        let event_dir = format!("letters/{}/events", original.id);
        for (args, expected) in [
            (vec!["diff", "--name-only", "main^", "main"], ""),
            (
                vec!["ls-tree", "-r", "--name-only", "main", "--", &event_dir],
                "",
            ),
        ] {
            let output = Command::new("git")
                .arg("--git-dir")
                .arg(&bare)
                .args(args)
                .output()?;
            assert!(output.status.success());
            assert_eq!(output.stdout, expected.as_bytes());
        }
    }
    fs::remove_dir_all(root)
}

#[test]
fn workspace_save_submit_and_defer_preserve_branch_boundaries() -> io::Result<()> {
    use synod::session::protocol::{WorkAction as A, WorkResult as R};
    let root = std::env::temp_dir().join(format!("synod-work-{}", std::process::id()));
    fs::create_dir(&root)?;
    let bare = root.join("team.git");
    assert!(
        Command::new("git")
            .args(["init", "--bare"])
            .arg(&bare)
            .output()?
            .status
            .success()
    );
    let remote = bare.to_str().expect("fixture path");
    let team: NewTeam = serde_json::from_str(include_str!("fixtures/v1/team/team.json"))?;
    operations::publish_initial(
        &root,
        remote,
        &team,
        &operations::freeze_initial("member-alice")?,
    )?;
    let work = root.join("work");
    fs::create_dir_all(work.join("code"))?;
    let local = work.join("code/result");
    fs::write(&local, "first")?;
    let action =
        |actor, a| operations::workspace_action(&root, &work, &team, remote, actor, "code", a);
    assert_eq!(action("member-alice", A::Submit)?, R::Dirty);
    assert_eq!(action("member-alice", A::Save)?, R::Saved);
    let show = |spec: &str| {
        Command::new("git")
            .arg("--git-dir")
            .arg(&bare)
            .args(["show", spec])
            .output()
    };
    assert!(!show("main:workspace/code/result")?.status.success());
    assert_eq!(
        show("work/member-alice/code:workspace/code/result")?.stdout,
        b"first"
    );
    assert_eq!(action("member-alice", A::Submit)?, R::Published);
    fs::write(&local, "bob")?;
    assert_eq!(action("member-bob", A::SaveAndSubmit)?, R::Published);
    fs::write(&local, "alice")?;
    assert_eq!(action("member-alice", A::SaveAndSubmit)?, R::Conflict);
    assert_eq!(action("member-alice", A::Defer)?, R::Deferred);
    assert_eq!(fs::read(&local)?, b"alice");
    assert_eq!(show("main:workspace/code/result")?.stdout, b"bob");
    assert_eq!(
        show("work/member-alice/code:workspace/code/result")?.stdout,
        b"alice"
    );
    assert_eq!(action("member-alice", A::Submit)?, R::Conflict);
    let marker = root
        .join("teams")
        .join(team.id())
        .join("spaces/code/member-alice/merge-attempt");
    let dir = root
        .join("teams")
        .join(team.id())
        .join("worktrees")
        .join(fs::read_to_string(marker)?);
    for (key, value) in [
        ("merge.tool", "fixture"),
        ("mergetool.fixture.cmd", "cp \"$REMOTE\" \"$MERGED\""),
        ("mergetool.fixture.trustExitCode", "true"),
    ] {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(["config", key, value])
                .output()?
                .status
                .success()
        );
    }
    assert_eq!(action("member-alice", A::Resolve)?, R::Published);
    assert_eq!(show("main:workspace/code/result")?.stdout, b"alice");
    fs::remove_dir_all(root)
}

#[test]
fn cloud_drafts_survive_device_changes_and_diverging_edits() -> io::Result<()> {
    use synod::{
        adapters::git::drafts,
        application::letters::{Attachment, Draft},
    };
    let root = std::env::temp_dir().join(format!("synod-cloud-test-{}", std::process::id()));
    fs::create_dir(&root)?;
    let bare = root.join("remote.git");
    assert!(
        Command::new("git")
            .args(["init", "--bare"])
            .arg(&bare)
            .output()?
            .status
            .success()
    );
    let repo = bare.to_str().expect("fixture path");
    let team: NewTeam = serde_json::from_str(include_str!("fixtures/v1/team/team.json"))?;
    let (a, b) = (root.join("a"), root.join("b"));
    let actor = "member-alice";
    operations::publish_initial(&a, repo, &team, &operations::freeze_initial(actor)?)?;
    let first = Draft {
        title: "Unsynced research".into(),
        body: "First draft".into(),
        to: "member-chen".into(),
        attachments: vec![Attachment {
            filename: "图.png".into(),
            bytes: vec![0, 255],
        }],
        ..Default::default()
    };
    let operation = |d| operations::FrozenSend::new(actor, team.id(), d);
    let initial = operation(first)?;
    let primary = drafts::save(&a, &team, repo, &initial)?;
    assert_eq!(drafts::save(&a, &team, repo, &initial)?, primary);
    let (mut found, _) = drafts::discover(&b, &team, repo, actor)?;
    assert_eq!(found.len(), 1);
    let mut second = found.pop().unwrap();
    assert_eq!(second.attachments[0].bytes, [0, 255]);
    assert_eq!(second.cloud.as_ref(), Some(&primary));
    let mut left = second.clone();
    left.body = "From A".into();
    drafts::save(&a, &team, repo, &operation(left)?)?;
    second.body = "From B".into();
    let stale = operation(second)?;
    let fork = drafts::save(&b, &team, repo, &stale)?;
    assert_ne!(fork.copy, primary.copy);
    assert_eq!(drafts::save(&b, &team, repo, &stale)?, fork);
    let (all, _) = drafts::discover(&a, &team, repo, actor)?;
    assert_eq!(all.len(), 2);
    assert!(
        drafts::discover(&a, &team, repo, "member-bob")?
            .0
            .is_empty()
    );
    let sent = operation(
        all.into_iter()
            .find(|d| d.body == "From B")
            .expect("conflict copy"),
    )?;
    operations::send_letter(&a, &team, repo, &sent)?;
    let after = drafts::discover(&b, &team, repo, actor)?;
    assert!(after.1.contains(&(fork.letter, fork.copy)));
    assert_eq!(after.0.len(), 1);
    assert_eq!(after.0[0].body, "From A");
    fs::remove_dir_all(root)
}
