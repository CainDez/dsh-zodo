use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zodo_kernel::error::{ErrorCode, Result, ZodoError};
use zodo_kernel::model::{ModelRoute, PrivacyPolicy};

pub const MODEL_CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProvider {
    pub id: String,
    pub label: String,
    /// OpenAI-compatible base URL. The gateway appends `/chat/completions`.
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// `device` or `cloud`.
    pub location: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub version: u32,
    pub local: ModelProvider,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud: Option<ModelProvider>,
    /// Optional role routing: role name -> `local` or `cloud`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub roles: BTreeMap<String, String>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        let local_model = std::env::var("ZODO_LOCAL_MODEL")
            .unwrap_or_else(|_| "aravhawk/qwen3.5-opus-4.6:latest".to_string());
        let local_url = std::env::var("ZODO_LOCAL_MODEL_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:11434/v1".to_string());
        Self {
            version: MODEL_CONFIG_VERSION,
            local: ModelProvider {
                id: "local-ollama".to_string(),
                label: "本地 Ollama".to_string(),
                base_url: local_url,
                model: local_model,
                api_key_env: Some("OLLAMA_API_KEY".to_string()),
                location: "device".to_string(),
                enabled: true,
            },
            roles: BTreeMap::new(),
            cloud: Some(ModelProvider {
                id: "deepseek-cloud".to_string(),
                label: "DeepSeek 云端".to_string(),
                base_url: "https://api.deepseek.com/v1".to_string(),
                model: "deepseek-chat".to_string(),
                api_key_env: Some("DEEPSEEK_API_KEY".to_string()),
                location: "cloud".to_string(),
                enabled: false,
            }),
        }
    }
}

impl ModelConfig {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("model.json")
    }

    pub fn load(data_dir: &Path) -> Self {
        let path = Self::path(data_dir);
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Self>(&text).ok())
            .filter(|config| config.version == MODEL_CONFIG_VERSION)
            .unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        let path = Self::path(data_dir);
        let temporary = path.with_extension("tmp");
        let body = serde_json::to_vec_pretty(self)
            .map_err(|error| ZodoError::new(ErrorCode::StorageFailure, error.to_string()))?;
        std::fs::write(&temporary, body)
            .and_then(|_| std::fs::rename(&temporary, &path))
            .map_err(|error| ZodoError::new(ErrorCode::StorageFailure, error.to_string()))
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != MODEL_CONFIG_VERSION {
            return Err(ZodoError::validation("unsupported model config version"));
        }
        validate_provider(&self.local, "device")?;
        if let Some(cloud) = &self.cloud {
            validate_provider(cloud, "cloud")?;
        }
        for (role, route) in &self.roles {
            if role.trim().is_empty() || role.len() > 80 {
                return Err(ZodoError::validation(
                    "model role names must be 1..80 chars",
                ));
            }
            if route != "local" && route != "cloud" {
                return Err(ZodoError::validation(
                    "model role routes must be `local` or `cloud`",
                ));
            }
        }
        Ok(())
    }

    /// Browser-safe projection: no environment values, only whether a credential is configured.
    pub fn sanitized(&self) -> Value {
        let local = provider_view(&self.local);
        let cloud = self.cloud.as_ref().map(provider_view);
        json!({
            "version": self.version,
            "local": local,
            "cloud": cloud,
            "editable": serde_json::to_value(self).unwrap_or(Value::Null),
            "policy_note": "模型配置只保存环境变量名；API key 本身不进入此文件、浏览器或证据。",
        })
    }

    /// Finite provider/action catalog surfaced to clients. This is descriptive; route enforcement
    /// remains in `ModelGateway::route_for` and the Core command/run validation.
    pub fn catalog(&self) -> Value {
        let providers = std::iter::once(provider_view(&self.local))
            .chain(self.cloud.as_ref().map(provider_view))
            .collect::<Vec<_>>();
        json!({
            "schema": "zodo.model-catalog/1",
            "catalog_version": self.version,
            "providers": providers,
            "actions": [
                {
                    "id": "chat.completions",
                    "data_classes": ["public", "workspace"],
                    "budgets": {"max_tasks_per_run": 12, "max_questions": 8, "timeout_seconds": 60},
                    "authorization": {"local": "enabled_provider", "cloud": "policy_cloud_assisted_explicit_provider_and_credential"},
                    "failures": ["PROVIDER_UNAVAILABLE", "PERMISSION_DENIED", "INVALID_MODEL_OUTPUT"]
                },
                {
                    "id": "goal.decompose",
                    "budget": {"tasks": "3..7", "estimates_minutes": "15..240"},
                    "produces": "proposal_only"
                },
                {
                    "id": "capture.clarify",
                    "budget": {"questions_max": 4, "input_chars_max": 4000},
                    "produces": "run_output_only"
                }
            ]
        })
    }

    pub fn provider(&self, id: &str) -> Option<&ModelProvider> {
        if self.local.id == id {
            return Some(&self.local);
        }
        self.cloud.as_ref().filter(|provider| provider.id == id)
    }
}

fn validate_provider(provider: &ModelProvider, expected_location: &str) -> Result<()> {
    if provider.id.trim().is_empty() || provider.model.trim().is_empty() {
        return Err(ZodoError::validation(
            "model provider id and model are required",
        ));
    }
    if provider.location != expected_location {
        return Err(ZodoError::validation(format!(
            "provider {} must have location {expected_location}",
            provider.id
        )));
    }
    if !(provider.base_url.starts_with("http://") || provider.base_url.starts_with("https://")) {
        return Err(ZodoError::validation("model base_url must be http(s)"));
    }
    let uri: axum::http::Uri = provider
        .base_url
        .parse()
        .map_err(|_| ZodoError::validation("invalid model base_url"))?;
    let authority = uri
        .authority()
        .ok_or_else(|| ZodoError::validation("model base_url requires a host"))?;
    if authority.as_str().contains('@') || provider.base_url.contains('#') {
        return Err(ZodoError::validation(
            "model base_url must not contain credentials or fragments",
        ));
    }
    if expected_location == "device" {
        let host = authority
            .host()
            .trim_start_matches('[')
            .trim_end_matches(']');
        if !(host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback()))
            || uri.query().is_some()
        {
            return Err(ZodoError::validation(
                "local model endpoint must use a loopback IP or localhost without a query",
            ));
        }
    }
    if provider.base_url.len() > 500 || provider.model.len() > 200 {
        return Err(ZodoError::validation("model provider strings are too long"));
    }
    Ok(())
}

fn provider_credential(provider: &ModelProvider) -> Option<String> {
    provider.api_key_env.as_deref().and_then(|name| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    })
}

fn credential_ready(provider: &ModelProvider) -> bool {
    provider.location == "device" || provider_credential(provider).is_some()
}

// Local DNS is not consulted: localhost is explicitly pinned to loopback, and all
// other accepted hosts must be literal loopback IPs. Proxies/redirects cannot escape.
fn loopback_resolve(netloc: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
    let address = if let Some((host, port)) = netloc.rsplit_once(':') {
        if host.eq_ignore_ascii_case("localhost") {
            format!("127.0.0.1:{port}")
        } else {
            netloc.to_string()
        }
    } else {
        netloc.to_string()
    };
    match address.parse::<std::net::SocketAddr>() {
        Ok(address) if address.ip().is_loopback() => Ok(vec![address]),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "local model address must be loopback",
        )),
    }
}

fn provider_agent(provider: &ModelProvider) -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new()
        .redirects(0)
        .try_proxy_from_env(false)
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(120))
        .timeout_write(Duration::from_secs(15));
    if provider.location == "device" {
        builder = builder.resolver(loopback_resolve);
    }
    builder.build()
}

fn provider_view(provider: &ModelProvider) -> Value {
    let credential_present = provider_credential(provider).is_some();
    json!({
        "id": provider.id,
        "label": provider.label,
        "base_url": provider.base_url,
        "model": provider.model,
        "location": provider.location,
        "enabled": provider.enabled,
        "credential_env": provider.api_key_env,
        "credential_present": credential_present,
        "credential_ready": credential_ready(provider),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCompletion {
    pub provider_id: String,
    pub model: String,
    pub route: String,
    pub content: String,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone)]
pub struct ModelGateway {
    pub config: ModelConfig,
    data_dir: PathBuf,
}

struct RouteDecision<'a> {
    provider: &'a ModelProvider,
    reason: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSensitivity {
    DeviceOnly,
    Workspace,
    Public,
}

impl ModelGateway {
    pub fn load(data_dir: &Path) -> Self {
        Self {
            config: ModelConfig::load(data_dir),
            data_dir: data_dir.to_path_buf(),
        }
    }

    pub fn reload(&mut self) {
        self.config = ModelConfig::load(&self.data_dir);
    }

    pub fn configure(&mut self, config: ModelConfig) -> Result<()> {
        config.validate()?;
        config.save(&self.data_dir)?;
        self.config = config;
        Ok(())
    }

    pub fn route_for(
        &self,
        policy: &PrivacyPolicy,
        sensitivity: DataSensitivity,
        role: Option<&str>,
    ) -> Result<&ModelProvider> {
        self.route_decision(policy, sensitivity, role, credential_ready)
            .map(|decision| decision.provider)
    }

    // A single preflight is shared by execution and the browser-safe status projection.
    // The credential predicate keeps policy tests deterministic without mutating process env.
    fn route_decision(
        &self,
        policy: &PrivacyPolicy,
        sensitivity: DataSensitivity,
        role: Option<&str>,
        credentials: impl Fn(&ModelProvider) -> bool,
    ) -> Result<RouteDecision<'_>> {
        let preference = role.and_then(|name| self.config.roles.get(name));
        let reason = if policy.model_route == ModelRoute::LocalOnly {
            Some("local_only")
        } else if sensitivity == DataSensitivity::DeviceOnly {
            Some("device_only")
        } else if preference.is_some_and(|value| value == "local") {
            Some("role_local")
        } else {
            None
        };
        if let Some(reason) = reason {
            return self.local_decision(reason);
        }
        let Some(cloud) = self.config.cloud.as_ref() else {
            return self.local_decision("cloud_provider_missing");
        };
        if !cloud.enabled {
            return self.local_decision("cloud_provider_disabled");
        }
        // Cloud-assisted consent must bind this exact provider ID.
        let allowed = policy
            .allowed_cloud_providers
            .iter()
            .any(|id| id == &cloud.id);
        if !allowed {
            return self.local_decision("cloud_provider_not_allowed");
        }
        if validate_provider(cloud, "cloud").is_err() {
            return self.local_decision("cloud_provider_invalid");
        }
        if !credentials(cloud) {
            return self.local_decision("cloud_credentials_missing");
        }
        Ok(RouteDecision {
            provider: cloud,
            reason: "cloud_authorized",
        })
    }

    fn local_decision(&self, reason: &'static str) -> Result<RouteDecision<'_>> {
        if !self.config.local.enabled {
            return Err(
                ZodoError::new(ErrorCode::ProviderUnavailable, "local_provider_disabled")
                    .with_details(json!({"fallback_reason": reason})),
            );
        }
        if validate_provider(&self.config.local, "device").is_err() {
            return Err(
                ZodoError::new(ErrorCode::ProviderUnavailable, "local_endpoint_invalid")
                    .with_details(json!({"fallback_reason": reason})),
            );
        }
        Ok(RouteDecision {
            provider: &self.config.local,
            reason,
        })
    }

    fn route_status(
        &self,
        policy: &PrivacyPolicy,
        sensitivity: DataSensitivity,
        role: Option<&str>,
    ) -> Value {
        match self.route_decision(policy, sensitivity, role, credential_ready) {
            Ok(decision) => json!({
                "route": decision.provider.location,
                "provider_id": decision.provider.id,
                "reason": decision.reason,
                "ready": true,
            }),
            Err(error) => json!({
                "route": "unavailable", "provider_id": null, "ready": false,
                "reason": error.message, "error_code": error.code,
                "fallback_reason": error.details.get("fallback_reason"),
            }),
        }
    }

    /// No connection is made: ready means configuration/credential preflight only.
    pub fn effective(&self, policy: &PrivacyPolicy) -> Value {
        json!({
            "mode": policy.model_route,
            "connection_tested": false,
            "connection_status": "not_tested",
            "workspace": self.route_status(policy, DataSensitivity::Workspace, None),
            "device_only": self.route_status(policy, DataSensitivity::DeviceOnly, None),
            "roles": {
                "decompose": self.route_status(policy, DataSensitivity::Workspace, Some("decompose")),
                "clarify": self.route_status(policy, DataSensitivity::Workspace, Some("clarify")),
            },
        })
    }

    pub fn complete(
        &self,
        policy: &PrivacyPolicy,
        sensitivity: DataSensitivity,
        role: Option<&str>,
        messages: &[ChatMessage],
    ) -> Result<ModelCompletion> {
        let provider = self.route_for(policy, sensitivity, role)?;
        let started = std::time::Instant::now();
        let base = provider.base_url.trim_end_matches('/');
        let url = if base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/chat/completions")
        };
        let api_key = provider_credential(provider);
        // Recheck the actual key used by this request, before constructing/sending any HTTP.
        if provider.location == "cloud" && api_key.is_none() {
            return Err(ZodoError::new(
                ErrorCode::ProviderUnavailable,
                "cloud_credentials_missing",
            ));
        }
        let body = json!({
            "model": provider.model,
            "messages": messages,
            "temperature": 0.1,
            "stream": false,
            "response_format": {"type": "json_object"},
            "format": "json",
        });
        let agent = provider_agent(provider);
        let mut request = agent
            .post(&url)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json");
        if let Some(key) = api_key.as_deref() {
            request = request.set("Authorization", &format!("Bearer {key}"));
        }
        let response = request.send_json(body).map_err(|error| match error {
            ureq::Error::Status(status, response) => {
                let detail = response.into_string().unwrap_or_default();
                ZodoError::new(
                    ErrorCode::ProviderUnavailable,
                    format!(
                        "model provider {} returned HTTP {status}: {}",
                        provider.id,
                        detail.chars().take(500).collect::<String>()
                    ),
                )
            }
            other => ZodoError::new(
                ErrorCode::ProviderUnavailable,
                format!("model provider {} is unavailable: {other}", provider.id),
            ),
        })?;
        let value: Value = response.into_json().map_err(|error| {
            ZodoError::new(
                ErrorCode::ProviderUnavailable,
                format!("model provider returned invalid JSON: {error}"),
            )
        })?;
        let content = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ZodoError::new(
                    ErrorCode::ProviderUnavailable,
                    "model provider returned no assistant content",
                )
            })?;
        Ok(ModelCompletion {
            provider_id: provider.id.clone(),
            model: provider.model.clone(),
            route: provider.location.clone(),
            content: content.to_string(),
            elapsed_ms: started.elapsed().as_millis(),
        })
    }
}

/// Extract a JSON object from a model response that may be wrapped in prose or a code fence.
pub fn parse_json_object(content: &str) -> Result<Value> {
    let trimmed = content.trim();
    let unfenced = if trimmed.starts_with("```") {
        let without_start = trimmed
            .strip_prefix("```json")
            .or_else(|| trimmed.strip_prefix("```"))
            .unwrap_or(trimmed);
        without_start
            .strip_suffix("```")
            .unwrap_or(without_start)
            .trim()
    } else {
        trimmed
    };
    if let Ok(value) = serde_json::from_str::<Value>(unfenced) {
        if value.is_object() {
            return Ok(value);
        }
    }
    let start = unfenced.find('{');
    let end = unfenced.rfind('}');
    if let (Some(start), Some(end)) = (start, end) {
        if end > start {
            return serde_json::from_str::<Value>(&unfenced[start..=end]).map_err(|error| {
                ZodoError::validation(format!("model did not return valid JSON: {error}"))
            });
        }
    }
    Err(ZodoError::validation(
        "model response did not contain a JSON object",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_model_json() {
        let value = parse_json_object("```json\n{\"tasks\":[{\"title\":\"a\"}]}\n```").unwrap();
        assert_eq!(value["tasks"][0]["title"], "a");
    }

    fn gateway() -> ModelGateway {
        let mut config = ModelConfig::default();
        config.local.base_url = "http://127.0.0.1:11434/v1".into();
        config.cloud.as_mut().unwrap().enabled = true;
        ModelGateway {
            config,
            data_dir: PathBuf::new(),
        }
    }

    fn assisted_policy() -> PrivacyPolicy {
        PrivacyPolicy {
            model_route: ModelRoute::CloudAssisted,
            allowed_cloud_providers: vec!["deepseek-cloud".into()],
            diagnostic_capture: false,
        }
    }

    #[test]
    fn routing_matrix_preserves_mode_sensitivity_role_and_explicit_consent() {
        let mut gateway = gateway();
        gateway
            .config
            .roles
            .insert("cloud_role".into(), "cloud".into());
        gateway
            .config
            .roles
            .insert("local_role".into(), "local".into());
        let mut cases = 0;
        for mode in [ModelRoute::LocalOnly, ModelRoute::CloudAssisted] {
            for sensitivity in [
                DataSensitivity::DeviceOnly,
                DataSensitivity::Workspace,
                DataSensitivity::Public,
            ] {
                for role in [None, Some("cloud_role"), Some("local_role")] {
                    for allowlist in [
                        vec![],
                        vec!["deepseek-cloud".into()],
                        vec!["other".into()],
                        vec!["*".into()],
                    ] {
                        for credentials in [false, true] {
                            for local_enabled in [false, true] {
                                gateway.config.local.enabled = local_enabled;
                                let policy = PrivacyPolicy {
                                    model_route: mode.clone(),
                                    allowed_cloud_providers: allowlist.clone(),
                                    diagnostic_capture: false,
                                };
                                let expected_cloud = mode == ModelRoute::CloudAssisted
                                    && sensitivity != DataSensitivity::DeviceOnly
                                    && role != Some("local_role")
                                    && allowlist.iter().any(|id| id == "deepseek-cloud")
                                    && credentials;
                                let actual =
                                    gateway.route_decision(&policy, sensitivity, role, |_| {
                                        credentials
                                    });
                                if expected_cloud || local_enabled {
                                    let actual = actual.unwrap();
                                    assert_eq!(
                                        actual.provider.location,
                                        if expected_cloud { "cloud" } else { "device" }
                                    );
                                } else {
                                    assert_eq!(
                                        actual.err().unwrap().code,
                                        ErrorCode::ProviderUnavailable
                                    );
                                }
                                cases += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 288);
    }

    #[test]
    fn unavailable_cloud_falls_back_before_any_request_or_errors_if_local_disabled() {
        let mut gateway = gateway();
        let policy = assisted_policy();
        gateway.config.cloud = None;
        assert_eq!(
            gateway
                .route_decision(&policy, DataSensitivity::Workspace, None, |_| true)
                .unwrap()
                .reason,
            "cloud_provider_missing"
        );
        gateway.config.cloud = ModelConfig::default().cloud;
        assert_eq!(
            gateway
                .route_decision(&policy, DataSensitivity::Workspace, None, |_| true)
                .unwrap()
                .reason,
            "cloud_provider_disabled"
        );
        gateway.config.cloud.as_mut().unwrap().enabled = true;
        gateway.config.cloud.as_mut().unwrap().api_key_env = None;
        assert_eq!(
            gateway
                .route_for(&policy, DataSensitivity::Workspace, None)
                .unwrap()
                .location,
            "device"
        );
        let status = gateway.effective(&policy);
        assert_eq!(status["workspace"]["reason"], "cloud_credentials_missing");
        assert_eq!(status["connection_tested"], false);
        assert_eq!(status["connection_status"], "not_tested");
        assert_eq!(status["workspace"]["ready"], true);
        let view = gateway.config.sanitized();
        assert_eq!(view["local"]["credential_ready"], true);
        assert_eq!(view["cloud"]["credential_ready"], false);
        gateway.config.local.enabled = false;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        gateway.config.cloud.as_mut().unwrap().base_url =
            format!("http://{}/v1", listener.local_addr().unwrap());
        let error = gateway
            .complete(
                &policy,
                DataSensitivity::Workspace,
                None,
                &[ChatMessage::user("synthetic")],
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::ProviderUnavailable);
        assert_eq!(
            error.details["fallback_reason"],
            "cloud_credentials_missing"
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        assert_eq!(gateway.effective(&policy)["workspace"]["ready"], false);
    }

    #[test]
    fn local_endpoints_are_validated_on_save_and_on_use() {
        let mut gateway = gateway();
        for url in [
            "http://127.0.0.1:11434/v1",
            "http://127.0.0.2/v1",
            "https://localhost/v1",
            "http://[::1]:123/v1",
        ] {
            gateway.config.local.base_url = url.into();
            assert!(gateway.config.validate().is_ok(), "{url}");
            assert!(
                gateway
                    .route_for(&PrivacyPolicy::default(), DataSensitivity::DeviceOnly, None)
                    .is_ok(),
                "{url}"
            );
        }
        for url in [
            "https://example.com/v1",
            "http://192.168.1.2/v1",
            "http://0.0.0.0/v1",
            "http://localhost.example.com/v1",
            "http://127.0.0.1@example.com/v1",
            "http://127.0.0.1/v1?url=https://example.com",
            "http://127.0.0.1/v1#fragment",
            "file:///tmp/model",
            "http://2130706433/v1",
            "http://127.1/v1",
            "http://[::ffff:127.0.0.1]/v1",
        ] {
            gateway.config.local.base_url = url.into();
            assert!(gateway.config.validate().is_err(), "{url}");
            assert!(
                gateway
                    .route_for(&PrivacyPolicy::default(), DataSensitivity::DeviceOnly, None)
                    .is_err(),
                "{url}"
            );
        }
        assert_eq!(
            loopback_resolve("localhost:123").unwrap()[0].to_string(),
            "127.0.0.1:123"
        );
        assert!(loopback_resolve("[::1]:123").is_ok());
        assert!(loopback_resolve("192.168.1.2:123").is_err());
        assert!(loopback_resolve("example.com:123").is_err());
    }

    // Every network test uses ephemeral loopback fixtures with synthetic payloads only.
    fn fixture(response: String) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("fixture did not receive request: {error}"),
                }
            };
            // Windows may inherit nonblocking mode from the listening socket.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 2048];
            loop {
                let size = stream.read(&mut buffer).unwrap();
                assert!(size > 0);
                request.extend_from_slice(&buffer[..size]);
                let text = String::from_utf8_lossy(&request);
                if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            key.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if body.len() >= length {
                        break;
                    }
                }
            }
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8(request).unwrap()
        });
        (url, handle)
    }

    #[test]
    fn cloud_missing_key_uses_local_without_contacting_cloud() {
        let body = r#"{"choices":[{"message":{"content":"{}"}}]}"#;
        let (url, fixture) = fixture(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ));
        let cloud = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        cloud.set_nonblocking(true).unwrap();
        let mut gateway = gateway();
        gateway.config.local.base_url = url;
        gateway.config.local.api_key_env = None;
        gateway.config.cloud.as_mut().unwrap().base_url =
            format!("http://{}/v1", cloud.local_addr().unwrap());
        gateway.config.cloud.as_mut().unwrap().api_key_env = None;
        let completion = gateway.complete(
            &assisted_policy(),
            DataSensitivity::Workspace,
            Some("decompose"),
            &[ChatMessage::user("synthetic")],
        );
        let request = fixture.join().unwrap();
        assert_eq!(completion.unwrap().route, "device");
        assert!(request.contains("synthetic"));
        assert_eq!(
            cloud.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn device_only_uses_local_even_with_cloud_pin_and_ready_cloud() {
        let body = r#"{"choices":[{"message":{"content":"{}"}}]}"#;
        let (url, fixture) = fixture(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ));
        let cloud = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        cloud.set_nonblocking(true).unwrap();
        let mut gateway = gateway();
        gateway.config.local.base_url = url;
        gateway.config.local.api_key_env = None;
        gateway
            .config
            .roles
            .insert("decompose".into(), "cloud".into());
        gateway.config.cloud.as_mut().unwrap().base_url =
            format!("http://{}/v1", cloud.local_addr().unwrap());
        let decision = gateway
            .route_decision(
                &assisted_policy(),
                DataSensitivity::DeviceOnly,
                Some("decompose"),
                |_| panic!("device-only must never consult cloud credentials"),
            )
            .unwrap();
        assert_eq!(decision.reason, "device_only");
        let completion = gateway.complete(
            &assisted_policy(),
            DataSensitivity::DeviceOnly,
            Some("decompose"),
            &[ChatMessage::user("synthetic-private")],
        );
        let request = fixture.join().unwrap();
        assert_eq!(completion.unwrap().route, "device");
        assert!(request.contains("synthetic-private"));
        assert_eq!(
            cloud.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    fn local_redirect_is_not_followed_and_cannot_leak_payload() {
        let target = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let (url, fixture) = fixture(format!(
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{}/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            target.local_addr().unwrap()
        ));
        let mut gateway = gateway();
        gateway.config.local.base_url = url;
        gateway.config.local.api_key_env = None;
        let result = gateway.complete(
            &PrivacyPolicy::default(),
            DataSensitivity::DeviceOnly,
            None,
            &[ChatMessage::user("synthetic-private")],
        );
        let request = fixture.join().unwrap();
        assert!(request.contains("synthetic-private"));
        assert!(result.is_err());
        assert_eq!(
            target.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}
