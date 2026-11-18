use chrono::{TimeZone, Utc};
use serde_json::{Value, json};
use zodo_kernel::app::Application;
use zodo_kernel::commands::{CommandEnvelope, CommandResult, QueryEnvelope};
use zodo_kernel::model::{CommandContext, PrincipalKind, Workspace};
use zodo_kernel::ports::{Clock, FixedClock, SequentialIds, Store};
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

    fn task(&mut self, minutes: u32) -> String {
        self.execute(
            "task.create",
            json!({"task": {"title": "Work", "estimate_minutes": minutes}}),
        )
        .unwrap()
        .data["changes"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn capacity(&mut self, start: &str, end: &str, minutes: u32) -> String {
        self.execute("capacity.create", json!({"capacity": {"title": "Capacity", "start": start, "end": end, "capacity_minutes": minutes}})).unwrap().data["changes"][0]["id"].as_str().unwrap().to_owned()
    }

    fn allocation(
        &mut self,
        task: &str,
        block: Option<&str>,
        start: &str,
        end: &str,
        minutes: u32,
        locked: bool,
    ) -> String {
        self.execute("allocation.create", json!({"allocation": {
            "task_id": task, "capacity_block_id": block, "start": start, "end": end, "minutes": minutes, "locked": locked
        }})).unwrap().data["changes"][0]["id"].as_str().unwrap().to_owned()
    }

    fn world(&mut self) -> zodo_kernel::model::World {
        self.store.load_world("ws_test").unwrap().unwrap()
    }

    fn schedule(&mut self) -> Value {
        let envelope = QueryEnvelope {
            schema: "zodo.query/1".into(),
            request_id: "query".into(),
            workspace_id: "ws_test".into(),
            query: "schedule.get".into(),
            payload: json!({"start": "2026-10-24T00:00:00Z", "end": "2026-10-24T05:00:00Z"}),
        };
        Application {
            store: &mut self.store,
            clock: &self.clock,
            ids: &self.ids,
            instance_id: "rt_test".into(),
        }
        .query(&self.context, envelope)
        .unwrap()
        .data
    }

    fn plan_and_commit(&mut self, task: &str) -> Value {
        let before = self.world();
        let now = self.clock.now();
        let proposed = self.execute("plan.propose", json!({
            "task_ids": [task], "horizon_start": "2026-10-24T02:00:00Z", "horizon_end": "2026-10-24T04:00:00Z", "move_unlocked": true
        })).unwrap();
        assert_eq!(
            self.world().allocations,
            before.allocations,
            "preview must not mutate history"
        );
        let proposal_id = proposed.data["proposal"]["id"].as_str().unwrap();
        let approved = self
            .execute("proposal.approve", json!({"proposal_id": proposal_id}))
            .unwrap();
        self.execute(
            "changes.commit",
            json!({"proposal_id": proposal_id, "approval_grant": approved.data["approval"]["id"]}),
        )
        .unwrap();
        assert_eq!(
            self.clock.now(),
            now,
            "repro uses the same clock throughout"
        );
        let after = self.world();
        assert_eq!(before.activities, after.activities);
        assert_eq!(
            before.tasks, after.tasks,
            "planning must not invent progress"
        );
        for historical in before
            .allocations
            .iter()
            .filter(|a| a.end <= now || a.locked)
        {
            assert!(
                after.allocations.contains(historical),
                "history and locked rows stay unchanged"
            );
        }
        proposed.data["plan"].clone()
    }
}

fn placed(plan: &Value) -> u64 {
    plan["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["minutes"].as_u64().unwrap())
        .sum()
}

#[test]
fn missed_elapsed_schedule_does_not_consume_remaining_work() {
    let mut f = Fixture::new();
    let task = f.task(60);
    f.allocation(
        &task,
        None,
        "2026-10-24T00:00:00Z",
        "2026-10-24T01:00:00Z",
        60,
        false,
    );
    f.capacity("2026-10-24T02:00:00Z", "2026-10-24T04:00:00Z", 120);
    let plan = f.plan_and_commit(&task);
    assert_eq!(placed(&plan), 60);
    let world = f.world();
    assert_eq!(world.task(&task).unwrap().actual_minutes, 0);
    assert_eq!(world.task(&task).unwrap().remaining_minutes, Some(60));
    assert_eq!(world.allocations.len(), 2);
}

#[test]
fn recorded_progress_and_elapsed_allocation_allow_remaining_replan() {
    let mut f = Fixture::new();
    let task = f.task(60);
    f.allocation(
        &task,
        None,
        "2026-10-24T00:30:00Z",
        "2026-10-24T01:00:00Z",
        30,
        true,
    );
    f.execute("progress.record", json!({"task_id": task, "minutes": 30}))
        .unwrap();
    f.capacity("2026-10-24T02:00:00Z", "2026-10-24T04:00:00Z", 120);
    assert_eq!(placed(&f.plan_and_commit(&task)), 30);
    let world = f.world();
    assert_eq!(world.task(&task).unwrap().actual_minutes, 30);
    assert_eq!(world.task(&task).unwrap().remaining_minutes, Some(30));
    assert_eq!(world.activities.len(), 1);
}

#[test]
fn allocation_ending_exactly_now_is_history_and_unplaced_matches_planner() {
    let mut f = Fixture::new(); // Asia/Shanghai workspace, UTC clock instant.
    let task = f.task(30);
    f.allocation(
        &task,
        None,
        "2026-10-24T00:50:00Z",
        "2026-10-24T01:20:00Z",
        30,
        true,
    );
    f.capacity("2026-10-24T02:00:00Z", "2026-10-24T04:00:00Z", 120);
    assert_eq!(f.schedule()["unplaced"][0]["unplaced_minutes"], 30);
    assert_eq!(placed(&f.plan_and_commit(&task)), 30);
    assert!(f.schedule()["unplaced"].as_array().unwrap().is_empty());
}

#[test]
fn straddling_locked_allocation_reserves_its_full_minutes() {
    let mut f = Fixture::new();
    let task = f.task(60);
    f.allocation(
        &task,
        None,
        "2026-10-24T01:00:00Z",
        "2026-10-24T01:40:00Z",
        40,
        true,
    );
    f.capacity("2026-10-24T02:00:00Z", "2026-10-24T04:00:00Z", 120);
    assert_eq!(f.schedule()["unplaced"][0]["unplaced_minutes"], 20);
    assert_eq!(placed(&f.plan_and_commit(&task)), 20);
}

#[test]
fn elapsed_allocations_still_consume_their_capacity_block_budget() {
    let mut f = Fixture::new();
    let block = f.capacity("2026-10-24T00:00:00Z", "2026-10-24T04:00:00Z", 60);
    let old = f.task(30);
    f.allocation(
        &old,
        Some(&block),
        "2026-10-24T00:30:00Z",
        "2026-10-24T01:00:00Z",
        30,
        true,
    );
    let next = f.task(60);
    let plan = f.plan_and_commit(&next);
    assert_eq!(placed(&plan), 30);
    assert_eq!(plan["unplaced"][0]["minutes"], 30);
    assert_eq!(
        f.world().allocations.iter().map(|a| a.minutes).sum::<u32>(),
        60
    );
}

#[test]
fn explicit_historical_write_keeps_existing_all_time_work_bound() {
    let mut f = Fixture::new();
    let task = f.task(60);
    f.allocation(
        &task,
        None,
        "2026-10-24T00:00:00Z",
        "2026-10-24T00:40:00Z",
        40,
        false,
    );
    f.execute("progress.record", json!({"task_id": task, "minutes": 30}))
        .unwrap();
    let before = f.world().allocations;
    let result = f.execute("allocation.create", json!({"allocation": {
        "task_id": task, "start": "2026-10-24T00:50:00Z", "end": "2026-10-24T01:00:00Z", "minutes": 10
    }}));
    assert!(result.is_err());
    assert_eq!(f.world().allocations, before);
}

#[test]
fn future_allocation_update_excludes_itself_and_elapsed_history() {
    let mut f = Fixture::new();
    let task = f.task(60);
    let past = f.allocation(
        &task,
        None,
        "2026-10-24T00:30:00Z",
        "2026-10-24T01:00:00Z",
        30,
        false,
    );
    let future = f.allocation(
        &task,
        None,
        "2026-10-24T02:00:00Z",
        "2026-10-24T02:30:00Z",
        30,
        false,
    );
    f.execute("progress.record", json!({"task_id": task, "minutes": 30}))
        .unwrap();
    let before = f.world();
    f.execute("allocation.update", json!({"id": future, "allocation": {
        "task_id": task, "start": "2026-10-24T03:00:00Z", "end": "2026-10-24T03:30:00Z", "minutes": 30
    }})).unwrap();
    let after = f.world();
    assert_eq!(
        before.allocations.iter().find(|a| a.id == past),
        after.allocations.iter().find(|a| a.id == past)
    );
    assert_eq!(before.tasks, after.tasks);
}

#[test]
fn review_unplaced_uses_current_reservations_without_inventing_actual_effort() {
    let mut f = Fixture::new();
    let task = f.task(30);
    f.allocation(
        &task,
        None,
        "2026-10-24T00:50:00Z",
        "2026-10-24T01:20:00Z",
        30,
        false,
    );
    let review = f
        .execute(
            "review.build",
            json!({
                "period_start": "2026-10-24", "period_end": "2026-10-24", "notes": null
            }),
        )
        .unwrap();
    assert_eq!(review.data["review"]["facts"]["unplaced_minutes"], 30);
    assert_eq!(review.data["review"]["facts"]["actual_minutes"], 0);
    assert!(f.world().activities.is_empty());
}

#[test]
fn commit_rechecks_reservations_at_its_clock_without_rewriting_approved_plan() {
    let mut f = Fixture::new();
    let task = f.task(60);
    f.allocation(
        &task,
        None,
        "2026-10-24T00:50:10Z",
        "2026-10-24T01:20:10Z",
        30,
        true,
    );
    f.capacity("2026-10-24T02:00:00Z", "2026-10-24T04:00:00Z", 120);
    let before = f.world();
    let proposal = f.execute("plan.propose", json!({
        "task_ids": [task], "horizon_start": "2026-10-24T02:00:00Z", "horizon_end": "2026-10-24T04:00:00Z", "move_unlocked": true
    })).unwrap();
    assert_eq!(placed(&proposal.data["plan"]), 30);
    let id = proposal.data["proposal"]["id"].as_str().unwrap();
    let approval = f
        .execute("proposal.approve", json!({"proposal_id": id}))
        .unwrap();
    // The existing allocation becomes historical without changing the workspace revision.
    // Neither its expiration nor commit is allowed to fabricate completion or extra work.
    f.clock
        .set(Utc.with_ymd_and_hms(2026, 10, 24, 1, 20, 10).unwrap());
    f.execute(
        "changes.commit",
        json!({"proposal_id": id, "approval_grant": approval.data["approval"]["id"]}),
    )
    .unwrap();
    let after = f.world();
    assert_eq!(before.tasks, after.tasks);
    assert_eq!(before.activities, after.activities);
    assert!(after.allocations.contains(&before.allocations[0]));
    assert_eq!(after.allocations.len(), 2);
    assert_eq!(f.schedule()["unplaced"][0]["unplaced_minutes"], 30);
}
