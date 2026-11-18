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

    fn world(&mut self) -> zodo_kernel::model::World {
        self.store.load_world("ws_test").unwrap().unwrap()
    }

    fn allocation(
        &mut self,
        task: &str,
        start: &str,
        end: &str,
        minutes: u32,
        locked: bool,
    ) -> String {
        let block = self.world().capacity_blocks[0].id.clone();
        self.execute("allocation.create", json!({"allocation": {
            "task_id": task, "capacity_block_id": block, "start": start, "end": end, "minutes": minutes, "locked": locked
        }})).unwrap().data["changes"][0]["id"].as_str().unwrap().to_owned()
    }

    fn propose_and_commit(&mut self, task_ids: Vec<String>, move_unlocked: bool) -> Value {
        let before = self.world();
        let proposal = self.execute("plan.propose", json!({
            "task_ids": task_ids, "horizon_start": "2026-10-25T01:00:00Z", "horizon_end": "2026-10-25T05:00:00Z", "move_unlocked": move_unlocked
        })).unwrap();
        assert_eq!(
            self.world().allocations,
            before.allocations,
            "preview cannot write allocation facts"
        );
        let id = proposal.data["proposal"]["id"].as_str().unwrap();
        let approved = self
            .execute("proposal.approve", json!({"proposal_id": id}))
            .unwrap();
        assert_eq!(
            self.world().allocations,
            before.allocations,
            "approval cannot write allocation facts"
        );
        let committed = self
            .execute(
                "changes.commit",
                json!({"proposal_id": id, "approval_grant": approved.data["approval"]["id"]}),
            )
            .unwrap();
        assert_eq!(self.world().tasks, before.tasks);
        json!({"plan": proposal.data["plan"], "commit": committed.data})
    }
}

#[test]
fn fully_scheduled_retain_plan_commits_no_allocation_changes() {
    let mut f = Fixture::new();
    f.capacity("2026-10-25T01:00:00Z", "2026-10-25T05:00:00Z", 240);
    let task = f.task("Scheduled", 60, 1, None);
    let id = f.allocation(
        &task,
        "2026-10-25T02:00:00Z",
        "2026-10-25T03:00:00Z",
        60,
        false,
    );
    let before = f.world().allocations;
    let result = f.propose_and_commit(vec![task], false);
    assert_eq!(
        f.world().allocations,
        before,
        "retained fully scheduled task must not lose its allocation"
    );
    assert!(result["plan"]["assignments"].as_array().unwrap().is_empty());
    assert!(
        result["plan"]["fixed_allocations"]
            .as_array()
            .unwrap()
            .contains(&json!(id))
    );
    assert!(
        result["plan"]["replace_task_ids"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        result["commit"]["changed"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["kind"] != "allocation")
    );
}

#[test]
fn partially_scheduled_retain_plan_adds_only_missing_work() {
    let mut f = Fixture::new();
    f.capacity("2026-10-25T01:00:00Z", "2026-10-25T05:00:00Z", 240);
    let task = f.task("Partially scheduled", 60, 1, None);
    f.allocation(
        &task,
        "2026-10-25T02:00:00Z",
        "2026-10-25T02:30:00Z",
        30,
        false,
    );
    let retained = f.world().allocations[0].clone();
    let result = f.propose_and_commit(vec![task], false);
    let after = f.world();
    assert!(
        after.allocations.contains(&retained),
        "existing allocation identity and contents must survive"
    );
    assert_eq!(after.allocations.iter().map(|a| a.minutes).sum::<u32>(), 60);
    assert_eq!(after.allocations.len(), 2);
    assert_eq!(result["plan"]["assignments"][0]["minutes"], 30);
}

#[test]
fn retain_plan_with_empty_selection_preserves_every_resolved_task() {
    let mut f = Fixture::new();
    f.capacity("2026-10-25T01:00:00Z", "2026-10-25T05:00:00Z", 240);
    let first = f.task("First", 30, 1, None);
    let second = f.task("Second", 30, 1, None);
    f.allocation(
        &first,
        "2026-10-25T02:00:00Z",
        "2026-10-25T02:30:00Z",
        30,
        false,
    );
    f.allocation(
        &second,
        "2026-10-25T03:00:00Z",
        "2026-10-25T03:30:00Z",
        30,
        false,
    );
    let before = f.world().allocations;
    f.propose_and_commit(vec![], false);
    assert_eq!(f.world().allocations, before);
}

#[test]
fn replace_plan_still_replaces_only_selected_unlocked_allocations() {
    let mut f = Fixture::new();
    f.capacity("2026-10-25T01:00:00Z", "2026-10-25T05:00:00Z", 240);
    let selected = f.task("Selected", 60, 1, None);
    let unrelated = f.task("Unrelated", 30, 1, None);
    let replace = f.allocation(
        &selected,
        "2026-10-25T03:00:00Z",
        "2026-10-25T03:30:00Z",
        30,
        false,
    );
    f.allocation(
        &selected,
        "2026-10-25T04:00:00Z",
        "2026-10-25T04:30:00Z",
        30,
        true,
    );
    f.allocation(
        &unrelated,
        "2026-10-25T02:00:00Z",
        "2026-10-25T02:30:00Z",
        30,
        false,
    );
    let preserve: Vec<_> = f
        .world()
        .allocations
        .into_iter()
        .filter(|a| a.id != replace)
        .collect();
    let result = f.propose_and_commit(vec![selected.clone()], true);
    let after = f.world();
    assert!(!after.allocations.iter().any(|a| a.id == replace));
    assert!(preserve.iter().all(|a| after.allocations.contains(a)));
    assert_eq!(
        after
            .allocations
            .iter()
            .filter(|a| a.task_id == selected)
            .map(|a| a.minutes)
            .sum::<u32>(),
        60
    );
    assert_eq!(result["plan"]["replace_task_ids"], json!([selected]));
}

#[test]
fn retain_plan_preserves_future_allocation_outside_requested_horizon() {
    let mut f = Fixture::new();
    f.capacity("2026-10-26T01:00:00Z", "2026-10-26T03:00:00Z", 120);
    let task = f.task("Tomorrow", 60, 1, None);
    f.allocation(
        &task,
        "2026-10-26T01:00:00Z",
        "2026-10-26T02:00:00Z",
        60,
        false,
    );
    let before = f.world().allocations;
    f.propose_and_commit(vec![task], false);
    assert_eq!(f.world().allocations, before);
}
