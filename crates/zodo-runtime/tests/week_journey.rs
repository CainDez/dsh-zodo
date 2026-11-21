use chrono::{TimeZone, Utc};
use serde_json::{Value, json};
use zodo_kernel::app::Application;
use zodo_kernel::commands::{CommandEnvelope, CommandResult, QueryEnvelope};
use zodo_kernel::model::{CommandContext, PrincipalKind, Workspace};
use zodo_kernel::ports::{FixedClock, SequentialIds, Store};
use zodo_runtime::SqliteStore;

struct Fixture {
    store: Option<SqliteStore>,
    clock: FixedClock,
    ids: SequentialIds,
    context: CommandContext,
    counter: u64,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SqliteStore::open(dir.path().join("week.sqlite3")).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 10, 24, 1, 20, 0).unwrap();
        let world = zodo_kernel::model::World::new(Workspace::new(
            "ws_test".to_string(),
            "Test".to_string(),
            now,
        ));
        store.create_workspace(&world).unwrap();
        Self {
            store: Some(store),
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
            store: self.store.as_mut().unwrap(),
            clock: &self.clock,
            ids: &self.ids,
            instance_id: "rt_test".to_string(),
        };
        app.execute(&self.context, envelope)
    }

    fn world(&mut self) -> zodo_kernel::model::World {
        self.store
            .as_mut()
            .unwrap()
            .load_world("ws_test")
            .unwrap()
            .unwrap()
    }
    fn reopen(&mut self) {
        drop(self.store.take()); // Close the actual connection before reopening the same fixture DB.
        self.store = Some(SqliteStore::open(self._dir.path().join("week.sqlite3")).unwrap());
    }
    fn task(&mut self, title: &str, minutes: u32) -> String {
        self.execute(
            "task.create",
            json!({"task": {"title": title, "estimate_minutes": minutes}}),
        )
        .unwrap()
        .data["changes"][0]["id"]
            .as_str()
            .unwrap()
            .into()
    }
    fn capacity(&mut self, start: &str, end: &str, minutes: u32) -> String {
        self.execute("capacity.create", json!({"capacity": {"title": "Week capacity", "start": start, "end": end, "capacity_minutes": minutes}})).unwrap().data["changes"][0]["id"].as_str().unwrap().into()
    }
    fn propose(&mut self, tasks: &[String], replan: bool) -> CommandResult {
        let before = self.world();
        let result = self.execute("plan.propose", json!({"task_ids": tasks,
            "horizon_start": "2026-10-25T16:00:00Z", "horizon_end": "2026-11-01T16:00:00Z", "move_unlocked": replan
        })).unwrap();
        assert_eq!(self.world().allocations, before.allocations);
        assert_eq!(result.data["proposal"]["status"], "validated");
        assert!(
            result.data["plan"]["unplaced"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        result
    }
    fn approve(&mut self, proposal: &CommandResult) -> (String, String) {
        let id = proposal.data["proposal"]["id"].as_str().unwrap().to_owned();
        let before = self.world().allocations;
        let result = self
            .execute("proposal.approve", json!({"proposal_id": id}))
            .unwrap();
        assert_eq!(self.world().allocations, before);
        (id, result.data["approval"]["id"].as_str().unwrap().into())
    }
    fn commit(&mut self, ids: &(String, String)) -> zodo_kernel::Result<CommandResult> {
        self.execute(
            "changes.commit",
            json!({"proposal_id": ids.0, "approval_grant": ids.1}),
        )
    }
    fn schedule(&mut self, start: &str, end: &str) -> Value {
        let query = QueryEnvelope {
            schema: "zodo.query/1".into(),
            request_id: "week-query".into(),
            workspace_id: "ws_test".into(),
            query: "schedule.get".into(),
            payload: json!({"start": start, "end": end}),
        };
        Application {
            store: self.store.as_mut().unwrap(),
            clock: &self.clock,
            ids: &self.ids,
            instance_id: "rt_test".into(),
        }
        .query(&self.context, query)
        .unwrap()
        .data
    }
}

fn assert_feasible(world: &zodo_kernel::model::World) {
    for (index, allocation) in world.allocations.iter().enumerate() {
        for other in &world.allocations[index + 1..] {
            assert!(allocation.end <= other.start || other.end <= allocation.start);
        }
        for fixed in &world.commitments {
            assert!(allocation.end <= fixed.start || fixed.end <= allocation.start);
        }
        for busy in world.calendar_events.iter().filter(|event| event.busy) {
            assert!(allocation.end <= busy.start || busy.end <= allocation.start);
        }
    }
    for block in &world.capacity_blocks {
        let rows: Vec<_> = world
            .allocations
            .iter()
            .filter(|a| a.capacity_block_id.as_deref() == Some(block.id.as_str()))
            .collect();
        assert!(rows.iter().map(|a| a.minutes).sum::<u32>() <= block.capacity_minutes);
        assert!(
            rows.iter()
                .all(|a| a.start >= block.start && a.end <= block.end)
        );
    }
    assert!(
        world.activities.is_empty(),
        "scheduling is not proof of performed work"
    );
    assert!(
        world.focus_sessions.is_empty(),
        "no focus actions in this journey"
    );
}

#[test]
fn whole_week_additive_replan_conflict_and_storage_reopen_preserve_facts() {
    let mut f = Fixture::new();
    assert_eq!(f.world().workspace.settings.timezone, "Asia/Shanghai");
    // One synthetic local Monday–Sunday week, with usable capacity on five days and a
    // night block. Local day boundaries are UTC16:00, not UTC00:00 or machine-local time.
    for day in 26..=30 {
        f.capacity(
            &format!("2026-10-{day}T01:00:00Z"),
            &format!("2026-10-{day}T03:00:00Z"),
            120,
        );
    }
    let night = f.capacity("2026-10-27T15:00:00Z", "2026-10-27T17:00:00Z", 120);
    f.execute("commitment.create", json!({"commitment": {"title": "Monday meeting", "start": "2026-10-26T01:30:00Z", "end": "2026-10-26T02:00:00Z"}})).unwrap();
    f.execute("commitment.create", json!({"commitment": {"title": "Fixed overnight appointment", "start": "2026-10-26T15:30:00Z", "end": "2026-10-26T16:30:00Z"}})).unwrap();
    f.execute("calendar.import_busy", json!({"source_id": null, "provider": "fixture", "display_name": "Synthetic busy", "external_id": "week-calendar", "events": [{"external_id": "wed-busy", "title": "Synthetic event", "start": "2026-10-28T01:00:00Z", "end": "2026-10-28T02:00:00Z"}], "replace_source": true, "cursor": null})).unwrap();
    let main = f.task("Main week work", 420);
    f.execute("allocation.create", json!({"allocation": {"task_id": main, "capacity_block_id": night, "start": "2026-10-27T15:45:00Z", "end": "2026-10-27T16:15:00Z", "minutes": 30, "locked": true}})).unwrap();
    let fixed = f.world();
    let locked = fixed.allocations[0].clone();

    // Initial schedule, approved and committed, actually spans several local days.
    let first = f.propose(std::slice::from_ref(&main), false);
    let first_ids = f.approve(&first);
    f.commit(&first_ids).unwrap();
    let initial = f.world();
    assert_feasible(&initial);
    let tz: chrono_tz::Tz = "Asia/Shanghai".parse().unwrap();
    let dates: std::collections::BTreeSet<_> = initial
        .allocations
        .iter()
        .map(|a| a.start.with_timezone(&tz).date_naive())
        .collect();
    assert!(
        dates.len() >= 4,
        "fixture must genuinely span the week, not one large slot"
    );
    assert!(initial.allocations.contains(&locked));
    assert_eq!(
        initial
            .allocations
            .iter()
            .filter(|a| a.task_id == main)
            .map(|a| a.minutes)
            .sum::<u32>(),
        420
    );

    // The UI's default mode sends move_unlocked=false; an API field omission is NOT tested.
    let extra = f.task("New work arriving mid-plan", 150);
    let tasks = vec![main.clone(), extra.clone()];
    let additive = f.propose(&tasks, false);
    assert_eq!(additive.data["plan"]["replace_task_ids"], json!([]));
    let additive_ids = f.approve(&additive);
    f.commit(&additive_ids).unwrap();
    let expanded = f.world();
    assert_feasible(&expanded);
    assert!(
        initial
            .allocations
            .iter()
            .all(|a| expanded.allocations.contains(a)),
        "additive scheduling retains original identities and contents"
    );
    assert_eq!(
        expanded
            .allocations
            .iter()
            .filter(|a| a.task_id == extra)
            .map(|a| a.minutes)
            .sum::<u32>(),
        150
    );

    // Deliberately create a genuine new fixed-time conflict after approving an explicit
    // replan. The stale grant must not erase either the conflict or any existing allocation.
    let pending = f.propose(&tasks, true);
    let pending_ids = f.approve(&pending);
    f.execute("commitment.create", json!({"commitment": {"title": "New Thursday appointment", "start": "2026-10-29T01:00:00Z", "end": "2026-10-29T01:30:00Z"}})).unwrap();
    let conflicted = f.world();
    let conflict = conflicted
        .commitments
        .iter()
        .find(|c| c.title == "New Thursday appointment")
        .unwrap();
    assert!(
        conflicted
            .allocations
            .iter()
            .any(|a| a.start < conflict.end && a.end > conflict.start)
    );
    assert_eq!(
        f.commit(&pending_ids).unwrap_err().code,
        zodo_kernel::error::ErrorCode::RevisionConflict
    );
    let rejected = f.world();
    assert_eq!(rejected.allocations, conflicted.allocations);
    assert_eq!(rejected.commitments, conflicted.commitments);
    assert_eq!(rejected.approval_grants, conflicted.approval_grants);
    assert_eq!(rejected.workspace.revision, conflicted.workspace.revision);

    // Actual SQLite close/reopen between failure and recovery, not just another query.
    f.reopen();
    assert_eq!(
        serde_json::to_value(f.world()).unwrap(),
        serde_json::to_value(&rejected).unwrap()
    );
    let fresh = f.propose(&tasks, true);
    let expected: std::collections::BTreeSet<_> = rejected
        .allocations
        .iter()
        .filter(|a| !a.locked)
        .map(|a| a.id.as_str())
        .collect();
    let bound: std::collections::BTreeSet<_> =
        fresh.data["proposal"]["operations"][0]["payload"]["expected_removed_allocation_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap())
            .collect();
    assert_eq!(
        bound, expected,
        "fresh replan binds only the actual movable allocation IDs"
    );
    let fresh_ids = f.approve(&fresh);
    f.commit(&fresh_ids).unwrap();
    let recovered = f.world();
    assert_feasible(&recovered);
    assert!(recovered.allocations.contains(&locked));
    assert_eq!(recovered.commitments, conflicted.commitments);
    assert_eq!(recovered.calendar_events, fixed.calendar_events);
    assert_eq!(recovered.capacity_blocks, fixed.capacity_blocks);
    assert!(
        recovered
            .allocations
            .iter()
            .filter(|a| !a.locked)
            .all(|a| !expected.contains(a.id.as_str()))
    );
    for (task, minutes) in [(&main, 420), (&extra, 150)] {
        assert_eq!(
            recovered
                .allocations
                .iter()
                .filter(|a| &a.task_id == task)
                .map(|a| a.minutes)
                .sum::<u32>(),
            minutes
        );
        assert_eq!(recovered.task(task).unwrap().actual_minutes, 0);
    }

    // Cross-midnight identity remains discoverable in both adjacent local-day queries.
    let tuesday = f.schedule("2026-10-26T16:00:00Z", "2026-10-27T16:00:00Z");
    let wednesday = f.schedule("2026-10-27T16:00:00Z", "2026-10-28T16:00:00Z");
    for day in [&tuesday, &wednesday] {
        assert!(
            day["allocations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a["id"] == locked.id)
        );
    }
    assert!(
        tuesday["commitments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["title"] == "Fixed overnight appointment")
    );
    f.reopen();
    assert_eq!(
        serde_json::to_value(f.world()).unwrap(),
        serde_json::to_value(&recovered).unwrap()
    );
    assert_eq!(
        f.schedule("2026-10-26T16:00:00Z", "2026-10-27T16:00:00Z"),
        tuesday
    );
}
