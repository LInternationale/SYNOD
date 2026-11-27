//! Durable delivery intents owned by the local session.
use super::*;
use std::path::PathBuf;

fn team_root(file: &Path, p: &Profile) -> io::Result<PathBuf> {
    Ok(bootstrap::configured(file)?
        .ok_or_else(|| io::Error::other("Setup required"))?
        .state
        .join("teams")
        .join(p.team.id()))
}

pub(in crate::session) fn enqueue(file: &Path, entry: Entry, mut draft: Draft) -> io::Result<()> {
    let p =
        profile(&ClientConfig::read(file)?)?.ok_or_else(|| io::Error::other("Team required"))?;
    if !entry.bound(&p) {
        return Err(io::Error::other("Select an active identity"));
    }
    let team = team_root(file, &p)?;
    let base = team.join("drafts").join(entry.actor(&p));
    fs::create_dir_all(&base)?;
    let dir = team.join("deliveries");
    fs::create_dir_all(&dir)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("owner.lock"))?;
    lock.lock()?;
    let copy = editing_copy(&base, &mut draft)?;
    if copy.join("sent").exists() {
        return Err(io::Error::other("Draft already sent"));
    }
    for mut row in rows(&dir)? {
        if row.actor == entry.actor(&p) && row.reference == draft.reference {
            let mut frozen: operations::FrozenSend =
                serde_json::from_slice(&fs::read(dir.join(&row.key).join("intent.json"))?)?;
            if matches!(row.state, DeliveryState::Failed(..)) {
                frozen.draft = draft;
                row.title = frozen.draft.title.clone();
                row.state = DeliveryState::Waiting;
                let item = dir.join(&row.key);
                write_local(&item.join("intent.json"), &frozen)?;
                write_local(&item.join("state.json"), &row)?;
                return Ok(());
            }
            return if frozen.draft == draft {
                Ok(())
            } else {
                Err(io::Error::other("This draft already has a queued delivery"))
            };
        }
    }
    let mut intent = operations::FrozenSend::new(entry.actor(&p), p.team.id(), draft)?;
    let identity = copy.join("delivery-id.json");
    if let Some(key) = read_optional::<String>(&identity)? {
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
            return Err(io::Error::other("Invalid local delivery identity"));
        }
        intent.key = key;
    } else {
        write_local(&identity, &intent.key)?;
    }
    let item = dir.join(&intent.key);
    if item.join("state.json").exists() {
        return Err(io::Error::other(
            "Delivery identity already belongs to another draft",
        ));
    }
    fs::create_dir_all(&item)?;
    write_local(&item.join("intent.json"), &intent)?;
    write_local(&item.join("remote.json"), &p.remote)?;
    write_local(
        &item.join("state.json"),
        &DeliveryRow {
            key: intent.key.clone(),
            actor: intent.actor.clone(),
            reference: intent.draft.reference.clone(),
            letter: intent.letter(),
            title: intent.draft.title.clone(),
            state: DeliveryState::Waiting,
        },
    )?;
    fs::File::open(&dir)?.sync_all()
}

fn rows(dir: &Path) -> io::Result<Vec<DeliveryRow>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for item in fs::read_dir(dir)? {
        let item = item?;
        if item.file_type()?.is_dir()
            && let Some(row) = read_optional::<DeliveryRow>(&item.path().join("state.json"))?
        {
            if item.file_name().to_str() != Some(row.key.as_str())
                || !row.key.bytes().all(|b| b.is_ascii_digit() || b == b'-')
                || !row
                    .reference
                    .strip_prefix("letter-local-")
                    .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                || row.actor.is_empty()
                || !row
                    .actor
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            {
                return Err(io::Error::other("Invalid delivery metadata"));
            }
            rows.push(row);
        }
    }
    rows.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(rows)
}
pub(in crate::session) fn snapshot(file: &Path) -> io::Result<(Vec<DeliveryRow>, Vec<String>)> {
    let Some(p) = profile(&ClientConfig::read(file)?)? else {
        return Ok((Vec::new(), Vec::new()));
    };
    let dir = team_root(file, &p)?.join("deliveries");
    let mut completed =
        read_optional::<Vec<Completed>>(&dir.join("completed.json"))?.unwrap_or_default();
    completed.truncate(ClientConfig::read(file)?.completed_rows);
    Ok((
        rows(&dir)?,
        completed.into_iter().map(|row| row.label).collect(),
    ))
}
pub(super) fn drain(file: &Path, p: &Profile) -> io::Result<()> {
    let state = bootstrap::configured(file)?
        .ok_or_else(|| io::Error::other("Setup required"))?
        .state;
    let root = state.join("teams").join(p.team.id());
    let dir = root.join("deliveries");
    for mut row in rows(&dir)? {
        if matches!(row.state, DeliveryState::Failed(..)) {
            continue;
        }
        let item = dir.join(&row.key);
        let intent: operations::FrozenSend =
            serde_json::from_slice(&fs::read(item.join("intent.json"))?)?;
        let remote: String = serde_json::from_slice(&fs::read(item.join("remote.json"))?)?;
        if row.actor != intent.actor
            || row.reference != intent.draft.reference
            || row.key != intent.key
            || intent.team != p.team.id()
        {
            return Err(io::Error::other("Inconsistent delivery intent"));
        }
        let state_file = item.join("state.json");
        let mut step = DeliveryStep::Pull;
        let result = if remote == p.remote && intent.key == row.key && intent.team == p.team.id() {
            operations::deliver(
                &state,
                &p.team,
                &remote,
                &intent,
                &mut || {},
                Some(&mut |current| {
                    if matches!(
                        (step, current),
                        (DeliveryStep::Pull, DeliveryStep::Push)
                            | (DeliveryStep::Push, DeliveryStep::Merge)
                    ) {
                        remember_steps(file, &row, &[step])?;
                    }
                    step = current;
                    row.state = DeliveryState::Running(current);
                    write_local(&state_file, &row)
                }),
            )
        } else {
            Err(io::Error::other("Delivery identity changed"))
        };
        match result {
            Ok(()) => {
                remember_steps(
                    file,
                    &row,
                    &[DeliveryStep::Pull, DeliveryStep::Push, DeliveryStep::Merge],
                )?;
                let base = root.join("drafts").join(&intent.actor);
                let copy = base.join(&intent.draft.reference).join("primary");
                write_local(&copy.join("sent"), &intent.key)?;
                match fs::remove_file(base.join("current-list.json")) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    r => r?,
                }
                fs::remove_file(state_file)?;
                fs::remove_file(item.join("remote.json"))?;
                fs::remove_file(item.join("intent.json"))?;
                fs::remove_dir(item)?;
            }
            Err(e) => {
                let base = root.join("drafts").join(&intent.actor);
                write_local(
                    &base.join(&row.reference).join("primary/editing.json"),
                    &intent.draft,
                )?;
                match fs::remove_file(base.join("current-list.json")) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    r => r?,
                }
                row.state = DeliveryState::Failed(step, e.to_string());
                write_local(&state_file, &row)?;
            }
        }
    }
    Ok(())
}

pub(super) fn editable(team: &Path, actor: &str, reference: &str) -> io::Result<Option<Draft>> {
    if reference.is_empty() {
        return Ok(None);
    }
    let dir = team.join("deliveries");
    for row in rows(&dir)? {
        if row.actor == actor && row.reference == reference {
            if !matches!(row.state, DeliveryState::Failed(..)) {
                return Err(io::Error::other("这封信正在寄送"));
            }
            let frozen: operations::FrozenSend =
                serde_json::from_slice(&fs::read(dir.join(row.key).join("intent.json"))?)?;
            return Ok(Some(frozen.draft));
        }
    }
    Ok(None)
}
pub(super) fn clear_failed(team: &Path, actor: &str, reference: &str) -> io::Result<()> {
    let dir = team.join("deliveries");
    if !dir.join("owner.lock").is_file() {
        return Ok(());
    }
    let lock = fs::OpenOptions::new()
        .write(true)
        .open(dir.join("owner.lock"))?;
    lock.lock()?;
    for row in rows(&dir)? {
        if row.actor == actor
            && row.reference == reference
            && matches!(row.state, DeliveryState::Failed(..))
        {
            let item = dir.join(row.key);
            for name in ["state.json", "intent.json", "remote.json"] {
                fs::remove_file(item.join(name))?;
            }
            fs::remove_dir(item)?;
        }
    }
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Completed {
    key: Option<String>,
    label: String,
}
fn remember_steps(file: &Path, row: &DeliveryRow, steps: &[DeliveryStep]) -> io::Result<()> {
    let records = steps
        .iter()
        .map(|step| {
            let name = match step {
                DeliveryStep::Pull => "pull",
                DeliveryStep::Push => "push",
                DeliveryStep::Merge => "merge",
            };
            Completed {
                key: Some(format!("{}-{name}", row.key)),
                label: format!("{name} · {}", row.title),
            }
        })
        .collect();
    store_completed(file, records)
}
pub(in crate::session) fn remember(file: &Path, labels: &[String]) -> io::Result<()> {
    store_completed(
        file,
        labels
            .iter()
            .map(|s| Completed {
                key: None,
                label: s.clone(),
            })
            .collect(),
    )
}
fn store_completed(file: &Path, records: Vec<Completed>) -> io::Result<()> {
    let config = ClientConfig::read(file)?;
    let Some(p) = profile(&config)? else {
        return Ok(());
    };
    let dir = team_root(file, &p)?.join("deliveries");
    fs::create_dir_all(&dir)?;
    let path = dir.join("completed.json");
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("completed.lock"))?;
    lock.lock()?;
    let mut recent = read_optional::<Vec<Completed>>(&path)?.unwrap_or_default();
    for record in records {
        if let Some(key) = &record.key {
            recent.retain(|entry| entry.key.as_ref() != Some(key));
        }
        recent.insert(0, record);
    }
    recent.truncate(ClientConfig::read(file)?.completed_rows);
    write_local(&path, &recent)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_letter_can_be_edited_saved_and_resent_after_restart() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("synod-delivery-test-{}", std::process::id()));
        fs::create_dir(&root)?;
        let file = root.join("client.json");
        let roots = bootstrap::initialize(&file, &root.join("state"), &root.join("work"))?;
        let team = serde_json::from_str(include_str!("../../tests/fixtures/v1/team/team.json"))?;
        let remote = root.join("remote.git");
        let p = Profile {
            team,
            remote: remote.display().to_string(),
            human: "member-alice".into(),
            assistant: String::new(),
        };
        ClientConfig::update(&file, |config| {
            config
                .fields
                .insert("profile".into(), serde_json::to_value(&p).unwrap());
        })?;
        let draft = Draft {
            title: "First title".into(),
            to: "member-chen".into(),
            body: "original".into(),
            ..Default::default()
        };
        enqueue(&file, Entry::Human, draft)?;
        let waiting = snapshot(&file)?.0;
        assert_eq!(waiting.len(), 1);
        assert!(matches!(waiting[0].state, DeliveryState::Waiting));
        let reference = &waiting[0].reference;
        drain(&file, &p)?;
        assert!(matches!(
            snapshot(&file)?.0[0].state,
            DeliveryState::Failed(DeliveryStep::Pull, _)
        ));
        let Reply::Draft(mut edited) = super::super::cached_query(
            &file,
            &Request::OpenDraft(reference.clone()),
            Entry::Human,
        )?
        .unwrap() else {
            panic!("missing failed draft")
        };
        edited.body = "recovered text".into();
        enqueue(&file, Entry::Human, edited.clone())?;
        assert_eq!(snapshot(&file)?.0[0].key, waiting[0].key);
        drain(&file, &p)?;
        assert!(matches!(
            snapshot(&file)?.0[0].state,
            DeliveryState::Failed(DeliveryStep::Pull, _)
        ));
        super::super::perform_as(&file, Request::SaveDraft(edited.clone()), Entry::Human)?;
        assert!(snapshot(&file)?.0.is_empty());
        assert!(
            std::process::Command::new("git")
                .args(["init", "--bare"])
                .arg(&remote)
                .output()?
                .status
                .success()
        );
        operations::publish_initial(
            &roots.state,
            &p.remote,
            &p.team,
            &operations::freeze_initial("member-alice")?,
        )?;
        enqueue(&file, Entry::Human, edited.clone())?;
        enqueue(&file, Entry::Human, edited)?;
        assert_eq!(snapshot(&file)?.0.len(), 1);
        drain(&file, &p)?;
        let (remaining, recent) = snapshot(&file)?;
        assert!(remaining.is_empty());
        assert_eq!(
            recent,
            [
                "merge · First title",
                "push · First title",
                "pull · First title"
            ]
        );
        let published = operations::published_letters(&roots.state, &p.team, &p.remote)?;
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].body, "recovered text");
        ClientConfig::update(&file, |c| c.completed_rows = 1)?;
        assert_eq!(snapshot(&file)?.1.len(), 1);
        fs::remove_dir_all(root)
    }
}
