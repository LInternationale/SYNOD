//! Verify the first-parent history before exposing a published bulletin.
use super::{
    commit_message::{self, Operation},
    operations::git,
};
use crate::application::letters::LetterEvent;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
};
fn invalid() -> io::Error {
    io::Error::other("Unrecognized or inconsistent team history; keeping the previous view")
}
fn safe(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}
pub(super) fn replay(dir: &Path, team: &str) -> io::Result<BTreeMap<String, Vec<LetterEvent>>> {
    let commits = String::from_utf8(git(
        dir,
        &[
            "rev-list",
            "--first-parent",
            "--reverse",
            "--parents",
            "FETCH_HEAD",
        ],
    )?)
    .map_err(io::Error::other)?;
    let mut letters = BTreeMap::<String, Vec<LetterEvent>>::new();
    let mut operations = BTreeSet::new();
    for line in commits.lines() {
        let chain = line.split_whitespace().collect::<Vec<_>>();
        let sha = chain[0];
        let parents = &chain[1..];
        let obj = git(dir, &["cat-file", "commit", sha])?;
        let offset = obj
            .windows(2)
            .position(|v| v == b"\n\n")
            .ok_or_else(invalid)?;
        let msg = commit_message::decode(
            std::str::from_utf8(&obj[offset + 2..]).map_err(io::Error::other)?,
        )
        .map_err(|_| invalid())?;
        if msg.team_id != team || !operations.insert(msg.operation_id) || !safe(&msg.actor_id) {
            return Err(invalid());
        }
        let raw = if let Some(p) = parents.first() {
            git(
                dir,
                &["diff", "--no-renames", "--name-status", "-z", p, sha],
            )?
        } else {
            git(
                dir,
                &[
                    "diff-tree",
                    "--root",
                    "--no-commit-id",
                    "--no-renames",
                    "-r",
                    "--name-status",
                    "-z",
                    sha,
                ],
            )?
        };
        let parts: Vec<_> = raw.split(|b| *b == 0).filter(|p| !p.is_empty()).collect();
        if parts.len() % 2 != 0 {
            return Err(invalid());
        }
        let changes = parts
            .chunks_exact(2)
            .map(|pair| (pair[0], pair[1]))
            .collect::<Vec<_>>();
        let mut subtasks = None;
        let mut checklist_change = None;
        let (letter, event, kind) = match msg.operation {
            Operation::TeamConfigured => {
                if parents.len() > 1
                    || changes.len() != 1
                    || changes[0].1 != b"team.json"
                    || !matches!(changes[0].0, b"A" | b"M")
                {
                    return Err(invalid());
                }
                continue;
            }
            Operation::WorkspacePublished(t) => {
                let prefix = format!("workspace/{}/", t.space_id);
                if !safe(&t.space_id)
                    || parents.is_empty()
                    || parents.len() > 2
                    || changes
                        .iter()
                        .any(|(_, p)| !p.starts_with(prefix.as_bytes()))
                {
                    return Err(invalid());
                }
                continue;
            }
            Operation::LetterSent(t) => {
                if parents.len() != 2 || letters.contains_key(&t.letter_id) {
                    return Err(invalid());
                }
                (t.letter_id, t.event_id, "LetterSent".to_owned())
            }
            Operation::LetterEvent(t) => {
                if parents.len() != 1
                    || !letters.contains_key(&t.letter_id)
                    || !matches!(
                        t.event_kind.as_str(),
                        "MarkedCompleted"
                            | "MarkedIncomplete"
                            | "ChecklistSet"
                            | "SubtasksSet"
                            | "ProgressReported"
                            | "DependencyAdded"
                            | "DependencyRemoved"
                    )
                {
                    return Err(invalid());
                }
                if msg.version == 2 {
                    if !matches!(
                        t.event_kind.as_str(),
                        "MarkedCompleted" | "MarkedIncomplete" | "SubtasksSet" | "ChecklistSet"
                    ) || (t.event_kind == "SubtasksSet") != t.subtasks.is_some()
                        || (t.event_kind == "ChecklistSet") != t.checklist_change.is_some()
                    {
                        return Err(invalid());
                    }
                    subtasks = t.subtasks;
                    checklist_change = t.checklist_change;
                }
                (t.letter_id, t.event_id, t.event_kind)
            }
            _ => return Err(invalid()),
        };
        if !safe(&letter) || !safe(&event) {
            return Err(invalid());
        }
        let prefix = format!("letters/{letter}/");
        let event_path = format!("{prefix}events/{event}.json");
        if msg.version == 1
            && !changes
                .iter()
                .any(|(s, p)| *s == b"A" && *p == event_path.as_bytes())
        {
            return Err(invalid());
        }
        if kind == "LetterSent" {
            for name in ["envelope.json", "body.md"] {
                if !changes
                    .iter()
                    .any(|(_, p)| *p == format!("{prefix}{name}").as_bytes())
                {
                    return Err(invalid());
                }
            }
            for (status, path) in &changes {
                let path = std::str::from_utf8(path).map_err(io::Error::other)?;
                if *status != b"A" {
                    return Err(invalid());
                }
                if msg.version == 1 && path == event_path {
                    continue;
                }
                let suffix = path.strip_prefix(&prefix).ok_or_else(invalid)?;
                if !matches!(suffix, "body.md" | "envelope.json")
                    && !suffix.strip_prefix("attachments/").is_some_and(safe)
                {
                    return Err(invalid());
                }
                if git(dir, &["rev-parse", &format!("{sha}:{path}")])?
                    != git(dir, &["rev-parse", &format!("{}:{path}", parents[1])])?
                {
                    return Err(invalid());
                }
            }
        } else if changes.len() != usize::from(msg.version == 1) {
            return Err(invalid());
        }
        let fact: LetterEvent = if msg.version == 1 {
            serde_json::from_slice(&git(dir, &["show", &format!("{sha}:{event_path}")])?)?
        } else {
            LetterEvent {
                schema_version: 1,
                letter_id: letter.clone(),
                event_id: event.clone(),
                kind: kind.clone(),
                actor_id: msg.actor_id.clone(),
                occurred_at: msg.occurred_at.to_string(),
                subtasks,
                checklist_change,
                percent: None,
                dependency: None,
            }
        };
        if fact.schema_version != 1
            || fact.letter_id != letter
            || fact.actor_id != msg.actor_id
            || fact.event_id != event
            || fact.kind != kind
            || fact.occurred_at.parse::<toml::value::Datetime>().ok() != Some(msg.occurred_at)
        {
            return Err(invalid());
        }
        letters.entry(letter).or_default().push(fact);
    }
    Ok(letters)
}
