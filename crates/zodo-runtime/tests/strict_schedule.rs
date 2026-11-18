use chrono::{TimeZone, Utc};
use serde_json::{Value, json};
use zodo_kernel::app::Application;
use zodo_kernel::commands::{CommandEnvelope, CommandResult};
use zodo_kernel::model::{CommandContext, PrincipalKind, Workspace};
use zodo_kernel::ports::{FixedClock, SequentialIds, Store};
use zodo_runtime::SqliteStore;

struct Fixture {
    store: SqliteStore,
    clock: FixedClock,
    ids: SequentialIds,
    context: CommandContext,
    counter: u64,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SqliteStore::open(dir.path().join("overlap.sqlite3")).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 10, 24, 1, 20, 0).unwrap();
        let world = zodo_kernel::model::World::new(Workspace::new(
            "ws_test".to_string(),
            "Test".to_string(),
            now,
        ));
        store.create_workspace(&world).unwrap();
        Self {
            store,
            clock: FixedClock::new(now),
            ids: SequentialIds::new(),
            context: CommandContext {
                principal_id: "user_ui".to_string(),
                principal_kind: PrincipalKind::InteractiveUi,
                channel: "test_ui".to_string(),
                capabilities: [
                    "read", "write", "capture", "propose", "approve", "execute", "admin",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                workspace_id: "ws_test".to_string(),
                request_id: "req_test".to_string(),
                received_at: now,
                runtime_instance: "rt_test".to_string(),
            },
            counter: 0,
            _dir: dir,
        }
    }

    fn execute(&mut self, operation: &str, payload: Value) -> zodo_kernel::Result<CommandResult> {
        self.counter += 1;
        let envelope = CommandEnvelope {
            schema: "zodo.command/1".to_string(),
            request_id: format!("req_{}", self.counter),
            idempotency_key: format!("op_{}", self.counter),
            workspace_id: "ws_test".to_string(),
            operation: operation.to_string(),
            operation_version: 1,
            base_revision: None,
            policy_revision: None,
            payload,
        };
        let mut app = Application {
            store: &mut self.store,
            clock: &self.clock,
            ids: &self.ids,
            instance_id: "rt_test".to_string(),
        };
        app.execute(&self.context, envelope)
    }

    fn task(&mut self, extra: Value) -> String {
        let mut task = json!({"title": "Work", "estimate_minutes": 60});
        task.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        self.execute("task.create", json!({"task": task}))
            .unwrap()
            .data["changes"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }
    fn capacity(&mut self, minutes: u32) -> String {
        self.execute("capacity.create", json!({"capacity": {"title": "Morning", "start": "2026-10-25T01:00:00Z", "end": "2026-10-25T04:00:00Z", "capacity_minutes": minutes}})).unwrap().data["changes"][0]["id"].as_str().unwrap().to_owned()
    }
    fn world(&mut self) -> zodo_kernel::model::World {
        self.store.load_world("ws_test").unwrap().unwrap()
    }
    fn slot(&mut self, task: &str, block: &str) -> Value {
        json!({"task_id": task, "expected_task_version": self.world().task(task).unwrap().version, "capacity_block_id": block, "start": "2026-10-25T02:00:00Z", "end": "2026-10-25T02:30:00Z"})
    }
    fn propose(&mut self, slot: Value) -> zodo_kernel::Result<CommandResult> {
        self.execute("proposal.create", json!({"intent": "Add this exact time slot", "operations": [{"operation": "allocation.schedule", "payload": {"allocation": slot}}]}))
    }
    fn approve(&mut self, proposal: &CommandResult) -> (String, String) {
        let id = proposal.data["proposal"]["id"].as_str().unwrap().to_owned();
        let approved = self
            .execute("proposal.approve", json!({"proposal_id": id}))
            .unwrap();
        (
            id,
            approved.data["approval"]["id"].as_str().unwrap().to_owned(),
        )
    }
    fn commit(&mut self, ids: &(String, String)) -> zodo_kernel::Result<CommandResult> {
        self.execute(
            "changes.commit",
            json!({"proposal_id": ids.0, "approval_grant": ids.1}),
        )
    }
    fn reject(&mut self, slot: Value) {
        let before = self.world();
        if let Ok(proposal) = self.propose(slot) {
            assert_eq!(proposal.data["proposal"]["status"], "draft");
            assert!(
                self.execute(
                    "proposal.approve",
                    json!({"proposal_id": proposal.data["proposal"]["id"]})
                )
                .is_err()
            );
        }
        let after = self.world();
        assert_eq!(after.allocations, before.allocations);
        assert_eq!(after.tasks, before.tasks);
        assert_eq!(after.activities, before.activities);
    }
}

#[test]
fn strict_slot_is_additive_preview_only_until_approved_commit() {
    let mut f = Fixture::new();
    let task = f.task(json!({}));
    let block = f.capacity(180);
    let old = f.task(json!({}));
    f.execute("allocation.create", json!({"allocation": {"task_id": old, "start": "2026-10-25T03:00:00Z", "end": "2026-10-25T03:30:00Z", "minutes": 30, "locked": true}})).unwrap();
    let before = f.world();
    let slot = f.slot(&task, &block);
    let proposal = f.propose(slot).unwrap();
    assert_eq!(proposal.data["proposal"]["status"], "validated");
    assert_eq!(f.world().allocations, before.allocations);
    assert!(
        f.execute(
            "changes.commit",
            json!({"proposal_id": proposal.data["proposal"]["id"], "approval_grant": "missing"})
        )
        .is_err()
    );
    let ids = f.approve(&proposal);
    assert_eq!(f.world().allocations, before.allocations);
    f.commit(&ids).unwrap();
    let after = f.world();
    assert_eq!(after.allocations.len(), 2);
    assert!(after.allocations.contains(&before.allocations[0]));
    assert_eq!(after.tasks, before.tasks);
    assert_eq!(after.activities, before.activities);
    let added = after
        .allocations
        .iter()
        .find(|a| a.task_id == task)
        .unwrap();
    assert_eq!(added.minutes, 30);
    assert!(!added.locked);
    assert_eq!(added.capacity_block_id.as_deref(), Some(block.as_str()));
    assert!(f.commit(&ids).is_err());
    assert_eq!(f.world().allocations, after.allocations);
}

#[test]
fn strict_slot_rejects_task_dates_parent_blocked_and_stale_version() {
    for extra in [
        json!({"not_before": "2026-10-25T03:00:00Z"}),
        json!({"hard_deadline": "2026-10-25T02:15:00Z"}),
        json!({}),
    ] {
        let mut f = Fixture::new();
        let task = f.task(extra.clone());
        let block = f.capacity(180);
        let mut slot = f.slot(&task, &block);
        if extra.as_object().unwrap().is_empty() {
            slot["expected_task_version"] = json!(999);
        }
        f.reject(slot);
    }
    let mut f = Fixture::new();
    let parent = f.task(json!({}));
    f.task(json!({"parent_id": parent}));
    let block = f.capacity(180);
    let slot = f.slot(&parent, &block);
    f.reject(slot);
    let mut f = Fixture::new();
    let task = f.task(json!({}));
    f.execute(
        "task.set_blocked",
        json!({"id": task, "blocked": true, "reason": "Waiting"}),
    )
    .unwrap();
    let block = f.capacity(180);
    let slot = f.slot(&task, &block);
    f.reject(slot);
}

#[test]
fn strict_slot_requires_existing_capacity_membership_and_budget() {
    for case in ["missing", "unknown", "outside", "budget"] {
        let mut f = Fixture::new();
        let task = f.task(json!({}));
        let block = f.capacity(if case == "budget" { 15 } else { 180 });
        let mut slot = f.slot(&task, &block);
        match case {
            "missing" => {
                slot.as_object_mut().unwrap().remove("capacity_block_id");
            }
            "unknown" => slot["capacity_block_id"] = json!("none"),
            "outside" => {
                slot["start"] = json!("2026-10-25T04:00:00Z");
                slot["end"] = json!("2026-10-25T04:30:00Z");
            }
            _ => {}
        }
        f.reject(slot);
    }
}

#[test]
fn strict_slot_rejects_busy_and_existing_allocation_overlap() {
    for busy in [true, false] {
        let mut f = Fixture::new();
        let task = f.task(json!({}));
        let block = f.capacity(180);
        if busy {
            f.execute("calendar.import_busy", json!({"source_id": null, "provider": "test", "display_name": "Private", "external_id": "cal", "events": [{"external_id": "busy", "title": "Secret event", "start": "2026-10-25T02:00:00Z", "end": "2026-10-25T02:30:00Z"}], "replace_source": true, "cursor": null})).unwrap();
        } else {
            let other = f.task(json!({}));
            f.execute("allocation.create", json!({"allocation": {"task_id": other, "start": "2026-10-25T02:00:00Z", "end": "2026-10-25T02:30:00Z", "minutes": 30, "locked": true}})).unwrap();
        }
        let slot = f.slot(&task, &block);
        f.reject(slot);
    }
}

#[test]
fn strict_slot_dependency_must_be_fully_reserved_before_start() {
    for minutes in [0, 30, 60] {
        let mut f = Fixture::new();
        let predecessor = f.task(json!({}));
        let successor = f.task(json!({}));
        let block = f.capacity(180);
        f.execute(
            "dependency.add",
            json!({"predecessor_id": predecessor, "successor_id": successor}),
        )
        .unwrap();
        if minutes > 0 {
            f.execute("allocation.create", json!({"allocation": {"task_id": predecessor, "start": "2026-10-25T01:00:00Z", "end": if minutes == 30 {"2026-10-25T01:30:00Z"} else {"2026-10-25T02:00:00Z"}, "minutes": minutes}})).unwrap();
        }
        let slot = f.slot(&successor, &block);
        if minutes < 60 {
            f.reject(slot);
        } else {
            let p = f.propose(slot).unwrap();
            let ids = f.approve(&p);
            f.commit(&ids).unwrap();
        }
    }
}

#[test]
fn strict_slot_rechecks_commit_time_and_revision_without_partial_writes() {
    for change_clock in [true, false] {
        let mut f = Fixture::new();
        let task = f.task(json!({}));
        let block = f.capacity(180);
        f.clock
            .set(Utc.with_ymd_and_hms(2026, 10, 25, 1, 59, 30).unwrap());
        let slot = f.slot(&task, &block);
        let p = f.propose(slot).unwrap();
        let ids = f.approve(&p);
        if change_clock {
            f.clock
                .set(Utc.with_ymd_and_hms(2026, 10, 25, 2, 0, 0).unwrap());
        } else {
            f.task(json!({"title": "Concurrent change"}));
        }
        let before = f.world();
        let error = f.commit(&ids).unwrap_err();
        if change_clock {
            assert!(error.message.contains("future"), "{error:?}");
        }
        assert_eq!(f.world().allocations, before.allocations);
    }
}

#[test]
fn strict_slot_enforces_hard_work_windows_and_weekend_denial() {
    for weekend in ["allow", "deny"] {
        let mut f = Fixture::new();
        let task = f.task(json!({}));
        let block = f.capacity(180);
        f.execute("workspace.update", json!({"settings": {"timezone": "Asia/Shanghai", "week_start": 1, "weekend_policy": weekend, "work_windows": [{"weekday": 7, "start_minute": 540, "end_minute": if weekend == "deny" {720} else {600}, "hard": true}], "default_estimate_minutes": 30, "daily_limit_minutes": null}, "policy": null})).unwrap();
        let slot = f.slot(&task, &block); // Sunday 10:00–10:30, outside the hard 09:00–10:00 window.
        f.reject(slot);
    }
}

#[test]
fn existing_manual_historical_create_stays_permissive() {
    let mut f = Fixture::new();
    let task = f.task(json!({"not_before": "2026-10-26T00:00:00Z"}));
    f.execute("allocation.create", json!({"allocation": {"task_id": task, "start": "2026-10-23T00:00:00Z", "end": "2026-10-23T00:30:00Z", "minutes": 30}})).unwrap();
    assert_eq!(f.world().allocations.len(), 1);
}

#[test]
fn strict_slot_allows_soft_windows_and_user_confirmed_weekend_ask() {
    let mut f = Fixture::new();
    let task = f.task(json!({}));
    let block = f.capacity(180);
    f.execute("workspace.update", json!({"settings": {"timezone": "Asia/Shanghai", "week_start": 1, "weekend_policy": "ask", "work_windows": [{"weekday": 7, "start_minute": 540, "end_minute": 600, "hard": false}], "default_estimate_minutes": 30, "daily_limit_minutes": null}, "policy": null})).unwrap();
    let slot = f.slot(&task, &block);
    let p = f.propose(slot).unwrap();
    let ids = f.approve(&p);
    f.commit(&ids).unwrap();
}

#[test]
fn strict_slot_expired_predecessor_reservation_is_not_completion_at_commit() {
    let mut f = Fixture::new();
    let predecessor = f.task(json!({"estimate_minutes": 30}));
    let task = f.task(json!({}));
    let block = f.capacity(180);
    f.execute(
        "dependency.add",
        json!({"predecessor_id": predecessor, "successor_id": task}),
    )
    .unwrap();
    f.execute("allocation.create", json!({"allocation": {"task_id": predecessor, "start": "2026-10-25T01:20:00Z", "end": "2026-10-25T01:50:00Z", "minutes": 30}})).unwrap();
    f.clock
        .set(Utc.with_ymd_and_hms(2026, 10, 25, 1, 49, 30).unwrap());
    let slot = f.slot(&task, &block);
    let p = f.propose(slot).unwrap();
    let ids = f.approve(&p);
    f.clock
        .set(Utc.with_ymd_and_hms(2026, 10, 25, 1, 50, 0).unwrap());
    let before = f.world();
    let error = f.commit(&ids).unwrap_err();
    assert!(error.message.contains("predecessor"));
    let after = f.world();
    assert_eq!(after.allocations, before.allocations);
    assert_eq!(after.tasks, before.tasks);
}

#[test]
fn strict_slot_batch_conflict_is_atomic_and_agent_cannot_approve() {
    let mut f = Fixture::new();
    let task = f.task(json!({}));
    let block = f.capacity(180);
    let slot = f.slot(&task, &block);
    let operation = json!({"operation": "allocation.schedule", "payload": {"allocation": slot}});
    let before = f.world();
    let proposal = f.execute("proposal.create", json!({"intent": "Invalid duplicate batch", "operations": [operation.clone(), operation]})).unwrap();
    assert_eq!(proposal.data["proposal"]["status"], "draft");
    assert_eq!(f.world().allocations, before.allocations);
    let p = f.propose(slot).unwrap();
    f.context.principal_kind = PrincipalKind::DelegatedAgent;
    assert!(
        f.execute(
            "proposal.approve",
            json!({"proposal_id": p.data["proposal"]["id"]})
        )
        .is_err()
    );
    assert_eq!(f.world().allocations, before.allocations);
}
