use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zodo_kernel::error::{ErrorCode, Result, ZodoError};
use zodo_kernel::model::{DataClass, PrivacyPolicy};

pub const MODEL_CONFIG_VERSION: u32 = 2;
pub const MAX_MODEL_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelSource {
    Dsh {
        #[serde(deserialize_with = "required_nullable")]
        provider: Option<String>,
        #[serde(deserialize_with = "required_nullable")]
        model: Option<String>,
    },
    Custom {
        base_url: String,
        model: String,
        #[serde(deserialize_with = "required_nullable")]
        api_key_env: Option<String>,
    },
}

fn required_nullable<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub version: u32,
    pub source: ModelSource,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            version: MODEL_CONFIG_VERSION,
            source: ModelSource::Dsh {
                provider: None,
                model: None,
            },
        }
    }
}

fn valid_name(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 200 && !value.chars().any(char::is_control)
}

impl ModelConfig {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("model.json")
    }

    /// Missing is unselected. Malformed/old configs fail closed, never migrate or substitute.
    pub fn load(data_dir: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(Self::path(data_dir)) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(_) => {
                return Err(ZodoError::new(
                    ErrorCode::StorageFailure,
                    "model_config_unreadable",
                ));
            }
        };
        let config: Self = serde_json::from_str(&text).map_err(|_| {
            ZodoError::validation("invalid model config; only version 2 is supported")
        })?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        self.validate()?;
        let path = Self::path(data_dir);
        let temporary = path.with_extension("tmp");
        let body = serde_json::to_vec_pretty(self)
            .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "model_config_encode_failed"))?;
        std::fs::write(&temporary, body)
            .and_then(|_| std::fs::rename(&temporary, &path))
            .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "model_config_write_failed"))
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != MODEL_CONFIG_VERSION {
            return Err(ZodoError::validation("unsupported model config version"));
        }
        match &self.source {
            ModelSource::Dsh { provider, model } => match (provider, model) {
                (None, None) => Ok(()),
                (Some(provider), Some(model)) if valid_name(provider) && valid_name(model) => {
                    Ok(())
                }
                _ => Err(ZodoError::validation(
                    "select both DSH provider and model, or neither",
                )),
            },
            ModelSource::Custom {
                base_url,
                model,
                api_key_env,
            } => {
                endpoint_uri(base_url)?;
                if !valid_name(model) {
                    return Err(ZodoError::validation(
                        "model is required and must be at most 200 bytes",
                    ));
                }
                if let Some(name) = api_key_env {
                    if name.is_empty()
                        || name.len() > 100
                        || !name.bytes().enumerate().all(|(i, c)| {
                            c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit())
                        })
                    {
                        return Err(ZodoError::validation(
                            "api_key_env must be an environment variable name, not a secret",
                        ));
                    }
                }
                Ok(())
            }
        }
    }

    pub fn catalog(&self) -> Value {
        json!({"schema":"zodo.model-catalog/1", "catalog_version":2, "source":self.source,
            "actions":[{"id":"goal.decompose","produces":"proposal_only"},{"id":"capture.clarify","produces":"run_output_only"}]})
    }
}

pub(crate) fn endpoint_uri(url: &str) -> Result<axum::http::Uri> {
    let uri: axum::http::Uri = url
        .parse()
        .map_err(|_| ZodoError::validation("invalid model endpoint"))?;
    let authority = uri
        .authority()
        .ok_or_else(|| ZodoError::validation("model endpoint requires a host"))?;
    if !matches!(uri.scheme_str(), Some("http" | "https"))
        || authority.as_str().contains('@')
        || url.contains('#')
        || uri.query().is_some()
        || url.len() > 500
    {
        return Err(ZodoError::validation(
            "model endpoint must be HTTP(S), without credentials, query or fragment",
        ));
    }
    Ok(uri)
}

pub(crate) fn is_loopback_endpoint(url: &str) -> bool {
    endpoint_uri(url)
        .ok()
        .and_then(|uri| uri.host().map(str::to_string))
        .is_some_and(|host| {
            let host = host.trim_start_matches('[').trim_end_matches(']');
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        })
}

// localhost never uses DNS. Literal loopback only, no proxies, no redirects.
pub(crate) fn loopback_resolve(netloc: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
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
            "endpoint must be loopback",
        )),
    }
}

pub(crate) fn http_agent(loopback: bool) -> ureq::Agent {
    let mut builder = ureq::AgentBuilder::new()
        .redirects(0)
        .try_proxy_from_env(false)
        .timeout(Duration::from_secs(65))
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(60))
        .timeout_write(Duration::from_secs(15));
    if loopback {
        builder = builder.resolver(loopback_resolve);
    }
    builder.build()
}

pub(crate) fn read_response(response: ureq::Response) -> Result<Value> {
    if response.status() != 200 {
        return Err(unavailable("model_response_rejected"));
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take((MAX_MODEL_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| unavailable("model_response_unreadable"))?;
    if bytes.len() > MAX_MODEL_BYTES {
        return Err(unavailable("model_response_too_large"));
    }
    serde_json::from_slice(&bytes).map_err(|_| unavailable("model_response_invalid_json"))
}

pub(crate) fn unavailable(reason: &str) -> ZodoError {
    ZodoError::new(ErrorCode::ProviderUnavailable, reason)
}
pub(crate) fn denied(reason: &str) -> ZodoError {
    ZodoError::new(ErrorCode::PermissionDenied, reason)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}
impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCompletion {
    pub provider_id: String,
    pub model: String,
    pub source: String,
    pub content: String,
    pub elapsed_ms: u128,
}

pub fn authorize_scope(policy: &PrivacyPolicy, scope: &str) -> Result<()> {
    match scope {
        "goal.decompose" if policy.model_access.goals => Ok(()),
        "capture.clarify" if policy.model_access.captures => Ok(()),
        "goal.decompose" => Err(denied("goals_not_allowed")),
        "capture.clarify" => Err(denied("captures_not_allowed")),
        _ => Err(denied("invalid_model_scope")),
    }
}

pub fn authorize_class(config: &ModelConfig, class: &DataClass) -> Result<()> {
    if *class != DataClass::DeviceOnly
        || matches!(&config.source, ModelSource::Custom { base_url, .. } if is_loopback_endpoint(base_url))
    {
        return Ok(());
    }
    Err(denied("device_only_requires_custom_loopback"))
}

fn custom_key(name: &Option<String>) -> Option<String> {
    name.as_ref()
        .and_then(|name| std::env::var(name).ok())
        .filter(|key| !key.trim().is_empty())
}

pub struct ModelGateway {
    pub config: ModelConfig,
}
impl ModelGateway {
    pub fn load(dir: &Path) -> Result<Self> {
        Ok(Self {
            config: ModelConfig::load(dir)?,
        })
    }

    pub fn preflight(&self, host: &crate::host::HostState) -> Result<()> {
        self.config.validate()?;
        match &self.config.source {
            ModelSource::Dsh {
                provider: Some(provider),
                model: Some(model),
            } => host.available(provider, model),
            ModelSource::Dsh { .. } => Err(unavailable("model_not_selected")),
            ModelSource::Custom { api_key_env, .. } => {
                if api_key_env.is_some() && custom_key(api_key_env).is_none() {
                    Err(unavailable("custom_credentials_missing"))
                } else {
                    Ok(())
                }
            }
        }
    }

    pub fn effective(&self, policy: &PrivacyPolicy, host: &crate::host::HostState) -> Value {
        let result = self.preflight(host);
        let reason = result
            .as_ref()
            .err()
            .map(|e| e.message.as_str())
            .unwrap_or("configured");
        let (source, provider, model) = match &self.config.source {
            ModelSource::Dsh { provider, model } => ("dsh", provider.clone(), model.clone()),
            ModelSource::Custom { model, .. } => ("custom", None, Some(model.clone())),
        };
        let operation = |scope| {
            let error = authorize_scope(policy, scope)
                .err()
                .or_else(|| self.preflight(host).err());
            json!({"available":error.is_none(), "reason":error.map(|e| e.message).unwrap_or_else(|| "configured".into())})
        };
        json!({"status":"preflight", "connection_tested":false,
            "selected":{"source":source,"provider":provider,"model":model,"available":result.is_ok(),"reason":reason},
            "operations":{"decompose":operation("goal.decompose"),"clarify":operation("capture.clarify")}})
    }

    /// Only the custom branch speaks OpenAI HTTP. DSH never copies connection or credential facts.
    pub(crate) fn complete_custom(
        &self,
        class: &DataClass,
        messages: &[ChatMessage],
        guard: crate::model_transport::DispatchGuard,
    ) -> Result<ModelCompletion> {
        authorize_class(&self.config, class)?;
        let ModelSource::Custom {
            base_url,
            model,
            api_key_env,
        } = &self.config.source
        else {
            return Err(unavailable("host_unavailable"));
        };
        self.config.validate()?;
        let key = custom_key(api_key_env);
        if api_key_env.is_some() && key.is_none() {
            return Err(unavailable("custom_credentials_missing"));
        }
        let base = base_url.trim_end_matches('/');
        let url = if base.ends_with("/chat/completions") {
            base.into()
        } else {
            format!("{base}/chat/completions")
        };
        let start = std::time::Instant::now();
        let value = crate::model_transport::post_json(
            &url,
            key,
            json!({"model":model,"messages":messages,"stream":false,"max_tokens":4096,"response_format":{"type":"json_object"}}),
            guard,
        )?;
        let content = value
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .ok_or_else(|| unavailable("model_response_missing_content"))?;
        Ok(ModelCompletion {
            provider_id: "custom".into(),
            model: model.clone(),
            source: "custom".into(),
            content: content.into(),
            elapsed_ms: start.elapsed().as_millis(),
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
    use crate::host::HostState;
    use zodo_kernel::model::ModelAccess;

    fn config(source: Value) -> ModelConfig {
        serde_json::from_value(json!({"version":2,"source":source})).unwrap()
    }

    #[test]
    fn strict_single_source_config_and_fail_closed_load() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            ModelConfig::load(dir.path()).unwrap(),
            ModelConfig::default()
        );
        for invalid in [
            json!({"version":1,"local":{},"cloud":{}}),
            json!({"version":2,"source":{"kind":"dsh","provider":"p","model":"m","endpoint":"http://localhost"}}),
            json!({"version":2,"source":{"kind":"custom","base_url":"http://localhost/v1","model":"m","api_key_env":null,"roles":{}}}),
            json!({"version":2,"source":{"kind":"dsh"}}),
            json!({"version":2,"source":{"kind":"dsh","provider":null,"model":null},"roles":{}}),
        ] {
            assert!(serde_json::from_value::<ModelConfig>(invalid).is_err());
        }
        for invalid in [
            json!({"version":1,"source":{"kind":"dsh","provider":null,"model":null}}),
            json!({"version":2,"source":{"kind":"dsh","provider":"p","model":null}}),
            json!({"version":2,"source":{"kind":"custom","base_url":"http://secret@localhost/v1","model":"m","api_key_env":null}}),
            json!({"version":2,"source":{"kind":"custom","base_url":"http://localhost/v1?q=secret","model":"m","api_key_env":null}}),
        ] {
            assert!(
                serde_json::from_value::<ModelConfig>(invalid)
                    .unwrap()
                    .validate()
                    .is_err()
            );
        }
        std::fs::write(ModelConfig::path(dir.path()), r#"{"version":1,"local":{}}"#).unwrap();
        assert!(ModelConfig::load(dir.path()).is_err());
    }

    #[test]
    fn content_consent_never_selects_or_falls_back_a_model() {
        let denied = PrivacyPolicy::default();
        let allowed = PrivacyPolicy {
            model_access: ModelAccess {
                goals: true,
                projects: false,
                captures: true,
            },
            diagnostic_capture: false,
        };
        assert!(authorize_scope(&denied, "goal.decompose").is_err());
        assert!(authorize_scope(&denied, "capture.clarify").is_err());
        assert!(authorize_scope(&allowed, "goal.decompose").is_ok());
        let unselected = ModelGateway {
            config: ModelConfig::default(),
        };
        assert_eq!(
            unselected
                .preflight(&HostState::default())
                .unwrap_err()
                .message,
            "model_not_selected"
        );
        let selected = ModelGateway {
            config: config(json!({"kind":"dsh","provider":"subscription","model":"m"})),
        };
        assert_eq!(
            selected
                .preflight(&HostState::default())
                .unwrap_err()
                .message,
            "host_unavailable"
        );
        assert_eq!(
            selected.effective(&allowed, &HostState::default())["selected"]["source"],
            "dsh"
        );
        assert!(
            !selected.effective(&allowed, &HostState::default())["connection_tested"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn device_only_requires_original_custom_loopback_not_dsh_labels() {
        let selected = config(json!({"kind":"dsh","provider":"localhost","model":"device"}));
        assert!(authorize_class(&selected, &DataClass::DeviceOnly).is_err());
        for url in [
            "http://127.0.0.1:1234/v1",
            "http://localhost:1234/v1",
            "http://[::1]:1234/v1",
        ] {
            let selected =
                config(json!({"kind":"custom","base_url":url,"model":"m","api_key_env":null}));
            assert!(authorize_class(&selected, &DataClass::DeviceOnly).is_ok());
        }
        for url in [
            "https://example.invalid/v1",
            "http://127.0.0.1.example.invalid/v1",
            "http://192.168.1.2/v1",
        ] {
            let selected =
                config(json!({"kind":"custom","base_url":url,"model":"m","api_key_env":null}));
            assert!(authorize_class(&selected, &DataClass::DeviceOnly).is_err());
        }
        assert_eq!(
            loopback_resolve("localhost:1234").unwrap()[0]
                .ip()
                .to_string(),
            "127.0.0.1"
        );
        assert!(loopback_resolve("example.invalid:80").is_err());
    }

    #[test]
    fn parses_model_json() {
        assert_eq!(
            parse_json_object(r#"{"tasks":[]}"#).unwrap()["tasks"],
            json!([])
        );
    }
}
