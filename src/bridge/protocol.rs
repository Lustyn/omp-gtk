use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_REASSEMBLED_BYTES: usize = 64 * 1024 * 1024;
const CHUNK_PAYLOAD_BYTES: usize = 256 * 1024;
const MAX_CHUNKS: usize = MAX_REASSEMBLED_BYTES.div_ceil(CHUNK_PAYLOAD_BYTES);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageContentType {
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageContent {
    #[serde(rename = "type")]
    pub kind: ImageContentType,
    pub data: String,
    pub mime_type: String,
}

impl ImageContent {
    pub fn new(data: String, mime_type: impl Into<String>) -> Self {
        Self {
            kind: ImageContentType::Image,
            data,
            mime_type: mime_type.into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommand {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub description: Option<String>,
    pub input: Option<SlashCommandInput>,
    #[serde(default)]
    pub subcommands: Vec<SlashSubcommand>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SlashCommandInput {
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SlashSubcommand {
    pub name: String,
    pub description: Option<String>,
    pub usage: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSummary {
    pub provider: String,
    pub id: String,
    pub name: Option<String>,
    pub thinking: Option<ModelThinking>,
    pub context_window: Option<u64>,
}

impl ModelSummary {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelThinking {
    #[serde(default)]
    pub efforts: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    #[serde(default)]
    pub tokens: u64,
    #[serde(default)]
    pub context_window: u64,
    #[serde(default)]
    pub percent: f64,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum QueueMode {
    All,
    #[default]
    OneAtATime,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InterruptMode {
    #[default]
    Immediate,
    Wait,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Abandoned,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoPhase {
    pub name: String,
    pub tasks: Vec<TodoItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetTodosResponse {
    pub todo_phases: Vec<TodoPhase>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    pub model: Option<ModelSummary>,
    pub thinking_level: Option<String>,
    #[serde(default)]
    pub is_streaming: bool,
    pub session_name: Option<String>,
    pub session_file: Option<String>,
    #[serde(default)]
    pub is_compacting: bool,
    pub tokens_per_second: Option<f64>,
    pub context_usage: Option<ContextUsage>,
    #[serde(default)]
    pub steering_mode: QueueMode,
    #[serde(default)]
    pub follow_up_mode: QueueMode,
    #[serde(default)]
    pub interrupt_mode: InterruptMode,
    #[serde(default)]
    pub queued_message_count: usize,
    #[serde(default)]
    pub todo_phases: Vec<TodoPhase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchMessage {
    pub entry_id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchMessagesResponse {
    pub messages: Vec<BranchMessage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchRequest<'a> {
    pub entry_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchResponse {
    pub text: String,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_instructions: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffResponse {
    pub saved_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RpcResponse {
    pub id: Option<String>,
    pub command: String,
    pub success: bool,
    pub data: Option<Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolStart {
    pub id: String,
    pub name: String,
    pub args: Value,
    pub intent: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolUpdate {
    pub id: String,
    pub name: String,
    pub partial_result: Value,
}

#[derive(Debug, Clone)]
pub struct ToolEnd {
    pub id: String,
    pub name: String,
    pub result: Value,
    pub is_error: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentUpdateKind {
    Lifecycle,
    Progress,
    Event,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentSnapshot {
    pub id: String,
    pub index: usize,
    pub agent: String,
    pub agent_source: Option<String>,
    pub description: Option<String>,
    pub status: String,
    pub task: Option<String>,
    pub assignment: Option<String>,
    pub session_file: Option<String>,
    #[serde(default)]
    pub last_update: u64,
    pub progress: Option<SubagentProgress>,
    pub parent_tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentProgress {
    pub id: String,
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub agent: String,
    pub agent_source: Option<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub task: String,
    pub assignment: Option<String>,
    pub description: Option<String>,
    pub last_intent: Option<String>,
    pub current_tool: Option<String>,
    pub current_tool_args: Option<String>,
    pub current_tool_start_ms: Option<u64>,
    #[serde(default)]
    pub tool_count: u64,
    #[serde(default)]
    pub requests: u64,
    #[serde(default)]
    pub tokens: u64,
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub duration_ms: u64,
    pub model_role: Option<String>,
    pub resolved_model: Option<String>,
    #[serde(default)]
    pub resolved_model_is_fallback: bool,
    pub retry_state: Option<SubagentRetryState>,
    pub retry_failure: Option<SubagentRetryFailure>,
    pub inflight_task_details: Option<SubagentTaskDetails>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentRetryState {
    pub attempt: u64,
    pub max_attempts: u64,
    pub delay_ms: u64,
    pub error_message: String,
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentRetryFailure {
    pub attempt: u64,
    pub error_message: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubagentTaskDetails {
    #[serde(default)]
    pub progress: Vec<SubagentProgress>,
}

#[derive(Debug, Clone)]
pub struct SubagentUpdate {
    pub kind: SubagentUpdateKind,
    pub id: Option<String>,
    pub index: Option<usize>,
    pub agent: Option<String>,
    pub agent_source: Option<String>,
    pub status: Option<String>,
    pub description: Option<String>,
    pub task: Option<String>,
    pub assignment: Option<String>,
    pub session_file: Option<String>,
    pub parent_tool_call_id: Option<String>,
    pub progress: Option<SubagentProgress>,
    pub activity_event: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentMessages {
    pub session_file: String,
    pub from_byte: u64,
    pub next_byte: u64,
    #[serde(default)]
    pub reset: bool,
    #[serde(default)]
    pub messages: Vec<Value>,
}

#[derive(Debug, Clone)]
pub enum RpcEvent {
    Ready,
    Response(RpcResponse),
    Commands(Vec<SlashCommand>),
    MessageStart(Value),
    TextDelta(String),
    ThinkingDelta(String),
    MessageEnd(Value),
    AgentStart,
    AgentEnd,
    ToolStart(ToolStart),
    ToolUpdate(ToolUpdate),
    ToolEnd(ToolEnd),
    Subagent(SubagentUpdate),
    CommandOutput(String),
    PromptResult(bool),
    SessionInfo { title: Option<String> },
    ConfigChanged,
    Notice { level: String, message: String },
    ModelChanged,
    ThinkingChanged(Option<String>),
    ExtensionUi(Value),
    Stderr(String),
    Disconnected(String),
    Other,
}

pub fn decode_event(value: Value) -> RpcEvent {
    let Some(kind) = value.get("type").and_then(Value::as_str) else {
        return RpcEvent::Other;
    };

    match kind {
        "ready" => RpcEvent::Ready,
        "response" => RpcEvent::Response(RpcResponse {
            id: string_field(&value, "id"),
            command: string_field(&value, "command").unwrap_or_default(),
            success: value
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            data: value.get("data").cloned(),
            error: string_field(&value, "error"),
        }),
        "available_commands_update" => {
            let commands = value
                .get("commands")
                .cloned()
                .and_then(|commands| serde_json::from_value(commands).ok())
                .unwrap_or_default();
            RpcEvent::Commands(commands)
        }
        "message_start" => {
            RpcEvent::MessageStart(value.get("message").cloned().unwrap_or(Value::Null))
        }
        "message_update" => match value
            .get("assistantMessageEvent")
            .and_then(|event| event.get("type"))
            .and_then(Value::as_str)
        {
            Some("text_delta") => RpcEvent::TextDelta(
                value
                    .get("assistantMessageEvent")
                    .and_then(|event| event.get("delta"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ),
            Some("thinking_delta") => RpcEvent::ThinkingDelta(
                value
                    .get("assistantMessageEvent")
                    .and_then(|event| event.get("delta"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            ),
            _ => RpcEvent::Other,
        },
        "message_end" => RpcEvent::MessageEnd(value.get("message").cloned().unwrap_or(Value::Null)),
        "agent_start" => RpcEvent::AgentStart,
        "agent_end" => RpcEvent::AgentEnd,
        "tool_execution_start" => RpcEvent::ToolStart(ToolStart {
            id: string_field(&value, "toolCallId").unwrap_or_default(),
            name: string_field(&value, "toolName").unwrap_or_else(|| "tool".to_owned()),
            args: value.get("args").cloned().unwrap_or(Value::Null),
            intent: string_field(&value, "intent"),
        }),
        "tool_execution_update" => RpcEvent::ToolUpdate(ToolUpdate {
            id: string_field(&value, "toolCallId").unwrap_or_default(),
            name: string_field(&value, "toolName").unwrap_or_else(|| "tool".to_owned()),
            partial_result: value.get("partialResult").cloned().unwrap_or(Value::Null),
        }),
        "tool_execution_end" => RpcEvent::ToolEnd(ToolEnd {
            id: string_field(&value, "toolCallId").unwrap_or_default(),
            name: string_field(&value, "toolName").unwrap_or_else(|| "tool".to_owned()),
            result: value.get("result").cloned().unwrap_or(Value::Null),
            is_error: value
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }),
        "subagent_lifecycle" => {
            RpcEvent::Subagent(decode_subagent(SubagentUpdateKind::Lifecycle, value))
        }
        "subagent_progress" => {
            RpcEvent::Subagent(decode_subagent(SubagentUpdateKind::Progress, value))
        }
        "subagent_event" => RpcEvent::Subagent(decode_subagent(SubagentUpdateKind::Event, value)),
        "notice" => RpcEvent::Notice {
            level: string_field(&value, "level").unwrap_or_else(|| "info".to_owned()),
            message: string_field(&value, "message").unwrap_or_default(),
        },
        "command_output" => {
            RpcEvent::CommandOutput(string_field(&value, "text").unwrap_or_default())
        }
        "prompt_result" => RpcEvent::PromptResult(
            value
                .get("agentInvoked")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),
        "session_info_update" => RpcEvent::SessionInfo {
            title: string_field(&value, "title"),
        },
        "config_update" => RpcEvent::ConfigChanged,
        "rpc_frame_error" => RpcEvent::Notice {
            level: "error".to_owned(),
            message: string_field(&value, "error")
                .unwrap_or_else(|| "omp could not deliver a complete RPC frame".to_owned()),
        },
        "model_changed" => RpcEvent::ModelChanged,
        "thinking_level_changed" => RpcEvent::ThinkingChanged(
            string_field(&value, "configured").or_else(|| string_field(&value, "thinkingLevel")),
        ),
        "extension_ui_request" => RpcEvent::ExtensionUi(value),
        _ => RpcEvent::Other,
    }
}

fn decode_subagent(kind: SubagentUpdateKind, value: Value) -> SubagentUpdate {
    let payload = value.get("payload").cloned().unwrap_or(Value::Null);
    let progress_value = payload.get("progress").cloned().unwrap_or(Value::Null);
    let activity_event = payload
        .get("event")
        .and_then(|event| string_field(event, "type"));
    let progress = serde_json::from_value::<SubagentProgress>(progress_value).ok();
    SubagentUpdate {
        kind,
        id: string_field(&payload, "id")
            .or_else(|| progress.as_ref().map(|progress| progress.id.clone())),
        index: payload
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
            .or_else(|| progress.as_ref().map(|progress| progress.index)),
        agent: string_field(&payload, "agent")
            .or_else(|| progress.as_ref().map(|progress| progress.agent.clone())),
        agent_source: string_field(&payload, "agentSource").or_else(|| {
            progress
                .as_ref()
                .and_then(|progress| progress.agent_source.clone())
        }),
        status: string_field(&payload, "status")
            .or_else(|| progress.as_ref().map(|progress| progress.status.clone())),
        description: string_field(&payload, "description").or_else(|| {
            progress
                .as_ref()
                .and_then(|progress| progress.description.clone())
        }),
        task: string_field(&payload, "task")
            .or_else(|| progress.as_ref().map(|progress| progress.task.clone())),
        assignment: string_field(&payload, "assignment").or_else(|| {
            progress
                .as_ref()
                .and_then(|progress| progress.assignment.clone())
        }),
        session_file: string_field(&payload, "sessionFile"),
        parent_tool_call_id: string_field(&payload, "parentToolCallId"),
        progress,
        activity_event,
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

pub fn message_role(message: &Value) -> Option<&str> {
    message.get("role").and_then(Value::as_str)
}

pub fn message_text(message: &Value) -> String {
    let Some(content) = message.get("content") else {
        return String::new();
    };
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    let Some(blocks) = content.as_array() else {
        return String::new();
    };

    blocks
        .iter()
        .filter_map(|block| match block.get("type").and_then(Value::as_str) {
            Some("text") => block.get("text").and_then(Value::as_str),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn message_thinking(message: &Value) -> String {
    message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("thinking"))
        .filter_map(|block| block.get("thinking").and_then(Value::as_str))
        .filter(|text| {
            let trimmed = text.trim();
            !trimmed.is_empty() && trimmed != "<!-- -->"
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn message_cost(message: &Value) -> f64 {
    message
        .pointer("/usage/cost/total")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(0.0)
}

pub fn message_tool_calls(message: &Value) -> Vec<ToolStart> {
    let Some(blocks) = message.get("content").and_then(Value::as_array) else {
        return Vec::new();
    };
    blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("toolCall"))
        .map(|block| ToolStart {
            id: string_field(block, "id").unwrap_or_default(),
            name: string_field(block, "name").unwrap_or_else(|| "tool".to_owned()),
            args: block.get("arguments").cloned().unwrap_or(Value::Null),
            intent: string_field(block, "intent"),
        })
        .collect()
}

pub fn tool_result_parts(message: &Value) -> Option<(String, String, Value, bool)> {
    if message_role(message) != Some("toolResult") {
        return None;
    }
    Some((
        string_field(message, "toolCallId").unwrap_or_default(),
        string_field(message, "toolName").unwrap_or_else(|| "tool".to_owned()),
        message.get("content").cloned().unwrap_or(Value::Null),
        message
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    ))
}

#[derive(Debug)]
pub struct ProtocolError(String);

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ProtocolError {}

struct PendingChunks {
    chunk_id: String,
    expected_count: usize,
    expected_len: usize,
    next_index: usize,
    chunks: Vec<Vec<u8>>,
    received_bytes: usize,
}

#[derive(Default)]
pub struct RpcFrameDecoder {
    pending: Option<PendingChunks>,
}

impl RpcFrameDecoder {
    pub fn push(&mut self, value: Value) -> Result<Option<Value>, ProtocolError> {
        if value.get("type").and_then(Value::as_str) != Some("rpc_chunk") {
            if self.pending.is_some() {
                return Err(ProtocolError(
                    "RPC chunk sequence was interrupted".to_owned(),
                ));
            }
            if !value.is_object() {
                return Err(ProtocolError("RPC frame must be an object".to_owned()));
            }
            return Ok(Some(value));
        }

        let chunk_id = string_field(&value, "chunkId")
            .filter(|id| !id.is_empty() && id.len() <= 128)
            .ok_or_else(|| ProtocolError("RPC chunk has invalid chunkId".to_owned()))?;
        let index = usize_field(&value, "index")?;
        let count = usize_field(&value, "count")?;
        let byte_len = usize_field(&value, "byteLength")?;
        if !(2..=MAX_CHUNKS).contains(&count) || index >= count {
            return Err(ProtocolError("RPC chunk bounds are invalid".to_owned()));
        }
        if byte_len > MAX_REASSEMBLED_BYTES {
            return Err(ProtocolError(
                "RPC frame exceeds the 64 MiB ceiling".to_owned(),
            ));
        }
        let encoded = string_field(&value, "data")
            .filter(|data| !data.is_empty())
            .ok_or_else(|| ProtocolError("RPC chunk is missing data".to_owned()))?;
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|error| ProtocolError(format!("RPC chunk has invalid base64: {error}")))?;
        if bytes.len() > CHUNK_PAYLOAD_BYTES {
            return Err(ProtocolError(
                "RPC chunk payload exceeds the 256 KiB ceiling".to_owned(),
            ));
        }

        if self.pending.is_none() {
            if index != 0 {
                return Err(ProtocolError(
                    "RPC chunk sequence must start at index zero".to_owned(),
                ));
            }
            self.pending = Some(PendingChunks {
                chunk_id: chunk_id.clone(),
                expected_count: count,
                expected_len: byte_len,
                next_index: 0,
                chunks: Vec::with_capacity(count),
                received_bytes: 0,
            });
        }

        let pending = self.pending.as_mut().expect("pending frame exists");
        if pending.chunk_id != chunk_id
            || pending.expected_count != count
            || pending.expected_len != byte_len
            || pending.next_index != index
        {
            return Err(ProtocolError("RPC chunk sequence mismatch".to_owned()));
        }
        pending.received_bytes += bytes.len();
        if pending.received_bytes > pending.expected_len {
            return Err(ProtocolError(
                "RPC chunks exceed their declared byte length".to_owned(),
            ));
        }
        pending.chunks.push(bytes);
        pending.next_index += 1;
        if pending.next_index < pending.expected_count {
            return Ok(None);
        }
        if pending.received_bytes != pending.expected_len {
            return Err(ProtocolError(format!(
                "RPC frame byte length mismatch: expected {}, got {}",
                pending.expected_len, pending.received_bytes
            )));
        }

        let completed = self.pending.take().expect("completed frame exists");
        let mut json = Vec::with_capacity(completed.expected_len);
        for chunk in completed.chunks {
            json.extend(chunk);
        }
        let frame: Value = serde_json::from_slice(&json)
            .map_err(|error| ProtocolError(format!("RPC frame contains invalid JSON: {error}")))?;
        if !frame.is_object() {
            return Err(ProtocolError("RPC frame must be an object".to_owned()));
        }
        Ok(Some(frame))
    }
}

fn usize_field(value: &Value, key: &str) -> Result<usize, ProtocolError> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| ProtocolError(format!("RPC chunk has invalid {key}")))
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde_json::json;

    use super::{
        BranchMessagesResponse, BranchResponse, HandoffResponse, RpcEvent, RpcFrameDecoder,
        SessionState, SetTodosResponse, SubagentMessages, SubagentSnapshot, TodoPhase, TodoStatus,
        decode_event,
    };

    #[test]
    fn passes_regular_frames_through() {
        let mut decoder = RpcFrameDecoder::default();
        let frame = json!({"type": "ready"});
        assert_eq!(decoder.push(frame.clone()).unwrap(), Some(frame));
    }

    #[test]
    fn reassembles_protocol_v2_chunks() {
        let payload = br#"{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"hello"}}"#;
        let split = payload.len() / 2;
        let mut decoder = RpcFrameDecoder::default();
        let first = json!({
            "type": "rpc_chunk",
            "chunkId": "chunk-1",
            "index": 0,
            "count": 2,
            "byteLength": payload.len(),
            "data": STANDARD.encode(&payload[..split]),
        });
        let second = json!({
            "type": "rpc_chunk",
            "chunkId": "chunk-1",
            "index": 1,
            "count": 2,
            "byteLength": payload.len(),
            "data": STANDARD.encode(&payload[split..]),
        });

        assert!(decoder.push(first).unwrap().is_none());
        let decoded = decoder.push(second).unwrap().unwrap();
        assert_eq!(decoded["assistantMessageEvent"]["delta"], "hello");
    }

    #[test]
    fn rejects_duplicate_chunks() {
        let payload = br#"{"type":"ready"}"#;
        let frame = json!({
            "type": "rpc_chunk",
            "chunkId": "chunk-1",
            "index": 0,
            "count": 2,
            "byteLength": payload.len() * 2,
            "data": STANDARD.encode(payload),
        });
        let mut decoder = RpcFrameDecoder::default();
        assert!(decoder.push(frame.clone()).unwrap().is_none());
        assert!(decoder.push(frame).is_err());
    }

    #[test]
    fn normalizes_subagent_progress_identity() {
        let event = decode_event(json!({
            "type": "subagent_progress",
            "payload": {
                "index": 0,
                "agent": "scout",
                "task": "Read Cargo.toml",
                "progress": {
                    "id": "CargoPackageScout",
                    "status": "running",
                    "description": "Inspecting package metadata"
                }
            }
        }));
        let RpcEvent::Subagent(update) = event else {
            panic!("expected a subagent update");
        };
        assert_eq!(update.id.as_deref(), Some("CargoPackageScout"));
        assert_eq!(update.status.as_deref(), Some("running"));
        assert_eq!(update.task.as_deref(), Some("Read Cargo.toml"));
    }

    #[test]
    fn preserves_response_id_for_submission_reconciliation() {
        let event = decode_event(json!({
            "type": "response",
            "id": "native_42",
            "command": "prompt",
            "success": false,
            "error": "rejected"
        }));
        let RpcEvent::Response(response) = event else {
            panic!("expected an RPC response");
        };
        assert_eq!(response.id.as_deref(), Some("native_42"));
        assert_eq!(response.command, "prompt");
        assert!(!response.success);
    }

    #[test]
    fn preserves_full_snapshot_activity_metrics_and_nested_agent_ids() {
        let child = json!({
            "id": "Child",
            "index": 0,
            "agent": "scout",
            "status": "running",
            "task": "Read RPC",
            "recentTools": [],
            "recentOutput": [],
            "toolCount": 1,
            "requests": 1,
            "tokens": 2000,
            "cost": 0.005,
            "durationMs": 10000
        });
        let snapshot = serde_json::from_value::<SubagentSnapshot>(json!({
            "id": "Parent",
            "index": 0,
            "agent": "task",
            "agentSource": "project",
            "description": "Coordinate work",
            "status": "running",
            "task": "Build the hub",
            "assignment": "Implement the runtime surface",
            "sessionFile": "/tmp/parent.jsonl",
            "parentToolCallId": "tool-parent",
            "lastUpdate": 100,
            "progress": {
                "id": "Parent",
                "index": 0,
                "agent": "task",
                "agentSource": "project",
                "status": "running",
                "task": "Build the hub",
                "lastIntent": "Inspecting metadata",
                "currentTool": "read",
                "recentTools": [],
                "recentOutput": [],
                "toolCount": 4,
                "requests": 3,
                "tokens": 12000,
                "contextTokens": 6000,
                "contextWindow": 272000,
                "cost": 0.04,
                "durationMs": 90000,
                "inflightTaskDetails": { "progress": [child] }
            }
        }))
        .unwrap();

        let progress = snapshot.progress.unwrap();
        assert_eq!(progress.current_tool.as_deref(), Some("read"));
        assert_eq!(progress.context_window, Some(272_000));
        assert_eq!(
            progress.inflight_task_details.unwrap().progress[0].id,
            "Child"
        );
    }

    #[test]
    fn keeps_raw_agent_activity_separate_from_lifecycle_status() {
        let event = decode_event(json!({
            "type": "subagent_event",
            "payload": {
                "id": "Worker",
                "event": { "type": "tool_execution_start" }
            }
        }));
        let RpcEvent::Subagent(update) = event else {
            panic!("expected subagent event");
        };
        assert!(update.status.is_none());
        assert_eq!(
            update.activity_event.as_deref(),
            Some("tool_execution_start")
        );
    }

    #[test]
    fn decodes_incremental_subagent_transcript_cursor() {
        let transcript = serde_json::from_value::<SubagentMessages>(json!({
            "sessionFile": "/tmp/worker.jsonl",
            "fromByte": 128,
            "nextByte": 512,
            "reset": false,
            "entries": [],
            "messages": [{ "role": "assistant", "content": "Done" }]
        }))
        .unwrap();
        assert_eq!(transcript.session_file, "/tmp/worker.jsonl");
        assert_eq!(transcript.from_byte, 128);
        assert_eq!(transcript.next_byte, 512);
        assert_eq!(transcript.messages.len(), 1);
    }

    #[test]
    fn decodes_slash_command_output() {
        let event = decode_event(json!({
            "type": "command_output",
            "text": "Current model: openai-codex/gpt-5.6-sol"
        }));
        let RpcEvent::CommandOutput(text) = event else {
            panic!("expected command output");
        };
        assert_eq!(text, "Current model: openai-codex/gpt-5.6-sol");
    }

    #[test]
    fn todo_phases_round_trip_the_exact_rpc_shape_and_every_state() {
        let wire = json!([
            {
                "name": "Ordered",
                "tasks": [
                    {"content": "Wait", "status": "pending"},
                    {"content": "Work", "status": "in_progress"},
                    {"content": "Done", "status": "completed"},
                    {"content": "Drop", "status": "abandoned"},
                    {"content": "Blocked", "status": "blocked", "blocker": "Needs approval"}
                ]
            }
        ]);
        let phases: Vec<TodoPhase> = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(phases[0].tasks[0].status, TodoStatus::Pending);
        assert_eq!(phases[0].tasks[1].status, TodoStatus::InProgress);
        assert_eq!(phases[0].tasks[2].status, TodoStatus::Completed);
        assert_eq!(phases[0].tasks[3].status, TodoStatus::Abandoned);
        assert_eq!(phases[0].tasks[4].status, TodoStatus::Blocked);
        assert_eq!(serde_json::to_value(&phases).unwrap(), wire);
    }

    #[test]
    fn state_and_set_todos_response_decode_camel_case_todo_phases() {
        let phases = json!([
            {
                "name": "Ship",
                "tasks": [{"content": "Commit", "status": "in_progress"}]
            }
        ]);
        let state: SessionState =
            serde_json::from_value(json!({"todoPhases": phases.clone()})).unwrap();
        assert_eq!(state.todo_phases[0].name, "Ship");
        let response: SetTodosResponse =
            serde_json::from_value(json!({"todoPhases": phases})).unwrap();
        assert_eq!(response.todo_phases, state.todo_phases);
    }

    #[test]
    fn decodes_branch_and_handoff_response_shapes() {
        let messages: BranchMessagesResponse = serde_json::from_value(json!({
            "messages": [
                {"entryId": "entry-a", "text": "Start here"},
                {"entryId": "entry-b", "text": "Or here"}
            ]
        }))
        .expect("decode branch messages");
        assert_eq!(messages.messages[1].entry_id, "entry-b");

        let branch: BranchResponse = serde_json::from_value(json!({
            "text": "Or here",
            "cancelled": false
        }))
        .expect("decode branch response");
        assert_eq!(branch.text, "Or here");
        assert!(!branch.cancelled);

        let handoff: HandoffResponse = serde_json::from_value(json!({
            "savedPath": "/tmp/handoff.txt"
        }))
        .expect("decode handoff response");
        assert_eq!(handoff.saved_path.as_deref(), Some("/tmp/handoff.txt"));
    }
}
