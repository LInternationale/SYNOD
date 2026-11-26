//! Unix session ownership and JSON line transport.
use super::{
    bootstrap,
    protocol::{Entry, Reply, Request, VERSION},
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

fn connection(
    stream: UnixStream,
    file: Arc<PathBuf>,
    writer: Arc<Mutex<()>>,
    generation: String,
    wake: mpsc::Sender<()>,
    sync_error: Arc<Mutex<Option<String>>>,
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
            let _guard = writer
                .lock()
                .map_err(|_| io::Error::other("Settings lock unavailable"))?;
            match request {
                Request::ApplyTheme(_) | Request::SetInterval(_) => unreachable!(),
                Request::SyncStatus => unreachable!(),
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
            }
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
    let sync_worker = thread::spawn(move || {
        loop {
            if let Ok(_guard) = worker_lock.lock() {
                let error = super::host::synchronize(&worker_file)
                    .err()
                    .map(|e| e.to_string());
                if let Ok(mut last) = worker_error.lock() {
                    *last = error;
                }
            }
            let wait = ClientConfig::read(&worker_file)
                .map(|c| Duration::from_secs(c.sync_interval_seconds.get().into()))
                .unwrap_or(Duration::from_secs(30));
            match stop_rx.recv_timeout(wait) {
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                _ => {}
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
                workers.push(thread::spawn(move || {
                    let _ = connection(stream, file, writer, generation, wake, error);
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
        client.request(Request::Close)?;
        drop(guard);
        worker.join().unwrap()?;
        fs::remove_dir_all(root)
    }
}
