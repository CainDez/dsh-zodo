use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Result, ZodoError};
use crate::model::{AllocationState, World};
use crate::ops::{AllocationDraft, UnplacedWork};

#[derive(Debug, Clone)]
pub struct PlanInput {
    pub task_ids: Vec<String>,
    pub horizon_start: DateTime<Utc>,
    pub horizon_end: DateTime<Utc>,
    pub move_unlocked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanOutput {
    pub assignments: Vec<AllocationDraft>,
    pub unplaced: Vec<UnplacedWork>,
    pub fixed_allocations: Vec<String>,
    pub diagnostics: Vec<PlanDiagnostic>,
    pub replace_task_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanDiagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone)]
struct Slot {
    capacity_block_id: String,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    cursor: DateTime<Utc>,
    capacity_left: u32,
}

pub fn plan(world: &World, input: &PlanInput, now: DateTime<Utc>) -> Result<PlanOutput> {
    if input.horizon_end <= input.horizon_start {
        return Err(ZodoError::validation(
            "planning horizon end must be after start",
        ));
    }
    let mut replace_task_ids: Vec<String> = if input.task_ids.is_empty() {
        world
            .open_leaf_tasks()
            .into_iter()
            .map(|task| task.id.clone())
            .collect()
    } else {
        input.task_ids.clone()
    };
    replace_task_ids.sort();
    replace_task_ids.dedup();
    for id in &replace_task_ids {
        let task = world
            .task(id)
            .ok_or_else(|| ZodoError::not_found("task", id))?;
        if task.archived
            || matches!(
                task.status,
                crate::model::TaskStatus::Done | crate::model::TaskStatus::Cancelled
            )
        {
            return Err(ZodoError::validation(format!(
                "task {id} is closed and cannot be planned"
            )));
        }
        if !world.is_leaf(id) {
            return Err(ZodoError::validation(format!(
                "task {id} is a rollup parent; schedule its leaf children instead"
            )));
        }
    }
    let scope: BTreeSet<&str> = replace_task_ids.iter().map(String::as_str).collect();
    let mut removed_allocations: BTreeSet<String> = BTreeSet::new();
    if input.move_unlocked {
        for allocation in &world.allocations {
            if scope.contains(allocation.task_id.as_str())
                && allocation.state == AllocationState::Planned
                && !allocation.locked
                && allocation.start >= now
            {
                removed_allocations.insert(allocation.id.clone());
            }
        }
    }

    let mut fixed_allocations = Vec::new();
    let mut fixed_minutes: BTreeMap<String, u32> = BTreeMap::new();
    let mut fixed_end: BTreeMap<String, DateTime<Utc>> = BTreeMap::new();
    for allocation in &world.allocations {
        if removed_allocations.contains(&allocation.id) {
            continue;
        }
        fixed_allocations.push(allocation.id.clone());
        if allocation.reserves_work_at(now) {
            *fixed_minutes.entry(allocation.task_id.clone()).or_default() += allocation.minutes;
            fixed_end
                .entry(allocation.task_id.clone())
                .and_modify(|end| *end = (*end).max(allocation.end))
                .or_insert(allocation.end);
        }
    }

    let mut diagnostics = Vec::new();
    let mut slots = build_slots(world, input, now, &removed_allocations, &scope);
    if slots.is_empty() {
        for task_id in &replace_task_ids {
            diagnostics.push(PlanDiagnostic {
                code: "NO_CAPACITY".to_string(),
                severity: "hard".to_string(),
                message: "no free capacity exists in this horizon".to_string(),
                task_id: Some(task_id.clone()),
            });
        }
    }

    // Ready-task scheduling: hard deadlines first, then desired date, priority and stable id.
    let mut remaining_tasks: Vec<String> = replace_task_ids.clone();
    let mut scheduled_end: BTreeMap<String, DateTime<Utc>> = BTreeMap::new();
    let mut assignments = Vec::new();
    let mut unplaced = Vec::new();
    let mut visited: BTreeSet<String> = BTreeSet::new();

    while !remaining_tasks.is_empty() {
        let mut ready = Vec::new();
        for task_id in &remaining_tasks {
            let mut blocked = false;
            for dependency in world.dependencies_of(task_id) {
                let predecessor_id = &dependency.predecessor_id;
                if scope.contains(predecessor_id.as_str()) {
                    if !scheduled_end.contains_key(predecessor_id)
                        && !matches!(
                            world.task(predecessor_id).map(|task| &task.status),
                            Some(crate::model::TaskStatus::Done)
                                | Some(crate::model::TaskStatus::Cancelled)
                        )
                    {
                        blocked = true;
                        break;
                    }
                } else {
                    let predecessor = world.task(predecessor_id);
                    if predecessor.is_some_and(|task| {
                        !matches!(
                            task.status,
                            crate::model::TaskStatus::Done | crate::model::TaskStatus::Cancelled
                        ) && {
                            let required = task
                                .remaining_minutes
                                .or(task.estimate_minutes)
                                .unwrap_or(0);
                            required == 0
                                || fixed_minutes.get(predecessor_id).copied().unwrap_or(0)
                                    < required
                        }
                    }) {
                        blocked = true;
                        break;
                    }
                }
            }
            if !blocked {
                ready.push(task_id.clone());
            }
        }
        if ready.is_empty() {
            for task_id in remaining_tasks.drain(..) {
                unplaced.push(UnplacedWork {
                    task_id: task_id.clone(),
                    minutes: world
                        .task(&task_id)
                        .and_then(|task| task.remaining_minutes.or(task.estimate_minutes))
                        .unwrap_or(0),
                    reason: "dependency_blocked".to_string(),
                });
                diagnostics.push(PlanDiagnostic {
                    code: "DEPENDENCY_BLOCKED".to_string(),
                    severity: "hard".to_string(),
                    message: "the task's predecessors are not completed or scheduled".to_string(),
                    task_id: Some(task_id),
                });
            }
            break;
        }
        ready.sort_by_key(|task_id| sort_key(world, task_id));
        let task_id = ready.remove(0);
        remaining_tasks.retain(|value| value != &task_id);
        visited.insert(task_id.clone());
        let task = world.task(&task_id).expect("validated task");
        let total = task
            .remaining_minutes
            .or(task.estimate_minutes)
            .unwrap_or(0);
        if total == 0 {
            diagnostics.push(PlanDiagnostic {
                code: "NO_REMAINING_WORK".to_string(),
                severity: "hard".to_string(),
                message: "task has no remaining minutes".to_string(),
                task_id: Some(task_id.clone()),
            });
            continue;
        }
        let already = fixed_minutes.get(&task_id).copied().unwrap_or(0).min(total);
        let mut remaining = total.saturating_sub(already);
        let earliest = effective_earliest(world, &task_id, input, now, &scheduled_end);
        if remaining == 0 {
            scheduled_end.insert(
                task_id.clone(),
                earliest.max(fixed_end.get(&task_id).copied().unwrap_or(earliest)),
            );
            continue;
        }
        let hard = task.hard_deadline;
        let mut chunk_start = earliest;
        let mut last_end = earliest.max(fixed_end.get(&task_id).copied().unwrap_or(earliest));
        while remaining > 0 {
            let Some((slot_index, start, end)) = choose_slot(
                &mut slots,
                task.preferred_window.as_ref(),
                chunk_start,
                hard,
                &input.horizon_end,
            ) else {
                break;
            };
            let available_minutes = (end - start).num_minutes().max(0) as u32;
            let take = available_minutes.min(remaining);
            if take < 5 {
                // Too small to be useful; consume this tail and try the next slot.
                slots[slot_index].capacity_left = 0;
                continue;
            }
            let allocation_end = start + Duration::minutes(i64::from(take));
            if let Some(hard) = hard {
                if allocation_end > hard {
                    break;
                }
            }
            slots[slot_index].capacity_left = slots[slot_index].capacity_left.saturating_sub(take);
            slots[slot_index].cursor = allocation_end;
            assignments.push(AllocationDraft {
                id: None,
                task_id: task_id.clone(),
                capacity_block_id: Some(slots[slot_index].capacity_block_id.clone()),
                start,
                end: allocation_end,
                minutes: take,
                locked: false,
            });
            remaining -= take;
            chunk_start = allocation_end;
            last_end = last_end.max(allocation_end);
            if slots[slot_index].capacity_left == 0 || allocation_end >= slots[slot_index].end {
                slots[slot_index].capacity_left = 0;
            }
            // Different capacity blocks may describe the same wall-clock time. A new
            // assignment reserves that interval globally, not only in its source block.
            reserve_interval(&mut slots, start, allocation_end);
        }
        if remaining > 0 {
            let reason = if hard.is_some_and(|deadline| deadline <= input.horizon_end) {
                "hard_deadline_or_capacity"
            } else {
                "insufficient_capacity"
            };
            unplaced.push(UnplacedWork {
                task_id: task_id.clone(),
                minutes: remaining,
                reason: reason.to_string(),
            });
            diagnostics.push(PlanDiagnostic {
                code: "UNPLACED_WORK".to_string(),
                severity: if hard.is_some() { "hard" } else { "soft" }.to_string(),
                message: format!("{remaining} minutes could not be placed"),
                task_id: Some(task_id.clone()),
            });
        }
        // A dependency is satisfied only once all remaining work has a scheduled finish.
        // A partial placement must not release successors into the leftover capacity.
        if remaining == 0 {
            scheduled_end.insert(task_id.clone(), last_end);
        }
    }

    if visited.len() < replace_task_ids.len() {
        // Tasks never reached because they were neither ready nor cleaned up above.
        for task_id in &replace_task_ids {
            if !visited.contains(task_id.as_str())
                && !unplaced.iter().any(|item| item.task_id == *task_id)
            {
                let minutes = world
                    .task(task_id)
                    .and_then(|task| task.remaining_minutes.or(task.estimate_minutes))
                    .unwrap_or(0);
                unplaced.push(UnplacedWork {
                    task_id: task_id.clone(),
                    minutes,
                    reason: "dependency_blocked".to_string(),
                });
            }
        }
    }

    assignments.sort_by(|left, right| {
        left.start
            .cmp(&right.start)
            .then_with(|| left.task_id.cmp(&right.task_id))
    });
    Ok(PlanOutput {
        assignments,
        unplaced,
        fixed_allocations,
        diagnostics,
        // This field authorizes deletion at apply time, not merely the planning scope.
        // Retain mode is additive, including when every task is already fully scheduled.
        replace_task_ids: if input.move_unlocked {
            replace_task_ids
        } else {
            Vec::new()
        },
    })
}

/// Remove a newly assigned interval from all remaining slots. Splitting preserves free
/// time on both sides, while distributing (never duplicating) each slot's minute budget.
/// The source slot has already consumed its minutes and advanced its cursor.
fn reserve_interval(slots: &mut Vec<Slot>, start: DateTime<Utc>, end: DateTime<Utc>) {
    let mut available = Vec::with_capacity(slots.len());
    for slot in slots.drain(..) {
        if slot.capacity_left == 0 || slot.cursor >= slot.end {
            continue;
        }
        if end <= slot.cursor || start >= slot.end {
            available.push(slot);
            continue;
        }
        let mut budget = slot.capacity_left;
        for (free_start, free_end) in [
            (slot.cursor, start.min(slot.end)),
            (end.max(slot.cursor), slot.end),
        ] {
            let minutes = (free_end - free_start).num_minutes().max(0) as u32;
            let usable = budget.min(minutes);
            if usable == 0 {
                continue;
            }
            available.push(Slot {
                capacity_block_id: slot.capacity_block_id.clone(),
                start: free_start,
                end: free_start + Duration::minutes(i64::from(usable)),
                cursor: free_start,
                capacity_left: usable,
            });
            budget -= usable;
        }
    }
    available.sort_by_key(|slot| slot.start);
    *slots = available;
}

fn sort_key(
    world: &World,
    task_id: &str,
) -> (bool, Option<DateTime<Utc>>, String, i32, i64, String) {
    let task = world.task(task_id).expect("validated task");
    (
        task.hard_deadline.is_none(),
        task.hard_deadline,
        task.target_due
            .map(|date| date.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "9999-12-31".to_string()),
        -task.priority,
        task.created_at.timestamp_millis(),
        task.id.clone(),
    )
}

fn effective_earliest(
    world: &World,
    task_id: &str,
    input: &PlanInput,
    now: DateTime<Utc>,
    scheduled_end: &BTreeMap<String, DateTime<Utc>>,
) -> DateTime<Utc> {
    let task = world.task(task_id).expect("validated task");
    let mut earliest = input.horizon_start.max(now);
    if let Some(not_before) = task.not_before {
        earliest = earliest.max(not_before);
    }
    for dependency in world.dependencies_of(task_id) {
        if let Some(end) = scheduled_end.get(&dependency.predecessor_id) {
            earliest = earliest.max(*end);
        } else {
            let last = world
                .allocations
                .iter()
                .filter(|allocation| allocation.task_id == dependency.predecessor_id)
                .map(|allocation| allocation.end)
                .max();
            if let Some(end) = last {
                earliest = earliest.max(end);
            }
        }
    }
    earliest
}

fn build_slots(
    world: &World,
    input: &PlanInput,
    now: DateTime<Utc>,
    removed_allocations: &BTreeSet<String>,
    _scope: &BTreeSet<&str>,
) -> Vec<Slot> {
    let mut slots = Vec::new();
    let mut blocks: Vec<_> = world.capacity_blocks.iter().collect();
    blocks.sort_by_key(|block| block.start);
    for block in blocks {
        let start = block.start.max(input.horizon_start).max(now);
        let end = block.end.min(input.horizon_end);
        if end <= start {
            continue;
        }
        let mut blocked = Vec::new();
        for commitment in &world.commitments {
            if commitment.end > start && commitment.start < end {
                blocked.push((commitment.start.max(start), commitment.end.min(end)));
            }
        }
        for event in &world.calendar_events {
            if event.busy && event.end > start && event.start < end {
                blocked.push((event.start.max(start), event.end.min(end)));
            }
        }
        for allocation in &world.allocations {
            if removed_allocations.contains(&allocation.id) {
                continue;
            }
            if allocation.end > start && allocation.start < end {
                blocked.push((allocation.start.max(start), allocation.end.min(end)));
            }
        }
        blocked.sort();
        let mut cursor = start;
        let mut free = Vec::new();
        for (blocked_start, blocked_end) in blocked {
            if blocked_start > cursor {
                free.push((cursor, blocked_start));
            }
            if blocked_end > cursor {
                cursor = blocked_end;
            }
        }
        if cursor < end {
            free.push((cursor, end));
        }
        // Match allocation validation: retained allocations consume the block's minute
        // budget as well as its wall-clock availability, including historical allocations.
        let used: u32 = world
            .allocations
            .iter()
            .filter(|allocation| {
                allocation.capacity_block_id.as_deref() == Some(block.id.as_str())
                    && !removed_allocations.contains(&allocation.id)
            })
            .map(|allocation| allocation.minutes)
            .sum();
        let mut capacity_left = block.capacity_minutes.saturating_sub(used);
        for (free_start, free_end) in free {
            for (allowed_start, allowed_end) in
                workspace_allowed_ranges(world, free_start, free_end)
            {
                let minutes = (allowed_end - allowed_start).num_minutes().max(0) as u32;
                let usable = capacity_left.min(minutes);
                if usable == 0 {
                    break;
                }
                let usable_end = allowed_start + Duration::minutes(i64::from(usable));
                slots.push(Slot {
                    capacity_block_id: block.id.clone(),
                    start: allowed_start,
                    end: usable_end,
                    cursor: allowed_start,
                    capacity_left: usable,
                });
                capacity_left -= usable;
                if capacity_left == 0 {
                    break;
                }
            }
            if capacity_left == 0 {
                break;
            }
        }
    }
    slots.sort_by_key(|slot| slot.start);
    slots
}

/// Hard work windows and weekend policy are availability constraints. Soft windows are only a
/// preference and never silently become hard exclusions.
pub(crate) fn workspace_allowed_ranges(
    world: &World,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
    let settings = &world.workspace.settings;
    let hard_windows: Vec<_> = settings
        .work_windows
        .iter()
        .filter(|window| window.hard)
        .collect();
    if hard_windows.is_empty()
        && !matches!(settings.weekend_policy, crate::model::WeekendPolicy::Deny)
    {
        return vec![(start, end)];
    }
    let Ok(timezone) = settings.timezone.parse::<chrono_tz::Tz>() else {
        return vec![(start, end)];
    };
    let mut ranges = Vec::new();
    let mut cursor = start;
    let mut range_start: Option<DateTime<Utc>> = None;
    while cursor < end {
        let local = cursor.with_timezone(&timezone);
        let weekday = local.weekday().number_from_monday() as u8;
        let minute = (local.hour() * 60 + local.minute()) as u16;
        let weekend_denied =
            matches!(settings.weekend_policy, crate::model::WeekendPolicy::Deny) && weekday >= 6;
        let hard_window_ok = hard_windows.is_empty()
            || hard_windows.iter().any(|window| {
                window.weekday == weekday
                    && window.start_minute <= minute
                    && minute < window.end_minute
            });
        let allowed = !weekend_denied && hard_window_ok;
        match (allowed, range_start) {
            (true, None) => range_start = Some(cursor),
            (false, Some(range_begin)) => {
                ranges.push((range_begin, cursor));
                range_start = None;
            }
            _ => {}
        }
        cursor += Duration::minutes(1);
    }
    if let Some(range_begin) = range_start {
        ranges.push((range_begin, end));
    }
    ranges
}

fn choose_slot(
    slots: &mut [Slot],
    preferred: Option<&crate::model::TimeWindow>,
    not_before: DateTime<Utc>,
    hard: Option<DateTime<Utc>>,
    horizon_end: &DateTime<Utc>,
) -> Option<(usize, DateTime<Utc>, DateTime<Utc>)> {
    let mut candidates: Vec<(usize, DateTime<Utc>, DateTime<Utc>, usize)> = Vec::new();
    for (index, slot) in slots.iter().enumerate() {
        if slot.capacity_left == 0 {
            continue;
        }
        let start = slot.cursor.max(not_before);
        if start >= slot.end {
            continue;
        }
        // capacity_left is the real remaining budget; the wall-clock slot alone would allow
        // double-booking when two tasks share one capacity block.
        let end = (start + Duration::minutes(i64::from(slot.capacity_left))).min(slot.end);
        if end <= start {
            continue;
        }
        if let Some(hard) = hard {
            if start >= hard {
                continue;
            }
            if end > hard {
                // A partial interval before the hard deadline is still useful.
                let clipped = end.min(hard);
                if clipped <= start {
                    continue;
                }
                candidates.push((
                    index,
                    start,
                    clipped,
                    if preferred
                        .is_some_and(|window| window.start <= start && clipped <= window.end)
                    {
                        0
                    } else {
                        1
                    },
                ));
                continue;
            }
        }
        if end > *horizon_end {
            continue;
        }
        let preference_rank =
            if preferred.is_some_and(|window| window.start <= start && end <= window.end) {
                0
            } else {
                1
            };
        candidates.push((index, start, end, preference_rank));
    }
    candidates.sort_by_key(|(index, start, _end, rank)| (*rank, *start, *index));
    let (index, start, end, _) = candidates.first().copied()?;
    let slot = &mut slots[index];
    if start > slot.start {
        // The prefix was occupied by a hard lower bound; consuming it is not needed because the
        // lower bound applies to this task, not the block. Keep the allocation start at `start`.
    }
    let capacity_end = (start + Duration::minutes(i64::from(slot.capacity_left))).min(slot.end);
    Some((index, start, end.min(capacity_end)))
}
