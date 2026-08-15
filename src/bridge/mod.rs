pub mod protocol;

use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
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

impl OmpBridge {
    pub fn spawn() -> io::Result<Self> {
        let executable = std::env::var_os("OMP_BIN").unwrap_or_else(|| "omp".into());
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
