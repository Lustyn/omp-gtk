pub mod protocol;

use std::ffi::{OsStr, OsString};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use serde::Serialize;
use serde_json::{Map, Value, json};

use self::protocol::{
    BranchRequest, HandoffRequest, ImageContent, InterruptMode, QueueMode, RpcEvent,
    RpcFrameDecoder, decode_event,
};
use crate::commands::unsupported_native_mode_error;

#[derive(Clone)]
pub struct BridgeClient {
    writer: mpsc::Sender<WriterMessage>,
    next_request_id: Arc<AtomicU64>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MessageRequest<'a> {
    message: &'a str,
    #[serde(skip_serializing_if = "image_slice_is_empty")]
    images: &'a [ImageContent],
}
fn image_slice_is_empty(images: &&[ImageContent]) -> bool {
    images.is_empty()
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

    pub fn prompt(&self, message: &str, images: &[ImageContent]) -> Result<String, BridgeError> {
        self.message_request("prompt", message, images)
    }

    pub fn steer(&self, message: &str, images: &[ImageContent]) -> Result<String, BridgeError> {
        self.message_request("steer", message, images)
    }

    pub fn follow_up(&self, message: &str, images: &[ImageContent]) -> Result<String, BridgeError> {
        self.message_request("follow_up", message, images)
    }
    pub fn set_steering_mode(&self, mode: QueueMode) -> Result<(), BridgeError> {
        self.request("set_steering_mode", json!({ "mode": mode }))?;
        Ok(())
    }

    pub fn set_follow_up_mode(&self, mode: QueueMode) -> Result<(), BridgeError> {
        self.request("set_follow_up_mode", json!({ "mode": mode }))?;
        Ok(())
    }

    pub fn set_interrupt_mode(&self, mode: InterruptMode) -> Result<(), BridgeError> {
        self.request("set_interrupt_mode", json!({ "mode": mode }))?;
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

    pub fn abort(&self) -> Result<(), BridgeError> {
        self.request("abort", Value::Null)?;
        Ok(())
    }

    pub fn get_branch_messages(&self) -> Result<(), BridgeError> {
        self.request("get_branch_messages", Value::Null)?;
        Ok(())
    }

    pub fn branch(&self, entry_id: &str) -> Result<(), BridgeError> {
        let request = serde_json::to_value(BranchRequest { entry_id })
            .map_err(|error| BridgeError(format!("failed to encode branch request: {error}")))?;
        self.request("branch", request)?;
        Ok(())
    }

    pub fn handoff(&self, custom_instructions: Option<&str>) -> Result<(), BridgeError> {
        let request = serde_json::to_value(HandoffRequest {
            custom_instructions,
        })
        .map_err(|error| BridgeError(format!("failed to encode handoff request: {error}")))?;
        self.request("handoff", request)?;
        Ok(())
    }

    pub fn set_session_name(&self, name: &str) -> Result<(), BridgeError> {
        self.request("set_session_name", json!({ "name": name }))?;
        Ok(())
    }

    pub fn move_session(&self, path: &Path) -> Result<(), BridgeError> {
        let path = path
            .to_str()
            .ok_or_else(|| BridgeError("workspace path is not valid UTF-8".to_owned()))?;
        if path.contains('\n') || path.contains('\r') {
            return Err(BridgeError(
                "workspace path cannot contain a line break".to_owned(),
            ));
        }
        self.prompt(&format!("/move {path}"), &[]).map(|_| ())
    }

    pub fn get_subagent_messages(
        &self,
        subagent_id: &str,
        from_byte: Option<u64>,
    ) -> Result<String, BridgeError> {
        let fields = match from_byte {
            Some(from_byte) => json!({ "subagentId": subagent_id, "fromByte": from_byte }),
            None => json!({ "subagentId": subagent_id }),
        };
        self.request("get_subagent_messages", fields)
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

    fn message_request(
        &self,
        command: &str,
        message: &str,
        images: &[ImageContent],
    ) -> Result<String, BridgeError> {
        if let Some(error) = unsupported_native_mode_error(message) {
            return Err(BridgeError(error));
        }
        let fields = serde_json::to_value(MessageRequest { message, images })
            .map_err(|error| BridgeError(format!("failed to encode RPC request: {error}")))?;
        self.request(command, fields)
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
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn omp_command(
    shell: &Path,
    executable: &OsStr,
    session_path: Option<&Path>,
    workspace: Option<&Path>,
) -> Command {
    let mut command = Command::new(shell);
    command
        .args(["-l", "-c", r#"exec "$0" "$@""#])
        .arg(executable)
        .args(["--mode", "rpc-ui"]);
    if let Some(path) = session_path {
        command.arg("--session").arg(path);
    }
    if let Some(workspace) = workspace {
        command.current_dir(workspace);
    }
    command
}

impl OmpBridge {
    pub fn spawn() -> io::Result<Self> {
        Self::spawn_for_session(None, None)
    }

    pub fn spawn_for_session(
        session_path: Option<&Path>,
        workspace: Option<&Path>,
    ) -> io::Result<Self> {
        let shell: OsString = std::env::var_os("SHELL")
            .filter(|shell| !shell.is_empty())
            .unwrap_or_else(|| "/bin/sh".into());
        let override_bin: Option<OsString> = std::env::var_os("OMP_BIN");
        let search_path = std::env::var_os("PATH");
        let home_dir = std::env::var_os("HOME");
        let executable = resolve_omp_executable(
            override_bin.as_deref(),
            search_path.as_deref(),
            home_dir.as_deref(),
        );
        let mut child = omp_command(
            Path::new(&shell),
            executable.as_os_str(),
            session_path,
            workspace,
        )
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
    use super::{BridgeClient, WriterMessage, omp_command, resolve_omp_executable};
    use crate::bridge::protocol::{ImageContent, InterruptMode, QueueMode};
    use crate::commands::unsupported_native_mode_error;
    use serde_json::Value;
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::atomic::AtomicU64;
    use std::sync::{Arc, mpsc};
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

    fn create_executable(path: &Path, contents: &[u8]) {
        fs::create_dir_all(path.parent().expect("executable has a parent"))
            .expect("create executable directory");
        fs::write(path, contents).expect("create executable");
        let mut permissions = fs::metadata(path)
            .expect("read executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("make fixture executable");
    }

    #[test]
    fn resolves_omp_from_user_local_bin_when_desktop_path_does_not_contain_it() {
        let fixture = fixture_directory("user-local");
        let path_directory = fixture.join("path");
        let home = fixture.join("home");
        fs::create_dir_all(&path_directory).expect("create PATH fixture");
        let expected = home.join(".local/bin/omp");
        create_executable(&expected, b"");

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

    #[test]
    fn resumed_session_runtime_starts_in_the_user_login_shell_and_workspace() {
        let session = Path::new("/tmp/session with spaces.jsonl");
        let workspace = Path::new("/tmp/existing workspace");
        let command = omp_command(
            Path::new("/bin/zsh"),
            OsStr::new("/opt/omp/bin/omp"),
            Some(session),
            Some(workspace),
        );

        assert_eq!(command.get_program(), OsStr::new("/bin/zsh"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                OsStr::new("-l"),
                OsStr::new("-c"),
                OsStr::new(r#"exec "$0" "$@""#),
                OsStr::new("/opt/omp/bin/omp"),
                OsStr::new("--mode"),
                OsStr::new("rpc-ui"),
                OsStr::new("--session"),
                session.as_os_str(),
            ]
        );
        assert_eq!(command.get_current_dir(), Some(workspace));
    }

    #[test]
    fn omp_inherits_environment_from_the_user_login_shell() {
        let fixture = fixture_directory("shell-environment");
        let home = fixture.join("home");
        let executable = fixture.join("bin/omp");
        fs::create_dir_all(&home).expect("create home fixture");
        fs::write(
            home.join(".profile"),
            b"export OMP_NATIVE_SHELL_ENV=from-login-shell\n",
        )
        .expect("write login profile");
        create_executable(
            &executable,
            b"#!/bin/sh\nprintf '%s\\n' \"$OMP_NATIVE_SHELL_ENV\" \"$@\"\n",
        );

        let output = omp_command(Path::new("/bin/sh"), executable.as_os_str(), None, None)
            .env("HOME", &home)
            .output()
            .expect("run omp fixture through login shell");

        assert!(
            output.status.success(),
            "fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).expect("fixture output is UTF-8"),
            "from-login-shell\n--mode\nrpc-ui\n"
        );
        fs::remove_dir_all(fixture).expect("remove fixture");
    }

    #[test]
    fn emits_typed_running_turn_and_global_delivery_requests() {
        let (writer, receiver) = mpsc::channel();
        let client = BridgeClient {
            writer,
            next_request_id: Arc::new(AtomicU64::new(1)),
        };

        client
            .steer("adjust course", &[])
            .expect("queue steer request");
        client
            .follow_up("verify afterward", &[])
            .expect("queue follow-up request");
        client
            .set_steering_mode(QueueMode::All)
            .expect("queue global steering mode request");
        client
            .set_follow_up_mode(QueueMode::OneAtATime)
            .expect("queue global follow-up mode request");
        client
            .set_interrupt_mode(InterruptMode::Wait)
            .expect("queue global interrupt mode request");

        let frames = (0..5)
            .map(|_| {
                let WriterMessage::Frame(frame) = receiver.recv().expect("receive RPC request")
                else {
                    panic!("expected RPC frame");
                };
                serde_json::from_str::<Value>(&frame).expect("decode RPC request")
            })
            .collect::<Vec<_>>();
        assert_eq!(frames[0]["type"], "steer");
        assert_eq!(frames[0]["message"], "adjust course");
        assert_eq!(frames[1]["type"], "follow_up");
        assert_eq!(frames[1]["message"], "verify afterward");
        assert_eq!(frames[2]["type"], "set_steering_mode");
        assert_eq!(frames[2]["mode"], "all");
        assert_eq!(frames[3]["type"], "set_follow_up_mode");
        assert_eq!(frames[3]["mode"], "one-at-a-time");
        assert_eq!(frames[4]["type"], "set_interrupt_mode");
        assert_eq!(frames[4]["mode"], "wait");
    }

    #[test]
    fn moves_session_to_directory_with_spaces() {
        let (writer, receiver) = mpsc::channel();
        let client = BridgeClient {
            writer,
            next_request_id: Arc::new(AtomicU64::new(1)),
        };

        client
            .move_session(Path::new("/tmp/project with spaces"))
            .expect("queue move request");

        let WriterMessage::Frame(frame) = receiver.recv().expect("receive move request") else {
            panic!("expected RPC frame");
        };
        let frame = serde_json::from_str::<Value>(&frame).expect("decode move request");
        assert_eq!(frame["type"], "prompt");
        assert_eq!(frame["message"], "/move /tmp/project with spaces");
    }

    #[test]
    fn terminal_only_mode_commands_never_reach_rpc() {
        for message in [
            "/vibe",
            "/goal set ship it",
            "/guided-goal rough objective",
            "/loop 5 prompt",
        ] {
            let (writer, receiver) = mpsc::channel();
            let client = BridgeClient {
                writer,
                next_request_id: Arc::new(AtomicU64::new(1)),
            };

            let error = client
                .prompt(message, &[])
                .expect_err("reject mode command");

            assert_eq!(
                error.to_string(),
                unsupported_native_mode_error(message).expect("mode command error")
            );
            assert!(
                matches!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty)),
                "{message} reached RPC"
            );
        }
    }

    #[test]
    fn serializes_ordered_images_for_message_requests() {
        let (writer, receiver) = mpsc::channel();
        let client = BridgeClient {
            writer,
            next_request_id: Arc::new(AtomicU64::new(1)),
        };
        let images = [
            ImageContent::new("Zmlyc3Q=".to_owned(), "image/png"),
            ImageContent::new("c2Vjb25k".to_owned(), "image/jpeg"),
        ];

        let id = client
            .prompt("compare these", &images)
            .expect("queue prompt request");
        assert_eq!(id, "native_1");
        let WriterMessage::Frame(frame) = receiver.recv().expect("receive prompt request") else {
            panic!("expected RPC frame");
        };
        let frame = serde_json::from_str::<Value>(&frame).expect("decode prompt request");
        assert_eq!(frame["type"], "prompt");
        assert_eq!(frame["images"][0]["type"], "image");
        assert_eq!(frame["images"][0]["data"], "Zmlyc3Q=");
        assert_eq!(frame["images"][0]["mimeType"], "image/png");
        assert_eq!(frame["images"][1]["data"], "c2Vjb25k");
        assert_eq!(frame["images"][1]["mimeType"], "image/jpeg");

        client
            .steer("use the second image", &images)
            .expect("queue steer request");
        let WriterMessage::Frame(frame) = receiver.recv().expect("receive steer request") else {
            panic!("expected RPC frame");
        };
        let frame = serde_json::from_str::<Value>(&frame).expect("decode steer request");
        assert_eq!(frame["type"], "steer");
        assert_eq!(frame["images"][0]["data"], "Zmlyc3Q=");
        assert_eq!(frame["images"][1]["data"], "c2Vjb25k");

        client
            .follow_up("then summarize both", &images)
            .expect("queue follow-up request");
        let WriterMessage::Frame(frame) = receiver.recv().expect("receive follow-up request")
        else {
            panic!("expected RPC frame");
        };
        let frame = serde_json::from_str::<Value>(&frame).expect("decode follow-up request");
        assert_eq!(frame["type"], "follow_up");
        assert_eq!(frame["images"][0]["mimeType"], "image/png");
        assert_eq!(frame["images"][1]["mimeType"], "image/jpeg");
    }

    #[test]
    fn serializes_branch_requests_with_stable_entry_ids() {
        let (writer, receiver) = mpsc::channel();
        let client = BridgeClient {
            writer,
            next_request_id: Arc::new(AtomicU64::new(1)),
        };

        client
            .get_branch_messages()
            .expect("queue branch message request");
        client.branch("entry-7f2b").expect("queue branch selection");

        let WriterMessage::Frame(messages) =
            receiver.recv().expect("receive branch message request")
        else {
            panic!("expected RPC frame");
        };
        let WriterMessage::Frame(branch) = receiver.recv().expect("receive branch request") else {
            panic!("expected RPC frame");
        };
        assert_eq!(
            serde_json::from_str::<Value>(&messages).expect("decode branch message request"),
            serde_json::json!({
                "id": "native_1",
                "type": "get_branch_messages",
            })
        );
        assert_eq!(
            serde_json::from_str::<Value>(&branch).expect("decode branch request"),
            serde_json::json!({
                "id": "native_2",
                "type": "branch",
                "entryId": "entry-7f2b",
            })
        );
    }

    #[test]
    fn serializes_optional_handoff_instructions_exactly() {
        let (writer, receiver) = mpsc::channel();
        let client = BridgeClient {
            writer,
            next_request_id: Arc::new(AtomicU64::new(1)),
        };

        client
            .handoff(Some("Focus on the migration boundary"))
            .expect("queue focused handoff");
        client.handoff(None).expect("queue default handoff");

        let WriterMessage::Frame(focused) = receiver.recv().expect("receive focused handoff")
        else {
            panic!("expected RPC frame");
        };
        let WriterMessage::Frame(default) = receiver.recv().expect("receive default handoff")
        else {
            panic!("expected RPC frame");
        };
        assert_eq!(
            serde_json::from_str::<Value>(&focused).expect("decode focused handoff"),
            serde_json::json!({
                "id": "native_1",
                "type": "handoff",
                "customInstructions": "Focus on the migration boundary",
            })
        );
        assert_eq!(
            serde_json::from_str::<Value>(&default).expect("decode default handoff"),
            serde_json::json!({
                "id": "native_2",
                "type": "handoff",
            })
        );
    }
}
