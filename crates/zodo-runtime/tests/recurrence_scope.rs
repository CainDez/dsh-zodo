//! UI-L06 / REC-04: `recurrence.update` applies a series edit to `future` or `series` scope.
//!
//! Model: a weekly series expands into generated occurrences/tasks. Editing the series must
//! rewrite the rule and the still-generated future instances while leaving past, completed and
//! locked instances, occurrence dates/keys and every unrelated field untouched.

use chrono::{NaiveDate, TimeZone, Utc};
use serde_json::{Value, json};
use zodo_kernel::app::Application;
use zodo_kernel::commands::{CommandEnvelope, CommandResult};
use zodo_kernel::error::ErrorCode;
use zodo_kernel::model::{
    CommandContext, OccurrenceStatus, PrincipalKind, TaskStatus, Workspace, World,
};
use zodo_kernel::ports::{EventRecord, FixedClock, SequentialIds, Store};
use zodo_runtime::SqliteStore;

struct Fixture {
    store: SqliteStore,
    clock: FixedClock,
    ids: SequentialIds,
    context: CommandContext,
    counter: u64,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap().keep();
        let mut store = SqliteStore::open(dir.join("recurrence_scope.sqlite3")).unwrap();
        // 2026-10-24T01:20Z is 2026-10-24 09:20 in the default Asia/Shanghai workspace.
        let now = Utc.with_ymd_and_hms(2026, 10, 24, 1, 20, 0).unwrap();
        let world = World::new(Workspace::new(
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
        }
    }

    fn execute(&mut self, operation: &str, payload: Value) -> zodo_kernel::Result<CommandResult> {
        self.counter += 1;
        let key = format!("op_{}", self.counter);
        self.execute_keyed(operation, payload, &key)
    }

    fn execute_keyed(
        &mut self,
        operation: &str,
        payload: Value,
        idempotency_key: &str,
    ) -> zodo_kernel::Result<CommandResult> {
        let envelope = CommandEnvelope {
            schema: "zodo.command/1".to_string(),
            request_id: format!("req_{idempotency_key}"),
            idempotency_key: idempotency_key.to_string(),
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

    fn execute_as(
        &mut self,
        context: &CommandContext,
        operation: &str,
        payload: Value,
    ) -> zodo_kernel::Result<CommandResult> {
        let envelope = CommandEnvelope {
            schema: "zodo.command/1".to_string(),
            request_id: "req_authz".to_string(),
            idempotency_key: "op_authz".to_string(),
            workspace_id: context.workspace_id.clone(),
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
        app.execute(context, envelope)
    }

    fn world(&mut self) -> World {
        self.store.load_world("ws_test").unwrap().unwrap()
    }

    fn events(&mut self) -> Vec<EventRecord> {
        self.store.list_events("ws_test", 0, 500).unwrap()
    }
}

/// Create the fixture series: weekly on Monday/Wednesday from 2026-10-19.
///
/// Occurrences: 10-19 (past, Monday), 10-21 (past, Wednesday), 10-26, 10-28, 11-02, 11-04, 11-09.
/// "Today" in the workspace timezone is 2026-10-24 (Saturday), so five occurrences are future.
fn create_weekly_series(f: &mut Fixture) -> String {
    let created = f
        .execute(
            "recurrence.create",
            json!({
                "rule": {
                    "title": "每周复盘",
                    "timezone": "Asia/Shanghai",
                    "start_date": "2026-10-19",
                    "frequency": "weekly",
                    "interval": 1,
                    "weekdays": [1, 3],
                    "estimate_minutes": 30,
                    "priority": 2
                },
                "through": "2026-11-09"
            }),
        )
        .unwrap();
    assert_eq!(created.data["task_ids"].as_array().unwrap().len(), 7);
    created.data["rule"]["id"].as_str().unwrap().to_string()
}

fn task_on(f: &mut Fixture, rule_id: &str, date: &str) -> String {
    let target = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
    let world = f.world();
    world
        .occurrences
        .iter()
        .find(|occurrence| occurrence.rule_id == rule_id && occurrence.original_date == target)
        .and_then(|occurrence| occurrence.task_id.clone())
        .unwrap_or_else(|| panic!("no occurrence for {date}"))
}

fn sorted_ids(value: &Value) -> Vec<String> {
    let mut ids: Vec<String> = value
        .as_array()
        .expect("array")
        .iter()
        .map(|item| item.as_str().expect("string id").to_string())
        .collect();
    ids.sort();
    ids
}

#[test]
fn future_scope_rewrites_generated_instances_and_preserves_history() {
    let mut f = Fixture::new();
    let rule_id = create_weekly_series(&mut f);

    let past_open = task_on(&mut f, &rule_id, "2026-10-21");
    let past_done = task_on(&mut f, &rule_id, "2026-10-19");
    let future_open = task_on(&mut f, &rule_id, "2026-10-26");
    let future_done = task_on(&mut f, &rule_id, "2026-10-28");
    let future_locked = task_on(&mut f, &rule_id, "2026-11-02");
    let future_custom = task_on(&mut f, &rule_id, "2026-11-04");
    let future_last = task_on(&mut f, &rule_id, "2026-11-09");

    // Fixed facts: a completed past instance, a completed future instance and a locked allocation.
    f.execute("task.complete", json!({"id": past_done}))
        .unwrap();
    f.execute("task.complete", json!({"id": future_done}))
        .unwrap();
    f.execute(
        "allocation.create",
        json!({"allocation": {
            "task_id": future_locked,
            "start": "2026-11-02T01:00:00Z",
            "end": "2026-11-02T01:30:00Z",
            "minutes": 30,
            "locked": true
        }}),
    )
    .unwrap();
    // A manually customised future instance must keep the fields the series edit does not own.
    f.execute(
        "task.update",
        json!({
            "id": future_custom,
            "task": {
                "title": "每周复盘",
                "description": "补充说明",
                "estimate_minutes": 30,
                "priority": 2,
                "target_due": "2026-11-04",
                "tags": ["复盘"]
            }
        }),
    )
    .unwrap();

    let revision_before = f.world().workspace.revision;
    let updated = f
        .execute(
            "recurrence.update",
            json!({
                "rule_id": rule_id,
                "scope": "future",
                "title": "深度复盘",
                "estimate_minutes": 45,
                "priority": 3,
                "time_of_day": "07:30"
            }),
        )
        .unwrap();
    assert_eq!(
        updated.status,
        zodo_kernel::commands::ResultStatus::Committed
    );
    assert_eq!(updated.revision, revision_before + 1);
    assert_eq!(updated.data["scope"], "future");
    assert_eq!(updated.data["today"], "2026-10-24");

    let expected_updated = {
        let mut ids = vec![
            future_open.clone(),
            future_custom.clone(),
            future_last.clone(),
        ];
        ids.sort();
        ids
    };
    assert_eq!(
        sorted_ids(&updated.data["updated_task_ids"]),
        expected_updated
    );
    assert_eq!(
        sorted_ids(&updated.data["skipped_locked_task_ids"]),
        vec![future_locked.clone()]
    );

    let world = f.world();
    assert_eq!(world.workspace.revision, revision_before + 1);

    // Rule: editable fields rewritten, identity/dates preserved, version bumped.
    let rule = world
        .recurrence_rules
        .iter()
        .find(|rule| rule.id == rule_id)
        .unwrap();
    assert_eq!(rule.title, "深度复盘");
    assert_eq!(rule.estimate_minutes, Some(45));
    assert_eq!(rule.priority, 3);
    assert_eq!(
        rule.time_of_day,
        Some(chrono::NaiveTime::from_hms_opt(7, 30, 0).unwrap())
    );
    assert_eq!(rule.version, 2);
    assert_eq!(
        rule.start_date,
        NaiveDate::from_ymd_opt(2026, 10, 19).unwrap()
    );
    assert_eq!(rule.weekdays, vec![1, 3]);
    assert_eq!(
        rule.frequency,
        zodo_kernel::model::RecurrenceFrequency::Weekly
    );

    // Updated future instances: rule-owned fields changed through the normal task-update path.
    for (task_id, due) in [(&future_open, "2026-10-26"), (&future_last, "2026-11-09")] {
        let task = world.task(task_id).unwrap();
        assert_eq!(task.title, "深度复盘");
        assert_eq!(task.estimate_minutes, Some(45));
        assert_eq!(task.remaining_minutes, Some(45));
        assert_eq!(task.priority, 3);
        assert_eq!(task.status, TaskStatus::Open);
        assert_eq!(
            task.target_due,
            Some(NaiveDate::parse_from_str(due, "%Y-%m-%d").unwrap())
        );
        assert_eq!(task.series_id.as_deref(), Some(rule_id.as_str()));
        assert_eq!(task.version, 2);
    }

    // Manually customised future instance keeps its own fields while the rule owns the rest.
    let custom = world.task(&future_custom).unwrap();
    assert_eq!(custom.title, "深度复盘");
    assert_eq!(custom.estimate_minutes, Some(45));
    assert_eq!(custom.remaining_minutes, Some(45));
    assert_eq!(custom.priority, 3);
    assert_eq!(custom.description.as_deref(), Some("补充说明"));
    assert_eq!(custom.tags, vec!["复盘".to_string()]);
    assert_eq!(
        custom.target_due,
        Some(NaiveDate::from_ymd_opt(2026, 11, 4).unwrap())
    );
    assert_eq!(custom.version, 3);

    // Past generated instance is history and must not change.
    let past = world.task(&past_open).unwrap();
    assert_eq!(past.title, "每周复盘");
    assert_eq!(past.estimate_minutes, Some(30));
    assert_eq!(past.priority, 2);
    assert_eq!(past.version, 1);

    // Completed instances stay completed with their old values (no reopen, no rewrite).
    for task_id in [&past_done, &future_done] {
        let task = world.task(task_id).unwrap();
        assert_eq!(task.title, "每周复盘");
        assert_eq!(task.estimate_minutes, Some(30));
        assert_eq!(task.status, TaskStatus::Done);
        assert_eq!(task.version, 2);
    }

    // Locked instance is a fixed fact: untouched and reported as skipped.
    let locked = world.task(&future_locked).unwrap();
    assert_eq!(locked.title, "每周复盘");
    assert_eq!(locked.estimate_minutes, Some(30));
    assert_eq!(locked.version, 1);
    let locked_allocation = world
        .allocations
        .iter()
        .find(|allocation| allocation.task_id == future_locked)
        .unwrap();
    assert!(locked_allocation.locked);

    // Occurrence identity never moves: same count, dates, keys, statuses and task links.
    assert_eq!(world.occurrences.len(), 7);
    for occurrence in &world.occurrences {
        assert_eq!(occurrence.rule_id, rule_id);
        assert_eq!(occurrence.original_date, occurrence.current_date);
        assert_eq!(
            occurrence.occurrence_key,
            occurrence.original_date.format("%Y-%m-%d").to_string()
        );
        assert_eq!(occurrence.status, OccurrenceStatus::Generated);
        assert!(occurrence.task_id.is_some());
    }

    // The command is recorded as a recurrence change with an explicit rule update.
    let events = f.events();
    let last = events.last().expect("event recorded");
    assert_eq!(last.event_type, "recurrence.changed");
    assert_eq!(last.payload["operation"], "recurrence.update");
    let changes = last.payload["changes"].as_array().unwrap();
    assert!(changes.iter().any(|change| {
        change["kind"] == "recurrence_rule"
            && change["id"] == rule_id
            && change["action"] == "updated"
    }));
    assert!(changes.iter().any(|change| {
        change["kind"] == "task" && change["id"] == future_open && change["action"] == "updated"
    }));

    // Expanding the edited series afterwards uses the new rule fields for the new occurrence.
    let expanded = f
        .execute(
            "recurrence.expand",
            json!({"rule_id": rule_id, "through": "2026-11-12"}),
        )
        .unwrap();
    assert_eq!(expanded.data["task_ids"].as_array().unwrap().len(), 1);
    let new_task_id = expanded.data["task_ids"][0].as_str().unwrap().to_string();
    let world = f.world();
    let new_task = world.task(&new_task_id).unwrap();
    assert_eq!(new_task.title, "深度复盘");
    assert_eq!(new_task.estimate_minutes, Some(45));
    assert_eq!(new_task.remaining_minutes, Some(45));
    assert_eq!(new_task.priority, 3);
    assert_eq!(
        new_task.target_due,
        Some(NaiveDate::from_ymd_opt(2026, 11, 11).unwrap())
    );
    assert_eq!(new_task.series_id.as_deref(), Some(rule_id.as_str()));
    assert_eq!(world.occurrences.len(), 8);
}

#[test]
fn update_rejects_invalid_payloads_and_unknown_rules() {
    let mut f = Fixture::new();
    let rule_id = create_weekly_series(&mut f);
    let revision_before = f.world().workspace.revision;

    let cases: Vec<(&str, Value)> = vec![
        ("empty title", json!({"rule_id": rule_id, "title": "   "})),
        (
            "title too long",
            json!({"rule_id": rule_id, "title": "长".repeat(501)}),
        ),
        (
            "estimate too small",
            json!({"rule_id": rule_id, "estimate_minutes": 14}),
        ),
        (
            "estimate too large",
            json!({"rule_id": rule_id, "estimate_minutes": 1441}),
        ),
        (
            "priority too high",
            json!({"rule_id": rule_id, "priority": 5}),
        ),
        (
            "priority negative",
            json!({"rule_id": rule_id, "priority": -1}),
        ),
        (
            "clock without minutes",
            json!({"rule_id": rule_id, "time_of_day": "7"}),
        ),
        (
            "clock out of range",
            json!({"rule_id": rule_id, "time_of_day": "25:00"}),
        ),
        (
            "clock with seconds out of range",
            json!({"rule_id": rule_id, "time_of_day": "07:60:00"}),
        ),
        (
            "unknown scope",
            json!({"rule_id": rule_id, "scope": "this", "title": "只改本次"}),
        ),
        (
            "empty rule id",
            json!({"rule_id": "   ", "title": "无规则"}),
        ),
    ];
    for (label, payload) in cases {
        let failure = f
            .execute("recurrence.update", payload)
            .err()
            .unwrap_or_else(|| panic!("{label} should be rejected"));
        assert_eq!(
            failure.code,
            ErrorCode::ValidationFailed,
            "{label} returned {:?}",
            failure.code
        );
    }

    // A valid clock without zero padding is accepted and normalised.
    f.execute(
        "recurrence.update",
        json!({"rule_id": rule_id, "time_of_day": "7:05"}),
    )
    .unwrap();
    let world = f.world();
    assert_eq!(
        world.recurrence_rules[0].time_of_day,
        Some(chrono::NaiveTime::from_hms_opt(7, 5, 0).unwrap())
    );

    // Explicit null clears the rule clock without touching task instances.
    f.execute(
        "recurrence.update",
        json!({"rule_id": rule_id, "time_of_day": null}),
    )
    .unwrap();
    let world = f.world();
    assert_eq!(world.recurrence_rules[0].time_of_day, None);
    assert_eq!(world.tasks.len(), 7);

    // Unknown rule is a not-found error, and nothing is written.
    let missing = f
        .execute(
            "recurrence.update",
            json!({"rule_id": "rec_missing", "title": "不存在"}),
        )
        .unwrap_err();
    assert_eq!(missing.code, ErrorCode::NotFound);

    let world = f.world();
    assert_eq!(world.workspace.revision, revision_before + 2);
    assert_eq!(world.recurrence_rules.len(), 1);
}

#[test]
fn update_uses_normal_idempotency_and_requires_write() {
    let mut f = Fixture::new();
    let rule_id = create_weekly_series(&mut f);
    let revision_before = f.world().workspace.revision;

    let payload = json!({
        "rule_id": rule_id,
        "scope": "future",
        "title": "幂等复盘",
        "estimate_minutes": 60
    });
    let first = f
        .execute_keyed("recurrence.update", payload.clone(), "op_recurrence_scope")
        .unwrap();
    let second = f
        .execute_keyed("recurrence.update", payload, "op_recurrence_scope")
        .unwrap();

    assert!(second.replayed);
    assert_eq!(second.operation_id, first.operation_id);
    assert_eq!(second.revision, first.revision);
    let world = f.world();
    assert_eq!(world.workspace.revision, revision_before + 1);
    assert_eq!(world.recurrence_rules[0].version, 2);
    assert_eq!(world.recurrence_rules[0].title, "幂等复盘");
    // Five open future instances were rewritten once, not twice.
    let rewritten: Vec<_> = world
        .tasks
        .iter()
        .filter(|task| task.title == "幂等复盘")
        .collect();
    assert_eq!(rewritten.len(), 5);
    assert!(rewritten.iter().all(|task| task.version == 2));

    // Same key with different content is an idempotency mismatch.
    let mismatch = f
        .execute_keyed(
            "recurrence.update",
            json!({"rule_id": rule_id, "title": "不同内容"}),
            "op_recurrence_scope",
        )
        .unwrap_err();
    assert_eq!(mismatch.code, ErrorCode::IdempotencyMismatch);

    // The command requires an explicit write capability, not just read.
    let mut read_only = f.context.clone();
    read_only.capabilities = vec!["read".to_string()];
    let denied = f
        .execute_as(
            &read_only,
            "recurrence.update",
            json!({"rule_id": rule_id, "title": "只读不应写入"}),
        )
        .unwrap_err();
    assert_eq!(denied.code, ErrorCode::PermissionDenied);
    let world = f.world();
    assert_eq!(world.recurrence_rules[0].title, "幂等复盘");
}
