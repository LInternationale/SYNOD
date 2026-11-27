//! Versioned local requests for the live terminal.
use serde::{Deserialize, Serialize};
pub const VERSION: u8 = 8;
#[derive(Serialize, Deserialize)]
pub enum Request {
    Hello(u8, String, Entry),
    SyncStatus,
    PublicationStatus,
    EnqueueSend(Draft),
    DeliveryQueue,
    QueuePreferences,
    SetRemotePreferences(std::num::NonZeroU32, usize),
    Background(BackgroundWrite),
    Interval,
    SetInterval(std::num::NonZeroU32),
    SpaceSync {
        space: String,
        action: SyncAction,
    },
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
    SendCurrent(Draft),
    Task {
        letter: String,
        change: crate::application::letters::TaskChange,
    },
    Read(String),
    Published,
    Board,
    OpenDraft(String),
    Drafts,
    SaveDraft(Draft),
    WorkspaceAction {
        space: String,
        action: WorkAction,
    },
    Tags,
    ToggleTag(String),
    Workspace,
    Close,
}
#[derive(Serialize, Deserialize)]
pub enum Reply {
    SyncStatus(Option<String>),
    PublicationStatus(PublicationState),
    DeliveryQueue(Vec<DeliveryRow>, Vec<String>, Vec<RemoteActivity>),
    Queued,
    QueuePreferences(usize),
    Committed,
    DraftCommitted(Draft),
    Interval(std::num::NonZeroU32),
    SpaceSync(crate::adapters::git::workspace_sync::Preview),
    Letter(PublishedLetter),
    Published(Vec<PublishedLetter>),
    Board {
        actor: String,
        letters: Vec<LetterRow>,
        drafts: Vec<DraftRow>,
    },
    Draft(Draft),
    Work(WorkResult),
    Pending(bool),
    Ready,
    Team(Option<Profile>),
    Drafts(Vec<Draft>),
    Saved(Draft, Option<String>),
    Tags(crate::adapters::local::tags::Tags),
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

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum Entry {
    Human,
    Assistant,
}
impl Entry {
    pub fn actor(self, profile: &Profile) -> &str {
        match self {
            Self::Human => &profile.human,
            Self::Assistant => &profile.assistant,
        }
    }
    pub fn bound(self, profile: &Profile) -> bool {
        match self {
            Self::Human => profile.team.human(&profile.human),
            Self::Assistant => {
                profile.team.human(&profile.human) && profile.team.assistant(&profile.assistant)
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum SyncAction {
    Status,
    RefreshPreview,
    SetMode(crate::adapters::git::workspace_sync::Mode),
    Apply(crate::adapters::git::workspace_sync::Preview),
}

#[derive(Serialize, Deserialize)]
pub struct DraftRow {
    pub reference: String,
    pub title: String,
}
#[derive(Serialize, Deserialize)]
pub struct LetterRow {
    pub id: String,
    pub title: String,
    pub author: String,
    pub to: String,
    pub cc: Vec<String>,
    pub unread: bool,
}

#[derive(Serialize, Deserialize)]
pub enum BackgroundWrite {
    Draft(Draft),
    Send(Draft),
    Task {
        letter: String,
        change: crate::application::letters::TaskChange,
    },
    Workspace(String),
}
impl BackgroundWrite {
    pub(crate) fn request(self) -> Request {
        match self {
            Self::Draft(d) => Request::SaveDraft(d),
            Self::Send(draft) => Request::SendCurrent(draft),
            Self::Task { letter, change } => Request::Task { letter, change },
            Self::Workspace(space) => Request::WorkspaceAction {
                space,
                action: WorkAction::Save,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PublicationState {
    Idle,
    Pushing,
    Confirmed,
    Failed(String),
}

pub use crate::application::letters::DeliveryStep;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum DeliveryState {
    Waiting,
    Running(DeliveryStep),
    Failed(DeliveryStep, String),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeliveryRow {
    pub key: String,
    pub actor: String,
    pub reference: String,
    pub title: String,
    pub letter: String,
    pub state: DeliveryState,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RemoteActivity {
    pub label: String,
    pub state: ActivityState,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum ActivityState {
    Waiting,
    Running,
    Failed(String),
}
