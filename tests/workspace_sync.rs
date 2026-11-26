use std::{fs, io, path::Path, process::Command};
use synod::{
    adapters::{
        git::{
            operations,
            workspace_sync::{Mode, Space},
        },
        local::tags,
    },
    application::setup::NewTeam,
    session::protocol::WorkAction,
};
#[test]
fn automatic_updates_preserve_normal_folders_and_isolate_dirty_tags() -> io::Result<()> {
    let root = std::env::temp_dir().join(format!("synod-sync-spaces-{}", std::process::id()));
    fs::create_dir(&root)?;
    let repo = root.join("remote.git");
    assert!(
        Command::new("git")
            .args(["init", "--bare"])
            .arg(&repo)
            .output()?
            .status
            .success()
    );
    let remote = repo.to_str().unwrap();
    let team: NewTeam = serde_json::from_str(include_str!("fixtures/v1/team/team.json"))?;
    let source = root.join("source");
    let state = root.join("state");
    let work = root.join("editable");
    let author = root.join("author");
    let nested = "project/研究 绘图";
    for (s, w) in [(&state, &work), (&source, &author)] {
        fs::create_dir_all(w.join(nested))?;
        fs::create_dir_all(w.join("code"))?;
        let existing = tags::toggle(s, w, &team, nested)?;
        assert!(existing.contains_key("code"));
        assert!(tags::toggle(s, w, &team, "project").is_err());
        assert!(tags::toggle(s, w, &team, "../outside").is_err());
    }
    operations::publish_initial(
        &source,
        remote,
        &team,
        &operations::freeze_initial("member-alice")?,
    )?;
    let publish = |space: &str, bytes: &str| -> io::Result<()> {
        fs::write(author.join(space).join("file.txt"), bytes)?;
        operations::workspace_action(
            &source,
            &author,
            &team,
            remote,
            "member-alice",
            space,
            WorkAction::SaveAndSubmit,
        )?;
        Ok(())
    };
    fs::write(author.join(".gitignore"), "*.log\n")?;
    fs::write(
        author.join(nested).join("generated.log"),
        "local generated output",
    )?;
    publish(nested, "first")?;
    let ignored_path = format!("main:workspace/{nested}/generated.log");
    assert!(
        !Command::new("git")
            .arg("--git-dir")
            .arg(&repo)
            .args(["cat-file", "-e", &ignored_path])
            .output()?
            .status
            .success()
    );

    publish("code", "code1")?;
    operations::published_letters(&state, &team, remote)?;
    let paper = Space::open(&state, &work, &team, nested)?;
    let code = Space::open(&state, &work, &team, "code")?;
    let local = work.join(nested);
    #[cfg(unix)]
    let inode = {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(&local)?.ino()
    };
    let preview = paper.synchronize(None)?;
    assert_eq!(preview.mode, Mode::Manual);
    assert!(!local.join("file.txt").exists());
    paper.synchronize(Some(&preview))?;
    code.set_mode(Mode::WhenClean)?;
    code.synchronize(None)?;
    assert_eq!(fs::read(work.join("code/file.txt"))?, b"code1");
    paper.set_mode(Mode::WhenClean)?;
    fs::write(work.join("code/file.txt"), "my changes")?;
    fs::write(author.join(".gitignore"), "*.log\nfile.txt\n")?;
    fs::write(work.join(".gitignore"), "*.log\nfile.txt\n")?;
    fs::write(local.join("generated.log"), "keep my output")?;
    publish(nested, "second")?;
    publish("code", "code2")?;
    operations::published_letters(&state, &team, remote)?;
    assert!(paper.synchronize(Some(&preview)).is_err());
    let dirty = code.synchronize(None)?;
    assert!(dirty.dirty);
    assert_eq!(dirty.conflicts, ["file.txt"]);
    assert_eq!(fs::read(work.join("code/file.txt"))?, b"my changes");
    paper.synchronize(None)?;
    assert_eq!(fs::read(local.join("file.txt"))?, b"second");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(fs::metadata(&local)?.ino(), inode);
        std::os::unix::fs::symlink(&root, work.join("outside"))?;
        assert!(tags::toggle(&state, &work, &team, "outside").is_err());
    }
    assert_eq!(fs::read(local.join("generated.log"))?, b"keep my output");
    assert_eq!(fs::read_dir(&local)?.count(), 2);
    tags::toggle(&state, &work, &team, nested)?;
    assert!(!tags::load(&state, &team)?.contains_key(nested));
    assert!(tags::load(&source, &team)?.contains_key(nested));
    assert!(Path::new(&local).join("file.txt").is_file());
    fs::remove_dir_all(root)
}
