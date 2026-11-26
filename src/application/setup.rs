//! Data frozen by the new-team wizard before publishing team.json.

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum MemberKind {
    Human,
    Assistant,
}

#[derive(Debug, Deserialize, Serialize)]
struct Member {
    actor_id: String,
    kind: MemberKind,
    display_name: String,
}
#[derive(Debug, Deserialize, Serialize)]
struct Space {
    space_id: String,
    display_name: String,
}

/// Initial team configuration, in the repository's version-one wire format.
#[derive(Debug, Deserialize, Serialize)]
pub struct NewTeam {
    schema_version: u8,
    team_id: String,
    members: Vec<Member>,
    spaces: Vec<Space>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum SetupError {
    InvalidTeamId,
    EmptyHumanName(usize),
    EmptyAssistantName(usize),
    EmptySpaceName(usize),
}

impl NewTeam {
    /// Prepare a manifest from the creator's six member names and workspace names.
    pub fn from_wizard(
        team_id: &str,
        humans: [String; 3],
        assistants: [String; 3],
        spaces: Vec<String>,
    ) -> Result<Self, SetupError> {
        if team_id.is_empty()
            || !team_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err(SetupError::InvalidTeamId);
        }
        let mut members = Vec::with_capacity(6);
        for (i, display_name) in humans.into_iter().enumerate() {
            if display_name.trim().is_empty() {
                return Err(SetupError::EmptyHumanName(i));
            }
            members.push(Member {
                actor_id: format!("member-{}", i + 1),
                kind: MemberKind::Human,
                display_name,
            });
        }
        for (i, display_name) in assistants.into_iter().enumerate() {
            if display_name.trim().is_empty() {
                return Err(SetupError::EmptyAssistantName(i));
            }
            members.push(Member {
                actor_id: format!("assistant-{}", i + 1),
                kind: MemberKind::Assistant,
                display_name,
            });
        }
        let spaces = spaces
            .into_iter()
            .enumerate()
            .map(|(i, display_name)| {
                if display_name.trim().is_empty() {
                    return Err(SetupError::EmptySpaceName(i));
                }
                Ok(Space {
                    space_id: format!("space-{}", i + 1),
                    display_name,
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            schema_version: 1,
            team_id: team_id.into(),
            members,
            spaces,
        })
    }
}

/// A local-only initial manifest waiting for an identity and remote confirmation.
#[derive(Debug, Deserialize, Serialize)]
pub struct PendingTeam {
    manifest: NewTeam,
}

pub struct InitialPublication<'a> {
    pub manifest: &'a NewTeam,
    pub actor_id: &'a str,
}

impl PendingTeam {
    pub fn new(manifest: NewTeam) -> Self {
        Self { manifest }
    }

    /// Prepare the initial publication only for a bound member of this team.
    pub fn publication_for(&self, bound_actor: Option<&str>) -> Option<InitialPublication<'_>> {
        let actor = bound_actor?;
        let member = self.manifest.members.iter().find(|m| m.actor_id == actor)?;
        Some(InitialPublication {
            manifest: &self.manifest,
            actor_id: &member.actor_id,
        })
    }
}

impl NewTeam {
    pub fn id(&self) -> &str {
        &self.team_id
    }
    pub fn human(&self, id: &str) -> bool {
        self.members
            .iter()
            .any(|m| matches!(m.kind, MemberKind::Human) && m.actor_id == id)
    }
    pub fn assistant(&self, id: &str) -> bool {
        self.members
            .iter()
            .any(|m| matches!(m.kind, MemberKind::Assistant) && m.actor_id == id)
    }
    pub fn spaces(&self) -> Vec<(&str, &str)> {
        self.spaces
            .iter()
            .map(|s| (s.space_id.as_str(), s.display_name.as_str()))
            .collect()
    }
    pub fn roster(&self) -> Vec<(&str, &str)> {
        self.members
            .iter()
            .map(|m| (m.actor_id.as_str(), m.display_name.as_str()))
            .collect()
    }
    pub fn validate(&self) -> std::io::Result<()> {
        fn safe(id: &str) -> bool {
            !id.is_empty()
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        }
        let mut actors = std::collections::HashSet::new();
        let mut spaces = std::collections::HashSet::new();
        if self.schema_version != 1
            || !safe(&self.team_id)
            || !self
                .members
                .iter()
                .all(|m| safe(&m.actor_id) && actors.insert(&m.actor_id))
            || !self
                .spaces
                .iter()
                .all(|s| safe(&s.space_id) && spaces.insert(&s.space_id))
        {
            return Err(std::io::Error::other("Invalid team manifest"));
        }
        Ok(())
    }
}
