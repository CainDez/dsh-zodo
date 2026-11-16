use std::path::PathBuf;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{info, warn};
use zodo_kernel::app::Application;
use zodo_kernel::commands::CommandEnvelope;
use zodo_kernel::error::{ErrorCode, Result, ZodoError};
use zodo_kernel::model::{CommandContext, PrincipalKind};
use zodo_kernel::ops::{ChangeOp, TaskSpec};
use zodo_kernel::ports::{Store, SystemClock, UuidIds};

use crate::ai::{ChatMessage, DataSensitivity, ModelGateway, parse_json_object};
use crate::runtime::{AppState, WORKSPACE_ID};
use rusqlite::OptionalExtension;

use crate::store::SqliteStore;

pub const RUN_STATES: &[&str] = &[
    "queued",
    "running",
    "waiting",
    "succeeded",
    "failed",
    "cancelled",
    "interrupted",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub workspace_id: String,
    pub state: String,
    pub intent: String,
    pub input: Value,
    pub checkpoint: Option<Value>,
    pub output: Option<Value>,
    pub error: Option<Value>,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub cancel_requested: bool,
    pub lease_owner: Option<String>,
    pub heartbeat_at: Option<String>,
}

impl RunRecord {
    pub fn safe_view(&self) -> Value {
        json!({
            "id": self.id,
            "schema": "zodo.run/1",
            "workspace_id": self.workspace_id,
            "state": self.state,
            "intent": self.intent,
            "input": self.input,
            "checkpoint": self.checkpoint,
            "output": self.output,
            "error": self.error,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
            "started_at": self.started_at,
            "ended_at": self.ended_at,
            "cancel_requested": self.cancel_requested,
        })
    }
}

fn parse_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunRecord> {
    let input: String = row.get(4)?;
    let checkpoint: Option<String> = row.get(5)?;
    let output: Option<String> = row.get(6)?;
    let error: Option<String> = row.get(7)?;
    Ok(RunRecord {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        state: row.get(2)?,
        intent: row.get(3)?,
        input: serde_json::from_str(&input).unwrap_or(Value::Null),
        checkpoint: checkpoint.and_then(|value| serde_json::from_str(&value).ok()),
        output: output.and_then(|value| serde_json::from_str(&value).ok()),
        error: error.and_then(|value| serde_json::from_str(&value).ok()),
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        started_at: row.get(10)?,
        ended_at: row.get(11)?,
        cancel_requested: row.get::<_, i64>(12)? != 0,
        lease_owner: row.get(13)?,
        heartbeat_at: row.get(14)?,
    })
}

const RUN_COLUMNS: &str = "id, workspace_id, state, intent, input_json, checkpoint_json, output_json, error_json, created_at, updated_at, started_at, ended_at, cancel_requested, lease_owner, heartbeat_at";

impl SqliteStore {
    pub fn create_run(&self, workspace_id: &str, intent: &str, input: &Value) -> Result<RunRecord> {
        let id = format!("run_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        self.raw_connection()
            .execute(
                "INSERT INTO runs (id, workspace_id, state, intent, input_json, created_at, updated_at)
                 VALUES (?1, ?2, 'queued', ?3, ?4, ?5, ?5)",
                rusqlite::params![
                    id,
                    workspace_id,
                    intent,
                    serde_json::to_string(input).map_err(storage)?,
                    now
                ],
            )
            .map_err(storage)?;
        self.get_run(&id)?.ok_or_else(|| {
            ZodoError::new(ErrorCode::StorageFailure, "run disappeared after insert")
        })
    }

    pub fn get_run(&self, id: &str) -> Result<Option<RunRecord>> {
        self.raw_connection()
            .query_row(
                &format!("SELECT {RUN_COLUMNS} FROM runs WHERE id = ?1"),
                rusqlite::params![id],
                parse_run,
            )
            .optional()
            .map_err(storage)
    }

    pub fn list_runs(&self, workspace_id: &str, limit: u32) -> Result<Vec<RunRecord>> {
        let mut statement = self
            .raw_connection()
            .prepare(&format!(
                "SELECT {RUN_COLUMNS} FROM runs WHERE workspace_id = ?1 ORDER BY created_at DESC LIMIT ?2"
            ))
            .map_err(storage)?;
        let rows = statement
            .query_map(
                rusqlite::params![workspace_id, limit.min(200) as i64],
                parse_run,
            )
            .map_err(storage)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage)
    }

    pub fn claim_next_run(&self, workspace_id: &str, owner: &str) -> Result<Option<RunRecord>> {
        let now = Utc::now().to_rfc3339();
        self.raw_connection()
            .query_row(
                &format!(
                    "UPDATE runs SET state = 'running', started_at = COALESCE(started_at, ?2),
                     heartbeat_at = ?2, lease_owner = ?3, updated_at = ?2
                     WHERE id = (SELECT id FROM runs
                       WHERE workspace_id = ?1 AND state = 'queued' AND cancel_requested = 0
                       ORDER BY created_at ASC LIMIT 1)
                     RETURNING {RUN_COLUMNS}"
                ),
                rusqlite::params![workspace_id, now, owner],
                parse_run,
            )
            .optional()
            .map_err(storage)
    }

    pub fn update_run_state(
        &self,
        id: &str,
        state: &str,
        output: Option<&Value>,
        error: Option<&Value>,
    ) -> Result<()> {
        if !RUN_STATES.contains(&state) {
            return Err(ZodoError::validation("unknown run state"));
        }
        let now = Utc::now().to_rfc3339();
        let ended = matches!(state, "succeeded" | "failed" | "cancelled" | "interrupted")
            .then_some(now.clone());
        let changed = self
            .raw_connection()
            .execute(
                "UPDATE runs SET state = ?2, output_json = COALESCE(?3, output_json),
                 error_json = COALESCE(?4, error_json), ended_at = COALESCE(?5, ended_at),
                 lease_owner = NULL, heartbeat_at = ?6, updated_at = ?6
                 WHERE id = ?1 AND NOT (cancel_requested = 1 AND ?2 = 'succeeded')",
                rusqlite::params![
                    id,
                    state,
                    output
                        .map(serde_json::to_string)
                        .transpose()
                        .map_err(storage)?,
                    error
                        .map(serde_json::to_string)
                        .transpose()
                        .map_err(storage)?,
                    ended,
                    now,
                ],
            )
            .map_err(storage)?;
        if changed == 0 && state == "succeeded" {
            return Err(ZodoError::validation(
                "run was cancelled before it could succeed",
            ));
        }
        Ok(())
    }

    pub fn checkpoint_run(&self, id: &str, checkpoint: &Value) -> Result<()> {
        self.raw_connection()
            .execute(
                "UPDATE runs SET checkpoint_json = ?2, updated_at = ?3 WHERE id = ?1",
                rusqlite::params![
                    id,
                    serde_json::to_string(checkpoint).map_err(storage)?,
                    Utc::now().to_rfc3339()
                ],
            )
            .map_err(storage)?;
        Ok(())
    }

    pub fn append_run_step(&self, run_id: &str, kind: &str, payload: &Value) -> Result<()> {
        let next: i64 = self
            .raw_connection()
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM run_steps WHERE run_id = ?1",
                rusqlite::params![run_id],
                |row| row.get(0),
            )
            .map_err(storage)?;
        self.raw_connection()
            .execute(
                "INSERT INTO run_steps (run_id, seq, kind, payload_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    run_id,
                    next,
                    kind,
                    serde_json::to_string(payload).map_err(storage)?,
                    Utc::now().to_rfc3339()
                ],
            )
            .map_err(storage)?;
        Ok(())
    }

    pub fn request_run_cancel(&self, id: &str) -> Result<Option<RunRecord>> {
        let now = Utc::now().to_rfc3339();
        self.raw_connection()
            .execute(
                "UPDATE runs SET cancel_requested = 1, updated_at = ?2
                 WHERE id = ?1 AND state IN ('queued', 'running', 'waiting')",
                rusqlite::params![id, now],
            )
            .map_err(storage)?;
        self.raw_connection()
            .execute(
                "UPDATE runs SET state = 'cancelled', ended_at = ?2, updated_at = ?2
                 WHERE id = ?1 AND state IN ('queued', 'waiting') AND cancel_requested = 1",
                rusqlite::params![id, now],
            )
            .map_err(storage)?;
        self.get_run(id)
    }

    pub fn recover_runs(&self, workspace_id: &str) -> Result<u32> {
        let now = Utc::now().to_rfc3339();
        let cancelled = self
            .raw_connection()
            .execute(
                "UPDATE runs SET state = 'cancelled', ended_at = ?2, updated_at = ?2
                 WHERE workspace_id = ?1 AND state IN ('queued', 'running') AND cancel_requested = 1",
                rusqlite::params![workspace_id, now],
            )
            .map_err(storage)?;
        let recovered = self
            .raw_connection()
            .execute(
                "UPDATE runs SET state = 'queued', lease_owner = NULL, heartbeat_at = NULL, updated_at = ?2
                 WHERE workspace_id = ?1 AND state = 'running' AND cancel_requested = 0",
                rusqlite::params![workspace_id, Utc::now().to_rfc3339()],
            )
            .map_err(storage)?;
        Ok((cancelled + recovered) as u32)
    }
}

fn storage(error: impl std::fmt::Display) -> ZodoError {
    ZodoError::new(ErrorCode::StorageFailure, error.to_string())
}

pub fn validate_run_intent(intent: &str, input: &Value) -> Result<()> {
    match intent {
        "goal.decompose" => {
            let goal_id = input
                .get("goal_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if goal_id.trim().is_empty() {
                return Err(ZodoError::validation("goal.decompose requires goal_id"));
            }
            if input
                .get("project_id")
                .and_then(Value::as_str)
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err(ZodoError::validation("project_id cannot be empty"));
            }
            Ok(())
        }
        "capture.clarify" => {
            let text = input
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if text.trim().is_empty() {
                return Err(ZodoError::validation("capture.clarify requires text"));
            }
            if text.chars().count() > 4_000 {
                return Err(ZodoError::validation(
                    "capture.clarify text is limited to 4000 chars",
                ));
            }
            Ok(())
        }
        other => Err(ZodoError::new(
            ErrorCode::UnsupportedOperation,
            format!("unsupported run intent {other}"),
        )),
    }
}

pub fn spawn_run_worker(state: AppState, data_dir: PathBuf) {
    if let Ok(inner) = state.inner.lock() {
        if let Err(error) = inner.store.recover_runs(WORKSPACE_ID) {
            warn!("failed to recover runs: {}", error.message);
        }
    }
    std::thread::Builder::new()
        .name("zodo-run-worker".to_string())
        .spawn(move || run_loop(state, data_dir))
        .expect("spawn Zodo run worker");
}

fn run_loop(state: AppState, data_dir: PathBuf) {
    let owner = format!("worker_{}", std::process::id());
    loop {
        let claimed = {
            let inner = match state.inner.lock() {
                Ok(inner) => inner,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
            };
            inner.store.claim_next_run(WORKSPACE_ID, &owner)
        };
        let run = match claimed {
            Ok(Some(run)) => run,
            Ok(None) => {
                std::thread::sleep(std::time::Duration::from_millis(400));
                continue;
            }
            Err(error) => {
                warn!("run worker claim failed: {}", error.message);
                std::thread::sleep(std::time::Duration::from_millis(1000));
                continue;
            }
        };
        if let Err(error) = process_run(&state, &data_dir, &run) {
            warn!(
                "run {} failed: {} ({:?})",
                run.id, error.message, error.code
            );
            let error_json = json!({
                "code": error.code,
                "message": error.message,
                "details": error.details,
            });
            if let Ok(inner) = state.inner.lock() {
                let _ = inner.store.append_run_step(&run.id, "failed", &error_json);
                let _ = inner
                    .store
                    .update_run_state(&run.id, "failed", None, Some(&error_json));
            }
        }
    }
}

fn process_clarify(state: &AppState, data_dir: &std::path::Path, run: &RunRecord) -> Result<()> {
    let (policy, text) = {
        let mut inner = state
            .inner
            .lock()
            .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"))?;
        if run.cancel_requested {
            inner
                .store
                .update_run_state(&run.id, "cancelled", None, None)?;
            return Ok(());
        }
        let world = inner
            .store
            .load_world(&run.workspace_id)?
            .ok_or_else(|| ZodoError::not_found("workspace", &run.workspace_id))?;
        let text = run
            .input
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        inner.store.append_run_step(
            &run.id,
            "context.loaded",
            &json!({"chars": text.chars().count(), "revision": world.workspace.revision}),
        )?;
        (world.workspace.policy.clone(), text)
    };
    let gateway = ModelGateway::load(data_dir);
    let system = "You clarify a raw personal capture for Zodo. Return ONLY JSON: {\"questions\":[string],\"suggested_kind\":\"task\"|\"note\"|\"goal\"|\"keep\",\"assumptions\":[string]}. Ask at most 4 concise questions that resolve ambiguity; do not invent facts and do not execute anything.";
    let user = format!(
        "Raw capture:\n{text}\n\nWhat must be clarified before it can become a task, note, or goal?"
    );
    let mut completion = gateway.complete(
        &policy,
        DataSensitivity::Workspace,
        Some("clarify"),
        &[ChatMessage::system(system), ChatMessage::user(user.clone())],
    )?;
    let mut parsed = parse_json_object(&completion.content);
    if parsed.is_err() {
        {
            let inner = state.inner.lock().map_err(|_| {
                ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable")
            })?;
            inner.store.append_run_step(
                &run.id,
                "model.invalid_json",
                &json!({"content": completion.content.chars().take(4000).collect::<String>()}),
            )?;
        }
        let retry = gateway.complete(
            &policy,
            DataSensitivity::Workspace,
            Some("clarify"),
            &[
                ChatMessage::system("Return ONLY one valid compact JSON object."),
                ChatMessage::user(user),
            ],
        )?;
        completion = retry;
        parsed = parse_json_object(&completion.content);
    }
    let parsed = parsed?;
    let checkpoint = json!({
        "model": completion.model,
        "provider": completion.provider_id,
        "route": completion.route,
        "questions": parsed.get("questions").cloned().unwrap_or_else(|| json!([])),
        "assumptions": parsed.get("assumptions").cloned().unwrap_or_else(|| json!([])),
        "suggested_kind": parsed.get("suggested_kind").cloned().unwrap_or_else(|| json!("keep")),
    });
    {
        let inner = state
            .inner
            .lock()
            .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"))?;
        inner.store.checkpoint_run(&run.id, &checkpoint)?;
        inner
            .store
            .append_run_step(&run.id, "model.completed", &checkpoint)?;
    }
    if is_cancelled(state, &run.id)? {
        mark_cancelled(state, &run.id)?;
        return Ok(());
    }
    {
        let inner = state
            .inner
            .lock()
            .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"))?;
        let output = json!({
            "status": "clarification_ready",
            "questions": checkpoint.get("questions").cloned().unwrap_or_else(|| json!([])),
            "assumptions": checkpoint.get("assumptions").cloned().unwrap_or_else(|| json!([])),
            "suggested_kind": checkpoint.get("suggested_kind").cloned().unwrap_or_else(|| json!("keep")),
            "model": checkpoint.get("model"),
            "provider": checkpoint.get("provider"),
            "route": checkpoint.get("route"),
            "facts_written": false,
        });
        inner
            .store
            .update_run_state(&run.id, "succeeded", Some(&output), None)?;
        inner
            .store
            .append_run_step(&run.id, "clarification.ready", &output)?;
    }
    Ok(())
}

fn process_run(state: &AppState, data_dir: &std::path::Path, run: &RunRecord) -> Result<()> {
    if run.intent == "capture.clarify" {
        return process_clarify(state, data_dir, run);
    }
    let (goal, project, policy) = {
        let mut inner = state
            .inner
            .lock()
            .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"))?;
        if run.cancel_requested {
            inner
                .store
                .update_run_state(&run.id, "cancelled", None, None)?;
            return Ok(());
        }
        let world = inner
            .store
            .load_world(&run.workspace_id)?
            .ok_or_else(|| ZodoError::not_found("workspace", &run.workspace_id))?;
        let goal_id = run
            .input
            .get("goal_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let goal = world
            .goal(goal_id)
            .cloned()
            .ok_or_else(|| ZodoError::not_found("goal", goal_id))?;
        let project_id = run.input.get("project_id").and_then(Value::as_str);
        let project = project_id.and_then(|id| world.project(id).cloned());
        inner.store.append_run_step(
            &run.id,
            "context.loaded",
            &json!({"goal_id": goal.id, "project_id": project_id, "revision": world.workspace.revision}),
        )?;
        (goal, project, world.workspace.policy.clone())
    };

    if run.cancel_requested {
        mark_cancelled(state, &run.id)?;
        return Ok(());
    }

    let run_input = run.input.clone();
    let plan_payload = if let Some(checkpoint) = &run.checkpoint {
        checkpoint.clone()
    } else {
        let gateway = ModelGateway::load(data_dir);
        let _route = gateway.route_for(&policy, DataSensitivity::Workspace, Some("decompose"))?;
        let system = "You are the planning component of Zodo, a personal operations system. \
Return ONLY a JSON object. Do not include prose. The object must have this shape: \
{\"tasks\":[{\"title\":\"...\",\"estimate_minutes\":30,\"priority\":2,\"target_due\":\"YYYY-MM-DD or null\",\"depends_on\":[0]}],\"assumptions\":[\"...\"],\"questions\":[\"...\"]}. \
Use depends_on only for 0-based indexes of tasks in this same array that must finish first; leave it [] when independent. \
Produce 3 to 7 concrete tasks, each independently executable, with estimates between 15 and 240 minutes. \
Do not invent facts about the user. Put uncertainty into questions.";
        let mut user = format!(
            "Goal: {}\nSuccess criteria: {}\nTarget due: {}\n",
            goal.title,
            goal.success_criteria.as_deref().unwrap_or("(none)"),
            goal.target_due
                .map(|date| date.to_string())
                .unwrap_or_else(|| "(none)".to_string())
        );
        if let Some(project) = &project {
            user.push_str(&format!(
                "Existing project: {}\nProject scope: {}\n",
                project.title,
                project.scope.as_deref().unwrap_or("(none)")
            ));
        }
        if let Some(horizon) = run_input.get("horizon_end").and_then(Value::as_str) {
            user.push_str(&format!("Horizon end: {horizon}\n"));
        }
        let completion = gateway.complete(
            &policy,
            DataSensitivity::Workspace,
            Some("decompose"),
            &[ChatMessage::system(system), ChatMessage::user(user.clone())],
        )?;
        let parsed = match parse_json_object(&completion.content) {
            Ok(value) => value,
            Err(first_error) => {
                {
                    let inner = state.inner.lock().map_err(|_| {
                        ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable")
                    })?;
                    inner.store.append_run_step(
                        &run.id,
                        "model.invalid_json",
                        &json!({
                            "error": first_error.message,
                            "content": completion.content.chars().take(4000).collect::<String>(),
                        }),
                    )?;
                }
                let retry_system = "Return ONLY one compact valid JSON object. No markdown, no prose, no code fence. Escape all quotes and newlines inside strings. The schema is: {\"tasks\":[{\"title\":string,\"estimate_minutes\":integer,\"priority\":integer,\"target_due\":string|null,\"depends_on\":[integer]}],\"assumptions\":[string],\"questions\":[string]}";
                let retry = gateway.complete(
                    &policy,
                    DataSensitivity::Workspace,
                    Some("decompose"),
                    &[ChatMessage::system(retry_system), ChatMessage::user(user)],
                )?;
                parse_json_object(&retry.content).map_err(|second| {
                    ZodoError::validation(format!(
                        "model did not return valid JSON: {} (retry: {})",
                        first_error.message, second.message
                    ))
                })?
            }
        };
        let tasks = parsed
            .get("tasks")
            .and_then(Value::as_array)
            .ok_or_else(|| ZodoError::validation("model response has no tasks array"))?;
        if tasks.is_empty() || tasks.len() > 12 {
            return Err(ZodoError::validation("model must return 1..12 tasks"));
        }
        let checkpoint = json!({
            "model": completion.model,
            "provider": completion.provider_id,
            "route": completion.route,
            "elapsed_ms": completion.elapsed_ms,
            "tasks": tasks,
            "assumptions": parsed.get("assumptions").cloned().unwrap_or_else(|| json!([])),
            "questions": parsed.get("questions").cloned().unwrap_or_else(|| json!([])),
        });
        {
            let inner = state.inner.lock().map_err(|_| {
                ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable")
            })?;
            inner.store.checkpoint_run(&run.id, &checkpoint)?;
            inner
                .store
                .append_run_step(&run.id, "model.completed", &checkpoint)?;
        }
        checkpoint
    };

    if is_cancelled(state, &run.id)? {
        mark_cancelled(state, &run.id)?;
        return Ok(());
    }

    let tasks = plan_payload
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| ZodoError::validation("checkpoint has no tasks array"))?;
    let mut operations = Vec::new();
    let mut dependency_specs: Vec<(usize, Vec<usize>)> = Vec::new();
    for (index, raw) in tasks.iter().enumerate() {
        let title = raw
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .chars()
            .take(200)
            .collect::<String>();
        if title.is_empty() {
            continue;
        }
        let estimate = raw
            .get("estimate_minutes")
            .and_then(Value::as_u64)
            .unwrap_or(30)
            .clamp(15, 240) as u32;
        let priority = raw
            .get("priority")
            .and_then(Value::as_i64)
            .unwrap_or(2)
            .clamp(0, 4) as i32;
        let target_due = raw
            .get("target_due")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<chrono::NaiveDate>().ok())
            .or(goal.target_due);
        let tags = if let Some(project) = &project {
            vec!["ai".to_string(), format!("project:{}", project.id)]
        } else {
            vec!["ai".to_string(), format!("goal:{}", goal.id)]
        };
        let client_ref = format!("task-{index}");
        if let Some(deps) = raw.get("depends_on").and_then(Value::as_array) {
            let indexes = deps
                .iter()
                .filter_map(Value::as_u64)
                .map(|value| value as usize)
                .filter(|value| *value != index && *value < tasks.len())
                .collect::<Vec<_>>();
            if !indexes.is_empty() {
                dependency_specs.push((index, indexes));
            }
        }
        operations.push(ChangeOp::TaskCreate {
            task: TaskSpec {
                client_ref: Some(client_ref),
                title,
                description: Some(format!(
                    "由本地/授权模型拆解目标《{}》生成，待用户审阅。",
                    goal.title
                )),
                project_id: project.as_ref().map(|project| project.id.clone()),
                parent_id: None,
                estimate_minutes: Some(estimate),
                priority,
                target_due,
                hard_deadline: None,
                not_before: None,
                preferred_window: None,
                tags,
                due: None,
            },
        });
        if index > 20 {
            break;
        }
    }
    // Only task creations count as tasks; dependency operations are added below.
    let task_count = operations.len();
    for (successor_index, predecessors) in dependency_specs {
        for predecessor_index in predecessors {
            operations.push(ChangeOp::DependencyAddRef {
                predecessor_ref: format!("task-{predecessor_index}"),
                successor_ref: format!("task-{successor_index}"),
            });
        }
    }
    if operations.is_empty() {
        return Err(ZodoError::validation(
            "model produced no usable task titles",
        ));
    }
    let assumptions = plan_payload
        .get("assumptions")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .take(8)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let questions = plan_payload
        .get("questions")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .take(8)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let intent = format!(
        "AI 拆解目标「{}」为 {} 个任务（待审阅，不自动提交）",
        goal.title, task_count
    );
    let operations: Vec<Value> = operations
        .iter()
        .map(|operation| serde_json::to_value(operation).unwrap_or(Value::Null))
        .collect();
    let envelope = CommandEnvelope {
        schema: "zodo.command/1".to_string(),
        request_id: format!("run-{}-{}", run.id, Utc::now().timestamp_millis()),
        idempotency_key: format!("run:{}:proposal", run.id),
        workspace_id: run.workspace_id.clone(),
        operation: "proposal.create".to_string(),
        operation_version: 1,
        base_revision: None,
        policy_revision: None,
        payload: json!({
            "intent": intent,
            "operations": operations,
            "reads": [{"kind": "goal", "id": goal.id}],
            "assumptions": assumptions,
            "questions": questions,
            "ttl_seconds": 86_400,
        }),
    };
    let (result, revision) = {
        let mut inner = state
            .inner
            .lock()
            .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"))?;
        let instance_id = inner.instance_id.clone();
        let context = CommandContext {
            principal_id: "ai_planner".to_string(),
            principal_kind: PrincipalKind::DelegatedAgent,
            channel: "ai".to_string(),
            capabilities: vec!["read".to_string(), "propose".to_string()],
            workspace_id: run.workspace_id.clone(),
            request_id: envelope.request_id.clone(),
            received_at: Utc::now(),
            runtime_instance: instance_id.clone(),
        };
        let revision = inner
            .store
            .load_world(&run.workspace_id)?
            .map(|world| world.workspace.revision)
            .unwrap_or(0);
        let mut app = Application {
            store: &mut inner.store,
            clock: &SystemClock,
            ids: &UuidIds::default(),
            instance_id,
        };
        let result = app.execute(&context, envelope)?;
        (result, revision)
    };
    let proposal_id = result
        .data
        .get("proposal")
        .and_then(|proposal| proposal.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let output = json!({
        "proposal_id": proposal_id,
        "task_count": task_count,
        "model": plan_payload.get("model"),
        "provider": plan_payload.get("provider"),
        "route": plan_payload.get("route"),
        "base_revision": revision,
        "status": "proposal_created",
    });
    {
        let inner = state
            .inner
            .lock()
            .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"))?;
        inner
            .store
            .append_run_step(&run.id, "proposal.created", &output)?;
        inner
            .store
            .update_run_state(&run.id, "succeeded", Some(&output), None)?;
    }
    info!("AI run {} created proposal {}", run.id, proposal_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::RuntimeInner;
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::{Arc, Mutex};
    use zodo_kernel::model::{Proposal, Workspace, World};

    // Resume a checkpoint directly: no worker, HTTP listener, model call, or notification is started.
    fn checkpoint_proposal(tasks: Value) -> (Value, Proposal) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SqliteStore::open(dir.path().join("run-test.sqlite3")).unwrap();
        let now = Utc::now();
        let mut world = World::new(Workspace::new(
            "ws_test".to_string(),
            "Test".to_string(),
            now,
        ));
        world.goals.push(
            serde_json::from_value(json!({
                "id": "goal_test", "workspace_id": "ws_test", "title": "Test goal",
                "status": "active", "created_at": now, "updated_at": now, "version": 1
            }))
            .unwrap(),
        );
        store.create_workspace(&world).unwrap();
        let run = store
            .create_run(
                "ws_test",
                "goal.decompose",
                &json!({"goal_id": "goal_test"}),
            )
            .unwrap();
        store
            .checkpoint_run(&run.id, &json!({"tasks": tasks}))
            .unwrap();
        let run = store
            .claim_next_run("ws_test", "test_worker")
            .unwrap()
            .unwrap();
        let state = AppState {
            inner: Arc::new(Mutex::new(RuntimeInner {
                store,
                instance_id: "rt_test".to_string(),
                workspace_id: "ws_test".to_string(),
                started_at: now,
                tokens: BTreeMap::new(),
                revoked_channels: BTreeSet::new(),
                revoked_devices: BTreeSet::new(),
                data_dir: dir.path().to_path_buf(),
                _lock: std::fs::File::create(dir.path().join("test.lock")).unwrap(),
                _lock_path: dir.path().join("test.lock"),
                _info_path: dir.path().join("runtime.json"),
            })),
        };
        process_run(&state, dir.path(), &run).unwrap();
        let mut inner = state.inner.lock().unwrap();
        let completed = inner.store.get_run(&run.id).unwrap().unwrap();
        assert_eq!(completed.state, "succeeded");
        let output = completed.output.unwrap();
        let world = inner.store.load_world("ws_test").unwrap().unwrap();
        assert!(
            world.tasks.is_empty(),
            "decomposition only proposes; it must never commit tasks"
        );
        assert_eq!(world.proposals.len(), 1);
        let proposal = world.proposals.into_iter().next().unwrap();
        assert_eq!(output["proposal_id"], proposal.id);
        (output, proposal)
    }

    #[test]
    fn decomposition_task_count_excludes_dependency_operations() {
        let (output, proposal) = checkpoint_proposal(json!([
            {"title": "First"},
            {"title": "Second", "depends_on": [0]},
            {"title": "Third", "depends_on": [1]}
        ]));
        assert_eq!(proposal.operations.as_array().unwrap().len(), 5);
        assert_eq!(output["task_count"], 3);
        assert!(
            proposal.intent.contains("为 3 个任务"),
            "{}",
            proposal.intent
        );
    }

    #[test]
    fn decomposition_task_count_only_includes_usable_titles() {
        let (output, proposal) = checkpoint_proposal(json!([
            {"title": "First"}, {"title": " \t"}, {}, {"title": "Last"}
        ]));
        assert_eq!(proposal.operations.as_array().unwrap().len(), 2);
        assert_eq!(output["task_count"], 2);
        assert!(
            proposal.intent.contains("为 2 个任务"),
            "{}",
            proposal.intent
        );
    }
}

fn is_cancelled(state: &AppState, run_id: &str) -> Result<bool> {
    let inner = state
        .inner
        .lock()
        .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"))?;
    Ok(inner
        .store
        .get_run(run_id)?
        .is_some_and(|run| run.cancel_requested))
}

fn mark_cancelled(state: &AppState, run_id: &str) -> Result<()> {
    let inner = state
        .inner
        .lock()
        .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"))?;
    inner.store.append_run_step(
        run_id,
        "cancelled",
        &json!({"reason": "cancellation requested before proposal commit"}),
    )?;
    inner
        .store
        .update_run_state(run_id, "cancelled", None, None)
}
