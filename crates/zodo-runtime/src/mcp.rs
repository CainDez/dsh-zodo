use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};
use zodo_kernel::error::{ErrorCode, Result, ZodoError};

use crate::runtime::read_runtime_info;

const PROTOCOL_FALLBACK: &str = "2025-11-25";
const SUPPORTED_PROTOCOLS: &[&str] = &["2026-07-28", "2025-11-25", "2025-06-18"];

struct McpCore {
    base: String,
    token: String,
    workspace_id: String,
    agent: ureq::Agent,
}

impl McpCore {
    fn connect(data_dir: &Path) -> Result<Self> {
        let info = read_runtime_info(data_dir)?;
        let token = info
            .tokens
            .get("mcp")
            .cloned()
            .ok_or_else(|| ZodoError::new(ErrorCode::PermissionDenied, "MCP token missing"))?;
        Ok(Self {
            base: format!("http://{}:{}", info.host, info.port),
            token,
            workspace_id: info.workspace_id,
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(5))
                .timeout_read(Duration::from_secs(30))
                .build(),
        })
    }

    fn post(&self, suffix: &str, body: &Value) -> Result<Value> {
        let response = self
            .agent
            .post(&format!("{}{}", self.base, suffix))
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|error| match error {
                ureq::Error::Status(status, response) => {
                    let detail = response.into_string().unwrap_or_default();
                    ZodoError::new(
                        ErrorCode::ProviderUnavailable,
                        format!("Core returned HTTP {status}: {detail}"),
                    )
                }
                other => ZodoError::new(
                    ErrorCode::ProviderUnavailable,
                    format!("Core request failed: {other}"),
                ),
            })?;
        response.into_json().map_err(|error| {
            ZodoError::new(
                ErrorCode::StorageFailure,
                format!("Core returned invalid JSON: {error}"),
            )
        })
    }

    fn command(&self, operation: &str, payload: Value) -> Result<Value> {
        let request_id = format!("mcp-{}", uuid::Uuid::new_v4().simple());
        let body = json!({
            "schema": "zodo.command/1",
            "request_id": request_id,
            "idempotency_key": format!("mcp:{}", uuid::Uuid::new_v4().simple()),
            "workspace_id": self.workspace_id,
            "operation": operation,
            "operation_version": 1,
            "payload": payload,
        });
        self.post("/v1/commands", &body)
    }

    fn query(&self, name: &str, payload: Value) -> Result<Value> {
        let body = json!({
            "schema": "zodo.query/1",
            "request_id": format!("mcp-{}", uuid::Uuid::new_v4().simple()),
            "workspace_id": self.workspace_id,
            "query": name,
            "payload": payload,
        });
        self.post("/v1/queries", &body)
    }
}

fn tools() -> Value {
    json!([
        {
            "name": "zodo_list_tasks",
            "description": "List a small safe projection of Zodo tasks. Private note bodies are never returned.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 50}
                },
                "additionalProperties": false
            }
        },
        {
            "name": "zodo_propose",
            "description": "Submit structured change operations as a Core proposal. The MCP client cannot approve or commit it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "intent": {"type": "string"},
                    "operations": {"type": "array", "items": {"type": "object", "additionalProperties": true}}
                },
                "required": ["intent", "operations"],
                "additionalProperties": false
            }
        },
        {
            "name": "zodo_plan_propose",
            "description": "Ask the Core to compute a deterministic schedule proposal. This is not a committed plan.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_ids": {"type": "array", "items": {"type": "string"}},
                    "horizon_start": {"type": "string"},
                    "horizon_end": {"type": "string"}
                },
                "required": ["task_ids", "horizon_start", "horizon_end"],
                "additionalProperties": false
            }
        },
        {
            "name": "zodo_replan",
            "description": "Recompute a local deterministic plan while preserving locked, started and past facts. Returns a proposal, never commits.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_ids": {"type": "array", "items": {"type": "string"}},
                    "horizon_start": {"type": "string"},
                    "horizon_end": {"type": "string"}
                },
                "required": ["task_ids", "horizon_start", "horizon_end"],
                "additionalProperties": false
            }
        },
        {
            "name": "zodo_get_proposal",
            "description": "Read a proposal and its diagnostics by id.",
            "inputSchema": {
                "type": "object",
                "properties": {"proposal_id": {"type": "string"}},
                "required": ["proposal_id"],
                "additionalProperties": false
            }
        }
    ])
}

fn safe_task(task: &Value) -> Value {
    json!({
        "id": task.get("id").cloned().unwrap_or(Value::Null),
        "title": task.get("title").cloned().unwrap_or(Value::Null),
        "status": task.get("status").cloned().unwrap_or(Value::Null),
        "priority": task.get("priority").cloned().unwrap_or(Value::Null),
        "estimate_minutes": task.get("estimate_minutes").cloned().unwrap_or(Value::Null),
        "target_due": task.get("target_due").cloned().unwrap_or(Value::Null),
    })
}

fn call_tool(core: &McpCore, name: &str, arguments: &Value) -> Result<Value> {
    match name {
        "zodo_list_tasks" => {
            let limit = arguments
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(20)
                .clamp(1, 50);
            let text = arguments.get("text").and_then(Value::as_str);
            let response = core.query(
                "tasks.list",
                json!({"filter": {"text": text, "limit": limit, "include_archived": false}}),
            )?;
            let tasks = response
                .get("data")
                .and_then(|data| data.get("tasks"))
                .and_then(Value::as_array)
                .map(|rows| {
                    rows.iter()
                        .map(|row| safe_task(row.get("task").unwrap_or(row)))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Ok(json!({
                "ok": true,
                "revision": response.get("revision").cloned().unwrap_or(Value::Null),
                "tasks": tasks,
            }))
        }
        "zodo_propose" => {
            let intent = arguments
                .get("intent")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let operations = arguments
                .get("operations")
                .cloned()
                .unwrap_or_else(|| json!([]));
            let response = core.command(
                "proposal.create",
                json!({"intent": intent, "operations": operations}),
            )?;
            let proposal = response.get("data").and_then(|data| data.get("proposal"));
            Ok(json!({
                "ok": true,
                "proposal_id": proposal.and_then(|p| p.get("id")).cloned().unwrap_or(Value::Null),
                "status": proposal.and_then(|p| p.get("status")).cloned().unwrap_or(Value::Null),
                "intent": proposal.and_then(|p| p.get("intent")).cloned().unwrap_or(Value::Null),
            }))
        }
        "zodo_plan_propose" | "zodo_replan" => {
            let response = core.command(
                "plan.propose",
                json!({
                    "task_ids": arguments.get("task_ids").cloned().unwrap_or_else(|| json!([])),
                    "horizon_start": arguments.get("horizon_start").cloned().unwrap_or(Value::Null),
                    "horizon_end": arguments.get("horizon_end").cloned().unwrap_or(Value::Null),
                    "move_unlocked": true,
                }),
            )?;
            let data = response.get("data").cloned().unwrap_or(Value::Null);
            let proposal = data.get("proposal");
            let plan = data.get("plan");
            Ok(json!({
                "ok": true,
                "proposal_id": proposal.and_then(|p| p.get("id")).cloned().unwrap_or(Value::Null),
                "status": proposal.and_then(|p| p.get("status")).cloned().unwrap_or(Value::Null),
                "assigned_minutes": plan.and_then(|p| p.get("assignments")).and_then(Value::as_array)
                    .map(|rows| rows.iter().filter_map(|r| r.get("minutes").and_then(Value::as_u64)).sum::<u64>()).unwrap_or(0),
                "unplaced_minutes": plan.and_then(|p| p.get("unplaced")).and_then(Value::as_array)
                    .map(|rows| rows.iter().filter_map(|r| r.get("minutes").and_then(Value::as_u64)).sum::<u64>()).unwrap_or(0),
                "diagnostics": plan.and_then(|p| p.get("diagnostics")).cloned().unwrap_or_else(|| json!([])),
            }))
        }
        "zodo_get_proposal" => {
            let id = arguments
                .get("proposal_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let response = core.query("proposal.get", json!({"id": id}))?;
            let proposal = response.get("data").and_then(|data| data.get("proposal"));
            Ok(json!({
                "ok": true,
                "proposal_id": proposal.and_then(|p| p.get("id")).cloned().unwrap_or(Value::Null),
                "status": proposal.and_then(|p| p.get("status")).cloned().unwrap_or(Value::Null),
                "intent": proposal.and_then(|p| p.get("intent")).cloned().unwrap_or(Value::Null),
                "base_revision": proposal.and_then(|p| p.get("base_revision")).cloned().unwrap_or(Value::Null),
                "diagnostics": proposal.and_then(|p| p.get("diagnostics")).cloned().unwrap_or_else(|| json!([])),
                "preview": proposal.and_then(|p| p.get("preview")).cloned().unwrap_or(Value::Null),
            }))
        }
        other => Err(ZodoError::new(
            ErrorCode::UnsupportedOperation,
            format!("unknown MCP tool {other}"),
        )),
    }
}

pub fn run_mcp(data_dir: &Path) -> Result<()> {
    let core = McpCore::connect(data_dir)?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|error| {
            ZodoError::new(
                ErrorCode::StorageFailure,
                format!("read MCP stdin: {error}"),
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {"code": -32700, "message": format!("parse error: {error}")},
                });
                writeln!(stdout, "{}", response).ok();
                stdout.flush().ok();
                continue;
            }
        };
        let id = request.get("id").cloned();
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        let notification = id.is_none();
        let result: Result<Value> = match method {
            "initialize" => {
                let requested = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or(PROTOCOL_FALLBACK);
                let version = if SUPPORTED_PROTOCOLS.contains(&requested) {
                    requested
                } else {
                    PROTOCOL_FALLBACK
                };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "zodo", "version": env!("CARGO_PKG_VERSION")},
                }))
            }
            "notifications/initialized" | "notifications/cancelled" => Ok(Value::Null),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools()})),
            "tools/call" => {
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match call_tool(&core, name, &arguments) {
                    Ok(value) => Ok(json!({
                        "content": [{"type": "text", "text": serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())}],
                        "isError": false,
                    })),
                    Err(error) => Ok(json!({
                        "content": [{"type": "text", "text": serde_json::to_string(&json!({
                            "ok": false,
                            "error": {"code": error.code, "message": error.message}
                        })).unwrap_or_else(|_| "{}".to_string())}],
                        "isError": true,
                    })),
                }
            }
            "resources/list" => Ok(json!({"resources": []})),
            "prompts/list" => Ok(json!({"prompts": []})),
            other => Err(ZodoError::new(
                ErrorCode::UnsupportedOperation,
                format!("unsupported MCP method {other}"),
            )),
        };
        if notification {
            continue;
        }
        let response = match result {
            Ok(value) => json!({"jsonrpc": "2.0", "id": id, "result": value}),
            Err(error) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": error.message},
            }),
        };
        writeln!(stdout, "{}", response).map_err(|error| {
            ZodoError::new(
                ErrorCode::StorageFailure,
                format!("write MCP stdout: {error}"),
            )
        })?;
        stdout.flush().ok();
    }
    Ok(())
}
