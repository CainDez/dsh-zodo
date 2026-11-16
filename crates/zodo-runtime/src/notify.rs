use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{Duration, Utc};
use serde_json::json;
use tracing::{info, warn};
use zodo_kernel::app::Application;
use zodo_kernel::commands::CommandEnvelope;
use zodo_kernel::error::{ErrorCode, Result, ZodoError};
use zodo_kernel::model::{CommandContext, PrincipalKind, ReminderStatus};
use zodo_kernel::ports::{Store, SystemClock, UuidIds};

use crate::runtime::{AppState, WORKSPACE_ID};

const MAX_ATTEMPTS: u32 = 5;

pub fn spawn_effect_worker(state: AppState, data_dir: PathBuf) {
    if cfg!(windows) {
        let script = toast_script_path(&data_dir);
        if let Err(error) =
            std::fs::create_dir_all(&data_dir).and_then(|_| std::fs::write(&script, TOAST_SCRIPT))
        {
            warn!("could not prepare Windows toast script: {error}");
        }
    }
    std::thread::Builder::new()
        .name("zodo-effect-worker".to_string())
        .spawn(move || effect_loop(state, data_dir))
        .expect("spawn Zodo effect worker");
}

fn effect_loop(state: AppState, data_dir: PathBuf) {
    loop {
        let effects = {
            let mut inner = match state.inner.lock() {
                Ok(inner) => inner,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(1000));
                    continue;
                }
            };
            inner
                .store
                .pending_effects(Utc::now(), 10)
                .unwrap_or_default()
        };
        if effects.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(500));
            continue;
        }
        for effect in effects {
            if let Err(error) = deliver_effect(&state, &data_dir, &effect) {
                warn!(
                    "effect {} ({}) delivery failed: {}",
                    effect.id, effect.effect_type, error.message
                );
                let attempts = effect.attempts.saturating_add(1);
                let status = if attempts >= MAX_ATTEMPTS {
                    "failed"
                } else {
                    "pending"
                };
                let retry_at = Utc::now() + Duration::seconds(30);
                if let Ok(mut inner) = state.inner.lock() {
                    let _ = inner.store.update_effect(
                        &effect.id,
                        status,
                        attempts,
                        Some(&error.message),
                        retry_at,
                    );
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
}

fn deliver_effect(
    state: &AppState,
    data_dir: &Path,
    effect: &zodo_kernel::ports::EffectRecord,
) -> Result<()> {
    if effect.effect_type == "calendar.external_write" {
        return deliver_calendar_external(state, effect);
    }
    if effect.effect_type != "reminder.deliver" {
        return Err(ZodoError::new(
            ErrorCode::UnsupportedOperation,
            format!("unknown effect type {}", effect.effect_type),
        ));
    }
    let reminder_id = effect
        .payload
        .get("reminder_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ZodoError::validation("reminder effect has no reminder_id"))?;
    let (reminder, runtime_instance) = {
        let mut inner = state
            .inner
            .lock()
            .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"))?;
        let world = inner
            .store
            .load_world(WORKSPACE_ID)?
            .ok_or_else(|| ZodoError::not_found("workspace", WORKSPACE_ID))?;
        let reminder = world
            .reminders
            .iter()
            .find(|reminder| reminder.id == reminder_id)
            .cloned();
        (reminder, inner.instance_id.clone())
    };
    let Some(reminder) = reminder else {
        // The reminder was removed as a fact; the effect is no longer actionable.
        mark_effect_done(state, effect, "delivered", "reminder_missing")?;
        return Ok(());
    };
    match reminder.status {
        ReminderStatus::Cancelled => {
            mark_effect_done(state, effect, "delivered", "cancelled_before_delivery")?;
            return Ok(());
        }
        ReminderStatus::Delivered => {
            mark_effect_done(state, effect, "delivered", "already_delivered")?;
            return Ok(());
        }
        _ => {}
    }
    let title = "Zodo 提醒";
    let body = if reminder.message.trim().is_empty() {
        reminder_id.to_string()
    } else {
        reminder.message.clone()
    };
    let delivered = deliver_notification(data_dir, title, &body);
    append_notification_log(
        data_dir,
        json!({
            "schema": "zodo.notification/1",
            "effect_id": effect.id,
            "reminder_id": reminder.id,
            "title": title,
            "message": body,
            "attempts": effect.attempts + 1,
            "delivered": delivered.is_ok(),
            "backend": if cfg!(windows) { "windows_toast" } else { "local_log" },
            "error": delivered.as_ref().err().map(|error| error.message.clone()),
        }),
    );
    match delivered {
        Ok(()) => {
            let now = Utc::now();
            let envelope = CommandEnvelope {
                schema: "zodo.command/1".to_string(),
                request_id: format!("effect-{}-{}", effect.id, now.timestamp_millis()),
                idempotency_key: format!("effect:{}", effect.id),
                workspace_id: WORKSPACE_ID.to_string(),
                operation: "reminder.mark_delivered".to_string(),
                operation_version: 1,
                base_revision: None,
                policy_revision: None,
                payload: json!({
                    "id": reminder.id,
                    "delivered_at": now.to_rfc3339(),
                    "failure": null,
                }),
            };
            {
                let mut inner = state.inner.lock().map_err(|_| {
                    ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable")
                })?;
                let context = CommandContext {
                    principal_id: "notification_worker".to_string(),
                    principal_kind: PrincipalKind::System,
                    channel: "system".to_string(),
                    capabilities: vec!["write".to_string(), "execute".to_string()],
                    workspace_id: WORKSPACE_ID.to_string(),
                    request_id: envelope.request_id.clone(),
                    received_at: now,
                    runtime_instance,
                };
                let instance_id = inner.instance_id.clone();
                let mut app = Application {
                    store: &mut inner.store,
                    clock: &SystemClock,
                    ids: &UuidIds::default(),
                    instance_id,
                };
                app.execute(&context, envelope)?;
                inner.store.update_effect(
                    &effect.id,
                    "delivered",
                    effect.attempts + 1,
                    None,
                    Utc::now(),
                )?;
            }
            info!("delivered reminder {} (effect {})", reminder.id, effect.id);
            Ok(())
        }
        Err(error) => {
            let attempts = effect.attempts + 1;
            let failure = json!({"code": error.code, "message": error.message});
            let envelope = CommandEnvelope {
                schema: "zodo.command/1".to_string(),
                request_id: format!("effect-{}-{}", effect.id, Utc::now().timestamp_millis()),
                idempotency_key: format!("effect:{}", effect.id),
                workspace_id: WORKSPACE_ID.to_string(),
                operation: "reminder.mark_delivered".to_string(),
                operation_version: 1,
                base_revision: None,
                policy_revision: None,
                payload: json!({"id": reminder.id, "delivered_at": null, "failure": failure}),
            };
            if attempts >= MAX_ATTEMPTS {
                let mut inner = state.inner.lock().map_err(|_| {
                    ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable")
                })?;
                let context = CommandContext {
                    principal_id: "notification_worker".to_string(),
                    principal_kind: PrincipalKind::System,
                    channel: "system".to_string(),
                    capabilities: vec!["write".to_string(), "execute".to_string()],
                    workspace_id: WORKSPACE_ID.to_string(),
                    request_id: envelope.request_id.clone(),
                    received_at: Utc::now(),
                    runtime_instance,
                };
                let instance_id = inner.instance_id.clone();
                let mut app = Application {
                    store: &mut inner.store,
                    clock: &SystemClock,
                    ids: &UuidIds::default(),
                    instance_id,
                };
                app.execute(&context, envelope)?;
            }
            Err(error)
        }
    }
}

fn deliver_calendar_external(
    state: &AppState,
    effect: &zodo_kernel::ports::EffectRecord,
) -> Result<()> {
    let Some(url) = std::env::var("ZODO_CALENDAR_WRITE_URL")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        mark_effect_done(state, effect, "delivered", "calendar_adapter_absent")?;
        return Ok(());
    };
    let external_id = effect
        .payload
        .get("external_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ZodoError::validation("calendar effect has no external_id"))?;
    let response = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("Idempotency-Key", external_id)
        .timeout(std::time::Duration::from_secs(10))
        .send_json(effect.payload.clone());
    match response {
        Ok(_) => {
            mark_effect_done(state, effect, "delivered", "external_calendar_written")?;
            Ok(())
        }
        Err(ureq::Error::Status(status, response)) => {
            let detail = response.into_string().unwrap_or_default();
            Err(ZodoError::new(
                ErrorCode::ProviderUnavailable,
                format!(
                    "external calendar returned HTTP {status}: {}",
                    detail.chars().take(300).collect::<String>()
                ),
            ))
        }
        Err(error) => Err(ZodoError::new(
            ErrorCode::ProviderUnavailable,
            format!("external calendar request failed: {error}"),
        )),
    }
}

fn mark_effect_done(
    state: &AppState,
    effect: &zodo_kernel::ports::EffectRecord,
    status: &str,
    _reason: &str,
) -> Result<()> {
    let mut inner = state
        .inner
        .lock()
        .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"))?;
    inner
        .store
        .update_effect(&effect.id, status, effect.attempts, None, Utc::now())
}

fn append_notification_log(data_dir: &Path, value: serde_json::Value) {
    let path = data_dir.join("notifications.jsonl");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{value}");
    }
}

fn toast_script_path(data_dir: &Path) -> PathBuf {
    data_dir.join("zodo-toast.ps1")
}

fn deliver_notification(data_dir: &Path, title: &str, body: &str) -> Result<()> {
    #[cfg(windows)]
    {
        let powershell = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";
        if !Path::new(powershell).exists() {
            return Err(ZodoError::new(
                ErrorCode::ProviderUnavailable,
                "Windows PowerShell is not available for toast delivery",
            ));
        }
        let output = Command::new(powershell)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-WindowStyle",
                "Hidden",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(toast_script_path(data_dir))
            .env("ZODO_NOTIFY_TITLE", title)
            .env("ZODO_NOTIFY_MESSAGE", body)
            .output()
            .map_err(|error| {
                ZodoError::new(
                    ErrorCode::ProviderUnavailable,
                    format!("failed to run Windows toast: {error}"),
                )
            })?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() || !stdout.contains("TOAST_OK") {
            return Err(ZodoError::new(
                ErrorCode::ProviderUnavailable,
                format!(
                    "Windows toast exited {:?}: {}",
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr)
                        .chars()
                        .take(300)
                        .collect::<String>()
                ),
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (data_dir, title);
        // Linux/macOS delivery is intentionally a recorded local notification until a platform
        // backend is installed.
        let _ = body;
        Ok(())
    }
}

const TOAST_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null
$template = [Windows.UI.Notifications.ToastTemplateType]::ToastText02
$xml = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent($template)
$texts = $xml.GetElementsByTagName('text')
$title = if ($env:ZODO_NOTIFY_TITLE) { $env:ZODO_NOTIFY_TITLE } else { 'Zodo' }
$message = if ($env:ZODO_NOTIFY_MESSAGE) { $env:ZODO_NOTIFY_MESSAGE } else { '' }
$texts.Item(0).AppendChild($xml.CreateTextNode($title)) > $null
$texts.Item(1).AppendChild($xml.CreateTextNode($message)) > $null
$toast = [Windows.UI.Notifications.ToastNotification]::new($xml)
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Zodo').Show($toast)
'TOAST_OK'
"#;
