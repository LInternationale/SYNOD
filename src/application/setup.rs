//! Data frozen by the new-team wizard before publishing team.json.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum MemberKind {
    Human,
    Assistant,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Member {
    actor_id: String,
    kind: MemberKind,
    display_name: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Space {
    space_id: String,
    display_name: String,
}

/// Initial team configuration, in the repository's version-one wire format.
#[derive(Clone, Debug, Deserialize, Serialize)]
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
    /// Prepare a manifest from the creator's member names and workspace names.
    pub fn from_wizard<const H: usize, const A: usize>(
        team_id: &str,
        humans: [String; H],
        assistants: [String; A],
        spaces: Vec<String>,
    ) -> Result<Self, SetupError> {
        Self::from_names(team_id, humans.to_vec(), assistants.to_vec(), spaces)
    }

    pub fn empty(team_id: &str, spaces: Vec<String>) -> Result<Self, SetupError> {
        Self::from_names(team_id, vec![], vec![], spaces)
    }

    fn from_names(
        team_id: &str,
        humans: Vec<String>,
        assistants: Vec<String>,
        spaces: Vec<String>,
    ) -> Result<Self, SetupError> {
        if team_id.is_empty()
            || !team_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        {
            return Err(SetupError::InvalidTeamId);
        }
        let mut members = Vec::with_capacity(humans.len() + assistants.len());
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
#[derive(Clone, Debug, Deserialize, Serialize)]
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

impl NewTeam {
    /// Append each supplied name once for a stable registration operation.
    pub fn register(
        &mut self,
        key: &str,
        human: &str,
        assistant: &str,
    ) -> std::io::Result<(String, String)> {
        if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(std::io::Error::other("Invalid registration identifier"));
        }
        let mut ids = [String::new(), String::new()];
        for (index, (prefix, kind, name)) in [
            ("member", MemberKind::Human, human),
            ("assistant", MemberKind::Assistant, assistant),
        ]
        .into_iter()
        .enumerate()
        {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            let id = format!("{prefix}-{key}");
            if let Some(existing) = self.members.iter().find(|m| m.actor_id == id) {
                if existing.display_name != name {
                    return Err(std::io::Error::other("Registration identifier reused"));
                }
            } else {
                self.members.push(Member {
                    actor_id: id.clone(),
                    kind,
                    display_name: name.into(),
                })
            }
            ids[index] = id;
        }
        let [h, a] = ids;
        Ok((h, a))
    }
}

/// Locally bound identities. Roster membership alone does not activate a slot.
pub enum Activation<'a> {
    Inactive,
    Human(&'a str),
    Pair(&'a str, &'a str),
}
impl<'a> Activation<'a> {
    pub fn parse(human: &'a str, assistant: &'a str) -> std::io::Result<Self> {
        match (human.trim(), assistant.trim()) {
            ("", "") => Ok(Self::Inactive),
            ("", _) => Err(std::io::Error::other("请先激活人类身份，再激活 AI")),
            (h, "") => Ok(Self::Human(h)),
            (h, a) => Ok(Self::Pair(h, a)),
        }
    }
    pub fn slots(self) -> (&'a str, &'a str) {
        match self {
            Self::Inactive => ("", ""),
            Self::Human(h) => (h, ""),
            Self::Pair(h, a) => (h, a),
        }
    }
}
