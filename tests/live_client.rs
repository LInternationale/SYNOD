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
        assistant: String::new(),
        remote: remote.into(),
    };
    c.fields.insert(
        "profile".into(),
        serde_json::to_value(profile).expect("profile"),
    );
    c.save(&config).expect("settings");
    let file = config.clone();
    let host = std::thread::spawn(move || synod::session::transport::host(file));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !roots.state.join("runtime/endpoint.json").exists() {
        assert!(std::time::Instant::now() < until);
        std::thread::yield_now();
    }
    synod::tui::live::run(&config).expect("terminal");
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
        body: "Deliver figure".into(),
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
    fs::remove_dir_all(root)
}
