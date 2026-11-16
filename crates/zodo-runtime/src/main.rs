use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use clap::{Parser, Subcommand};
use fs2::FileExt;
use serde_json::{Value, json};
use zodo_kernel::error::{ErrorCode, Result, ZodoError};

use zodo_runtime::runtime::{RuntimeInfo, ServeConfig, default_data_dir, read_runtime_info, serve};

#[derive(Debug, Clone, Parser)]
#[command(
    name = "zodo",
    version,
    about = "Zodo independent personal-operations kernel"
)]
struct Cli {
    /// Data directory holding SQLite, runtime lock and the scoped client tokens.
    #[arg(long, global = true, default_value_os_t = default_data_dir())]
    data_dir: PathBuf,

    /// Which scoped token to use (cli, ui, agent, mcp).
    #[arg(long, global = true, default_value = "cli")]
    token_kind: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Subcommand)]
enum Command {
    /// Start the authoritative single-writer runtime.
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 0)]
        port: u16,
    },
    /// Show the running runtime and workspace summary.
    Status,
    /// Send one typed command envelope. Payload defaults to {}.
    Command {
        operation: String,
        #[arg(long, default_value = "{}")]
        payload: String,
        #[arg(long)]
        payload_file: Option<PathBuf>,
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        request_id: Option<String>,
        #[arg(long)]
        base_revision: Option<u64>,
        #[arg(long)]
        policy_revision: Option<u64>,
    },
    /// Send one read-only query envelope.
    Query {
        query: String,
        #[arg(long, default_value = "{}")]
        payload: String,
        #[arg(long)]
        payload_file: Option<PathBuf>,
    },
    /// Collect raw text into the Inbox.
    Capture { text: String },
    /// Create a manually managed task.
    TaskAdd {
        title: String,
        #[arg(long)]
        estimate: Option<u32>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long, default_value_t = 2)]
        priority: i32,
        #[arg(long)]
        target_due: Option<NaiveDate>,
        #[arg(long)]
        hard_deadline: Option<String>,
    },
    /// List tasks.
    TaskList {
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        project: Option<String>,
        #[arg(long)]
        text: Option<String>,
    },
    /// Complete a task explicitly.
    TaskComplete { id: String },
    /// Create a deterministic planning proposal.
    Plan {
        #[arg(long)]
        start: Option<String>,
        #[arg(long)]
        end: Option<String>,
        #[arg(long, value_delimiter = ',')]
        tasks: Vec<String>,
    },
    /// Validate and preview a proposal again.
    Validate { proposal: String },
    /// Approve a validated proposal with the human CLI face.
    Approve { proposal: String },
    /// Commit a proposal using its current approval grant.
    Commit { proposal: String, grant: String },
    /// Record execution feedback for a task.
    Progress {
        task: String,
        #[arg(long)]
        minutes: u32,
        #[arg(long)]
        note: Option<String>,
    },
    /// Start a focus session.
    FocusStart {
        #[arg(long)]
        task: Option<String>,
    },
    /// Finish the active focus session without completing the task.
    FocusFinish,
    /// Build a fact-based review for a date range.
    Review { start: NaiveDate, end: NaiveDate },
    /// Search permitted manual facts.
    Search { text: String },
    /// Start the Model Context Protocol stdio server.
    Mcp,
    /// Start a durable AI goal decomposition run.
    AiDecompose {
        goal: String,
        #[arg(long)]
        horizon: Option<String>,
    },
    /// List durable AI runs.
    RunList,
    /// Read a durable AI run.
    RunGet { id: String },
    /// Request cancellation of a durable AI run.
    RunCancel { id: String },
    /// Read the sanitized Core model configuration.
    ModelInfo,
    /// Restore a preflight-validated SQLite backup while the runtime is stopped.
    Restore { backup: PathBuf },
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("zodo_runtime=info")),
        )
        .with_target(false)
        .init();
    let cli = Cli::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    let result = runtime.block_on(run(cli));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!(
                "{}",
                serde_json::to_string(&json!({
                    "schema": "zodo.error/1",
                    "error": {
                        "code": error.code,
                        "message": error.message,
                        "retryable": error.retryable,
                        "details": error.details,
                    }
                }))
                .unwrap_or_else(|_| error.message.clone())
            );
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    let selected_command = cli.command.clone();
    match selected_command {
        Command::Serve { host, port } => {
            serve(ServeConfig {
                data_dir: cli.data_dir,
                host,
                port,
            })
            .await?;
            Ok(())
        }
        Command::Status => {
            let info = read_runtime_info(&cli.data_dir)?;
            let health = get_json(&info, "/v1/health", None)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "runtime": {
                        "instance_id": info.instance_id,
                        "pid": info.pid,
                        "url": format!("http://{}:{}", info.host, info.port),
                        "workspace_id": info.workspace_id,
                        "data_dir": info.data_dir,
                    },
                    "health": health,
                }))
                .map_err(serialize_error)?
            );
            Ok(())
        }
        Command::Command {
            operation,
            payload,
            payload_file,
            key,
            request_id,
            base_revision,
            policy_revision,
        } => {
            let info = read_runtime_info(&cli.data_dir)?;
            let token = token(&info, &cli.token_kind)?;
            let payload = read_payload(payload, payload_file)?;
            let request_id =
                request_id.unwrap_or_else(|| format!("req_{}", uuid::Uuid::new_v4().simple()));
            let body = json!({
                "schema": "zodo.command/1",
                "request_id": request_id,
                "idempotency_key": key.unwrap_or_else(|| format!("op_{}", uuid::Uuid::new_v4().simple())),
                "workspace_id": info.workspace_id,
                "operation": operation,
                "operation_version": 1,
                "base_revision": base_revision,
                "policy_revision": policy_revision,
                "payload": payload,
            });
            let response = post_json(&info, "/v1/commands", Some(token), &body)?;
            print_result(response)
        }
        Command::Query {
            query,
            payload,
            payload_file,
        } => {
            let info = read_runtime_info(&cli.data_dir)?;
            let token = token(&info, &cli.token_kind)?;
            let payload = read_payload(payload, payload_file)?;
            let body = json!({
                "schema": "zodo.query/1",
                "request_id": format!("req_{}", uuid::Uuid::new_v4().simple()),
                "workspace_id": info.workspace_id,
                "query": query,
                "payload": payload,
            });
            let response = post_json(&info, "/v1/queries", Some(token), &body)?;
            print_result(response)
        }
        Command::Capture { text } => command(&cli, "capture.collect", json!({"text": text})),
        Command::TaskAdd {
            title,
            estimate,
            project,
            parent,
            priority,
            target_due,
            hard_deadline,
        } => {
            let hard_deadline = hard_deadline
                .map(|value| {
                    value.parse::<DateTime<Utc>>().map_err(|error| {
                        ZodoError::validation(format!("hard_deadline must be RFC3339: {error}"))
                    })
                })
                .transpose()?
                .map(|value| value.to_rfc3339());
            command(
                &cli,
                "task.create",
                json!({
                    "task": {
                        "title": title,
                        "estimate_minutes": estimate,
                        "project_id": project,
                        "parent_id": parent,
                        "priority": priority,
                        "target_due": target_due.map(|date| date.format("%Y-%m-%d").to_string()),
                        "hard_deadline": hard_deadline,
                    }
                }),
            )
        }
        Command::TaskList {
            status,
            project,
            text,
        } => {
            let payload = json!({
                "filter": {
                    "status": status,
                    "project_id": project,
                    "text": text,
                    "limit": 200,
                }
            });
            query(&cli, "tasks.list", payload)
        }
        Command::TaskComplete { id } => command(&cli, "task.complete", json!({"id": id})),
        Command::Plan { start, end, tasks } => {
            let start = start
                .map(|value| parse_datetime(&value))
                .transpose()?
                .unwrap_or_else(Utc::now);
            let end = end
                .map(|value| parse_datetime(&value))
                .transpose()?
                .unwrap_or_else(|| start + Duration::days(7));
            command(
                &cli,
                "plan.propose",
                json!({
                    "task_ids": tasks,
                    "horizon_start": start,
                    "horizon_end": end,
                    "move_unlocked": true,
                }),
            )
        }
        Command::Validate { proposal } => {
            command(&cli, "proposal.validate", json!({"proposal_id": proposal}))
        }
        Command::Approve { proposal } => {
            command(&cli, "proposal.approve", json!({"proposal_id": proposal}))
        }
        Command::Commit { proposal, grant } => command(
            &cli,
            "changes.commit",
            json!({"proposal_id": proposal, "approval_grant": grant}),
        ),
        Command::Progress {
            task,
            minutes,
            note,
        } => command(
            &cli,
            "progress.record",
            json!({"task_id": task, "minutes": minutes, "note": note}),
        ),
        Command::FocusStart { task } => command(&cli, "focus.start", json!({"task_id": task})),
        Command::FocusFinish => command(&cli, "focus.finish", json!({})),
        Command::Review { start, end } => command(
            &cli,
            "review.build",
            json!({"period_start": start, "period_end": end}),
        ),
        Command::Search { text } => query(&cli, "search.query", json!({"text": text, "limit": 50})),
        Command::Mcp => {
            zodo_runtime::mcp::run_mcp(&cli.data_dir)?;
            Ok(())
        }
        Command::AiDecompose { goal, horizon } => {
            let info = read_runtime_info(&cli.data_dir)?;
            let token = token(&info, &cli.token_kind)?;
            let body = json!({
                "intent": "goal.decompose",
                "input": {
                    "goal_id": goal,
                    "horizon_end": horizon.unwrap_or_else(|| (Utc::now() + Duration::days(30)).to_rfc3339()),
                },
                "idempotency_key": format!("run_{}", uuid::Uuid::new_v4().simple()),
            });
            print_result(post_json(&info, "/v1/runs", Some(token), &body)?)
        }
        Command::RunList => {
            let info = read_runtime_info(&cli.data_dir)?;
            let token = token(&info, &cli.token_kind)?;
            let value = get_json(&info, "/v1/runs", Some(token))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&value).map_err(serialize_error)?
            );
            Ok(())
        }
        Command::RunGet { id } => {
            let info = read_runtime_info(&cli.data_dir)?;
            let token = token(&info, &cli.token_kind)?;
            let value = get_json(&info, &format!("/v1/runs/{id}"), Some(token))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&value).map_err(serialize_error)?
            );
            Ok(())
        }
        Command::RunCancel { id } => {
            let info = read_runtime_info(&cli.data_dir)?;
            let token = token(&info, &cli.token_kind)?;
            print_result(post_json(
                &info,
                &format!("/v1/runs/{id}/cancel"),
                Some(token),
                &json!({}),
            )?)
        }
        Command::ModelInfo => {
            let info = read_runtime_info(&cli.data_dir)?;
            let token = token(&info, &cli.token_kind)?;
            let value = get_json(&info, "/v1/model", Some(token))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&value).map_err(serialize_error)?
            );
            Ok(())
        }
        Command::Restore { backup } => restore_backup(&cli.data_dir, &backup),
    }
}

fn command(cli: &Cli, operation: &str, payload: Value) -> Result<()> {
    let info = read_runtime_info(&cli.data_dir)?;
    let token = token(&info, &cli.token_kind)?;
    let body = json!({
        "schema": "zodo.command/1",
        "request_id": format!("req_{}", uuid::Uuid::new_v4().simple()),
        "idempotency_key": format!("op_{}", uuid::Uuid::new_v4().simple()),
        "workspace_id": info.workspace_id,
        "operation": operation,
        "operation_version": 1,
        "payload": payload,
    });
    print_result(post_json(&info, "/v1/commands", Some(token), &body)?)
}

fn query(cli: &Cli, name: &str, payload: Value) -> Result<()> {
    let info = read_runtime_info(&cli.data_dir)?;
    let token = token(&info, &cli.token_kind)?;
    let body = json!({
        "schema": "zodo.query/1",
        "request_id": format!("req_{}", uuid::Uuid::new_v4().simple()),
        "workspace_id": info.workspace_id,
        "query": name,
        "payload": payload,
    });
    print_result(post_json(&info, "/v1/queries", Some(token), &body)?)
}

fn parse_datetime(value: &str) -> Result<DateTime<Utc>> {
    value
        .parse::<DateTime<Utc>>()
        .map_err(|error| ZodoError::validation(format!("RFC3339 datetime required: {error}")))
}

fn read_payload(default: String, file: Option<PathBuf>) -> Result<Value> {
    let text = match file {
        Some(file) => std::fs::read_to_string(&file)
            .map_err(|error| ZodoError::validation(format!("read payload file: {error}")))?
            .trim_start_matches('\u{feff}')
            .to_string(),
        None => default,
    };
    serde_json::from_str(&text)
        .map_err(|error| ZodoError::validation(format!("payload must be valid JSON: {error}")))
}

fn token<'a>(info: &'a RuntimeInfo, kind: &str) -> Result<&'a str> {
    info.tokens.get(kind).map(String::as_str).ok_or_else(|| {
        ZodoError::new(
            ErrorCode::PermissionDenied,
            format!("unknown token kind {kind}"),
        )
    })
}

fn print_result((status, body): (u16, String)) -> Result<()> {
    if status >= 200 && status < 300 {
        if let Ok(value) = serde_json::from_str::<Value>(&body) {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).map_err(serialize_error)?
            );
        } else {
            println!("{body}");
        }
        return Ok(());
    }
    let value = serde_json::from_str::<Value>(&body)
        .unwrap_or_else(|_| json!({"status": status, "body": body}));
    if let Some(error) = value.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_str)
            .and_then(|code| serde_json::from_value::<ErrorCode>(json!(code)).ok())
            .unwrap_or(ErrorCode::StorageFailure);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("runtime request failed")
            .to_string();
        let retryable = error
            .get("retryable")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut result = ZodoError::new(code, message).with_details(value);
        result.retryable = retryable;
        return Err(result);
    }
    Err(ZodoError::new(
        ErrorCode::StorageFailure,
        format!("runtime returned HTTP {status}: {value}"),
    )
    .with_details(value))
}

fn serialize_error(error: serde_json::Error) -> ZodoError {
    ZodoError::new(ErrorCode::StorageFailure, error.to_string())
}

fn get_json(info: &RuntimeInfo, path: &str, token: Option<&str>) -> Result<Value> {
    let (status, body) = request(info, "GET", path, token, None)?;
    if status >= 200 && status < 300 {
        serde_json::from_str(&body).map_err(|error| {
            ZodoError::new(
                ErrorCode::StorageFailure,
                format!("decode response: {error}"),
            )
        })
    } else {
        Err(ZodoError::new(
            ErrorCode::StorageFailure,
            format!("runtime returned HTTP {status}: {body}"),
        ))
    }
}

fn post_json(
    info: &RuntimeInfo,
    path: &str,
    token: Option<&str>,
    body: &Value,
) -> Result<(u16, String)> {
    let body = serde_json::to_vec(body).map_err(serialize_error)?;
    request(info, "POST", path, token, Some(&body))
}

fn request(
    info: &RuntimeInfo,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&[u8]>,
) -> Result<(u16, String)> {
    let mut stream = TcpStream::connect((info.host.as_str(), info.port)).map_err(|error| {
        ZodoError::new(
            ErrorCode::ProviderUnavailable,
            format!("connect to runtime at {}:{}: {error}", info.host, info.port),
        )
    })?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|error| ZodoError::new(ErrorCode::StorageFailure, error.to_string()))?;
    let body = body.unwrap_or_default();
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: application/json\r\n",
        info.host,
        info.port,
        body.len()
    );
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .and_then(|()| stream.write_all(body))
        .map_err(|error| {
            ZodoError::new(
                ErrorCode::ProviderUnavailable,
                format!("write request: {error}"),
            )
        })?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|error| {
        ZodoError::new(
            ErrorCode::ProviderUnavailable,
            format!("read response: {error}"),
        )
    })?;
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| ZodoError::new(ErrorCode::StorageFailure, "malformed HTTP response"))?;
    let header = std::str::from_utf8(&raw[..split])
        .map_err(|error| ZodoError::new(ErrorCode::StorageFailure, error.to_string()))?;
    let status = header
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| ZodoError::new(ErrorCode::StorageFailure, "malformed HTTP status"))?;
    let body = String::from_utf8_lossy(&raw[split + 4..]).to_string();
    Ok((status, body))
}

// Keep the JSON map type available in debug builds for future CLI aliases.
#[allow(dead_code)]
type JsonMap = BTreeMap<String, Value>;

fn restore_backup(data_dir: &Path, backup: &Path) -> Result<()> {
    if !backup.is_file() {
        return Err(ZodoError::validation(format!(
            "backup file does not exist: {}",
            backup.display()
        )));
    }
    std::fs::create_dir_all(data_dir)
        .map_err(|error| ZodoError::new(ErrorCode::StorageFailure, error.to_string()))?;
    let lock_path = data_dir.join("runtime.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| ZodoError::new(ErrorCode::StorageFailure, error.to_string()))?;
    lock.try_lock_exclusive().map_err(|_| {
        ZodoError::validation("the Zodo runtime is running; stop it before restoring a backup")
    })?;
    let db_path = data_dir.join("zodo.sqlite3");
    let backup_path = backup
        .canonicalize()
        .map_err(|error| ZodoError::new(ErrorCode::StorageFailure, error.to_string()))?;
    if backup_path == db_path.canonicalize().unwrap_or_else(|_| db_path.clone()) {
        return Err(ZodoError::validation(
            "the backup path is the live database itself",
        ));
    }
    let source = rusqlite::Connection::open_with_flags(
        &backup_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|error| ZodoError::validation(format!("cannot open backup: {error}")))?;
    let integrity: String = source
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|error| {
            ZodoError::validation(format!("backup integrity check failed: {error}"))
        })?;
    if integrity != "ok" {
        return Err(ZodoError::validation(format!(
            "backup integrity check returned {integrity}"
        )));
    }
    let workspace_count: i64 = source
        .query_row(
            "SELECT COUNT(*) FROM workspaces WHERE id = 'ws_local'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| ZodoError::validation(format!("backup compatibility failed: {error}")))?;
    if workspace_count != 1 {
        return Err(ZodoError::validation(
            "backup does not contain the expected local workspace",
        ));
    }
    drop(source);

    let backup_dir = data_dir.join("backups");
    std::fs::create_dir_all(&backup_dir)
        .map_err(|error| ZodoError::new(ErrorCode::StorageFailure, error.to_string()))?;
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let mut pre_restore = backup_dir.join(format!("pre-restore-{stamp}.sqlite3"));
    let mut sequence = 1;
    while pre_restore.exists() {
        pre_restore = backup_dir.join(format!("pre-restore-{stamp}-{sequence}.sqlite3"));
        sequence += 1;
    }
    if db_path.exists() {
        let live = rusqlite::Connection::open(&db_path)
            .map_err(|error| ZodoError::new(ErrorCode::StorageFailure, error.to_string()))?;
        live.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|error| ZodoError::new(ErrorCode::StorageFailure, error.to_string()))?;
        live.execute(
            "VACUUM INTO ?1",
            rusqlite::params![pre_restore.to_string_lossy().to_string()],
        )
        .map_err(|error| {
            ZodoError::new(
                ErrorCode::StorageFailure,
                format!("create pre-restore snapshot: {error}"),
            )
        })?;
        drop(live);
    }
    let temporary = data_dir.join("restore.tmp.sqlite3");
    let _ = std::fs::remove_file(&temporary);
    std::fs::copy(&backup_path, &temporary)
        .map_err(|error| ZodoError::new(ErrorCode::StorageFailure, error.to_string()))?;
    for suffix in ["", "-wal", "-shm"] {
        let path = PathBuf::from(format!("{}{suffix}", db_path.display()));
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&temporary, &db_path)
        .map_err(|error| ZodoError::new(ErrorCode::StorageFailure, error.to_string()))?;
    // Credentials and ephemeral runtime state must never be resurrected by a backup.
    let _ = std::fs::remove_file(data_dir.join("runtime.json"));
    let _ = std::fs::remove_file(data_dir.join("auth.json"));
    let restored = rusqlite::Connection::open(&db_path)
        .map_err(|error| ZodoError::new(ErrorCode::StorageFailure, error.to_string()))?;
    let tasks: i64 = restored
        .query_row(
            "SELECT COUNT(*) FROM entities WHERE kind = 'task'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    drop(restored);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "zodo.restore/1",
            "backup": backup_path.display().to_string(),
            "pre_restore_snapshot": if pre_restore.exists() { Some(pre_restore.display().to_string()) } else { None },
            "restored_tasks": tasks,
            "auth_reset": true,
            "runtime_restart_required": true,
        }))
        .map_err(serialize_error)?
    );
    Ok(())
}
