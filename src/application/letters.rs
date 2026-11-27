//! Letter composition and publication views.
use serde::{Deserialize, Serialize};
#[derive(Default, Clone, Eq, PartialEq, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct Draft {
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud: Option<CloudDraft>,
    pub title: String,
    pub to: String,
    pub cc: String,
    pub body: String,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct Attachment {
    pub filename: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PublishedLetter {
    pub id: String,
    pub title: String,
    pub author: String,
    pub to: String,
    pub cc: Vec<String>,
    pub references: Vec<String>,
    pub dependencies: Vec<String>,
    pub progress: u8,
    pub progress_done: usize,
    pub progress_total: usize,
    pub completed: bool,
    pub checklist: Vec<ChecklistItem>,
    #[serde(default)]
    pub unread: bool,
    pub events: Vec<LetterEvent>,
    pub body: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChecklistItem {
    pub line: usize,
    pub label: String,
    pub checked: bool,
}

/// Read task markers from the immutable Markdown body.
///
/// ```
/// use synod::application::letters::checklist;
/// assert!(checklist("- [x] shipped")[0].checked);
/// ```
pub fn checklist(body: &str) -> Vec<ChecklistItem> {
    let mut items = Vec::new();
    let mut fence = None;
    for (line, raw) in body.lines().enumerate() {
        let text = raw.trim_start();
        let first = text.chars().next();
        if matches!(first, Some('`' | '~')) {
            let width = text.chars().take_while(|c| Some(*c) == first).count();
            if width >= 3 {
                match fence {
                    Some((marker, min))
                        if marker == first && width >= min && text[width..].trim().is_empty() =>
                    {
                        fence = None
                    }
                    None => fence = Some((first, width)),
                    _ => {}
                }
                continue;
            }
        }
        if fence.is_some() {
            continue;
        }
        let Some(marker) = text.strip_prefix("- [") else {
            continue;
        };
        let Some((state, label)) = marker.split_once(']') else {
            continue;
        };
        if !label.is_empty() && !label.starts_with(char::is_whitespace) {
            continue;
        }
        let checked = match state {
            " " => false,
            "x" | "X" => true,
            _ => continue,
        };
        items.push(ChecklistItem {
            line,
            label: label.trim_start().into(),
            checked,
        });
    }
    items
}

impl Draft {
    pub fn replying_to(letter: &PublishedLetter, board: &[PublishedLetter]) -> Self {
        Self::reply_to(
            &letter.id,
            &letter.title,
            &letter.author,
            board.iter().map(|l| l.title.as_str()),
        )
    }
    pub(crate) fn reply_to<'a>(
        id: &str,
        subject: &str,
        author: &str,
        titles: impl Iterator<Item = &'a str>,
    ) -> Self {
        let titles: std::collections::HashSet<_> = titles.map(str::trim).collect();
        let base = format!("Re: {subject}");
        let mut title = base.clone();
        let mut suffix = 2;
        while titles.contains(title.trim()) {
            title = format!("{base} - {suffix}");
            suffix += 1;
        }
        Self {
            title,
            to: author.into(),
            references: vec![id.into()],
            ..Self::default()
        }
    }
}

/// Remote revision observed when this draft copy was saved or opened.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, rmcp::schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CloudDraft {
    pub letter: String,
    pub copy: String,
    pub revision: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LetterEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub letter_id: String,
    pub actor_id: String,
    pub occurred_at: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checklist_change: Option<ChecklistChange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtasks: Option<Vec<String>>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChecklistChange {
    pub line: usize,
    pub checked: bool,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum TaskChange {
    Checklist(ChecklistChange),
    Completion(bool),
    Subtasks {
        selected: Vec<String>,
        observed: Vec<String>,
    },
}

/// Compute per-letter progress from the distinct reachable nodes and their manual flags.
pub fn project_progress(letters: &mut [PublishedLetter]) -> std::io::Result<()> {
    use std::collections::{HashMap, HashSet, VecDeque};
    let index: HashMap<_, _> = letters
        .iter()
        .enumerate()
        .map(|(i, l)| (l.id.as_str(), i))
        .collect();
    let edges = letters
        .iter()
        .map(|l| {
            l.dependencies
                .iter()
                .map(|id| {
                    index
                        .get(id.as_str())
                        .copied()
                        .ok_or_else(|| std::io::Error::other("Unknown subtask in history"))
                })
                .collect::<std::io::Result<Vec<_>>>()
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let mut incoming = vec![0; edges.len()];
    for children in &edges {
        let mut unique = HashSet::new();
        for &child in children {
            if !unique.insert(child) {
                return Err(std::io::Error::other("Repeated subtask in history"));
            }
            incoming[child] += 1
        }
    }
    let mut queue: VecDeque<_> = (0..edges.len()).filter(|i| incoming[*i] == 0).collect();
    let mut seen = 0;
    while let Some(i) = queue.pop_front() {
        seen += 1;
        for &child in &edges[i] {
            incoming[child] -= 1;
            if incoming[child] == 0 {
                queue.push_back(child)
            }
        }
    }
    if seen != edges.len() {
        return Err(std::io::Error::other("Cyclic subtask history"));
    }
    let flags: Vec<_> = letters.iter().map(|l| l.completed).collect();
    let units: Vec<_> = letters
        .iter()
        .map(|l| {
            (
                1 + l.checklist.len(),
                l.checklist.iter().filter(|item| item.checked).count(),
            )
        })
        .collect();
    for (root, l) in letters.iter_mut().enumerate() {
        let mut visited = HashSet::new();
        let mut total = HashSet::new();
        let mut done = HashSet::new();
        let mut stack = vec![(root, false)];
        while let Some((node, inherited)) = stack.pop() {
            let covered = inherited || flags[node];
            if !visited.insert((node, covered)) {
                continue;
            }
            total.insert(node);
            if covered {
                done.insert(node);
            }
            stack.extend(edges[node].iter().map(|child| (*child, covered)));
        }
        l.progress_done = total
            .iter()
            .map(|n| {
                if done.contains(n) {
                    units[*n].0
                } else {
                    units[*n].1
                }
            })
            .sum();
        l.progress_total = total.iter().map(|n| units[*n].0).sum();
        l.progress = ((l.progress_done as u128 * 100) / (l.progress_total as u128)) as u8;
    }
    Ok(())
}

/// Ordered remote operations for a frozen outgoing letter.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DeliveryStep {
    Pull,
    Push,
    Merge,
}
