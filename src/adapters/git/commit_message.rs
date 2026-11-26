//! Decoding for team repository commit messages.

use serde::Deserialize;
use toml::{Table, value::Datetime};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    kind: String,
    schema_version: u32,
    team_id: String,
    operation_id: String,
    actor_id: String,
    occurred_at: Datetime,
    target: Option<Table>,
}

/// A typed operation from a team's commit message.
#[derive(Debug)]
pub enum Operation {
    TeamConfigured,
    DraftSaved(DraftTarget),
    LetterSent(SentTarget),
    LetterEvent(EventTarget),
    WorkspacePublished(SpaceTarget),
}

macro_rules! target {
    ($name:ident { $($field:ident),+ }) => {
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $(pub $field: String),+ }
    };
}

target!(DraftTarget { letter_id, copy_id });
target!(SentTarget {
    letter_id,
    event_id
});
target!(EventTarget {
    letter_id,
    event_id,
    event_kind
});
target!(SpaceTarget { space_id });

/// Decoded metadata. The Git adapter verifies it against the actual tree and parents.
#[derive(Debug)]
pub struct TeamCommit {
    pub team_id: String,
    pub operation_id: String,
    pub actor_id: String,
    pub occurred_at: Datetime,
    pub operation: Operation,
}

/// Reasons a message cannot describe a recognized team operation.
#[derive(Debug)]
pub enum DecodeError {
    LineEnding,
    Toml(toml::de::Error),
    Version(u32),
    Timestamp,
    Target,
    UnknownKind(String),
}

/// Read the complete UTF-8 message after the Git object headers.
///
/// ```
/// use synod::adapters::git::commit_message::{decode, DecodeError};
/// assert!(matches!(decode("feat: add a screen"), Err(DecodeError::LineEnding)));
/// ```
pub fn decode(message: &str) -> Result<TeamCommit, DecodeError> {
    if !message.ends_with('\n') || message.contains('\r') {
        return Err(DecodeError::LineEnding);
    }
    let data: Metadata = toml::from_str(message).map_err(DecodeError::Toml)?;
    if data.schema_version != 1 {
        return Err(DecodeError::Version(data.schema_version));
    }
    if data.occurred_at.offset.is_none() {
        return Err(DecodeError::Timestamp);
    }
    let operation = match (data.kind.as_str(), data.target) {
        ("team.configured", None) => Operation::TeamConfigured,
        ("draft.saved", Some(t)) => Operation::DraftSaved(t.try_into().map_err(DecodeError::Toml)?),
        ("letter.sent", Some(t)) => Operation::LetterSent(t.try_into().map_err(DecodeError::Toml)?),
        ("letter.event", Some(t)) => {
            Operation::LetterEvent(t.try_into().map_err(DecodeError::Toml)?)
        }
        ("workspace.published", Some(t)) => {
            Operation::WorkspacePublished(t.try_into().map_err(DecodeError::Toml)?)
        }
        (
            "team.configured"
            | "draft.saved"
            | "letter.sent"
            | "letter.event"
            | "workspace.published",
            _,
        ) => return Err(DecodeError::Target),
        _ => return Err(DecodeError::UnknownKind(data.kind)),
    };
    Ok(TeamCommit {
        team_id: data.team_id,
        actor_id: data.actor_id,
        operation_id: data.operation_id,
        occurred_at: data.occurred_at,
        operation,
    })
}
