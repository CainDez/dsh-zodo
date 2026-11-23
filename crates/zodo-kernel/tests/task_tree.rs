use chrono::{DateTime, Duration, TimeZone, Utc};
use serde_json::{Value, json};
use zodo_kernel::apply::{ApplyOutcome, apply_change};
use zodo_kernel::error::ErrorCode;
use zodo_kernel::model::{
    Allocation, AllocationState, SourceRef, Task, TaskStatus, Workspace, World,
};
use zodo_kernel::ops::ChangeOp;
use zodo_kernel::planner::{PlanInput, plan};
use zodo_kernel::ports::SequentialIds;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 24, 1, 20, 0).unwrap()
}

fn world() -> World {
    World::new(Workspace::new("ws".into(), "Tree".into(), now()))
}

fn seed(id: &str, parent: Option<&str>) -> Task {
    serde_json::from_value(json!({
        "id": id, "workspace_id": "ws", "title": id, "parent_id": parent,
        "status": "open", "priority": 2, "estimate_minutes": 60,
        "remaining_minutes": 30, "actual_minutes": 10,
        "created_at": now(), "updated_at": now(), "version": 3
    }))
    .unwrap()
}

fn apply(world: &mut World, name: &str, payload: Value) -> zodo_kernel::Result<ApplyOutcome> {
    let op: ChangeOp =
        serde_json::from_value(json!({"operation": name, "payload": payload})).unwrap();
    op.validate_shape().unwrap();
    let mut result = ApplyOutcome::default();
    apply_change(
        world,
        &op,
        "user",
        "test",
        now(),
        &SequentialIds::new(),
        &mut result,
    )?;
    Ok(result)
}

#[test]
fn deep_and_wide_tree_blocks_cycle_and_walks_beyond_terminal_intermediate_nodes() {
    const DEPTH: usize = 12_000;
    const WIDTH: usize = 2_000;
    let mut world = world();
    for i in 0..DEPTH {
        world.tasks.push(seed(
            &format!("deep_{i}"),
            (i > 0).then(|| format!("deep_{}", i - 1)).as_deref(),
        ));
    }
    let bottom = format!("deep_{}", DEPTH - 1);
    for i in 0..WIDTH {
        world.tasks.push(seed(&format!("wide_{i}"), Some(&bottom)));
    }
    assert_eq!(world.ancestor_chain("wide_0").len(), DEPTH);
    assert_eq!(world.open_leaf_tasks().len(), WIDTH);
    let err = apply(
        &mut world,
        "task.update",
        json!({"id":"deep_0", "expected_version":3, "task":{"title":"root", "parent_id":"wide_0"}}),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::ValidationFailed);
    assert!(err.message.contains("cycle"));
    assert!(world.task("deep_0").unwrap().parent_id.is_none());
    for task in &mut world.tasks[1..] {
        task.status = TaskStatus::Done;
    }
    // Legacy leaf archived while still Open behind 11,999 terminal intermediate nodes.
    let last = world.tasks.last_mut().unwrap();
    last.status = TaskStatus::Open;
    last.archived = true;
    for status in ["done", "cancelled"] {
        let err = apply(
            &mut world,
            "task.status",
            json!({"id":"deep_0", "status":status}),
        )
        .unwrap_err();
        assert!(err.message.contains("unfinished descendants"));
    }
    let err = apply(
        &mut world,
        "task.archive",
        json!({"id":"deep_0", "archived":true}),
    )
    .unwrap_err();
    assert!(err.message.contains("unfinished descendants"));
    assert_eq!(world.task("deep_0").unwrap().version, 3);
    world.tasks.last_mut().unwrap().status = TaskStatus::Cancelled;
    apply(
        &mut world,
        "task.status",
        json!({"id":"deep_0", "status":"done"}),
    )
    .unwrap();
    assert_eq!(world.task("deep_0").unwrap().status, TaskStatus::Done);
    assert_eq!(world.tasks.last().unwrap().status, TaskStatus::Cancelled);
}

#[test]
fn move_validates_destination_ancestors_and_leaves_legacy_links_untouched_until_explicitly_moved() {
    let mut world = world();
    world.tasks.extend([
        seed("a", Some("b")),
        seed("b", Some("a")),
        seed("orphan", Some("unknown")),
        seed("foreign", None),
        seed("local_child_of_foreign", Some("foreign")),
    ]);
    world.task_mut("foreign").unwrap().workspace_id = "other".into();
    assert!(world.open_leaf_tasks().is_empty());
    assert_eq!(world.ancestor_chain("a"), ["b"]);
    for (id, wanted) in [
        ("a", ErrorCode::ValidationFailed),
        ("orphan", ErrorCode::NotFound),
        ("foreign", ErrorCode::NotFound),
        ("local_child_of_foreign", ErrorCode::NotFound),
    ] {
        assert_eq!(
            apply(
                &mut world,
                "task.create",
                json!({"task": {"title":"new", "parent_id":id}})
            )
            .unwrap_err()
            .code,
            wanted
        );
    }
    let err = apply(&mut world, "task.update", json!({"id":"orphan", "expected_version":2, "task":{"title":"Orphan", "estimate_minutes":60, "parent_id":null}})).unwrap_err();
    assert_eq!(err.code, ErrorCode::RevisionConflict);
    let mut update = json!({"id":"orphan", "expected_version":3, "task":{"title":"Orphan", "estimate_minutes":60, "parent_id":"unknown"}});
    assert!(
        apply(&mut world, "task.update", update.clone())
            .unwrap()
            .warnings
            .is_empty()
    );
    assert_eq!(
        world.task("orphan").unwrap().parent_id.as_deref(),
        Some("unknown")
    );
    update["expected_version"] = json!(4);
    update["task"]["parent_id"] = Value::Null;
    assert!(
        apply(&mut world, "task.update", update)
            .unwrap()
            .warnings
            .is_empty()
    );
    let orphan = world.task("orphan").unwrap();
    assert_eq!(
        (
            orphan.version,
            orphan.remaining_minutes,
            orphan.actual_minutes
        ),
        (5, Some(30), 10)
    );
    assert_eq!(
        world
            .open_leaf_tasks()
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        ["orphan"]
    );
}

#[test]
fn legacy_rollup_schedule_is_never_replaced_and_allocations_are_not_retargeted() {
    let mut world = world();
    world.tasks.extend([
        seed("summary", None),
        seed("child", Some("summary")),
        seed("leaf", None),
    ]);
    world.task_mut("child").unwrap().archived = true;
    let start = now() + Duration::minutes(40);
    let end = start + Duration::minutes(15);
    let historical = Allocation {
        id: "old".into(),
        workspace_id: "ws".into(),
        task_id: "summary".into(),
        capacity_block_id: None,
        start,
        end,
        minutes: 15,
        locked: false,
        state: AllocationState::Planned,
        source: SourceRef::system(),
        created_at: now(),
        updated_at: now(),
    };
    world.allocations.push(historical.clone());
    assert!(!world.is_leaf("summary"));
    let out = plan(
        &world,
        &PlanInput {
            task_ids: vec![],
            horizon_start: now(),
            horizon_end: end,
            move_unlocked: true,
        },
        now(),
    )
    .unwrap();
    assert_eq!(out.replace_task_ids, ["leaf"]);
    assert_eq!(out.fixed_allocations, ["old"]);
    // A replay drafted before the first child existed must fail, even if the expected
    // removal IDs still match and the assignment itself would be a valid leaf.
    let err = apply(&mut world, "plan.apply", json!({"replace_task_ids":["summary"], "expected_removed_allocation_ids":["old"], "assignments":[], "unplaced":[]})).unwrap_err();
    assert!(err.message.contains("rollup parent"));
    let target_start = start + Duration::hours(1);
    let target_end = end + Duration::hours(1);
    let err = apply(&mut world, "allocation.update", json!({"id":"old", "allocation":{"task_id":"leaf", "start":target_start, "end":target_end, "minutes":15}})).unwrap_err();
    assert!(err.message.contains("rollup parent"));
    let err = apply(
        &mut world,
        "allocation.move",
        json!({"id":"old", "expected_task_version":3, "capacity_block_id":"block", "start":target_start, "end":target_end}),
    ).unwrap_err();
    assert!(err.message.contains("rollup parent"));
    assert_eq!(world.allocations, [historical]);
}

#[test]
fn first_split_distinguishes_started_from_settled_and_later_children_leave_legacy_facts_alone() {
    let mut world = world();
    world
        .tasks
        .extend([seed("parent", None), seed("to_move", None)]);
    let start = now() - Duration::minutes(40);
    let row = Allocation {
        id: "started".into(),
        workspace_id: "ws".into(),
        task_id: "parent".into(),
        capacity_block_id: None,
        start,
        end: start + Duration::minutes(15),
        minutes: 15,
        locked: true,
        state: AllocationState::Started,
        source: SourceRef::system(),
        created_at: start,
        updated_at: start,
    };
    world.allocations.push(row.clone());
    let err = apply(
        &mut world,
        "task.create",
        json!({"task":{"title":"Child", "parent_id":"parent"}}),
    )
    .unwrap_err();
    assert!(err.message.contains("outstanding allocations"));
    assert_eq!(world.allocations, [row]);
    world.allocations[0].state = AllocationState::Done;
    world.allocations[0].start = now() + Duration::minutes(5);
    world.allocations[0].end = now() + Duration::minutes(20);
    let before = world.allocations.clone();
    apply(
        &mut world,
        "task.create",
        json!({"task":{"title":"Child", "parent_id":"parent"}}),
    )
    .unwrap();
    assert_eq!(world.allocations, before);
    // Existing non-leaf with a legacy unsettled row: only a leaf -> rollup transition checks it.
    world.allocations[0].state = AllocationState::Started;
    let before = world.allocations.clone();
    apply(&mut world, "task.update", json!({"id":"to_move", "expected_version":3, "task":{"title":"Moved", "parent_id":"parent", "estimate_minutes":60}})).unwrap();
    assert_eq!(world.children_of("parent").len(), 2);
    assert_eq!(world.allocations, before);
}
