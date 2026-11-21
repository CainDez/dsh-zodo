use chrono::{TimeZone, Utc};
use serde_json::{Value, json};
use zodo_kernel::app::Application;
use zodo_kernel::commands::{CommandEnvelope, CommandResult};
use zodo_kernel::model::{CommandContext, PrincipalKind, Workspace};
use zodo_kernel::ports::{Clock, FixedClock, SaveOptions, SequentialIds, Store};
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

    fn world(&mut self) -> zodo_kernel::model::World {
        self.store.load_world("ws_test").unwrap().unwrap()
    }
    fn task(&mut self, minutes: u32) -> String {
        self.execute(
            "task.create",
            json!({"task": {"title": "Replay task", "estimate_minutes": minutes}}),
        )
        .unwrap()
        .data["changes"][0]["id"]
            .as_str()
            .unwrap()
            .into()
    }
    fn plan(&mut self, task: &str, start: &str, end: &str, replace: bool) -> CommandResult {
        self.execute("plan.propose", json!({"task_ids": [task], "horizon_start": start, "horizon_end": end, "move_unlocked": replace})).unwrap()
    }
    fn approve(&mut self, plan: &CommandResult) -> (String, String) {
        let id = plan.data["proposal"]["id"].as_str().unwrap().to_owned();
        let grant = self
            .execute("proposal.approve", json!({"proposal_id": id}))
            .unwrap();
        (
            id,
            grant.data["approval"]["id"].as_str().unwrap().to_owned(),
        )
    }
    fn commit(&mut self, ids: &(String, String)) -> zodo_kernel::Result<CommandResult> {
        self.execute(
            "changes.commit",
            json!({"proposal_id": ids.0, "approval_grant": ids.1}),
        )
    }
    fn deletion_plan(&mut self) -> (String, CommandResult) {
        let task = self.task(30);
        self.execute("allocation.create", json!({"allocation": {"task_id": task, "start": "2026-10-24T01:21:00Z", "end": "2026-10-24T01:51:00Z", "minutes": 30}})).unwrap();
        // User previews removing the old placement and leaving work unplaced in this horizon.
        let plan = self.plan(&task, "2026-10-24T02:00:00Z", "2026-10-24T03:00:00Z", true);
        assert_eq!(
            plan.data["proposal"]["preview"]["diff"]["removed"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        (task, plan)
    }
    fn assert_rejected_unchanged(&mut self, ids: &(String, String)) {
        let before = self.world();
        let error = self.commit(ids).unwrap_err();
        assert!(error.message.contains("regenerate"), "{error:?}");
        assert!(
            self.store
                .find_receipt("ws_test", "user_ui", &format!("op_{}", self.counter))
                .unwrap()
                .is_none()
        );
        let after = self.world();
        assert_eq!(before.allocations, after.allocations);
        assert_eq!(before.tasks, after.tasks);
        assert_eq!(before.proposals, after.proposals);
        assert_eq!(before.approval_grants, after.approval_grants);
        assert_eq!(before.workspace.revision, after.workspace.revision);
        assert!(
            after
                .approval_grants
                .iter()
                .find(|g| g.id == ids.1)
                .unwrap()
                .consumed_at
                .is_none()
        );
    }
}

#[test]
fn deletion_set_cannot_change_with_clock_only_after_approval() {
    let mut f = Fixture::new();
    let (_, plan) = f.deletion_plan();
    let ids = f.approve(&plan);
    f.clock
        .set(Utc.with_ymd_and_hms(2026, 10, 24, 1, 22, 0).unwrap());
    f.assert_rejected_unchanged(&ids);
}

#[test]
fn approval_rejects_clock_changed_effect_without_issuing_grant() {
    let mut f = Fixture::new();
    let (_, plan) = f.deletion_plan();
    f.clock
        .set(Utc.with_ymd_and_hms(2026, 10, 24, 1, 22, 0).unwrap());
    let before = f.world();
    let error = f
        .execute(
            "proposal.approve",
            json!({"proposal_id": plan.data["proposal"]["id"]}),
        )
        .unwrap_err();
    assert!(error.message.contains("regenerate"), "{error:?}");
    let after = f.world();
    assert_eq!(before.allocations, after.allocations);
    assert_eq!(before.approval_grants, after.approval_grants);
}

#[test]
fn newly_past_plan_assignment_is_not_committed_as_manual_history() {
    for minute in [22, 27] {
        let mut f = Fixture::new();
        let task = f.task(5);
        f.execute("capacity.create", json!({"capacity": {"title": "Soon", "start": "2026-10-24T01:21:00Z", "end": "2026-10-24T01:26:00Z", "capacity_minutes": 5}})).unwrap();
        let plan = f.plan(&task, "2026-10-24T01:21:00Z", "2026-10-24T01:26:00Z", false);
        let ids = f.approve(&plan);
        f.clock
            .set(Utc.with_ymd_and_hms(2026, 10, 24, 1, minute, 0).unwrap());
        f.assert_rejected_unchanged(&ids);
    }
}

#[test]
fn unchanged_effect_commits_before_start_and_exact_boundary_stays_eligible() {
    for second in [30, 60] {
        let mut f = Fixture::new();
        let (_, plan) = f.deletion_plan();
        let ids = f.approve(&plan);
        f.clock
            .set(f.clock.now() + chrono::Duration::seconds(second));
        f.commit(&ids).unwrap();
        assert!(f.world().allocations.is_empty());
    }
}

#[test]
fn legacy_serialized_grant_is_readable_but_cannot_authorize_ambiguous_plan() {
    for legacy_additive in [true, false] {
        let mut f = Fixture::new();
        let (task, initial) = f.deletion_plan();
        let plan = if legacy_additive {
            initial
        } else {
            f.execute("capacity.create", json!({"capacity": {"title": "Replacement", "start": "2026-10-24T02:00:00Z", "end": "2026-10-24T03:00:00Z", "capacity_minutes": 60}})).unwrap();
            f.plan(&task, "2026-10-24T02:00:00Z", "2026-10-24T03:00:00Z", true)
        };
        let ids = f.approve(&plan);
        // Synthetic upgrade fixture: old operation bytes have no deletion binding. Keep a
        // matching old content hash and unexpired grant rather than simulating corruption.
        let mut world = f.world();
        let proposal = world.proposals.iter_mut().find(|p| p.id == ids.0).unwrap();
        proposal.operations[0]["payload"]
            .as_object_mut()
            .unwrap()
            .remove("expected_removed_allocation_ids");
        if legacy_additive {
            proposal.operations[0]["payload"]["assignments"] = json!([]);
        }
        let ops: Vec<zodo_kernel::ops::ChangeOp> =
            serde_json::from_value(proposal.operations.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(&ops).unwrap(),
            proposal.operations,
            "legacy bytes must round-trip without adding fields"
        );
        proposal.content_hash = zodo_kernel::proposal::proposal_content_hash(
            &world.workspace.id,
            &proposal.intent,
            &ops,
            proposal.base_revision,
            proposal.policy_revision,
        );
        world
            .approval_grants
            .iter_mut()
            .find(|g| g.id == ids.1)
            .unwrap()
            .content_hash = proposal.content_hash.clone();
        f.store
            .save(
                &world,
                SaveOptions {
                    expected_revision: world.workspace.revision,
                    bump_revision: false,
                },
                None,
                &[],
                &[],
                &[],
            )
            .unwrap();
        // Reload through SQLite serialization before attempting commit.
        f.store = SqliteStore::open(f._dir.path().join("overlap.sqlite3")).unwrap();
        f.assert_rejected_unchanged(&ids);
        let before = f.world();
        let error = f
            .execute("proposal.approve", json!({"proposal_id": ids.0}))
            .unwrap_err();
        assert!(error.message.contains("regenerate"));
        assert_eq!(f.world().approval_grants, before.approval_grants);
        let fresh = f.plan(&task, "2026-10-24T02:00:00Z", "2026-10-24T03:00:00Z", false);
        let fresh_ids = f.approve(&fresh);
        f.commit(&fresh_ids).unwrap();
        assert_eq!(f.world().allocations, before.allocations);
    }
}

#[test]
fn explicit_manual_history_proposal_remains_usable() {
    let mut f = Fixture::new();
    let task = f.task(30);
    let proposal = f.execute("proposal.create", json!({"intent": "Record manual historical placement", "operations": [{"operation": "allocation.create", "payload": {"allocation": {"task_id": task, "start": "2026-10-23T01:00:00Z", "end": "2026-10-23T01:30:00Z", "minutes": 30}}}]})).unwrap();
    let ids = f.approve(&proposal);
    f.clock.set(f.clock.now() + chrono::Duration::seconds(30));
    f.commit(&ids).unwrap();
    assert_eq!(f.world().allocations.len(), 1);
}
