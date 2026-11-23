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

fn seed_allocation(
    f: &mut Fixture,
    task: &str,
    block: Option<&str>,
    start: &str,
    end: &str,
    minutes: u32,
    locked: bool,
) -> String {
    f.execute("allocation.create", json!({"allocation": {
        "task_id": task, "capacity_block_id": block, "start": format!("2026-10-25T{start}:00Z"), "end": format!("2026-10-25T{end}:00Z"),
        "minutes": minutes, "locked": locked
    }})).unwrap().data["changes"][0]["id"].as_str().unwrap().to_owned()
}

fn move_payload(f: &mut Fixture, id: &str, block: &str, start: &str, end: &str) -> Value {
    let world = f.world();
    let row = world.allocations.iter().find(|row| row.id == id).unwrap();
    json!({"id": id, "expected_task_version": world.task(&row.task_id).unwrap().version, "capacity_block_id": block,
        "start": format!("2026-10-25T{start}:00Z"), "end": format!("2026-10-25T{end}:00Z")})
}

fn propose_move(f: &mut Fixture, payload: Value) -> CommandResult {
    f.execute("proposal.create", json!({"intent": "Move this slot", "operations": [{"operation": "allocation.move", "payload": payload}]})).unwrap()
}

fn reject_move(f: &mut Fixture, payload: Value, reason: &str) {
    let before = f.world();
    let proposal = propose_move(f, payload);
    assert_eq!(proposal.data["proposal"]["status"], "draft");
    let diagnostics = &proposal.data["proposal"]["diagnostics"];
    assert!(diagnostics.to_string().contains(reason), "{diagnostics}");
    assert!(
        f.execute(
            "proposal.approve",
            json!({"proposal_id": proposal.data["proposal"]["id"]})
        )
        .is_err()
    );
    assert_eq!(f.world().allocations, before.allocations);
    assert_eq!(f.world().workspace.revision, before.workspace.revision);
}

#[test]
fn strict_move_preserves_identity_duration_and_all_other_facts_and_uses_only_its_own_reservation() {
    let mut f = Fixture::new();
    let task = f.task(json!({"estimate_minutes": 30}));
    let other = f.task(json!({}));
    let block = f.capacity(30); // Source already consumes the full remaining work and capacity.
    let id = seed_allocation(&mut f, &task, Some(&block), "02:00", "02:30", 30, false);
    seed_allocation(&mut f, &other, None, "03:00", "03:30", 30, true);
    f.clock
        .set(Utc.with_ymd_and_hms(2026, 10, 25, 1, 50, 0).unwrap());
    let before = f.world();
    let payload = move_payload(&mut f, &id, &block, "02:10", "02:40"); // Physically overlaps old slot.
    assert!(f.execute("allocation.move", payload.clone()).is_err());
    let proposal = propose_move(&mut f, payload);
    let p = &proposal.data["proposal"];
    assert_eq!(p["status"], "validated");
    assert_eq!(p["preview"]["added"], json!([]));
    assert_eq!(p["preview"]["removed"], json!([]));
    assert_eq!(p["preview"]["changed"].as_array().unwrap().len(), 1);
    assert_eq!(p["preview"]["changed"][0]["id"], id);
    assert_eq!(
        p["preview"]["changed"][0]["after"]["start"],
        "2026-10-25T02:10:00Z"
    );
    assert_eq!(f.world().allocations, before.allocations);
    let ids = f.approve(&proposal);
    assert_eq!(f.world().allocations, before.allocations);
    f.clock
        .set(Utc.with_ymd_and_hms(2026, 10, 25, 1, 55, 0).unwrap());
    let result = f.commit(&ids).unwrap();
    assert_eq!(
        result.data["changed"],
        json!([{"id": id, "kind": "allocation", "action": "moved"}])
    );
    assert_eq!(result.data["effects"], json!([]));
    let mut expected = before.allocations.clone();
    let moved = expected.iter_mut().find(|row| row.id == id).unwrap();
    moved.start = Utc.with_ymd_and_hms(2026, 10, 25, 2, 10, 0).unwrap();
    moved.end = Utc.with_ymd_and_hms(2026, 10, 25, 2, 40, 0).unwrap();
    moved.updated_at = Utc.with_ymd_and_hms(2026, 10, 25, 1, 55, 0).unwrap();
    let after = f.world();
    assert_eq!(after.allocations, expected);
    assert_eq!(after.tasks, before.tasks);
    assert_eq!(after.commitments, before.commitments);
    assert_eq!(after.capacity_blocks, before.capacity_blocks);
    assert_eq!(after.calendar_events, before.calendar_events);
    assert_eq!(after.activities, before.activities);
    assert!(f.commit(&ids).is_err());
    assert_eq!(f.world().allocations, expected);
}

#[test]
fn strict_move_rejects_noop_duration_stale_version_missing_capacity_locked_and_overlap() {
    let mut f = Fixture::new();
    let task = f.task(json!({}));
    let other = f.task(json!({}));
    let block = f.capacity(180);
    let id = seed_allocation(&mut f, &task, Some(&block), "02:00", "02:30", 30, false);
    let locked = seed_allocation(&mut f, &other, None, "03:00", "03:30", 30, true);
    for (source, capacity, start, end, reason) in [
        (
            &id,
            block.as_str(),
            "02:00",
            "02:30",
            "destination is unchanged",
        ),
        (
            &id,
            block.as_str(),
            "01:00",
            "01:20",
            "preserve its duration",
        ),
        (&id, "missing", "01:00", "01:30", "capacity_block"),
        (
            &id,
            block.as_str(),
            "03:00",
            "03:30",
            "overlaps another allocation",
        ),
        (
            &locked,
            block.as_str(),
            "01:00",
            "01:30",
            "only unlocked planned allocations",
        ),
    ] {
        let payload = move_payload(&mut f, source, capacity, start, end);
        reject_move(&mut f, payload, reason);
    }
    // Generic/manual create can store extra seconds while accounting in whole
    // minutes. A strict move must not silently shorten that original interval.
    let fractional = f
        .execute(
            "allocation.create",
            json!({"allocation": {
                "task_id": other, "start": "2026-10-25T03:30:01Z", "end": "2026-10-25T04:00:02Z",
                "minutes": 30
            }}),
        )
        .unwrap()
        .data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let payload = move_payload(&mut f, &fractional, &block, "02:30", "03:00");
    reject_move(&mut f, payload, "preserve its duration");
    let mut stale = move_payload(&mut f, &id, &block, "01:00", "01:30");
    stale["expected_task_version"] = json!(999);
    reject_move(&mut f, stale, "task changed");
}

#[test]
fn strict_move_rejects_commit_if_either_start_or_revision_changed() {
    for scenario in ["source started", "destination started", "revision changed"] {
        let mut f = Fixture::new();
        let task = f.task(json!({}));
        let block = f.capacity(180);
        let source_start = if scenario == "source started" {
            "02:00"
        } else {
            "02:30"
        };
        let source_end = if scenario == "source started" {
            "02:30"
        } else {
            "03:00"
        };
        let target_start = if scenario == "source started" {
            "02:30"
        } else {
            "02:00"
        };
        let target_end = if scenario == "source started" {
            "03:00"
        } else {
            "02:30"
        };
        let id = seed_allocation(
            &mut f,
            &task,
            Some(&block),
            source_start,
            source_end,
            30,
            false,
        );
        f.clock
            .set(Utc.with_ymd_and_hms(2026, 10, 25, 1, 59, 30).unwrap());
        let payload = move_payload(&mut f, &id, &block, target_start, target_end);
        let p = propose_move(&mut f, payload);
        assert_eq!(p.data["proposal"]["status"], "validated");
        let ids = f.approve(&p);
        if scenario == "revision changed" {
            f.task(json!({"title": "Concurrent change"}));
        } else {
            f.clock
                .set(Utc.with_ymd_and_hms(2026, 10, 25, 2, 0, 0).unwrap());
        }
        let before = f.world();
        let error = f.commit(&ids).unwrap_err();
        if scenario == "revision changed" {
            assert_eq!(error.code, zodo_kernel::ErrorCode::RevisionConflict);
        } else {
            assert!(error.message.contains("future"), "{error:?}");
        }
        let after = f.world();
        assert_eq!(after.workspace.revision, before.workspace.revision);
        assert_eq!(after.allocations, before.allocations);
        assert_eq!(after.approval_grants, before.approval_grants);
    }
}

#[test]
fn strict_move_preserves_successor_boundary_and_requires_full_predecessor_work() {
    for required in [30, 60] {
        let mut f = Fixture::new();
        let predecessor = f.task(json!({"estimate_minutes": required}));
        let successor = f.task(json!({}));
        let block = f.capacity(180);
        f.execute(
            "dependency.add",
            json!({"predecessor_id": predecessor, "successor_id": successor}),
        )
        .unwrap();
        let id = seed_allocation(&mut f, &predecessor, None, "01:00", "01:30", 30, false);
        let successor_id = seed_allocation(&mut f, &successor, None, "02:00", "02:30", 30, false);
        let after_successor = move_payload(&mut f, &id, &block, "02:30", "03:00");
        reject_move(&mut f, after_successor, "retained successor interval");
        let at_boundary = move_payload(&mut f, &id, &block, "01:30", "02:00");
        if required == 60 {
            reject_move(&mut f, at_boundary, "incompletely scheduled");
        } else {
            let before = f.world();
            let p = propose_move(&mut f, at_boundary);
            assert_eq!(p.data["proposal"]["status"], "validated");
            let ids = f.approve(&p);
            f.commit(&ids).unwrap();
            let after = f.world();
            assert_eq!(
                after.allocations.iter().find(|a| a.id == successor_id),
                before.allocations.iter().find(|a| a.id == successor_id)
            );
            assert_eq!(
                after
                    .allocations
                    .iter()
                    .find(|a| a.id == id)
                    .unwrap()
                    .capacity_block_id
                    .as_deref(),
                Some(block.as_str())
            );
        }
    }
}

fn tree_query(f: &mut Fixture, query: &str, payload: Value) -> Value {
    use zodo_kernel::commands::QueryEnvelope;
    use zodo_kernel::model::SCHEMA_QUERY;
    let mut app = Application {
        store: &mut f.store,
        clock: &f.clock,
        ids: &f.ids,
        instance_id: "rt_test".into(),
    };
    app.query(
        &f.context,
        QueryEnvelope {
            schema: SCHEMA_QUERY.into(),
            request_id: "query_tree".into(),
            workspace_id: "ws_test".into(),
            query: query.into(),
            payload,
        },
    )
    .unwrap()
    .data
}

#[test]
fn tree_first_child_requires_no_active_focus_or_unsettled_allocation_and_move_preserves_work() {
    let mut f = Fixture::new();
    let root = f.task(json!({}));
    let other = f.task(json!({}));
    let child_input = json!({"task":{"title":"Child", "parent_id":root}});
    f.execute("focus.start", json!({"task_id":root})).unwrap();
    f.execute("focus.pause", json!({})).unwrap();
    let before = f.world();
    let err = f.execute("task.create", child_input.clone()).unwrap_err();
    assert!(err.message.contains("active focus"));
    assert_eq!(f.world().tasks, before.tasks);
    assert_eq!(f.world().focus_sessions, before.focus_sessions);
    f.execute("focus.cancel", json!({})).unwrap();
    let historical = seed_allocation(&mut f, &root, None, "01:00", "01:15", 15, true);
    let reserved = seed_allocation(&mut f, &root, None, "02:00", "02:15", 15, false);
    f.clock
        .set(Utc.with_ymd_and_hms(2026, 10, 25, 1, 20, 0).unwrap());
    let before = f.world();
    let err = f.execute("task.create", child_input).unwrap_err();
    assert!(err.message.contains("outstanding allocations"));
    assert_eq!(f.world().allocations, before.allocations);
    let mut move_other = json!({"id":other, "expected_version":1, "task":{"title":"Moved", "estimate_minutes":60,"parent_id":root}});
    assert!(
        f.execute("task.update", move_other.clone())
            .unwrap_err()
            .message
            .contains("outstanding allocations")
    );
    f.execute("allocation.remove", json!({"id":reserved}))
        .unwrap();
    f.execute(
        "progress.record",
        json!({"task_id":other,"minutes":10,"remaining_minutes":30}),
    )
    .unwrap();
    seed_allocation(&mut f, &other, None, "03:00", "03:15", 15, false);
    let before = f.world();
    assert!(before.allocations.iter().any(|row| row.id == historical));
    let err = f.execute("task.update", move_other.clone()).unwrap_err();
    assert_eq!(err.code, zodo_kernel::error::ErrorCode::RevisionConflict);
    assert_eq!(f.world().workspace.revision, before.workspace.revision);
    move_other["expected_version"] = json!(before.task(&other).unwrap().version);
    let moved = f.execute("task.update", move_other).unwrap();
    assert_eq!(moved.data["warnings"], json!([]));
    let after = f.world();
    assert_eq!(after.allocations, before.allocations);
    assert_eq!(after.focus_sessions, before.focus_sessions);
    assert_eq!(after.task(&root), before.task(&root));
    let moved = after.task(&other).unwrap();
    assert_eq!(moved.parent_id.as_deref(), Some(root.as_str()));
    assert_eq!(
        (
            moved.estimate_minutes,
            moved.remaining_minutes,
            moved.actual_minutes
        ),
        (Some(60), Some(30), 10)
    );
}

#[test]
fn tree_status_guards_do_not_cascade_and_archived_child_does_not_make_parent_leaf() {
    let mut f = Fixture::new();
    let root = f.task(json!({"target_due":"2026-10-25"}));
    let middle = f.task(json!({"parent_id":root}));
    let leaf = f.task(json!({"parent_id":middle}));
    f.execute("task.archive", json!({"id":leaf,"archived":true}))
        .unwrap();
    let before = f.world();
    for op in ["task.complete", "task.cancel", "task.archive"] {
        let input = if op == "task.archive" {
            json!({"id":middle, "archived":true})
        } else {
            json!({"id":middle})
        };
        assert!(
            f.execute(op, input)
                .unwrap_err()
                .message
                .contains("unfinished descendants")
        );
    }
    assert_eq!(f.world().workspace.revision, before.workspace.revision);
    assert_eq!(f.world().tasks, before.tasks);
    assert_eq!(
        tree_query(&mut f, "tasks.list", json!({"filter":{"only_leaf":true}}))["tasks"],
        json!([])
    );
    let leaves = tree_query(
        &mut f,
        "tasks.list",
        json!({"filter":{"only_leaf":true,"include_archived":true}}),
    );
    assert_eq!(leaves["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(leaves["tasks"][0]["task"]["id"], leaf);
    f.execute("task.complete", json!({"id":leaf})).unwrap();
    f.execute("task.cancel", json!({"id":middle})).unwrap();
    f.execute("task.complete", json!({"id":root})).unwrap();
    assert_eq!(
        f.world().task(&leaf).unwrap().status,
        zodo_kernel::model::TaskStatus::Done
    );
    let before = f.world();
    for (op, input) in [
        ("task.reopen", json!({"id":middle})),
        ("task.archive", json!({"id":leaf,"archived":false})),
        (
            "task.create",
            json!({"task":{"title":"Too late","parent_id":root}}),
        ),
    ] {
        assert!(
            f.execute(op, input)
                .unwrap_err()
                .message
                .contains("closed or archived ancestor")
        );
    }
    assert_eq!(f.world().tasks, before.tasks);
    f.execute("task.reopen", json!({"id":root})).unwrap();
    f.execute("task.reopen", json!({"id":middle})).unwrap();
    f.execute("task.archive", json!({"id":leaf,"archived":false}))
        .unwrap();
    f.execute("task.reopen", json!({"id":leaf})).unwrap();
    assert!(!f.world().is_leaf(&middle));
}

#[test]
fn tree_preview_and_approval_are_leaf_only_and_keep_parent_schedule_history_queryable() {
    let mut f = Fixture::new();
    let rollup = f.task(json!({"estimate_minutes":30, "target_due":"2026-10-25"}));
    let candidate = f.task(json!({"estimate_minutes":30}));
    let block = f.capacity(180);
    let start = "2026-10-25T01:30:00Z";
    let end = "2026-10-25T04:00:00Z";
    let historical = seed_allocation(&mut f, &rollup, None, "01:00", "01:15", 15, false);
    f.clock
        .set(Utc.with_ymd_and_hms(2026, 10, 25, 1, 20, 0).unwrap());
    let strict_slot = f.slot(&rollup, &block);
    let strict = f.propose(strict_slot).unwrap();
    let strict_grant = f.approve(&strict);
    let plan = f
        .execute(
            "plan.propose",
            json!({"task_ids":[rollup],"horizon_start":start,"horizon_end":end,"move_unlocked":false}),
        )
        .unwrap();
    let plan_grant = f.approve(&plan);
    assert_eq!(f.world().allocations.len(), 1);
    let child = f.task(json!({"parent_id":rollup}));
    f.execute("task.cancel", json!({"id":child})).unwrap();
    f.execute("task.archive", json!({"id":child,"archived":true}))
        .unwrap();
    let before = f.world();
    let grants = before.approval_grants.clone();
    assert!(f.commit(&strict_grant).is_err());
    assert!(f.commit(&plan_grant).is_err());
    assert_eq!(f.world().approval_grants, grants);
    assert_eq!(f.world().allocations, before.allocations);
    let query = tree_query(
        &mut f,
        "schedule.get",
        json!({"start":"2026-10-25T00:00:00Z","end":end}),
    );
    assert_eq!(query["allocations"][0]["id"], historical);
    assert!(
        query["unplaced"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["task_id"] != rollup)
    );
    let today = tree_query(&mut f, "today.get", json!({"date":"2026-10-25"}));
    assert_eq!(today["allocations"][0]["id"], historical);
    assert!(
        today["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["id"] != rollup)
    );
    let slot = f.slot(&rollup, &block);
    f.reject(slot);
    let err = f
        .execute(
            "plan.propose",
            json!({"task_ids":[rollup],"horizon_start":start,"horizon_end":end,"move_unlocked":false}),
        )
        .unwrap_err();
    assert!(err.message.contains("rollup parent"));
    assert!(f.execute("allocation.create",json!({"allocation":{"task_id":rollup,"start":"2026-10-25T02:00:00Z","end":"2026-10-25T02:15:00Z","minutes":15}})).unwrap_err().message.contains("rollup parent"));
    let proposal = f
        .execute(
            "plan.propose",
            json!({"task_ids":[],"horizon_start":start,"horizon_end":end,"move_unlocked":true}),
        )
        .unwrap();
    assert_eq!(
        proposal.data["plan"]["replace_task_ids"],
        json!([candidate])
    );
    assert_eq!(
        proposal.data["plan"]["fixed_allocations"],
        json!([historical])
    );
    assert_eq!(
        proposal.data["plan"]["assignments"][0]["task_id"],
        candidate
    );
    assert_eq!(f.world().allocations, before.allocations);
    let ids = f.approve(&proposal);
    assert_eq!(f.world().allocations, before.allocations);
    f.commit(&ids).unwrap();
    let after = f.world();
    assert_eq!(after.allocations.len(), 2);
    assert_eq!(
        after.allocations.iter().find(|row| row.id == historical),
        before.allocations.first()
    );
    assert!(after.allocations.iter().any(|row| row.task_id == candidate));
}
