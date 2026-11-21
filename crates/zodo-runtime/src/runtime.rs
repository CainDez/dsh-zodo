use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::extract::{Path as AxumPath, Query as AxumQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tracing::{info, warn};
use zodo_kernel::app::Application;
use zodo_kernel::commands::{CommandEnvelope, ErrorEnvelope, QueryEnvelope, QueryResult};
use zodo_kernel::error::{ErrorCode, Result, ZodoError};
use zodo_kernel::model::{CommandContext, PrincipalKind, World};
use zodo_kernel::ports::{NewAudit, NewEvent, SaveOptions, Store, SystemClock, UuidIds};

use crate::ai::{ModelConfig, ModelGateway};
use crate::runs::{RunRecord, validate_run_intent};
use crate::store::SqliteStore;

pub const PROTOCOL: &str = "zodo/1";
pub const WORKSPACE_ID: &str = "ws_local";
pub const DEFAULT_WORKSPACE_NAME: &str = "鎴戠殑浜嬪姟";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInfo {
    pub schema: String,
    pub protocol: String,
    pub runtime_version: String,
    pub instance_id: String,
    pub pid: u32,
    pub host: String,
    pub port: u16,
    pub data_dir: String,
    pub db_path: String,
    pub workspace_id: String,
    pub started_at: DateTime<Utc>,
    pub tokens: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ServeConfig {
    pub data_dir: PathBuf,
    pub host: String,
    pub port: u16,
}

#[derive(Clone)]
pub struct AuthPrincipal {
    pub token: String,
    pub principal_id: String,
    pub kind: PrincipalKind,
    pub channel: String,
    pub capabilities: Vec<String>,
    pub workspace_id: String,
}

pub struct RuntimeInner {
    pub store: SqliteStore,
    pub instance_id: String,
    pub workspace_id: String,
    pub started_at: DateTime<Utc>,
    pub tokens: BTreeMap<String, AuthPrincipal>,
    pub revoked_channels: BTreeSet<String>,
    pub revoked_devices: BTreeSet<String>,
    pub data_dir: PathBuf,
    pub host: crate::host::HostState,
    pub _lock: File,
    pub _lock_path: PathBuf,
    pub _info_path: PathBuf,
}

#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<Mutex<RuntimeInner>>,
}

pub fn default_data_dir() -> PathBuf {
    if let Ok(value) = std::env::var("ZODO_DATA_DIR") {
        return PathBuf::from(value);
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("zodo")
}

pub fn runtime_info_path(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime.json")
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthState {
    #[serde(default)]
    revoked_channels: BTreeSet<String>,
    #[serde(default)]
    revoked_devices: BTreeSet<String>,
}

fn auth_state_path(data_dir: &Path) -> PathBuf {
    data_dir.join("auth.json")
}

fn read_auth_state(data_dir: &Path) -> AuthState {
    std::fs::read_to_string(auth_state_path(data_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_auth_state(data_dir: &Path, state: &AuthState) -> Result<()> {
    let path = auth_state_path(data_dir);
    let temporary = path.with_extension("tmp");
    std::fs::write(
        &temporary,
        serde_json::to_vec_pretty(state).map_err(|error| io_error("encode auth state", error))?,
    )
    .and_then(|_| std::fs::rename(&temporary, &path))
    .map_err(|error| io_error("write auth state", error))
}

pub fn read_runtime_info(data_dir: &Path) -> Result<RuntimeInfo> {
    let path = runtime_info_path(data_dir);
    let text = std::fs::read_to_string(&path).map_err(|error| {
        ZodoError::new(
            ErrorCode::ProviderUnavailable,
            format!(
                "Zodo runtime info was not found at {} (start `zodo serve`): {error}",
                path.display()
            ),
        )
    })?;
    serde_json::from_str(&text).map_err(|error| {
        ZodoError::new(
            ErrorCode::StorageFailure,
            format!("runtime info is malformed: {error}"),
        )
    })
}

fn io_error(context: &str, error: impl std::fmt::Display) -> ZodoError {
    ZodoError::new(ErrorCode::StorageFailure, format!("{context}: {error}"))
}

fn random_token() -> String {
    uuid::Uuid::new_v4().simple().to_string() + &uuid::Uuid::new_v4().simple().to_string()
}

fn tokens(_workspace_id: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("ui".to_string(), random_token()),
        ("cli".to_string(), random_token()),
        ("agent".to_string(), random_token()),
        ("mcp".to_string(), random_token()),
    ])
}

fn principals(
    tokens: BTreeMap<String, String>,
    workspace_id: &str,
) -> BTreeMap<String, AuthPrincipal> {
    let mut result = BTreeMap::new();
    for (kind, token) in tokens {
        let (principal_id, principal_kind, channel, capabilities) = match kind.as_str() {
            "ui" => (
                "user_ui",
                PrincipalKind::InteractiveUi,
                "harness_ui",
                vec![
                    "read",
                    "read_private",
                    "write",
                    "capture",
                    "propose",
                    "approve",
                    "execute",
                    "admin",
                ],
            ),
            "cli" => (
                "owner_cli",
                PrincipalKind::Owner,
                "cli",
                vec![
                    "read",
                    "read_private",
                    "write",
                    "capture",
                    "propose",
                    "approve",
                    "execute",
                    "admin",
                    "export",
                ],
            ),
            "agent" => (
                "agent_harness",
                PrincipalKind::DelegatedAgent,
                "harness_agent",
                vec!["read", "capture", "propose", "execute"],
            ),
            "mcp" => (
                "agent_mcp",
                PrincipalKind::McpClient,
                "mcp_stdio",
                vec!["read", "propose"],
            ),
            _ => continue,
        };
        result.insert(
            token.clone(),
            AuthPrincipal {
                token,
                principal_id: principal_id.to_string(),
                kind: principal_kind,
                channel: channel.to_string(),
                capabilities: capabilities.into_iter().map(str::to_string).collect(),
                workspace_id: workspace_id.to_string(),
            },
        );
    }
    result
}

pub async fn serve(config: ServeConfig) -> Result<()> {
    std::fs::create_dir_all(&config.data_dir)
        .map_err(|error| io_error("create data directory", error))?;
    let lock_path = config.data_dir.join("runtime.lock");
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| io_error("open runtime lock", error))?;
    lock.try_lock_exclusive().map_err(|_| {
        ZodoError::new(
            ErrorCode::ProviderUnavailable,
            format!(
                "another Zodo runtime already owns {}; use `zodo status` to reach it",
                config.data_dir.display()
            ),
        )
    })?;
    {
        let mut file = &lock;
        let _ = writeln!(
            file,
            "pid={} started_at={}",
            std::process::id(),
            Utc::now().to_rfc3339()
        );
    }

    let db_path = config.data_dir.join("zodo.sqlite3");
    let mut store = SqliteStore::open(&db_path)?;
    store.get_or_create_default(WORKSPACE_ID, DEFAULT_WORKSPACE_NAME)?;

    let instance_id = format!("rt_{}", uuid::Uuid::new_v4().simple());
    let started_at = Utc::now();
    let token_map = tokens(WORKSPACE_ID);
    let principals = principals(token_map.clone(), WORKSPACE_ID);

    let listener = TcpListener::bind((config.host.as_str(), config.port))
        .await
        .map_err(|error| io_error("bind runtime listener", error))?;
    let address = listener
        .local_addr()
        .map_err(|error| io_error("read listener address", error))?;
    let port = address.port();
    let info = RuntimeInfo {
        schema: "zodo.runtime/1".to_string(),
        protocol: PROTOCOL.to_string(),
        runtime_version: env!("CARGO_PKG_VERSION").to_string(),
        instance_id: instance_id.clone(),
        pid: std::process::id(),
        host: config.host.clone(),
        port,
        data_dir: config.data_dir.display().to_string(),
        db_path: db_path.display().to_string(),
        workspace_id: WORKSPACE_ID.to_string(),
        started_at,
        tokens: token_map,
    };
    let info_path = runtime_info_path(&config.data_dir);
    write_runtime_info(&info_path, &info)?;

    let state = AppState {
        inner: Arc::new(Mutex::new(RuntimeInner {
            store,
            instance_id,
            workspace_id: WORKSPACE_ID.to_string(),
            started_at,
            tokens: principals,
            revoked_channels: read_auth_state(&config.data_dir).revoked_channels,
            revoked_devices: read_auth_state(&config.data_dir).revoked_devices,
            data_dir: config.data_dir.clone(),
            host: crate::host::HostState::default(),
            _lock: lock,
            _lock_path: lock_path,
            _info_path: info_path.clone(),
        })),
    };

    let app = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/info", get(info_handler))
        .route("/v1/commands", post(command))
        .route("/v1/queries", post(query))
        .route("/v1/events", get(events))
        .route("/v1/runs", get(list_runs).post(create_run))
        .route("/v1/runs/{id}", get(get_run))
        .route("/v1/runs/{id}/cancel", post(cancel_run))
        .route("/v1/model", get(model_info).post(update_model))
        .route("/v1/model/catalog", get(model_catalog))
        .route("/v1/host/bind", post(crate::host::register))
        .route("/v1/host/unbind", post(crate::host::unregister))
        .route("/v1/host/authorize", post(crate::host::check))
        .route("/v1/admin/revoke", post(revoke_channel))
        .route("/v1/admin/revocations", get(list_revocations))
        .route("/v1/data/export", get(data_export))
        .route("/v1/data/export.csv", get(data_export_csv))
        .route("/v1/data/import/preflight", post(data_import_preflight))
        .route("/v1/data/import", post(data_import))
        .route("/v1/data/backup", post(data_backup))
        .route("/v1/data/backups", get(data_backups))
        .route("/v1/sync/export", get(sync_export))
        .route("/v1/sync/export/encrypted", post(sync_export_encrypted))
        .route("/v1/sync/import", post(sync_import))
        .route("/v1/sync/revoke", post(revoke_device))
        .layer(axum::extract::DefaultBodyLimit::max(4 * 1024 * 1024))
        .with_state(state.clone());

    crate::runs::spawn_run_worker(state.clone(), config.data_dir.clone());
    crate::notify::spawn_effect_worker(state.clone(), config.data_dir.clone());
    crate::automation::spawn_automation_worker(state.clone());

    info!(
        "zodo runtime listening on http://{}:{} instance={} workspace={}",
        config.host, port, info.instance_id, WORKSPACE_ID
    );
    println!(
        "{}",
        serde_json::to_string(&info).unwrap_or_else(|_| "{}".to_string())
    );
    let server = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal());
    if let Err(error) = server.await {
        let _ = std::fs::remove_file(&info_path);
        return Err(io_error("runtime server", error));
    }
    let _ = std::fs::remove_file(&info_path);
    Ok(())
}

fn write_runtime_info(path: &Path, info: &RuntimeInfo) -> Result<()> {
    let body =
        serde_json::to_vec_pretty(info).map_err(|error| io_error("encode runtime info", error))?;
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, body).map_err(|error| io_error("write runtime info", error))?;
    std::fs::rename(&temporary, path).map_err(|error| io_error("publish runtime info", error))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(path) {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o600);
            let _ = std::fs::set_permissions(path, permissions);
        }
    }
    Ok(())
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        warn!("failed to install Ctrl+C handler: {error}");
        return;
    }
    info!("shutdown requested");
}

pub(crate) fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<AuthPrincipal> {
    let header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            ZodoError::new(
                ErrorCode::PermissionDenied,
                "missing bearer token for the Zodo runtime",
            )
        })?;
    let token = header.strip_prefix("Bearer ").ok_or_else(|| {
        ZodoError::new(ErrorCode::PermissionDenied, "authorization must be Bearer")
    })?;
    let inner = state
        .inner
        .lock()
        .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "runtime state is unavailable"))?;
    let principal = inner
        .tokens
        .get(token)
        .cloned()
        .ok_or_else(|| ZodoError::new(ErrorCode::PermissionDenied, "unknown runtime token"))?;
    if inner.revoked_channels.contains(&principal.channel) {
        return Err(ZodoError::new(
            ErrorCode::PermissionDenied,
            format!(
                "channel {} has been revoked by the workspace owner",
                principal.channel
            ),
        ));
    }
    Ok(principal)
}

fn context(principal: &AuthPrincipal, request_id: &str, runtime_instance: &str) -> CommandContext {
    CommandContext {
        principal_id: principal.principal_id.clone(),
        principal_kind: principal.kind.clone(),
        channel: principal.channel.clone(),
        capabilities: principal.capabilities.clone(),
        workspace_id: principal.workspace_id.clone(),
        request_id: request_id.to_string(),
        received_at: Utc::now(),
        runtime_instance: runtime_instance.to_string(),
    }
}

pub(crate) fn status_for(error: &ZodoError) -> StatusCode {
    match error.code {
        ErrorCode::ValidationFailed | ErrorCode::UnsupportedOperation => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        ErrorCode::NotFound | ErrorCode::RunNotFound => StatusCode::NOT_FOUND,
        ErrorCode::RevisionConflict
        | ErrorCode::PolicyChanged
        | ErrorCode::ApprovalRequired
        | ErrorCode::ApprovalExpired
        | ErrorCode::IdempotencyMismatch => StatusCode::CONFLICT,
        ErrorCode::PermissionDenied => StatusCode::FORBIDDEN,
        ErrorCode::ProviderUnavailable | ErrorCode::ResourceExhausted => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

pub(crate) fn error_response(status: StatusCode, request_id: &str, error: &ZodoError) -> Response {
    (status, Json(ErrorEnvelope::new(request_id, error))).into_response()
}

async fn health(State(state): State<AppState>) -> Response {
    let (instance, workspace_id, started_at) = {
        match state.inner.lock() {
            Ok(inner) => (
                inner.instance_id.clone(),
                inner.workspace_id.clone(),
                inner.started_at,
            ),
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "",
                    &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
                );
            }
        }
    };
    let revision = {
        let mut inner = match state.inner.lock() {
            Ok(inner) => inner,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "",
                    &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
                );
            }
        };
        match inner.store.load_world(&workspace_id) {
            Ok(Some(world)) => world.workspace.revision,
            _ => 0,
        }
    };
    Json(serde_json::json!({
        "schema": "zodo.health/1",
        "protocol": PROTOCOL,
        "runtime_version": env!("CARGO_PKG_VERSION"),
        "instance_id": instance,
        "workspace_id": workspace_id,
        "revision": revision,
        "started_at": started_at,
    }))
    .into_response()
}

async fn info_handler(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    let runtime_instance = match state.inner.lock() {
        Ok(inner) => inner.instance_id.clone(),
        Err(_) => String::new(),
    };
    let ctx = context(&principal, "info", &runtime_instance);
    let envelope = QueryEnvelope {
        schema: "zodo.query/1".to_string(),
        request_id: "info".to_string(),
        workspace_id: principal.workspace_id.clone(),
        query: "health.info".to_string(),
        payload: serde_json::json!({}),
    };
    let result = {
        let mut inner = match state.inner.lock() {
            Ok(inner) => inner,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "info",
                    &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
                );
            }
        };
        let instance_id = inner.instance_id.clone();
        let mut app = Application {
            store: &mut inner.store,
            clock: &SystemClock,
            ids: &UuidIds::default(),
            instance_id,
        };
        app.query(&ctx, envelope)
    };
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(status_for(&error), "info", &error),
    }
}

async fn command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(envelope): Json<CommandEnvelope>,
) -> Response {
    let request_id = envelope.request_id.clone();
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), &request_id, &error),
    };
    let (runtime_instance, result) = {
        let mut inner = match state.inner.lock() {
            Ok(inner) => inner,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &request_id,
                    &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
                );
            }
        };
        let runtime_instance = inner.instance_id.clone();
        let ctx = context(&principal, &request_id, &runtime_instance);
        let instance_id = inner.instance_id.clone();
        let mut app = Application {
            store: &mut inner.store,
            clock: &SystemClock,
            ids: &UuidIds::default(),
            instance_id,
        };
        (runtime_instance, app.execute(&ctx, envelope))
    };
    let _ = runtime_instance;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(status_for(&error), &request_id, &error),
    }
}

async fn query(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(envelope): Json<QueryEnvelope>,
) -> Response {
    let request_id = envelope.request_id.clone();
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), &request_id, &error),
    };
    if envelope.workspace_id != principal.workspace_id {
        return error_response(
            StatusCode::FORBIDDEN,
            &request_id,
            &ZodoError::new(
                ErrorCode::PermissionDenied,
                "workspace is outside this authenticated connection",
            ),
        );
    }
    if envelope.query == "runs.list" || envelope.query == "run.get" {
        return run_query(&state, &principal, envelope);
    }
    let result = {
        let mut inner = match state.inner.lock() {
            Ok(inner) => inner,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &request_id,
                    &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
                );
            }
        };
        let runtime_instance = inner.instance_id.clone();
        let ctx = context(&principal, &request_id, &runtime_instance);
        let instance_id = inner.instance_id.clone();
        let mut app = Application {
            store: &mut inner.store,
            clock: &SystemClock,
            ids: &UuidIds::default(),
            instance_id,
        };
        app.query(&ctx, envelope)
    };
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(status_for(&error), &request_id, &error),
    }
}

#[derive(Debug, Deserialize)]
struct EventsParams {
    #[serde(default)]
    after: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumQuery(params): AxumQuery<EventsParams>,
) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    let request_id = format!("events-{}", Utc::now().timestamp_millis());
    let envelope = QueryEnvelope {
        schema: "zodo.query/1".to_string(),
        request_id: request_id.clone(),
        workspace_id: principal.workspace_id.clone(),
        query: "events.list".to_string(),
        payload: serde_json::json!({
            "after_cursor": params.after,
            "limit": params.limit.unwrap_or(100),
        }),
    };
    let result = {
        let mut inner = match state.inner.lock() {
            Ok(inner) => inner,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &request_id,
                    &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
                );
            }
        };
        let runtime_instance = inner.instance_id.clone();
        let ctx = context(&principal, &request_id, &runtime_instance);
        let instance_id = inner.instance_id.clone();
        let mut app = Application {
            store: &mut inner.store,
            clock: &SystemClock,
            ids: &UuidIds::default(),
            instance_id,
        };
        app.query(&ctx, envelope)
    };
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(status_for(&error), &request_id, &error),
    }
}

pub(crate) fn require_capability(principal: &AuthPrincipal, capability: &str) -> Result<()> {
    if principal
        .capabilities
        .iter()
        .any(|value| value == capability)
    {
        Ok(())
    } else {
        Err(ZodoError::new(
            ErrorCode::PermissionDenied,
            format!(
                "principal {} lacks capability {capability}",
                principal.principal_id
            ),
        ))
    }
}

fn run_query(state: &AppState, principal: &AuthPrincipal, envelope: QueryEnvelope) -> Response {
    if let Err(error) = require_capability(principal, "read") {
        return error_response(status_for(&error), &envelope.request_id, &error);
    }
    let mut inner = match state.inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &envelope.request_id,
                &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
            );
        }
    };
    let (revision, event_cursor) = match inner.store.load_world(&envelope.workspace_id) {
        Ok(Some(world)) => (
            world.workspace.revision,
            inner
                .store
                .latest_cursor(&envelope.workspace_id)
                .unwrap_or_else(|_| "ev_0".to_string()),
        ),
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                &envelope.request_id,
                &ZodoError::not_found("workspace", &envelope.workspace_id),
            );
        }
        Err(error) => return error_response(status_for(&error), &envelope.request_id, &error),
    };
    let data = if envelope.query == "runs.list" {
        match inner.store.list_runs(&envelope.workspace_id, 100) {
            Ok(runs) => json!({"runs": runs.iter().map(RunRecord::safe_view).collect::<Vec<_>>()}),
            Err(error) => {
                return error_response(status_for(&error), &envelope.request_id, &error);
            }
        }
    } else {
        let id = envelope
            .payload
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match inner.store.get_run(id) {
            Ok(Some(run)) if run.workspace_id == envelope.workspace_id => {
                json!({"run": run.safe_view()})
            }
            Ok(Some(_)) | Ok(None) => {
                return error_response(
                    StatusCode::NOT_FOUND,
                    &envelope.request_id,
                    &ZodoError::not_found("run", id),
                );
            }
            Err(error) => {
                return error_response(status_for(&error), &envelope.request_id, &error);
            }
        }
    };
    Json(QueryResult::new(
        envelope.request_id,
        revision,
        event_cursor,
        data,
    ))
    .into_response()
}

#[derive(Debug, Deserialize)]
struct NewRunRequest {
    intent: String,
    #[serde(default)]
    input: Value,
    #[serde(default)]
    idempotency_key: Option<String>,
}

fn project_run(run: &RunRecord, principal: &AuthPrincipal) -> Value {
    let mut view = run.safe_view();
    if principal.channel == "harness_ui" && require_capability(principal, "read_private").is_ok() {
        if let Some(messages) = run.input.get("messages") {
            view["input"]["messages"] = messages.clone();
        }
    }
    view
}

async fn create_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<NewRunRequest>,
) -> Response {
    let request_id = request
        .idempotency_key
        .clone()
        .unwrap_or_else(|| format!("run-{}", Utc::now().timestamp_millis()));
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), &request_id, &error),
    };
    if let Err(error) = require_capability(&principal, "propose") {
        return error_response(status_for(&error), &request_id, &error);
    }
    if let Err(error) = validate_run_intent(&request.intent, &request.input) {
        return error_response(status_for(&error), &request_id, &error);
    }
    let inner = match state.inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &request_id,
                &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
            );
        }
    };
    let created = if let Some(key) = request.idempotency_key.as_deref() {
        if key.is_empty() || key.len() > 200 || key.chars().any(char::is_control) {
            return error_response(
                StatusCode::BAD_REQUEST,
                &request_id,
                &ZodoError::validation("invalid run idempotency key"),
            );
        }
        let scope = json!([
            principal.workspace_id,
            principal.principal_id,
            principal.channel,
            key
        ]);
        let id = format!(
            "run_{}",
            hex::encode(Sha256::digest(scope.to_string().as_bytes()))
        );
        let now = Utc::now().to_rfc3339();
        let inserted = inner.store.raw_connection().execute(
            "INSERT INTO runs (id, workspace_id, state, intent, input_json, created_at, updated_at)
             VALUES (?1, ?2, 'queued', ?3, ?4, ?5, ?5) ON CONFLICT(id) DO NOTHING",
            rusqlite::params![
                id,
                principal.workspace_id,
                request.intent,
                request.input.to_string(),
                now
            ],
        );
        match inserted {
            Ok(changed) => match inner.store.get_run(&id) {
                Ok(Some(run)) if run.intent == request.intent && run.input == request.input => {
                    if changed == 0 {
                        return (StatusCode::ACCEPTED, Json(json!({"schema": "zodo.run/1", "run": project_run(&run, &principal)}))).into_response();
                    }
                    Ok(run)
                }
                Ok(Some(_)) => Err(ZodoError::new(
                    ErrorCode::IdempotencyMismatch,
                    "run key was used with a different input",
                )),
                Ok(None) => Err(ZodoError::new(
                    ErrorCode::StorageFailure,
                    "run lookup failed",
                )),
                Err(error) => Err(error),
            },
            Err(_) => Err(ZodoError::new(
                ErrorCode::StorageFailure,
                "run insert failed",
            )),
        }
    } else {
        inner
            .store
            .create_run(&principal.workspace_id, &request.intent, &request.input)
    };
    let run = match created {
        Ok(run) => run,
        Err(error) => return error_response(status_for(&error), &request_id, &error),
    };
    let _ = inner.store.append_run_step(
        &run.id,
        "queued",
        &json!({"actor_id": principal.principal_id, "channel": principal.channel}),
    );
    (
        StatusCode::ACCEPTED,
        Json(json!({"schema": "zodo.run/1", "run": project_run(&run, &principal)})),
    )
        .into_response()
}

async fn list_runs(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "read") {
        return error_response(status_for(&error), "", &error);
    }
    let inner = match state.inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "",
                &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
            );
        }
    };
    match inner.store.list_runs(&principal.workspace_id, 100) {
        Ok(runs) => Json(json!({
            "schema": "zodo.run-list/1",
            "runs": runs.iter().map(|run| project_run(run, &principal)).collect::<Vec<_>>(),
        }))
        .into_response(),
        Err(error) => error_response(status_for(&error), "", &error),
    }
}

async fn get_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    let inner = match state.inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "",
                &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
            );
        }
    };
    match inner.store.get_run(&id) {
        Ok(Some(run)) if run.workspace_id == principal.workspace_id => {
            Json(json!({"schema": "zodo.run/1", "run": project_run(&run, &principal)}))
                .into_response()
        }
        Ok(Some(_)) => error_response(StatusCode::NOT_FOUND, "", &ZodoError::not_found("run", &id)),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "", &ZodoError::not_found("run", &id)),
        Err(error) => error_response(status_for(&error), "", &error),
    }
}

async fn cancel_run(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "propose") {
        return error_response(status_for(&error), "", &error);
    }
    let inner = match state.inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "",
                &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
            );
        }
    };
    match inner.store.request_run_cancel(&id) {
        Ok(Some(run)) if run.workspace_id == principal.workspace_id => {
            Json(json!({"schema": "zodo.run/1", "run": run.safe_view()})).into_response()
        }
        Ok(Some(_)) => error_response(StatusCode::NOT_FOUND, "", &ZodoError::not_found("run", &id)),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "", &ZodoError::not_found("run", &id)),
        Err(error) => error_response(status_for(&error), "", &error),
    }
}

async fn model_catalog(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "read") {
        return error_response(status_for(&error), "", &error);
    }
    let inner = match state.inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "",
                &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
            );
        }
    };
    match ModelConfig::load(&inner.data_dir) {
        Ok(config) => Json(config.catalog()).into_response(),
        Err(error) => error_response(status_for(&error), "", &error),
    }
}

fn model_response(state: &AppState, workspace_id: &str) -> Result<Value> {
    let mut inner = state
        .inner
        .lock()
        .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"))?;
    let world = inner
        .store
        .load_world(workspace_id)?
        .ok_or_else(|| ZodoError::not_found("workspace", workspace_id))?;
    let gateway = ModelGateway::load(&inner.data_dir)?;
    Ok(
        json!({"schema":"zodo.model/1", "config":gateway.config, "effective":gateway.effective(&world.workspace.policy, &inner.host)}),
    )
}

async fn model_info(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "read") {
        return error_response(status_for(&error), "", &error);
    }
    match model_response(&state, &principal.workspace_id) {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(status_for(&error), "", &error),
    }
}

async fn update_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(config): Json<crate::ai::ModelConfig>,
) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "admin") {
        return error_response(status_for(&error), "", &error);
    }
    let saved = (|| -> Result<()> {
        let mut inner = state
            .inner
            .lock()
            .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"))?;
        config.save(&inner.data_dir)?;
        inner.host.model_revision += 1;
        Ok(())
    })();
    match saved.and_then(|_| model_response(&state, &principal.workspace_id)) {
        Ok(value) => Json(value).into_response(),
        Err(error) => error_response(status_for(&error), "", &error),
    }
}

async fn list_revocations(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "admin") {
        return error_response(status_for(&error), "", &error);
    }
    let revoked = {
        let inner = match state.inner.lock() {
            Ok(inner) => inner,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "",
                    &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
                );
            }
        };
        inner.revoked_channels.iter().cloned().collect::<Vec<_>>()
    };
    Json(json!({"schema": "zodo.auth/1", "revoked_channels": revoked})).into_response()
}

#[derive(Debug, Deserialize)]
struct RevokeRequest {
    channel: String,
    #[serde(default = "default_revoked")]
    revoked: bool,
}

fn default_revoked() -> bool {
    true
}

async fn revoke_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RevokeRequest>,
) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "admin") {
        return error_response(status_for(&error), "", &error);
    }
    let requested_channel = request.channel.trim();
    if requested_channel.is_empty() || requested_channel.len() > 50 {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "",
            &ZodoError::validation("channel must be 1..50 bytes"),
        );
    }
    let channel = match requested_channel {
        "agent" => "harness_agent",
        "ui" => "harness_ui",
        "mcp" => "mcp_stdio",
        other => other,
    };
    let (data_dir, revoked_channels, revoked_devices) = {
        let inner = match state.inner.lock() {
            Ok(inner) => inner,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "",
                    &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
                );
            }
        };
        (
            inner.data_dir.clone(),
            inner.revoked_channels.clone(),
            inner.revoked_devices.clone(),
        )
    };
    let mut next = revoked_channels;
    if request.revoked {
        next.insert(channel.to_string());
    } else {
        next.remove(channel);
    }
    let auth_state = AuthState {
        revoked_channels: next.clone(),
        revoked_devices: revoked_devices.clone(),
    };
    if let Err(error) = write_auth_state(&data_dir, &auth_state) {
        return error_response(status_for(&error), "", &error);
    }
    {
        let mut inner = match state.inner.lock() {
            Ok(inner) => inner,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "",
                    &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
                );
            }
        };
        inner.revoked_channels = next.clone();
    }
    Json(json!({
        "schema": "zodo.auth/1",
        "revoked_channels": next.iter().collect::<Vec<_>>(),
        "channel": channel,
        "revoked": request.revoked,
    }))
    .into_response()
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

async fn data_export_csv(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "read_private") {
        return error_response(status_for(&error), "", &error);
    }
    let mut inner = match state.inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "",
                &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
            );
        }
    };
    let world = match inner.store.load_world(&principal.workspace_id) {
        Ok(Some(world)) => world,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "",
                &ZodoError::not_found("workspace", &principal.workspace_id),
            );
        }
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    let mut rows = world.tasks.iter().collect::<Vec<_>>();
    rows.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    let mut out = String::from(
        "id,title,status,project_id,estimate_minutes,actual_minutes,remaining_minutes,target_due,hard_deadline,priority,tags,updated_at\n",
    );
    for task in rows {
        let fields = [
            task.id.clone(),
            task.title.clone(),
            serde_json::to_value(&task.status)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_default(),
            task.project_id.clone().unwrap_or_default(),
            task.estimate_minutes
                .map(|value| value.to_string())
                .unwrap_or_default(),
            task.actual_minutes.to_string(),
            task.remaining_minutes
                .map(|value| value.to_string())
                .unwrap_or_default(),
            task.target_due
                .map(|value| value.to_string())
                .unwrap_or_default(),
            task.hard_deadline
                .map(|value| value.to_rfc3339())
                .unwrap_or_default(),
            task.priority.to_string(),
            task.tags.join(";"),
            task.updated_at.to_rfc3339(),
        ];
        out.push_str(
            &fields
                .iter()
                .map(|value| csv_escape(value))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
    }
    (
        [(axum::http::header::CONTENT_TYPE, "text/csv; charset=utf-8")],
        out,
    )
        .into_response()
}

async fn data_export(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "read_private") {
        return error_response(status_for(&error), "", &error);
    }
    let mut inner = match state.inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "",
                &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
            );
        }
    };
    let world = match inner.store.load_world(&principal.workspace_id) {
        Ok(Some(world)) => world,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "",
                &ZodoError::not_found("workspace", &principal.workspace_id),
            );
        }
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    let facts = serde_json::to_value(&world).unwrap_or(Value::Null);
    let response = json!({
        "schema": "zodo.export/1",
        "exported_at": Utc::now(),
        "source_instance": inner.instance_id,
        "workspace_id": principal.workspace_id,
        "contains_credentials": false,
        "counts": {
            "tasks": world.tasks.len(),
            "goals": world.goals.len(),
            "projects": world.projects.len(),
            "inbox": world.inbox.len(),
            "habits": world.habits.len(),
            "activities": world.activities.len(),
            "notes": world.notes.len(),
        },
        "facts": facts,
    });
    Json(response).into_response()
}

fn bundle_fingerprint(facts: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_vec(facts).unwrap_or_default());
    format!("{:x}", hasher.finalize())
}

async fn data_import_preflight(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "admin") {
        return error_response(status_for(&error), "", &error);
    }
    let bundle = request.get("bundle").cloned().unwrap_or(Value::Null);
    if bundle.get("schema").and_then(Value::as_str) != Some("zodo.export/1") {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "",
            &ZodoError::validation("import bundle schema must be zodo.export/1"),
        );
    }
    if bundle.get("contains_credentials").and_then(Value::as_bool) != Some(false) {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "",
            &ZodoError::validation("import bundle must not contain credentials"),
        );
    }
    let facts = bundle.get("facts").cloned().unwrap_or(Value::Null);
    let imported: World = match serde_json::from_value(facts.clone()) {
        Ok(world) => world,
        Err(error) => {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "",
                &ZodoError::validation(format!("invalid import facts: {error}")),
            );
        }
    };
    let mut inner = match state.inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "",
                &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
            );
        }
    };
    let current = match inner.store.load_world(&principal.workspace_id) {
        Ok(Some(world)) => world,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "",
                &ZodoError::not_found("workspace", &principal.workspace_id),
            );
        }
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    let non_empty = !current.tasks.is_empty()
        || !current.goals.is_empty()
        || !current.projects.is_empty()
        || !current.inbox.is_empty()
        || !current.habits.is_empty()
        || !current.activities.is_empty()
        || !current.notes.is_empty()
        || !current.proposals.is_empty();
    Json(json!({
        "schema": "zodo.import-preflight/1",
        "fingerprint": bundle_fingerprint(&facts),
        "contains_credentials": false,
        "empty_workspace_required": true,
        "workspace_empty": !non_empty,
        "can_import": !non_empty,
        "counts": {
            "tasks": imported.tasks.len(),
            "goals": imported.goals.len(),
            "projects": imported.projects.len(),
            "inbox": imported.inbox.len(),
            "habits": imported.habits.len(),
            "notes": imported.notes.len(),
        },
        "local_revision": current.workspace.revision,
        "imported_workspace_id": imported.workspace.id,
    }))
    .into_response()
}

async fn data_import(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "admin") {
        return error_response(status_for(&error), "", &error);
    }
    let bundle_values: Vec<Value> = match request.get("bundles") {
        Some(Value::Array(items)) => items.clone(),
        Some(_) => {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "",
                &ZodoError::validation("bundles must be an array"),
            );
        }
        None => vec![request.get("bundle").cloned().unwrap_or(Value::Null)],
    };
    if bundle_values.is_empty() {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "",
            &ZodoError::validation("at least one import bundle is required"),
        );
    }
    let mut parsed_worlds: Vec<World> = Vec::new();
    let mut fact_values: Vec<Value> = Vec::new();
    for bundle in bundle_values {
        if bundle.get("schema").and_then(Value::as_str) != Some("zodo.export/1") {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "",
                &ZodoError::validation("import bundle schema must be zodo.export/1"),
            );
        }
        if bundle.get("contains_credentials").and_then(Value::as_bool) != Some(false) {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "",
                &ZodoError::validation("import bundle must not contain credentials"),
            );
        }
        let facts = bundle.get("facts").cloned().unwrap_or(Value::Null);
        fact_values.push(facts.clone());
        match serde_json::from_value(facts) {
            Ok(world) => parsed_worlds.push(world),
            Err(error) => {
                return error_response(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "",
                    &ZodoError::validation(format!("invalid import facts: {error}")),
                );
            }
        }
    }
    let fingerprint = bundle_fingerprint(&json!({"bundles": fact_values}));
    let idempotency_key = request
        .get("idempotency_key")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut inner = match state.inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "",
                &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
            );
        }
    };
    if let Some(key) = &idempotency_key {
        match inner.store.get_data_import(&principal.workspace_id, key) {
            Ok(Some((stored_fingerprint, mut result, revision))) => {
                if stored_fingerprint == fingerprint {
                    result["replayed"] = Value::Bool(true);
                    result["fingerprint"] = Value::String(fingerprint);
                    result["revision"] = Value::Number(revision.into());
                    return Json(result).into_response();
                }
                return error_response(
                    StatusCode::CONFLICT,
                    "",
                    &ZodoError::new(
                        ErrorCode::IdempotencyMismatch,
                        "idempotency key was already used for a different import bundle",
                    ),
                );
            }
            Ok(None) => {}
            Err(error) => return error_response(status_for(&error), "", &error),
        }
    }
    let mut current = match inner.store.load_world(&principal.workspace_id) {
        Ok(Some(world)) => world,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "",
                &ZodoError::not_found("workspace", &principal.workspace_id),
            );
        }
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    let non_empty = !current.tasks.is_empty()
        || !current.goals.is_empty()
        || !current.projects.is_empty()
        || !current.inbox.is_empty()
        || !current.habits.is_empty()
        || !current.activities.is_empty()
        || !current.notes.is_empty()
        || !current.proposals.is_empty();
    if non_empty {
        return error_response(
            StatusCode::CONFLICT,
            "",
            &ZodoError::validation(
                "JSON import is only allowed into a workspace with no facts; use backup restore for replacement",
            ),
        );
    }
    let mut merged_map: BTreeMap<(String, String), Value> = BTreeMap::new();
    for world in parsed_worlds {
        for (key, value) in world.entity_json_map() {
            if let Some(existing) = merged_map.get(&key) {
                if existing != &value {
                    return error_response(
                        StatusCode::CONFLICT,
                        "",
                        &ZodoError::validation(
                            "batched import bundles contain conflicting entities; no partial import was written",
                        ),
                    );
                }
            } else {
                merged_map.insert(key, value);
            }
        }
    }
    let imported = match World::from_entities(current.workspace.clone(), merged_map) {
        Ok(world) => world,
        Err(error) => {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "",
                &ZodoError::validation(format!("cannot assemble batched import: {error}")),
            );
        }
    };
    let now = Utc::now();
    let workspace_id = current.workspace.id.clone();
    let revision = current.workspace.revision.saturating_add(1);
    current.workspace.revision = revision;
    current.workspace.updated_at = now;
    current.inbox = imported.inbox;
    current.goals = imported.goals;
    current.projects = imported.projects;
    current.tasks = imported.tasks;
    current.dependencies = imported.dependencies;
    current.commitments = imported.commitments;
    current.capacity_blocks = imported.capacity_blocks;
    current.allocations = imported.allocations;
    current.tags = imported.tags;
    current.notes = imported.notes;
    current.attachments = imported.attachments;
    current.saved_queries = imported.saved_queries;
    current.reminders = imported.reminders;
    current.focus_sessions = imported.focus_sessions;
    current.activities = imported.activities;
    current.reviews = imported.reviews;
    current.habits = imported.habits;
    current.recurrence_rules = imported.recurrence_rules;
    current.occurrences = imported.occurrences;
    current.proposals = imported.proposals;
    current.approval_grants = imported.approval_grants;
    current.calendar_sources = imported.calendar_sources;
    current.calendar_events = imported.calendar_events;
    current.automation_rules = imported.automation_rules;
    let operation_id = format!("op_{}", uuid::Uuid::new_v4().simple());
    let event = NewEvent {
        workspace_id: workspace_id.clone(),
        event_type: "data.imported".to_string(),
        actor_id: principal.principal_id.clone(),
        payload: json!({
            "operation": "data.import",
            "operation_id": operation_id,
            "revision": revision,
            "counts": {
                "tasks": current.tasks.len(),
                "goals": current.goals.len(),
                "projects": current.projects.len(),
                "inbox": current.inbox.len(),
                "habits": current.habits.len(),
            }
        }),
        created_at: now,
    };
    let audit = NewAudit {
        workspace_id: workspace_id.clone(),
        actor_id: principal.principal_id.clone(),
        principal_kind: format!("{:?}", principal.kind).to_lowercase(),
        channel: principal.channel.clone(),
        operation: "data.import".to_string(),
        operation_id: operation_id.clone(),
        revision,
        status: "committed".to_string(),
        summary: json!({"source": "zodo.export/1", "contains_credentials": false}),
        created_at: now,
    };
    let outcome = inner.store.save(
        &current,
        SaveOptions {
            expected_revision: revision.saturating_sub(1),
            bump_revision: true,
        },
        None,
        std::slice::from_ref(&event),
        std::slice::from_ref(&audit),
        &[],
    );
    match outcome {
        Ok(saved) => {
            let result = json!({
                "schema": "zodo.import-result/1",
                "imported": true,
                "replayed": false,
                "fingerprint": fingerprint,
                "revision": saved.revision,
                "event_cursor": saved.event_cursor,
                "counts": {
                    "tasks": current.tasks.len(),
                    "goals": current.goals.len(),
                    "projects": current.projects.len(),
                    "inbox": current.inbox.len(),
                    "habits": current.habits.len(),
                },
                "credentials_imported": false,
            });
            if let Some(key) = &idempotency_key {
                if let Err(error) = inner.store.record_data_import(
                    &principal.workspace_id,
                    key,
                    &fingerprint,
                    &result,
                    saved.revision,
                ) {
                    return error_response(status_for(&error), "", &error);
                }
            }
            Json(result).into_response()
        }
        Err(error) => error_response(status_for(&error), "", &error),
    }
}

async fn data_backup(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "admin") {
        return error_response(status_for(&error), "", &error);
    }
    let (data_dir, db_path) = {
        let inner = match state.inner.lock() {
            Ok(inner) => inner,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "",
                    &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
                );
            }
        };
        let path = inner.store.path().to_path_buf();
        (inner.data_dir.clone(), path)
    };
    let backup_dir = data_dir.join("backups");
    if let Err(error) = std::fs::create_dir_all(&backup_dir) {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "",
            &ZodoError::new(
                ErrorCode::StorageFailure,
                format!("create backup dir: {error}"),
            ),
        );
    }
    let filename = format!(
        "zodo-{}-{}.sqlite3",
        Utc::now().format("%Y%m%d-%H%M%S"),
        Utc::now().timestamp_millis() % 1000
    );
    let path = backup_dir.join(&filename);
    let backup_result = {
        let inner = match state.inner.lock() {
            Ok(inner) => inner,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "",
                    &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
                );
            }
        };
        inner
            .store
            .raw_connection()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .and_then(|()| {
                inner
                    .store
                    .raw_connection()
                    .execute("VACUUM INTO ?1", rusqlite::params![path.to_string_lossy()])
                    .map(|_| ())
            })
    };
    if let Err(error) = backup_result {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "",
            &ZodoError::new(ErrorCode::StorageFailure, format!("backup failed: {error}")),
        );
    }
    let size = std::fs::metadata(&path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let sha256 = sha256_file(&path).unwrap_or_default();
    Json(json!({
        "schema": "zodo.backup/1",
        "file": filename,
        "path": path.display().to_string(),
        "size_bytes": size,
        "sha256": sha256,
        "db_path": db_path.display().to_string(),
        "credentials_included": false,
    }))
    .into_response()
}

async fn data_backups(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "admin") {
        return error_response(status_for(&error), "", &error);
    }
    let data_dir = match state.inner.lock() {
        Ok(inner) => inner.data_dir.clone(),
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "",
                &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
            );
        }
    };
    let dir = data_dir.join("backups");
    let mut backups = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("sqlite3") {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            backups.push(json!({
                "file": path.file_name().and_then(|value| value.to_str()).unwrap_or_default(),
                "size_bytes": metadata.len(),
                "modified_at": metadata.modified().ok().map(DateTime::<Utc>::from),
                "sha256": sha256_file(&path).unwrap_or_default(),
            }));
        }
    }
    backups.sort_by(|left, right| right["file"].as_str().cmp(&left["file"].as_str()));
    Json(json!({"schema": "zodo.backup-list/1", "backups": backups})).into_response()
}

fn device_id(data_dir: &Path) -> String {
    let path = data_dir.join("device.json");
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Ok(value) = serde_json::from_str::<Value>(&text) {
            if let Some(id) = value.get("device_id").and_then(Value::as_str) {
                return id.to_string();
            }
        }
    }
    let id = format!("dev_{}", uuid::Uuid::new_v4().simple());
    let _ = std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({"schema":"zodo.device/1","device_id":id}))
            .unwrap_or_default(),
    );
    id
}

async fn sync_export_encrypted(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "read_private") {
        return error_response(status_for(&error), "", &error);
    }
    let passphrase = request
        .get("passphrase")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut inner = match state.inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "",
                &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
            );
        }
    };
    let world = match inner.store.load_world(&principal.workspace_id) {
        Ok(Some(world)) => world,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "",
                &ZodoError::not_found("workspace", &principal.workspace_id),
            );
        }
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    let bundle = json!({
        "schema": "zodo.sync/1",
        "device_id": device_id(&inner.data_dir),
        "exported_at": Utc::now(),
        "workspace_id": principal.workspace_id,
        "revision": world.workspace.revision,
        "contains_credentials": false,
        "facts": serde_json::to_value(&world).unwrap_or(Value::Null),
    });
    match crate::crypto::encrypt_bundle(&bundle, passphrase) {
        Ok(envelope) => Json(envelope).into_response(),
        Err(error) => error_response(status_for(&error), "", &error),
    }
}

async fn sync_export(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "read_private") {
        return error_response(status_for(&error), "", &error);
    }
    let mut inner = match state.inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "",
                &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
            );
        }
    };
    let world = match inner.store.load_world(&principal.workspace_id) {
        Ok(Some(world)) => world,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "",
                &ZodoError::not_found("workspace", &principal.workspace_id),
            );
        }
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    let device = device_id(&inner.data_dir);
    Json(json!({
        "schema": "zodo.sync/1",
        "device_id": device,
        "exported_at": Utc::now(),
        "workspace_id": principal.workspace_id,
        "revision": world.workspace.revision,
        "contains_credentials": false,
        "facts": serde_json::to_value(&world).unwrap_or(Value::Null),
    }))
    .into_response()
}

async fn revoke_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "admin") {
        return error_response(status_for(&error), "", &error);
    }
    let device_id = request
        .get("device_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    if device_id.is_empty() || device_id.len() > 200 {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "",
            &ZodoError::validation("device_id must be 1..200 bytes"),
        );
    }
    let revoked = request
        .get("revoked")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let (data_dir, revoked_channels, mut devices) = {
        let inner = match state.inner.lock() {
            Ok(inner) => inner,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "",
                    &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
                );
            }
        };
        (
            inner.data_dir.clone(),
            inner.revoked_channels.clone(),
            inner.revoked_devices.clone(),
        )
    };
    if revoked {
        devices.insert(device_id);
    } else {
        devices.remove(&device_id);
    }
    if let Err(error) = write_auth_state(
        &data_dir,
        &AuthState {
            revoked_channels,
            revoked_devices: devices.clone(),
        },
    ) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, "", &error);
    }
    if let Ok(mut inner) = state.inner.lock() {
        inner.revoked_devices = devices.clone();
    }
    Json(json!({
        "schema": "zodo.auth/1",
        "revoked_devices": devices.iter().collect::<Vec<_>>(),
    }))
    .into_response()
}

async fn sync_import(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    let principal = match authenticate(&state, &headers) {
        Ok(value) => value,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    if let Err(error) = require_capability(&principal, "admin") {
        return error_response(status_for(&error), "", &error);
    }
    let mut bundle = request.get("bundle").cloned().unwrap_or(Value::Null);
    if bundle.get("schema").and_then(Value::as_str) == Some(crate::crypto::ENCRYPTED_SCHEMA) {
        let passphrase = request
            .get("passphrase")
            .and_then(Value::as_str)
            .unwrap_or_default();
        bundle = match crate::crypto::decrypt_bundle(&bundle, passphrase) {
            Ok(value) => value,
            Err(error) => return error_response(status_for(&error), "", &error),
        };
    }
    if bundle.get("schema").and_then(Value::as_str) != Some("zodo.sync/1")
        && bundle.get("schema").and_then(Value::as_str) != Some("zodo.export/1")
    {
        return error_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "",
            &ZodoError::validation("sync bundle schema must be zodo.sync/1 or zodo.export/1"),
        );
    }
    let imported: World =
        match serde_json::from_value(bundle.get("facts").cloned().unwrap_or(Value::Null)) {
            Ok(world) => world,
            Err(error) => {
                return error_response(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "",
                    &ZodoError::validation(format!("invalid sync facts: {error}")),
                );
            }
        };
    let mut inner = match state.inner.lock() {
        Ok(inner) => inner,
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "",
                &ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"),
            );
        }
    };
    if let Some(device) = bundle.get("device_id").and_then(Value::as_str) {
        if inner.revoked_devices.contains(device) {
            return error_response(
                StatusCode::FORBIDDEN,
                "",
                &ZodoError::new(
                    ErrorCode::PermissionDenied,
                    "this sync device has been revoked by the workspace owner",
                ),
            );
        }
    }
    let mut current = match inner.store.load_world(&principal.workspace_id) {
        Ok(Some(world)) => world,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                "",
                &ZodoError::not_found("workspace", &principal.workspace_id),
            );
        }
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    let local_workspace = current.workspace.clone();
    let local_map = current.entity_json_map();
    let remote_map = imported.entity_json_map();
    let resolutions = request.get("resolutions").and_then(Value::as_object);
    let choice_for = |id: &str| {
        resolutions
            .and_then(|map| map.get(id))
            .and_then(Value::as_str)
    };
    let mut merged = local_map.clone();
    let mut conflicts: Vec<Value> = Vec::new();
    let mut added = 0usize;
    let mut resolved = 0usize;
    for ((kind, id), remote_value) in &remote_map {
        match local_map.get(&(kind.clone(), id.clone())) {
            Some(local_value) if local_value == remote_value => {}
            Some(_) => match choice_for(id) {
                Some("incoming") => {
                    merged.insert((kind.clone(), id.clone()), remote_value.clone());
                    resolved += 1;
                    added += 1;
                }
                Some("local") => resolved += 1,
                _ => conflicts.push(json!({"kind": kind, "id": id})),
            },
            None => {
                merged.insert((kind.clone(), id.clone()), remote_value.clone());
                added += 1;
            }
        }
    }
    if !conflicts.is_empty() {
        return error_response(
            StatusCode::CONFLICT,
            "",
            &ZodoError::validation("sync has conflicting entities; no partial import was written")
                .with_details(json!({"conflicts": conflicts})),
        );
    }
    if added == 0 {
        return Json(json!({
            "schema": "zodo.sync-result/1",
            "imported": true,
            "unchanged": true,
            "added": 0,
            "resolved": resolved,
            "device_id": device_id(&inner.data_dir),
        }))
        .into_response();
    }
    let now = Utc::now();
    let revision = current.workspace.revision.saturating_add(1);
    current = match World::from_entities(local_workspace, merged) {
        Ok(world) => world,
        Err(error) => return error_response(status_for(&error), "", &error),
    };
    current.workspace.revision = revision;
    current.workspace.updated_at = now;
    let operation_id = format!("op_{}", uuid::Uuid::new_v4().simple());
    let event = NewEvent {
        workspace_id: principal.workspace_id.clone(),
        event_type: "sync.imported".to_string(),
        actor_id: principal.principal_id.clone(),
        payload: json!({"operation":"sync.import","operation_id":operation_id,"revision":revision,"added":added}),
        created_at: now,
    };
    let audit = NewAudit {
        workspace_id: principal.workspace_id.clone(),
        actor_id: principal.principal_id.clone(),
        principal_kind: format!("{:?}", principal.kind).to_lowercase(),
        channel: principal.channel.clone(),
        operation: "sync.import".to_string(),
        operation_id,
        revision,
        status: "committed".to_string(),
        summary: json!({"added": added, "resolved": resolved, "conflicts": 0, "contains_credentials": false}),
        created_at: now,
    };
    match inner.store.save(
        &current,
        SaveOptions {
            expected_revision: revision.saturating_sub(1),
            bump_revision: true,
        },
        None,
        std::slice::from_ref(&event),
        std::slice::from_ref(&audit),
        &[],
    ) {
        Ok(saved) => Json(json!({
            "schema": "zodo.sync-result/1",
            "imported": true,
            "added": added,
            "resolved": resolved,
            "revision": saved.revision,
            "event_cursor": saved.event_cursor,
            "device_id": device_id(&inner.data_dir),
        }))
        .into_response(),
        Err(error) => error_response(status_for(&error), "", &error),
    }
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = std::io::Read::read(&mut file, &mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod model_api_tests {
    use super::*;
    use zodo_kernel::model::{ModelAccess, PrivacyPolicy, Workspace};

    fn state(dir: &Path, policy: PrivacyPolicy) -> AppState {
        let mut store = SqliteStore::open(dir.join("test.sqlite3")).unwrap();
        let mut world = World::new(Workspace::new(
            WORKSPACE_ID.into(),
            "Synthetic test".into(),
            Utc::now(),
        ));
        world.workspace.policy = policy;
        store.create_workspace(&world).unwrap();
        AppState {
            inner: Arc::new(Mutex::new(RuntimeInner {
                store,
                instance_id: "test".into(),
                workspace_id: WORKSPACE_ID.into(),
                started_at: Utc::now(),
                tokens: principals(
                    BTreeMap::from([
                        ("ui".into(), "ui-token".into()),
                        ("agent".into(), "agent-token".into()),
                    ]),
                    WORKSPACE_ID,
                ),
                revoked_channels: BTreeSet::new(),
                revoked_devices: BTreeSet::new(),
                data_dir: dir.into(),
                host: crate::host::HostState::default(),
                _lock: File::create(dir.join("test.lock")).unwrap(),
                _lock_path: dir.join("test.lock"),
                _info_path: dir.join("test-info.json"),
            })),
        }
    }

    fn headers(token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            format!("Bearer {token}").parse().unwrap(),
        );
        headers
    }

    async fn body(response: Response) -> Value {
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn model_info_uses_saved_policy_and_reports_preflight_not_connection_success() {
        let dir = tempfile::tempdir().unwrap();
        let policy = PrivacyPolicy {
            model_access: ModelAccess {
                goals: true,
                projects: false,
                captures: false,
            },
            diagnostic_capture: false,
        };
        let state = state(dir.path(), policy.clone());
        let config: ModelConfig = serde_json::from_value(json!({"version":2,"source":{"kind":"dsh","provider":"subscription","model":"fixture"}})).unwrap();
        config.save(dir.path()).unwrap();
        let response = body(model_info(State(state.clone()), headers("ui-token")).await).await;
        assert_eq!(response["schema"], "zodo.model/1");
        assert_eq!(response["config"], serde_json::to_value(&config).unwrap());
        assert_eq!(response["effective"]["selected"]["source"], "dsh");
        assert_eq!(
            response["effective"]["selected"]["reason"],
            "host_unavailable"
        );
        assert_eq!(
            response["effective"]["operations"]["decompose"]["reason"],
            "host_unavailable"
        );
        assert_eq!(
            response["effective"]["operations"]["clarify"]["reason"],
            "captures_not_allowed"
        );
        assert_eq!(response["effective"]["connection_tested"], false);
        assert!(!response.to_string().contains("token"));
        assert_eq!(
            state
                .inner
                .lock()
                .unwrap()
                .store
                .load_world(WORKSPACE_ID)
                .unwrap()
                .unwrap()
                .workspace
                .policy,
            policy
        );
        assert_eq!(
            update_model(
                State(state.clone()),
                headers("agent-token"),
                Json(config.clone())
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        let response =
            body(update_model(State(state.clone()), headers("ui-token"), Json(config)).await).await;
        assert_eq!(
            response["effective"]["selected"]["reason"],
            "host_unavailable"
        );
        assert_eq!(
            model_info(State(state), HeaderMap::new()).await.status(),
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn invalid_model_versions_never_replace_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path(), PrivacyPolicy::default());
        let mut config = ModelConfig::default();
        config.version = 1;
        assert_eq!(
            update_model(State(state), headers("ui-token"), Json(config))
                .await
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert!(!ModelConfig::path(dir.path()).exists());
    }
}
