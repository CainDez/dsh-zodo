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

    fn task(
        &mut self,
        title: &str,
        minutes: u32,
        priority: u32,
        not_before: Option<&str>,
    ) -> String {
        self.execute("task.create", json!({"task": {
            "title": title, "estimate_minutes": minutes, "priority": priority, "not_before": not_before
        }})).unwrap().data["changes"][0]["id"].as_str().unwrap().to_owned()
    }

    fn capacity(&mut self, start: &str, end: &str, minutes: u32) {
        self.execute(
            "capacity.create",
            json!({"capacity": {
                "title": "Capacity", "start": start, "end": end, "capacity_minutes": minutes
            }}),
        )
        .unwrap();
    }

    fn plan_and_commit(&mut self) -> Value {
        let before = self.store.load_world("ws_test").unwrap().unwrap();
        let proposed = self
            .execute(
                "plan.propose",
                json!({
                    "task_ids": [], "horizon_start": "2026-10-25T01:00:00Z",
                    "horizon_end": "2026-10-25T05:00:00Z", "move_unlocked": true
                }),
            )
            .unwrap();
        let after_preview = self.store.load_world("ws_test").unwrap().unwrap();
        assert_eq!(before.allocations, after_preview.allocations);
        let plan = proposed.data["plan"].clone();
        let proposal_id = proposed.data["proposal"]["id"].as_str().unwrap();
        let approved = self
            .execute("proposal.approve", json!({"proposal_id": proposal_id}))
            .unwrap();
        self.execute(
            "changes.commit",
            json!({
                "proposal_id": proposal_id, "approval_grant": approved.data["approval"]["id"]
            }),
        )
        .unwrap();
        let world = self.store.load_world("ws_test").unwrap().unwrap();
        for (index, left) in world.allocations.iter().enumerate() {
            for right in &world.allocations[index + 1..] {
                assert!(left.end <= right.start || right.end <= left.start);
            }
        }
        for block in &world.capacity_blocks {
            let used: u32 = world
                .allocations
                .iter()
                .filter(|a| a.capacity_block_id.as_deref() == Some(block.id.as_str()))
                .map(|a| a.minutes)
                .sum();
            assert!(used <= block.capacity_minutes);
        }
        plan
    }
}

fn placed_minutes(plan: &Value) -> u64 {
    plan["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["minutes"].as_u64().unwrap())
        .sum()
}

#[test]
fn identical_capacity_blocks_do_not_double_book_shared_time() {
    let mut f = Fixture::new();
    f.capacity("2026-10-25T01:00:00Z", "2026-10-25T02:00:00Z", 60);
    f.capacity("2026-10-25T01:00:00Z", "2026-10-25T02:00:00Z", 60);
    f.task("First", 60, 2, None);
    f.task("Second", 60, 1, None);
    let plan = f.plan_and_commit();
    assert_eq!(placed_minutes(&plan), 60);
    assert_eq!(plan["unplaced"].as_array().unwrap().len(), 1);
}

#[test]
fn partially_overlapping_blocks_keep_non_overlapping_tails() {
    let mut f = Fixture::new();
    f.capacity("2026-10-25T01:00:00Z", "2026-10-25T03:00:00Z", 120);
    f.capacity("2026-10-25T02:00:00Z", "2026-10-25T04:00:00Z", 120);
    f.task("First", 90, 2, None);
    f.task("Second", 90, 1, None);
    let plan = f.plan_and_commit();
    assert_eq!(placed_minutes(&plan), 180);
    assert!(plan["unplaced"].as_array().unwrap().is_empty());
}

#[test]
fn reserving_middle_of_another_block_keeps_both_free_fragments() {
    let mut f = Fixture::new();
    f.capacity("2026-10-25T01:00:00Z", "2026-10-25T04:00:00Z", 180);
    f.capacity("2026-10-25T01:30:00Z", "2026-10-25T03:00:00Z", 90);
    f.task("Later priority task", 30, 4, Some("2026-10-25T02:00:00Z"));
    let next = f.task("Flexible work", 90, 1, None);
    let plan = f.plan_and_commit();
    assert_eq!(placed_minutes(&plan), 120);
    let assignments = plan["assignments"].as_array().unwrap();
    assert!(assignments.iter().any(|a| a["task_id"] == next
        && a["start"] == "2026-10-25T01:30:00Z"
        && a["end"] == "2026-10-25T02:00:00Z"));
    assert!(
        assignments
            .iter()
            .filter(|a| a["task_id"] == next)
            .any(|a| a["start"].as_str().unwrap() >= "2026-10-25T02:30:00Z")
    );
    assert!(plan["unplaced"].as_array().unwrap().is_empty());
}

#[test]
fn touching_capacity_blocks_remain_fully_usable() {
    let mut f = Fixture::new();
    f.capacity("2026-10-25T01:00:00Z", "2026-10-25T02:00:00Z", 60);
    f.capacity("2026-10-25T02:00:00Z", "2026-10-25T03:00:00Z", 60);
    f.task("First", 60, 2, None);
    f.task("Second", 60, 1, None);
    let plan = f.plan_and_commit();
    assert_eq!(placed_minutes(&plan), 120);
    assert!(plan["unplaced"].as_array().unwrap().is_empty());
}

#[test]
fn multiple_overlapping_blocks_respect_fixed_commitments_and_budget() {
    let mut f = Fixture::new();
    for _ in 0..3 {
        f.capacity("2026-10-25T01:00:00Z", "2026-10-25T04:00:00Z", 120);
    }
    f.execute(
        "commitment.create",
        json!({"commitment": {
            "title": "Meeting", "start": "2026-10-25T02:00:00Z", "end": "2026-10-25T02:30:00Z"
        }}),
    )
    .unwrap();
    for index in 0..3 {
        f.task(&format!("Work {index}"), 60, 1, None);
    }
    let plan = f.plan_and_commit();
    assert_eq!(placed_minutes(&plan), 120);
    assert_eq!(plan["unplaced"].as_array().unwrap().len(), 1);
    let world = f.store.load_world("ws_test").unwrap().unwrap();
    assert_eq!(world.commitments.len(), 1);
    for allocation in &world.allocations {
        assert!(
            allocation.end <= world.commitments[0].start
                || allocation.start >= world.commitments[0].end
        );
    }
}
