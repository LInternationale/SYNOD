//! Versioned local requests for the live terminal.
use serde::{Deserialize, Serialize};
pub const VERSION: u8 = 1;
#[derive(Serialize, Deserialize)]
pub enum Request {
    Hello(u8, String),
    Theme,
    ApplyTheme([u8; 3]),
    Profile,
    Join(String),
    Create {
        owner: String,
        name: String,
        public: bool,
        manifest: crate::application::setup::NewTeam,
    },
    Identities {
        human: String,
        assistant: String,
    },
    Drafts,
    SaveDraft(Draft),
    Workspace,
    Close,
}
#[derive(Serialize, Deserialize)]
pub enum Reply {
    Ready,
    Team(Option<Profile>),
    Drafts(Vec<Draft>),
    Saved(Draft),
    Files(Vec<String>),
    Color([u8; 3]),
    Failure(String),
}

#[derive(Serialize, Deserialize)]
pub struct Profile {
    pub team: crate::application::setup::NewTeam,
    pub human: String,
    pub assistant: String,
    pub remote: String,
}
#[derive(Default, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    pub reference: String,
    pub title: String,
    pub to: String,
    pub cc: String,
    pub body: String,
}
