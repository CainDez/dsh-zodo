use std::time::Duration;

use chrono::Utc;
use tracing::{info, warn};
use zodo_kernel::app::Application;
use zodo_kernel::commands::CommandEnvelope;
use zodo_kernel::error::{ErrorCode, Result, ZodoError};
use zodo_kernel::model::{AutomationRule, AutomationTrigger, CommandContext, PrincipalKind};
use zodo_kernel::ports::{Clock, Store, SystemClock, UuidIds};

use crate::runtime::{AppState, WORKSPACE_ID};

const MIN_INTERVAL_SECONDS: i64 = 60;

pub fn spawn_automation_worker(state: AppState) {
    std::thread::Builder::new()
        .name("zodo-automation-worker".to_string())
        .spawn(move || {
            loop {
                if let Err(error) = tick(&state) {
                    warn!("automation scheduler tick failed: {}", error.message);
                }
                std::thread::sleep(Duration::from_secs(5));
            }
        })
        .expect("spawn Zodo automation worker");
}

fn tick(state: &AppState) -> Result<()> {
    let due = {
        let mut inner = state
            .inner
            .lock()
            .map_err(|_| ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable"))?;
        let world = inner
            .store
            .load_world(WORKSPACE_ID)?
            .ok_or_else(|| ZodoError::not_found("workspace", WORKSPACE_ID))?;
        let now = SystemClock.now();
        world
            .automation_rules
            .iter()
            .filter(|rule| is_due(rule, now))
            .map(|rule| rule.id.clone())
            .collect::<Vec<_>>()
    };
    for id in due {
        let result = {
            let mut inner = state.inner.lock().map_err(|_| {
                ZodoError::new(ErrorCode::StorageFailure, "runtime state unavailable")
            })?;
            let now = SystemClock.now();
            let request_id = format!("automation-{}-{}", id, now.timestamp());
            let context = CommandContext {
                principal_id: "automation_worker".to_string(),
                principal_kind: PrincipalKind::System,
                channel: "system".to_string(),
                capabilities: vec!["write".to_string(), "execute".to_string()],
                workspace_id: WORKSPACE_ID.to_string(),
                request_id: request_id.clone(),
                received_at: now,
                runtime_instance: inner.instance_id.clone(),
            };
            let envelope = CommandEnvelope {
                schema: "zodo.command/1".to_string(),
                request_id,
                idempotency_key: format!("automation:{}:{}", id, now.timestamp()),
                workspace_id: WORKSPACE_ID.to_string(),
                operation: "automation.run".to_string(),
                operation_version: 1,
                base_revision: None,
                policy_revision: None,
                payload: serde_json::json!({"id": id}),
            };
            let instance_id = inner.instance_id.clone();
            let mut app = Application {
                store: &mut inner.store,
                clock: &SystemClock,
                ids: &UuidIds::default(),
                instance_id,
            };
            app.execute(&context, envelope).map(|_| ())
        };
        match result {
            Ok(()) => info!("automation rule {} ran on schedule", id),
            Err(error) => warn!("scheduled automation {} failed: {}", id, error.message),
        }
    }
    Ok(())
}

/// A rule is due only when it is explicitly a Schedule trigger, enabled, under budget, has a bounded
/// interval of at least 60s, and its last run is older than that interval. This is deliberately
/// stateless/pure so the loop-prevention boundary can be unit tested.
pub fn is_due(rule: &AutomationRule, now: chrono::DateTime<Utc>) -> bool {
    if !rule.enabled || rule.runs >= rule.budget {
        return false;
    }
    if rule.trigger != AutomationTrigger::Schedule {
        return false;
    }
    let interval = rule
        .conditions
        .get("interval_seconds")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(3600)
        .max(MIN_INTERVAL_SECONDS);
    match rule.last_run_at {
        Some(last) => (now - last).num_seconds() >= interval,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn rule(trigger: AutomationTrigger, last: Option<chrono::DateTime<Utc>>) -> AutomationRule {
        AutomationRule {
            id: "aut_test".to_string(),
            workspace_id: WORKSPACE_ID.to_string(),
            name: "test".to_string(),
            trigger,
            conditions: serde_json::json!({"interval_seconds": 60}),
            actions: serde_json::json!([]),
            enabled: true,
            budget: 2,
            runs: 0,
            last_run_at: last,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn schedule_interval_clamps_to_a_minute_and_is_inclusive_at_the_boundary() {
        let now = Utc::now();
        for interval in [-1, 0, 1, 59, 60] {
            let mut candidate = rule(
                AutomationTrigger::Schedule,
                Some(now - Duration::seconds(59)),
            );
            candidate.conditions = serde_json::json!({"interval_seconds": interval});
            assert!(!is_due(&candidate, now), "interval {interval}");
            candidate.last_run_at = Some(now - Duration::seconds(60));
            assert!(is_due(&candidate, now), "interval {interval}");
        }
    }

    #[test]
    fn invalid_or_absent_interval_uses_hour_default() {
        let now = Utc::now();
        for conditions in [
            serde_json::json!({}),
            serde_json::json!({"interval_seconds": "60"}),
            serde_json::json!({"interval_seconds": null}),
        ] {
            let mut candidate = rule(
                AutomationTrigger::Schedule,
                Some(now - Duration::seconds(3599)),
            );
            candidate.conditions = conditions;
            assert!(!is_due(&candidate, now));
            candidate.last_run_at = Some(now - Duration::seconds(3600));
            assert!(is_due(&candidate, now));
        }
    }

    #[test]
    fn disabled_exhausted_and_future_last_run_are_never_due() {
        let now = Utc::now();
        let mut candidate = rule(AutomationTrigger::Schedule, None);
        candidate.enabled = false;
        assert!(!is_due(&candidate, now));
        candidate.enabled = true;
        candidate.budget = 0;
        assert!(!is_due(&candidate, now));
        candidate.budget = 2;
        candidate.runs = 3;
        assert!(!is_due(&candidate, now));
        candidate.runs = 0;
        candidate.last_run_at = Some(now + Duration::seconds(1));
        assert!(!is_due(&candidate, now));
    }

    #[test]
    fn schedule_interval_is_bounded_and_due_once() {
        let now = Utc::now();
        let mut candidate = rule(AutomationTrigger::Schedule, None);
        assert!(is_due(&candidate, now));
        candidate.last_run_at = Some(now - Duration::seconds(30));
        assert!(!is_due(&candidate, now));
        candidate.last_run_at = Some(now - Duration::seconds(61));
        assert!(is_due(&candidate, now));
        candidate.runs = 2;
        assert!(!is_due(&candidate, now));
        let manual = rule(AutomationTrigger::Manual, None);
        assert!(!is_due(&manual, now));
    }
}
