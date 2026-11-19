use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::error::{Result, ZodoError};
use crate::model::{
    ActivityKind, Allocation, AllocationState, CapacityBlock, Commitment, Dependency, Goal,
    GoalEvidence, GoalStatus, Note, Project, ProjectStatus, Reminder, ReminderStatus, SourceRef,
    Tag, Task, TaskStatus, World,
};
use crate::ops::{AllocationDraft, ChangeOp, UnplacedWork};
use crate::ports::{IdSource, NewEffect};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedChange {
    pub kind: String,
    pub id: String,
    pub action: String,
}

#[derive(Debug, Clone, Default)]
pub struct ApplyOutcome {
    pub changes: Vec<AppliedChange>,
    pub effects: Vec<NewEffect>,
    pub warnings: Vec<String>,
}

impl ApplyOutcome {
    pub fn record(&mut self, kind: &str, id: &str, action: &str) {
        self.changes.push(AppliedChange {
            kind: kind.to_string(),
            id: id.to_string(),
            action: action.to_string(),
        });
    }
}

pub fn timezone(world: &World) -> Tz {
    world
        .workspace
        .settings
        .timezone
        .parse::<Tz>()
        .unwrap_or(chrono_tz::Asia::Shanghai)
}

pub fn source_for(actor_id: &str, channel: &str) -> SourceRef {
    SourceRef {
        kind: channel.to_string(),
        id: None,
        actor_id: Some(actor_id.to_string()),
    }
}

pub fn apply_change(
    world: &mut World,
    op: &ChangeOp,
    actor_id: &str,
    channel: &str,
    now: DateTime<Utc>,
    ids: &dyn IdSource,
    outcome: &mut ApplyOutcome,
) -> Result<()> {
    let tz = timezone(world);
    let source = source_for(actor_id, channel);
    match op {
        ChangeOp::TaskCreate { task } => {
            let created = create_task(world, task, &source, now, ids, tz, outcome)?;
            outcome.record("task", &created, "created");
        }
        ChangeOp::TaskUpdate {
            id,
            task,
            expected_version,
        } => {
            let current_version = world.task(id).map(|task| task.version);
            if let (Some(expected), Some(current)) = (expected_version, current_version) {
                if *expected != current {
                    return Err(ZodoError::new(
                        crate::error::ErrorCode::RevisionConflict,
                        "the task changed since it was loaded",
                    ));
                }
            }
            update_task(world, id, task, now, tz, ids, outcome)?;
            outcome.record("task", id, "updated");
        }
        ChangeOp::TaskStatus { id, status, note } => {
            match status {
                TaskStatus::Done => complete_task(
                    world,
                    id,
                    note.clone(),
                    actor_id,
                    channel,
                    now,
                    ids,
                    outcome,
                )?,
                TaskStatus::Cancelled => cancel_task(world, id, note.clone(), now, outcome)?,
                TaskStatus::Open => reopen_task(world, id, now, outcome)?,
                TaskStatus::InProgress => {
                    let task = world
                        .task_mut(id)
                        .ok_or_else(|| ZodoError::not_found("task", id))?;
                    task.status = TaskStatus::InProgress;
                    task.updated_at = now;
                    task.version += 1;
                }
            }
            outcome.record("task", id, "status_changed");
        }
        ChangeOp::TaskArchive { id, archived } => {
            let task = world
                .task_mut(id)
                .ok_or_else(|| ZodoError::not_found("task", id))?;
            task.archived = *archived;
            task.updated_at = now;
            task.version += 1;
            outcome.record("task", id, if *archived { "archived" } else { "restored" });
        }
        ChangeOp::TaskSetBlocked {
            id,
            blocked,
            reason,
        } => {
            let task = world
                .task_mut(id)
                .ok_or_else(|| ZodoError::not_found("task", id))?;
            task.blocked_manual = *blocked;
            task.blocked_reason = if *blocked { reason.clone() } else { None };
            task.updated_at = now;
            task.version += 1;
            outcome.record("task", id, "blocked_changed");
        }
        ChangeOp::GoalCreate { goal } => {
            let id = ids.next("goal");
            let created = Goal {
                id: id.clone(),
                workspace_id: world.workspace.id.clone(),
                title: goal.title.trim().to_string(),
                description: goal.description.clone(),
                success_criteria: goal.success_criteria.clone(),
                target_due: goal.target_due,
                hard_deadline: goal.hard_deadline,
                status: GoalStatus::Active,
                evidence: Vec::new(),
                achieved_at: None,
                created_at: now,
                updated_at: now,
                version: 1,
            };
            world.goals.push(created);
            outcome.record("goal", &id, "created");
        }
        ChangeOp::GoalUpdate { id, goal } => {
            let target = world
                .goals
                .iter_mut()
                .find(|value| value.id == *id)
                .ok_or_else(|| ZodoError::not_found("goal", id))?;
            target.title = goal.title.trim().to_string();
            target.description = goal.description.clone();
            target.success_criteria = goal.success_criteria.clone();
            target.target_due = goal.target_due;
            target.hard_deadline = goal.hard_deadline;
            target.updated_at = now;
            target.version += 1;
            outcome.record("goal", id, "updated");
        }
        ChangeOp::GoalStatus {
            id,
            status,
            evidence,
        } => {
            let target = world
                .goals
                .iter_mut()
                .find(|value| value.id == *id)
                .ok_or_else(|| ZodoError::not_found("goal", id))?;
            if *status == GoalStatus::Achieved && target.evidence.is_empty() && evidence.is_none() {
                return Err(ZodoError::validation(
                    "a goal needs evidence or explicit confirmation before it is achieved",
                ));
            }
            if let Some(evidence) = evidence {
                target.evidence.push(GoalEvidence {
                    text: evidence.clone(),
                    recorded_at: now,
                    source: source.clone(),
                });
            }
            target.status = status.clone();
            target.achieved_at = if *status == GoalStatus::Achieved {
                Some(now)
            } else {
                None
            };
            target.updated_at = now;
            target.version += 1;
            outcome.record("goal", id, "status_changed");
        }
        ChangeOp::ProjectCreate { project } => {
            if let Some(goal_id) = &project.goal_id {
                if world.goal(goal_id).is_none() {
                    return Err(ZodoError::not_found("goal", goal_id));
                }
            }
            let id = ids.next("prj");
            world.projects.push(Project {
                id: id.clone(),
                workspace_id: world.workspace.id.clone(),
                title: project.title.trim().to_string(),
                scope: project.scope.clone(),
                goal_id: project.goal_id.clone(),
                status: ProjectStatus::Active,
                milestones: Vec::new(),
                created_at: now,
                updated_at: now,
                version: 1,
            });
            outcome.record("project", &id, "created");
        }
        ChangeOp::ProjectUpdate { id, project } => {
            if let Some(goal_id) = &project.goal_id {
                if world.goal(goal_id).is_none() {
                    return Err(ZodoError::not_found("goal", goal_id));
                }
            }
            let target = world
                .projects
                .iter_mut()
                .find(|value| value.id == *id)
                .ok_or_else(|| ZodoError::not_found("project", id))?;
            target.title = project.title.trim().to_string();
            target.scope = project.scope.clone();
            target.goal_id = project.goal_id.clone();
            target.updated_at = now;
            target.version += 1;
            outcome.record("project", id, "updated");
        }
        ChangeOp::ProjectStatus { id, status } => {
            let target = world
                .projects
                .iter_mut()
                .find(|value| value.id == *id)
                .ok_or_else(|| ZodoError::not_found("project", id))?;
            target.status = status.clone();
            target.updated_at = now;
            target.version += 1;
            outcome.record("project", id, "status_changed");
        }
        ChangeOp::DependencyAddRef { .. } => {
            return Err(ZodoError::validation(
                "dependency refs must be resolved inside a proposal batch",
            ));
        }
        ChangeOp::DependencyAdd {
            predecessor_id,
            successor_id,
        } => {
            if world.task(predecessor_id).is_none() {
                return Err(ZodoError::not_found("task", predecessor_id));
            }
            if world.task(successor_id).is_none() {
                return Err(ZodoError::not_found("task", successor_id));
            }
            if world.would_create_dependency_cycle(predecessor_id, successor_id) {
                return Err(ZodoError::validation(
                    "dependency would create a cycle; the original graph is unchanged",
                ));
            }
            if world.dependencies.iter().any(|dep| {
                dep.predecessor_id == *predecessor_id && dep.successor_id == *successor_id
            }) {
                return Err(ZodoError::validation("dependency already exists"));
            }
            let id = ids.next("dep");
            world.dependencies.push(Dependency {
                id: id.clone(),
                workspace_id: world.workspace.id.clone(),
                predecessor_id: predecessor_id.clone(),
                successor_id: successor_id.clone(),
                created_at: now,
            });
            outcome.record("dependency", &id, "created");
        }
        ChangeOp::DependencyRemove { id } => {
            let position = world
                .dependencies
                .iter()
                .position(|dep| dep.id == *id)
                .ok_or_else(|| ZodoError::not_found("dependency", id))?;
            world.dependencies.remove(position);
            outcome.record("dependency", id, "removed");
        }
        ChangeOp::TaskSetTags { task_id, tags } => {
            validate_tags(tags)?;
            let task = world
                .task_mut(task_id)
                .ok_or_else(|| ZodoError::not_found("task", task_id))?;
            task.tags = tags.clone();
            task.updated_at = now;
            task.version += 1;
            ensure_tags(world, tags, now, ids);
            outcome.record("task", task_id, "updated");
        }
        ChangeOp::CommitmentCreate { commitment } => {
            if let Some(external_id) = commitment
                .external_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                if world
                    .commitments
                    .iter()
                    .any(|existing| existing.external_id.as_deref() == Some(external_id))
                {
                    return Err(ZodoError::validation(format!(
                        "commitment external_id {external_id} already exists"
                    )));
                }
            }
            let id = create_commitment(world, commitment, &source, now, ids)?;
            if let Some(external_id) = commitment
                .external_id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                outcome.effects.push(NewEffect {
                    id: ids.next("eff"),
                    workspace_id: world.workspace.id.clone(),
                    effect_type: "calendar.external_write".to_string(),
                    payload: serde_json::json!({
                        "commitment_id": id,
                        "external_id": external_id,
                        "title": commitment.title.trim(),
                        "start": commitment.start,
                        "end": commitment.end,
                    }),
                    dedupe_key: Some(format!("calendar:{external_id}")),
                    available_at: now,
                });
            }
            outcome.record("commitment", &id, "created");
        }
        ChangeOp::CommitmentUpdate { id, commitment } => {
            update_commitment(world, id, commitment, now)?;
            outcome.record("commitment", id, "updated");
        }
        ChangeOp::CommitmentRemove { id } => {
            let position = world
                .commitments
                .iter()
                .position(|value| value.id == *id)
                .ok_or_else(|| ZodoError::not_found("commitment", id))?;
            world.commitments.remove(position);
            outcome.record("commitment", id, "removed");
        }
        ChangeOp::CapacityCreate { capacity } => {
            let id = create_capacity(world, capacity, &source, now, ids)?;
            outcome.record("capacity_block", &id, "created");
        }
        ChangeOp::CapacityUpdate { id, capacity } => {
            update_capacity(world, id, capacity, now)?;
            outcome.record("capacity_block", id, "updated");
        }
        ChangeOp::CapacityRemove { id } => {
            if let Some(block) = world.capacity_blocks.iter().find(|value| value.id == *id) {
                if block.locked {
                    return Err(ZodoError::validation(
                        "locked capacity cannot be removed; unlock it explicitly first",
                    ));
                }
            }
            if world
                .allocations
                .iter()
                .any(|allocation| allocation.capacity_block_id.as_deref() == Some(id))
            {
                return Err(ZodoError::validation(
                    "capacity block still contains allocations; remove or move them first",
                ));
            }
            let position = world
                .capacity_blocks
                .iter()
                .position(|value| value.id == *id)
                .ok_or_else(|| ZodoError::not_found("capacity_block", id))?;
            world.capacity_blocks.remove(position);
            outcome.record("capacity_block", id, "removed");
        }
        ChangeOp::AllocationCreate { allocation } => {
            let id = create_allocation(world, allocation, &source, now, ids)?;
            outcome.record("allocation", &id, "created");
        }
        ChangeOp::AllocationSchedule { allocation } => {
            let draft = validate_scheduled_allocation(world, allocation, now)?;
            let id = create_allocation(world, &draft, &source, now, ids)?;
            outcome.record("allocation", &id, "created");
        }
        ChangeOp::AllocationUpdate { id, allocation } => {
            update_allocation(world, id, allocation, source.clone(), now)?;
            outcome.record("allocation", id, "updated");
        }
        ChangeOp::AllocationRemove { id } => {
            let position = world
                .allocations
                .iter()
                .position(|value| value.id == *id)
                .ok_or_else(|| ZodoError::not_found("allocation", id))?;
            let allocation = world.allocations[position].clone();
            require_manual_allocation_editable(&allocation, now)?;
            world.allocations.remove(position);
            outcome.record("allocation", id, "removed");
        }
        ChangeOp::AllocationSetLocked { id, locked } => {
            let allocation = world
                .allocations
                .iter_mut()
                .find(|value| value.id == *id)
                .ok_or_else(|| ZodoError::not_found("allocation", id))?;
            allocation.locked = *locked;
            allocation.updated_at = now;
            outcome.record("allocation", id, "lock_changed");
        }
        ChangeOp::NoteCreate { note } => {
            validate_links(world, &note.linked_tasks, &note.linked_goals)?;
            let id = ids.next("note");
            world.notes.push(Note {
                id: id.clone(),
                workspace_id: world.workspace.id.clone(),
                title: note.title.trim().to_string(),
                body: note.body.clone(),
                source: source.clone(),
                data_class: note.data_class.clone(),
                linked_tasks: note.linked_tasks.clone(),
                linked_goals: note.linked_goals.clone(),
                created_at: now,
                updated_at: now,
                version: 1,
            });
            outcome.record("note", &id, "created");
        }
        ChangeOp::NoteUpdate {
            id,
            note,
            expected_version,
        } => {
            validate_links(world, &note.linked_tasks, &note.linked_goals)?;
            let target = world
                .notes
                .iter_mut()
                .find(|value| value.id == *id)
                .ok_or_else(|| ZodoError::not_found("note", id))?;
            if let Some(expected) = expected_version {
                if *expected != target.version {
                    return Err(ZodoError::new(
                        crate::error::ErrorCode::RevisionConflict,
                        "the note changed since it was loaded",
                    ));
                }
            }
            target.title = note.title.trim().to_string();
            target.body = note.body.clone();
            target.data_class = note.data_class.clone();
            target.linked_tasks = note.linked_tasks.clone();
            target.linked_goals = note.linked_goals.clone();
            target.updated_at = now;
            target.version += 1;
            outcome.record("note", id, "updated");
        }
        ChangeOp::ReminderCreate {
            task_id,
            message,
            fire_at,
            dedupe_key,
        } => {
            if let Some(task_id) = task_id {
                if world.task(task_id).is_none() {
                    return Err(ZodoError::not_found("task", task_id));
                }
            }
            if let Some(key) = dedupe_key {
                if world
                    .reminders
                    .iter()
                    .any(|reminder| reminder.dedupe_key.as_deref() == Some(key))
                {
                    return Err(ZodoError::validation(
                        "reminder with this dedupe key already exists",
                    ));
                }
            }
            let id = ids.next("rem");
            world.reminders.push(Reminder {
                id: id.clone(),
                workspace_id: world.workspace.id.clone(),
                task_id: task_id.clone(),
                message: message.clone(),
                fire_at: *fire_at,
                status: ReminderStatus::Pending,
                dedupe_key: dedupe_key.clone(),
                created_at: now,
                updated_at: now,
                delivered_at: None,
                failure: None,
            });
            let effect_id = ids.next("eff");
            outcome.effects.push(NewEffect {
                id: effect_id,
                workspace_id: world.workspace.id.clone(),
                effect_type: "reminder.deliver".to_string(),
                payload: serde_json::json!({"reminder_id": id, "fire_at": fire_at}),
                dedupe_key: Some(format!("reminder:{id}")),
                available_at: *fire_at,
            });
            outcome.record("reminder", &id, "created");
        }
        ChangeOp::ReminderCancel { id } => {
            let reminder = world
                .reminders
                .iter_mut()
                .find(|value| value.id == *id)
                .ok_or_else(|| ZodoError::not_found("reminder", id))?;
            if matches!(reminder.status, ReminderStatus::Delivered) {
                return Err(ZodoError::validation(
                    "a delivered reminder cannot be cancelled",
                ));
            }
            reminder.status = ReminderStatus::Cancelled;
            reminder.updated_at = now;
            outcome.record("reminder", id, "cancelled");
        }
        ChangeOp::PlanApply {
            replace_task_ids,
            expected_removed_allocation_ids,
            assignments,
            unplaced,
        } => {
            apply_plan(
                world,
                replace_task_ids,
                expected_removed_allocation_ids.as_deref(),
                assignments,
                unplaced,
                now,
                ids,
                &source,
                outcome,
            )?;
        }
    }
    world.workspace.updated_at = now;
    Ok(())
}

fn create_task(
    world: &mut World,
    spec: &crate::ops::TaskSpec,
    source: &SourceRef,
    now: DateTime<Utc>,
    ids: &dyn IdSource,
    tz: Tz,
    outcome: &mut ApplyOutcome,
) -> Result<String> {
    validate_task_relations(world, spec, None)?;
    let id = ids.next("tsk");
    let remaining = spec.estimate_minutes;
    world.tasks.push(Task {
        id: id.clone(),
        workspace_id: world.workspace.id.clone(),
        title: spec.title.trim().to_string(),
        description: spec.description.clone(),
        project_id: spec.project_id.clone(),
        parent_id: spec.parent_id.clone(),
        status: TaskStatus::Open,
        estimate_minutes: spec.estimate_minutes,
        remaining_minutes: remaining,
        actual_minutes: 0,
        priority: spec.priority,
        target_due: spec.effective_target_due(),
        hard_deadline: spec.effective_hard_deadline(tz),
        not_before: spec.not_before,
        preferred_window: spec.preferred_window.clone(),
        tags: spec.tags.clone(),
        archived: false,
        blocked_manual: false,
        series_id: None,
        occurrence_key: None,
        blocked_reason: None,
        created_at: now,
        updated_at: now,
        completed_at: None,
        version: 1,
    });
    ensure_tags(world, &spec.tags, now, ids);
    let _ = source;
    let _ = outcome;
    Ok(id)
}

fn update_task(
    world: &mut World,
    id: &str,
    spec: &crate::ops::TaskSpec,
    now: DateTime<Utc>,
    tz: Tz,
    ids: &dyn IdSource,
    outcome: &mut ApplyOutcome,
) -> Result<()> {
    validate_task_relations(world, spec, Some(id))?;
    let (completed, has_children) = {
        let task = world
            .task(id)
            .ok_or_else(|| ZodoError::not_found("task", id))?;
        (
            task.status == TaskStatus::Done,
            !world.children_of(id).is_empty(),
        )
    };
    if completed && has_children {
        return Err(ZodoError::validation(
            "a completed parent with open children cannot be edited",
        ));
    }
    let target = world
        .task_mut(id)
        .ok_or_else(|| ZodoError::not_found("task", id))?;
    let estimate = spec.estimate_minutes;
    let consumed = target.actual_minutes;
    target.title = spec.title.trim().to_string();
    target.description = spec.description.clone();
    target.project_id = spec.project_id.clone();
    target.parent_id = spec.parent_id.clone();
    target.estimate_minutes = estimate;
    target.remaining_minutes = estimate.map(|value| value.saturating_sub(consumed));
    target.priority = spec.priority;
    target.target_due = spec.effective_target_due();
    target.hard_deadline = spec.effective_hard_deadline(tz);
    target.not_before = spec.not_before;
    target.preferred_window = spec.preferred_window.clone();
    target.tags = spec.tags.clone();
    target.updated_at = now;
    target.version += 1;
    ensure_tags(world, &spec.tags, now, ids);
    outcome
        .warnings
        .push("editing the estimate recalculates remaining minutes".to_string());
    Ok(())
}

fn validate_task_relations(
    world: &World,
    spec: &crate::ops::TaskSpec,
    current_id: Option<&str>,
) -> Result<()> {
    if let Some(project_id) = &spec.project_id {
        if world.project(project_id).is_none() {
            return Err(ZodoError::not_found("project", project_id));
        }
    }
    if let Some(parent_id) = &spec.parent_id {
        if current_id == Some(parent_id.as_str()) {
            return Err(ZodoError::validation("a task cannot be its own parent"));
        }
        if world.task(parent_id).is_none() {
            return Err(ZodoError::not_found("task", parent_id));
        }
        if let Some(current_id) = current_id {
            let mut stack = vec![parent_id.clone()];
            let mut seen = BTreeSet::new();
            while let Some(candidate) = stack.pop() {
                if candidate == current_id {
                    return Err(ZodoError::validation(
                        "parent relationship would create a cycle",
                    ));
                }
                if !seen.insert(candidate.clone()) {
                    continue;
                }
                if let Some(parent) = world
                    .task(&candidate)
                    .and_then(|task| task.parent_id.clone())
                {
                    stack.push(parent);
                }
            }
        }
    }
    Ok(())
}

fn ensure_tags(world: &mut World, tags: &[String], now: DateTime<Utc>, ids: &dyn IdSource) {
    for tag in tags {
        if !world
            .tags
            .iter()
            .any(|value| value.name.eq_ignore_ascii_case(tag))
        {
            world.tags.push(Tag {
                id: ids.next("tag"),
                workspace_id: world.workspace.id.clone(),
                name: tag.clone(),
                color: None,
                created_at: now,
            });
        }
    }
}

fn validate_tags(tags: &[String]) -> Result<()> {
    if tags.len() > 32 {
        return Err(ZodoError::validation("at most 32 tags are allowed"));
    }
    for tag in tags {
        if tag.trim().is_empty() || tag.chars().count() > 64 {
            return Err(ZodoError::validation("tags must be 1..64 characters"));
        }
    }
    Ok(())
}

fn complete_task(
    world: &mut World,
    id: &str,
    note: Option<String>,
    actor_id: &str,
    channel: &str,
    now: DateTime<Utc>,
    ids: &dyn IdSource,
    outcome: &mut ApplyOutcome,
) -> Result<()> {
    let has_open_children = world
        .children_of(id)
        .into_iter()
        .any(|child| !matches!(child.status, TaskStatus::Done | TaskStatus::Cancelled));
    if has_open_children {
        return Err(ZodoError::validation(
            "a parent with open children cannot be completed; complete or cancel the children explicitly",
        ));
    }
    let task = world
        .task_mut(id)
        .ok_or_else(|| ZodoError::not_found("task", id))?;
    task.status = TaskStatus::Done;
    task.completed_at = Some(now);
    task.remaining_minutes = Some(0);
    task.updated_at = now;
    task.version += 1;
    let _ = note;
    // Planned future work is no longer useful, but executed and locked facts remain.
    let ids_to_remove: Vec<String> = world
        .allocations
        .iter()
        .filter(|allocation| {
            allocation.task_id == id
                && !allocation.locked
                && allocation.state == AllocationState::Planned
                && allocation.end > now
        })
        .map(|allocation| allocation.id.clone())
        .collect();
    for allocation_id in ids_to_remove {
        world
            .allocations
            .retain(|allocation| allocation.id != allocation_id);
        outcome.record("allocation", &allocation_id, "removed_with_task");
    }
    crate::recurrence::schedule_after_completion(world, id, actor_id, channel, now, ids, outcome)?;
    Ok(())
}

fn cancel_task(
    world: &mut World,
    id: &str,
    _note: Option<String>,
    now: DateTime<Utc>,
    outcome: &mut ApplyOutcome,
) -> Result<()> {
    let task = world
        .task_mut(id)
        .ok_or_else(|| ZodoError::not_found("task", id))?;
    task.status = TaskStatus::Cancelled;
    task.updated_at = now;
    task.version += 1;
    let ids_to_remove: Vec<String> = world
        .allocations
        .iter()
        .filter(|allocation| {
            allocation.task_id == id
                && !allocation.locked
                && allocation.state == AllocationState::Planned
                && allocation.end > now
        })
        .map(|allocation| allocation.id.clone())
        .collect();
    for allocation_id in ids_to_remove {
        world
            .allocations
            .retain(|allocation| allocation.id != allocation_id);
        outcome.record("allocation", &allocation_id, "removed_with_task");
    }
    Ok(())
}

fn reopen_task(
    world: &mut World,
    id: &str,
    now: DateTime<Utc>,
    outcome: &mut ApplyOutcome,
) -> Result<()> {
    let task = world
        .task_mut(id)
        .ok_or_else(|| ZodoError::not_found("task", id))?;
    if task.status != TaskStatus::Done && task.status != TaskStatus::Cancelled {
        return Err(ZodoError::validation(
            "only a done or cancelled task can be reopened",
        ));
    }
    let remaining = task
        .estimate_minutes
        .map(|estimate| estimate.saturating_sub(task.actual_minutes))
        .filter(|minutes| *minutes > 0)
        .or(task.remaining_minutes)
        .or(task.estimate_minutes)
        .unwrap_or(0);
    task.status = TaskStatus::Open;
    task.completed_at = None;
    task.remaining_minutes = Some(remaining.max(1));
    task.updated_at = now;
    task.version += 1;
    let _ = outcome;
    Ok(())
}

fn create_commitment(
    world: &mut World,
    input: &crate::ops::CommitmentInput,
    source: &SourceRef,
    now: DateTime<Utc>,
    ids: &dyn IdSource,
) -> Result<String> {
    let id = ids.next("cmt");
    world.commitments.push(Commitment {
        id: id.clone(),
        workspace_id: world.workspace.id.clone(),
        title: input.title.trim().to_string(),
        start: input.start,
        end: input.end,
        locked: input.locked,
        source: source.clone(),
        external_id: input.external_id.clone(),
        calendar_source_id: input.calendar_source_id.clone(),
        created_at: now,
        updated_at: now,
    });
    Ok(id)
}

fn update_commitment(
    world: &mut World,
    id: &str,
    input: &crate::ops::CommitmentInput,
    now: DateTime<Utc>,
) -> Result<()> {
    let commitment = world
        .commitments
        .iter_mut()
        .find(|value| value.id == id)
        .ok_or_else(|| ZodoError::not_found("commitment", id))?;
    commitment.title = input.title.trim().to_string();
    commitment.start = input.start;
    commitment.end = input.end;
    commitment.locked = input.locked;
    commitment.external_id = input.external_id.clone();
    commitment.calendar_source_id = input.calendar_source_id.clone();
    commitment.updated_at = now;
    Ok(())
}

fn create_capacity(
    world: &mut World,
    input: &crate::ops::CapacityInput,
    source: &SourceRef,
    now: DateTime<Utc>,
    ids: &dyn IdSource,
) -> Result<String> {
    let id = ids.next("cap");
    world.capacity_blocks.push(CapacityBlock {
        id: id.clone(),
        workspace_id: world.workspace.id.clone(),
        title: input.title.trim().to_string(),
        start: input.start,
        end: input.end,
        capacity_minutes: input.capacity_minutes,
        locked: input.locked,
        source: source.clone(),
        created_at: now,
        updated_at: now,
    });
    Ok(id)
}

fn update_capacity(
    world: &mut World,
    id: &str,
    input: &crate::ops::CapacityInput,
    now: DateTime<Utc>,
) -> Result<()> {
    let block = world
        .capacity_blocks
        .iter()
        .find(|value| value.id == id)
        .ok_or_else(|| ZodoError::not_found("capacity_block", id))?;
    if block.locked {
        return Err(ZodoError::validation(
            "locked capacity cannot be modified; unlock it explicitly first",
        ));
    }
    if world.allocations.iter().any(|allocation| {
        allocation.capacity_block_id.as_deref() == Some(id)
            && (allocation.locked
                || allocation.state != AllocationState::Planned
                || allocation.start < now)
    }) {
        return Err(ZodoError::validation(
            "capacity with locked, started or past allocations cannot be modified",
        ));
    }
    let used: u32 = world
        .allocations
        .iter()
        .filter(|allocation| allocation.capacity_block_id.as_deref() == Some(id))
        .map(|allocation| allocation.minutes)
        .sum();
    if used > input.capacity_minutes {
        return Err(ZodoError::validation(
            "capacity cannot be reduced below the minutes already allocated inside it",
        ));
    }
    let block = world
        .capacity_blocks
        .iter_mut()
        .find(|value| value.id == id)
        .ok_or_else(|| ZodoError::not_found("capacity_block", id))?;
    block.title = input.title.trim().to_string();
    block.start = input.start;
    block.end = input.end;
    block.capacity_minutes = input.capacity_minutes;
    block.locked = input.locked;
    block.updated_at = now;
    Ok(())
}

/// Rechecked during both proposal validation and commit. Only the eventual allocation is
/// appended; no task, existing allocation, capacity, activity, or provider fact is modified.
fn validate_scheduled_allocation(
    world: &World,
    input: &crate::ops::ScheduledAllocation,
    now: DateTime<Utc>,
) -> Result<AllocationDraft> {
    input.validate()?;
    if input.start <= now {
        return Err(ZodoError::validation(
            "scheduled interval must start in the future",
        ));
    }
    let task = world
        .task(&input.task_id)
        .ok_or_else(|| ZodoError::not_found("task", &input.task_id))?;
    if task.version != input.expected_task_version {
        return Err(ZodoError::new(
            crate::error::ErrorCode::RevisionConflict,
            "task changed; refresh and create a new scheduling proposal",
        ));
    }
    if !world.is_leaf(&task.id) || task.blocked_manual {
        return Err(ZodoError::validation(
            "only an unblocked leaf task can be scheduled",
        ));
    }
    if task.not_before.is_some_and(|start| input.start < start)
        || task.hard_deadline.is_some_and(|end| input.end > end)
    {
        return Err(ZodoError::validation(
            "scheduled interval violates task time constraints",
        ));
    }
    // Match planner dependency semantics: completed/cancelled predecessors are satisfied;
    // otherwise all remaining work must have outstanding reservations ending before us.
    // An elapsed plan is never treated as proof that predecessor work was completed.
    for dependency in world.dependencies_of(&task.id) {
        let predecessor = world
            .task(&dependency.predecessor_id)
            .ok_or_else(|| ZodoError::not_found("task", &dependency.predecessor_id))?;
        if matches!(predecessor.status, TaskStatus::Done | TaskStatus::Cancelled) {
            continue;
        }
        let required = predecessor
            .remaining_minutes
            .or(predecessor.estimate_minutes)
            .unwrap_or(0);
        let mut reserved = 0_u64;
        let mut latest_end = now;
        for allocation in world.allocations.iter().filter(|allocation| {
            allocation.task_id == predecessor.id && allocation.reserves_work_at(now)
        }) {
            reserved += u64::from(allocation.minutes);
            latest_end = latest_end.max(allocation.end);
        }
        if predecessor.archived
            || predecessor.blocked_manual
            || required == 0
            || reserved < u64::from(required)
            || latest_end > input.start
        {
            return Err(ZodoError::validation(
                "predecessor work is not fully scheduled before this interval",
            ));
        }
    }
    if world.workspace.settings.timezone.parse::<Tz>().is_err() {
        return Err(ZodoError::validation(
            "workspace timezone must be valid before scheduling",
        ));
    }
    if crate::planner::workspace_allowed_ranges(world, input.start, input.end)
        != vec![(input.start, input.end)]
    {
        return Err(ZodoError::validation(
            "scheduled interval is outside workspace availability",
        ));
    }
    // Existing allocation validation remains authoritative for membership/budget, open-task
    // work bounds, and overlap with allocations, commitments, and imported busy events.
    Ok(AllocationDraft {
        id: None,
        task_id: input.task_id.clone(),
        capacity_block_id: Some(input.capacity_block_id.clone()),
        start: input.start,
        end: input.end,
        minutes: (input.end - input.start).num_minutes() as u32,
        locked: false,
    })
}

fn create_allocation(
    world: &mut World,
    draft: &AllocationDraft,
    source: &SourceRef,
    now: DateTime<Utc>,
    ids: &dyn IdSource,
) -> Result<String> {
    validate_allocation(world, draft, None, now)?;
    let id = draft.id.clone().unwrap_or_else(|| ids.next("alc"));
    world.allocations.push(Allocation {
        id: id.clone(),
        workspace_id: world.workspace.id.clone(),
        task_id: draft.task_id.clone(),
        capacity_block_id: draft.capacity_block_id.clone(),
        start: draft.start,
        end: draft.end,
        minutes: draft.minutes,
        locked: draft.locked,
        state: AllocationState::Planned,
        source: source.clone(),
        created_at: now,
        updated_at: now,
    });
    Ok(id)
}

// Keep past and in-progress allocation facts immutable for manual changes, even
// if their persisted state remains Planned. Planner replacement has its own
// preview/replay boundary and is intentionally not changed here.
fn require_manual_allocation_editable(allocation: &Allocation, now: DateTime<Utc>) -> Result<()> {
    if allocation.locked || allocation.state != AllocationState::Planned || allocation.start <= now
    {
        return Err(ZodoError::validation(
            "only unlocked planned allocations starting in the future can be updated or removed",
        ));
    }
    Ok(())
}

fn update_allocation(
    world: &mut World,
    id: &str,
    draft: &AllocationDraft,
    _source: SourceRef,
    now: DateTime<Utc>,
) -> Result<()> {
    let existing = world
        .allocations
        .iter()
        .find(|value| value.id == id)
        .ok_or_else(|| ZodoError::not_found("allocation", id))?
        .clone();
    require_manual_allocation_editable(&existing, now)?;
    if draft.start <= now {
        return Err(ZodoError::validation(
            "new allocation start must be in the future",
        ));
    }
    validate_allocation(world, draft, Some(id), now)?;
    let allocation = world
        .allocations
        .iter_mut()
        .find(|value| value.id == id)
        .expect("allocation exists");
    allocation.task_id = draft.task_id.clone();
    allocation.capacity_block_id = draft.capacity_block_id.clone();
    allocation.start = draft.start;
    allocation.end = draft.end;
    allocation.minutes = draft.minutes;
    allocation.locked = draft.locked;
    allocation.updated_at = now;
    Ok(())
}

fn validate_allocation(
    world: &World,
    draft: &AllocationDraft,
    ignore_id: Option<&str>,
    now: DateTime<Utc>,
) -> Result<()> {
    draft.validate()?;
    let task = world
        .task(&draft.task_id)
        .ok_or_else(|| ZodoError::not_found("task", &draft.task_id))?;
    if task.archived || matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled) {
        return Err(ZodoError::validation(
            "cannot allocate a closed or archived task",
        ));
    }
    let remaining = task
        .remaining_minutes
        .or(task.estimate_minutes)
        .ok_or_else(|| {
            ZodoError::validation(
                "task needs estimate_minutes or remaining_minutes before it can be scheduled",
            )
        })?;
    if remaining == 0 {
        return Err(ZodoError::validation(
            "task has no remaining work to allocate",
        ));
    }
    let allocated: u32 = world
        .allocations
        .iter()
        .filter(|allocation| {
            allocation.task_id == draft.task_id
                && Some(allocation.id.as_str()) != ignore_id
                // Current/future reservations are bounded by outstanding work, not schedule
                // history. Explicit historical writes retain their existing all-time bound.
                && (draft.end <= now || allocation.reserves_work_at(now))
        })
        .map(|allocation| allocation.minutes)
        .sum();
    if allocated + draft.minutes > remaining {
        return Err(ZodoError::validation(format!(
            "allocation would exceed remaining work ({remaining} minutes)"
        )));
    }
    if let Some(block_id) = &draft.capacity_block_id {
        let block = world
            .capacity_blocks
            .iter()
            .find(|value| value.id == *block_id)
            .ok_or_else(|| ZodoError::not_found("capacity_block", block_id))?;
        if draft.start < block.start || draft.end > block.end {
            return Err(ZodoError::validation(
                "allocation is outside its capacity block",
            ));
        }
        let used: u32 = world
            .allocations
            .iter()
            .filter(|allocation| {
                allocation.capacity_block_id.as_deref() == Some(block_id.as_str())
                    && Some(allocation.id.as_str()) != ignore_id
            })
            .map(|allocation| allocation.minutes)
            .sum();
        if used + draft.minutes > block.capacity_minutes {
            return Err(ZodoError::validation(
                "capacity block is already fully consumed",
            ));
        }
    }
    for allocation in &world.allocations {
        if Some(allocation.id.as_str()) == ignore_id {
            continue;
        }
        if overlaps(draft.start, draft.end, allocation.start, allocation.end) {
            return Err(ZodoError::validation(format!(
                "allocation overlaps another allocation ({}..{})",
                allocation.start, allocation.end
            )));
        }
    }
    for commitment in &world.commitments {
        if overlaps(draft.start, draft.end, commitment.start, commitment.end) {
            return Err(ZodoError::validation(format!(
                "allocation overlaps fixed commitment {:?}",
                commitment.title
            )));
        }
    }
    for event in &world.calendar_events {
        if event.busy && overlaps(draft.start, draft.end, event.start, event.end) {
            return Err(ZodoError::validation(
                "allocation overlaps external busy time",
            ));
        }
    }
    Ok(())
}

fn overlaps(
    a_start: DateTime<Utc>,
    a_end: DateTime<Utc>,
    b_start: DateTime<Utc>,
    b_end: DateTime<Utc>,
) -> bool {
    a_start < b_end && b_start < a_end
}

/// Exact eligible replacement IDs at one command instant; used for both approval binding
/// and replay. Crossing a start boundary must not silently change the approved deletion set.
pub(crate) fn plan_removal_ids(
    world: &World,
    replace_task_ids: &[String],
    now: DateTime<Utc>,
) -> Vec<String> {
    let replace: BTreeSet<&str> = replace_task_ids.iter().map(String::as_str).collect();
    let mut removed: Vec<String> = world
        .allocations
        .iter()
        .filter(|allocation| {
            replace.contains(allocation.task_id.as_str())
                && allocation.state == AllocationState::Planned
                && !allocation.locked
                && allocation.start >= now
        })
        .map(|allocation| allocation.id.clone())
        .collect();
    removed.sort();
    removed
}

fn apply_plan(
    world: &mut World,
    replace_task_ids: &[String],
    expected_removed_allocation_ids: Option<&[String]>,
    assignments: &[AllocationDraft],
    unplaced: &[UnplacedWork],
    now: DateTime<Utc>,
    ids: &dyn IdSource,
    source: &SourceRef,
    outcome: &mut ApplyOutcome,
) -> Result<()> {
    // Remove only future, unlocked planned allocations for tasks that this proposal claims. Fixed
    // facts (started, locked, past) remain untouched; unrelated tasks are never re-planned.
    let expected = expected_removed_allocation_ids.ok_or_else(|| {
        ZodoError::new(
            crate::error::ErrorCode::ApprovalRequired,
            "legacy plan has no deletion binding; regenerate plan and approve its new preview",
        )
    })?;
    let removed = plan_removal_ids(world, replace_task_ids, now);
    let mut expected = expected.to_vec();
    expected.sort();
    if expected != removed {
        return Err(ZodoError::new(
            crate::error::ErrorCode::ApprovalRequired,
            "plan deletion effects changed with time; regenerate plan and approve its new preview",
        ));
    }
    if assignments.iter().any(|allocation| allocation.start < now) {
        return Err(ZodoError::new(
            crate::error::ErrorCode::ApprovalRequired,
            "planned interval has already started; regenerate plan and approve its new preview",
        ));
    }
    for id in removed {
        world.allocations.retain(|allocation| allocation.id != id);
        outcome.record("allocation", &id, "removed_by_plan");
    }
    for draft in assignments {
        let id = create_allocation(world, draft, source, now, ids)?;
        outcome.record("allocation", &id, "created");
    }
    for item in unplaced {
        outcome.warnings.push(format!(
            "task {} has {} unplaced minutes: {}",
            item.task_id, item.minutes, item.reason
        ));
    }
    Ok(())
}

fn validate_links(world: &World, task_ids: &[String], goal_ids: &[String]) -> Result<()> {
    for id in task_ids {
        if world.task(id).is_none() {
            return Err(ZodoError::not_found("task", id));
        }
    }
    for id in goal_ids {
        if world.goal(id).is_none() {
            return Err(ZodoError::not_found("goal", id));
        }
    }
    Ok(())
}

pub fn mark_activity(
    world: &mut World,
    task_id: &str,
    minutes: u32,
    now: DateTime<Utc>,
    note: Option<String>,
    source: SourceRef,
    ids: &dyn IdSource,
    kind: ActivityKind,
) -> Result<String> {
    let task = world
        .task_mut(task_id)
        .ok_or_else(|| ZodoError::not_found("task", task_id))?;
    if matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled) {
        return Err(ZodoError::validation(
            "closed tasks cannot receive progress",
        ));
    }
    let remaining = task.remaining_minutes.unwrap_or(0);
    if minutes > remaining {
        return Err(ZodoError::validation(format!(
            "progress of {minutes} minutes exceeds remaining {remaining}"
        )));
    }
    task.actual_minutes = task.actual_minutes.saturating_add(minutes);
    task.remaining_minutes = Some(remaining - minutes);
    task.status = TaskStatus::InProgress;
    task.updated_at = now;
    task.version += 1;
    let id = ids.next("act");
    world.activities.push(crate::model::ActivityRecord {
        id: id.clone(),
        workspace_id: world.workspace.id.clone(),
        kind,
        task_id: Some(task_id.to_string()),
        habit_id: None,
        minutes,
        occurred_at: now,
        note,
        source,
        dedupe_key: None,
        created_at: now,
    });
    Ok(id)
}
