//! Versioned local requests for the live terminal.
use serde::{Deserialize, Serialize};
pub const VERSION: u8 = 3;
#[derive(Serialize, Deserialize)]
pub enum Request {
    Hello(u8, String),
    Theme,
    ApplyTheme([u8; 3]),
    Profile,
    SetupPending,
    Register {
        human: String,
        assistant: String,
        initial: bool,
    },
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
    Send(String),
    Published,
    Drafts,
    SaveDraft(Draft),
    WorkspaceAction {
        space: String,
        action: WorkAction,
    },
    Workspace,
    Close,
}
#[derive(Serialize, Deserialize)]
pub enum Reply {
    Published(Vec<PublishedLetter>),
    Work(WorkResult),
    Pending(bool),
    Ready,
    Team(Option<Profile>),
    Drafts(Vec<Draft>),
    Saved(Draft, Option<String>),
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
pub use crate::application::letters::{Attachment, Draft, PublishedLetter};

#[derive(Clone, Serialize, Deserialize)]
pub enum WorkAction {
    Save,
    Submit,
    SaveAndSubmit,
    Defer,
    Resolve,
    ResolveOnTerminal(String),
}
#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum WorkResult {
    Saved,
    Published,
    Dirty,
    Conflict,
    Deferred,
}
