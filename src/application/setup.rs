//! Data frozen by the new-team wizard before publishing team.json.

use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum MemberKind {
    Human,
    Assistant,
}

#[derive(Serialize)]
struct Member {
    actor_id: String,
    kind: MemberKind,
    display_name: String,
}
#[derive(Serialize)]
struct Space {
    space_id: String,
    display_name: String,
}

/// Initial team configuration, in the repository's version-one wire format.
#[derive(Serialize)]
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
