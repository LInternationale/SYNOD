//! Unix session ownership and JSON line transport.
use super::protocol::ActivityState;
use super::{
    bootstrap,
    protocol::{Entry, PublicationState, Reply, Request, VERSION},
};
use crate::adapters::local::state::ClientConfig;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    os::unix::{
        fs::DirBuilderExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

fn write<T: Serialize>(stream: &mut UnixStream, value: &T) -> io::Result<()> {
    let mut output = std::io::BufWriter::new(stream);
    serde_json::to_writer(&mut output, value)?;
    output.write_all(b"\n")?;
    output.flush()
}
fn read<T: DeserializeOwned>(reader: &mut BufReader<UnixStream>) -> io::Result<T> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
    }
    Ok(serde_json::from_str(&line)?)
}

#[derive(Deserialize, Serialize)]
struct Endpoint {
    version: u8,
    generation: String,
    address: PathBuf,
}

fn discover(root: &Path) -> io::Result<(UnixStream, String)> {
    let endpoint: Endpoint =
        serde_json::from_reader(fs::File::open(root.join("runtime/endpoint.json"))?)?;
    if endpoint.version != VERSION {
        return Err(io::Error::other("Incompatible session version"));
    }
    Ok((UnixStream::connect(endpoint.address)?, endpoint.generation))
}

pub struct Client(BufReader<UnixStream>);
impl Client {
    pub fn connect(root: &Path) -> io::Result<Self> {
        Self::connect_as(root, Entry::Human)
    }
    pub fn connect_as(root: &Path, entry: Entry) -> io::Result<Self> {
        let connect = || discover(root);
        let (stream, generation) = match connect() {
            Ok(s) => s,
            Err(_) => {
                let mut child = Command::new(std::env::current_exe()?)
                    .arg("--session-host")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?;
                let until = Instant::now() + Duration::from_secs(3);
                loop {
                    match connect() {
                        Ok(s) => {
                            thread::spawn(move || child.wait());
                            break s;
                        }
                        Err(e) if Instant::now() >= until => {
                            thread::spawn(move || child.wait());
                            return Err(e);
                        }
                        Err(_) => thread::sleep(Duration::from_millis(20)),
                    }
                }
            }
        };
        let mut client = Self(BufReader::new(stream));
        match client.request(Request::Hello(VERSION, generation, entry))? {
            Reply::Ready => Ok(client),
            _ => Err(io::Error::other("Incompatible session")),
        }
    }
    pub fn request(&mut self, request: Request) -> io::Result<Reply> {
        write(self.0.get_mut(), &request)?;
        match read(&mut self.0)? {
            Reply::Failure(message) => Err(io::Error::other(message)),
            reply => Ok(reply),
        }
    }
}

#[derive(Default)]
struct RemoteTasks {
    next: u64,
    items: std::collections::BTreeMap<u64, super::protocol::RemoteActivity>,
}
impl RemoteTasks {
    fn add(&mut self, label: String) -> u64 {
        self.items.retain(|_, item| {
            item.label != label || !matches!(item.state, ActivityState::Failed(_))
        });
        self.next += 1;
        self.items.insert(
            self.next,
            super::protocol::RemoteActivity {
                label,
                state: ActivityState::Waiting,
            },
        );
        self.next
    }
}
struct TrackedTask {
    tasks: Arc<Mutex<RemoteTasks>>,
    id: u64,
}
impl TrackedTask {
    fn new(tasks: &Arc<Mutex<RemoteTasks>>, label: String) -> io::Result<Self> {
        let id = tasks
            .lock()
            .map_err(|_| io::Error::other("Queue unavailable"))?
            .add(label);
        Ok(Self {
            tasks: tasks.clone(),
            id,
        })
    }
    fn start(&self) {
        if let Ok(mut tasks) = self.tasks.lock()
            && let Some(item) = tasks.items.get_mut(&self.id)
        {
            item.state = ActivityState::Running;
        }
    }
    fn fail(&self, reason: String) {
        if let Ok(mut tasks) = self.tasks.lock()
            && let Some(task) = tasks.items.get_mut(&self.id)
        {
            task.state = ActivityState::Failed(reason);
        }
    }
    fn outcome(&self, file: &Path, result: &io::Result<Reply>) -> io::Result<()> {
        match result {
            Err(e) => self.fail(e.to_string()),
            Ok(Reply::Saved(_, Some(reason))) => self.fail(reason.clone()),
            Ok(_) => self.finish(file)?,
        }
        Ok(())
    }
    fn finish(&self, file: &Path) -> io::Result<()> {
        let label = self
            .tasks
            .lock()
            .map_err(|_| io::Error::other("Queue unavailable"))?
            .items
            .get(&self.id)
            .map(|item| item.label.clone());
        if let Some(label) = label {
            super::host::delivery::remember(file, &[label])?;
        }
        Ok(())
    }
}
impl Drop for TrackedTask {
    fn drop(&mut self) {
        if let Ok(mut tasks) = self.tasks.lock()
            && !tasks
                .items
                .get(&self.id)
                .is_some_and(|a| matches!(a.state, ActivityState::Failed(_)))
        {
            tasks.items.remove(&self.id);
        }
    }
}
fn remote_label(request: &Request) -> Option<String> {
    Some(match request {
        Request::SaveDraft(d) => format!("保存草稿 · {}", d.title),
        Request::SendCurrent(d) => format!("寄信 · {}", d.title),
        Request::Send(_) => "寄信".into(),
        Request::Task { .. } => "更新信件任务".into(),
        Request::WorkspaceAction { space, .. } => format!("同步工作 · {space}"),
        Request::SpaceSync { space, .. } => format!("更新工作区 · {space}"),
        Request::Register { .. } | Request::Identities { .. } => "更新队伍成员".into(),
        Request::Create { .. } => "创建队伍".into(),
        Request::Join(_) => "加入队伍".into(),
        Request::Published | Request::Drafts => "获取队伍更新".into(),
        _ => return None,
    })
}

fn connection(
    stream: UnixStream,
    file: Arc<PathBuf>,
    writer: Arc<Mutex<()>>,
    generation: String,
    wake: mpsc::Sender<()>,
    sync_error: Arc<Mutex<Option<String>>>,
    publication: Arc<Mutex<PublicationState>>,
    tasks: Arc<Mutex<RemoteTasks>>,
) -> io::Result<()> {
    let mut reader = BufReader::new(stream);
    let entry = match read(&mut reader)? {
        Request::Hello(VERSION, g, entry) if g == generation => entry,
        _ => {
            return write(
                reader.get_mut(),
                &Reply::Failure("Incompatible session".into()),
            );
        }
    };
    write(reader.get_mut(), &Reply::Ready)?;
    thread::scope(|scope| {
        loop {
            let request = read(&mut reader)?;
            let close = matches!(request, Request::Close);
            let refresh_team = matches!(
                request,
                Request::Join(_)
                    | Request::Create { .. }
                    | Request::Register { .. }
                    | Request::Identities { .. }
            );
            let result = (|| -> io::Result<Reply> {
                match request {
                    Request::EnqueueSend(draft) => {
                        super::host::delivery::enqueue(&file, entry, draft)?;
                        let _ = wake.send(());
                        return Ok(Reply::Queued);
                    }
                    Request::ReadQueued(id) => {
                        return super::host::delivery::read_queued(&file, entry, &id);
                    }
                    Request::DeliveryQueue => {
                        let (rows, recent) = super::host::delivery::snapshot(&file)?;
                        return Ok(Reply::DeliveryQueue(
                            rows,
                            recent,
                            tasks
                                .lock()
                                .map_err(|_| io::Error::other("Queue unavailable"))?
                                .items
                                .values()
                                .cloned()
                                .collect(),
                        ));
                    }
                    Request::QueuePreferences => {
                        return Ok(Reply::QueuePreferences(
                            ClientConfig::read(&file)?.completed_rows,
                        ));
                    }
                    Request::SetRemotePreferences(interval, limit) => {
                        ClientConfig::update(&file, |c| {
                            c.sync_interval_seconds = interval;
                            c.completed_rows = limit;
                        })?;
                        super::host::delivery::remember(&file, &[])?;
                        let _ = wake.send(());
                        return Ok(Reply::Ready);
                    }
                    _ => {}
                }
                if matches!(request, Request::PublicationStatus) {
                    let state = publication
                        .lock()
                        .map_err(|_| io::Error::other("Publication status unavailable"))?
                        .clone();
                    if matches!(state, PublicationState::Idle | PublicationState::Confirmed)
                        && let Some(error) = sync_error
                            .lock()
                            .map_err(|_| io::Error::other("Sync status unavailable"))?
                            .clone()
                    {
                        return Ok(Reply::PublicationStatus(PublicationState::Failed(error)));
                    }
                    return Ok(Reply::PublicationStatus(state));
                }
                if let Request::Background(job) = request {
                    let (tx, rx) = mpsc::channel();
                    let lock = writer.clone();
                    let file = file.clone();
                    let status = publication.clone();
                    let wake = wake.clone();
                    let request = job.request();
                    let task =
                        TrackedTask::new(&tasks, remote_label(&request).unwrap_or_default())?;
                    scope.spawn(move || {
                        let result = (|| {
                            let _guard = lock
                                .lock()
                                .map_err(|_| io::Error::other("Writer unavailable"))?;
                            task.start();
                            let mut receipt = Some(tx.clone());
                            let result = super::host::perform_observed(
                                &file,
                                request,
                                entry,
                                &mut |reply| {
                                    if let Some(channel) = receipt.take() {
                                        if let Ok(mut state) = status.lock() {
                                            *state = PublicationState::Pushing;
                                        }
                                        let _ = channel.send(Ok(reply));
                                    }
                                },
                            );
                            task.outcome(&file, &result)?;
                            if let Ok(mut state) = status.lock() {
                                *state = match &result {
                                    Err(e) => PublicationState::Failed(e.to_string()),
                                    Ok(Reply::Saved(_, Some(e))) => {
                                        PublicationState::Failed(e.clone())
                                    }
                                    Ok(_) => PublicationState::Confirmed,
                                };
                            }
                            result
                        })();
                        let _ = tx.send(result);
                        let _ = wake.send(());
                    });
                    return rx
                        .recv()
                        .map_err(|_| io::Error::other("Publication worker stopped"))?;
                }
                if matches!(request, Request::SyncStatus) {
                    return Ok(Reply::SyncStatus(
                        sync_error
                            .lock()
                            .map_err(|_| io::Error::other("Sync status unavailable"))?
                            .clone(),
                    ));
                }
                if let Some(reply) = super::host::cached_query(&file, &request, entry)? {
                    return Ok(reply);
                }
                match &request {
                    Request::ApplyTheme(rgb) => {
                        ClientConfig::update(&file, |c| {
                            c.theme_rgb = *rgb;
                            c.fields.insert("custom_theme".into(), true.into());
                        })?;
                        return Ok(Reply::Color(*rgb));
                    }
                    Request::SetInterval(value) => {
                        ClientConfig::update(&file, |c| c.sync_interval_seconds = *value)?;
                        let _ = wake.send(());
                        return Ok(Reply::Ready);
                    }
                    Request::Close => return Ok(Reply::Ready),
                    _ => {}
                }
                let task = remote_label(&request)
                    .map(|label| TrackedTask::new(&tasks, label))
                    .transpose()?;
                let _guard = writer
                    .lock()
                    .map_err(|_| io::Error::other("Settings lock unavailable"))?;
                if let Some(task) = &task {
                    task.start();
                }
                let reply = match request {
                    Request::ApplyTheme(_) | Request::SetInterval(_) => unreachable!(),
                    Request::QueuePreferences
                    | Request::SetRemotePreferences(..)
                    | Request::ReadQueued(_)
                    | Request::DeliveryQueue
                    | Request::EnqueueSend(_)
                    | Request::SyncStatus
                    | Request::Background(_)
                    | Request::PublicationStatus => {
                        unreachable!()
                    }
                    Request::Interval => Ok(Reply::Interval(
                        ClientConfig::read(&file)?.sync_interval_seconds,
                    )),
                    Request::Theme => Ok(Reply::Color(ClientConfig::read(&file)?.theme_rgb)),
                    Request::Close => Ok(Reply::Ready),
                    other @ (Request::Profile
                    | Request::Register { .. }
                    | Request::SetupPending
                    | Request::Join(_)
                    | Request::Create { .. }
                    | Request::Identities { .. }
                    | Request::Send(_)
                    | Request::SendCurrent(_)
                    | Request::Task { .. }
                    | Request::Read(_)
                    | Request::Published
                    | Request::Board
                    | Request::OpenDraft(_)
                    | Request::Drafts
                    | Request::SaveDraft(_)
                    | Request::Tags
                    | Request::ToggleTag(_)
                    | Request::SpaceSync { .. }
                    | Request::WorkspaceAction { .. }
                    | Request::Workspace) => super::host::perform_as(&file, other, entry),
                    Request::Hello(..) => Err(io::Error::other("Session already connected")),
                };
                if let Some(task) = task {
                    task.outcome(&file, &reply)?;
                }
                reply
            })();
            if refresh_team && result.is_ok() {
                let _ = wake.send(());
            }
            let reply = result.unwrap_or_else(|e| Reply::Failure(e.to_string()));
            write(reader.get_mut(), &reply)?;
            if close {
                return Ok(());
            }
        }
    })
}

pub fn host(file: PathBuf) -> io::Result<()> {
    let roots = bootstrap::configured(&file)?.ok_or_else(|| io::Error::other("Setup required"))?;
    let runtime = roots.state.join("runtime");
    fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(&runtime)?;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let meta = fs::symlink_metadata(&runtime)?;
    if !meta.is_dir()
        || meta.permissions().mode() & 0o077 != 0
        || meta.uid() != fs::metadata(&file)?.uid()
    {
        return Err(io::Error::other("Runtime directory must be private"));
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(runtime.join("owner.lock"))?;
    if let Err(e) = lock.try_lock() {
        return match e {
            std::fs::TryLockError::WouldBlock => Ok(()),
            std::fs::TryLockError::Error(e) => Err(e),
        };
    }
    let mut serial = 0;
    let (private_dir, generation) = loop {
        let generation = format!(
            "{}-{}-{serial}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos()
        );
        let dir = Path::new("/tmp").join(format!("synod-{generation}"));
        match fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => break (dir, generation),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => serial += 1,
            Err(e) => return Err(e),
        }
    };
    let path = private_dir.join("ipc");
    let listener = UnixListener::bind(&path)?;
    let endpoint = Endpoint {
        version: VERSION,
        generation: generation.clone(),
        address: path.clone(),
    };
    let tmp = runtime.join("endpoint.tmp");
    fs::write(&tmp, serde_json::to_vec(&endpoint)?)?;
    fs::rename(tmp, runtime.join("endpoint.json"))?;
    listener.set_nonblocking(true)?;
    let file = Arc::new(file);
    let writer = Arc::new(Mutex::new(()));
    let (stop, stop_rx) = mpsc::channel::<()>();
    let worker_file = file.clone();
    let worker_lock = writer.clone();
    let sync_error = Arc::new(Mutex::new(None));
    let worker_error = sync_error.clone();
    let publication = Arc::new(Mutex::new(PublicationState::Idle));
    let recovery_status = publication.clone();
    let tasks = Arc::new(Mutex::new(RemoteTasks::default()));
    let worker_tasks = tasks.clone();
    let sync_worker = thread::spawn(move || {
        loop {
            if let Ok(_guard) = worker_lock.lock() {
                let task = TrackedTask::new(&worker_tasks, "检查远端更新".into()).ok();
                if let Some(t) = &task {
                    t.start();
                }
                let error = super::host::synchronize(&worker_file)
                    .err()
                    .map(|e| e.to_string());
                if let Some(t) = task {
                    if let Some(reason) = &error {
                        t.fail(reason.clone());
                    } else {
                        let _ = t.finish(&worker_file);
                    }
                }
                if error.is_none()
                    && let Ok(mut state) = recovery_status.lock()
                    && matches!(*state, PublicationState::Failed(_))
                {
                    *state = PublicationState::Idle;
                }
                if let Ok(mut last) = worker_error.lock() {
                    *last = error;
                }
            }
            let wait = ClientConfig::read(&worker_file)
                .map(|c| Duration::from_secs(c.sync_interval_seconds.get().into()))
                .unwrap_or(Duration::from_secs(30));
            match stop_rx.recv_timeout(wait) {
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(()) => while stop_rx.try_recv().is_ok() {},
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    });
    let (done, rx) = mpsc::channel();
    let mut workers = Vec::new();
    let start = Instant::now();
    let mut connected = false;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                connected = true;
                let file = file.clone();
                let done = done.clone();
                let writer = writer.clone();
                let wake = stop.clone();
                let error = sync_error.clone();
                let generation = generation.clone();
                let publication = publication.clone();
                let tasks = tasks.clone();
                workers.push(thread::spawn(move || {
                    let _ = connection(
                        stream,
                        file,
                        writer,
                        generation,
                        wake,
                        error,
                        publication,
                        tasks,
                    );
                    let _ = done.send(());
                }));
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                workers.retain(|w| !w.is_finished());
                if workers.is_empty() && (connected || start.elapsed() > Duration::from_secs(5)) {
                    break;
                }
                let _ = rx.recv_timeout(Duration::from_millis(20));
            }
            Err(e) => return Err(e),
        }
    }
    drop(stop);
    sync_worker
        .join()
        .map_err(|_| io::Error::other("Sync worker stopped unexpectedly"))?;
    drop(listener);
    fs::remove_file(runtime.join("endpoint.json"))?;
    fs::remove_file(path)?;
    fs::remove_dir(private_dir)
}

#[cfg(test)]
mod navigation_tests {
    use super::*;
    use crate::session::protocol::Profile;

    #[test]
    fn cold_navigation_and_preferences_complete_while_sync_writer_is_busy() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("synod-nav-{}", std::process::id()));
        fs::create_dir(&root)?;
        let file = root.join("client.json");
        let roots = bootstrap::initialize(&file, &root.join("state"), &root.join("work"))?;
        let team: crate::application::setup::NewTeam =
            serde_json::from_str(include_str!("../../tests/fixtures/v1/team/team.json"))?;
        let copy = roots
            .state
            .join("teams")
            .join(team.id())
            .join("drafts/member-alice/letter-local-1/primary");
        fs::create_dir_all(&copy)?;
        fs::write(copy.join("editing.json"),br#"{"reference":"letter-local-1","title":"local edit","to":"","cc":"","body":"offline body"}"#)?;
        let mut cfg = ClientConfig::read(&file)?;
        let profile = Profile {
            team,
            human: "member-alice".into(),
            assistant: String::new(),
            remote: root.join("missing.git").display().to_string(),
        };
        cfg.fields
            .insert("profile".into(), serde_json::to_value(profile)?);
        cfg.save(&file)?;
        let writer = Arc::new(Mutex::new(()));
        let guard = writer.lock().unwrap();
        let (socket, server) = UnixStream::pair()?;
        socket.set_read_timeout(Some(Duration::from_secs(2)))?;
        let lock = writer.clone();
        let (wake, _rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            connection(
                server,
                Arc::new(file),
                lock,
                "fixture".into(),
                wake,
                Arc::new(Mutex::new(None)),
                Arc::new(Mutex::new(PublicationState::Idle)),
                Arc::default(),
            )
        });
        fs::create_dir_all(roots.work.join("local-notes"))?;
        let mut client = Client(BufReader::new(socket));
        assert!(matches!(
            client.request(Request::Hello(VERSION, "fixture".into(), Entry::Human))?,
            Reply::Ready
        ));
        for request in [
            Request::Theme,
            Request::Profile,
            Request::SetupPending,
            Request::Workspace,
            Request::Tags,
            Request::Drafts,
            Request::Published,
            Request::ApplyTheme([1, 2, 3]),
            Request::ToggleTag("local-notes".into()),
            Request::SpaceSync {
                space: "local-notes".into(),
                action: crate::session::protocol::SyncAction::SetMode(
                    crate::adapters::git::workspace_sync::Mode::WhenClean,
                ),
            },
            Request::Identities {
                human: "member-alice".into(),
                assistant: String::new(),
            },
        ] {
            client.request(request)?;
        }
        let Reply::Board {
            drafts, letters, ..
        } = client.request(Request::Board)?
        else {
            panic!("missing board")
        };
        assert_eq!(drafts[0].title, "local edit");
        assert!(letters.is_empty());
        assert!(
            matches!(client.request(Request::OpenDraft("letter-local-1".into()))?,Reply::Draft(d) if d.body=="offline body")
        );
        assert!(matches!(
            client.request(Request::EnqueueSend(crate::application::letters::Draft {
                title: "Send while remote is blocked".into(),
                to: "member-chen".into(),
                ..Default::default()
            }))?,
            Reply::Queued
        ));
        assert!(
            matches!(client.request(Request::DeliveryQueue)?,Reply::DeliveryQueue(rows,_,_) if rows.len()==1 && matches!(rows[0].state,crate::session::protocol::DeliveryState::Waiting))
        );
        client.request(Request::Close)?;
        drop(guard);
        worker.join().unwrap()?;
        fs::remove_dir_all(root)
    }
}
