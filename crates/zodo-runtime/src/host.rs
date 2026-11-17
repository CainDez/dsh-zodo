//! Ephemeral Core -> Host LLM capability. No token is serialized to disk or a public projection.
use crate::{
    ai::{
        ChatMessage, ModelCompletion, ModelConfig, ModelGateway, ModelSource, authorize_class,
        authorize_scope, denied, endpoint_uri, http_agent, is_loopback_endpoint, read_response,
        unavailable,
    },
    runs::RunRecord,
    runtime::{
        AppState, RuntimeInner, authenticate, error_response, require_capability, status_for,
    },
};
use axum::{
    Json,
    extract::State,
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use zodo_kernel::{
    error::Result,
    model::{DataClass, PrivacyPolicy},
    ports::Store,
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostRoute {
    pub provider: String,
    pub model: String,
}

// Deliberately neither Debug nor Serialize: this is a live capability, not data.
#[derive(Clone)]
pub struct HostBinding {
    pub binding_id: String,
    pub endpoint: String,
    pub token: String,
    pub expires_at_ms: i64,
    pub routes: Vec<HostRoute>,
}

#[derive(Default)]
pub struct HostState {
    pub binding: Option<HostBinding>,
    pub model_revision: u64,
    pending: BTreeMap<String, PendingCall>,
}

impl HostState {
    pub fn available(&self, provider: &str, model: &str) -> Result<()> {
        let binding = self
            .binding
            .as_ref()
            .filter(|b| b.expires_at_ms > Utc::now().timestamp_millis())
            .ok_or_else(|| unavailable("host_unavailable"))?;
        if !binding
            .routes
            .iter()
            .any(|r| r.provider == provider && r.model == model)
        {
            return Err(unavailable("host_model_unavailable"));
        }
        Ok(())
    }
}

#[derive(Clone)]
struct PendingCall {
    binding_id: String,
    workspace_id: String,
    run_id: String,
    scope: String,
    config: ModelConfig,
    model_revision: u64,
    policy_revision: u64,
    body_sha256: String,
    expires_at_ms: i64,
    consumed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindRequest {
    schema: String,
    instance_id: String,
    binding_id: String,
    endpoint: String,
    token: String,
    expires_at_ms: i64,
    routes: Vec<HostRoute>,
}

fn host_admin(state: &AppState, headers: &HeaderMap) -> Result<()> {
    // This interface is server-to-server, not a browser endpoint (even an authenticated UI).
    if headers.contains_key("origin") {
        return Err(denied("host_browser_request_denied"));
    }
    let principal = authenticate(state, headers)?;
    require_capability(&principal, "admin")
}

fn bind(inner: &mut RuntimeInner, request: BindRequest) -> Result<()> {
    let now = Utc::now().timestamp_millis();
    let uri = endpoint_uri(&request.endpoint)?;
    if request.schema != "zodo.host-binding/1"
        || request.instance_id != inner.instance_id
        || request.binding_id.len() != 32
        || !request.binding_id.bytes().all(|b| b.is_ascii_hexdigit())
        || request.token.len() != 64
        || !request.token.bytes().all(|b| b.is_ascii_hexdigit())
        || !is_loopback_endpoint(&request.endpoint)
        || uri.scheme_str() != Some("http")
        || uri.path() != "/zodo-host/v1/complete"
        || uri.port_u16().is_none()
        || request.expires_at_ms <= now
        || request.expires_at_ms > now + 600_000
        || request.routes.len() > 4096
        || request.routes.iter().any(|r| {
            r.provider.is_empty()
                || r.model.is_empty()
                || r.provider.len() > 200
                || r.model.len() > 200
        })
    {
        return Err(denied("invalid_host_binding"));
    }
    inner.host.pending.clear();
    inner.host.binding = Some(HostBinding {
        binding_id: request.binding_id,
        endpoint: request.endpoint,
        token: request.token,
        expires_at_ms: request.expires_at_ms,
        routes: request.routes,
    });
    Ok(())
}

pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<BindRequest>,
) -> Response {
    let result = host_admin(&state, &headers).and_then(|_| {
        let mut inner = state
            .inner
            .lock()
            .map_err(|_| unavailable("runtime_unavailable"))?;
        bind(&mut inner, request)
    });
    match result {
        Ok(()) => Json(json!({"schema":"zodo.host-binding/1","bound":true})).into_response(),
        Err(error) => error_response(status_for(&error), "", &error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnbindRequest {
    instance_id: String,
    binding_id: String,
}

pub async fn unregister(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<UnbindRequest>,
) -> Response {
    let result = host_admin(&state, &headers).and_then(|_| {
        let mut inner = state
            .inner
            .lock()
            .map_err(|_| unavailable("runtime_unavailable"))?;
        if request.instance_id != inner.instance_id
            || !inner
                .host
                .binding
                .as_ref()
                .is_some_and(|binding| binding.binding_id == request.binding_id)
        {
            return Err(denied("host_binding_changed"));
        }
        inner.host.binding = None;
        inner.host.pending.clear();
        Ok(())
    });
    match result {
        Ok(()) => Json(json!({"bound":false})).into_response(),
        Err(error) => error_response(status_for(&error), "", &error),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizeRequest {
    instance_id: String,
    binding_id: String,
    request_id: String,
    body_sha256: String,
    phase: String,
}

fn authorize(inner: &mut RuntimeInner, request: &AuthorizeRequest) -> Result<()> {
    let now = Utc::now().timestamp_millis();
    let binding = inner
        .host
        .binding
        .as_ref()
        .ok_or_else(|| denied("host_unavailable"))?;
    let pending = inner
        .host
        .pending
        .get(&request.request_id)
        .cloned()
        .ok_or_else(|| denied("host_request_unknown"))?;
    if request.instance_id != inner.instance_id
        || request.binding_id != binding.binding_id
        || pending.binding_id != binding.binding_id
        || now >= binding.expires_at_ms
        || now >= pending.expires_at_ms
        || request.body_sha256 != pending.body_sha256
        || !matches!(request.phase.as_str(), "dispatch" | "continue")
        || (request.phase == "dispatch" && pending.consumed)
        || (request.phase == "continue" && !pending.consumed)
    {
        return Err(denied("host_request_rejected"));
    }
    let run = inner
        .store
        .get_run(&pending.run_id)?
        .ok_or_else(|| denied("host_run_unavailable"))?;
    if run.workspace_id != pending.workspace_id
        || run.intent != pending.scope
        || run.cancel_requested
        || run.state != "running"
    {
        return Err(denied("host_run_cancelled"));
    }
    let world = inner
        .store
        .load_world(&pending.workspace_id)?
        .ok_or_else(|| denied("host_workspace_unavailable"))?;
    if world.workspace.policy_revision != pending.policy_revision
        || inner.host.model_revision != pending.model_revision
        || ModelConfig::load(&inner.data_dir)? != pending.config
    {
        return Err(denied("host_request_stale"));
    }
    authorize_scope(&world.workspace.policy, &pending.scope)?;
    let ModelSource::Dsh {
        provider: Some(provider),
        model: Some(model),
    } = &pending.config.source
    else {
        return Err(denied("host_selection_changed"));
    };
    inner.host.available(provider, model)?;
    if request.phase == "dispatch" {
        inner
            .host
            .pending
            .get_mut(&request.request_id)
            .unwrap()
            .consumed = true;
    }
    Ok(())
}

pub async fn check(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<AuthorizeRequest>,
) -> Response {
    let result = host_admin(&state, &headers).and_then(|_| {
        let mut inner = state
            .inner
            .lock()
            .map_err(|_| unavailable("runtime_unavailable"))?;
        authorize(&mut inner, &request)
    });
    match result {
        Ok(()) => Json(json!({"authorized":true})).into_response(),
        Err(error) => error_response(status_for(&error), "", &error),
    }
}

/// Only called by Core's fixed run producers, never exposed as a generic browser/agent completion API.
pub(crate) fn complete_run(
    state: &AppState,
    run: &RunRecord,
    policy: &PrivacyPolicy,
    policy_revision: u64,
    class: &DataClass,
    messages: &[ChatMessage],
) -> Result<ModelCompletion> {
    authorize_scope(policy, &run.intent)?;
    if messages.len() != 2
        || messages[0].role != "system"
        || messages[1].role != "user"
        || messages.iter().map(|m| m.content.len()).sum::<usize>() > 64 * 1024
    {
        return Err(denied("invalid_model_messages"));
    }
    let (gateway, model_revision, dispatch) = {
        let mut inner = state
            .inner
            .lock()
            .map_err(|_| unavailable("runtime_unavailable"))?;
        let world = inner
            .store
            .load_world(&run.workspace_id)?
            .ok_or_else(|| denied("host_workspace_unavailable"))?;
        let current_run = inner
            .store
            .get_run(&run.id)?
            .ok_or_else(|| denied("host_run_unavailable"))?;
        if current_run.cancel_requested || current_run.state != "running" {
            return Err(denied("host_run_cancelled"));
        }
        if world.workspace.policy_revision != policy_revision || &world.workspace.policy != policy {
            return Err(denied("host_request_stale"));
        }
        let gateway = ModelGateway::load(&inner.data_dir)?;
        authorize_class(&gateway.config, class)?;
        gateway.preflight(&inner.host)?;
        let dispatch = if let ModelSource::Dsh {
            provider: Some(provider),
            model: Some(model),
        } = &gateway.config.source
        {
            let binding = inner.host.binding.as_ref().unwrap().clone();
            let request_id = uuid::Uuid::new_v4().simple().to_string();
            let expires_at_ms = (Utc::now().timestamp_millis() + 60_000).min(binding.expires_at_ms);
            let body = serde_json::to_string(&json!({
                "schema":"zodo.host-call/1", "instance_id":inner.instance_id, "binding_id":binding.binding_id,
                "request_id":request_id, "run_id":run.id, "workspace_id":run.workspace_id,
                "provider":provider,"model":model,"scope":run.intent,"data_class":class,
                "policy_revision":policy_revision,"model_revision":inner.host.model_revision,
                "expires_at_ms":expires_at_ms,"messages":messages
            })).map_err(|_| unavailable("host_request_encode_failed"))?;
            let pending = PendingCall {
                binding_id: binding.binding_id.clone(),
                workspace_id: run.workspace_id.clone(),
                run_id: run.id.clone(),
                scope: run.intent.clone(),
                config: gateway.config.clone(),
                model_revision: inner.host.model_revision,
                policy_revision,
                body_sha256: hex::encode(Sha256::digest(body.as_bytes())),
                expires_at_ms,
                consumed: false,
            };
            inner
                .host
                .pending
                .retain(|_, value| value.expires_at_ms > Utc::now().timestamp_millis());
            inner.host.pending.insert(request_id.clone(), pending);
            Some((binding, request_id, body, provider.clone(), model.clone()))
        } else {
            None
        };
        (gateway, inner.host.model_revision, dispatch)
    };
    let Some((binding, request_id, body, provider, model)) = dispatch else {
        let state = state.clone();
        let run = run.clone();
        let config = gateway.config.clone();
        let guard = std::sync::Arc::new(move || -> Result<()> {
            let mut inner = state
                .inner
                .lock()
                .map_err(|_| unavailable("runtime_unavailable"))?;
            let current = inner
                .store
                .get_run(&run.id)?
                .ok_or_else(|| denied("model_run_unavailable"))?;
            let world = inner
                .store
                .load_world(&run.workspace_id)?
                .ok_or_else(|| denied("model_workspace_unavailable"))?;
            if current.cancel_requested || current.state != "running" {
                return Err(denied("model_run_cancelled"));
            }
            if inner.host.model_revision != model_revision
                || ModelConfig::load(&inner.data_dir)? != config
                || world.workspace.policy_revision != policy_revision
            {
                return Err(denied("model_request_stale"));
            }
            authorize_scope(&world.workspace.policy, &run.intent)
        });
        return gateway.complete_custom(class, messages, guard);
    };
    let start = std::time::Instant::now();
    let result = (|| {
        let response = http_agent(true)
            .post(&binding.endpoint)
            .set("Content-Type", "application/json")
            .set("Authorization", &format!("Bearer {}", binding.token))
            .send_string(&body)
            .map_err(|_| unavailable("host_model_request_failed"))?;
        let value = read_response(response)?;
        let content = value
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| unavailable("host_response_missing_content"))?;
        Ok(ModelCompletion {
            provider_id: provider,
            model,
            source: "dsh".into(),
            content: content.into(),
            elapsed_ms: start.elapsed().as_millis(),
        })
    })();
    if let Ok(mut inner) = state.inner.lock() {
        inner.host.pending.remove(&request_id);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::AuthPrincipal;
    use crate::store::SqliteStore;
    use std::{collections::BTreeSet, fs::File};
    use zodo_kernel::model::{ModelAccess, PrincipalKind, Workspace, World};

    fn fixture() -> (tempfile::TempDir, RuntimeInner, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SqliteStore::open(dir.path().join("fixture.sqlite3")).unwrap();
        let mut world = World::new(Workspace::new(
            "ws_test".into(),
            "Synthetic".into(),
            Utc::now(),
        ));
        world.workspace.policy.model_access = ModelAccess {
            goals: true,
            projects: false,
            captures: false,
        };
        store.create_workspace(&world).unwrap();
        let run = store
            .create_run("ws_test", "goal.decompose", &json!({"goal_id":"g"}))
            .unwrap();
        store.claim_next_run("ws_test", "test").unwrap();
        let config: ModelConfig = serde_json::from_value(
            json!({"version":2,"source":{"kind":"dsh","provider":"subscription","model":"test"}}),
        )
        .unwrap();
        config.save(dir.path()).unwrap();
        let mut inner = RuntimeInner {
            store,
            instance_id: "instance".into(),
            workspace_id: "ws_test".into(),
            started_at: Utc::now(),
            tokens: BTreeMap::new(),
            revoked_channels: BTreeSet::new(),
            revoked_devices: BTreeSet::new(),
            data_dir: dir.path().into(),
            host: HostState::default(),
            _lock: File::create(dir.path().join("lock")).unwrap(),
            _lock_path: dir.path().join("lock"),
            _info_path: dir.path().join("runtime.json"),
        };
        bind(&mut inner, binding()).unwrap();
        let id = "c".repeat(32);
        inner.host.pending.insert(
            id.clone(),
            PendingCall {
                binding_id: "a".repeat(32),
                workspace_id: "ws_test".into(),
                run_id: run.id.clone(),
                scope: "goal.decompose".into(),
                config,
                model_revision: 0,
                policy_revision: 1,
                body_sha256: "digest".into(),
                expires_at_ms: Utc::now().timestamp_millis() + 60_000,
                consumed: false,
            },
        );
        (dir, inner, id, run.id)
    }
    fn binding() -> BindRequest {
        BindRequest {
            schema: "zodo.host-binding/1".into(),
            instance_id: "instance".into(),
            binding_id: "a".repeat(32),
            endpoint: "http://127.0.0.1:1234/zodo-host/v1/complete".into(),
            token: "b".repeat(64),
            expires_at_ms: Utc::now().timestamp_millis() + 300_000,
            routes: vec![HostRoute {
                provider: "subscription".into(),
                model: "test".into(),
            }],
        }
    }
    fn request(id: &str) -> AuthorizeRequest {
        AuthorizeRequest {
            instance_id: "instance".into(),
            binding_id: "a".repeat(32),
            request_id: id.into(),
            body_sha256: "digest".into(),
            phase: "dispatch".into(),
        }
    }

    #[test]
    fn callback_authorization_is_one_shot_and_instance_hash_scope_bound() {
        let (_dir, mut inner, id, _) = fixture();
        for mutate in 0..5 {
            let mut input = request(&id);
            match mutate {
                0 => input.instance_id = "other".into(),
                1 => input.binding_id = "other".into(),
                2 => input.body_sha256 = "forged".into(),
                3 => input.phase = "unknown".into(),
                _ => input.request_id = "unknown".into(),
            }
            assert!(authorize(&mut inner, &input).is_err());
        }
        assert!(!inner.host.pending[&id].consumed);
        authorize(&mut inner, &request(&id)).unwrap();
        assert!(authorize(&mut inner, &request(&id)).is_err());
        let mut input = request(&id);
        input.phase = "continue".into();
        authorize(&mut inner, &input).unwrap();
    }

    #[test]
    fn callback_rejects_expiry_cancel_model_and_policy_changes() {
        for scenario in 0..6 {
            let (dir, mut inner, id, run_id) = fixture();
            match scenario {
                0 => inner.host.pending.get_mut(&id).unwrap().expires_at_ms = 0,
                1 => {
                    inner.store.request_run_cancel(&run_id).unwrap();
                }
                2 => inner.host.model_revision += 1,
                3 => {
                    ModelConfig::default().save(dir.path()).unwrap();
                }
                4 => inner.host.pending.get_mut(&id).unwrap().policy_revision += 1,
                _ => inner.host.pending.get_mut(&id).unwrap().scope = "capture.clarify".into(),
            }
            assert!(authorize(&mut inner, &request(&id)).is_err());
        }
    }

    #[test]
    fn bindings_are_ephemeral_expiring_loopback_capabilities() {
        let (_dir, mut inner, _, _) = fixture();
        for endpoint in [
            "http://example.invalid/zodo-host/v1/complete",
            "http://127.0.0.1:1234/other",
            "https://127.0.0.1:1234/zodo-host/v1/complete",
            "http://secret@127.0.0.1:1234/zodo-host/v1/complete",
        ] {
            let mut input = binding();
            input.endpoint = endpoint.into();
            assert!(bind(&mut inner, input).is_err());
        }
        let mut input = binding();
        input.expires_at_ms = Utc::now().timestamp_millis() + 601_000;
        assert!(bind(&mut inner, input).is_err());
        let mut input = binding();
        input.instance_id = "other".into();
        assert!(bind(&mut inner, input).is_err());
        let mut input = binding();
        input.binding_id = "d".repeat(32);
        bind(&mut inner, input).unwrap();
        assert!(inner.host.pending.is_empty());
        assert!(
            HostState::default()
                .available("subscription", "test")
                .is_err()
        );
    }

    #[test]
    fn host_endpoints_require_admin_and_refuse_browser_origin() {
        let (_dir, mut inner, _, _) = fixture();
        for (token, caps) in [
            ("agent", vec!["read".into(), "propose".into()]),
            ("admin", vec!["admin".into()]),
        ] {
            inner.tokens.insert(
                token.into(),
                AuthPrincipal {
                    token: token.into(),
                    principal_id: token.into(),
                    kind: PrincipalKind::Owner,
                    channel: "test".into(),
                    capabilities: caps,
                    workspace_id: "ws_test".into(),
                },
            );
        }
        let state = AppState {
            inner: std::sync::Arc::new(std::sync::Mutex::new(inner)),
        };
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer agent".parse().unwrap());
        assert!(host_admin(&state, &headers).is_err());
        headers.insert("authorization", "Bearer admin".parse().unwrap());
        host_admin(&state, &headers).unwrap();
        headers.insert("origin", "http://127.0.0.1:1".parse().unwrap());
        assert!(host_admin(&state, &headers).is_err());
    }
}
