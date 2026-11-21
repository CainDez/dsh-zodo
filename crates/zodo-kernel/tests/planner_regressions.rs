use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use zodo_kernel::apply::{ApplyOutcome, apply_change};
use zodo_kernel::model::{Workspace, World};
use zodo_kernel::ops::ChangeOp;
use zodo_kernel::planner::{PlanInput, PlanOutput, plan};
use zodo_kernel::ports::SequentialIds;

fn instant(value: &str) -> DateTime<Utc> {
    value.parse().unwrap()
}

struct Fixture {
    world: World,
    ids: SequentialIds,
    now: DateTime<Utc>,
}

impl Fixture {
    fn new() -> Self {
        let now = instant("2026-11-18T00:00:00Z");
        Self {
            world: World::new(Workspace::new("ws".into(), "Planner tests".into(), now)),
            ids: SequentialIds::new(),
            now,
        }
    }

    fn change(&mut self, operation: &str, payload: Value) -> String {
        let op: ChangeOp =
            serde_json::from_value(json!({"operation": operation, "payload": payload})).unwrap();
        let mut outcome = ApplyOutcome::default();
        apply_change(
            &mut self.world,
            &op,
            "test",
            "test",
            self.now,
            &self.ids,
            &mut outcome,
        )
        .unwrap();
        outcome.changes.last().unwrap().id.clone()
    }

    fn task(&mut self, title: &str, minutes: u32) -> String {
        self.change(
            "task.create",
            json!({"task": {"title": title, "estimate_minutes": minutes}}),
        )
    }

    fn capacity(&mut self, minutes: u32) -> String {
        self.change(
            "capacity.create",
            json!({"capacity": {
                "title": "Morning", "start": "2026-11-18T01:00:00Z",
                "end": "2026-11-18T04:00:00Z", "capacity_minutes": minutes
            }}),
        )
    }

    fn allocation(
        &mut self,
        task: &str,
        block: &str,
        start: &str,
        end: &str,
        minutes: u32,
        locked: bool,
    ) -> String {
        self.change(
            "allocation.create",
            json!({"allocation": {
                "task_id": task, "capacity_block_id": block, "start": start, "end": end,
                "minutes": minutes, "locked": locked
            }}),
        )
    }

    fn dependency(&mut self, predecessor: &str, successor: &str) {
        self.change(
            "dependency.add",
            json!({"predecessor_id": predecessor, "successor_id": successor}),
        );
    }

    fn plan(&self, tasks: Vec<String>, move_unlocked: bool) -> PlanOutput {
        plan(
            &self.world,
            &PlanInput {
                task_ids: tasks,
                horizon_start: instant("2026-11-18T01:00:00Z"),
                horizon_end: instant("2026-11-18T04:00:00Z"),
                move_unlocked,
            },
            self.now,
        )
        .unwrap()
    }

    fn validate_assignments(&mut self, output: &PlanOutput) {
        for allocation in &output.assignments {
            self.change("allocation.create", json!({"allocation": allocation}));
        }
    }
}

#[test]
fn retained_allocation_consumes_capacity_budget_and_plan_is_applicable() {
    let mut f = Fixture::new();
    let fixed_task = f.task("Fixed", 30);
    let next_task = f.task("Next", 60);
    let block = f.capacity(60);
    let allocation = f.allocation(
        &fixed_task,
        &block,
        "2026-11-18T01:00:00Z",
        "2026-11-18T01:30:00Z",
        30,
        true,
    );
    let before = f.world.allocations.clone();
    let output = f.plan(vec![next_task.clone()], true);
    assert_eq!(
        output.assignments.iter().map(|a| a.minutes).sum::<u32>(),
        30
    );
    assert!(
        output
            .unplaced
            .iter()
            .any(|u| u.task_id == next_task && u.minutes == 30)
    );
    assert!(output.fixed_allocations.contains(&allocation));
    assert_eq!(
        f.world.allocations, before,
        "preview must not mutate locked facts"
    );
    f.validate_assignments(&output);
}

#[test]
fn replacing_unlocked_allocation_releases_its_capacity_budget() {
    let mut f = Fixture::new();
    let task = f.task("Replan", 60);
    let block = f.capacity(60);
    let old = f.allocation(
        &task,
        &block,
        "2026-11-18T01:00:00Z",
        "2026-11-18T01:30:00Z",
        30,
        false,
    );
    let output = f.plan(vec![task], true);
    assert_eq!(
        output.assignments.iter().map(|a| a.minutes).sum::<u32>(),
        60
    );
    assert!(!output.fixed_allocations.contains(&old));
    f.world.allocations.retain(|a| a.id != old);
    f.validate_assignments(&output);
}

#[test]
fn partial_predecessor_does_not_release_successor_into_later_capacity() {
    let mut f = Fixture::new();
    let predecessor = f.task("Predecessor", 60);
    let successor = f.task("Successor", 30);
    f.world.task_mut(&predecessor).unwrap().hard_deadline = Some(instant("2026-11-18T01:30:00Z"));
    f.capacity(180);
    f.dependency(&predecessor, &successor);
    let output = f.plan(vec![predecessor.clone(), successor.clone()], true);
    assert!(
        output
            .assignments
            .iter()
            .any(|a| a.task_id == predecessor && a.minutes == 30)
    );
    assert!(
        output
            .unplaced
            .iter()
            .any(|u| u.task_id == predecessor && u.minutes == 30)
    );
    assert!(!output.assignments.iter().any(|a| a.task_id == successor));
    assert!(
        output
            .unplaced
            .iter()
            .any(|u| u.task_id == successor && u.reason == "dependency_blocked")
    );
    f.validate_assignments(&output);
}

#[test]
fn partial_fixed_predecessor_outside_scope_does_not_release_successor() {
    let mut f = Fixture::new();
    let predecessor = f.task("Predecessor", 60);
    let successor = f.task("Successor", 30);
    let block = f.capacity(180);
    f.allocation(
        &predecessor,
        &block,
        "2026-11-18T01:00:00Z",
        "2026-11-18T01:30:00Z",
        30,
        true,
    );
    f.dependency(&predecessor, &successor);
    let output = f.plan(vec![successor.clone()], true);
    assert!(output.assignments.is_empty());
    assert!(
        output
            .unplaced
            .iter()
            .any(|u| u.task_id == successor && u.reason == "dependency_blocked")
    );
}

#[test]
fn fully_fixed_predecessor_releases_successor_only_at_fixed_finish() {
    for include_predecessor in [true, false] {
        let mut f = Fixture::new();
        let predecessor = f.task("Predecessor", 30);
        let successor = f.task("Successor", 30);
        let block = f.capacity(180);
        f.allocation(
            &predecessor,
            &block,
            "2026-11-18T02:00:00Z",
            "2026-11-18T02:30:00Z",
            30,
            true,
        );
        f.dependency(&predecessor, &successor);
        let tasks = if include_predecessor {
            vec![predecessor, successor.clone()]
        } else {
            vec![successor.clone()]
        };
        let output = f.plan(tasks, true);
        let assigned = output
            .assignments
            .iter()
            .find(|a| a.task_id == successor)
            .unwrap();
        assert_eq!(assigned.start, instant("2026-11-18T02:30:00Z"));
        assert!(output.unplaced.is_empty());
        f.validate_assignments(&output);
    }
}

#[test]
fn mixed_fixed_and_new_predecessor_uses_latest_finish() {
    let mut f = Fixture::new();
    let predecessor = f.task("Predecessor", 60);
    let successor = f.task("Successor", 30);
    let block = f.capacity(180);
    f.allocation(
        &predecessor,
        &block,
        "2026-11-18T02:00:00Z",
        "2026-11-18T02:30:00Z",
        30,
        true,
    );
    f.dependency(&predecessor, &successor);
    let output = f.plan(vec![predecessor.clone(), successor.clone()], true);
    assert!(
        output
            .assignments
            .iter()
            .any(|a| a.task_id == predecessor && a.end == instant("2026-11-18T01:30:00Z"))
    );
    assert_eq!(
        output
            .assignments
            .iter()
            .find(|a| a.task_id == successor)
            .unwrap()
            .start,
        instant("2026-11-18T02:30:00Z")
    );
    f.validate_assignments(&output);
}

#[test]
fn historical_allocation_still_consumes_its_block_budget() {
    let mut f = Fixture::new();
    let old_task = f.task("History", 30);
    let next_task = f.task("Next", 60);
    let block = f.capacity(60);
    f.allocation(
        &old_task,
        &block,
        "2026-11-18T01:00:00Z",
        "2026-11-18T01:30:00Z",
        30,
        false,
    );
    f.now = instant("2026-11-18T01:45:00Z");
    let output = f.plan(vec![next_task], true);
    assert_eq!(
        output.assignments.iter().map(|a| a.minutes).sum::<u32>(),
        30
    );
    assert!(output.assignments.iter().all(|a| a.start >= f.now));
    f.validate_assignments(&output);
}

#[test]
fn retained_allocations_can_exhaust_budget_before_block_end() {
    let mut f = Fixture::new();
    let fixed_task = f.task("Fixed", 60);
    let next_task = f.task("Next", 30);
    let block = f.capacity(60);
    f.allocation(
        &fixed_task,
        &block,
        "2026-11-18T01:00:00Z",
        "2026-11-18T02:00:00Z",
        60,
        true,
    );
    let output = f.plan(vec![next_task.clone()], true);
    assert!(output.assignments.is_empty());
    assert!(
        output
            .unplaced
            .iter()
            .any(|u| u.task_id == next_task && u.minutes == 30)
    );
}
