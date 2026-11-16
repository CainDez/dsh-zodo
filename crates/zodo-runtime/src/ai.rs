use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zodo_kernel::error::{ErrorCode, Result, ZodoError};
use zodo_kernel::model::{DataClass, ModelRoute, PrivacyPolicy};

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
                    "authorization": {"local": "enabled_provider", "cloud": "policy_cloud_allowed_and_credential"},
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
    if provider.base_url.len() > 500 || provider.model.len() > 200 {
        return Err(ZodoError::validation("model provider strings are too long"));
    }
    Ok(())
}

fn provider_view(provider: &ModelProvider) -> Value {
    let credential_present = provider.api_key_env.as_deref().is_some_and(|name| {
        std::env::var(name)
            .map(|value| !value.is_empty())
            .unwrap_or(false)
    });
    json!({
        "id": provider.id,
        "label": provider.label,
        "base_url": provider.base_url,
        "model": provider.model,
        "location": provider.location,
        "enabled": provider.enabled,
        "credential_env": provider.api_key_env,
        "credential_present": credential_present,
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
        if let Some(preference) = role.and_then(|name| self.config.roles.get(name)) {
            if preference == "local" {
                if self.config.local.enabled {
                    return Ok(&self.config.local);
                }
                return Err(ZodoError::new(
                    ErrorCode::ProviderUnavailable,
                    "this role is pinned to the local model but the local provider is disabled",
                ));
            }
            if policy.model_route == ModelRoute::LocalOnly {
                return Err(ZodoError::new(
                    ErrorCode::PermissionDenied,
                    "this role is pinned to cloud but the workspace is local_only",
                ));
            }
            let cloud = self
                .config
                .cloud
                .as_ref()
                .filter(|provider| provider.enabled)
                .filter(|provider| {
                    policy.allowed_cloud_providers.is_empty()
                        || policy
                            .allowed_cloud_providers
                            .iter()
                            .any(|id| id == &provider.id || id == "*")
                });
            return match cloud {
                Some(provider)
                    if matches!(
                        sensitivity,
                        DataSensitivity::Workspace | DataSensitivity::Public
                    ) && policy.private_notes == DataClass::CloudAllowed =>
                {
                    Ok(provider)
                }
                Some(_) => Err(ZodoError::new(
                    ErrorCode::PermissionDenied,
                    "role-pinned cloud route is not authorized for this data class or provider allowlist",
                )),
                None => Err(ZodoError::new(
                    ErrorCode::ProviderUnavailable,
                    "this role is pinned to cloud but no enabled cloud provider is allowed",
                )),
            };
        }
        match policy.model_route {
            ModelRoute::LocalOnly => {
                if self.config.local.enabled {
                    Ok(&self.config.local)
                } else {
                    Err(ZodoError::new(
                        ErrorCode::ProviderUnavailable,
                        "local_only is configured but the local model provider is disabled",
                    ))
                }
            }
            ModelRoute::CloudAllowed => {
                let cloud = self
                    .config
                    .cloud
                    .as_ref()
                    .filter(|provider| provider.enabled);
                let allowed = cloud.filter(|provider| {
                    policy.allowed_cloud_providers.is_empty()
                        || policy
                            .allowed_cloud_providers
                            .iter()
                            .any(|id| id == &provider.id || id == "*")
                });
                match (allowed, sensitivity) {
                    (Some(provider), DataSensitivity::Workspace | DataSensitivity::Public)
                        if policy.private_notes == DataClass::CloudAllowed =>
                    {
                        Ok(provider)
                    }
                    (Some(_), DataSensitivity::DeviceOnly)
                    | (Some(_), DataSensitivity::Workspace | DataSensitivity::Public) => {
                        Err(ZodoError::new(
                            ErrorCode::PermissionDenied,
                            "cloud route is not authorized for this data class; set private_notes to cloud_allowed explicitly",
                        ))
                    }
                    (None, _) => {
                        if self.config.local.enabled {
                            Ok(&self.config.local)
                        } else {
                            Err(ZodoError::new(
                                ErrorCode::ProviderUnavailable,
                                "no enabled model provider matches the workspace policy",
                            ))
                        }
                    }
                }
            }
        }
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
        let api_key = provider.api_key_env.as_deref().and_then(|name| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        });
        let body = json!({
            "model": provider.model,
            "messages": messages,
            "temperature": 0.1,
            "stream": false,
            "response_format": {"type": "json_object"},
            "format": "json",
        });
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(120))
            .timeout_write(Duration::from_secs(15))
            .build();
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

    #[test]
    fn local_only_never_uses_cloud() {
        let dir = tempfile::tempdir().unwrap();
        let gateway = ModelGateway::load(dir.path());
        let policy = PrivacyPolicy {
            model_route: ModelRoute::LocalOnly,
            private_notes: DataClass::CloudAllowed,
            allowed_cloud_providers: vec!["*".to_string()],
            diagnostic_capture: false,
        };
        let provider = gateway
            .route_for(&policy, DataSensitivity::Workspace, None)
            .unwrap();
        assert_eq!(provider.location, "device");
    }

    #[test]
    fn role_routing_can_pin_cloud_or_local_and_never_bypasses_local_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut gateway = ModelGateway::load(dir.path());
        gateway.config.cloud.as_mut().unwrap().enabled = true;
        gateway
            .config
            .roles
            .insert("decompose".to_string(), "cloud".to_string());
        gateway
            .config
            .roles
            .insert("clarify".to_string(), "local".to_string());
        let cloud_policy = PrivacyPolicy {
            model_route: ModelRoute::CloudAllowed,
            private_notes: DataClass::CloudAllowed,
            allowed_cloud_providers: vec!["deepseek-cloud".to_string()],
            diagnostic_capture: false,
        };
        assert_eq!(
            gateway
                .route_for(&cloud_policy, DataSensitivity::Workspace, Some("decompose"))
                .unwrap()
                .id,
            "deepseek-cloud"
        );
        assert_eq!(
            gateway
                .route_for(&cloud_policy, DataSensitivity::Workspace, Some("clarify"))
                .unwrap()
                .location,
            "device"
        );
        let local_policy = PrivacyPolicy {
            model_route: ModelRoute::LocalOnly,
            ..cloud_policy.clone()
        };
        assert!(
            gateway
                .route_for(&local_policy, DataSensitivity::Workspace, Some("decompose"))
                .is_err(),
            "a cloud-pinned role must fail truthfully under local_only"
        );
    }

    #[test]
    fn cloud_requires_explicit_data_class_and_allowlist() {
        let dir = tempfile::tempdir().unwrap();
        let mut gateway = ModelGateway::load(dir.path());
        gateway.config.cloud.as_mut().unwrap().enabled = true;
        let policy = PrivacyPolicy {
            model_route: ModelRoute::CloudAllowed,
            private_notes: DataClass::DeviceOnly,
            allowed_cloud_providers: vec!["deepseek-cloud".to_string()],
            diagnostic_capture: false,
        };
        assert!(
            gateway
                .route_for(&policy, DataSensitivity::Workspace, None)
                .is_err()
        );
        let policy = PrivacyPolicy {
            private_notes: DataClass::CloudAllowed,
            ..policy
        };
        assert_eq!(
            gateway
                .route_for(&policy, DataSensitivity::Workspace, None)
                .unwrap()
                .id,
            "deepseek-cloud"
        );
    }
}
