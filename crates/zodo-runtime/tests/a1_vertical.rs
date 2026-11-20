use chrono::{Duration, TimeZone, Utc};
use serde_json::{Value, json};
use zodo_kernel::app::Application;
use zodo_kernel::commands::{CommandEnvelope, CommandResult, ResultStatus};
use zodo_kernel::error::ErrorCode;
use zodo_kernel::model::{CommandContext, PrincipalKind, Workspace};
use zodo_kernel::ports::{Clock, FixedClock, SequentialIds, Store};
use zodo_runtime::SqliteStore;

struct Fixture {
    store: SqliteStore,
    clock: FixedClock,
    ids: SequentialIds,
    context: CommandContext,
    path: std::path::PathBuf,
    counter: u64,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap().keep();
        let path = dir.join("zodo-test.sqlite3");
        let mut store = SqliteStore::open(&path).unwrap();
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
                capabilities: ["read", "write", "propose", "approve", "execute", "admin"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                workspace_id: "ws_test".to_string(),
                request_id: "req_test".to_string(),
                received_at: now,
                runtime_instance: "rt_test".to_string(),
            },
            path,
            counter: 0,
        }
    }

    fn execute(&mut self, operation: &str, payload: Value) -> zodo_kernel::Result<CommandResult> {
        let key = format!("op_{}", self.counter + 1);
        self.execute_with(operation, payload, None, &key)
    }

    fn execute_with(
        &mut self,
        operation: &str,
        payload: Value,
        base_revision: Option<u64>,
        key: &str,
    ) -> zodo_kernel::Result<CommandResult> {
        self.counter += 1;
        let envelope = CommandEnvelope {
            schema: "zodo.command/1".to_string(),
            request_id: format!("req_{}", self.counter),
            idempotency_key: key.to_string(),
            workspace_id: "ws_test".to_string(),
            operation: operation.to_string(),
            operation_version: 1,
            base_revision,
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

    fn advance(&mut self, minutes: i64) {
        let now = self.clock.now() + Duration::minutes(minutes);
        self.clock.set(now);
    }
}

fn create_task(fixture: &mut Fixture, title: &str, estimate: u32) -> String {
    let result = fixture
        .execute(
            "task.create",
            json!({
                "task": {
                    "title": title,
                    "estimate_minutes": estimate,
                    "priority": 2,
                }
            }),
        )
        .unwrap();
    result.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn create_capacity(fixture: &mut Fixture, start: &str, end: &str, minutes: u32) -> String {
    let result = fixture
        .execute(
            "capacity.create",
            json!({
                "capacity": {
                    "title": "Focus",
                    "start": start,
                    "end": end,
                    "capacity_minutes": minutes,
                    "locked": false,
                }
            }),
        )
        .unwrap();
    result.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn plan_and_commit(fixture: &mut Fixture, task_id: &str) -> (String, String, String) {
    let planned = fixture
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id],
                "horizon_start": "2026-10-25T00:00:00Z",
                "horizon_end": "2026-11-01T00:00:00Z",
                "move_unlocked": true,
            }),
        )
        .unwrap();
    assert_eq!(planned.status, ResultStatus::Validated);
    let proposal = planned.data["proposal"]["id"].as_str().unwrap().to_string();
    let approved = fixture
        .execute("proposal.approve", json!({"proposal_id": proposal}))
        .unwrap();
    assert_eq!(approved.status, ResultStatus::Approved);
    let grant = approved.data["approval"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let committed = fixture
        .execute(
            "changes.commit",
            json!({"proposal_id": proposal, "approval_grant": grant}),
        )
        .unwrap();
    assert_eq!(committed.status, ResultStatus::Committed);
    (
        proposal,
        grant,
        committed.data["change_set_id"]
            .as_str()
            .unwrap()
            .to_string(),
    )
}

#[test]
fn capture_plan_commit_execute_and_review_without_host() {
    let mut fixture = Fixture::new();
    let captured = fixture
        .execute_with(
            "capture.collect",
            json!({"text": "Organize the portfolio before month end"}),
            None,
            "vertical_capture",
        )
        .unwrap();
    assert_eq!(captured.status, ResultStatus::Committed);
    let inbox_id = captured.data["inbox_entry"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let task_id = create_task(&mut fixture, "Select portfolio material", 45);
    create_capacity(
        &mut fixture,
        "2026-10-26T01:00:00Z",
        "2026-10-26T04:00:00Z",
        180,
    );
    let (proposal, _grant, change_set) = plan_and_commit(&mut fixture, &task_id);
    assert!(!proposal.is_empty());
    assert!(!change_set.is_empty());

    let world = fixture.world();
    let task = world.task(&task_id).unwrap();
    assert_eq!(task.remaining_minutes, Some(45));
    assert_eq!(world.allocations.len(), 1);
    assert_eq!(world.allocations[0].minutes, 45);

    fixture
        .execute(
            "progress.record",
            json!({"task_id": task_id, "minutes": 20, "note": "First block"}),
        )
        .unwrap();
    let world = fixture.world();
    assert_eq!(world.task(&task_id).unwrap().actual_minutes, 20);
    assert_eq!(world.task(&task_id).unwrap().remaining_minutes, Some(25));

    fixture
        .execute("focus.start", json!({"task_id": task_id}))
        .unwrap();
    fixture.advance(25);
    fixture.execute("focus.finish", json!({})).unwrap();
    let world = fixture.world();
    let task = world.task(&task_id).unwrap();
    assert_eq!(task.actual_minutes, 45);
    assert_eq!(task.remaining_minutes, Some(0));
    assert_eq!(task.status, zodo_kernel::model::TaskStatus::InProgress);
    assert_eq!(world.focus_sessions.len(), 1);
    assert_eq!(
        world.focus_sessions[0].state,
        zodo_kernel::model::FocusState::Completed
    );

    let review = fixture
        .execute(
            "review.build",
            json!({"period_start": "2026-10-24", "period_end": "2026-10-24"}),
        )
        .unwrap();
    let review_id = review.data["review"]["id"].as_str().unwrap().to_string();
    let facts = &review.data["review"]["facts"];
    assert_eq!(facts["focus_minutes"], 25);
    assert_eq!(facts["actual_minutes"], 45);

    let receipt = fixture
        .store
        .find_receipt("ws_test", "user_ui", "vertical_capture")
        .unwrap();
    assert!(receipt.is_some());

    drop(fixture.store);
    let mut reopened = SqliteStore::open(&fixture.path).unwrap();
    let world = reopened.load_world("ws_test").unwrap().unwrap();
    assert_eq!(world.workspace.revision, 8);
    assert!(world.task(&task_id).is_some());
    assert_eq!(world.reviews.len(), 1);
    assert_eq!(world.reviews[0].id, review_id);
    assert!(world.inbox.iter().any(|entry| entry.id == inbox_id));
}

#[test]
fn idempotency_replays_and_rejects_different_content() {
    let mut fixture = Fixture::new();
    let payload = json!({"task": {"title": "Idempotent task", "estimate_minutes": 30}});
    let first = fixture
        .execute_with("task.create", payload.clone(), None, "key_same")
        .unwrap();
    assert!(!first.replayed);
    assert_eq!(first.revision, 1);
    let replay = fixture
        .execute_with("task.create", payload, None, "key_same")
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.revision, 1);
    assert_eq!(fixture.world().tasks.len(), 1);

    let error = fixture
        .execute_with(
            "task.create",
            json!({"task": {"title": "Different", "estimate_minutes": 30}}),
            None,
            "key_same",
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::IdempotencyMismatch);
    assert_eq!(fixture.world().tasks.len(), 1);
}

#[test]
fn stale_revision_and_stale_proposal_are_refused_without_writes() {
    let mut fixture = Fixture::new();
    let task_id = create_task(&mut fixture, "Needs replanning", 45);
    create_capacity(
        &mut fixture,
        "2026-10-26T01:00:00Z",
        "2026-10-26T04:00:00Z",
        90,
    );
    let proposal = fixture
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id],
                "horizon_start": "2026-10-25T00:00:00Z",
                "horizon_end": "2026-11-01T00:00:00Z",
                "move_unlocked": true,
            }),
        )
        .unwrap();
    let proposal_id = proposal.data["proposal"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    fixture
        .execute(
            "task.create",
            json!({"task": {"title": "Concurrent task", "estimate_minutes": 15}}),
        )
        .unwrap();

    let error = fixture
        .execute("proposal.approve", json!({"proposal_id": proposal_id}))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RevisionConflict);
    assert!(fixture.world().allocations.is_empty());
}

#[test]
fn dependency_cycles_are_refused_and_graph_is_preserved() {
    let mut fixture = Fixture::new();
    let a = create_task(&mut fixture, "A", 20);
    let b = create_task(&mut fixture, "B", 20);
    fixture
        .execute(
            "dependency.add",
            json!({"predecessor_id": a, "successor_id": b}),
        )
        .unwrap();
    let error = fixture
        .execute(
            "dependency.add",
            json!({"predecessor_id": b, "successor_id": a}),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ValidationFailed);
    let world = fixture.world();
    assert_eq!(world.dependencies.len(), 1);
    assert_eq!(world.dependencies[0].predecessor_id, a);
}

#[test]
fn agent_cannot_approve_or_commit() {
    let mut fixture = Fixture::new();
    let task_id = create_task(&mut fixture, "Agent task", 30);
    create_capacity(
        &mut fixture,
        "2026-10-26T01:00:00Z",
        "2026-10-26T04:00:00Z",
        60,
    );
    let proposal = fixture
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id],
                "horizon_start": "2026-10-25T00:00:00Z",
                "horizon_end": "2026-11-01T00:00:00Z",
                "move_unlocked": true,
            }),
        )
        .unwrap();
    let proposal_id = proposal.data["proposal"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    fixture.context.principal_id = "agent_harness".to_string();
    fixture.context.principal_kind = PrincipalKind::DelegatedAgent;
    fixture.context.capabilities = vec![
        "read".to_string(),
        "propose".to_string(),
        "execute".to_string(),
    ];
    let error = fixture
        .execute("proposal.approve", json!({"proposal_id": proposal_id}))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert!(fixture.world().allocations.is_empty());
}

#[test]
fn infeasible_plan_reports_gap_and_keeps_task_record() {
    let mut fixture = Fixture::new();
    let task_id = create_task(&mut fixture, "Large task", 90);
    create_capacity(
        &mut fixture,
        "2026-10-26T01:00:00Z",
        "2026-10-26T02:00:00Z",
        60,
    );
    let planned = fixture
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id],
                "horizon_start": "2026-10-25T00:00:00Z",
                "horizon_end": "2026-11-01T00:00:00Z",
                "move_unlocked": true,
            }),
        )
        .unwrap();
    assert_eq!(planned.data["plan"]["assignments"][0]["minutes"], 60);
    assert_eq!(planned.data["plan"]["unplaced"][0]["minutes"], 30);
    assert_eq!(
        planned.data["plan"]["unplaced"][0]["reason"],
        "insufficient_capacity"
    );
    let world = fixture.world();
    assert_eq!(world.task(&task_id).unwrap().estimate_minutes, Some(90));
    assert!(world.allocations.is_empty());
}

#[test]
fn locked_allocation_is_not_moved_and_capacity_is_not_double_charged() {
    let mut fixture = Fixture::new();
    let task_id = create_task(&mut fixture, "Fixed work", 30);
    create_capacity(
        &mut fixture,
        "2026-10-26T01:00:00Z",
        "2026-10-26T03:00:00Z",
        120,
    );
    let allocation = fixture
        .execute(
            "allocation.create",
            json!({"allocation": {
                "task_id": task_id,
                "capacity_block_id": null,
                "start": "2026-10-26T01:00:00Z",
                "end": "2026-10-26T01:30:00Z",
                "minutes": 30,
                "locked": true,
            }}),
        )
        .unwrap();
    let allocation_id = allocation.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let planned = fixture
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id],
                "horizon_start": "2026-10-25T00:00:00Z",
                "horizon_end": "2026-11-01T00:00:00Z",
                "move_unlocked": true,
            }),
        )
        .unwrap();
    assert!(
        planned.data["plan"]["assignments"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let world = fixture.world();
    assert!(
        world
            .allocations
            .iter()
            .any(|item| item.id == allocation_id)
    );
}

#[test]
fn expired_approval_is_not_reusable() {
    let mut fixture = Fixture::new();
    let task_id = create_task(&mut fixture, "Expired approval", 30);
    create_capacity(
        &mut fixture,
        "2026-10-26T01:00:00Z",
        "2026-10-26T02:00:00Z",
        60,
    );
    let planned = fixture
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id],
                "horizon_start": "2026-10-25T00:00:00Z",
                "horizon_end": "2026-11-01T00:00:00Z",
                "move_unlocked": true,
            }),
        )
        .unwrap();
    let proposal = planned.data["proposal"]["id"].as_str().unwrap().to_string();
    let approved = fixture
        .execute("proposal.approve", json!({"proposal_id": proposal}))
        .unwrap();
    let grant = approved.data["approval"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    fixture.advance(16 * 60);
    let error = fixture
        .execute(
            "changes.commit",
            json!({"proposal_id": proposal, "approval_grant": grant}),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ApprovalExpired);
    assert!(fixture.world().allocations.is_empty());
}

#[test]
fn rejected_proposal_cannot_commit_with_its_previously_valid_grant() {
    let mut fixture = Fixture::new();
    let task_id = create_task(&mut fixture, "Rejected approval", 30);
    create_capacity(
        &mut fixture,
        "2026-10-26T01:00:00Z",
        "2026-10-26T02:00:00Z",
        60,
    );
    let before = fixture.world();
    let planned = fixture
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id],
                "horizon_start": "2026-10-26T00:00:00Z",
                "horizon_end": "2026-10-27T00:00:00Z",
                "move_unlocked": false,
            }),
        )
        .unwrap();
    assert!(
        !planned.data["plan"]["assignments"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(planned.data["proposal"]["status"], "validated");
    let proposal = planned.data["proposal"]["id"].as_str().unwrap();
    let approved = fixture
        .execute("proposal.approve", json!({"proposal_id": proposal}))
        .unwrap();
    let grant = approved.data["approval"]["id"].as_str().unwrap();
    let rejected = fixture
        .execute(
            "proposal.reject",
            json!({"proposal_id": proposal, "reason": "No longer needed"}),
        )
        .unwrap();
    assert_eq!(rejected.status, ResultStatus::Rejected);
    assert_eq!(rejected.data["proposal"]["status"], "rejected");

    let after_reject = fixture.world();
    assert_eq!(after_reject.workspace.revision, before.workspace.revision);
    assert_eq!(after_reject.allocations, before.allocations);
    assert_eq!(after_reject.tasks, before.tasks);
    assert_eq!(after_reject.capacity_blocks, before.capacity_blocks);
    let saved_grant = after_reject
        .approval_grants
        .iter()
        .find(|item| item.id == grant)
        .unwrap();
    assert_eq!(saved_grant.proposal_id, proposal);
    assert!(saved_grant.consumed_at.is_none());

    let error = fixture
        .execute(
            "changes.commit",
            json!({"proposal_id": proposal, "approval_grant": grant}),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ApprovalRequired);
    assert_eq!(
        serde_json::to_value(fixture.world()).unwrap(),
        serde_json::to_value(after_reject).unwrap()
    );
}

#[test]
fn reminder_outbox_is_durable_and_not_duplicated() {
    let mut fixture = Fixture::new();
    let fire_at = fixture.clock.now() + Duration::minutes(1);
    let payload = json!({"message": "Contact printer", "fire_at": fire_at, "dedupe_key": "rem-1"});
    let result = fixture
        .execute_with("reminder.create", payload.clone(), None, "reminder_key")
        .unwrap();
    assert_eq!(result.status, ResultStatus::Committed);
    let replay = fixture
        .execute_with("reminder.create", payload, None, "reminder_key")
        .unwrap();
    assert!(replay.replayed);
    let effects = fixture
        .store
        .pending_effects(fire_at + Duration::seconds(1), 10)
        .unwrap();
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].effect_type, "reminder.deliver");
}

#[test]
fn recurrence_expands_idempotently_and_single_exceptions_do_not_duplicate() {
    let mut fixture = Fixture::new();
    let created = fixture
        .execute(
            "recurrence.create",
            json!({
                "rule": {
                    "title": "Weekly review",
                    "timezone": "Asia/Shanghai",
                    "start_date": "2026-10-26",
                    "frequency": "weekly",
                    "interval": 1,
                    "weekdays": [1, 3],
                    "estimate_minutes": 30
                },
                "through": "2026-11-02"
            }),
        )
        .unwrap();
    assert_eq!(created.status, ResultStatus::Committed);
    let rule_id = created.data["rule"]["id"].as_str().unwrap().to_string();
    assert_eq!(created.data["task_ids"].as_array().unwrap().len(), 3);

    let expanded = fixture
        .execute(
            "recurrence.expand",
            json!({"rule_id": rule_id, "through": "2026-11-09"}),
        )
        .unwrap();
    assert_eq!(expanded.data["task_ids"].as_array().unwrap().len(), 2);
    let expanded_again = fixture
        .execute(
            "recurrence.expand",
            json!({"rule_id": rule_id, "through": "2026-11-09"}),
        )
        .unwrap();
    assert!(
        expanded_again.data["task_ids"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let world = fixture.world();
    assert_eq!(world.recurrence_rules.len(), 1);
    assert_eq!(world.occurrences.len(), 5);
    let first = world
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.rule_id == rule_id)
        .min_by_key(|occurrence| occurrence.current_date)
        .unwrap()
        .clone();
    let first_task = first.task_id.clone().unwrap();

    fixture
        .execute(
            "recurrence.exception",
            json!({"occurrence_id": first.id, "action": "skip"}),
        )
        .unwrap();
    let world = fixture.world();
    assert_eq!(
        world.task(&first_task).unwrap().status,
        zodo_kernel::model::TaskStatus::Cancelled
    );
    assert_eq!(
        world
            .occurrences
            .iter()
            .find(|item| item.id == first.id)
            .unwrap()
            .status,
        zodo_kernel::model::OccurrenceStatus::Skipped
    );

    let second = world
        .occurrences
        .iter()
        .filter(|occurrence| {
            occurrence.rule_id == rule_id
                && occurrence.status == zodo_kernel::model::OccurrenceStatus::Generated
        })
        .min_by_key(|occurrence| occurrence.current_date)
        .unwrap()
        .clone();
    fixture
        .execute(
            "recurrence.exception",
            json!({
                "occurrence_id": second.id,
                "action": "move",
                "new_date": "2026-11-01"
            }),
        )
        .unwrap();
    let world = fixture.world();
    assert_eq!(world.occurrences.len(), 5);
    assert_eq!(
        world
            .occurrences
            .iter()
            .find(|item| item.id == second.id)
            .unwrap()
            .current_date,
        chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()
    );
    let moved_task = world.task(&second.task_id.unwrap()).unwrap();
    assert_eq!(
        moved_task.target_due,
        Some(chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap())
    );
}

#[test]
fn monthly_recurrence_skips_missing_month_end_dates() {
    let mut fixture = Fixture::new();
    let created = fixture
        .execute(
            "recurrence.create",
            json!({
                "rule": {
                    "title": "Month end close",
                    "timezone": "Asia/Shanghai",
                    "start_date": "2026-01-31",
                    "frequency": "monthly",
                    "interval": 1,
                    "estimate_minutes": 60
                },
                "through": "2026-04-30"
            }),
        )
        .unwrap();
    assert_eq!(created.data["task_ids"].as_array().unwrap().len(), 2);
    let world = fixture.world();
    let dates: Vec<_> = world
        .occurrences
        .iter()
        .map(|occurrence| occurrence.original_date.to_string())
        .collect();
    assert_eq!(dates, vec!["2026-01-31", "2026-03-31"]);
}
