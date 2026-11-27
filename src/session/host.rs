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
#[cfg(test)]
fn perform(file: &Path, request: Request) -> io::Result<Reply> {
    perform_as(file, request, Entry::Human)
}
pub(super) fn perform_as(file: &Path, request: Request, entry: Entry) -> io::Result<Reply> {
    perform_observed(file, request, entry, &mut |_| {})
}
pub(super) fn perform_observed(
    file: &Path,
    request: Request,
    entry: Entry,
    ready: &mut dyn FnMut(Reply),
) -> io::Result<Reply> {
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
        save_session(&config, file, false)?;
    }
    if matches!(entry, Entry::Assistant) && !current.as_ref().is_some_and(|p| entry.bound(p)) {
        return Err(io::Error::other(
            "Select an active assistant identity in preferences",
        ));
    }
    match request {
        Request::Task { letter, change } => {
            let p = current.ok_or_else(|| io::Error::other("Team required"))?;
            if !entry.bound(&p) {
                return Err(io::Error::other("请先激活人类身份"));
            }
            let mut intent = operations::freeze_initial(entry.actor(&p))?;
            intent.operation = format!(
                "task-{}-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(io::Error::other)?
                    .as_nanos(),
                std::process::id()
            );
            let dir = roots
                .state
                .join("teams")
                .join(p.team.id())
                .join("outbox")
                .join(&intent.operation);
            fs::create_dir_all(&dir)?;
            write_local(
                &dir.join("intent.json"),
                &(&p.remote, &letter, &change, &intent),
            )?;
            operations::change_task_observed(
                &roots.state,
                &p.team,
                &p.remote,
                &letter,
                &intent,
                &change,
                &mut || ready(Reply::Committed),
            )?;
            fs::remove_file(dir.join("intent.json"))?;
            fs::remove_dir(dir)?;
            Ok(Reply::Ready)
        }
        Request::Published | Request::Read(_) => {
            let team = current.ok_or_else(|| io::Error::other("Team required"))?;
            if config.pending_team.is_some() {
                return Ok(Reply::Published(Vec::new()));
            }
            let (manifest, mut letters) =
                operations::published_snapshot(&roots.state, &team.team, &team.remote)?;
            let reply = board_reply(&roots.state, &team, request, entry, &mut letters)?;
            if serde_json::to_value(&manifest)? != serde_json::to_value(&team.team)? {
                store_profile(
                    config,
                    file,
                    Profile {
                        team: manifest,
                        ..team
                    },
                    true,
                )?;
            }
            Ok(reply)
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
                            save_session(&config, file, true)?
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
                    save_session(&config, file, true)?;
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
            store_profile(config, file, team, !initial)
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
                    save_session(&config, file, true)?;
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
        Request::Drafts | Request::SaveDraft(_) | Request::Send(_) | Request::SendCurrent(_) => {
            let team = current.ok_or_else(|| io::Error::other("Team required"))?;
            if entry.actor(&team).is_empty() {
                return if matches!(request, Request::Drafts) {
                    Ok(Reply::Drafts(Vec::new()))
                } else {
                    Err(io::Error::other("Select a human identity in preferences"))
                };
            }
            team.team.validate()?;
            if !entry.bound(&team) {
                return Err(io::Error::other("Invalid human identity"));
            }
            let base = roots
                .state
                .join("teams")
                .join(team.team.id())
                .join("drafts")
                .join(entry.actor(&team));
            fs::create_dir_all(&base)?;
            if !matches!(request, Request::Drafts) {
                match fs::remove_file(base.join("current-list.json")) {
                    Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                    _ => {}
                }
            }
            let request = if let Request::SendCurrent(mut draft) = request {
                let copy = editing_copy(&base, &mut draft)?;
                write_local(&copy.join("editing.json"), &draft)?;
                Request::Send(draft.reference)
            } else {
                request
            };
            if let Request::Send(reference) = request {
                if !reference
                    .strip_prefix("letter-local-")
                    .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                {
                    return Err(io::Error::other("Invalid draft reference"));
                }
                let copy = base.join(&reference).join("primary");
                if copy.join("sent").is_file() {
                    return Ok(Reply::Ready);
                }
                let draft: Draft = serde_json::from_slice(&fs::read(copy.join("editing.json"))?)?;
                let pending = copy.join("send.json");
                let prior: Option<operations::FrozenSend> = match fs::read(&pending) {
                    Ok(bytes) => Some(serde_json::from_slice(&bytes)?),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                    Err(e) => return Err(e),
                };
                let intent = match prior {
                    Some(i) if i.draft == draft => i,
                    _ => operations::FrozenSend::new(entry.actor(&team), team.team.id(), draft)?,
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
                operations::send_letter_observed(
                    &roots.state,
                    &team.team,
                    &team.remote,
                    &intent,
                    &mut || ready(Reply::Committed),
                )?;
                fs::write(copy.join("sent"), &intent.key)?;
                fs::remove_file(out.join("intent.json"))?;
                fs::remove_dir(out)?;
                fs::remove_file(copy.join("send.json"))?;
                return Ok(Reply::Ready);
            }
            if let Request::SaveDraft(mut draft) = request {
                delivery::editable(
                    &roots.state.join("teams").join(team.team.id()),
                    entry.actor(&team),
                    &draft.reference,
                )?;
                let copy = editing_copy(&base, &mut draft)?;
                let prior =
                    read_optional::<operations::FrozenSend>(&copy.join("save-pending.json"))?;
                let intent = match prior {
                    Some(i) if i.draft == draft => i,
                    _ => operations::FrozenSend::new(entry.actor(&team), team.team.id(), draft)?,
                };
                write_local(&copy.join("save-pending.json"), &intent)?;
                write_local(&copy.join("editing.json"), &intent.draft)?;
                delivery::clear_failed(
                    &roots.state.join("teams").join(team.team.id()),
                    entry.actor(&team),
                    &intent.draft.reference,
                )?;
                return match finish_cloud_save_observed(
                    &roots.state,
                    &team,
                    &copy,
                    &intent,
                    &mut |location| {
                        let mut draft = intent.draft.clone();
                        draft.cloud = Some(location);
                        ready(Reply::DraftCommitted(draft));
                    },
                ) {
                    Ok(d) => Ok(Reply::Saved(d, None)),
                    Err(e) => Ok(Reply::Saved(intent.draft, Some(e.to_string()))),
                };
            }
            let mut local = Vec::new();
            let mut save_error = None;
            for entry in fs::read_dir(&base)? {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let copy = entry.path().join("primary");
                if copy.join("sent").exists() {
                    continue;
                }
                if let Some(intent) =
                    read_optional::<operations::FrozenSend>(&copy.join("save-pending.json"))?
                {
                    let result = finish_cloud_save(&roots.state, &team, &copy, &intent);
                    let draft = result.unwrap_or_else(|e| {
                        save_error = Some(e);
                        intent.draft
                    });
                    local.push((draft, copy));
                } else if let Some(draft) = read_optional::<Draft>(&copy.join("editing.json"))? {
                    local.push((draft, copy))
                }
            }
            let remote = crate::adapters::git::drafts::discover_verified(
                &roots.state,
                &team.team,
                &team.remote,
                entry.actor(&team),
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
            let drafts: Vec<_> = local.into_iter().map(|(d, _)| d).collect();
            write_local(&base.join("current-list.json"), &drafts)?;
            if let Some(error) = save_error {
                return Err(error);
            }
            Ok(Reply::Drafts(drafts))
        }
        Request::Tags | Request::ToggleTag(_) => {
            let p = current.ok_or_else(|| io::Error::other("Team required"))?;
            let list = match request {
                Request::ToggleTag(name) => {
                    crate::adapters::local::tags::toggle(&roots.state, &roots.work, &p.team, &name)?
                }
                _ => crate::adapters::local::tags::load(&roots.state, &p.team)?,
            };
            Ok(Reply::Tags(list))
        }
        Request::SpaceSync { space, action } => {
            let p = current.ok_or_else(|| io::Error::other("Team required"))?;
            let sync = crate::adapters::git::workspace_sync::Space::open(
                &roots.state,
                &roots.work,
                &p.team,
                &space,
            )?;
            if matches!(action, SyncAction::RefreshPreview | SyncAction::Apply(_)) {
                operations::published_snapshot(&roots.state, &p.team, &p.remote)?;
            }
            match action {
                SyncAction::SetMode(mode) => sync.set_mode(mode)?,
                SyncAction::Apply(plan) => {
                    return Ok(Reply::SpaceSync(sync.sync_and_cache(Some(&plan))?));
                }
                SyncAction::Status | SyncAction::RefreshPreview => {}
            }
            Ok(Reply::SpaceSync(sync.preview()?))
        }
        Request::WorkspaceAction { space, action } => {
            let profile = current.ok_or_else(|| io::Error::other("Team required"))?;
            let result = operations::workspace_action_observed(
                &roots.state,
                &roots.work,
                &profile.team,
                &profile.remote,
                &profile.human,
                &space,
                action,
                &mut || ready(Reply::Committed),
            )?;
            let sync = crate::adapters::git::workspace_sync::Space::open(
                &roots.state,
                &roots.work,
                &profile.team,
                &space,
            )?;
            match result {
                WorkResult::Saved | WorkResult::Conflict => sync.mark_saved()?,
                WorkResult::Published => sync.mark_published()?,
                _ => {}
            }
            Ok(Reply::Work(result))
        }
        Request::Workspace => workspace_tree(
            &roots,
            &current
                .ok_or_else(|| io::Error::other("Team required"))?
                .team,
        ),
        _ => Err(io::Error::other("Unexpected request")),
    }
}
fn workspace_tree(
    roots: &bootstrap::Roots,
    team: &crate::application::setup::NewTeam,
) -> io::Result<Reply> {
    use crate::adapters::git::ignored::Ignored;
    let root = &roots.work;
    let repo = roots.state.join("teams").join(team.id()).join("repo.git");
    let tracked = Ignored::known(&repo)?;
    let ignored = Ignored::read(root, &repo, tracked)?;
    let mut names = Vec::new();
    fn visit(
        root: &Path,
        dir: &Path,
        names: &mut Vec<String>,
        ignored: &Ignored,
    ) -> io::Result<()> {
        for item in fs::read_dir(dir)? {
            let item = item?;
            let path = item.path();
            let name = item.file_name();
            if ignored.excludes(path.strip_prefix(root).map_err(io::Error::other)?) {
                continue;
            }
            if name.to_string_lossy().eq_ignore_ascii_case(".git")
                || name.to_string_lossy().starts_with(".synod-sync-")
            {
                continue;
            }
            names.push(
                path.strip_prefix(root)
                    .map_err(io::Error::other)?
                    .display()
                    .to_string(),
            );
            if item.file_type()?.is_dir() {
                visit(root, &path, names, ignored)?
            }
        }
        Ok(())
    }
    visit(root, root, &mut names, &ignored)?;
    names.sort();
    Ok(Reply::Files(names))
}
fn board_reply(
    state: &Path,
    team: &Profile,
    request: Request,
    entry: Entry,
    letters: &mut Vec<PublishedLetter>,
) -> io::Result<Reply> {
    let actor = entry.actor(&team);
    let receipts = state
        .join("teams")
        .join(team.team.id())
        .join("notices")
        .join(actor);
    for letter in letters.iter_mut() {
        let event = letter
            .events
            .first()
            .ok_or_else(|| io::Error::other("Missing send event"))?;
        let directed =
            !actor.is_empty() && (letter.to == actor || letter.cc.iter().any(|c| c == actor));
        letter.unread = directed && !receipts.join(&event.event_id).exists();
        if letter.unread && matches!(&request,Request::Read(id) if id==&letter.id) {
            fs::create_dir_all(&receipts)?;
            fs::write(receipts.join(&event.event_id), b"read")?;
            letter.unread = false;
        }
    }

    let letters = std::mem::take(letters);
    if let Request::Read(id) = request {
        return letters
            .into_iter()
            .find(|l| l.id == id)
            .map(Reply::Letter)
            .ok_or_else(|| io::Error::other("Unknown published letter"));
    }
    Ok(Reply::Published(letters))
}

fn local_drafts<T: serde::de::DeserializeOwned>(base: &Path) -> io::Result<Vec<T>> {
    if let Some(rows) = read_optional(&base.join("current-list.json"))? {
        return Ok(rows);
    }
    let entries = match fs::read_dir(base) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut rows = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let copy = entry.path().join("primary");
        if !copy.join("sent").exists()
            && let Some(row) = read_optional(&copy.join("editing.json"))?
        {
            rows.push(row)
        }
    }
    Ok(rows)
}

fn local_board(state: &Path, p: &Profile, entry: Entry) -> io::Result<Reply> {
    #[derive(serde::Deserialize)]
    struct Projection {
        remote: String,
        team: crate::application::setup::NewTeam,
        letters: Vec<Row>,
    }
    #[derive(serde::Deserialize)]
    struct Row {
        id: String,
        title: String,
        author: String,
        to: String,
        cc: Vec<String>,
        events: Vec<EventId>,
    }
    #[derive(serde::Deserialize)]
    struct EventId {
        event_id: String,
    }
    let team = state.join("teams").join(p.team.id());
    let actor = entry.actor(p);
    let receipts = team.join("notices").join(actor);
    let mut letters = Vec::new();
    let path = team.join("verified-board.json");
    // Parse from bytes so skipping a large body does not require one system call per byte.
    match fs::read(path) {
        Ok(bytes) => {
            let projection: Projection = serde_json::from_slice(&bytes)?;
            if projection.remote == p.remote && projection.team.id() == p.team.id() {
                for row in projection.letters {
                    let directed = !actor.is_empty()
                        && (row.to == actor || row.cc.iter().any(|id| id == actor));
                    let unread = directed
                        && row
                            .events
                            .first()
                            .is_some_and(|e| !receipts.join(&e.event_id).exists());
                    letters.push(LetterRow {
                        id: row.id,
                        title: row.title,
                        author: row.author,
                        to: row.to,
                        cc: row.cc,
                        unread,
                    });
                }
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let drafts = if entry.bound(p) {
        local_drafts(&team.join("drafts").join(actor))?
    } else {
        Vec::new()
    };
    Ok(Reply::Board {
        actor: actor.into(),
        letters,
        drafts,
    })
}

pub(super) fn cached_query(
    file: &Path,
    request: &Request,
    entry: Entry,
) -> io::Result<Option<Reply>> {
    if !matches!(
        request,
        Request::Workspace
            | Request::Identities { .. }
            | Request::ToggleTag(_)
            | Request::Tags
            | Request::SpaceSync {
                action: SyncAction::Status | SyncAction::SetMode(_),
                ..
            }
            | Request::Theme
            | Request::SetupPending
            | Request::Interval
            | Request::Board
            | Request::OpenDraft(_)
            | Request::Published
            | Request::Read(_)
            | Request::Profile
            | Request::Drafts
    ) {
        return Ok(None);
    }
    let cfg = ClientConfig::read(file)?;
    match request {
        Request::Theme => return Ok(Some(Reply::Color(cfg.theme_rgb))),
        Request::Interval => return Ok(Some(Reply::Interval(cfg.sync_interval_seconds))),
        Request::SetupPending => {
            return Ok(Some(Reply::Pending(
                cfg.fields
                    .get("setup_members_pending")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            )));
        }
        _ => {}
    }
    let Some(p) = profile(&cfg)? else {
        return if matches!(request, Request::Profile) && matches!(entry, Entry::Human) {
            Ok(Some(Reply::Team(None)))
        } else {
            Ok(None)
        };
    };
    if matches!(entry, Entry::Assistant) && !entry.bound(&p) {
        return Err(io::Error::other(
            "Select an active assistant identity in preferences",
        ));
    }
    if matches!(request, Request::Profile) {
        return Ok(Some(Reply::Team(Some(p))));
    }
    if let Request::Identities { human, assistant } = request {
        if cfg.pending_team.is_some() {
            return Ok(None);
        }
        let (human, assistant) =
            crate::application::setup::Activation::parse(human, assistant)?.slots();
        return ClientConfig::try_update(file, |current| {
            let mut profile = profile(current)?.ok_or_else(|| io::Error::other("Team required"))?;
            if current.pending_team.is_some()
                || !(human.is_empty() || profile.team.human(human))
                || !(assistant.is_empty() || profile.team.assistant(assistant))
            {
                return Err(io::Error::other(
                    "Unknown member or incorrect identity slot",
                ));
            }
            profile.human = human.into();
            profile.assistant = assistant.into();
            current
                .fields
                .insert("profile".into(), serde_json::to_value(&profile)?);
            Ok(Some(Reply::Team(Some(profile))))
        });
    }
    let roots = bootstrap::configured(file)?.ok_or_else(|| io::Error::other("Setup required"))?;
    match request {
        Request::ToggleTag(name) => {
            return Ok(Some(Reply::Tags(crate::adapters::local::tags::toggle(
                &roots.state,
                &roots.work,
                &p.team,
                name,
            )?)));
        }
        Request::SpaceSync {
            space,
            action: SyncAction::SetMode(mode),
        } => {
            let local = crate::adapters::git::workspace_sync::Space::open(
                &roots.state,
                &roots.work,
                &p.team,
                space,
            )?;
            local.set_mode(*mode)?;
            return Ok(Some(Reply::SpaceSync(
                crate::adapters::git::workspace_sync::Space::status(&roots.state, &p.team, space)?,
            )));
        }
        Request::Workspace => return workspace_tree(&roots, &p.team).map(Some),
        Request::Tags => {
            return Ok(Some(Reply::Tags(crate::adapters::local::tags::view(
                &roots.state,
                &p.team,
            )?)));
        }
        Request::SpaceSync {
            space,
            action: SyncAction::Status,
        } => {
            return Ok(Some(Reply::SpaceSync(
                crate::adapters::git::workspace_sync::Space::status(&roots.state, &p.team, space)?,
            )));
        }
        _ => {}
    }
    if matches!(request, Request::Board) {
        return Ok(Some(local_board(&roots.state, &p, entry)?));
    }
    if let Request::OpenDraft(id) = request {
        if !entry.bound(&p)
            || !id
                .strip_prefix("letter-local-")
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(io::Error::other("Invalid draft reference"));
        }
        if let Some(draft) = delivery::editable(
            &roots.state.join("teams").join(p.team.id()),
            entry.actor(&p),
            id,
        )? {
            return Ok(Some(Reply::Draft(draft)));
        }
        let copy = roots
            .state
            .join("teams")
            .join(p.team.id())
            .join("drafts")
            .join(entry.actor(&p))
            .join(id)
            .join("primary");
        if copy.join("sent").exists() {
            return Err(io::Error::other("Draft already sent"));
        }
        return Ok(Some(Reply::Draft(serde_json::from_slice(&fs::read(
            copy.join("editing.json"),
        )?)?)));
    }
    if matches!(request, Request::Drafts) {
        let drafts = if entry.bound(&p) {
            local_drafts(
                &roots
                    .state
                    .join("teams")
                    .join(p.team.id())
                    .join("drafts")
                    .join(entry.actor(&p)),
            )?
        } else {
            Vec::new()
        };
        return Ok(Some(Reply::Drafts(drafts)));
    }
    let mut letters = operations::cached_board(&roots.state, &p.team, &p.remote)?
        .map(|b| b.letters)
        .unwrap_or_default();
    let query = match request {
        Request::Read(id) => Request::Read(id.clone()),
        _ => Request::Published,
    };
    Ok(Some(board_reply(
        &roots.state,
        &p,
        query,
        entry,
        &mut letters,
    )?))
}

pub(super) fn synchronize(file: &Path) -> io::Result<()> {
    let config = ClientConfig::read(file)?;
    let Some(p) = profile(&config)? else {
        return Ok(());
    };
    if config.pending_team.is_some() {
        return Ok(());
    }
    delivery::drain(file, &p)?;
    perform_as(file, Request::Published, Entry::Human)?;
    let roots = bootstrap::configured(file)?.ok_or_else(|| io::Error::other("Setup required"))?;
    let mut result = retry_outbox(&roots.state, &p).and(operations::retry_personal(
        &roots.state,
        &p.team,
        &p.remote,
    ));
    for (actor, _) in p.team.roster() {
        if actor == p.human || actor == p.assistant {
            continue;
        }
        let dir = roots
            .state
            .join("teams")
            .join(p.team.id())
            .join("drafts")
            .join(actor);
        if !dir.is_dir() {
            continue;
        }
        for item in fs::read_dir(dir)? {
            let item = item?;
            if !item.file_type()?.is_dir() {
                continue;
            }
            let copy = item.path().join("primary");
            if let Some(intent) =
                read_optional::<operations::FrozenSend>(&copy.join("save-pending.json"))?
                && intent.team == p.team.id()
                && intent.actor == actor
            {
                result =
                    result.and(finish_cloud_save(&roots.state, &p, &copy, &intent).map(|_| ()));
            }
        }
    }
    for entry in [Entry::Human, Entry::Assistant] {
        if entry.bound(&p) {
            result = result.and(perform_as(file, Request::Drafts, entry).map(|_| ()));
        }
    }
    for space in crate::adapters::local::tags::load(&roots.state, &p.team)?.keys() {
        let sync = crate::adapters::git::workspace_sync::Space::open(
            &roots.state,
            &roots.work,
            &p.team,
            space,
        );
        result = result.and(sync.and_then(|s| s.sync_and_cache(None).map(|_| ())));
    }
    result
}
fn retry_outbox(state: &Path, profile: &Profile) -> io::Result<()> {
    use crate::application::letters::TaskChange;
    let base = state.join("teams").join(profile.team.id());
    let items = match fs::read_dir(base.join("outbox")) {
        Ok(items) => items,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let mut pending = items.collect::<io::Result<Vec<_>>>()?;
    pending.sort_by_key(|e| e.file_name());
    fn retry(item: fs::DirEntry, base: &Path, state: &Path, profile: &Profile) -> io::Result<()> {
        if !item.file_type()?.is_dir() {
            return Ok(());
        }
        let dir = item.path();
        let path = dir.join("intent.json");
        if dir.join("confirmed").exists() || !path.exists() {
            return Ok(());
        }
        let key = item.file_name();
        let key = key
            .to_str()
            .ok_or_else(|| io::Error::other("Invalid outbox identity"))?;
        if key.starts_with("task-") {
            let (remote, letter, change, intent): (
                String,
                String,
                TaskChange,
                operations::InitialCommit,
            ) = serde_json::from_slice(&fs::read(&path)?)?;
            if remote != profile.remote || intent.operation != key {
                return Ok(());
            }
            operations::change_task(state, &profile.team, &remote, &letter, &intent, &change)?;
        } else {
            let intent: operations::FrozenSend = serde_json::from_slice(&fs::read(&path)?)?;
            if intent.team != profile.team.id()
                || intent.key != key
                || !(profile.team.human(&intent.actor) || profile.team.assistant(&intent.actor))
            {
                return Ok(());
            }
            let Some(suffix) = intent.draft.reference.strip_prefix("letter-local-") else {
                return Ok(());
            };
            if suffix.is_empty() || !suffix.bytes().all(|b| b.is_ascii_digit()) {
                return Ok(());
            }
            let copy = base
                .join("drafts")
                .join(&intent.actor)
                .join(&intent.draft.reference)
                .join("primary");
            let Some(active) = read_optional::<operations::FrozenSend>(&copy.join("send.json"))?
            else {
                return Ok(());
            };
            if active.key != intent.key {
                return Ok(());
            }
            operations::send_letter(state, &profile.team, &profile.remote, &intent)?;
            fs::write(copy.join("sent"), &intent.key)?;
            fs::remove_file(copy.join("send.json"))?;
        }
        fs::remove_file(path)?;
        fs::remove_dir(dir)
    }
    let mut result = Ok(());
    for item in pending {
        result = result.and(retry(item, &base, state, profile));
    }
    result
}
fn read_optional<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::File::open(path) {
        Ok(f) => Ok(Some(serde_json::from_reader(std::io::BufReader::new(f))?)),
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
    finish_cloud_save_observed(root, profile, copy, intent, &mut |_| {})
}
fn finish_cloud_save_observed(
    root: &Path,
    profile: &Profile,
    copy: &Path,
    intent: &operations::FrozenSend,
    ready: &mut dyn FnMut(crate::application::letters::CloudDraft),
) -> io::Result<Draft> {
    let location = crate::adapters::git::drafts::save_observed(
        root,
        &profile.team,
        &profile.remote,
        intent,
        &mut |location| {
            let mut local = intent.draft.clone();
            local.cloud = Some(location.clone());
            write_local(&copy.join("editing.json"), &local)?;
            ready(location);
            Ok(())
        },
    )?;
    let mut draft = intent.draft.clone();
    draft.cloud = Some(location);
    write_local(&copy.join("editing.json"), &draft)?;
    fs::remove_file(copy.join("save-pending.json"))?;
    Ok(draft)
}
fn save_session(config: &ClientConfig, file: &Path, preserve_slots: bool) -> io::Result<()> {
    ClientConfig::try_update(file, |current| {
        let active = profile(current)?;
        let custom = current.fields.remove("custom_theme");
        current.fields = config.fields.clone();
        current.fields.remove("custom_theme");
        if let Some(custom) = custom {
            current.fields.insert("custom_theme".into(), custom);
        }
        current.pending_team = config.pending_team.clone();
        if preserve_slots
            && let (Some(active), Some(mut updated)) = (active, profile(current)?)
            && active.team.id() == updated.team.id()
        {
            updated.human = active.human;
            updated.assistant = active.assistant;
            current
                .fields
                .insert("profile".into(), serde_json::to_value(updated)?);
        }
        Ok(())
    })
}
fn save_profile(config: ClientConfig, file: &Path, p: Profile) -> io::Result<Reply> {
    store_profile(config, file, p, false)
}
fn store_profile(
    mut config: ClientConfig,
    file: &Path,
    p: Profile,
    preserve_slots: bool,
) -> io::Result<Reply> {
    config
        .fields
        .insert("profile".into(), serde_json::to_value(&p)?);
    save_session(&config, file, preserve_slots)?;
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

#[path = "delivery.rs"]
pub(super) mod delivery;
