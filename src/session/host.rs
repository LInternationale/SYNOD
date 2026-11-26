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
    let current = profile(&config)?;
    match request {
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
        Request::Identities { human, assistant } => {
            let mut team = current.ok_or_else(|| io::Error::other("Team required"))?;
            if !(human.is_empty() || team.team.human(&human))
                || !(assistant.is_empty() || team.team.assistant(&assistant))
            {
                return Err(io::Error::other(
                    "Unknown member or incorrect identity slot",
                ));
            }
            team.human = human;
            team.assistant = assistant;
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
        Request::Drafts | Request::SaveDraft(_) => {
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
                .join(team.human);
            fs::create_dir_all(&base)?;
            if let Request::SaveDraft(mut draft) = request {
                let dir = if draft.reference.is_empty() {
                    let mut serial = 1u64;
                    loop {
                        let id = format!("letter-local-{serial}");
                        let dir = base.join(&id);
                        match fs::create_dir(&dir) {
                            Ok(()) => {
                                draft.reference = id;
                                break dir;
                            }
                            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => serial += 1,
                            Err(e) => return Err(e),
                        }
                    }
                } else {
                    if !draft
                        .reference
                        .strip_prefix("letter-local-")
                        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                    {
                        return Err(io::Error::other("Invalid draft reference"));
                    }
                    let dir = base.join(&draft.reference);
                    if !dir.is_dir() {
                        return Err(io::Error::other("Unknown draft"));
                    }
                    dir
                };
                let copy = dir.join("primary");
                fs::create_dir_all(&copy)?;
                let tmp = copy.join("editing.tmp");
                fs::write(&tmp, serde_json::to_vec(&draft)?)?;
                fs::File::open(&tmp)?.sync_all()?;
                fs::rename(tmp, copy.join("editing.json"))?;
                return Ok(Reply::Saved(draft));
            }
            let mut drafts = Vec::new();
            for entry in fs::read_dir(base)? {
                let path = entry?.path().join("primary/editing.json");
                if path.is_file() {
                    drafts.push(serde_json::from_slice(&fs::read(path)?)?)
                }
            }
            Ok(Reply::Drafts(drafts))
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
fn save_profile(mut config: ClientConfig, file: &Path, p: Profile) -> io::Result<Reply> {
    config
        .fields
        .insert("profile".into(), serde_json::to_value(&p)?);
    config.save(file)?;
    Ok(Reply::Team(Some(p)))
}
