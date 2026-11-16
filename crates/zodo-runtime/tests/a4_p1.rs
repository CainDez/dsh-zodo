use chrono::{TimeZone, Utc};
use serde_json::{Value, json};
use zodo_kernel::app::Application;
use zodo_kernel::commands::{CommandEnvelope, CommandResult, QueryEnvelope};
use zodo_kernel::error::ErrorCode;
use zodo_kernel::model::{CommandContext, PrincipalKind, Workspace};
use zodo_kernel::ports::{Clock, FixedClock, SequentialIds, Store};
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
        let mut store = SqliteStore::open(dir.join("a4.sqlite3")).unwrap();
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

    fn query_result(&mut self, name: &str, payload: Value) -> zodo_kernel::Result<Value> {
        self.counter += 1;
        let envelope = QueryEnvelope {
            schema: "zodo.query/1".to_string(),
            request_id: format!("q_{}", self.counter),
            workspace_id: "ws_test".to_string(),
            query: name.to_string(),
            payload,
        };
        let mut app = Application {
            store: &mut self.store,
            clock: &self.clock,
            ids: &self.ids,
            instance_id: "rt_test".to_string(),
        };
        app.query(&self.context, envelope).map(|result| result.data)
    }

    fn query(&mut self, name: &str, payload: Value) -> Value {
        self.query_result(name, payload).unwrap()
    }

    fn world(&mut self) -> zodo_kernel::model::World {
        self.store.load_world("ws_test").unwrap().unwrap()
    }
}

#[test]
fn habits_log_dedupe_and_persist() {
    let mut f = Fixture::new();
    let habit = f
        .execute(
            "habit.create",
            json!({
                "habit": {
                    "title": "晨间写作",
                    "timezone": "Asia/Shanghai",
                    "start_date": "2026-10-20",
                    "weekly_target": 5,
                    "scheduled_weekdays": [1,2,3,4,5],
                    "time_of_day": "07:30:00"
                }
            }),
        )
        .unwrap();
    let habit_id = habit.data["habit"]["id"].as_str().unwrap().to_string();
    assert_eq!(f.world().habits.len(), 1);

    let log = f
        .execute(
            "habit.log",
            json!({
                "habit_id": habit_id,
                "occurred_at": "2026-10-24T02:00:00Z",
                "minutes": 25,
                "note": "写了 300 字",
                "dedupe_key": "habit-day-1"
            }),
        )
        .unwrap();
    assert_eq!(log.revision, 2);
    let world = f.world();
    assert_eq!(world.activities.len(), 1);
    assert_eq!(
        world.activities[0].habit_id.as_deref(),
        Some(habit_id.as_str())
    );
    assert_eq!(world.activities[0].minutes, 25);

    let duplicate = f.execute(
        "habit.log",
        json!({
            "habit_id": habit_id,
            "minutes": 10,
            "dedupe_key": "habit-day-1"
        }),
    );
    assert_eq!(duplicate.unwrap_err().code, ErrorCode::ValidationFailed);

    let duplicate_title = f.execute(
        "habit.create",
        json!({
            "habit": {
                "title": "晨间写作",
                "timezone": "Asia/Shanghai",
                "start_date": "2026-10-20",
                "weekly_target": 3
            }
        }),
    );
    assert_eq!(
        duplicate_title.unwrap_err().code,
        ErrorCode::ValidationFailed
    );

    f.execute(
        "habit.update",
        json!({
            "id": habit_id,
            "habit": {
                "title": "晨间写作与复盘",
                "timezone": "Asia/Shanghai",
                "start_date": "2026-10-20",
                "end_date": "2026-12-31",
                "weekly_target": 6,
                "scheduled_weekdays": [1,2,3,4,5,6]
            }
        }),
    )
    .unwrap();
    let world = f.world();
    assert_eq!(world.habits[0].title, "晨间写作与复盘");
    assert_eq!(world.habits[0].weekly_target, 6);
}

#[test]
fn batch_preview_then_apply_is_atomic() {
    let mut f = Fixture::new();
    let first = f
        .execute(
            "task.create",
            json!({"task": {"title": "批量一", "estimate_minutes": 10}}),
        )
        .unwrap();
    let second = f
        .execute(
            "task.create",
            json!({"task": {"title": "批量二", "estimate_minutes": 10}}),
        )
        .unwrap();
    let ids = vec![
        first.data["changes"][0]["id"].as_str().unwrap().to_string(),
        second.data["changes"][0]["id"]
            .as_str()
            .unwrap()
            .to_string(),
    ];
    let preview = f
        .execute(
            "task.batch_update",
            json!({"ids": ids, "archived": true, "add_tags": ["批量"], "preview": true}),
        )
        .unwrap();
    assert_eq!(
        preview.status,
        zodo_kernel::commands::ResultStatus::Validated
    );
    assert_eq!(preview.data["affected"], 2);
    assert!(f.world().tasks.iter().all(|task| !task.archived));

    f.execute(
        "task.batch_update",
        json!({"ids": ids, "archived": true, "add_tags": ["批量"], "preview": false}),
    )
    .unwrap();
    let world = f.world();
    assert!(world.tasks.iter().all(|task| task.archived));
    assert!(
        world
            .tasks
            .iter()
            .all(|task| task.tags.contains(&"批量".to_string()))
    );

    let unknown = f.execute(
        "task.batch_update",
        json!({"ids": ["missing"], "archived": false, "preview": false}),
    );
    assert_eq!(unknown.unwrap_err().code, ErrorCode::NotFound);
}

#[test]
fn automation_manual_run_is_bounded_and_audited() {
    let mut f = Fixture::new();
    let created = f
        .execute(
            "automation.create",
            json!({
                "automation": {
                    "name": "每日回顾任务",
                    "trigger": "manual",
                    "conditions": {},
                    "actions": [{"kind": "task.create", "task": {"title": "自动创建：回顾今天", "estimate_minutes": 10}}],
                    "enabled": true,
                    "budget": 2
                }
            }),
        )
        .unwrap();
    let id = created.data["automation"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let first = f.execute("automation.run", json!({"id": id})).unwrap();
    assert_eq!(first.data["task_ids"].as_array().unwrap().len(), 1);
    assert_eq!(f.world().automation_rules[0].runs, 1);
    assert!(f.world().automation_rules[0].last_run_at.is_some());

    f.execute("automation.run", json!({"id": id})).unwrap();
    let world = f.world();
    assert_eq!(world.automation_rules[0].runs, 2);
    assert!(!world.automation_rules[0].enabled);
    assert_eq!(world.tasks.len(), 2);

    let budget_exceeded = f.execute("automation.run", json!({"id": id}));
    assert_eq!(
        budget_exceeded.unwrap_err().code,
        ErrorCode::ValidationFailed
    );

    f.execute("automation.toggle", json!({"id": id, "enabled": true}))
        .unwrap();
    assert!(f.world().automation_rules[0].enabled);
    f.execute("automation.delete", json!({"id": id})).unwrap();
    assert!(f.world().automation_rules.is_empty());
}

#[test]
fn tags_saved_views_and_milestones_are_crud_and_validated() {
    let mut f = Fixture::new();
    let tag = f
        .execute(
            "tag.create",
            json!({"tag": {"name": "深度工作", "color": "#157E78"}}),
        )
        .unwrap();
    let tag_id = tag.data["tag"]["id"].as_str().unwrap().to_string();
    let duplicate = f.execute("tag.create", json!({"tag": {"name": "深度工作"}}));
    assert_eq!(duplicate.unwrap_err().code, ErrorCode::ValidationFailed);
    f.execute(
        "tag.update",
        json!({"id": tag_id, "tag": {"name": "深度工作+", "color": "#106B66"}}),
    )
    .unwrap();
    assert_eq!(f.world().tags[0].name, "深度工作+");

    let view = f
        .execute(
            "saved_query.create",
            json!({"saved_query": {"name": "本周深度工作", "filter": {"tag": "深度工作+", "status": "open"}}}),
        )
        .unwrap();
    let view_id = view.data["saved_query"]["id"].as_str().unwrap().to_string();
    f.execute(
        "saved_query.update",
        json!({"id": view_id, "saved_query": {"name": "深度工作", "filter": {"tag": "深度工作+", "overdue": true}}}),
    )
    .unwrap();
    assert_eq!(f.world().saved_queries[0].name, "深度工作");
    assert_eq!(f.world().saved_queries[0].filter["overdue"], true);

    let project = f
        .execute(
            "project.create",
            json!({"project": {"title": "A4 项目", "scope": "验证里程碑"}}),
        )
        .unwrap();
    let project_id = project.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let milestone = f
        .execute(
            "milestone.add",
            json!({"project_id": project_id, "milestone": {"title": "完成核心", "due": "2026-11-01"}}),
        )
        .unwrap();
    let milestone_id = milestone.data["milestone"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.execute(
        "milestone.update",
        json!({
            "project_id": project_id,
            "milestone_id": milestone_id,
            "milestone": {"title": "完成核心并验证", "due": "2026-11-02", "done": true}
        }),
    )
    .unwrap();
    let world = f.world();
    assert!(world.projects[0].milestones[0].done);
    assert_eq!(world.projects[0].milestones[0].title, "完成核心并验证");
    f.execute(
        "milestone.remove",
        json!({"project_id": project_id, "milestone_id": milestone_id}),
    )
    .unwrap();
    assert!(f.world().projects[0].milestones.is_empty());
    f.execute("saved_query.delete", json!({"id": view_id}))
        .unwrap();
    f.execute("tag.delete", json!({"id": tag_id})).unwrap();
    assert!(f.world().saved_queries.is_empty());
    assert!(f.world().tags.is_empty());
}

#[test]
fn capacity_candidates_exclude_fixed_and_respect_minimum() {
    let mut f = Fixture::new();
    f.execute(
        "commitment.create",
        json!({
            "commitment": {
                "title": "固定会议",
                "start": "2026-10-24T02:00:00Z",
                "end": "2026-10-24T03:00:00Z"
            }
        }),
    )
    .unwrap();
    let data = f.query(
        "capacity.candidates",
        json!({"start": "2026-10-24", "end": "2026-10-24", "min_minutes": 30}),
    );
    let candidates = data["candidates"].as_array().unwrap();
    assert!(!candidates.is_empty());
    for candidate in candidates {
        let start = candidate["start"].as_str().unwrap();
        let end = candidate["end"].as_str().unwrap();
        assert!(
            end <= "2026-10-24T02:00:00Z" || start >= "2026-10-24T03:00:00Z",
            "candidate overlaps fixed commitment: {candidate}"
        );
        assert!(candidate["minutes"].as_u64().unwrap() >= 30);
    }
}

#[test]
fn duplicate_detection_and_merge_preserve_links() {
    let mut f = Fixture::new();
    let a = f
        .execute(
            "task.create",
            json!({"task": {"title": "重复任务", "estimate_minutes": 10}}),
        )
        .unwrap();
    let b = f
        .execute(
            "task.create",
            json!({"task": {"title": "重复 任务!", "estimate_minutes": 20, "tags": ["来自B"]}}),
        )
        .unwrap();
    let c = f
        .execute(
            "task.create",
            json!({"task": {"title": "下游任务", "estimate_minutes": 10}}),
        )
        .unwrap();
    let a_id = a.data["changes"][0]["id"].as_str().unwrap().to_string();
    let b_id = b.data["changes"][0]["id"].as_str().unwrap().to_string();
    let c_id = c.data["changes"][0]["id"].as_str().unwrap().to_string();
    f.execute(
        "dependency.add",
        json!({"predecessor_id": a_id, "successor_id": c_id}),
    )
    .unwrap();
    let duplicates = f.query("tasks.duplicates", json!({}));
    let groups = duplicates["groups"].as_array().unwrap();
    assert!(
        groups
            .iter()
            .any(|group| group["count"].as_u64().unwrap() >= 2)
    );

    let merged = f
        .execute(
            "task.merge",
            json!({"survivor_id": a_id, "duplicate_ids": [b_id]}),
        )
        .unwrap();
    assert_eq!(
        merged.status,
        zodo_kernel::commands::ResultStatus::Committed
    );
    let world = f.world();
    let survivor = world.task(&a_id).unwrap();
    assert!(survivor.tags.contains(&"来自B".to_string()));
    assert!(
        survivor
            .tags
            .iter()
            .any(|tag| tag == &format!("merged_from:{b_id}"))
    );
    let duplicate = world.task(&b_id).unwrap();
    assert!(duplicate.archived);
    assert!(
        duplicate
            .tags
            .iter()
            .any(|tag| tag == &format!("merged_into:{a_id}"))
    );
    assert!(
        world
            .dependencies
            .iter()
            .any(|dependency| dependency.predecessor_id == a_id && dependency.successor_id == c_id)
    );
    assert!(
        !world
            .dependencies
            .iter()
            .any(|dependency| dependency.predecessor_id == b_id)
    );
    assert!(
        f.execute(
            "task.merge",
            json!({"survivor_id": a_id, "duplicate_ids": [a_id]})
        )
        .is_err()
    );
}

#[test]
fn recurrence_chain_dependencies_are_bounded_and_idempotent() {
    let mut f = Fixture::new();
    let created = f
        .execute(
            "recurrence.create",
            json!({
                "rule": {
                    "title": "链式周任务",
                    "timezone": "Asia/Shanghai",
                    "start_date": "2026-10-26",
                    "frequency": "weekly",
                    "interval": 1,
                    "weekdays": [1],
                    "estimate_minutes": 30,
                    "depends_on_previous": true
                },
                "through": "2026-11-16"
            }),
        )
        .unwrap();
    assert_eq!(created.data["task_ids"].as_array().unwrap().len(), 4);
    let rule_id = created.data["rule"]["id"].as_str().unwrap().to_string();
    let world = f.world();
    assert_eq!(world.dependencies.len(), 3);
    let again = f
        .execute(
            "recurrence.expand",
            json!({"rule_id": rule_id, "through": "2026-11-16"}),
        )
        .unwrap();
    assert_eq!(again.data["task_ids"].as_array().unwrap().len(), 0);
    assert_eq!(f.world().dependencies.len(), 3);
}

#[test]
fn cross_period_dependency_planning_reports_diagnostics() {
    let mut f = Fixture::new();
    let predecessor = f
        .execute(
            "task.create",
            json!({"task": {"title": "跨期前置", "estimate_minutes": 30}}),
        )
        .unwrap();
    let successor = f
        .execute(
            "task.create",
            json!({"task": {"title": "跨期后继", "estimate_minutes": 30}}),
        )
        .unwrap();
    let predecessor_id = predecessor.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let successor_id = successor.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.execute(
        "dependency.add",
        json!({"predecessor_id": predecessor_id, "successor_id": successor_id}),
    )
    .unwrap();
    f.execute(
        "capacity.create",
        json!({"capacity": {
            "title": "次日容量",
            "start": "2026-10-25T01:00:00Z",
            "end": "2026-10-25T05:00:00Z",
            "capacity_minutes": 240
        }}),
    )
    .unwrap();
    let blocked = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": [successor_id],
                "horizon_start": "2026-10-25T00:00:00Z",
                "horizon_end": "2026-10-26T00:00:00Z",
                "move_unlocked": true
            }),
        )
        .unwrap();
    assert!(
        blocked.data["plan"]["unplaced"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["task_id"] == successor_id && row["reason"] == "dependency_blocked")
    );
    assert!(
        !blocked.data["plan"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // A future allocation for the predecessor satisfies the cross-period dependency.
    let future_capacity = f
        .execute(
            "capacity.create",
            json!({"capacity": {
                "title": "跨期容量",
                "start": "2026-10-27T01:00:00Z",
                "end": "2026-10-27T05:00:00Z",
                "capacity_minutes": 240
            }}),
        )
        .unwrap();
    let capacity_id = future_capacity.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.execute(
        "allocation.create",
        json!({"allocation": {
            "task_id": predecessor_id,
            "capacity_block_id": capacity_id,
            "start": "2026-10-27T01:00:00Z",
            "end": "2026-10-27T01:30:00Z",
            "minutes": 30
        }}),
    )
    .unwrap();
    let ready = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": [successor_id],
                "horizon_start": "2026-10-25T00:00:00Z",
                "horizon_end": "2026-10-26T00:00:00Z",
                "move_unlocked": true
            }),
        )
        .unwrap();
    assert!(
        !ready.data["plan"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["code"] == "DEPENDENCY_BLOCKED")
    );
}

#[test]
fn content_retrieval_requires_consent_and_is_local_only() {
    let mut f = Fixture::new();
    f.execute(
        "note.create",
        json!({"note": {
            "title": "作品集素材",
            "body": "线上展示需要三篇案例说明和首页结构",
            "data_class": "device_only"
        }}),
    )
    .unwrap();
    let denied = f.query_result(
        "content.retrieve",
        json!({"text": "案例", "consent": false}),
    );
    assert_eq!(denied.unwrap_err().code, ErrorCode::PermissionDenied);
    let found = f
        .query_result(
            "content.retrieve",
            json!({"text": "案例", "consent": true, "limit": 5}),
        )
        .unwrap();
    assert_eq!(found["count"], 1);
    assert_eq!(found["items"][0]["data_class"], "device_only");
    assert_eq!(found["scope"], "local_workspace_only");
    assert_eq!(found["personalisation"], "not_applied");
    assert!(
        f.query_result("content.retrieve", json!({"text": "  ", "consent": true}))
            .is_err()
    );
}

#[test]
fn focus_lifecycle_is_single_pause_resume_finish_and_recovers() {
    use chrono::Duration;
    use zodo_kernel::model::FocusState;

    let mut f = Fixture::new();
    let task = f
        .execute(
            "task.create",
            json!({"task": {"title": "专注任务", "estimate_minutes": 60}}),
        )
        .unwrap();
    let task_id = task.data["changes"][0]["id"].as_str().unwrap().to_string();
    f.execute("focus.start", json!({"task_id": task_id}))
        .unwrap();
    let duplicate = f.execute("focus.start", json!({"task_id": task_id}));
    assert!(duplicate.is_err());
    let start = f.clock.now();
    f.clock.set(start + Duration::minutes(10));
    f.execute("focus.pause", json!({})).unwrap();
    assert_eq!(f.world().active_focus().unwrap().state, FocusState::Paused);
    f.clock.set(start + Duration::minutes(15));
    f.execute("focus.resume", json!({})).unwrap();
    f.clock.set(start + Duration::minutes(30));
    f.execute("focus.finish", json!({"note": "完成一段"}))
        .unwrap();
    assert!(f.world().active_focus().is_none());
    let world = f.world();
    let session = world.focus_sessions.last().unwrap();
    assert_eq!(session.state, FocusState::Completed);
    assert!(session.segments.len() >= 2);

    // Restart in the middle of a focus: the persisted session remains active and can be finished.
    f.execute("focus.start", json!({"task_id": task_id}))
        .unwrap();
    let context = f.context.clone();
    let mut app = Application {
        store: &mut f.store,
        clock: &f.clock,
        ids: &f.ids,
        instance_id: "rt_restart".to_string(),
    };
    let envelope = CommandEnvelope {
        schema: "zodo.command/1".to_string(),
        request_id: "restart-finish".to_string(),
        idempotency_key: "restart-finish".to_string(),
        workspace_id: "ws_test".to_string(),
        operation: "focus.finish".to_string(),
        operation_version: 1,
        base_revision: None,
        policy_revision: None,
        payload: json!({}),
    };
    app.execute(&context, envelope).unwrap();
    assert!(f.world().active_focus().is_none());
}

#[test]
fn goal_achieved_needs_evidence_and_project_cancel_keeps_tasks() {
    use zodo_kernel::model::{GoalStatus, ProjectStatus, TaskStatus};

    let mut f = Fixture::new();
    let goal = f
        .execute("goal.create", json!({"goal": {"title": "目标证据测试"}}))
        .unwrap();
    let goal_id = goal.data["changes"][0]["id"].as_str().unwrap().to_string();
    let no_evidence = f.execute("goal.status", json!({"id": goal_id, "status": "achieved"}));
    assert!(no_evidence.is_err());
    f.execute(
        "goal.status",
        json!({"id": goal_id, "status": "achieved", "evidence": "用户确认：作品集已上线"}),
    )
    .unwrap();
    let world = f.world();
    let achieved = world.goal(&goal_id).unwrap();
    assert_eq!(achieved.status, GoalStatus::Achieved);
    assert!(achieved.achieved_at.is_some());
    assert_eq!(achieved.evidence.len(), 1);
    f.execute("goal.status", json!({"id": goal_id, "status": "active"}))
        .unwrap();
    let reopened = f.world().goal(&goal_id).unwrap().clone();
    assert_eq!(reopened.status, GoalStatus::Active);
    assert!(reopened.achieved_at.is_none());
    assert_eq!(reopened.evidence.len(), 1);
    f.execute(
        "goal.update",
        json!({"id": goal_id, "goal": {"title": "目标证据测试更新", "success_criteria": "上线并验证"}}),
    )
    .unwrap();
    assert_eq!(f.world().goal(&goal_id).unwrap().title, "目标证据测试更新");

    let project = f
        .execute(
            "project.create",
            json!({"project": {"title": "取消不完成任务", "goal_id": goal_id}}),
        )
        .unwrap();
    let project_id = project.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let task = f
        .execute(
            "task.create",
            json!({"task": {"title": "项目任务", "project_id": project_id}}),
        )
        .unwrap();
    let task_id = task.data["changes"][0]["id"].as_str().unwrap().to_string();
    f.execute(
        "project.update",
        json!({"id": project_id, "project": {"title": "取消不完成任务（更新）", "goal_id": goal_id}}),
    )
    .unwrap();
    assert_eq!(
        f.world().project(&project_id).unwrap().title,
        "取消不完成任务（更新）"
    );
    f.execute(
        "project.status",
        json!({"id": project_id, "status": "cancelled"}),
    )
    .unwrap();
    let world = f.world();
    assert_eq!(
        world.project(&project_id).unwrap().status,
        ProjectStatus::Cancelled
    );
    assert_eq!(world.task(&task_id).unwrap().status, TaskStatus::Open);
}

#[test]
fn parent_child_cycle_refused_and_rollup_is_not_leaf() {
    let mut f = Fixture::new();
    let parent = f
        .execute(
            "task.create",
            json!({"task": {"title": "父任务", "estimate_minutes": 30}}),
        )
        .unwrap();
    let parent_id = parent.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let child = f
        .execute(
            "task.create",
            json!({"task": {"title": "子任务", "parent_id": parent_id, "estimate_minutes": 30}}),
        )
        .unwrap();
    let child_id = child.data["changes"][0]["id"].as_str().unwrap().to_string();
    let cycle = f.execute(
        "task.update",
        json!({"id": parent_id, "task": {"title": "父任务", "parent_id": child_id}, "expected_version": 1}),
    );
    assert!(cycle.is_err());
    let world = f.world();
    assert_eq!(world.task(&parent_id).unwrap().parent_id, None);
    assert_eq!(
        world.task(&child_id).unwrap().parent_id.as_deref(),
        Some(parent_id.as_str())
    );
    assert!(!world.is_leaf(&parent_id));
    assert!(world.is_leaf(&child_id));
    assert!(
        f.execute(
            "plan.propose",
            json!({
                "task_ids": [parent_id],
                "horizon_start": "2026-10-25T00:00:00Z",
                "horizon_end": "2026-10-26T00:00:00Z",
                "move_unlocked": true
            })
        )
        .is_err()
    );
}

#[test]
fn due_strengths_are_distinct_and_ambiguous_rejected() {
    let mut f = Fixture::new();
    let soft = f
        .execute(
            "task.create",
            json!({"task": {
                "title": "软期限",
                "due": {"date": "2026-10-30", "strength": "soft"},
                "not_before": "2026-10-25T00:00:00Z",
                "preferred_window": {"start": "2026-10-26T01:00:00Z", "end": "2026-10-26T03:00:00Z"}
            }}),
        )
        .unwrap();
    let soft_id = soft.data["changes"][0]["id"].as_str().unwrap().to_string();
    let hard = f
        .execute(
            "task.create",
            json!({"task": {
                "title": "硬期限",
                "due": {"at": "2026-10-29T10:00:00Z", "strength": "hard"}
            }}),
        )
        .unwrap();
    let hard_id = hard.data["changes"][0]["id"].as_str().unwrap().to_string();
    let ambiguous = f.execute(
        "task.create",
        json!({"task": {
            "title": "两边都有",
            "due": {"date": "2026-10-30", "at": "2026-10-30T10:00:00Z", "strength": "soft"}
        }}),
    );
    assert!(ambiguous.is_err());
    let world = f.world();
    let soft_task = world.task(&soft_id).unwrap();
    assert_eq!(
        soft_task.target_due.map(|date| date.to_string()).as_deref(),
        Some("2026-10-30")
    );
    assert!(soft_task.hard_deadline.is_none());
    assert!(soft_task.not_before.is_some());
    assert!(soft_task.preferred_window.is_some());
    let hard_task = world.task(&hard_id).unwrap();
    assert_eq!(
        hard_task.hard_deadline.unwrap().to_rfc3339(),
        "2026-10-29T10:00:00+00:00"
    );
    assert!(hard_task.target_due.is_none());
}

#[test]
fn allocations_respect_remaining_minutes_and_capacity() {
    let mut f = Fixture::new();
    let task = f
        .execute(
            "task.create",
            json!({"task": {"title": "分配边界", "estimate_minutes": 60}}),
        )
        .unwrap();
    let task_id = task.data["changes"][0]["id"].as_str().unwrap().to_string();
    let capacity = f
        .execute(
            "capacity.create",
            json!({"capacity": {
                "title": "分配容量",
                "start": "2026-10-25T01:00:00Z",
                "end": "2026-10-25T03:00:00Z",
                "capacity_minutes": 120
            }}),
        )
        .unwrap();
    let capacity_id = capacity.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.execute(
        "allocation.create",
        json!({"allocation": {
            "task_id": task_id, "capacity_block_id": capacity_id,
            "start": "2026-10-25T01:00:00Z", "end": "2026-10-25T01:40:00Z", "minutes": 40
        }}),
    )
    .unwrap();
    let over_remaining = f.execute(
        "allocation.create",
        json!({"allocation": {
            "task_id": task_id, "capacity_block_id": capacity_id,
            "start": "2026-10-25T01:40:00Z", "end": "2026-10-25T02:10:00Z", "minutes": 30
        }}),
    );
    assert!(over_remaining.is_err());
    f.execute(
        "allocation.create",
        json!({"allocation": {
            "task_id": task_id, "capacity_block_id": capacity_id,
            "start": "2026-10-25T01:40:00Z", "end": "2026-10-25T02:00:00Z", "minutes": 20
        }}),
    )
    .unwrap();
    let world = f.world();
    let allocated: u32 = world
        .allocations
        .iter()
        .filter(|row| row.task_id == task_id)
        .map(|row| row.minutes)
        .sum();
    assert_eq!(allocated, 60);
    let capacity_used: u32 = world
        .allocations
        .iter()
        .filter(|row| row.capacity_block_id.as_deref() == Some(capacity_id.as_str()))
        .map(|row| row.minutes)
        .sum();
    assert_eq!(capacity_used, 60);
    assert!(
        world
            .allocations
            .iter()
            .any(|row| row.task_id == task_id && row.minutes == 40)
    );
}

#[test]
fn outbox_retry_backoff_and_terminal_failure_are_durable() {
    use chrono::Duration;
    let mut f = Fixture::new();
    let fire_at = f.clock.now() + Duration::minutes(1);
    f.execute(
        "reminder.create",
        json!({"message": "出箱重试", "fire_at": fire_at, "dedupe_key": "outbox-retry"}),
    )
    .unwrap();
    let effects = f
        .store
        .pending_effects(fire_at + Duration::seconds(1), 10)
        .unwrap();
    assert_eq!(effects.len(), 1);
    let effect = &effects[0];
    assert_eq!(effect.effect_type, "reminder.deliver");
    let future = fire_at + Duration::seconds(31);
    f.store
        .update_effect(
            &effect.id,
            "pending",
            4,
            Some("transient toast failure"),
            future,
        )
        .unwrap();
    assert!(
        f.store
            .pending_effects(fire_at + Duration::seconds(2), 10)
            .unwrap()
            .is_empty()
    );
    assert_eq!(f.store.pending_effects(future, 10).unwrap().len(), 1);
    f.store
        .update_effect(&effect.id, "failed", 5, Some("terminal failure"), future)
        .unwrap();
    assert!(f.store.pending_effects(future, 10).unwrap().is_empty());
}

#[test]
fn progress_facts_are_reported_immutable_and_review_derives_from_them() {
    let mut f = Fixture::new();
    let task = f
        .execute(
            "task.create",
            json!({"task": {"title": "执行事实", "estimate_minutes": 120}}),
        )
        .unwrap();
    let task_id = task.data["changes"][0]["id"].as_str().unwrap().to_string();
    f.execute(
        "progress.record",
        json!({
            "task_id": task_id, "minutes": 30, "remaining_minutes": 90,
            "note": "第一段", "dedupe_key": "progress-a"
        }),
    )
    .unwrap();
    f.execute(
        "progress.record",
        json!({
            "task_id": task_id, "minutes": 20, "remaining_minutes": 70,
            "note": "第二段", "dedupe_key": "progress-b"
        }),
    )
    .unwrap();
    let replay = f.execute(
        "progress.record",
        json!({
            "task_id": task_id, "minutes": 20, "remaining_minutes": 70,
            "note": "第二段", "dedupe_key": "progress-b"
        }),
    );
    assert!(replay.is_err());
    let world = f.world();
    let task = world.task(&task_id).unwrap();
    assert_eq!(task.actual_minutes, 50);
    assert_eq!(task.remaining_minutes, Some(70));
    assert_eq!(task.status, zodo_kernel::model::TaskStatus::InProgress);
    let progress: Vec<_> = world
        .activities
        .iter()
        .filter(|row| row.kind == zodo_kernel::model::ActivityKind::Progress)
        .collect();
    assert_eq!(progress.len(), 2);
    assert_eq!(progress[0].source.actor_id.as_deref(), Some("user_ui"));
    let review = f.query(
        "stats.get",
        json!({"start": "2026-10-24", "end": "2026-10-24"}),
    );
    assert_eq!(review["facts"]["actual_minutes"], 50);
}

#[test]
fn planning_is_deterministic_and_unrelated_changes_do_not_reorder() {
    let mut f = Fixture::new();
    let first = f
        .execute(
            "task.create",
            json!({"task": {"title": "同等任务 A", "estimate_minutes": 30}}),
        )
        .unwrap();
    let second = f
        .execute(
            "task.create",
            json!({"task": {"title": "同等任务 B", "estimate_minutes": 30}}),
        )
        .unwrap();
    let unrelated = f
        .execute(
            "task.create",
            json!({"task": {"title": "无关任务", "estimate_minutes": 10}}),
        )
        .unwrap();
    let ids = vec![
        first.data["changes"][0]["id"].as_str().unwrap().to_string(),
        second.data["changes"][0]["id"]
            .as_str()
            .unwrap()
            .to_string(),
    ];
    f.execute(
        "capacity.create",
        json!({"capacity": {
            "title": "确定性容量", "start": "2026-10-25T01:00:00Z",
            "end": "2026-10-25T05:00:00Z", "capacity_minutes": 240
        }}),
    )
    .unwrap();
    let plan_once = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": ids, "horizon_start": "2026-10-25T00:00:00Z",
                "horizon_end": "2026-10-26T00:00:00Z", "move_unlocked": true
            }),
        )
        .unwrap();
    let assignments_once = plan_once.data["plan"]["assignments"].clone();
    let plan_twice = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": ids, "horizon_start": "2026-10-25T00:00:00Z",
                "horizon_end": "2026-10-26T00:00:00Z", "move_unlocked": true
            }),
        )
        .unwrap();
    assert_eq!(assignments_once, plan_twice.data["plan"]["assignments"]);
    let unrelated_id = unrelated.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.execute(
        "task.update",
        json!({"id": unrelated_id, "task": {"title": "无关任务改名"}, "expected_version": 1}),
    )
    .unwrap();
    let plan_after = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": ids, "horizon_start": "2026-10-25T00:00:00Z",
                "horizon_end": "2026-10-26T00:00:00Z", "move_unlocked": true
            }),
        )
        .unwrap();
    assert_eq!(assignments_once, plan_after.data["plan"]["assignments"]);
}

#[test]
fn review_builds_facts_without_model() {
    let mut f = Fixture::new();
    let task = f
        .execute(
            "task.create",
            json!({"task": {"title": "回顾事实任务", "estimate_minutes": 60}}),
        )
        .unwrap();
    let task_id = task.data["changes"][0]["id"].as_str().unwrap().to_string();
    f.execute(
        "progress.record",
        json!({
            "task_id": task_id, "minutes": 25, "remaining_minutes": 35,
            "note": "回顾事实", "dedupe_key": "review-progress"
        }),
    )
    .unwrap();
    let built = f
        .execute(
            "review.build",
            json!({
                "period_start": "2026-10-24", "period_end": "2026-10-24",
                "notes": ["人工回顾"]
            }),
        )
        .unwrap();
    let review = &built.data["review"];
    assert_eq!(review["facts"]["actual_minutes"], 25);
    assert_eq!(review["facts"]["tasks_opened"], 1);
    assert_eq!(review["facts"]["tasks_completed"], 0);
    assert_eq!(review["notes"][0], "人工回顾");
    assert!(review["recommendations"].is_array());
    assert_eq!(f.world().reviews.len(), 1);
}

#[test]
fn note_attachment_metadata_hash_and_no_binary_in_task() {
    let mut f = Fixture::new();
    let task = f
        .execute(
            "task.create",
            json!({"task": {"title": "附件任务", "estimate_minutes": 10}}),
        )
        .unwrap();
    let task_id = task.data["changes"][0]["id"].as_str().unwrap().to_string();
    let note = f
        .execute(
            "note.create",
            json!({"note": {
                "title": "附件笔记", "body": "引用附件",
                "linked_tasks": [task_id], "data_class": "device_only"
            }}),
        )
        .unwrap();
    let note_id = note.data["changes"][0]["id"].as_str().unwrap().to_string();
    let hash = "a".repeat(64);
    let added = f
        .execute(
            "attachment.add",
            json!({
                "note_id": note_id, "location": "attachments/contract.pdf",
                "media_type": "application/pdf", "data_class": "device_only",
                "sha256": hash, "summary": "合同扫描件"
            }),
        )
        .unwrap();
    let attachment = &added.data["attachment"];
    assert_eq!(attachment["note_id"], note_id);
    assert_eq!(attachment["sha256"], hash);
    assert!(
        f.execute(
            "attachment.add",
            json!({
                "note_id": note_id, "location": "x", "media_type": "text/plain",
                "data_class": "device_only", "sha256": "not-a-hash"
            })
        )
        .is_err()
    );
    let world = f.world();
    let note = world.notes.iter().find(|note| note.id == note_id).unwrap();
    assert!(note.linked_tasks.contains(&task_id));
    let task_value = serde_json::to_string(world.task(&task_id).unwrap()).unwrap();
    assert!(!task_value.contains("attachments"));
    assert!(!task_value.contains(&hash));
    let attachment_id = attachment["id"].as_str().unwrap().to_string();
    f.execute("attachment.remove", json!({"id": attachment_id}))
        .unwrap();
    assert!(
        f.world()
            .attachments
            .iter()
            .all(|attachment| attachment.id != attachment_id)
    );
}

#[test]
fn reminder_intent_cancel_and_delivery_are_durable() {
    use chrono::Duration;
    use zodo_kernel::model::ReminderStatus;
    let mut f = Fixture::new();
    let fire_at = f.clock.now() + Duration::hours(1);
    let _created = f
        .execute(
            "reminder.create",
            json!({
                "message": "提醒取消测试", "fire_at": fire_at, "dedupe_key": "reminder-cancel"
            }),
        )
        .unwrap();
    let reminder_id = f
        .world()
        .reminders
        .iter()
        .find(|row| row.dedupe_key.as_deref() == Some("reminder-cancel"))
        .unwrap()
        .id
        .clone();
    assert_eq!(
        f.world()
            .reminders
            .iter()
            .find(|row| row.id == reminder_id)
            .unwrap()
            .status,
        ReminderStatus::Pending
    );
    f.execute("reminder.cancel", json!({"id": reminder_id}))
        .unwrap();
    let reminder = f
        .world()
        .reminders
        .iter()
        .find(|row| row.id == reminder_id)
        .unwrap()
        .clone();
    assert_eq!(reminder.status, ReminderStatus::Cancelled);

    let delivered = f
        .execute(
            "reminder.create",
            json!({
                "message": "提醒送达测试", "fire_at": fire_at
            }),
        )
        .unwrap();
    let _ = delivered;
    let delivered_id = f
        .world()
        .reminders
        .iter()
        .find(|row| row.message == "提醒送达测试")
        .unwrap()
        .id
        .clone();
    f.execute(
        "reminder.mark_delivered",
        json!({"id": delivered_id, "delivered_at": fire_at}),
    )
    .unwrap();
    let reminder = f
        .world()
        .reminders
        .iter()
        .find(|row| row.id == delivered_id)
        .unwrap()
        .clone();
    assert_eq!(reminder.status, ReminderStatus::Delivered);
    assert!(reminder.delivered_at.is_some());
}

#[test]
fn locked_and_past_capacity_is_immutable() {
    let mut f = Fixture::new();
    let locked = f
        .execute(
            "capacity.create",
            json!({"capacity": {
                "title": "锁定容量", "start": "2026-10-25T01:00:00Z",
                "end": "2026-10-25T03:00:00Z", "capacity_minutes": 120, "locked": true
            }}),
        )
        .unwrap();
    let locked_id = locked.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        f.execute(
            "capacity.update",
            json!({"id": locked_id, "capacity": {
                "title": "改锁定", "start": "2026-10-25T01:00:00Z",
                "end": "2026-10-25T03:00:00Z", "capacity_minutes": 60
            }})
        )
        .is_err()
    );
    assert!(
        f.execute("capacity.remove", json!({"id": locked_id}))
            .is_err()
    );

    let past = f
        .execute(
            "capacity.create",
            json!({"capacity": {
                "title": "过去容量", "start": "2026-10-23T01:00:00Z",
                "end": "2026-10-23T03:00:00Z", "capacity_minutes": 120
            }}),
        )
        .unwrap();
    let past_id = past.data["changes"][0]["id"].as_str().unwrap().to_string();
    let task = f
        .execute(
            "task.create",
            json!({"task": {"title": "过去任务", "estimate_minutes": 30}}),
        )
        .unwrap();
    let task_id = task.data["changes"][0]["id"].as_str().unwrap().to_string();
    f.execute(
        "allocation.create",
        json!({"allocation": {
            "task_id": task_id, "capacity_block_id": past_id,
            "start": "2026-10-23T01:00:00Z", "end": "2026-10-23T01:30:00Z", "minutes": 30
        }}),
    )
    .unwrap();
    let update_past = f.execute(
        "capacity.update",
        json!({"id": past_id, "capacity": {
            "title": "过去容量改动", "start": "2026-10-23T01:00:00Z",
            "end": "2026-10-23T03:00:00Z", "capacity_minutes": 120
        }}),
    );
    assert!(update_past.is_err());
}

#[test]
fn iana_timezone_validation_and_dst_deadline_boundaries() {
    let mut f = Fixture::new();
    let settings = |timezone: &str| {
        json!({
            "timezone": timezone, "week_start": 1, "weekend_policy": "ask",
            "work_windows": [], "default_estimate_minutes": 30, "daily_limit_minutes": null
        })
    };
    assert!(
        f.execute(
            "workspace.update",
            json!({"settings": settings("Not/AZone"), "policy": null})
        )
        .is_err()
    );
    f.execute(
        "workspace.update",
        json!({"settings": settings("America/New_York"), "policy": null}),
    )
    .unwrap();
    let spring = f
        .execute(
            "task.create",
            json!({"task": {
                "title": "DST 春季", "due": {"date": "2026-03-08", "strength": "hard"}
            }}),
        )
        .unwrap();
    let spring_id = spring.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let fall = f
        .execute(
            "task.create",
            json!({"task": {
                "title": "DST 秋季", "due": {"date": "2026-11-01", "strength": "hard"}
            }}),
        )
        .unwrap();
    let fall_id = fall.data["changes"][0]["id"].as_str().unwrap().to_string();
    let world = f.world();
    assert_eq!(
        world
            .task(&spring_id)
            .unwrap()
            .hard_deadline
            .unwrap()
            .to_rfc3339(),
        "2026-03-09T03:59:59+00:00"
    );
    assert_eq!(
        world
            .task(&fall_id)
            .unwrap()
            .hard_deadline
            .unwrap()
            .to_rfc3339(),
        "2026-11-02T04:59:59+00:00"
    );
}

#[test]
fn work_windows_and_weekend_policy_are_hard_constraints() {
    let mut f = Fixture::new();
    let settings = |windows: serde_json::Value, weekend: &str| {
        json!({
            "timezone": "Asia/Shanghai", "week_start": 1, "weekend_policy": weekend,
            "work_windows": windows, "default_estimate_minutes": 30, "daily_limit_minutes": null
        })
    };
    let task = f
        .execute(
            "task.create",
            json!({"task": {"title": "工作窗口任务", "estimate_minutes": 30}}),
        )
        .unwrap();
    let task_id = task.data["changes"][0]["id"].as_str().unwrap().to_string();
    // Monday 2026-10-26 is a workday; Tuesday 2026-10-27 is outside the hard Monday window.
    f.execute(
        "workspace.update",
        json!({"settings": settings(json!([
        {"weekday": 1, "start_minute": 540, "end_minute": 720, "hard": true}
    ]), "allow"), "policy": null}),
    )
    .unwrap();
    let tuesday = f
        .execute(
            "capacity.create",
            json!({"capacity": {
                "title": "周二容量", "start": "2026-10-27T02:00:00Z",
                "end": "2026-10-27T04:00:00Z", "capacity_minutes": 120
            }}),
        )
        .unwrap();
    let _ = tuesday;
    let planned = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id], "horizon_start": "2026-10-26T00:00:00Z",
                "horizon_end": "2026-10-28T00:00:00Z", "move_unlocked": true
            }),
        )
        .unwrap();
    assert!(
        planned.data["plan"]["assignments"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(planned.data["plan"]["unplaced"].as_array().unwrap().len() > 0);

    // Soft-only windows are preferences, not exclusions.
    f.execute(
        "workspace.update",
        json!({"settings": settings(json!([
        {"weekday": 1, "start_minute": 540, "end_minute": 720, "hard": false}
    ]), "allow"), "policy": null}),
    )
    .unwrap();
    let soft = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id], "horizon_start": "2026-10-26T00:00:00Z",
                "horizon_end": "2026-10-28T00:00:00Z", "move_unlocked": true
            }),
        )
        .unwrap();
    assert!(
        !soft.data["plan"]["assignments"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    // Weekend Deny excludes Sunday capacity; Monday capacity remains usable.
    f.execute(
        "workspace.update",
        json!({"settings": settings(json!([]), "deny"), "policy": null}),
    )
    .unwrap();
    f.execute(
        "capacity.create",
        json!({"capacity": {
            "title": "周日容量", "start": "2026-11-01T02:00:00Z",
            "end": "2026-11-01T04:00:00Z", "capacity_minutes": 120
        }}),
    )
    .unwrap();
    let weekend = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id], "horizon_start": "2026-10-31T00:00:00Z",
                "horizon_end": "2026-11-02T00:00:00Z", "move_unlocked": true
            }),
        )
        .unwrap();
    assert!(
        weekend.data["plan"]["assignments"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn search_pagination_filter_and_privacy_are_consistent() {
    let mut f = Fixture::new();
    for index in 0..25 {
        f.execute(
            "task.create",
            json!({"task": {
                "title": format!("分页检索任务 {index:02}"), "estimate_minutes": 10
            }}),
        )
        .unwrap();
    }
    f.execute("note.create", json!({"note": {
        "title": "分页秘密笔记", "body": "分页 device only", "data_class": "device_only", "linked_tasks": []
    }})).unwrap();
    f.execute("note.create", json!({"note": {
        "title": "分页公开笔记", "body": "分页 cloud allowed", "data_class": "cloud_allowed", "linked_tasks": []
    }})).unwrap();
    let first = f.query(
        "search.query",
        json!({"text": "分页", "limit": 10, "offset": 0}),
    );
    let first_results = first["results"].as_array().unwrap();
    assert_eq!(first_results.len(), 10);
    assert_eq!(first["total"], 26);
    assert_eq!(first["has_more"], true);
    assert_eq!(first["next_offset"], 10);
    let second = f.query(
        "search.query",
        json!({"text": "分页", "limit": 10, "offset": 10}),
    );
    let second_results = second["results"].as_array().unwrap();
    assert_eq!(second_results.len(), 10);
    for row in first_results {
        assert!(!second_results.iter().any(|other| other["id"] == row["id"]));
    }
    let last = f.query(
        "search.query",
        json!({"text": "分页", "limit": 10, "offset": 20}),
    );
    assert_eq!(last["results"].as_array().unwrap().len(), 6);
    assert_eq!(last["has_more"], false);
    assert!(
        last["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["kind"] != "note" || row["title"] == "分页公开笔记")
    );
    assert!(
        first_results
            .iter()
            .all(|row| row["title"] != "分页秘密笔记")
    );
}

#[test]
fn overlapping_busy_events_are_unioned_not_double_counted() {
    let mut f = Fixture::new();
    let task = f
        .execute(
            "task.create",
            json!({"task": {"title": "并集容量任务", "estimate_minutes": 240}}),
        )
        .unwrap();
    let task_id = task.data["changes"][0]["id"].as_str().unwrap().to_string();
    f.execute(
        "capacity.create",
        json!({"capacity": {
            "title": "并集容量", "start": "2026-10-26T01:00:00Z",
            "end": "2026-10-26T05:00:00Z", "capacity_minutes": 240
        }}),
    )
    .unwrap();
    f.execute("calendar.import_busy", json!({
        "source_id": null, "provider": "manual", "display_name": "重叠忙碌",
        "external_id": "overlap-union", "replace_source": true,
        "events": [
            {"external_id": "busy-a", "title": "会议 A", "start": "2026-10-26T02:00:00Z", "end": "2026-10-26T03:00:00Z"},
            {"external_id": "busy-b", "title": "会议 B", "start": "2026-10-26T02:30:00Z", "end": "2026-10-26T03:30:00Z"}
        ]
    })).unwrap();
    let planned = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id], "horizon_start": "2026-10-26T00:00:00Z",
                "horizon_end": "2026-10-27T00:00:00Z", "move_unlocked": true
            }),
        )
        .unwrap();
    let assigned: u32 = planned.data["plan"]["assignments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["minutes"].as_u64().unwrap() as u32)
        .sum();
    let unplaced: u32 = planned.data["plan"]["unplaced"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["minutes"].as_u64().unwrap() as u32)
        .sum();
    assert_eq!(assigned, 150);
    assert_eq!(unplaced, 90);
}

#[test]
fn flow_f01_first_capture_to_first_completed_transaction() {
    let mut f = Fixture::new();
    let captured = f
        .execute(
            "capture.collect",
            json!({"text": "第一个事务：联系供应商", "source_kind": "manual"}),
        )
        .unwrap();
    let entry_id = captured.data["inbox_entry"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let converted = f
        .execute(
            "capture.convert",
            json!({
                "id": entry_id,
                "tasks": [{"title": "联系供应商", "estimate_minutes": 15, "priority": 2}],
                "goal": null, "project": null
            }),
        )
        .unwrap();
    let task_id = converted.data["task_ids"][0].as_str().unwrap().to_string();
    let completed = f
        .execute("task.complete", json!({"id": task_id, "note": "已联系"}))
        .unwrap();
    assert_eq!(
        completed.status,
        zodo_kernel::commands::ResultStatus::Committed
    );
    let world = f.world();
    let task = world.task(&task_id).unwrap();
    assert_eq!(task.status, zodo_kernel::model::TaskStatus::Done);
    let entry = world.inbox.iter().find(|row| row.id == entry_id).unwrap();
    assert_eq!(entry.status, zodo_kernel::model::InboxStatus::Processed);
    assert!(
        f.execute(
            "capture.convert",
            json!({
                "id": "inb_missing", "tasks": [{"title": "x"}], "goal": null, "project": null
            })
        )
        .is_err()
    );
}

#[test]
fn flow_f02_fragmented_material_becomes_multiple_grounded_tasks() {
    let mut f = Fixture::new();
    let captured = f
        .execute(
            "capture.collect",
            json!({"text": "作品集：选 3 个项目；写案例；做线上展示"}),
        )
        .unwrap();
    let entry_id = captured.data["inbox_entry"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.execute(
        "capture.clarify",
        json!({"id": entry_id, "question": "截止日期？", "answer": "10 月底"}),
    )
    .unwrap();
    let converted = f.execute("capture.convert", json!({
        "id": entry_id,
        "tasks": [
            {"title": "选择 3 个项目", "estimate_minutes": 60, "priority": 1, "target_due": "2026-10-28"},
            {"title": "写项目案例", "estimate_minutes": 180, "priority": 1, "target_due": "2026-10-30"},
            {"title": "搭建线上展示", "estimate_minutes": 120, "priority": 2, "target_due": "2026-10-31"}
        ],
        "goal": null, "project": null
    })).unwrap();
    let task_ids = converted.data["task_ids"].as_array().unwrap();
    assert_eq!(task_ids.len(), 3);
    let world = f.world();
    assert_eq!(world.tasks.len(), 3);
    assert!(
        world
            .tasks
            .iter()
            .all(|task| task.status == zodo_kernel::model::TaskStatus::Open)
    );
    let entry = world.inbox.iter().find(|row| row.id == entry_id).unwrap();
    assert_eq!(entry.status, zodo_kernel::model::InboxStatus::Processed);
    assert!(entry.clarifications.len() >= 1);
}

#[test]
fn flow_f03_goal_to_project_to_committed_plan() {
    let mut f = Fixture::new();
    let goal = f.execute("goal.create", json!({"goal": {
        "title": "作品集上线", "success_criteria": "3 个项目上线", "target_due": "2026-11-01"
    }})).unwrap();
    let goal_id = goal.data["changes"][0]["id"].as_str().unwrap().to_string();
    let project = f
        .execute(
            "project.create",
            json!({"project": {
                "title": "作品集项目", "goal_id": goal_id
            }}),
        )
        .unwrap();
    let project_id = project.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let task = f
        .execute(
            "task.create",
            json!({"task": {
                "title": "搭建作品集页面", "project_id": project_id, "estimate_minutes": 60
            }}),
        )
        .unwrap();
    let task_id = task.data["changes"][0]["id"].as_str().unwrap().to_string();
    f.execute(
        "capacity.create",
        json!({"capacity": {
            "title": "作品集容量", "start": "2026-10-26T01:00:00Z",
            "end": "2026-10-26T04:00:00Z", "capacity_minutes": 120
        }}),
    )
    .unwrap();
    let planned = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id], "horizon_start": "2026-10-26T00:00:00Z",
                "horizon_end": "2026-10-27T00:00:00Z", "move_unlocked": true
            }),
        )
        .unwrap();
    assert_eq!(
        planned.status,
        zodo_kernel::commands::ResultStatus::Validated
    );
    let proposal_id = planned.data["proposal"]["id"].as_str().unwrap().to_string();
    let approved = f
        .execute("proposal.approve", json!({"proposal_id": proposal_id}))
        .unwrap();
    assert_eq!(
        approved.status,
        zodo_kernel::commands::ResultStatus::Approved
    );
    let grant = approved.data["approval"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let committed = f
        .execute(
            "changes.commit",
            json!({
                "proposal_id": proposal_id, "approval_grant": grant
            }),
        )
        .unwrap();
    assert_eq!(
        committed.status,
        zodo_kernel::commands::ResultStatus::Committed
    );
    let world = f.world();
    assert!(world.allocations.iter().any(|row| row.task_id == task_id));
    assert_eq!(
        world.project(&project_id).unwrap().goal_id.as_deref(),
        Some(goal_id.as_str())
    );
    assert_eq!(
        world.task(&task_id).unwrap().project_id.as_deref(),
        Some(project_id.as_str())
    );
    assert_eq!(
        world.goal(&goal_id).unwrap().status,
        zodo_kernel::model::GoalStatus::Active
    );
    // Infeasible horizon returns unplaced diagnostics and never commits an allocation.
    let infeasible = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id], "horizon_start": "2026-12-01T00:00:00Z",
                "horizon_end": "2026-12-02T00:00:00Z", "move_unlocked": true
            }),
        )
        .unwrap();
    assert_eq!(
        infeasible.status,
        zodo_kernel::commands::ResultStatus::Validated
    );
    assert!(
        !infeasible.data["plan"]["unplaced"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn flow_f04_schedule_change_replans_locally_and_keeps_locked_work() {
    let mut f = Fixture::new();
    let a = f
        .execute(
            "task.create",
            json!({"task": {"title": "锁定工作 A", "estimate_minutes": 30}}),
        )
        .unwrap();
    let a_id = a.data["changes"][0]["id"].as_str().unwrap().to_string();
    let b = f
        .execute(
            "task.create",
            json!({"task": {"title": "可重排工作 B", "estimate_minutes": 60}}),
        )
        .unwrap();
    let b_id = b.data["changes"][0]["id"].as_str().unwrap().to_string();
    let capacity = f
        .execute(
            "capacity.create",
            json!({"capacity": {
                "title": "重排容量", "start": "2026-10-26T01:00:00Z",
                "end": "2026-10-26T04:00:00Z", "capacity_minutes": 180
            }}),
        )
        .unwrap();
    let capacity_id = capacity.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.execute(
        "allocation.create",
        json!({"allocation": {
            "task_id": a_id, "capacity_block_id": capacity_id,
            "start": "2026-10-26T01:00:00Z", "end": "2026-10-26T01:30:00Z",
            "minutes": 30, "locked": true
        }}),
    )
    .unwrap();
    f.execute(
        "allocation.create",
        json!({"allocation": {
            "task_id": b_id, "capacity_block_id": capacity_id,
            "start": "2026-10-26T02:00:00Z", "end": "2026-10-26T03:00:00Z",
            "minutes": 60, "locked": false
        }}),
    )
    .unwrap();
    f.execute(
        "commitment.create",
        json!({"commitment": {
            "title": "临时会议", "start": "2026-10-26T02:00:00Z",
            "end": "2026-10-26T02:30:00Z"
        }}),
    )
    .unwrap();
    let planned = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": [a_id, b_id], "horizon_start": "2026-10-26T00:00:00Z",
                "horizon_end": "2026-10-27T00:00:00Z", "move_unlocked": true
            }),
        )
        .unwrap();
    let proposal_id = planned.data["proposal"]["id"].as_str().unwrap().to_string();
    let approved = f
        .execute("proposal.approve", json!({"proposal_id": proposal_id}))
        .unwrap();
    let grant = approved.data["approval"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.execute(
        "changes.commit",
        json!({"proposal_id": proposal_id, "approval_grant": grant}),
    )
    .unwrap();
    let world = f.world();
    let a_allocations: Vec<_> = world
        .allocations
        .iter()
        .filter(|row| row.task_id == a_id)
        .collect();
    assert_eq!(a_allocations.len(), 1);
    assert!(a_allocations[0].locked);
    let a_start: chrono::DateTime<chrono::Utc> = "2026-10-26T01:00:00Z".parse().unwrap();
    assert_eq!(a_allocations[0].start, a_start);
    let b_allocations: Vec<_> = world
        .allocations
        .iter()
        .filter(|row| row.task_id == b_id)
        .collect();
    let b_minutes: u32 = b_allocations.iter().map(|row| row.minutes).sum();
    assert_eq!(b_minutes, 60);
    let meeting_start: chrono::DateTime<chrono::Utc> = "2026-10-26T02:00:00Z".parse().unwrap();
    let meeting_end: chrono::DateTime<chrono::Utc> = "2026-10-26T02:30:00Z".parse().unwrap();
    for row in &b_allocations {
        assert!(!(row.start < meeting_end && row.end > meeting_start));
    }
}

#[test]
fn flow_f05_execute_progress_review_then_replan_remaining() {
    let mut f = Fixture::new();
    let task = f
        .execute(
            "task.create",
            json!({"task": {"title": "反馈循环任务", "estimate_minutes": 60}}),
        )
        .unwrap();
    let task_id = task.data["changes"][0]["id"].as_str().unwrap().to_string();
    f.execute(
        "capacity.create",
        json!({"capacity": {
            "title": "反馈循环容量", "start": "2026-10-26T01:00:00Z",
            "end": "2026-10-26T05:00:00Z", "capacity_minutes": 240
        }}),
    )
    .unwrap();
    let first = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id], "horizon_start": "2026-10-26T00:00:00Z",
                "horizon_end": "2026-10-27T00:00:00Z", "move_unlocked": true
            }),
        )
        .unwrap();
    let first_id = first.data["proposal"]["id"].as_str().unwrap().to_string();
    let approved = f
        .execute("proposal.approve", json!({"proposal_id": first_id}))
        .unwrap();
    let grant = approved.data["approval"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.execute(
        "changes.commit",
        json!({"proposal_id": first_id, "approval_grant": grant}),
    )
    .unwrap();
    f.execute(
        "progress.record",
        json!({
            "task_id": task_id, "minutes": 30, "remaining_minutes": 30, "note": "完成一半"
        }),
    )
    .unwrap();
    let review = f
        .execute(
            "review.build",
            json!({"period_start": "2026-10-24", "period_end": "2026-10-24", "notes": null}),
        )
        .unwrap();
    assert_eq!(review.data["review"]["facts"]["actual_minutes"], 30);
    let world = f.world();
    let task = world.task(&task_id).unwrap();
    assert_eq!(task.status, zodo_kernel::model::TaskStatus::InProgress);
    assert_eq!(task.remaining_minutes, Some(30));
    let second = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id], "horizon_start": "2026-10-26T00:00:00Z",
                "horizon_end": "2026-10-27T00:00:00Z", "move_unlocked": true
            }),
        )
        .unwrap();
    let second_id = second.data["proposal"]["id"].as_str().unwrap().to_string();
    let approved = f
        .execute("proposal.approve", json!({"proposal_id": second_id}))
        .unwrap();
    let grant = approved.data["approval"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.execute(
        "changes.commit",
        json!({"proposal_id": second_id, "approval_grant": grant}),
    )
    .unwrap();
    let world = f.world();
    let allocations: Vec<_> = world
        .allocations
        .iter()
        .filter(|row| row.task_id == task_id)
        .collect();
    assert_eq!(allocations.iter().map(|row| row.minutes).sum::<u32>(), 30);
    assert_eq!(
        world.task(&task_id).unwrap().status,
        zodo_kernel::model::TaskStatus::InProgress
    );
}

#[test]
fn flow_f06_recurring_tasks_and_habit_single_exception() {
    let mut f = Fixture::new();
    let habit = f
        .execute(
            "habit.create",
            json!({"habit": {
                "title": "F06 习惯", "timezone": "Asia/Shanghai", "start_date": "2026-10-20",
                "weekly_target": 3, "scheduled_weekdays": [1,3,5], "time_of_day": "08:00:00"
            }}),
        )
        .unwrap();
    let habit_id = habit.data["habit"]["id"].as_str().unwrap().to_string();
    f.execute(
        "habit.log",
        json!({
            "habit_id": habit_id, "occurred_at": "2026-10-24T00:00:00Z",
            "minutes": 20, "dedupe_key": "f06-habit-day"
        }),
    )
    .unwrap();
    assert!(
        f.execute(
            "habit.log",
            json!({
                "habit_id": habit_id, "minutes": 20, "dedupe_key": "f06-habit-day"
            })
        )
        .is_err()
    );
    assert_eq!(
        f.world()
            .activities
            .iter()
            .filter(|row| row.habit_id.as_deref() == Some(habit_id.as_str()))
            .count(),
        1
    );

    let created = f.execute("recurrence.create", json!({
        "rule": {
            "title": "F06 循环任务", "timezone": "Asia/Shanghai", "start_date": "2026-10-26",
            "frequency": "weekly", "interval": 1, "weekdays": [1,3], "estimate_minutes": 30
        },
        "through": "2026-11-02"
    })).unwrap();
    let rule_id = created.data["rule"]["id"].as_str().unwrap().to_string();
    let initial = f
        .world()
        .occurrences
        .iter()
        .filter(|row| row.rule_id == rule_id)
        .count();
    assert_eq!(initial, 3);
    let first = f
        .world()
        .occurrences
        .iter()
        .filter(|row| row.rule_id == rule_id)
        .min_by_key(|row| row.current_date)
        .unwrap()
        .clone();
    let first_task = first.task_id.clone().unwrap();
    f.execute(
        "recurrence.exception",
        json!({"occurrence_id": first.id, "action": "skip"}),
    )
    .unwrap();
    assert_eq!(
        f.world().task(&first_task).unwrap().status,
        zodo_kernel::model::TaskStatus::Cancelled
    );
    let second = f
        .world()
        .occurrences
        .iter()
        .filter(|row| {
            row.rule_id == rule_id && row.status == zodo_kernel::model::OccurrenceStatus::Generated
        })
        .min_by_key(|row| row.current_date)
        .unwrap()
        .clone();
    f.execute(
        "recurrence.exception",
        json!({"occurrence_id": second.id, "action": "move", "new_date": "2026-11-05"}),
    )
    .unwrap();
    let expanded_again = f
        .execute(
            "recurrence.expand",
            json!({"rule_id": rule_id, "through": "2026-11-02"}),
        )
        .unwrap();
    assert!(
        expanded_again.data["task_ids"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        f.world()
            .occurrences
            .iter()
            .filter(|row| row.rule_id == rule_id)
            .count(),
        3
    );
    assert_eq!(
        f.world()
            .occurrences
            .iter()
            .find(|row| row.id == second.id)
            .unwrap()
            .current_date,
        chrono::NaiveDate::from_ymd_opt(2026, 11, 5).unwrap()
    );
}

#[test]
fn flow_f07_weekly_review_adjustment_reversible_preference_and_next_plan() {
    let mut f = Fixture::new();
    let task = f
        .execute(
            "task.create",
            json!({"task": {"title": "F07 估算任务", "estimate_minutes": 60}}),
        )
        .unwrap();
    let task_id = task.data["changes"][0]["id"].as_str().unwrap().to_string();
    f.execute(
        "progress.record",
        json!({
            "task_id": task_id, "minutes": 30, "remaining_minutes": 30, "note": "投入超出估算"
        }),
    )
    .unwrap();
    let review = f
        .execute(
            "review.build",
            json!({"period_start": "2026-10-24", "period_end": "2026-10-24", "notes": ["周回顾"]}),
        )
        .unwrap();
    assert_eq!(review.data["review"]["facts"]["actual_minutes"], 30);

    let settings = json!({
        "timezone": "Asia/Shanghai", "week_start": 1, "weekend_policy": "allow",
        "work_windows": [], "default_estimate_minutes": 30, "daily_limit_minutes": 120
    });
    f.execute(
        "workspace.update",
        json!({"settings": settings, "policy": null}),
    )
    .unwrap();
    assert_eq!(f.world().workspace.settings.daily_limit_minutes, Some(120));
    let reverted = json!({
        "timezone": "Asia/Shanghai", "week_start": 1, "weekend_policy": "ask",
        "work_windows": [], "default_estimate_minutes": 30, "daily_limit_minutes": null
    });
    f.execute(
        "workspace.update",
        json!({"settings": reverted, "policy": null}),
    )
    .unwrap();
    assert_eq!(f.world().workspace.settings.daily_limit_minutes, None);

    f.execute(
        "capacity.create",
        json!({"capacity": {
            "title": "F07 下周容量", "start": "2026-10-27T01:00:00Z",
            "end": "2026-10-27T05:00:00Z", "capacity_minutes": 240
        }}),
    )
    .unwrap();
    let next_plan = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": [task_id], "horizon_start": "2026-10-27T00:00:00Z",
                "horizon_end": "2026-10-28T00:00:00Z", "move_unlocked": true
            }),
        )
        .unwrap();
    let plan_id = next_plan.data["proposal"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let approved = f
        .execute("proposal.approve", json!({"proposal_id": plan_id}))
        .unwrap();
    let grant = approved.data["approval"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.execute(
        "changes.commit",
        json!({"proposal_id": plan_id, "approval_grant": grant}),
    )
    .unwrap();
    assert!(
        f.world()
            .allocations
            .iter()
            .any(|row| row.task_id == task_id)
    );
}

#[test]
fn flow_ai01_multi_turn_clarification_never_writes_preview_facts() {
    let mut f = Fixture::new();
    let captured = f
        .execute(
            "capture.collect",
            json!({"text": "下个月办分享会，还没定形式"}),
        )
        .unwrap();
    let id = captured.data["inbox_entry"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let first = f
        .execute(
            "capture.clarify",
            json!({"id": id, "question": "线下还是线上？"}),
        )
        .unwrap();
    assert_eq!(
        first.data["inbox_entry"]["clarifications"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(f.world().tasks.len(), 0);
    let second = f
        .execute(
            "capture.clarify",
            json!({"id": id, "question": "预计多少人？", "answer": "线下，约 30 人"}),
        )
        .unwrap();
    assert_eq!(
        second.data["inbox_entry"]["clarifications"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(f.world().tasks.len(), 0);
    assert_eq!(f.world().goals.len(), 0);
    let converted = f
        .execute(
            "capture.convert",
            json!({
                "id": id,
                "tasks": [{"title": "确定分享会形式与人数", "estimate_minutes": 30}],
                "goal": null, "project": null
            }),
        )
        .unwrap();
    assert_eq!(converted.data["inbox_entry"]["status"], "processed");
    assert_eq!(f.world().tasks.len(), 1);
}

#[test]
fn project_milestones_crud_and_completion_are_independent() {
    let mut f = Fixture::new();
    let goal = f
        .execute("goal.create", json!({"goal": {"title": "里程碑目标"}}))
        .unwrap();
    let goal_id = goal.data["changes"][0]["id"].as_str().unwrap().to_string();
    let project = f
        .execute(
            "project.create",
            json!({"project": {"title": "里程碑项目", "goal_id": goal_id}}),
        )
        .unwrap();
    let project_id = project.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let task = f.execute("task.create", json!({"task": {"title": "项目任务", "project_id": project_id, "estimate_minutes": 15}})).unwrap();
    let task_id = task.data["changes"][0]["id"].as_str().unwrap().to_string();
    let added = f
        .execute(
            "milestone.add",
            json!({
                "project_id": project_id,
                "milestone": {"title": "第一个里程碑", "due": "2026-11-01"}
            }),
        )
        .unwrap();
    let milestone_id = added.data["milestone"]["id"].as_str().unwrap().to_string();
    assert_eq!(f.world().project(&project_id).unwrap().milestones.len(), 1);
    f.execute(
        "milestone.update",
        json!({
            "project_id": project_id, "milestone_id": milestone_id,
            "milestone": {"title": "第一个里程碑", "due": "2026-11-01", "done": true}
        }),
    )
    .unwrap();
    let world = f.world();
    let project = world.project(&project_id).unwrap();
    assert!(project.milestones[0].done);
    assert_eq!(project.status, zodo_kernel::model::ProjectStatus::Active);
    assert_eq!(
        world.task(&task_id).unwrap().status,
        zodo_kernel::model::TaskStatus::Open
    );
    f.execute(
        "milestone.remove",
        json!({"project_id": project_id, "milestone_id": milestone_id}),
    )
    .unwrap();
    assert!(
        f.world()
            .project(&project_id)
            .unwrap()
            .milestones
            .is_empty()
    );
}

#[test]
fn blocked_state_is_manual_or_dependency_derived_and_clearable() {
    let mut f = Fixture::new();
    let manual = f
        .execute(
            "task.create",
            json!({"task": {"title": "手动阻塞", "estimate_minutes": 15}}),
        )
        .unwrap();
    let manual_id = manual.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.execute(
        "task.set_blocked",
        json!({"id": manual_id, "blocked": true, "reason": "等待外部回复"}),
    )
    .unwrap();
    let task = f.world().task(&manual_id).unwrap().clone();
    assert!(task.blocked_manual);
    assert_eq!(task.blocked_reason.as_deref(), Some("等待外部回复"));
    f.execute(
        "task.set_blocked",
        json!({"id": manual_id, "blocked": false}),
    )
    .unwrap();
    let task = f.world().task(&manual_id).unwrap().clone();
    assert!(!task.blocked_manual);
    assert!(task.blocked_reason.is_none());

    let predecessor = f
        .execute(
            "task.create",
            json!({"task": {"title": "前置", "estimate_minutes": 15}}),
        )
        .unwrap();
    let predecessor_id = predecessor.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let successor = f
        .execute(
            "task.create",
            json!({"task": {"title": "后继", "estimate_minutes": 15}}),
        )
        .unwrap();
    let successor_id = successor.data["changes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.execute(
        "dependency.add",
        json!({"predecessor_id": predecessor_id, "successor_id": successor_id}),
    )
    .unwrap();
    f.execute(
        "capacity.create",
        json!({"capacity": {
            "title": "阻塞容量", "start": "2026-10-26T01:00:00Z",
            "end": "2026-10-26T04:00:00Z", "capacity_minutes": 120
        }}),
    )
    .unwrap();
    let blocked = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": [successor_id], "horizon_start": "2026-10-26T00:00:00Z",
                "horizon_end": "2026-10-27T00:00:00Z", "move_unlocked": true
            }),
        )
        .unwrap();
    assert!(
        blocked.data["plan"]["assignments"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        blocked.data["plan"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["code"] == "DEPENDENCY_BLOCKED")
    );
    f.execute("task.complete", json!({"id": predecessor_id}))
        .unwrap();
    let ready = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": [successor_id], "horizon_start": "2026-10-26T00:00:00Z",
                "horizon_end": "2026-10-27T00:00:00Z", "move_unlocked": true
            }),
        )
        .unwrap();
    assert!(
        !ready.data["plan"]["assignments"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn tags_attach_to_tasks_and_compose_filters() {
    let mut f = Fixture::new();
    f.execute(
        "tag.create",
        json!({"tag": {"name": "深度工作", "color": "#157E78"}}),
    )
    .unwrap();
    let a = f
        .execute(
            "task.create",
            json!({"task": {"title": "标签 A", "estimate_minutes": 10, "tags": ["深度工作"]}}),
        )
        .unwrap();
    let a_id = a.data["changes"][0]["id"].as_str().unwrap().to_string();
    let b = f
        .execute(
            "task.create",
            json!({"task": {"title": "标签 B", "estimate_minutes": 10}}),
        )
        .unwrap();
    let b_id = b.data["changes"][0]["id"].as_str().unwrap().to_string();
    f.execute(
        "task.set_tags",
        json!({"id": b_id, "tags": ["深度工作", "重要"]}),
    )
    .unwrap();
    assert_eq!(
        f.world().task(&b_id).unwrap().tags,
        vec!["深度工作".to_string(), "重要".to_string()]
    );
    let filtered = f.query(
        "tasks.list",
        json!({"filter": {"tag": "深度工作", "status": "open", "limit": 20}}),
    );
    let rows = filtered["tasks"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let important = f.query(
        "tasks.list",
        json!({"filter": {"tag": "重要", "limit": 20}}),
    );
    assert_eq!(important["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(important["tasks"][0]["task"]["id"], b_id);
    assert!(rows.iter().all(|row| {
        row["task"]["tags"]
            .as_array()
            .unwrap()
            .contains(&json!("深度工作"))
    }));
    let _ = a_id;
}

#[test]
fn review_stats_include_habit_adherence_and_evidence() {
    let mut f = Fixture::new();
    let habit = f
        .execute(
            "habit.create",
            json!({"habit": {
                "title": "周目标习惯", "timezone": "Asia/Shanghai", "start_date": "2026-10-19",
                "weekly_target": 5, "scheduled_weekdays": [1,2,3,4,5], "time_of_day": "08:00:00"
            }}),
        )
        .unwrap();
    let habit_id = habit.data["habit"]["id"].as_str().unwrap().to_string();
    for (day, key) in [(19, "ad-1"), (20, "ad-2"), (21, "ad-3")] {
        f.execute(
            "habit.log",
            json!({
                "habit_id": habit_id,
                "occurred_at": format!("2026-10-{day:02}T00:30:00Z"),
                "minutes": 20,
                "dedupe_key": key
            }),
        )
        .unwrap();
    }
    let review = f
        .execute(
            "review.build",
            json!({
                "period_start": "2026-10-19", "period_end": "2026-10-25", "notes": null
            }),
        )
        .unwrap();
    let facts = &review.data["review"]["facts"];
    assert_eq!(facts["habit_completions"], 3);
    assert_eq!(facts["habit_target"], 5);
    assert_eq!(facts["habit_adherence_percent"], 60);
    let stats = f.query(
        "stats.get",
        json!({"start": "2026-10-19", "end": "2026-10-25"}),
    );
    assert_eq!(stats["facts"]["habit_adherence_percent"], 60);
}

#[test]
fn habit_weekly_quota_and_opportunity_windows_are_explicit() {
    let mut f = Fixture::new();
    let habit = f
        .execute(
            "habit.create",
            json!({"habit": {
                "title": "配额习惯", "timezone": "Asia/Shanghai", "start_date": "2026-10-19",
                "weekly_target": 5, "scheduled_weekdays": [1,2,3,4,5], "time_of_day": "07:30:00"
            }}),
        )
        .unwrap();
    let habit_id = habit.data["habit"]["id"].as_str().unwrap().to_string();
    for (day, key) in [(19, "quota-1"), (20, "quota-2")] {
        f.execute(
            "habit.log",
            json!({
                "habit_id": habit_id, "occurred_at": format!("2026-10-{day:02}T00:00:00Z"),
                "minutes": 20, "dedupe_key": key
            }),
        )
        .unwrap();
    }
    let review = f
        .execute(
            "review.build",
            json!({"period_start": "2026-10-19", "period_end": "2026-10-25", "notes": null}),
        )
        .unwrap();
    assert_eq!(review.data["review"]["facts"]["habit_target"], 5);
    assert_eq!(review.data["review"]["facts"]["habit_completions"], 2);
    let world = f.world();
    let habit = world.habits.iter().find(|row| row.id == habit_id).unwrap();
    assert_eq!(habit.weekly_target, 5);
    assert_eq!(habit.scheduled_weekdays, vec![1, 2, 3, 4, 5]);
    assert_eq!(habit.time_of_day.unwrap().to_string(), "07:30:00");
    assert_eq!(world.tasks.len(), 0);
}

#[test]
fn calendar_source_cursor_and_provider_mapping_are_persisted() {
    let mut f = Fixture::new();
    f.execute("calendar.import_busy", json!({
        "source_id": null, "provider": "manual", "display_name": "游标日历",
        "external_id": "cursor-calendar", "replace_source": true, "cursor": "cursor-1",
        "events": [{"external_id": "ev-1", "title": "会议一", "start": "2026-10-26T01:00:00Z", "end": "2026-10-26T02:00:00Z"}]
    })).unwrap();
    assert_eq!(f.world().calendar_sources.len(), 1);
    assert_eq!(
        f.world().calendar_sources[0].cursor.as_deref(),
        Some("cursor-1")
    );
    f.execute("calendar.import_busy", json!({
        "source_id": null, "provider": "manual", "display_name": "游标日历",
        "external_id": "cursor-calendar", "replace_source": true, "cursor": "cursor-2",
        "events": [{"external_id": "ev-2", "title": "会议二", "start": "2026-10-26T03:00:00Z", "end": "2026-10-26T04:00:00Z"}]
    })).unwrap();
    let world = f.world();
    assert_eq!(world.calendar_sources.len(), 1);
    assert_eq!(
        world.calendar_sources[0].cursor.as_deref(),
        Some("cursor-2")
    );
    assert_eq!(world.calendar_events.len(), 1);
    assert_eq!(world.calendar_events[0].external_id, "ev-2");
    let bad = f.execute("calendar.import_busy", json!({
        "source_id": null, "provider": "manual", "display_name": "坏批次",
        "external_id": "bad-calendar", "replace_source": true, "cursor": "cursor-3",
        "events": [{"external_id": "bad", "title": "坏事件", "start": "2026-10-26T04:00:00Z", "end": "2026-10-26T03:00:00Z"}]
    }));
    assert!(bad.is_err());
    assert_eq!(f.world().calendar_sources.len(), 1);
}

#[test]
fn recurrence_scope_future_and_series_preserve_closed_history() {
    let mut f = Fixture::new();
    let rule = f
        .execute(
            "recurrence.create",
            json!({
                "rule": {
                    "title": "范围测试", "timezone": "Asia/Shanghai", "start_date": "2026-10-23",
                    "frequency": "daily", "interval": 1, "estimate_minutes": 30
                },
                "through": "2026-10-25"
            }),
        )
        .unwrap();
    let rule_id = rule.data["rule"]["id"].as_str().unwrap().to_string();
    let task_ids: Vec<String> = f
        .world()
        .occurrences
        .iter()
        .filter(|row| row.rule_id == rule_id)
        .filter_map(|row| row.task_id.clone())
        .collect();
    assert_eq!(task_ids.len(), 3);
    let future = f
        .execute(
            "recurrence.update",
            json!({
                "rule_id": rule_id, "scope": "future", "title": "未来改名", "estimate_minutes": 45
            }),
        )
        .unwrap();
    assert_eq!(future.data["scope"], "future");
    assert_eq!(future.data["updated_task_ids"].as_array().unwrap().len(), 2);
    let series = f
        .execute(
            "recurrence.update",
            json!({
                "rule_id": rule_id, "scope": "series", "title": "全系列改名", "estimate_minutes": 60
            }),
        )
        .unwrap();
    assert_eq!(series.data["scope"], "series");
    assert_eq!(series.data["updated_task_ids"].as_array().unwrap().len(), 3);
    let world = f.world();
    assert_eq!(world.recurrence_rules[0].title, "全系列改名");
    for task_id in task_ids {
        let task = world.task(&task_id).unwrap();
        assert_eq!(task.title, "全系列改名");
        assert_eq!(task.estimate_minutes, Some(60));
    }
}

#[test]
fn multi_week_planning_spans_capacity_blocks_across_periods() {
    let mut f = Fixture::new();
    for (start, end, title) in [
        ("2026-10-26T01:00:00Z", "2026-10-26T04:00:00Z", "Week 44"),
        ("2026-11-02T01:00:00Z", "2026-11-02T04:00:00Z", "Week 45"),
        ("2026-11-09T01:00:00Z", "2026-11-09T04:00:00Z", "Week 46"),
    ] {
        f.execute(
            "capacity.create",
            json!({"capacity": {
                "title": title, "start": start, "end": end, "capacity_minutes": 180
            }}),
        )
        .unwrap();
    }
    let mut task_ids = Vec::new();
    for (title, due) in [
        ("第四十四周任务", "2026-10-27"),
        ("第四十五周任务", "2026-11-03"),
        ("第四十六周任务", "2026-11-10"),
    ] {
        let created = f
            .execute(
                "task.create",
                json!({"task": {
                    "title": title, "estimate_minutes": 120, "target_due": due, "priority": 2
                }}),
            )
            .unwrap();
        task_ids.push(
            created.data["changes"][0]["id"]
                .as_str()
                .unwrap()
                .to_string(),
        );
    }
    let planned = f
        .execute(
            "plan.propose",
            json!({
                "task_ids": task_ids, "horizon_start": "2026-10-25T00:00:00Z",
                "horizon_end": "2026-11-11T00:00:00Z", "move_unlocked": false
            }),
        )
        .unwrap();
    let assignments = planned.data["plan"]["assignments"].as_array().unwrap();
    println!("PLAN {:?}", planned.data["plan"]);
    assert!(assignments.len() >= 3);
    let mut by_block: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    let mut by_task: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for row in assignments {
        *by_block
            .entry(row["capacity_block_id"].as_str().unwrap().to_string())
            .or_default() += row["minutes"].as_u64().unwrap() as u32;
        *by_task
            .entry(row["task_id"].as_str().unwrap().to_string())
            .or_default() += row["minutes"].as_u64().unwrap() as u32;
    }
    assert!(by_block.values().all(|minutes| *minutes <= 180));
    assert!(by_task.values().all(|minutes| *minutes == 120));
    let mut per_block = std::collections::BTreeMap::<String, Vec<(String, String)>>::new();
    for row in assignments {
        per_block
            .entry(row["capacity_block_id"].as_str().unwrap().to_string())
            .or_default()
            .push((
                row["start"].as_str().unwrap().to_string(),
                row["end"].as_str().unwrap().to_string(),
            ));
    }
    for rows in per_block.values_mut() {
        rows.sort();
        for pair in rows.windows(2) {
            assert!(
                pair[0].1 <= pair[1].0,
                "allocations in one capacity block must not overlap"
            );
        }
    }
    let dates: Vec<String> = assignments
        .iter()
        .map(|row| row["start"].as_str().unwrap()[..10].to_string())
        .collect();
    assert!(dates.contains(&"2026-10-26".to_string()));
    assert!(dates.contains(&"2026-11-02".to_string()));
    assert!(
        planned.data["plan"]["unplaced"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn content_retrieval_local_personalisation_ranks_linked_notes() {
    let mut f = Fixture::new();
    let task = f
        .execute(
            "task.create",
            json!({"task": {"title": "内容关联任务", "estimate_minutes": 30}}),
        )
        .unwrap();
    let task_id = task.data["changes"][0]["id"].as_str().unwrap().to_string();
    f.execute(
        "note.create",
        json!({"note": {
            "title": "普通案例", "body": "案例正文一", "data_class": "device_only"
        }}),
    )
    .unwrap();
    f.execute(
        "note.create",
        json!({"note": {
            "title": "关联案例", "body": "案例正文二", "data_class": "device_only",
            "linked_tasks": [task_id]
        }}),
    )
    .unwrap();
    let plain = f
        .query_result("content.retrieve", json!({"text": "案例", "consent": true}))
        .unwrap();
    assert_eq!(plain["personalisation"], "not_applied");
    let ranked = f
        .query_result(
            "content.retrieve",
            json!({
                "text": "案例", "consent": true, "personalise": true
            }),
        )
        .unwrap();
    assert_eq!(ranked["personalisation"], "local_linked_recency");
    assert_eq!(ranked["items"][0]["title"], "关联案例");
    assert!(
        ranked["items"][0]["personalisation_score"]
            .as_i64()
            .unwrap()
            >= 1
    );
    assert_eq!(ranked["scope"], "local_workspace_only");
    assert!(
        f.query_result(
            "content.retrieve",
            json!({"text": "案例", "consent": false, "personalise": true})
        )
        .is_err()
    );
}

#[test]
fn after_completion_interval_schedules_next_from_completion_date() {
    let mut f = Fixture::new();
    let created = f
        .execute(
            "recurrence.create",
            json!({
                "rule": {
                    "title": "完成后间隔", "timezone": "Asia/Shanghai", "start_date": "2026-10-24",
                    "frequency": "daily", "interval": 1, "estimate_minutes": 20,
                    "after_completion_days": 3, "max_occurrences": 5
                },
                "through": "2026-11-30"
            }),
        )
        .unwrap();
    let rule_id = created.data["rule"]["id"].as_str().unwrap().to_string();
    let first_ids = created.data["task_ids"].as_array().unwrap();
    assert_eq!(first_ids.len(), 1);
    let first_id = first_ids[0].as_str().unwrap().to_string();
    f.execute("task.complete", json!({"id": first_id, "note": null}))
        .unwrap();
    let occurrences: Vec<_> = f
        .world()
        .occurrences
        .iter()
        .filter(|row| row.rule_id == rule_id)
        .cloned()
        .collect();
    assert_eq!(occurrences.len(), 2);
    let next = occurrences
        .iter()
        .find(|row| row.occurrence_key == "2026-10-27")
        .expect("next after completion");
    let next_id = next.task_id.clone().unwrap();
    assert_eq!(
        f.world().task(&next_id).unwrap().target_due,
        Some(chrono::NaiveDate::from_ymd_opt(2026, 10, 27).unwrap())
    );
    // Completing again at the same instant must not duplicate the same occurrence key.
    f.execute("task.complete", json!({"id": next_id, "note": null}))
        .unwrap();
    assert_eq!(
        f.world()
            .occurrences
            .iter()
            .filter(|row| row.rule_id == rule_id)
            .count(),
        2
    );
}

#[test]
fn after_completion_interval_respects_end_date() {
    let mut f = Fixture::new();
    let created = f
        .execute(
            "recurrence.create",
            json!({
                "rule": {
                    "title": "结束条件", "timezone": "Asia/Shanghai", "start_date": "2026-10-24",
                    "frequency": "daily", "interval": 1, "estimate_minutes": 20,
                    "after_completion_days": 10, "end_date": "2026-10-26"
                },
                "through": "2026-11-30"
            }),
        )
        .unwrap();
    let rule_id = created.data["rule"]["id"].as_str().unwrap().to_string();
    let first_id = created.data["task_ids"][0].as_str().unwrap().to_string();
    f.execute("task.complete", json!({"id": first_id, "note": null}))
        .unwrap();
    assert_eq!(
        f.world()
            .occurrences
            .iter()
            .filter(|row| row.rule_id == rule_id)
            .count(),
        1
    );
}

#[test]
fn complex_weekly_monthly_rules_and_end_conditions_are_bounded() {
    let mut f = Fixture::new();
    let weekly = f
        .execute(
            "recurrence.create",
            json!({
                "rule": {
                    "title": "双周计划", "timezone": "Asia/Shanghai", "start_date": "2026-10-19",
                    "frequency": "weekly", "interval": 2, "weekdays": [1, 3],
                    "estimate_minutes": 20, "end_date": "2026-11-05"
                },
                "through": "2026-12-31"
            }),
        )
        .unwrap();
    let weekly_dates: Vec<String> = weekly.data["task_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| {
            f.world()
                .task(id.as_str().unwrap())
                .unwrap()
                .target_due
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        weekly_dates,
        vec!["2026-10-19", "2026-10-21", "2026-11-02", "2026-11-04"]
    );
    let monthly = f
        .execute(
            "recurrence.create",
            json!({
                "rule": {
                    "title": "双月计划", "timezone": "Asia/Shanghai", "start_date": "2026-10-15",
                    "frequency": "monthly", "interval": 2, "estimate_minutes": 20,
                    "end_date": "2027-02-15"
                },
                "through": "2027-02-28"
            }),
        )
        .unwrap();
    let monthly_dates: Vec<String> = monthly.data["task_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| {
            f.world()
                .task(id.as_str().unwrap())
                .unwrap()
                .target_due
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        monthly_dates,
        vec!["2026-10-15", "2026-12-15", "2027-02-15"]
    );
}

#[test]
fn habit_skip_and_undo_are_reconstructible_and_not_completions() {
    let mut f = Fixture::new();
    let habit = f
        .execute(
            "habit.create",
            json!({"habit": {
                "title": "可撤销习惯", "timezone": "Asia/Shanghai", "start_date": "2026-10-20",
                "weekly_target": 5, "scheduled_weekdays": [1,2,3,4,5]
            }}),
        )
        .unwrap();
    let habit_id = habit.data["habit"]["id"].as_str().unwrap().to_string();
    for (day, key) in [
        ("2026-10-20", "streak-1"),
        ("2026-10-21", "streak-2"),
        ("2026-10-22", "streak-3"),
    ] {
        f.execute("habit.log", json!({
            "habit_id": habit_id, "occurred_at": format!("{day}T00:00:00Z"), "minutes": 15, "dedupe_key": key
        })).unwrap();
    }
    f.execute(
        "habit.skip",
        json!({"habit_id": habit_id, "date": "2026-10-23", "note": "生病"}),
    )
    .unwrap();
    let repeat = f.execute(
        "habit.skip",
        json!({"habit_id": habit_id, "date": "2026-10-23"}),
    );
    assert!(repeat.is_err());
    let world = f.world();
    let completions: Vec<String> = world
        .activities
        .iter()
        .filter(|row| {
            row.habit_id.as_deref() == Some(habit_id.as_str())
                && row.kind == zodo_kernel::model::ActivityKind::Habit
        })
        .map(|row| row.occurred_at.format("%Y-%m-%d").to_string())
        .collect::<Vec<_>>();
    let skipped: Vec<String> = world
        .activities
        .iter()
        .filter(|row| {
            row.habit_id.as_deref() == Some(habit_id.as_str())
                && row.kind == zodo_kernel::model::ActivityKind::HabitSkip
        })
        .map(|row| row.occurred_at.format("%Y-%m-%d").to_string())
        .collect::<Vec<_>>();
    assert_eq!(completions, vec!["2026-10-20", "2026-10-21", "2026-10-22"]);
    assert_eq!(skipped, vec!["2026-10-23"]);
    let review = f
        .execute(
            "review.build",
            json!({"period_start": "2026-10-20", "period_end": "2026-10-25", "notes": null}),
        )
        .unwrap();
    assert_eq!(review.data["review"]["facts"]["habit_completions"], 3);
    // Undo removes the most recent skip first, then the most recent completion.
    f.execute("habit.undo", json!({"habit_id": habit_id}))
        .unwrap();
    assert_eq!(
        f.world()
            .activities
            .iter()
            .filter(|row| row.habit_id.as_deref() == Some(habit_id.as_str())
                && row.kind == zodo_kernel::model::ActivityKind::HabitSkip)
            .count(),
        0
    );
    f.execute("habit.undo", json!({"habit_id": habit_id}))
        .unwrap();
    assert_eq!(
        f.world()
            .activities
            .iter()
            .filter(|row| row.habit_id.as_deref() == Some(habit_id.as_str())
                && row.kind == zodo_kernel::model::ActivityKind::Habit)
            .count(),
        2
    );
}

#[test]
fn proposal_batch_creates_dependencies_by_client_ref() {
    let mut f = Fixture::new();
    let created = f.execute("proposal.create", json!({
        "intent": "依赖拆分",
        "operations": [
            {"operation": "task.create", "payload": {"task": {"client_ref": "a", "title": "拆分前置", "estimate_minutes": 20}}},
            {"operation": "task.create", "payload": {"task": {"client_ref": "b", "title": "拆分后继", "estimate_minutes": 20}}},
            {"operation": "dependency.add_ref", "payload": {"predecessor_ref": "a", "successor_ref": "b"}}
        ],
        "assumptions": [], "questions": [], "ttl_seconds": 86400
    })).unwrap();
    let proposal_id = created.data["proposal"]["id"].as_str().unwrap().to_string();
    let approved = f
        .execute("proposal.approve", json!({"proposal_id": proposal_id}))
        .unwrap();
    let grant = approved.data["approval"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    f.execute(
        "changes.commit",
        json!({"proposal_id": proposal_id, "approval_grant": grant}),
    )
    .unwrap();
    let world = f.world();
    assert_eq!(world.dependencies.len(), 1);
    let dependency = &world.dependencies[0];
    assert_eq!(
        world.task(&dependency.predecessor_id).unwrap().title,
        "拆分前置"
    );
    assert_eq!(
        world.task(&dependency.successor_id).unwrap().title,
        "拆分后继"
    );
    assert_eq!(world.tasks.len(), 2);
}
