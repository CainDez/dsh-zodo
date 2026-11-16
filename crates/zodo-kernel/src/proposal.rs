use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::apply::{ApplyOutcome, apply_change};
use crate::commands::canonical_json;
use crate::error::{Result, ZodoError};
use crate::model::World;
use crate::ops::ChangeOp;
use crate::ports::IdSource;

pub fn proposal_content_hash(
    workspace_id: &str,
    intent: &str,
    operations: &[ChangeOp],
    base_revision: u64,
    policy_revision: u64,
) -> String {
    let value = json!({
        "hash_version": crate::model::HASH_VERSION,
        "workspace_id": workspace_id,
        "intent": intent,
        "operations": operations,
        "base_revision": base_revision,
        "policy_revision": policy_revision,
    });
    hex::encode(Sha256::digest(canonical_json(&value).as_bytes()))
}

pub fn apply_operations(
    world: &mut World,
    operations: &[ChangeOp],
    actor_id: &str,
    channel: &str,
    now: DateTime<Utc>,
    ids: &dyn IdSource,
) -> Result<ApplyOutcome> {
    let mut outcome = ApplyOutcome::default();
    let mut refs: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for operation in operations {
        if let ChangeOp::DependencyAddRef {
            predecessor_ref,
            successor_ref,
        } = operation
        {
            let predecessor_id = refs.get(predecessor_ref).cloned().ok_or_else(|| {
                ZodoError::validation(format!(
                    "proposal dependency ref {predecessor_ref} was not created earlier in this batch"
                ))
            })?;
            let successor_id = refs.get(successor_ref).cloned().ok_or_else(|| {
                ZodoError::validation(format!(
                    "proposal dependency ref {successor_ref} was not created earlier in this batch"
                ))
            })?;
            apply_change(
                world,
                &ChangeOp::DependencyAdd {
                    predecessor_id,
                    successor_id,
                },
                actor_id,
                channel,
                now,
                ids,
                &mut outcome,
            )?;
            continue;
        }
        apply_change(world, operation, actor_id, channel, now, ids, &mut outcome)?;
        if let ChangeOp::TaskCreate { task } = operation {
            if let Some(client_ref) = &task.client_ref {
                if let Some(change) = outcome
                    .changes
                    .iter()
                    .rev()
                    .find(|change| change.kind == "task")
                {
                    refs.insert(client_ref.clone(), change.id.clone());
                }
            }
        }
    }
    Ok(outcome)
}

/// A transport-neutral preview: additions, updates, removals and explicit unplaced work. The
/// preview is derived from the same deterministic apply path that commit uses, so it cannot claim
/// effects that commit would not produce.
pub fn preview_diff(before: &World, after: &World, outcome: &ApplyOutcome) -> Value {
    let before_map = before.entity_json_map();
    let after_map = after.entity_json_map();
    let mut added = Vec::new();
    let mut changed = Vec::new();
    let mut removed = Vec::new();
    for ((kind, id), value) in &after_map {
        match before_map.get(&(kind.clone(), id.clone())) {
            None => added.push(json!({
                "kind": kind,
                "id": id,
                "summary": summary(value),
            })),
            Some(previous) if previous != value => changed.push(json!({
                "kind": kind,
                "id": id,
                "summary": summary(value),
                "before": previous,
                "after": value,
            })),
            Some(_) => {}
        }
    }
    for ((kind, id), value) in &before_map {
        if !after_map.contains_key(&(kind.clone(), id.clone())) {
            removed.push(json!({
                "kind": kind,
                "id": id,
                "summary": summary(value),
            }));
        }
    }
    json!({
        "added": added,
        "changed": changed,
        "removed": removed,
        "changes": outcome.changes,
        "warnings": outcome.warnings,
        "counts": {
            "added": after_map.len().saturating_sub(before_map.len()) + removed.len().saturating_sub(0),
            "total_after": after_map.len(),
            "total_before": before_map.len(),
        }
    })
}

fn summary(value: &Value) -> String {
    value
        .get("title")
        .or_else(|| value.get("intent"))
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
        .or_else(|| value.get("id").and_then(Value::as_str))
        .unwrap_or("")
        .to_string()
}

pub fn proposal_effects(operations: &[ChangeOp]) -> Vec<String> {
    let mut effects = BTreeMap::new();
    for operation in operations {
        for effect in operation.effects() {
            effects.insert(effect, ());
        }
    }
    effects.into_keys().collect()
}
