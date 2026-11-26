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
    let profile = Profile {
        team,
        human: "member-alice".into(),
        assistant: String::new(),
        remote: String::new(),
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
