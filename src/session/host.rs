//! Local user operations executed by the session owner.
use super::{bootstrap, protocol::*};
use crate::adapters::{
    git::operations,
    github::{RepositoryRequest, Visibility},
    local::state::ClientConfig,
};
use std::{fs, io, path::Path};
fn profile(config: &ClientConfig) -> io::Result<Option<Profile>> {
    config
        .fields
        .get("profile")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(io::Error::other)
}
pub(super) fn perform(file: &Path, request: Request) -> io::Result<Reply> {
    let mut config = ClientConfig::read(file)?;
    let roots = bootstrap::configured(file)?.ok_or_else(|| io::Error::other("Setup required"))?;
    let mut current = profile(&config)?;
    if let Some(p) = current.as_mut()
        && p.human.is_empty()
        && !p.assistant.is_empty()
    {
        p.assistant.clear();
        config
            .fields
            .insert("profile".into(), serde_json::to_value(p)?);
        config.save(file)?;
    }
    match request {
        Request::Published => {
            let team = current.ok_or_else(|| io::Error::other("Team required"))?;
            if config.pending_team.is_some() {
                return Ok(Reply::Published(Vec::new()));
            }
            let (manifest, letters) =
                operations::published_snapshot(&roots.state, &team.team, &team.remote)?;
            if serde_json::to_value(&manifest)? != serde_json::to_value(&team.team)? {
                save_profile(
                    config,
                    file,
                    Profile {
                        team: manifest,
                        ..team
                    },
                )?;
            }
            Ok(Reply::Published(letters))
        }
        Request::Profile => Ok(Reply::Team(current)),
        Request::Create {
            owner,
            name,
            public,
            manifest,
        } => {
            if current.is_some() {
                return Err(io::Error::other("Team already configured"));
            }
            manifest.validate()?;
            let req = RepositoryRequest::new(
                &owner,
                &name,
                if public {
                    Visibility::Public
                } else {
                    Visibility::Private
                },
            )
            .map_err(|e| io::Error::other(format!("{e:?}")))?;
            let remote = format!("https://github.com/{}.git", req.target());
            req.confirm()
                .create()
                .map_err(|e| io::Error::other(format!("{e:?}")))?;
            config
                .fields
                .insert("setup_members_pending".into(), true.into());
            let data = serde_json::to_value(&manifest)?;
            config.pending_team = Some(crate::application::setup::PendingTeam::new(manifest));
            let profile = Profile {
                team: serde_json::from_value(data)?,
                human: String::new(),
                assistant: String::new(),
                remote,
            };
            save_profile(config, file, profile)
        }
        Request::Join(remote) => {
            if current.is_some() {
                return Err(io::Error::other("Team already configured"));
            }
            let slug = remote
                .strip_prefix("https://github.com/")
                .ok_or_else(|| io::Error::other("Enter an HTTPS GitHub repository URL"))?
                .trim_end_matches('/')
                .trim_end_matches(".git");
            let (owner, name) = slug
                .split_once('/')
                .ok_or_else(|| io::Error::other("Missing owner or repository"))?;
            RepositoryRequest::new(owner, name, Visibility::Private)
                .map_err(|e| io::Error::other(format!("{e:?}")))?;
            let team = operations::fetch_manifest(&remote, &roots.state.join("setup-cache.git"))?;
            save_profile(
                config,
                file,
                Profile {
                    team,
                    human: String::new(),
                    assistant: String::new(),
                    remote,
                },
            )
        }
        Request::SetupPending => Ok(Reply::Pending(
            config
                .fields
                .get("setup_members_pending")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        )),
        Request::Register {
            human,
            assistant,
            initial,
        } => {
            let mut team = current.ok_or_else(|| io::Error::other("Team required"))?;
            if initial
                && config
                    .fields
                    .get("setup_members_pending")
                    .and_then(|v| v.as_bool())
                    != Some(true)
            {
                return Err(io::Error::other("Initial registration already complete"));
            }
            if initial {
                crate::application::setup::Activation::parse(&human, &assistant)?;
            }
            let prior = config.fields.get("registration").cloned();
            let (human, assistant, initial, intent): (
                String,
                String,
                bool,
                operations::InitialCommit,
            ) = match prior {
                Some(v) => serde_json::from_value(v)?,
                None => {
                    if human.trim().is_empty() && assistant.trim().is_empty() {
                        if initial {
                            config.fields.remove("setup_members_pending");
                            config.save(file)?
                        }
                        return Ok(Reply::Team(Some(team)));
                    }
                    if !initial && team.human.is_empty() && config.pending_team.is_none() {
                        return Err(io::Error::other("请先选择署名的人类身份"));
                    }
                    let key = format!(
                        "reg-{}-{}",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_err(io::Error::other)?
                            .as_nanos(),
                        std::process::id()
                    );
                    let author = if !team.human.is_empty() {
                        team.human.clone()
                    } else if !team.assistant.is_empty() {
                        team.assistant.clone()
                    } else if !human.trim().is_empty() {
                        format!("member-{key}")
                    } else {
                        format!("assistant-{key}")
                    };
                    let mut intent = operations::freeze_initial(&author)?;
                    intent.operation = key;
                    config.fields.insert(
                        "registration".into(),
                        serde_json::to_value((&human, &assistant, initial, &intent))?,
                    );
                    config.save(file)?;
                    (human, assistant, initial, intent)
                }
            };
            let (h, a) = team.team.register(&intent.operation, &human, &assistant)?;
            if config.pending_team.is_some() && !initial && team.human.is_empty() {
                config.pending_team = Some(crate::application::setup::PendingTeam::new(
                    team.team.clone(),
                ));
            } else if config.pending_team.is_some() {
                operations::publish_initial(&roots.state, &team.remote, &team.team, &intent)?;
                config.pending_team = None;
            } else {
                team.team = operations::register_members(
                    &roots.state,
                    &team.remote,
                    &team.team,
                    &intent,
                    &human,
                    &assistant,
                )?;
            }
            if initial {
                team.human = h;
                team.assistant = a;
            }
            config.fields.remove("registration");
            config.fields.remove("setup_members_pending");
            save_profile(config, file, team)
        }
        Request::Identities { human, assistant } => {
            let (human, assistant) =
                crate::application::setup::Activation::parse(&human, &assistant)?.slots();
            let mut team = current.ok_or_else(|| io::Error::other("Team required"))?;
            if !(human.is_empty() || team.team.human(&human))
                || !(assistant.is_empty() || team.team.assistant(&assistant))
            {
                return Err(io::Error::other(
                    "Unknown member or incorrect identity slot",
                ));
            }
            team.human = human.into();
            team.assistant = assistant.into();
            if let Some(pending) = &config.pending_team {
                let actor = if !team.human.is_empty() {
                    Some(team.human.as_str())
                } else if !team.assistant.is_empty() {
                    Some(team.assistant.as_str())
                } else {
                    None
                };
                if let Some(publication) = pending.publication_for(actor) {
                    let intent: operations::InitialCommit =
                        match config.fields.get("initial_commit") {
                            Some(v) => serde_json::from_value(v.clone())?,
                            None => operations::freeze_initial(publication.actor_id)?,
                        };
                    config
                        .fields
                        .insert("profile".into(), serde_json::to_value(&team)?);
                    config
                        .fields
                        .insert("initial_commit".into(), serde_json::to_value(&intent)?);
                    config.save(file)?;
                    operations::publish_initial(
                        &roots.state,
                        &team.remote,
                        publication.manifest,
                        &intent,
                    )?;
                    config.pending_team = None;
                    config.fields.remove("initial_commit");
                }
            }
            save_profile(config, file, team)
        }
        Request::Drafts | Request::SaveDraft(_) | Request::Send(_) => {
            let team = current.ok_or_else(|| io::Error::other("Team required"))?;
            if team.human.is_empty() {
                return if matches!(request, Request::Drafts) {
                    Ok(Reply::Drafts(Vec::new()))
                } else {
                    Err(io::Error::other("Select a human identity in preferences"))
                };
            }
            team.team.validate()?;
            if !team.team.human(&team.human) {
                return Err(io::Error::other("Invalid human identity"));
            }
            let base = roots
                .state
                .join("teams")
                .join(team.team.id())
                .join("drafts")
                .join(&team.human);
            fs::create_dir_all(&base)?;
            if let Request::Send(reference) = request {
                if !reference
                    .strip_prefix("letter-local-")
                    .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                {
                    return Err(io::Error::other("Invalid draft reference"));
                }
                let copy = base.join(&reference).join("primary");
                let draft: Draft = serde_json::from_slice(&fs::read(copy.join("editing.json"))?)?;
                let pending = copy.join("send.json");
                let prior: Option<operations::FrozenSend> = match fs::read(&pending) {
                    Ok(bytes) => Some(serde_json::from_slice(&bytes)?),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                    Err(e) => return Err(e),
                };
                let intent = match prior {
                    Some(i) if i.draft == draft => i,
                    _ => operations::FrozenSend::new(&team.human, team.team.id(), draft)?,
                };
                let bytes = serde_json::to_vec(&intent)?;
                let out = roots
                    .state
                    .join("teams")
                    .join(team.team.id())
                    .join("outbox")
                    .join(&intent.key);
                fs::create_dir_all(&out)?;
                for dest in [out.join("intent.json"), pending] {
                    let tmp = dest.with_extension("tmp");
                    fs::write(&tmp, &bytes)?;
                    fs::File::open(&tmp)?.sync_all()?;
                    fs::rename(tmp, &dest)?;
                }
                operations::send_letter(&roots.state, &team.team, &team.remote, &intent)?;
                fs::write(copy.join("sent"), &intent.key)?;
                return Ok(Reply::Ready);
            }
            if let Request::SaveDraft(mut draft) = request {
                let copy = editing_copy(&base, &mut draft)?;
                let prior =
                    read_optional::<operations::FrozenSend>(&copy.join("save-pending.json"))?;
                let intent = match prior {
                    Some(i) if i.draft == draft => i,
                    _ => operations::FrozenSend::new(&team.human, team.team.id(), draft)?,
                };
                write_local(&copy.join("save-pending.json"), &intent)?;
                write_local(&copy.join("editing.json"), &intent.draft)?;
                return match finish_cloud_save(&roots.state, &team, &copy, &intent) {
                    Ok(d) => Ok(Reply::Saved(d, None)),
                    Err(e) => Ok(Reply::Saved(intent.draft, Some(e.to_string()))),
                };
            }
            let mut local = Vec::new();
            for entry in fs::read_dir(&base)? {
                let copy = entry?.path().join("primary");
                if copy.join("sent").exists() {
                    continue;
                }
                if let Some(intent) =
                    read_optional::<operations::FrozenSend>(&copy.join("save-pending.json"))?
                {
                    let result = finish_cloud_save(&roots.state, &team, &copy, &intent);
                    local.push((result.unwrap_or(intent.draft), copy));
                } else if let Some(draft) = read_optional::<Draft>(&copy.join("editing.json"))? {
                    local.push((draft, copy))
                }
            }
            let remote = crate::adapters::git::drafts::discover(
                &roots.state,
                &team.team,
                &team.remote,
                &team.human,
            );
            if let Ok((drafts, sent)) = remote {
                local.retain(|(d, copy)| {
                    copy.join("save-pending.json").exists()
                        || !d
                            .cloud
                            .as_ref()
                            .is_some_and(|c| sent.contains(&(c.letter.clone(), c.copy.clone())))
                });
                for mut draft in drafts {
                    if let Some((stored, copy)) = local.iter_mut().find(|(d, _)| {
                        d.cloud
                            .as_ref()
                            .zip(draft.cloud.as_ref())
                            .is_some_and(|(a, b)| a.copy == b.copy && a.letter == b.letter)
                    }) {
                        if !copy.join("save-pending.json").exists() && stored.cloud != draft.cloud {
                            draft.reference = stored.reference.clone();
                            write_local(&copy.join("editing.json"), &draft)?;
                            *stored = draft;
                        }
                    } else {
                        let copy = editing_copy(&base, &mut draft)?;
                        write_local(&copy.join("editing.json"), &draft)?;
                        local.push((draft, copy))
                    }
                }
            }
            Ok(Reply::Drafts(local.into_iter().map(|(d, _)| d).collect()))
        }
        Request::WorkspaceAction { space, action } => {
            let profile = current.ok_or_else(|| io::Error::other("Team required"))?;
            Ok(Reply::Work(operations::workspace_action(
                &roots.state,
                &roots.work,
                &profile.team,
                &profile.remote,
                &profile.human,
                &space,
                action,
            )?))
        }
        Request::Workspace => {
            let mut names = Vec::new();
            fn visit(root: &Path, dir: &Path, names: &mut Vec<String>) -> io::Result<()> {
                for item in fs::read_dir(dir)? {
                    let item = item?;
                    let path = item.path();
                    names.push(
                        path.strip_prefix(root)
                            .map_err(io::Error::other)?
                            .display()
                            .to_string(),
                    );
                    if item.file_type()?.is_dir() {
                        visit(root, &path, names)?
                    }
                }
                Ok(())
            }
            visit(&roots.work, &roots.work, &mut names)?;
            names.sort();
            Ok(Reply::Files(names))
        }
        _ => Err(io::Error::other("Unexpected request")),
    }
}
fn read_optional<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::File::open(path) {
        Ok(f) => Ok(Some(serde_json::from_reader(f)?)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
fn write_local(path: &Path, value: &impl serde::Serialize) -> io::Result<()> {
    let temp = path.with_extension("tmp");
    let file = fs::File::create(&temp)?;
    serde_json::to_writer(&file, value)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, path)?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?
    }
    Ok(())
}
fn editing_copy(base: &Path, draft: &mut Draft) -> io::Result<std::path::PathBuf> {
    if draft.reference.is_empty() {
        for serial in 1..=u64::MAX {
            let id = format!("letter-local-{serial}");
            match fs::create_dir(base.join(&id)) {
                Ok(()) => {
                    draft.reference = id;
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
    }
    if !draft
        .reference
        .strip_prefix("letter-local-")
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
    {
        return Err(io::Error::other("Invalid local draft reference"));
    }
    let dir = base.join(&draft.reference);
    if !dir.is_dir() {
        return Err(io::Error::other("Unknown local draft"));
    }
    let copy = dir.join("primary");
    fs::create_dir_all(&copy)?;
    Ok(copy)
}
fn finish_cloud_save(
    root: &Path,
    profile: &Profile,
    copy: &Path,
    intent: &operations::FrozenSend,
) -> io::Result<Draft> {
    let location =
        crate::adapters::git::drafts::save(root, &profile.team, &profile.remote, intent)?;
    let mut draft = intent.draft.clone();
    draft.cloud = Some(location);
    write_local(&copy.join("editing.json"), &draft)?;
    fs::remove_file(copy.join("save-pending.json"))?;
    Ok(draft)
}
fn save_profile(mut config: ClientConfig, file: &Path, p: Profile) -> io::Result<Reply> {
    config
        .fields
        .insert("profile".into(), serde_json::to_value(&p)?);
    config.save(file)?;
    Ok(Reply::Team(Some(p)))
}

#[cfg(test)]
mod registration_tests {
    use super::*;
    use crate::application::setup::{NewTeam, PendingTeam};
    #[test]
    fn registration_grows_roster_and_preserves_activation() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("synod-roster-{}", std::process::id()));
        fs::create_dir(&root)?;
        let file = root.join("client.json");
        bootstrap::initialize(&file, &root.join("state"), &root.join("work"))?;
        let remote = root.join("team.git");
        assert!(
            std::process::Command::new("git")
                .args(["init", "--bare"])
                .arg(&remote)
                .output()?
                .status
                .success()
        );
        let team = NewTeam::empty("team-test", vec![]).expect("valid empty roster");
        let mut config = ClientConfig::read(&file)?;
        config.pending_team = Some(PendingTeam::new(team.clone()));
        config
            .fields
            .insert("setup_members_pending".into(), true.into());
        save_profile(
            config,
            &file,
            Profile {
                team,
                human: String::new(),
                assistant: String::new(),
                remote: remote.display().to_string(),
            },
        )?;
        let register = |h: &str, a: &str, initial| {
            perform(
                &file,
                Request::Register {
                    human: h.into(),
                    assistant: a.into(),
                    initial,
                },
            )
        };
        assert!(register("", "bot", true).is_err());
        let Reply::Team(Some(first)) = register("Alice", "Alice's bot", true)? else {
            panic!("missing profile")
        };
        assert!(first.team.human(&first.human) && first.team.assistant(&first.assistant));
        for n in 0..3 {
            register(&format!("Human {n}"), &format!("AI {n}"), false)?;
        }
        let Reply::Team(Some(last)) = register("", "One more AI", false)? else {
            panic!("missing profile")
        };
        assert_eq!(last.team.roster().len(), 9);
        assert_eq!(last.human, first.human);
        assert_eq!(last.assistant, first.assistant);
        assert!(
            perform(
                &file,
                Request::Identities {
                    human: "".into(),
                    assistant: first.assistant.clone()
                }
            )
            .is_err()
        );
        let Reply::Team(Some(human)) = perform(
            &file,
            Request::Identities {
                human: first.human,
                assistant: "".into(),
            },
        )?
        else {
            panic!("missing profile")
        };
        assert!(human.assistant.is_empty());
        assert!(human.team.human(&human.human));
        let Reply::Team(Some(inactive)) = perform(
            &file,
            Request::Identities {
                human: "".into(),
                assistant: "".into(),
            },
        )?
        else {
            panic!("missing profile")
        };
        assert!(inactive.human.is_empty() && inactive.assistant.is_empty());
        let remote_manifest = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(remote)
            .args(["show", "main:team.json"])
            .output()?;
        assert_eq!(
            serde_json::from_slice::<NewTeam>(&remote_manifest.stdout)?
                .roster()
                .len(),
            9
        );
        fs::remove_dir_all(root)
    }
}
