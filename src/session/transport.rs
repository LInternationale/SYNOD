//! Unix session ownership and JSON line transport.
use super::{
    bootstrap,
    protocol::{Reply, Request, VERSION},
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
    serde_json::to_writer(&mut *stream, value)?;
    stream.write_all(b"\n")
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
        match client.request(Request::Hello(VERSION, generation))? {
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

fn connection(stream: UnixStream, file: Arc<Mutex<PathBuf>>, generation: String) -> io::Result<()> {
    let mut reader = BufReader::new(stream);
    if !matches!(read(&mut reader)?, Request::Hello(VERSION,g) if g==generation) {
        return write(
            reader.get_mut(),
            &Reply::Failure("Incompatible session".into()),
        );
    }
    write(reader.get_mut(), &Reply::Ready)?;
    loop {
        let request = read(&mut reader)?;
        let close = matches!(request, Request::Close);
        let result = (|| -> io::Result<Reply> {
            let file = file
                .lock()
                .map_err(|_| io::Error::other("Settings lock unavailable"))?;
            match request {
                Request::ApplyTheme(rgb) => {
                    let mut config = ClientConfig::read(&file)?;
                    config.theme_rgb = rgb;
                    config.save(&file)?;
                    Ok(Reply::Color(rgb))
                }
                Request::Theme => Ok(Reply::Color(ClientConfig::read(&file)?.theme_rgb)),
                Request::Close => Ok(Reply::Ready),
                other @ (Request::Profile
                | Request::Join(_)
                | Request::Create { .. }
                | Request::Identities { .. }
                | Request::Send(_)
                | Request::Published
                | Request::Drafts
                | Request::SaveDraft(_)
                | Request::Workspace) => super::host::perform(&file, other),
                Request::Hello(..) => Err(io::Error::other("Session already connected")),
            }
        })();
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
    let file = Arc::new(Mutex::new(file));
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
                let generation = generation.clone();
                workers.push(thread::spawn(move || {
                    let _ = connection(stream, file, generation);
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
    drop(listener);
    fs::remove_file(runtime.join("endpoint.json"))?;
    fs::remove_file(path)?;
    fs::remove_dir(private_dir)
}
