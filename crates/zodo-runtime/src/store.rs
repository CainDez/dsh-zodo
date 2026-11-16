use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::Value;
use zodo_kernel::commands::ResultStatus;
use zodo_kernel::error::{ErrorCode, Result, ZodoError};
use zodo_kernel::model::{Workspace, World};
use zodo_kernel::ports::{
    AuditRecord, EffectRecord, EventRecord, NewAudit, NewEffect, NewEvent, ReceiptInsert,
    SaveOptions, SaveOutcome, Store, StoredReceipt,
};

const SCHEMA: &str = r#"
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS workspaces (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  timezone TEXT NOT NULL,
  revision INTEGER NOT NULL,
  policy_revision INTEGER NOT NULL,
  json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS entities (
  workspace_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  id TEXT NOT NULL,
  status TEXT,
  json TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (workspace_id, kind, id)
);
CREATE INDEX IF NOT EXISTS entities_kind_status ON entities (workspace_id, kind, status);
CREATE TABLE IF NOT EXISTS operation_receipts (
  workspace_id TEXT NOT NULL,
  principal_id TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  request_hash TEXT NOT NULL,
  operation_id TEXT NOT NULL,
  status TEXT NOT NULL,
  revision INTEGER NOT NULL,
  result_json TEXT NOT NULL,
  event_cursor TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (workspace_id, principal_id, idempotency_key)
);
CREATE INDEX IF NOT EXISTS receipts_operation ON operation_receipts (workspace_id, operation_id);
CREATE TABLE IF NOT EXISTS events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  workspace_id TEXT NOT NULL,
  type TEXT NOT NULL,
  actor_id TEXT NOT NULL,
  payload TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS events_workspace_seq ON events (workspace_id, seq);
CREATE TABLE IF NOT EXISTS audit_events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  workspace_id TEXT NOT NULL,
  actor_id TEXT NOT NULL,
  principal_kind TEXT NOT NULL,
  channel TEXT NOT NULL,
  operation TEXT NOT NULL,
  operation_id TEXT NOT NULL,
  revision INTEGER NOT NULL,
  status TEXT NOT NULL,
  summary TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS audit_workspace_seq ON audit_events (workspace_id, seq);
CREATE TABLE IF NOT EXISTS effect_outbox (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  effect_type TEXT NOT NULL,
  payload TEXT NOT NULL,
  status TEXT NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0,
  dedupe_key TEXT,
  available_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_error TEXT
);
CREATE INDEX IF NOT EXISTS effects_pending ON effect_outbox (status, available_at);
CREATE UNIQUE INDEX IF NOT EXISTS effects_dedupe
  ON effect_outbox (workspace_id, dedupe_key) WHERE dedupe_key IS NOT NULL;
CREATE TABLE IF NOT EXISTS data_imports (
  workspace_id TEXT NOT NULL,
  idempotency_key TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  result_json TEXT NOT NULL,
  revision INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (workspace_id, idempotency_key)
);
CREATE TABLE IF NOT EXISTS runs (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  state TEXT NOT NULL,
  intent TEXT NOT NULL,
  input_json TEXT NOT NULL,
  checkpoint_json TEXT,
  output_json TEXT,
  error_json TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  started_at TEXT,
  ended_at TEXT,
  cancel_requested INTEGER NOT NULL DEFAULT 0,
  lease_owner TEXT,
  heartbeat_at TEXT
);
CREATE INDEX IF NOT EXISTS runs_workspace ON runs (workspace_id, created_at);
CREATE TABLE IF NOT EXISTS run_steps (
  run_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  kind TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  PRIMARY KEY (run_id, seq)
);
"#;

pub struct SqliteStore {
    connection: Connection,
    path: PathBuf,
}

impl SqliteStore {
    pub fn get_data_import(
        &self,
        workspace_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<(String, Value, u64)>> {
        self.connection
            .query_row(
                "SELECT fingerprint, result_json, revision FROM data_imports
                 WHERE workspace_id = ?1 AND idempotency_key = ?2",
                params![workspace_id, idempotency_key],
                |row| {
                    let fingerprint: String = row.get(0)?;
                    let result: String = row.get(1)?;
                    let revision: i64 = row.get(2)?;
                    Ok((fingerprint, result, revision as u64))
                },
            )
            .optional()
            .map_err(storage_error)
            .and_then(|value| match value {
                Some((fingerprint, result, revision)) => {
                    let result = serde_json::from_str(&result).map_err(storage_error)?;
                    Ok(Some((fingerprint, result, revision)))
                }
                None => Ok(None),
            })
    }

    pub fn record_data_import(
        &self,
        workspace_id: &str,
        idempotency_key: &str,
        fingerprint: &str,
        result: &Value,
        revision: u64,
    ) -> Result<()> {
        self.connection
            .execute(
                "INSERT OR REPLACE INTO data_imports
                 (workspace_id, idempotency_key, fingerprint, result_json, revision, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    workspace_id,
                    idempotency_key,
                    fingerprint,
                    serde_json::to_string(result).map_err(storage_error)?,
                    revision as i64,
                    Utc::now().to_rfc3339()
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(storage_error)?;
        }
        let connection = Connection::open(&path).map_err(storage_error)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(storage_error)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(storage_error)?;
        connection
            .pragma_update(None, "synchronous", "NORMAL")
            .map_err(storage_error)?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(storage_error)?;
        connection.execute_batch(SCHEMA).map_err(storage_error)?;
        Ok(Self { connection, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn has_workspace(&self, workspace_id: &str) -> Result<bool> {
        self.connection
            .query_row(
                "SELECT 1 FROM workspaces WHERE id = ?1",
                params![workspace_id],
                |_| Ok(()),
            )
            .optional()
            .map(|value| value.is_some())
            .map_err(storage_error)
    }

    pub fn create_workspace(&mut self, world: &World) -> Result<()> {
        let tx = self.connection.transaction().map_err(storage_error)?;
        let json = serde_json::to_string(&world.workspace).map_err(storage_error)?;
        tx.execute(
            "INSERT INTO workspaces (id, name, timezone, revision, policy_revision, json, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                world.workspace.id,
                world.workspace.name,
                world.workspace.settings.timezone,
                world.workspace.revision as i64,
                world.workspace.policy_revision as i64,
                json,
                world.workspace.updated_at.to_rfc3339(),
            ],
        )
        .map_err(storage_error)?;
        let entities = world.entity_json_map();
        for ((kind, id), value) in entities {
            let updated_at = value
                .get("updated_at")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| world.workspace.updated_at.to_rfc3339());
            tx.execute(
                "INSERT INTO entities (workspace_id, kind, id, status, json, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    world.workspace.id,
                    kind,
                    id,
                    value.get("status").and_then(|v| v.as_str()),
                    serde_json::to_string(&value).map_err(storage_error)?,
                    updated_at,
                ],
            )
            .map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)?;
        Ok(())
    }

    pub fn get_or_create_default(&mut self, workspace_id: &str, name: &str) -> Result<World> {
        if let Some(world) = self.load_world(workspace_id)? {
            return Ok(world);
        }
        let now = Utc::now();
        let workspace = Workspace::new(workspace_id.to_string(), name.to_string(), now);
        let world = World::new(workspace);
        self.create_workspace(&world)?;
        Ok(world)
    }

    pub fn raw_connection(&self) -> &Connection {
        &self.connection
    }
}

fn storage_error(error: impl std::fmt::Display) -> ZodoError {
    ZodoError::new(ErrorCode::StorageFailure, error.to_string())
}

fn parse_status(value: &str) -> ResultStatus {
    serde_json::from_value(serde_json::json!(value)).unwrap_or(ResultStatus::Committed)
}

impl Store for SqliteStore {
    fn load_world(&mut self, workspace_id: &str) -> Result<Option<World>> {
        let workspace_row = self
            .connection
            .query_row(
                "SELECT json FROM workspaces WHERE id = ?1",
                params![workspace_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(storage_error)?;
        let Some(workspace_json) = workspace_row else {
            return Ok(None);
        };
        let workspace: Workspace = serde_json::from_str(&workspace_json).map_err(storage_error)?;
        let mut statement = self
            .connection
            .prepare("SELECT kind, id, json FROM entities WHERE workspace_id = ?1")
            .map_err(storage_error)?;
        let rows = statement
            .query_map(params![workspace_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(storage_error)?;
        let mut entities = BTreeMap::new();
        for row in rows {
            let (kind, id, json) = row.map_err(storage_error)?;
            let value = serde_json::from_str(&json).map_err(storage_error)?;
            entities.insert((kind, id), value);
        }
        World::from_entities(workspace, entities).map(Some)
    }

    fn find_receipt(
        &mut self,
        workspace_id: &str,
        principal_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<StoredReceipt>> {
        self.connection
            .query_row(
                "SELECT principal_id, idempotency_key, request_hash, operation_id, status, revision, result_json, event_cursor
                 FROM operation_receipts WHERE workspace_id = ?1 AND principal_id = ?2 AND idempotency_key = ?3",
                params![workspace_id, principal_id, idempotency_key],
                |row| {
                    Ok(StoredReceipt {
                        principal_id: row.get(0)?,
                        idempotency_key: row.get(1)?,
                        request_hash: row.get(2)?,
                        operation_id: row.get(3)?,
                        status: parse_status(&row.get::<_, String>(4)?),
                        revision: row.get::<_, i64>(5)? as u64,
                        data: serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or(serde_json::Value::Null),
                        event_cursor: row.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(storage_error)
    }

    fn save(
        &mut self,
        world: &World,
        options: SaveOptions,
        receipt: Option<ReceiptInsert>,
        events: &[NewEvent],
        audit: &[NewAudit],
        effects: &[NewEffect],
    ) -> Result<SaveOutcome> {
        let tx = self.connection.transaction().map_err(storage_error)?;
        let current: Option<i64> = tx
            .query_row(
                "SELECT revision FROM workspaces WHERE id = ?1",
                params![world.workspace.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        let Some(current) = current else {
            return Err(ZodoError::not_found("workspace", &world.workspace.id));
        };
        let current = current as u64;
        if current != options.expected_revision {
            return Err(ZodoError::conflict(options.expected_revision, current));
        }
        let expected_new = if options.bump_revision {
            current.saturating_add(1)
        } else {
            current
        };
        if world.workspace.revision != expected_new {
            return Err(ZodoError::new(
                ErrorCode::StorageFailure,
                "save requested a revision that does not match the world",
            ));
        }
        save_entities(&tx, world)?;
        let event_cursor = save_events(&tx, events)?;
        save_audit(&tx, audit)?;
        save_effects(&tx, effects)?;
        if let Some(receipt) = receipt {
            tx.execute(
                "INSERT INTO operation_receipts
                 (workspace_id, principal_id, idempotency_key, request_hash, operation_id, status, revision, result_json, event_cursor, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    world.workspace.id,
                    receipt.principal_id,
                    receipt.idempotency_key,
                    receipt.request_hash,
                    receipt.operation_id,
                    serde_json::to_string(&receipt.status).unwrap_or_else(|_| "\"committed\"".to_string()).trim_matches('"'),
                    receipt.revision as i64,
                    serde_json::to_string(&receipt.data).map_err(storage_error)?,
                    event_cursor,
                    receipt.created_at.to_rfc3339(),
                ],
            )
            .map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)?;
        Ok(SaveOutcome {
            revision: world.workspace.revision,
            event_cursor,
        })
    }

    fn list_events(
        &mut self,
        workspace_id: &str,
        after_seq: u64,
        limit: u32,
    ) -> Result<Vec<EventRecord>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT seq, workspace_id, type, actor_id, payload, created_at FROM events
                 WHERE workspace_id = ?1 AND seq > ?2 ORDER BY seq ASC LIMIT ?3",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map(
                params![workspace_id, after_seq as i64, limit as i64],
                |row| {
                    Ok(EventRecord {
                        seq: row.get::<_, i64>(0)? as u64,
                        workspace_id: row.get(1)?,
                        event_type: row.get(2)?,
                        actor_id: row.get(3)?,
                        payload: serde_json::from_str(&row.get::<_, String>(4)?)
                            .unwrap_or(serde_json::Value::Null),
                        created_at: row
                            .get::<_, String>(5)?
                            .parse::<DateTime<Utc>>()
                            .unwrap_or_else(|_| Utc::now()),
                    })
                },
            )
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    fn list_audit(
        &mut self,
        workspace_id: &str,
        after_seq: u64,
        limit: u32,
    ) -> Result<Vec<AuditRecord>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT seq, workspace_id, actor_id, principal_kind, channel, operation, operation_id, revision, status, summary, created_at
                 FROM audit_events WHERE workspace_id = ?1 AND seq > ?2 ORDER BY seq ASC LIMIT ?3",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map(
                params![workspace_id, after_seq as i64, limit as i64],
                |row| {
                    Ok(AuditRecord {
                        seq: row.get::<_, i64>(0)? as u64,
                        workspace_id: row.get(1)?,
                        actor_id: row.get(2)?,
                        principal_kind: row.get(3)?,
                        channel: row.get(4)?,
                        operation: row.get(5)?,
                        operation_id: row.get(6)?,
                        revision: row.get::<_, i64>(7)? as u64,
                        status: row.get(8)?,
                        summary: serde_json::from_str(&row.get::<_, String>(9)?)
                            .unwrap_or(serde_json::Value::Null),
                        created_at: row
                            .get::<_, String>(10)?
                            .parse::<DateTime<Utc>>()
                            .unwrap_or_else(|_| Utc::now()),
                    })
                },
            )
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    fn latest_cursor(&mut self, workspace_id: &str) -> Result<String> {
        let seq: Option<i64> = self
            .connection
            .query_row(
                "SELECT MAX(seq) FROM events WHERE workspace_id = ?1",
                params![workspace_id],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        Ok(format!("ev_{}", seq.unwrap_or(0)))
    }

    fn pending_effects(&mut self, now: DateTime<Utc>, limit: u32) -> Result<Vec<EffectRecord>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, workspace_id, effect_type, payload, status, attempts, dedupe_key, available_at, last_error
                 FROM effect_outbox WHERE status = 'pending' AND available_at <= ?1 ORDER BY available_at ASC LIMIT ?2",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map(params![now.to_rfc3339(), limit as i64], |row| {
                Ok(EffectRecord {
                    id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    effect_type: row.get(2)?,
                    payload: serde_json::from_str(&row.get::<_, String>(3)?)
                        .unwrap_or(serde_json::Value::Null),
                    status: row.get(4)?,
                    attempts: row.get::<_, i64>(5)? as u32,
                    dedupe_key: row.get(6)?,
                    available_at: row
                        .get::<_, String>(7)?
                        .parse::<DateTime<Utc>>()
                        .unwrap_or_else(|_| Utc::now()),
                    last_error: row.get(8)?,
                })
            })
            .map_err(storage_error)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)
    }

    fn update_effect(
        &mut self,
        id: &str,
        status: &str,
        attempts: u32,
        last_error: Option<&str>,
        available_at: DateTime<Utc>,
    ) -> Result<()> {
        self.connection
            .execute(
                "UPDATE effect_outbox SET status = ?2, attempts = ?3, last_error = ?4, available_at = ?5, updated_at = ?6 WHERE id = ?1",
                params![
                    id,
                    status,
                    attempts as i64,
                    last_error,
                    available_at.to_rfc3339(),
                    Utc::now().to_rfc3339(),
                ],
            )
            .map_err(storage_error)?;
        Ok(())
    }
}

fn save_entities(tx: &Transaction<'_>, world: &World) -> Result<()> {
    let mut existing = BTreeMap::new();
    {
        let mut statement = tx
            .prepare("SELECT kind, id, json FROM entities WHERE workspace_id = ?1")
            .map_err(storage_error)?;
        let rows = statement
            .query_map(params![world.workspace.id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(storage_error)?;
        for row in rows {
            let (kind, id, json) = row.map_err(storage_error)?;
            existing.insert((kind, id), json);
        }
    }
    let next = world.entity_json_map();
    for ((kind, id), value) in &next {
        let json = serde_json::to_string(value).map_err(storage_error)?;
        let changed = existing
            .get(&(kind.clone(), id.clone()))
            .is_none_or(|previous| previous != &json);
        if changed {
            let updated_at = value
                .get("updated_at")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| world.workspace.updated_at.to_rfc3339());
            tx.execute(
                "INSERT INTO entities (workspace_id, kind, id, status, json, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(workspace_id, kind, id) DO UPDATE SET status = excluded.status, json = excluded.json, updated_at = excluded.updated_at",
                params![
                    world.workspace.id,
                    kind,
                    id,
                    value.get("status").and_then(|v| v.as_str()),
                    json,
                    updated_at,
                ],
            )
            .map_err(storage_error)?;
        }
    }
    for (kind, id) in existing.keys() {
        if !next.contains_key(&(kind.clone(), id.clone())) {
            tx.execute(
                "DELETE FROM entities WHERE workspace_id = ?1 AND kind = ?2 AND id = ?3",
                params![world.workspace.id, kind, id],
            )
            .map_err(storage_error)?;
        }
    }
    let workspace_json = serde_json::to_string(&world.workspace).map_err(storage_error)?;
    tx.execute(
        "UPDATE workspaces SET name = ?2, timezone = ?3, revision = ?4, policy_revision = ?5, json = ?6, updated_at = ?7 WHERE id = ?1",
        params![
            world.workspace.id,
            world.workspace.name,
            world.workspace.settings.timezone,
            world.workspace.revision as i64,
            world.workspace.policy_revision as i64,
            workspace_json,
            world.workspace.updated_at.to_rfc3339(),
        ],
    )
    .map_err(storage_error)?;
    Ok(())
}

fn save_events(tx: &Transaction<'_>, events: &[NewEvent]) -> Result<String> {
    let mut last_seq = None;
    for event in events {
        tx.execute(
            "INSERT INTO events (workspace_id, type, actor_id, payload, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                event.workspace_id,
                event.event_type,
                event.actor_id,
                serde_json::to_string(&event.payload).map_err(storage_error)?,
                event.created_at.to_rfc3339(),
            ],
        )
        .map_err(storage_error)?;
        last_seq = Some(tx.last_insert_rowid());
    }
    let seq = match last_seq {
        Some(value) => value,
        None => tx
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM events", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(storage_error)?,
    };
    Ok(format!("ev_{seq}"))
}

fn save_audit(tx: &Transaction<'_>, audit: &[NewAudit]) -> Result<()> {
    for entry in audit {
        tx.execute(
            "INSERT INTO audit_events
             (workspace_id, actor_id, principal_kind, channel, operation, operation_id, revision, status, summary, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                entry.workspace_id,
                entry.actor_id,
                entry.principal_kind,
                entry.channel,
                entry.operation,
                entry.operation_id,
                entry.revision as i64,
                entry.status,
                serde_json::to_string(&entry.summary).map_err(storage_error)?,
                entry.created_at.to_rfc3339(),
            ],
        )
        .map_err(storage_error)?;
    }
    Ok(())
}

fn save_effects(tx: &Transaction<'_>, effects: &[NewEffect]) -> Result<()> {
    for effect in effects {
        tx.execute(
            "INSERT INTO effect_outbox
             (id, workspace_id, effect_type, payload, status, attempts, dedupe_key, available_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'pending', 0, ?5, ?6, ?7, ?7)
             ON CONFLICT(id) DO NOTHING",
            params![
                effect.id,
                effect.workspace_id,
                effect.effect_type,
                serde_json::to_string(&effect.payload).map_err(storage_error)?,
                effect.dedupe_key,
                effect.available_at.to_rfc3339(),
                Utc::now().to_rfc3339(),
            ],
        )
        .map_err(storage_error)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zodo_kernel::model::Workspace;

    #[test]
    fn creates_and_reloads_a_world() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zodo.db");
        let mut store = SqliteStore::open(&path).unwrap();
        let world = World::new(Workspace::new(
            "ws_test".to_string(),
            "Test".to_string(),
            Utc::now(),
        ));
        store.create_workspace(&world).unwrap();
        let loaded = store.load_world("ws_test").unwrap().unwrap();
        assert_eq!(loaded.workspace.id, "ws_test");
        assert_eq!(loaded.workspace.revision, 0);
    }

    #[test]
    fn rejects_a_stale_revision() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SqliteStore::open(dir.path().join("zodo.db")).unwrap();
        let world = World::new(Workspace::new(
            "ws".to_string(),
            "Test".to_string(),
            Utc::now(),
        ));
        store.create_workspace(&world).unwrap();
        let mut next = world.clone();
        next.workspace.revision = 1;
        store
            .save(
                &next,
                SaveOptions {
                    expected_revision: 0,
                    bump_revision: true,
                },
                None,
                &[],
                &[],
                &[],
            )
            .unwrap();
        let mut stale = next.clone();
        stale.workspace.revision = 2;
        let error = store
            .save(
                &stale,
                SaveOptions {
                    expected_revision: 0,
                    bump_revision: true,
                },
                None,
                &[],
                &[],
                &[],
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RevisionConflict);
    }
}
