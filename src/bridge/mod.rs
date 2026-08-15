pub mod protocol;

use std::ffi::{OsStr, OsString};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use serde_json::{Map, Value, json};

use self::protocol::{RpcEvent, RpcFrameDecoder, decode_event};

#[derive(Clone)]
pub struct BridgeClient {
    writer: mpsc::Sender<WriterMessage>,
    next_request_id: Arc<AtomicU64>,
}

impl BridgeClient {
    pub fn initialize(&self) -> Result<(), BridgeError> {
        self.request("negotiate_protocol", json!({ "protocolVersion": 2 }))?;
        self.request("get_state", Value::Null)?;
        self.request("get_available_models", Value::Null)?;
        self.request("get_available_commands", Value::Null)?;
        self.request("get_messages", Value::Null)?;
        self.request("set_subagent_subscription", json!({ "level": "events" }))?;
        self.request("get_subagents", Value::Null)?;
        Ok(())
    }

    pub fn prompt(&self, message: &str) -> Result<(), BridgeError> {
        self.request("prompt", json!({ "message": message }))?;
        Ok(())
    }

    pub fn set_model(&self, provider: &str, model_id: &str) -> Result<(), BridgeError> {
        self.request(
            "set_model",
            json!({ "provider": provider, "modelId": model_id }),
        )?;
        Ok(())
    }

    pub fn set_thinking_level(&self, level: &str) -> Result<(), BridgeError> {
        self.request("set_thinking_level", json!({ "level": level }))?;
        Ok(())
    }

    pub fn new_session(&self) -> Result<(), BridgeError> {
        self.request("new_session", Value::Null)?;
        Ok(())
    }

    pub fn abort(&self) -> Result<(), BridgeError> {
        self.request("abort", Value::Null)?;
        Ok(())
    }

    pub fn switch_session(&self, path: &Path) -> Result<(), BridgeError> {
        self.request(
            "switch_session",
            json!({ "sessionPath": path.to_string_lossy() }),
        )?;
        Ok(())
    }

    pub fn set_session_name(&self, name: &str) -> Result<(), BridgeError> {
        self.request("set_session_name", json!({ "name": name }))?;
        Ok(())
    }

    pub fn get_subagent_messages(&self, subagent_id: &str) -> Result<(), BridgeError> {
        self.request(
            "get_subagent_messages",
            json!({ "subagentId": subagent_id }),
        )?;
        Ok(())
    }

    pub fn refresh_state(&self) -> Result<(), BridgeError> {
        self.request("get_state", Value::Null)?;
        Ok(())
    }

    pub fn refresh_messages(&self) -> Result<(), BridgeError> {
        self.request("get_messages", Value::Null)?;
        Ok(())
    }

    pub fn refresh_subagents(&self) -> Result<(), BridgeError> {
        self.request("get_subagents", Value::Null)?;
        Ok(())
    }

    pub fn respond_to_extension(&self, payload: Value) -> Result<(), BridgeError> {
        self.send_frame(payload)
    }

    fn request(&self, command: &str, fields: Value) -> Result<String, BridgeError> {
        let id = format!(
            "native_{}",
            self.next_request_id.fetch_add(1, Ordering::Relaxed)
        );
        let mut frame = match fields {
            Value::Object(fields) => fields,
            Value::Null => Map::new(),
            _ => {
                return Err(BridgeError(
                    "RPC request fields must be an object".to_owned(),
                ));
            }
        };
        frame.insert("id".to_owned(), Value::String(id.clone()));
        frame.insert("type".to_owned(), Value::String(command.to_owned()));
        self.send_frame(Value::Object(frame))?;
        Ok(id)
    }

    fn send_frame(&self, frame: Value) -> Result<(), BridgeError> {
        let mut encoded = serde_json::to_string(&frame)
            .map_err(|error| BridgeError(format!("failed to encode RPC request: {error}")))?;
        encoded.push('\n');
        self.writer
            .send(WriterMessage::Frame(encoded))
            .map_err(|_| BridgeError("omp bridge is not running".to_owned()))
    }
}

pub struct OmpBridge {
    pub client: BridgeClient,
    pub events: Receiver<RpcEvent>,
    writer: mpsc::Sender<WriterMessage>,
    shutdown: mpsc::Sender<()>,
    stopped: Arc<AtomicBool>,
}

fn resolve_omp_executable(
    override_bin: Option<&OsStr>,
    search_path: Option<&OsStr>,
    home_dir: Option<&OsStr>,
) -> PathBuf {
    if let Some(override_bin) = override_bin {
        return override_bin.into();
    }

    if let Some(search_path) = search_path {
        for directory in std::env::split_paths(search_path) {
            let candidate = directory.join("omp");
            if is_executable(&candidate) {
                return candidate;
            }
        }
    }

    if let Some(home_dir) = home_dir {
        let candidate = Path::new(home_dir).join(".local/bin/omp");
        if is_executable(&candidate) {
            return candidate;
        }
    }

    "omp".into()
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

impl OmpBridge {
    pub fn spawn() -> io::Result<Self> {
        let override_bin: Option<OsString> = std::env::var_os("OMP_BIN");
        let search_path = std::env::var_os("PATH");
        let home_dir = std::env::var_os("HOME");
        let executable = resolve_omp_executable(
            override_bin.as_deref(),
            search_path.as_deref(),
            home_dir.as_deref(),
        );
        let mut child = Command::new(executable)
            .args(["--mode", "rpc-ui"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("omp stdin was not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("omp stdout was not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("omp stderr was not piped"))?;

        let (events_tx, events) = async_channel::unbounded();
        let (writer, writer_rx) = mpsc::channel();
        let (shutdown, shutdown_rx) = mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));

        spawn_writer(stdin, writer_rx, events_tx.clone());
        spawn_stdout_reader(stdout, events_tx.clone());
        spawn_stderr_reader(stderr, events_tx.clone());

        let stopped_for_supervisor = stopped.clone();
        thread::Builder::new()
            .name("omp-process-supervisor".to_owned())
            .spawn(move || {
                loop {
                    match shutdown_rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                            let _ = child.kill();
                            let _ = child.wait();
                            stopped_for_supervisor.store(true, Ordering::Release);
                            return;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }

                    match child.try_wait() {
                        Ok(Some(status)) => {
                            stopped_for_supervisor.store(true, Ordering::Release);
                            let message = match status.code() {
                                Some(0) => "omp exited".to_owned(),
                                Some(code) => format!("omp exited with status {code}"),
                                None => "omp was terminated".to_owned(),
                            };
                            let _ = events_tx.send_blocking(RpcEvent::Disconnected(message));
                            return;
                        }
                        Ok(None) => {}
                        Err(error) => {
                            stopped_for_supervisor.store(true, Ordering::Release);
                            let _ = events_tx.send_blocking(RpcEvent::Disconnected(format!(
                                "failed to monitor omp: {error}"
                            )));
                            return;
                        }
                    }
                }
            })?;

        Ok(Self {
            client: BridgeClient {
                writer: writer.clone(),
                next_request_id: Arc::new(AtomicU64::new(1)),
            },
            events,
            writer,
            shutdown,
            stopped,
        })
    }

    pub fn shutdown(&self) {
        if self.stopped.swap(true, Ordering::AcqRel) {
            return;
        }
        let _ = self.writer.send(WriterMessage::Shutdown);
        let _ = self.shutdown.send(());
    }
}

impl Drop for OmpBridge {
    fn drop(&mut self) {
        self.shutdown();
    }
}

enum WriterMessage {
    Frame(String),
    Shutdown,
}

fn spawn_writer(
    mut stdin: ChildStdin,
    receiver: mpsc::Receiver<WriterMessage>,
    events: Sender<RpcEvent>,
) {
    thread::Builder::new()
        .name("omp-rpc-writer".to_owned())
        .spawn(move || {
            while let Ok(message) = receiver.recv() {
                match message {
                    WriterMessage::Frame(frame) => {
                        if let Err(error) = stdin
                            .write_all(frame.as_bytes())
                            .and_then(|()| stdin.flush())
                        {
                            let _ = events.send_blocking(RpcEvent::Disconnected(format!(
                                "failed to write to omp: {error}"
                            )));
                            return;
                        }
                    }
                    WriterMessage::Shutdown => return,
                }
            }
        })
        .expect("omp writer thread can start");
}

fn spawn_stdout_reader(stdout: impl io::Read + Send + 'static, events: Sender<RpcEvent>) {
    thread::Builder::new()
        .name("omp-rpc-reader".to_owned())
        .spawn(move || {
            let mut decoder = RpcFrameDecoder::default();
            for line in BufReader::new(stdout).lines() {
                let line = match line {
                    Ok(line) => line,
                    Err(error) => {
                        let _ = events.send_blocking(RpcEvent::Disconnected(format!(
                            "failed to read omp output: {error}"
                        )));
                        return;
                    }
                };
                if line.trim().is_empty() {
                    continue;
                }
                let frame = match serde_json::from_str::<Value>(&line) {
                    Ok(frame) => frame,
                    Err(error) => {
                        let _ = events.send_blocking(RpcEvent::Disconnected(format!(
                            "omp emitted invalid JSON: {error}"
                        )));
                        return;
                    }
                };
                match decoder.push(frame) {
                    Ok(Some(frame)) => {
                        if events.send_blocking(decode_event(frame)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        let _ = events.send_blocking(RpcEvent::Disconnected(format!(
                            "omp protocol error: {error}"
                        )));
                        return;
                    }
                }
            }
        })
        .expect("omp reader thread can start");
}

fn spawn_stderr_reader(stderr: impl io::Read + Send + 'static, events: Sender<RpcEvent>) {
    thread::Builder::new()
        .name("omp-stderr-reader".to_owned())
        .spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else {
                    return;
                };
                if !line.trim().is_empty() && events.send_blocking(RpcEvent::Stderr(line)).is_err()
                {
                    return;
                }
            }
        })
        .expect("omp stderr thread can start");
}

#[derive(Debug, Clone)]
pub struct BridgeError(String);

impl std::fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for BridgeError {}

#[cfg(test)]
mod tests {
    use super::resolve_omp_executable;
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_directory(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after the Unix epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("omp-native-bridge-{name}-{nonce}"));
        fs::create_dir_all(&directory).expect("create fixture directory");
        directory
    }

    fn create_executable(path: &Path) {
        fs::create_dir_all(path.parent().expect("executable has a parent"))
            .expect("create executable directory");
        fs::write(path, b"").expect("create executable");
        let mut permissions = fs::metadata(path)
            .expect("read executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("make fixture executable");
    }

    #[test]
    fn resolves_omp_from_user_local_bin_when_path_does_not_contain_it() {
        let fixture = fixture_directory("user-local");
        let path_directory = fixture.join("path");
        let home = fixture.join("home");
        fs::create_dir_all(&path_directory).expect("create PATH fixture");
        let expected = home.join(".local/bin/omp");
        create_executable(&expected);

        let resolved = resolve_omp_executable(
            None,
            Some(path_directory.as_os_str()),
            Some(home.as_os_str()),
        );

        assert_eq!(resolved, expected);
        fs::remove_dir_all(fixture).expect("remove fixture");
    }

    #[test]
    fn omp_bin_override_takes_precedence() {
        let expected = OsStr::new("/opt/omp/bin/omp");
        let resolved = resolve_omp_executable(
            Some(expected),
            Some(OsStr::new("/usr/bin")),
            Some(OsStr::new("/home/user")),
        );

        assert_eq!(resolved, Path::new(expected));
    }
}
