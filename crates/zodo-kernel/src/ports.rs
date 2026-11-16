use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::model::World;

#[derive(Debug, Clone)]
pub struct ReceiptInsert {
    pub principal_id: String,
    pub idempotency_key: String,
    pub request_hash: String,
    pub operation_id: String,
    pub status: crate::commands::ResultStatus,
    pub revision: u64,
    pub data: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct StoredReceipt {
    pub principal_id: String,
    pub idempotency_key: String,
    pub request_hash: String,
    pub operation_id: String,
    pub status: crate::commands::ResultStatus,
    pub revision: u64,
    pub data: serde_json::Value,
    pub event_cursor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewEvent {
    pub workspace_id: String,
    pub event_type: String,
    pub actor_id: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    pub seq: u64,
    pub workspace_id: String,
    pub event_type: String,
    pub actor_id: String,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAudit {
    pub workspace_id: String,
    pub actor_id: String,
    pub principal_kind: String,
    pub channel: String,
    pub operation: String,
    pub operation_id: String,
    pub revision: u64,
    pub status: String,
    pub summary: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub seq: u64,
    pub workspace_id: String,
    pub actor_id: String,
    pub principal_kind: String,
    pub channel: String,
    pub operation: String,
    pub operation_id: String,
    pub revision: u64,
    pub status: String,
    pub summary: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewEffect {
    pub id: String,
    pub workspace_id: String,
    pub effect_type: String,
    pub payload: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
    pub available_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectRecord {
    pub id: String,
    pub workspace_id: String,
    pub effect_type: String,
    pub payload: serde_json::Value,
    pub status: String,
    pub attempts: u32,
    pub dedupe_key: Option<String>,
    pub available_at: DateTime<Utc>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SaveOptions {
    pub expected_revision: u64,
    pub bump_revision: bool,
}

#[derive(Debug, Clone)]
pub struct SaveOutcome {
    pub revision: u64,
    pub event_cursor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHandshake {
    pub protocol: String,
    pub runtime_version: String,
    pub instance_id: String,
    pub workspace_id: String,
    pub workspace_name: String,
    pub revision: u64,
    pub policy_revision: u64,
    pub started_at: DateTime<Utc>,
}

/// Durable storage port. The kernel owns the semantics; the runtime implements this with SQLite.
/// One implementation instance is the only writer for its workspace.
pub trait Store {
    fn load_world(&mut self, workspace_id: &str) -> Result<Option<World>>;
    fn find_receipt(
        &mut self,
        workspace_id: &str,
        principal_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<StoredReceipt>>;
    fn save(
        &mut self,
        world: &World,
        options: SaveOptions,
        receipt: Option<ReceiptInsert>,
        events: &[NewEvent],
        audit: &[NewAudit],
        effects: &[NewEffect],
    ) -> Result<SaveOutcome>;
    fn list_events(
        &mut self,
        workspace_id: &str,
        after_seq: u64,
        limit: u32,
    ) -> Result<Vec<EventRecord>>;
    fn list_audit(
        &mut self,
        workspace_id: &str,
        after_seq: u64,
        limit: u32,
    ) -> Result<Vec<AuditRecord>>;
    fn latest_cursor(&mut self, workspace_id: &str) -> Result<String>;
    fn pending_effects(&mut self, now: DateTime<Utc>, limit: u32) -> Result<Vec<EffectRecord>>;
    fn update_effect(
        &mut self,
        id: &str,
        status: &str,
        attempts: u32,
        last_error: Option<&str>,
        available_at: DateTime<Utc>,
    ) -> Result<()>;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

pub struct FixedClock {
    now: std::sync::Mutex<DateTime<Utc>>,
}

impl FixedClock {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            now: std::sync::Mutex::new(now),
        }
    }

    pub fn set(&self, now: DateTime<Utc>) {
        *self.now.lock().expect("clock poisoned") = now;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.lock().expect("clock poisoned")
    }
}

pub trait IdSource: Send + Sync {
    fn next(&self, prefix: &str) -> String;
}

#[derive(Default)]
pub struct UuidIds;

impl IdSource for UuidIds {
    fn next(&self, prefix: &str) -> String {
        let value = uuid::Uuid::new_v4().simple().to_string();
        format!("{prefix}_{}", &value[..20])
    }
}

#[derive(Default)]
pub struct SequentialIds {
    counter: AtomicU64,
}

impl SequentialIds {
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(1),
        }
    }
}

impl IdSource for SequentialIds {
    fn next(&self, prefix: &str) -> String {
        format!(
            "{prefix}_{:08}",
            self.counter.fetch_add(1, Ordering::SeqCst)
        )
    }
}
