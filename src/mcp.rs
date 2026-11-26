//! Standard-input/output bridge for the assistant's shared session.
use crate::{
    application::letters::{Draft, TaskChange},
    session::{
        bootstrap,
        protocol::{Entry, Reply, Request},
        transport::Client,
    },
};
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    schemars, tool, tool_handler, tool_router,
};
use std::{
    io,
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
struct Assistant {
    client: Arc<Mutex<Client>>,
    tool_router: ToolRouter<Self>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct LetterId {
    letter: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Filter {
    All,
    ToMe,
    CcMe,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct Selection {
    filter: Option<Filter>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct Completion {
    letter: String,
    completed: bool,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct DraftId {
    reference: String,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct Save {
    draft: Draft,
}

impl Assistant {
    async fn request(&self, request: Request) -> Result<Reply, String> {
        let client = self.client.clone();
        tokio::task::spawn_blocking(move || {
            client
                .lock()
                .map_err(|_| io::Error::other("Session lock poisoned"))?
                .request(request)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
    }
}

fn json(value: impl serde::Serialize) -> Result<String, String> {
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

#[tool_router]
impl Assistant {
    #[tool(description = "List published letters. Filter by all, to_me, or cc_me.")]
    async fn list_letters(&self, Parameters(s): Parameters<Selection>) -> Result<String, String> {
        let Reply::Team(Some(p)) = self.request(Request::Profile).await? else {
            return Err("Team unavailable".into());
        };
        let Reply::Published(mut letters) = self.request(Request::Published).await? else {
            return Err("Board unavailable".into());
        };
        letters.retain(|l| match s.filter {
            None | Some(Filter::All) => true,
            Some(Filter::ToMe) => l.to == p.assistant,
            Some(Filter::CcMe) => l.cc.contains(&p.assistant),
        });
        json(letters)
    }
    #[tool(description = "Read a published letter and acknowledge its unread marker.")]
    async fn read_letter(&self, Parameters(id): Parameters<LetterId>) -> Result<String, String> {
        match self.request(Request::Read(id.letter)).await? {
            Reply::Letter(l) => json(l),
            _ => Err("Letter unavailable".into()),
        }
    }
    #[tool(description = "List subtasks of a published letter.")]
    async fn list_dependencies(
        &self,
        Parameters(id): Parameters<LetterId>,
    ) -> Result<String, String> {
        let Reply::Published(letters) = self.request(Request::Published).await? else {
            return Err("Board unavailable".into());
        };
        let letter = letters
            .iter()
            .find(|l| l.id == id.letter)
            .ok_or("Unknown letter")?;
        json(&letter.dependencies)
    }
    #[tool(description = "List the active assistant's editable drafts.")]
    async fn list_drafts(&self) -> Result<String, String> {
        match self.request(Request::Drafts).await? {
            Reply::Drafts(d) => json(d),
            _ => Err("Drafts unavailable".into()),
        }
    }
    #[tool(
        description = "Save a complete draft. Use empty reference for a new draft. Preserve the returned reference and cloud revision when editing."
    )]
    async fn save_draft(&self, Parameters(s): Parameters<Save>) -> Result<String, String> {
        match self.request(Request::SaveDraft(s.draft)).await? {
            Reply::Saved(d, warning) => json((d, warning)),
            _ => Err("Missing saved draft".into()),
        }
    }
    #[tool(description = "Publish a saved draft by its local reference.")]
    async fn send_letter(&self, Parameters(id): Parameters<DraftId>) -> Result<String, String> {
        match self.request(Request::Send(id.reference)).await? {
            Reply::Ready => Ok("Published".into()),
            _ => Err("Missing send acknowledgement".into()),
        }
    }
    #[tool(description = "Set the manual completion state of a published letter.")]
    async fn mark_completed(
        &self,
        Parameters(c): Parameters<Completion>,
    ) -> Result<String, String> {
        match self
            .request(Request::Task {
                letter: c.letter,
                change: TaskChange::Completion(c.completed),
            })
            .await?
        {
            Reply::Ready => Ok("Saved".into()),
            _ => Err("Missing task acknowledgement".into()),
        }
    }
}
#[tool_handler(router = self.tool_router)]
impl ServerHandler for Assistant {}

pub fn run(file: &Path) -> io::Result<()> {
    let roots = bootstrap::configured(file)?
        .ok_or_else(|| io::Error::other("Finish initial setup in the terminal first"))?;
    let client = Client::connect_as(&roots.state, Entry::Assistant)?;
    let server = Assistant {
        client: Arc::new(Mutex::new(client)),
        tool_router: Assistant::tool_router(),
    };
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            server
                .serve(rmcp::transport::stdio())
                .await
                .map_err(io::Error::other)?
                .waiting()
                .await
                .map_err(io::Error::other)?;
            Ok(())
        })
}
