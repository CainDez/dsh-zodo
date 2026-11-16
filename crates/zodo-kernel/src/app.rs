use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::{Value, json};

use crate::apply::{ApplyOutcome, apply_change, mark_activity};
use crate::commands::{
    BusyEvent, CommandEnvelope, CommandPayload, CommandResult, ParsedCommand, ParsedQuery,
    QueryEnvelope, QueryPayload, QueryResult, ResultStatus, TaskFilter,
};
use crate::error::{ErrorCode, Result, ZodoError};
use crate::model::{
    ActivityKind, ActivityRecord, AllocationState, ApprovalGrant, CalendarEvent, CalendarSource,
    FocusSegment, FocusSession, FocusState, GoalStatus, Habit, InboxEntry, InboxStatus, Milestone,
    OccurrenceStatus, Proposal, ProposalStatus, Review, ReviewFacts, SavedQuery, SourceRef, Tag,
    Task, TaskStatus, World,
};
use crate::ops::{ChangeOp, ProjectSpec};
use crate::planner::{PlanInput, PlanOutput, plan};
use crate::ports::{
    Clock, IdSource, NewAudit, NewEffect, NewEvent, ReceiptInsert, SaveOptions, Store,
};
use crate::proposal::{apply_operations, preview_diff, proposal_content_hash, proposal_effects};
use crate::recurrence;

pub struct Application<'a> {
    pub store: &'a mut dyn Store,
    pub clock: &'a dyn Clock,
    pub ids: &'a dyn IdSource,
    pub instance_id: String,
}

impl<'a> Application<'a> {
    pub fn execute(
        &mut self,
        ctx: &crate::model::CommandContext,
        envelope: CommandEnvelope,
    ) -> Result<CommandResult> {
        if envelope.workspace_id != ctx.workspace_id {
            return Err(ZodoError::new(
                ErrorCode::PermissionDenied,
                "workspace is outside this authenticated connection",
            ));
        }
        let parsed = envelope.parse()?;
        let op_name = parsed.payload.operation_name();
        authorize(ctx, &parsed.payload)?;
        let request_id = parsed.envelope.request_id.clone();
        let idempotency_key = parsed.envelope.idempotency_key.clone();
        if let Some(existing) =
            self.store
                .find_receipt(&ctx.workspace_id, &ctx.principal_id, &idempotency_key)?
        {
            if existing.request_hash != parsed.canonical_hash {
                return Err(ZodoError::new(
                    ErrorCode::IdempotencyMismatch,
                    "the same idempotency key was used for different content",
                )
                .with_details(json!({
                    "idempotency_key": idempotency_key,
                    "operation_id": existing.operation_id,
                })));
            }
            return Ok(CommandResult::new(
                request_id,
                existing.operation_id,
                existing.status,
                existing.revision,
                existing.data,
                existing.event_cursor,
                true,
            ));
        }

        let mut world = self
            .store
            .load_world(&ctx.workspace_id)?
            .ok_or_else(|| ZodoError::not_found("workspace", &ctx.workspace_id))?;
        check_versions(&parsed, &world)?;
        let now = self.clock.now();
        let operation_id = self.ids.next("op");
        let mut outcome = ApplyOutcome::default();
        let mut bump = true;
        let (status, data, event_type) = match parsed.payload {
            CommandPayload::WorkspaceRename { name } => {
                ctx.require("admin")?;
                world.workspace.name = name.trim().to_string();
                world.workspace.updated_at = now;
                (
                    ResultStatus::Committed,
                    json!({"workspace": world.workspace}),
                    "workspace.changed",
                )
            }
            CommandPayload::WorkspaceUpdate { settings, policy } => {
                ctx.require("admin")?;
                if let Some(settings) = settings {
                    world.workspace.settings = settings;
                }
                if let Some(policy) = policy {
                    if policy.allowed_cloud_providers
                        != world.workspace.policy.allowed_cloud_providers
                        || policy.model_route != world.workspace.policy.model_route
                        || policy.private_notes != world.workspace.policy.private_notes
                    {
                        world.workspace.policy_revision =
                            world.workspace.policy_revision.saturating_add(1);
                    }
                    world.workspace.policy = policy;
                }
                world.workspace.updated_at = now;
                (
                    ResultStatus::Committed,
                    json!({"workspace": world.workspace}),
                    "workspace.changed",
                )
            }
            CommandPayload::CaptureCollect { text, source_kind } => {
                let id = self.ids.next("inb");
                let entry = InboxEntry {
                    id: id.clone(),
                    workspace_id: ctx.workspace_id.clone(),
                    text,
                    source: SourceRef {
                        kind: source_kind.unwrap_or_else(|| "user".to_string()),
                        id: None,
                        actor_id: Some(ctx.principal_id.clone()),
                    },
                    status: InboxStatus::Unprocessed,
                    created_at: now,
                    updated_at: now,
                    clarifications: Vec::new(),
                    converted_refs: Vec::new(),
                    dismissed_reason: None,
                    version: 1,
                };
                world.inbox.push(entry.clone());
                outcome.record("inbox", &id, "created");
                (
                    ResultStatus::Committed,
                    json!({"inbox_entry": entry}),
                    "inbox.changed",
                )
            }
            CommandPayload::CaptureClarify {
                id,
                question,
                answer,
            } => {
                let entry = world
                    .inbox
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| ZodoError::not_found("inbox", &id))?;
                let clarification_id = self.ids.next("clr");
                entry.clarifications.push(crate::model::Clarification {
                    id: clarification_id.clone(),
                    question,
                    answer: answer.clone(),
                    asked_at: now,
                    answered_at: answer.map(|_| now),
                });
                entry.status = if entry
                    .clarifications
                    .iter()
                    .all(|item| item.answer.is_some())
                {
                    InboxStatus::Unprocessed
                } else {
                    InboxStatus::NeedsClarification
                };
                entry.updated_at = now;
                entry.version += 1;
                let value = entry.clone();
                outcome.record("inbox", &id, "clarified");
                (
                    ResultStatus::Committed,
                    json!({"inbox_entry": value}),
                    "inbox.changed",
                )
            }
            CommandPayload::CaptureConvert {
                id,
                tasks,
                goal,
                project,
            } => {
                let entry = world
                    .inbox_entry(&id)
                    .ok_or_else(|| ZodoError::not_found("inbox", &id))?
                    .clone();
                if matches!(
                    entry.status,
                    InboxStatus::Processed | InboxStatus::Dismissed
                ) {
                    return Err(ZodoError::validation(
                        "this inbox entry was already processed",
                    ));
                }
                if tasks.is_empty() && goal.is_none() && project.is_none() {
                    return Err(ZodoError::validation(
                        "conversion needs at least one target",
                    ));
                }
                let mut created = Vec::new();
                let mut goal_id = None;
                let mut project_id = None;
                if let Some(goal) = goal {
                    let operation = ChangeOp::GoalCreate { goal };
                    apply_change(
                        &mut world,
                        &operation,
                        &ctx.principal_id,
                        &ctx.channel,
                        now,
                        self.ids,
                        &mut outcome,
                    )?;
                    goal_id = outcome
                        .changes
                        .iter()
                        .rev()
                        .find(|change| change.kind == "goal")
                        .map(|change| change.id.clone());
                }
                if let Some(project) = project {
                    let project = ProjectSpec {
                        goal_id: project.goal_id.or(goal_id.clone()),
                        ..project
                    };
                    let operation = ChangeOp::ProjectCreate { project };
                    apply_change(
                        &mut world,
                        &operation,
                        &ctx.principal_id,
                        &ctx.channel,
                        now,
                        self.ids,
                        &mut outcome,
                    )?;
                    project_id = outcome
                        .changes
                        .iter()
                        .rev()
                        .find(|change| change.kind == "project")
                        .map(|change| change.id.clone());
                }
                for mut task in tasks {
                    if task.project_id.is_none() {
                        task.project_id = project_id.clone();
                    }
                    let operation = ChangeOp::TaskCreate { task };
                    apply_change(
                        &mut world,
                        &operation,
                        &ctx.principal_id,
                        &ctx.channel,
                        now,
                        self.ids,
                        &mut outcome,
                    )?;
                    if let Some(change) = outcome
                        .changes
                        .iter()
                        .rev()
                        .find(|change| change.kind == "task")
                    {
                        created.push(change.id.clone());
                    }
                }
                let entry = world
                    .inbox
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .expect("entry exists");
                entry.status = InboxStatus::Processed;
                entry.updated_at = now;
                entry.version += 1;
                entry.converted_refs = created
                    .iter()
                    .map(|id| crate::model::EntityRef {
                        kind: "task".to_string(),
                        id: id.clone(),
                    })
                    .collect();
                if let Some(goal_id) = &goal_id {
                    entry.converted_refs.push(crate::model::EntityRef {
                        kind: "goal".to_string(),
                        id: goal_id.clone(),
                    });
                }
                if let Some(project_id) = &project_id {
                    entry.converted_refs.push(crate::model::EntityRef {
                        kind: "project".to_string(),
                        id: project_id.clone(),
                    });
                }
                let value = entry.clone();
                outcome.record("inbox", &id, "processed");
                (
                    ResultStatus::Committed,
                    json!({
                        "inbox_entry": value,
                        "task_ids": created,
                        "goal_id": goal_id,
                        "project_id": project_id,
                    }),
                    "inbox.changed",
                )
            }
            CommandPayload::CaptureDismiss { id, reason } => {
                let entry = world
                    .inbox
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| ZodoError::not_found("inbox", &id))?;
                entry.status = InboxStatus::Dismissed;
                entry.dismissed_reason = reason;
                entry.updated_at = now;
                entry.version += 1;
                let value = entry.clone();
                outcome.record("inbox", &id, "dismissed");
                (
                    ResultStatus::Committed,
                    json!({"inbox_entry": value}),
                    "inbox.changed",
                )
            }
            CommandPayload::ReminderMarkDelivered {
                id,
                delivered_at,
                failure,
            } => {
                let reminder = world
                    .reminders
                    .iter_mut()
                    .find(|reminder| reminder.id == id)
                    .ok_or_else(|| ZodoError::not_found("reminder", &id))?;
                if matches!(reminder.status, crate::model::ReminderStatus::Cancelled) {
                    return Err(ZodoError::validation(
                        "a cancelled reminder cannot be marked delivered",
                    ));
                }
                if let Some(failure) = failure {
                    reminder.status = crate::model::ReminderStatus::Failed;
                    reminder.failure = Some(failure);
                } else {
                    reminder.status = crate::model::ReminderStatus::Delivered;
                    reminder.delivered_at = Some(delivered_at.unwrap_or(now));
                }
                reminder.updated_at = now;
                let value = reminder.clone();
                outcome.record("reminder", &id, "delivery_recorded");
                (
                    ResultStatus::Committed,
                    json!({"reminder": value}),
                    "reminder.delivered",
                )
            }
            CommandPayload::HabitCreate { habit } => {
                if world
                    .habits
                    .iter()
                    .any(|existing| existing.title.eq_ignore_ascii_case(habit.title.trim()))
                {
                    return Err(ZodoError::validation(
                        "a habit with this title already exists",
                    ));
                }
                let id = self.ids.next("hab");
                let value = Habit {
                    id: id.clone(),
                    workspace_id: ctx.workspace_id.clone(),
                    title: habit.title.trim().to_string(),
                    timezone: habit.timezone,
                    start_date: habit.start_date,
                    end_date: habit.end_date,
                    weekly_target: habit.weekly_target,
                    scheduled_weekdays: habit.scheduled_weekdays,
                    time_of_day: habit.time_of_day,
                    created_at: now,
                    updated_at: now,
                    version: 1,
                };
                world.habits.push(value.clone());
                outcome.record("habit", &id, "created");
                (
                    ResultStatus::Committed,
                    json!({"habit": value}),
                    "habit.changed",
                )
            }
            CommandPayload::HabitUpdate { id, habit } => {
                let target = world
                    .habits
                    .iter_mut()
                    .find(|value| value.id == id)
                    .ok_or_else(|| ZodoError::not_found("habit", &id))?;
                target.title = habit.title.trim().to_string();
                target.timezone = habit.timezone;
                target.start_date = habit.start_date;
                target.end_date = habit.end_date;
                target.weekly_target = habit.weekly_target;
                target.scheduled_weekdays = habit.scheduled_weekdays;
                target.time_of_day = habit.time_of_day;
                target.updated_at = now;
                target.version += 1;
                let value = target.clone();
                outcome.record("habit", &id, "updated");
                (
                    ResultStatus::Committed,
                    json!({"habit": value}),
                    "habit.changed",
                )
            }
            CommandPayload::HabitSkip {
                habit_id,
                date,
                note,
            } => {
                let habit = world
                    .habits
                    .iter()
                    .find(|value| value.id == habit_id)
                    .cloned()
                    .ok_or_else(|| ZodoError::not_found("habit", &habit_id))?;
                if date < habit.start_date || habit.end_date.is_some_and(|end| date > end) {
                    return Err(ZodoError::validation(
                        "skip date must fall inside the habit window",
                    ));
                }
                let dedupe_key = format!("skip:{}:{}", habit_id, date);
                if world
                    .activities
                    .iter()
                    .any(|activity| activity.dedupe_key.as_deref() == Some(dedupe_key.as_str()))
                {
                    return Err(ZodoError::validation("this habit day is already skipped"));
                }
                let id = self.ids.next("act");
                let activity = ActivityRecord {
                    id: id.clone(),
                    workspace_id: ctx.workspace_id.clone(),
                    kind: ActivityKind::HabitSkip,
                    task_id: None,
                    habit_id: Some(habit_id.clone()),
                    minutes: 0,
                    occurred_at: date
                        .and_hms_opt(0, 0, 0)
                        .expect("midnight is valid")
                        .and_utc(),
                    note,
                    source: crate::apply::source_for(&ctx.principal_id, &ctx.channel),
                    dedupe_key: Some(dedupe_key),
                    created_at: now,
                };
                world.activities.push(activity.clone());
                outcome.record("habit", &habit_id, "skipped");
                (
                    ResultStatus::Committed,
                    json!({"activity": activity, "habit": habit, "skipped": true}),
                    "habit.changed",
                )
            }
            CommandPayload::HabitUndo {
                habit_id,
                activity_id,
            } => {
                let position = match activity_id {
                    Some(activity_id) => world.activities.iter().rposition(|activity| {
                        activity.id == activity_id
                            && activity.habit_id.as_deref() == Some(habit_id.as_str())
                            && matches!(
                                activity.kind,
                                ActivityKind::Habit | ActivityKind::HabitSkip
                            )
                    }),
                    None => world.activities.iter().rposition(|activity| {
                        activity.habit_id.as_deref() == Some(habit_id.as_str())
                            && matches!(
                                activity.kind,
                                ActivityKind::Habit | ActivityKind::HabitSkip
                            )
                    }),
                }
                .ok_or_else(|| ZodoError::not_found("habit activity", &habit_id))?;
                let removed = world.activities.remove(position);
                outcome.record("habit", &habit_id, "undone");
                (
                    ResultStatus::Committed,
                    json!({"activity": removed, "undone": true}),
                    "habit.changed",
                )
            }
            CommandPayload::HabitLog {
                habit_id,
                occurred_at,
                minutes,
                note,
                dedupe_key,
            } => {
                let habit = world
                    .habits
                    .iter()
                    .find(|value| value.id == habit_id)
                    .cloned()
                    .ok_or_else(|| ZodoError::not_found("habit", &habit_id))?;
                if let Some(end_date) = habit.end_date {
                    let occurred_date = occurred_at.unwrap_or(now).date_naive();
                    if occurred_date > end_date {
                        return Err(ZodoError::validation(
                            "habit has ended; its log cannot be extended",
                        ));
                    }
                }
                if let Some(key) = &dedupe_key {
                    if world
                        .activities
                        .iter()
                        .any(|activity| activity.dedupe_key.as_deref() == Some(key.as_str()))
                    {
                        return Err(ZodoError::validation("this habit log was already recorded"));
                    }
                }
                let id = self.ids.next("act");
                let activity = ActivityRecord {
                    id: id.clone(),
                    workspace_id: ctx.workspace_id.clone(),
                    kind: ActivityKind::Habit,
                    task_id: None,
                    habit_id: Some(habit_id.clone()),
                    minutes,
                    occurred_at: occurred_at.unwrap_or(now),
                    note,
                    source: crate::apply::source_for(&ctx.principal_id, &ctx.channel),
                    dedupe_key,
                    created_at: now,
                };
                world.activities.push(activity.clone());
                outcome.record("habit", &habit_id, "logged");
                (
                    ResultStatus::Committed,
                    json!({"activity": activity, "habit": habit}),
                    "habit.changed",
                )
            }
            CommandPayload::TagCreate { tag } => {
                let name = tag.name.trim();
                if world
                    .tags
                    .iter()
                    .any(|existing| existing.name.eq_ignore_ascii_case(name))
                {
                    return Err(ZodoError::validation("this tag already exists"));
                }
                let id = self.ids.next("tag");
                let value = Tag {
                    id: id.clone(),
                    workspace_id: ctx.workspace_id.clone(),
                    name: name.to_string(),
                    color: tag.color,
                    created_at: now,
                };
                world.tags.push(value.clone());
                outcome.record("tag", &id, "created");
                (
                    ResultStatus::Committed,
                    json!({"tag": value}),
                    "tag.changed",
                )
            }
            CommandPayload::TagUpdate { id, tag } => {
                let target = world
                    .tags
                    .iter_mut()
                    .find(|value| value.id == id)
                    .ok_or_else(|| ZodoError::not_found("tag", &id))?;
                target.name = tag.name.trim().to_string();
                target.color = tag.color;
                let value = target.clone();
                outcome.record("tag", &id, "updated");
                (
                    ResultStatus::Committed,
                    json!({"tag": value}),
                    "tag.changed",
                )
            }
            CommandPayload::TagDelete { id } => {
                let removed = world
                    .tags
                    .iter()
                    .find(|value| value.id == id)
                    .cloned()
                    .ok_or_else(|| ZodoError::not_found("tag", &id))?;
                world.tags.retain(|value| value.id != id);
                for task in &mut world.tasks {
                    task.tags.retain(|tag| tag != &removed.name);
                }
                outcome.record("tag", &id, "deleted");
                (
                    ResultStatus::Committed,
                    json!({"deleted": id}),
                    "tag.changed",
                )
            }
            CommandPayload::SavedQueryCreate { saved_query } => {
                let id = self.ids.next("viw");
                let value = SavedQuery {
                    id: id.clone(),
                    workspace_id: ctx.workspace_id.clone(),
                    name: saved_query.name.trim().to_string(),
                    filter: saved_query.filter,
                    created_at: now,
                    updated_at: now,
                };
                world.saved_queries.push(value.clone());
                outcome.record("saved_query", &id, "created");
                (
                    ResultStatus::Committed,
                    json!({"saved_query": value}),
                    "saved_query.changed",
                )
            }
            CommandPayload::SavedQueryUpdate { id, saved_query } => {
                let target = world
                    .saved_queries
                    .iter_mut()
                    .find(|value| value.id == id)
                    .ok_or_else(|| ZodoError::not_found("saved_query", &id))?;
                target.name = saved_query.name.trim().to_string();
                target.filter = saved_query.filter;
                target.updated_at = now;
                let value = target.clone();
                outcome.record("saved_query", &id, "updated");
                (
                    ResultStatus::Committed,
                    json!({"saved_query": value}),
                    "saved_query.changed",
                )
            }
            CommandPayload::SavedQueryDelete { id } => {
                world
                    .saved_queries
                    .iter()
                    .find(|value| value.id == id)
                    .ok_or_else(|| ZodoError::not_found("saved_query", &id))?;
                world.saved_queries.retain(|value| value.id != id);
                outcome.record("saved_query", &id, "deleted");
                (
                    ResultStatus::Committed,
                    json!({"deleted": id}),
                    "saved_query.changed",
                )
            }
            CommandPayload::MilestoneAdd {
                project_id,
                milestone,
            } => {
                let project = world
                    .projects
                    .iter_mut()
                    .find(|value| value.id == project_id)
                    .ok_or_else(|| ZodoError::not_found("project", &project_id))?;
                let value = Milestone {
                    id: self.ids.next("mil"),
                    title: milestone.title.trim().to_string(),
                    due: milestone.due,
                    done: milestone.done,
                };
                project.milestones.push(value.clone());
                project.updated_at = now;
                project.version += 1;
                outcome.record("project", &project_id, "milestone_added");
                (
                    ResultStatus::Committed,
                    json!({"milestone": value, "project_id": project_id}),
                    "project.changed",
                )
            }
            CommandPayload::MilestoneUpdate {
                project_id,
                milestone_id,
                milestone,
            } => {
                let project = world
                    .projects
                    .iter_mut()
                    .find(|value| value.id == project_id)
                    .ok_or_else(|| ZodoError::not_found("project", &project_id))?;
                let target = project
                    .milestones
                    .iter_mut()
                    .find(|value| value.id == milestone_id)
                    .ok_or_else(|| ZodoError::not_found("milestone", &milestone_id))?;
                target.title = milestone.title.trim().to_string();
                target.due = milestone.due;
                target.done = milestone.done;
                let value = target.clone();
                project.updated_at = now;
                project.version += 1;
                outcome.record("project", &project_id, "milestone_updated");
                (
                    ResultStatus::Committed,
                    json!({"milestone": value, "project_id": project_id}),
                    "project.changed",
                )
            }
            CommandPayload::MilestoneRemove {
                project_id,
                milestone_id,
            } => {
                let project = world
                    .projects
                    .iter_mut()
                    .find(|value| value.id == project_id)
                    .ok_or_else(|| ZodoError::not_found("project", &project_id))?;
                project.milestones.retain(|value| value.id != milestone_id);
                project.updated_at = now;
                project.version += 1;
                outcome.record("project", &project_id, "milestone_removed");
                (
                    ResultStatus::Committed,
                    json!({"project_id": project_id, "milestone_id": milestone_id}),
                    "project.changed",
                )
            }
            CommandPayload::TaskBatchUpdate {
                ids,
                status,
                archived,
                add_tags,
                remove_tags,
                preview,
            } => {
                for id in &ids {
                    if world.task(id).is_none() {
                        return Err(ZodoError::not_found("task", id));
                    }
                }
                if preview {
                    return Ok(CommandResult::new(
                        request_id,
                        operation_id,
                        ResultStatus::Validated,
                        world.workspace.revision,
                        json!({
                            "preview": true,
                            "affected": ids.len(),
                            "status": status,
                            "archived": archived,
                            "add_tags": add_tags,
                            "remove_tags": remove_tags,
                        }),
                        self.store.latest_cursor(&ctx.workspace_id)?,
                        false,
                    ));
                }
                for id in &ids {
                    if let Some(status) = &status {
                        apply_change(
                            &mut world,
                            &ChangeOp::TaskStatus {
                                id: id.clone(),
                                status: status.clone(),
                                note: None,
                            },
                            &ctx.principal_id,
                            &ctx.channel,
                            now,
                            self.ids,
                            &mut outcome,
                        )?;
                    }
                    if let Some(archived) = archived {
                        apply_change(
                            &mut world,
                            &ChangeOp::TaskArchive {
                                id: id.clone(),
                                archived,
                            },
                            &ctx.principal_id,
                            &ctx.channel,
                            now,
                            self.ids,
                            &mut outcome,
                        )?;
                    }
                    if !add_tags.is_empty() || !remove_tags.is_empty() {
                        let mut tags = world
                            .task(id)
                            .map(|task| task.tags.clone())
                            .unwrap_or_default();
                        for tag in &add_tags {
                            if !tags.iter().any(|value| value == tag) {
                                tags.push(tag.clone());
                            }
                        }
                        tags.retain(|tag| !remove_tags.iter().any(|value| value == tag));
                        apply_change(
                            &mut world,
                            &ChangeOp::TaskSetTags {
                                task_id: id.clone(),
                                tags,
                            },
                            &ctx.principal_id,
                            &ctx.channel,
                            now,
                            self.ids,
                            &mut outcome,
                        )?;
                    }
                }
                let data = json!({
                    "preview": false,
                    "affected": ids.len(),
                    "changes": outcome.changes,
                    "warnings": outcome.warnings,
                });
                (ResultStatus::Committed, data, "task.batch_changed")
            }
            CommandPayload::TaskMerge {
                survivor_id,
                duplicate_ids,
            } => {
                if world.task(&survivor_id).is_none() {
                    return Err(ZodoError::not_found("task", &survivor_id));
                }
                let mut duplicate_set = std::collections::BTreeSet::new();
                for id in &duplicate_ids {
                    if world.task(id).is_none() {
                        return Err(ZodoError::not_found("task", id));
                    }
                    duplicate_set.insert(id.clone());
                }
                if world
                    .allocations
                    .iter()
                    .any(|allocation| duplicate_set.contains(&allocation.task_id))
                {
                    return Err(ZodoError::validation(
                        "cannot merge a task with allocations; preserve or move history explicitly first",
                    ));
                }
                let mut merged_tags = Vec::new();
                for id in &duplicate_set {
                    if let Some(task) = world.task(id) {
                        for tag in &task.tags {
                            if !merged_tags.contains(tag) {
                                merged_tags.push(tag.clone());
                            }
                        }
                    }
                }
                if let Some(survivor) = world.task_mut(&survivor_id) {
                    for tag in merged_tags {
                        if !survivor.tags.contains(&tag) {
                            survivor.tags.push(tag);
                        }
                    }
                    for id in &duplicate_set {
                        survivor.tags.push(format!("merged_from:{id}"));
                    }
                    survivor.updated_at = now;
                    survivor.version = survivor.version.saturating_add(1);
                }
                // Re-point dependency edges and notes, then deduplicate/self-loop edges.
                for dependency in &mut world.dependencies {
                    if duplicate_set.contains(&dependency.predecessor_id) {
                        dependency.predecessor_id = survivor_id.clone();
                    }
                    if duplicate_set.contains(&dependency.successor_id) {
                        dependency.successor_id = survivor_id.clone();
                    }
                }
                let mut seen_edges = std::collections::BTreeSet::new();
                world.dependencies.retain(|dependency| {
                    if dependency.predecessor_id == dependency.successor_id {
                        return false;
                    }
                    seen_edges.insert((
                        dependency.predecessor_id.clone(),
                        dependency.successor_id.clone(),
                    ))
                });
                for note in &mut world.notes {
                    if note
                        .linked_tasks
                        .iter()
                        .any(|id| duplicate_set.contains(id))
                    {
                        note.linked_tasks.retain(|id| !duplicate_set.contains(id));
                        if !note.linked_tasks.contains(&survivor_id) {
                            note.linked_tasks.push(survivor_id.clone());
                        }
                        note.updated_at = now;
                        note.version = note.version.saturating_add(1);
                    }
                }
                for occurrence in &mut world.occurrences {
                    if occurrence
                        .task_id
                        .as_ref()
                        .is_some_and(|id| duplicate_set.contains(id))
                    {
                        occurrence.task_id = Some(survivor_id.clone());
                        occurrence.updated_at = now;
                    }
                }
                for id in &duplicate_set {
                    if let Some(task) = world.task_mut(id) {
                        task.archived = true;
                        task.updated_at = now;
                        task.version = task.version.saturating_add(1);
                        task.tags.push(format!("merged_into:{survivor_id}"));
                        outcome.record("task", id, "merged");
                    }
                }
                outcome.record("task", &survivor_id, "updated");
                let data = json!({
                    "survivor_id": survivor_id,
                    "merged_ids": duplicate_ids,
                    "links_updated": true,
                    "history_preserved": true,
                });
                (ResultStatus::Committed, data, "task.merged")
            }
            CommandPayload::AutomationCreate { automation } => {
                if world
                    .automation_rules
                    .iter()
                    .any(|rule| rule.name.eq_ignore_ascii_case(automation.name.trim()))
                {
                    return Err(ZodoError::validation(
                        "an automation with this name already exists",
                    ));
                }
                let id = self.ids.next("aut");
                let value = crate::model::AutomationRule {
                    id: id.clone(),
                    workspace_id: ctx.workspace_id.clone(),
                    name: automation.name.trim().to_string(),
                    trigger: automation.trigger,
                    conditions: automation.conditions,
                    actions: automation.actions,
                    enabled: automation.enabled,
                    budget: automation.budget,
                    runs: 0,
                    last_run_at: None,
                    created_at: now,
                };
                world.automation_rules.push(value.clone());
                outcome.record("automation_rule", &id, "created");
                (
                    ResultStatus::Committed,
                    json!({"automation": value}),
                    "automation.changed",
                )
            }
            CommandPayload::AutomationUpdate { id, automation } => {
                let target = world
                    .automation_rules
                    .iter_mut()
                    .find(|rule| rule.id == id)
                    .ok_or_else(|| ZodoError::not_found("automation", &id))?;
                target.name = automation.name.trim().to_string();
                target.trigger = automation.trigger;
                target.conditions = automation.conditions;
                target.actions = automation.actions;
                target.enabled = automation.enabled;
                target.budget = automation.budget;
                let value = target.clone();
                outcome.record("automation_rule", &id, "updated");
                (
                    ResultStatus::Committed,
                    json!({"automation": value}),
                    "automation.changed",
                )
            }
            CommandPayload::AutomationDelete { id } => {
                world
                    .automation_rules
                    .iter()
                    .find(|rule| rule.id == id)
                    .ok_or_else(|| ZodoError::not_found("automation", &id))?;
                world.automation_rules.retain(|rule| rule.id != id);
                outcome.record("automation_rule", &id, "deleted");
                (
                    ResultStatus::Committed,
                    json!({"deleted": id}),
                    "automation.changed",
                )
            }
            CommandPayload::AutomationToggle { id, enabled } => {
                let target = world
                    .automation_rules
                    .iter_mut()
                    .find(|rule| rule.id == id)
                    .ok_or_else(|| ZodoError::not_found("automation", &id))?;
                target.enabled = enabled;
                let value = target.clone();
                outcome.record("automation_rule", &id, "toggled");
                (
                    ResultStatus::Committed,
                    json!({"automation": value}),
                    "automation.changed",
                )
            }
            CommandPayload::AutomationRun { id } => {
                let rule = world
                    .automation_rules
                    .iter()
                    .find(|rule| rule.id == id)
                    .cloned()
                    .ok_or_else(|| ZodoError::not_found("automation", &id))?;
                if !rule.enabled {
                    return Err(ZodoError::validation("automation is disabled"));
                }
                if rule.runs >= rule.budget {
                    return Err(ZodoError::validation(
                        "automation budget is exhausted; re-enable after reviewing",
                    ));
                }
                let actions = rule
                    .actions
                    .as_array()
                    .cloned()
                    .ok_or_else(|| ZodoError::validation("automation actions are missing"))?;
                let mut created = Vec::new();
                for action in actions {
                    if action.get("kind").and_then(Value::as_str) != Some("task.create") {
                        return Err(ZodoError::new(
                            ErrorCode::UnsupportedOperation,
                            "only task.create automation actions are supported",
                        ));
                    }
                    let task: crate::ops::TaskSpec =
                        serde_json::from_value(action.get("task").cloned().ok_or_else(|| {
                            ZodoError::validation("automation action needs task")
                        })?)
                        .map_err(|error| {
                            ZodoError::validation(format!(
                                "invalid automation task action: {error}"
                            ))
                        })?;
                    apply_change(
                        &mut world,
                        &ChangeOp::TaskCreate { task },
                        &ctx.principal_id,
                        &ctx.channel,
                        now,
                        self.ids,
                        &mut outcome,
                    )?;
                    if let Some(change) = outcome
                        .changes
                        .iter()
                        .rev()
                        .find(|change| change.kind == "task")
                    {
                        created.push(change.id.clone());
                    }
                }
                let target = world
                    .automation_rules
                    .iter_mut()
                    .find(|rule| rule.id == id)
                    .expect("rule exists");
                target.runs += 1;
                target.last_run_at = Some(now);
                if target.runs >= target.budget {
                    target.enabled = false;
                }
                let value = target.clone();
                outcome.record("automation_rule", &id, "ran");
                (
                    ResultStatus::Committed,
                    json!({"automation": value, "task_ids": created, "note": "manual trigger only; conditions are recorded but not yet evaluated by a scheduler"}),
                    "automation.ran",
                )
            }
            CommandPayload::AttachmentAdd {
                note_id,
                location,
                media_type,
                data_class,
                sha256,
                summary,
            } => {
                if !world.notes.iter().any(|note| note.id == note_id) {
                    return Err(ZodoError::not_found("note", &note_id));
                }
                let attachment = crate::model::AttachmentRef {
                    id: self.ids.next("att"),
                    workspace_id: ctx.workspace_id.clone(),
                    note_id,
                    location,
                    media_type,
                    data_class,
                    sha256,
                    summary,
                    created_at: now,
                };
                let id = attachment.id.clone();
                world.attachments.push(attachment.clone());
                outcome.record("attachment", &id, "created");
                (
                    ResultStatus::Committed,
                    json!({"attachment": attachment}),
                    "attachment.created",
                )
            }
            CommandPayload::AttachmentRemove { id } => {
                let before = world.attachments.len();
                world.attachments.retain(|attachment| attachment.id != id);
                if world.attachments.len() == before {
                    return Err(ZodoError::not_found("attachment", &id));
                }
                outcome.record("attachment", &id, "removed");
                (
                    ResultStatus::Committed,
                    json!({"removed": id}),
                    "attachment.removed",
                )
            }
            command @ (CommandPayload::TaskCreate { .. }
            | CommandPayload::TaskUpdate { .. }
            | CommandPayload::TaskComplete { .. }
            | CommandPayload::TaskReopen { .. }
            | CommandPayload::TaskCancel { .. }
            | CommandPayload::TaskArchive { .. }
            | CommandPayload::TaskSetBlocked { .. }
            | CommandPayload::TaskSetTags { .. }
            | CommandPayload::GoalCreate { .. }
            | CommandPayload::GoalUpdate { .. }
            | CommandPayload::GoalStatus { .. }
            | CommandPayload::ProjectCreate { .. }
            | CommandPayload::ProjectUpdate { .. }
            | CommandPayload::ProjectStatus { .. }
            | CommandPayload::DependencyAdd { .. }
            | CommandPayload::DependencyRemove { .. }
            | CommandPayload::NoteCreate { .. }
            | CommandPayload::NoteUpdate { .. }
            | CommandPayload::CommitmentCreate { .. }
            | CommandPayload::CommitmentUpdate { .. }
            | CommandPayload::CommitmentRemove { .. }
            | CommandPayload::CapacityCreate { .. }
            | CommandPayload::CapacityUpdate { .. }
            | CommandPayload::CapacityRemove { .. }
            | CommandPayload::AllocationCreate { .. }
            | CommandPayload::AllocationUpdate { .. }
            | CommandPayload::AllocationRemove { .. }
            | CommandPayload::AllocationSetLocked { .. }
            | CommandPayload::ReminderCreate { .. }
            | CommandPayload::ReminderCancel { .. }) => {
                let operation = direct_change_op(command)?;
                apply_change(
                    &mut world,
                    &operation,
                    &ctx.principal_id,
                    &ctx.channel,
                    now,
                    self.ids,
                    &mut outcome,
                )?;
                let event_type = if operation_is_schedule(&operation) {
                    "schedule.changed"
                } else {
                    "workspace.changed"
                };
                (
                    ResultStatus::Committed,
                    json!({
                        "changes": outcome.changes,
                        "warnings": outcome.warnings,
                    }),
                    event_type,
                )
            }
            CommandPayload::PlanPropose {
                task_ids,
                horizon_start,
                horizon_end,
                move_unlocked,
                intent,
                ttl_seconds,
            } => {
                let input = PlanInput {
                    task_ids,
                    horizon_start,
                    horizon_end,
                    move_unlocked,
                };
                let planned = plan(&world, &input, now)?;
                let operation = ChangeOp::PlanApply {
                    replace_task_ids: planned.replace_task_ids.clone(),
                    assignments: planned.assignments.clone(),
                    unplaced: planned.unplaced.clone(),
                };
                let intent = intent.unwrap_or_else(|| "Schedule work within capacity".to_string());
                let operations = vec![operation];
                let (proposal, proposal_status) = self.create_proposal(
                    ctx,
                    &mut world,
                    &intent,
                    operations,
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    ttl_seconds,
                    now,
                    Some(json!({
                        "plan": planned.clone(),
                        "diagnostics": planned_diagnostics(&planned),
                    })),
                )?;
                bump = false;
                let status = if proposal_status == ProposalStatus::Validated {
                    ResultStatus::Validated
                } else {
                    ResultStatus::Proposed
                };
                (
                    status,
                    json!({"proposal": proposal, "plan": planned}),
                    "proposal.changed",
                )
            }
            CommandPayload::ProposalCreate {
                intent,
                operations,
                reads,
                assumptions,
                questions,
                ttl_seconds,
            } => {
                let (proposal, proposal_status) = self.create_proposal(
                    ctx,
                    &mut world,
                    &intent,
                    operations,
                    reads.unwrap_or_default(),
                    assumptions.unwrap_or_default(),
                    questions.unwrap_or_default(),
                    ttl_seconds,
                    now,
                    None,
                )?;
                bump = false;
                let status = if proposal_status == ProposalStatus::Validated {
                    ResultStatus::Validated
                } else {
                    ResultStatus::Proposed
                };
                (status, json!({"proposal": proposal}), "proposal.changed")
            }
            CommandPayload::ProposalValidate { proposal_id } => {
                let (proposal, status) =
                    self.validate_proposal(ctx, &mut world, &proposal_id, now)?;
                bump = false;
                (status, json!({"proposal": proposal}), "proposal.changed")
            }
            CommandPayload::ProposalApprove {
                proposal_id,
                ttl_seconds,
            } => {
                ctx.require("approve")?;
                if !ctx.principal_kind.can_approve() {
                    return Err(ZodoError::new(
                        ErrorCode::PermissionDenied,
                        "delegated agents cannot approve their own proposals",
                    ));
                }
                let grant =
                    self.approve_proposal(ctx, &mut world, &proposal_id, ttl_seconds, now)?;
                bump = false;
                (
                    ResultStatus::Approved,
                    json!({"approval": grant}),
                    "approval.granted",
                )
            }
            CommandPayload::ProposalReject {
                proposal_id,
                reason,
            } => {
                let proposal = world
                    .proposals
                    .iter_mut()
                    .find(|proposal| proposal.id == proposal_id)
                    .ok_or_else(|| ZodoError::not_found("proposal", &proposal_id))?;
                if matches!(proposal.status, ProposalStatus::Applied) {
                    return Err(ZodoError::validation(
                        "an applied proposal cannot be rejected",
                    ));
                }
                proposal.status = ProposalStatus::Rejected;
                proposal.diagnostics.push(crate::model::Diagnostic {
                    severity: "info".to_string(),
                    code: "REJECTED".to_string(),
                    message: reason.unwrap_or_else(|| "rejected by user".to_string()),
                    related_ids: vec![proposal_id.clone()],
                });
                let value = proposal.clone();
                bump = false;
                (
                    ResultStatus::Rejected,
                    json!({"proposal": value}),
                    "proposal.changed",
                )
            }
            CommandPayload::ChangesCommit {
                proposal_id,
                approval_grant,
            } => {
                ctx.require("write")?;
                if !ctx.principal_kind.can_approve() {
                    return Err(ZodoError::new(
                        ErrorCode::PermissionDenied,
                        "an agent cannot consume a business approval; approval is a user action",
                    ));
                }
                let data = self.commit_proposal(
                    ctx,
                    &mut world,
                    &proposal_id,
                    &approval_grant,
                    &mut outcome,
                    now,
                )?;
                bump = true;
                (ResultStatus::Committed, data, "changes.committed")
            }
            CommandPayload::ProgressRecord {
                task_id,
                minutes,
                remaining_minutes,
                note,
                occurred_at,
                dedupe_key,
            } => {
                if let Some(ref dedupe_key) = dedupe_key {
                    if world
                        .activities
                        .iter()
                        .any(|activity| activity.dedupe_key.as_deref() == Some(dedupe_key.as_str()))
                    {
                        return Err(ZodoError::validation("progress was already recorded"));
                    }
                }
                let activity_id = mark_activity(
                    &mut world,
                    &task_id,
                    minutes,
                    occurred_at.unwrap_or(now),
                    note,
                    crate::apply::source_for(&ctx.principal_id, &ctx.channel),
                    self.ids,
                    ActivityKind::Progress,
                )?;
                if let Some(remaining) = remaining_minutes {
                    let task = world.task_mut(&task_id).expect("task exists");
                    if remaining > task.remaining_minutes.unwrap_or(u32::MAX) {
                        return Err(ZodoError::validation(
                            "remaining minutes cannot increase in a progress record",
                        ));
                    }
                    task.remaining_minutes = Some(remaining);
                    task.updated_at = now;
                }
                if let Some(key) = dedupe_key {
                    if let Some(activity) =
                        world.activities.iter_mut().find(|a| a.id == activity_id)
                    {
                        activity.dedupe_key = Some(key);
                    }
                }
                outcome.record("activity", &activity_id, "created");
                outcome.record("task", &task_id, "progress_recorded");
                (
                    ResultStatus::Committed,
                    json!({"activity_id": activity_id, "task": world.task(&task_id)}),
                    "execution.changed",
                )
            }
            CommandPayload::FocusStart { task_id } => {
                if world.active_focus().is_some() {
                    return Err(ZodoError::validation(
                        "only one focus session can be active",
                    ));
                }
                if let Some(task_id) = &task_id {
                    let task = world
                        .task(task_id)
                        .ok_or_else(|| ZodoError::not_found("task", task_id))?;
                    if matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled) {
                        return Err(ZodoError::validation("cannot focus on a closed task"));
                    }
                }
                let id = self.ids.next("foc");
                let session = FocusSession {
                    id: id.clone(),
                    workspace_id: ctx.workspace_id.clone(),
                    task_id,
                    state: FocusState::Running,
                    started_at: now,
                    updated_at: now,
                    paused_at: None,
                    active_started_at: Some(now),
                    segments: Vec::new(),
                    ended_at: None,
                    accumulated_seconds: 0,
                };
                world.focus_sessions.push(session.clone());
                outcome.record("focus_session", &id, "started");
                (
                    ResultStatus::Committed,
                    json!({"focus": session}),
                    "focus.changed",
                )
            }
            CommandPayload::FocusPause {} => {
                let session = active_focus_mut(&mut world)?;
                let start = session
                    .active_started_at
                    .ok_or_else(|| ZodoError::validation("focus session is not running"))?;
                let seconds = (now - start).num_seconds().max(0) as u64;
                session.segments.push(FocusSegment { start, end: now });
                session.accumulated_seconds = session.accumulated_seconds.saturating_add(seconds);
                session.active_started_at = None;
                session.paused_at = Some(now);
                session.state = FocusState::Paused;
                session.updated_at = now;
                let value = session.clone();
                (
                    ResultStatus::Committed,
                    json!({"focus": value}),
                    "focus.changed",
                )
            }
            CommandPayload::FocusResume {} => {
                let session = active_focus_mut(&mut world)?;
                if session.state != FocusState::Paused {
                    return Err(ZodoError::validation(
                        "only a paused focus session can resume",
                    ));
                }
                session.state = FocusState::Running;
                session.paused_at = None;
                session.active_started_at = Some(now);
                session.updated_at = now;
                let value = session.clone();
                (
                    ResultStatus::Committed,
                    json!({"focus": value}),
                    "focus.changed",
                )
            }
            CommandPayload::FocusFinish { note } => {
                let activity_id = self.finish_focus(&mut world, now, note, true)?;
                let session = world.focus_sessions.last().cloned();
                (
                    ResultStatus::Committed,
                    json!({"focus": session, "activity_id": activity_id}),
                    "focus.changed",
                )
            }
            CommandPayload::FocusCancel {} => {
                self.finish_focus(&mut world, now, None, false)?;
                let session = world.focus_sessions.last().cloned();
                (
                    ResultStatus::Committed,
                    json!({"focus": session}),
                    "focus.changed",
                )
            }
            CommandPayload::ReviewBuild {
                period_start,
                period_end,
                notes,
            } => {
                let tz = crate::apply::timezone(&world);
                let start = local_day_start(tz, period_start);
                let end = local_day_end(tz, period_end);
                let facts = build_review_facts(&world, start, end, now, tz);
                let recommendations = review_recommendations(&facts);
                let id = self.ids.next("rev");
                let review = Review {
                    id: id.clone(),
                    workspace_id: ctx.workspace_id.clone(),
                    period_start,
                    period_end,
                    facts,
                    notes: notes.unwrap_or_default(),
                    recommendations,
                    proposal_id: None,
                    created_at: now,
                    source: crate::apply::source_for(&ctx.principal_id, &ctx.channel),
                };
                world.reviews.push(review.clone());
                outcome.record("review", &id, "created");
                (
                    ResultStatus::Committed,
                    json!({"review": review}),
                    "review.created",
                )
            }
            CommandPayload::RecurrenceCreate { rule, through } => {
                let (created, task_ids) = recurrence::create_rule(
                    &mut world,
                    &rule,
                    through,
                    &ctx.principal_id,
                    &ctx.channel,
                    now,
                    self.ids,
                    &mut outcome,
                )?;
                (
                    ResultStatus::Committed,
                    json!({"rule": created, "task_ids": task_ids}),
                    "recurrence.changed",
                )
            }
            CommandPayload::RecurrenceExpand { rule_id, through } => {
                let task_ids = recurrence::expand_existing(
                    &mut world,
                    &rule_id,
                    through,
                    &ctx.principal_id,
                    &ctx.channel,
                    now,
                    self.ids,
                    &mut outcome,
                )?;
                (
                    ResultStatus::Committed,
                    json!({"rule_id": rule_id, "task_ids": task_ids, "through": through}),
                    "recurrence.changed",
                )
            }
            CommandPayload::RecurrenceException {
                occurrence_id,
                action,
                new_date,
            } => {
                let occurrence = recurrence::apply_exception(
                    &mut world,
                    &occurrence_id,
                    action,
                    new_date,
                    now,
                    &mut outcome,
                )?;
                (
                    ResultStatus::Committed,
                    json!({"occurrence": occurrence}),
                    "recurrence.changed",
                )
            }
            CommandPayload::RecurrenceUpdate {
                rule_id,
                scope,
                title,
                estimate_minutes,
                priority,
                time_of_day,
            } => {
                let data = update_recurrence_series(
                    &mut world,
                    &rule_id,
                    scope.unwrap_or(crate::commands::RecurrenceUpdateScope::Future),
                    title.as_deref(),
                    estimate_minutes,
                    priority,
                    time_of_day.as_ref().map(|value| value.as_deref()),
                    &ctx.principal_id,
                    &ctx.channel,
                    now,
                    self.ids,
                    &mut outcome,
                )?;
                (ResultStatus::Committed, data, "recurrence.changed")
            }
            CommandPayload::CalendarImportBusy {
                source_id,
                provider,
                display_name,
                external_id,
                events,
                replace_source,
                cursor,
            } => {
                ctx.require("write")?;
                let data = import_busy(
                    &mut world,
                    source_id,
                    provider,
                    display_name,
                    external_id,
                    events,
                    replace_source,
                    cursor,
                    now,
                    self.ids,
                )?;
                (ResultStatus::Committed, data, "calendar.changed")
            }
        };

        // A non-bumping operation still needs to persist proposal/grant changes but must not create
        // a business revision. Business operations increment once, atomically with the receipt.
        let expected_revision = world.workspace.revision;
        if bump {
            world.workspace.revision = world.workspace.revision.saturating_add(1);
        }
        let event = NewEvent {
            workspace_id: ctx.workspace_id.clone(),
            event_type: event_type.to_string(),
            actor_id: ctx.principal_id.clone(),
            payload: json!({
                "operation": op_name,
                "operation_id": operation_id,
                "status": status,
                "revision": world.workspace.revision,
                "changes": outcome.changes.iter().map(|change| json!({
                    "kind": change.kind,
                    "id": change.id,
                    "action": change.action,
                })).collect::<Vec<_>>(),
            }),
            created_at: now,
        };
        let audit = NewAudit {
            workspace_id: ctx.workspace_id.clone(),
            actor_id: ctx.principal_id.clone(),
            principal_kind: format!("{:?}", ctx.principal_kind).to_lowercase(),
            channel: ctx.channel.clone(),
            operation: op_name,
            operation_id: operation_id.clone(),
            revision: world.workspace.revision,
            status: format!("{status:?}").to_lowercase(),
            summary: json!({
                "change_count": outcome.changes.len(),
                "warnings": outcome.warnings,
                "data": redact_data(&data),
            }),
            created_at: now,
        };
        let receipt = ReceiptInsert {
            principal_id: ctx.principal_id.clone(),
            idempotency_key,
            request_hash: parsed.canonical_hash,
            operation_id: operation_id.clone(),
            status: status.clone(),
            revision: world.workspace.revision,
            data: data.clone(),
            created_at: now,
        };
        let save = self.store.save(
            &world,
            SaveOptions {
                expected_revision,
                bump_revision: bump,
            },
            Some(receipt),
            std::slice::from_ref(&event),
            std::slice::from_ref(&audit),
            &outcome.effects,
        )?;
        if bump {
            emit_effects_events(&mut world, &outcome.effects);
        }
        Ok(CommandResult::new(
            request_id,
            operation_id,
            status,
            world.workspace.revision,
            data,
            save.event_cursor,
            false,
        ))
    }

    pub fn query(
        &mut self,
        ctx: &crate::model::CommandContext,
        envelope: QueryEnvelope,
    ) -> Result<QueryResult> {
        ctx.require("read")?;
        if envelope.workspace_id != ctx.workspace_id {
            return Err(ZodoError::new(
                ErrorCode::PermissionDenied,
                "workspace is outside this authenticated connection",
            ));
        }
        let parsed = envelope.parse()?;
        let request_id = parsed.envelope.request_id.clone();
        if matches!(&parsed.payload, QueryPayload::WorkspaceSnapshot {}) && !ctx.has("read_private")
        {
            return Err(ZodoError::new(
                ErrorCode::PermissionDenied,
                "this connection may read projections but not the full private workspace snapshot",
            ));
        }
        let world = self
            .store
            .load_world(&ctx.workspace_id)?
            .ok_or_else(|| ZodoError::not_found("workspace", &ctx.workspace_id))?;
        let cursor = self.store.latest_cursor(&ctx.workspace_id)?;
        let data = self.query_data(&world, &parsed)?;
        Ok(QueryResult::new(
            request_id,
            world.workspace.revision,
            cursor,
            data,
        ))
    }

    fn query_data(&mut self, world: &World, parsed: &ParsedQuery) -> Result<Value> {
        let tz = crate::apply::timezone(world);
        match &parsed.payload {
            QueryPayload::WorkspaceGet {} => Ok(json!({"workspace": world.workspace})),
            QueryPayload::WorkspaceSnapshot {} => Ok(json!({
                "workspace": world.workspace,
                "inbox": world.inbox,
                "goals": world.goals,
                "projects": world.projects,
                "tasks": world.tasks,
                "dependencies": world.dependencies,
                "commitments": world.commitments,
                "capacity_blocks": world.capacity_blocks,
                "allocations": world.allocations,
                "tags": world.tags,
                "notes": world.notes,
                "reminders": world.reminders,
                "focus_sessions": world.focus_sessions,
                "activities": world.activities,
                "reviews": world.reviews,
                "habits": world.habits,
                "saved_queries": world.saved_queries,
                "automation_rules": world.automation_rules,
                "recurrence_rules": world.recurrence_rules,
                "occurrences": world.occurrences,
                "proposals": world.proposals,
                "approval_grants": world.approval_grants,
                "calendar_sources": world.calendar_sources,
                "calendar_events": world.calendar_events,
                "settings": world.workspace.settings,
                "now": self.clock.now(),
            })),
            QueryPayload::TodayGet { date } => {
                let date = date.unwrap_or_else(|| self.clock.now().with_timezone(&tz).date_naive());
                let start = local_day_start(tz, date);
                let end = local_day_end(tz, date);
                let mut tasks = Vec::new();
                for task in world.open_leaf_tasks() {
                    let target = task.target_due == Some(date);
                    let hard = task.hard_deadline.is_some_and(|deadline| deadline <= end);
                    let due_soon = task.target_due.is_some_and(|target_due| target_due <= date);
                    let scheduled = world.allocations.iter().any(|allocation| {
                        allocation.task_id == task.id
                            && allocation.start < end
                            && allocation.end > start
                    });
                    if target || hard || due_soon || scheduled {
                        tasks.push(task);
                    }
                }
                tasks.sort_by_key(|task| {
                    (
                        task.hard_deadline,
                        task.target_due,
                        -task.priority,
                        task.id.clone(),
                    )
                });
                let next_task = tasks.first().map(|task| task.id.clone());
                Ok(json!({
                    "date": date,
                    "timezone": tz.name(),
                    "fixed_commitments": world.commitments.iter().filter(|item| item.start < end && item.end > start).collect::<Vec<_>>(),
                    "capacity_blocks": world.capacity_blocks.iter().filter(|item| item.start < end && item.end > start).collect::<Vec<_>>(),
                    "allocations": world.allocations.iter().filter(|item| item.start < end && item.end > start).collect::<Vec<_>>(),
                    "tasks": tasks,
                    "next_task_id": next_task,
                    "running_focus": world.active_focus(),
                    "risks": today_risks(world, now_start(end)),
                }))
            }
            QueryPayload::InboxList { status, limit } => {
                let mut entries: Vec<_> = world
                    .inbox
                    .iter()
                    .filter(|entry| status.as_ref().is_none_or(|status| &entry.status == status))
                    .cloned()
                    .collect();
                entries.sort_by(|left, right| right.created_at.cmp(&left.created_at));
                if let Some(limit) = limit {
                    entries.truncate(*limit as usize);
                }
                Ok(json!({"entries": entries}))
            }
            QueryPayload::GoalsList { .. } => Ok(json!({"goals": world.goals})),
            QueryPayload::ProjectsList {} => Ok(json!({"projects": world.projects})),
            QueryPayload::TasksList { filter } => {
                let filter = filter.clone().unwrap_or_default();
                Ok(json!({"tasks": filter_tasks(world, &filter, &tz, self.clock.now())}))
            }
            QueryPayload::TaskGet { id } => {
                let task = world
                    .task(id)
                    .ok_or_else(|| ZodoError::not_found("task", id))?;
                let dependencies = world.dependencies_of(id);
                let dependents = world.successors_of(id);
                let allocations: Vec<_> = world
                    .allocations
                    .iter()
                    .filter(|allocation| allocation.task_id == *id)
                    .collect();
                let activity: Vec<_> = world
                    .activities
                    .iter()
                    .filter(|activity| activity.task_id.as_deref() == Some(id))
                    .collect();
                let notes: Vec<_> = world
                    .notes
                    .iter()
                    .filter(|note| note.linked_tasks.contains(id))
                    .collect();
                let reminders: Vec<_> = world
                    .reminders
                    .iter()
                    .filter(|reminder| reminder.task_id.as_deref() == Some(id))
                    .collect();
                Ok(json!({
                    "task": task,
                    "dependencies": dependencies,
                    "dependents": dependents,
                    "allocations": allocations,
                    "activities": activity,
                    "notes": notes,
                    "reminders": reminders,
                    "blocked": derived_blocked(world, task),
                }))
            }
            QueryPayload::ScheduleGet { start, end } => Ok(json!({
                "start": start,
                "end": end,
                "commitments": world.commitments.iter().filter(|item| item.start < *end && item.end > *start).collect::<Vec<_>>(),
                "capacity_blocks": world.capacity_blocks.iter().filter(|item| item.start < *end && item.end > *start).collect::<Vec<_>>(),
                "allocations": world.allocations.iter().filter(|item| item.start < *end && item.end > *start).collect::<Vec<_>>(),
                "calendar_events": world.calendar_events.iter().filter(|item| item.start < *end && item.end > *start).collect::<Vec<_>>(),
                "unplaced": unplaced_summary(world),
                "calendar_freshness": world.calendar_sources,
            })),
            QueryPayload::CapacityCandidates {
                start,
                end,
                min_minutes,
                max_candidates,
            } => {
                if end < start || (*end - *start).num_days() > 30 {
                    return Err(ZodoError::validation("capacity horizon must be 0..30 days"));
                }
                let candidates = capacity_candidates(
                    world,
                    &tz,
                    self.clock.now(),
                    *start,
                    *end,
                    min_minutes.unwrap_or(30).clamp(5, 480),
                    max_candidates.unwrap_or(12).clamp(1, 50),
                );
                Ok(json!({
                    "candidates": candidates,
                    "horizon": {"start": start, "end": end},
                    "assumptions": [
                        "default free window is 09:00-18:00 in the workspace timezone",
                        "past time and existing commitments, capacity blocks, allocations and busy calendar events are excluded",
                        "candidates are proposals only; creating capacity remains an explicit command"
                    ],
                }))
            }
            QueryPayload::PlanContext {
                task_ids,
                horizon_start,
                horizon_end,
            } => {
                let planned = plan(
                    world,
                    &PlanInput {
                        task_ids: task_ids.clone(),
                        horizon_start: *horizon_start,
                        horizon_end: *horizon_end,
                        move_unlocked: true,
                    },
                    self.clock.now(),
                )?;
                Ok(json!({
                    "workspace": world.workspace,
                    "tasks": world.tasks,
                    "dependencies": world.dependencies,
                    "commitments": world.commitments,
                    "capacity_blocks": world.capacity_blocks,
                    "allocations": world.allocations,
                    "calendar_events": world.calendar_events,
                    "plan": planned,
                }))
            }
            QueryPayload::RecurrencesList {} => Ok(json!({"rules": world.recurrence_rules})),
            QueryPayload::OccurrencesList { rule_id } => Ok(json!({
                "occurrences": world.occurrences.iter().filter(|occurrence| rule_id.as_ref().is_none_or(|rule_id| &occurrence.rule_id == rule_id)).collect::<Vec<_>>()
            })),
            QueryPayload::ProposalsList { status } => {
                let proposals: Vec<_> = world
                    .proposals
                    .iter()
                    .filter(|proposal| {
                        status
                            .as_ref()
                            .is_none_or(|status| &proposal.status == status)
                    })
                    .cloned()
                    .collect();
                Ok(json!({"proposals": proposals}))
            }
            QueryPayload::ProposalGet { id } => {
                let proposal = world
                    .proposals
                    .iter()
                    .find(|proposal| proposal.id == *id)
                    .ok_or_else(|| ZodoError::not_found("proposal", id))?;
                Ok(json!({"proposal": proposal, "current_revision": world.workspace.revision}))
            }
            QueryPayload::FocusCurrent {} => Ok(json!({"focus": world.active_focus()})),
            QueryPayload::ReviewsList {} => Ok(json!({"reviews": world.reviews})),
            QueryPayload::ReviewGet { id } => {
                let review = world
                    .reviews
                    .iter()
                    .find(|review| review.id == *id)
                    .ok_or_else(|| ZodoError::not_found("review", id))?;
                Ok(json!({"review": review}))
            }
            QueryPayload::SearchQuery {
                text,
                kinds,
                limit,
                offset,
            } => {
                let text = text.to_lowercase();
                let limit = limit.unwrap_or(50).min(200) as usize;
                let include = |kind: &str| {
                    kinds
                        .as_ref()
                        .is_none_or(|kinds| kinds.iter().any(|value| value == kind))
                };
                let mut results = Vec::new();
                if include("task") {
                    for task in &world.tasks {
                        if task.title.to_lowercase().contains(&text)
                            || task
                                .description
                                .as_deref()
                                .is_some_and(|value| value.to_lowercase().contains(&text))
                        {
                            results.push(json!({"kind": "task", "id": task.id, "title": task.title, "status": task.status}));
                        }
                    }
                }
                if include("goal") {
                    for goal in &world.goals {
                        if goal.title.to_lowercase().contains(&text)
                            || goal
                                .success_criteria
                                .as_deref()
                                .is_some_and(|value| value.to_lowercase().contains(&text))
                        {
                            results.push(json!({"kind": "goal", "id": goal.id, "title": goal.title, "status": goal.status}));
                        }
                    }
                }
                if include("project") {
                    for project in &world.projects {
                        if project.title.to_lowercase().contains(&text)
                            || project
                                .scope
                                .as_deref()
                                .is_some_and(|value| value.to_lowercase().contains(&text))
                        {
                            results.push(json!({"kind": "project", "id": project.id, "title": project.title, "status": project.status}));
                        }
                    }
                }
                if include("note") {
                    for note in &world.notes {
                        if note.data_class != crate::model::DataClass::CloudAllowed {
                            continue;
                        }
                        if note.title.to_lowercase().contains(&text)
                            || note.body.to_lowercase().contains(&text)
                        {
                            results
                                .push(json!({"kind": "note", "id": note.id, "title": note.title}));
                        }
                    }
                }
                let total = results.len();
                let offset = offset.unwrap_or(0).min(100_000) as usize;
                let page: Vec<_> = results.into_iter().skip(offset).take(limit).collect();
                let next_offset = offset + page.len();
                Ok(json!({
                    "results": page,
                    "total": total,
                    "offset": offset,
                    "next_offset": (next_offset < total).then_some(next_offset),
                    "has_more": next_offset < total,
                    "query": text_count(parsed),
                }))
            }
            QueryPayload::ContentRetrieve {
                text,
                consent,
                limit,
                personalise,
            } => {
                if !consent {
                    return Err(ZodoError::new(
                        ErrorCode::PermissionDenied,
                        "content retrieval requires explicit consent",
                    ));
                }
                let needle = text.trim().to_lowercase();
                if needle.is_empty() {
                    return Err(ZodoError::validation("content retrieval text is required"));
                }
                let limit = limit.unwrap_or(20).clamp(1, 50) as usize;
                let mut candidates = world
                    .notes
                    .iter()
                    .filter(|note| {
                        note.title.to_lowercase().contains(&needle)
                            || note.body.to_lowercase().contains(&needle)
                    })
                    .map(|note| {
                        let linked = note.linked_tasks.len() + note.linked_goals.len();
                        (linked, note)
                    })
                    .collect::<Vec<_>>();
                if *personalise {
                    // Local-only ranking: linked notes outrank loosely related ones; no network or model call.
                    candidates.sort_by(|left, right| {
                        right
                            .0
                            .cmp(&left.0)
                            .then_with(|| right.1.updated_at.cmp(&left.1.updated_at))
                    });
                }
                let items = candidates
                    .into_iter()
                    .take(limit)
                    .map(|(linked, note)| {
                        let body = if note.body.chars().count() > 200 {
                            note.body.chars().take(200).collect::<String>()
                        } else {
                            note.body.clone()
                        };
                        json!({
                            "kind": "note",
                            "id": note.id,
                            "title": note.title,
                            "snippet": body,
                            "data_class": note.data_class,
                            "linked_tasks": note.linked_tasks,
                            "linked_goals": note.linked_goals,
                            "updated_at": note.updated_at,
                            "personalisation_score": linked,
                        })
                    })
                    .collect::<Vec<_>>();
                Ok(json!({
                    "items": items,
                    "count": items.len(),
                    "consent": true,
                    "personalisation": if *personalise { "local_linked_recency" } else { "not_applied" },
                    "scope": "local_workspace_only",
                }))
            }
            QueryPayload::TasksDuplicates {} => {
                let mut groups: std::collections::BTreeMap<String, Vec<&Task>> =
                    std::collections::BTreeMap::new();
                for task in world.tasks.iter().filter(|task| {
                    !task.archived
                        && task.status != TaskStatus::Done
                        && task.status != TaskStatus::Cancelled
                }) {
                    let normalized = normalize_duplicate_title(&task.title);
                    if !normalized.is_empty() {
                        groups.entry(normalized).or_default().push(task);
                    }
                }
                let groups = groups
                    .into_iter()
                    .filter(|(_, tasks)| tasks.len() > 1)
                    .map(|(normalized, tasks)| {
                        json!({
                            "normalized": normalized,
                            "count": tasks.len(),
                            "tasks": tasks.iter().map(|task| json!({
                                "id": task.id,
                                "title": task.title,
                                "status": task.status,
                                "project_id": task.project_id,
                                "version": task.version,
                            })).collect::<Vec<_>>(),
                        })
                    })
                    .collect::<Vec<_>>();
                Ok(json!({"groups": groups, "count": groups.len()}))
            }
            QueryPayload::StatsGet { start, end } => {
                let start_utc = local_day_start(tz, *start);
                let end_utc = local_day_end(tz, *end);
                Ok(json!({
                    "period": {"start": start, "end": end},
                    "facts": build_review_facts(world, start_utc, end_utc, self.clock.now(), tz),
                }))
            }
            QueryPayload::EventsList {
                after_cursor,
                limit,
            } => {
                let after = after_cursor
                    .as_deref()
                    .and_then(|cursor| cursor.strip_prefix("ev_"))
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);
                let events = self.store.list_events(
                    &world.workspace.id,
                    after,
                    limit.unwrap_or(100).min(500),
                )?;
                let latest = self.store.latest_cursor(&world.workspace.id)?;
                let latest_seq = latest
                    .strip_prefix("ev_")
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);
                let next_cursor = events
                    .last()
                    .map(|event| format!("ev_{}", event.seq))
                    .unwrap_or_else(|| format!("ev_{after}"));
                let has_more = events.len() as u32 >= limit.unwrap_or(100).min(500);
                Ok(json!({
                    "events": events,
                    "next_cursor": next_cursor,
                    "latest_cursor": latest,
                    "has_more": has_more,
                    "resync_required": after > latest_seq,
                }))
            }
            QueryPayload::AuditList { after_seq, limit } => {
                let audit = self.store.list_audit(
                    &world.workspace.id,
                    after_seq.unwrap_or(0),
                    limit.unwrap_or(100).min(500),
                )?;
                Ok(json!({"audit": audit}))
            }
            QueryPayload::RemindersList { status } => Ok(json!({
                "reminders": world.reminders.iter().filter(|reminder| status.as_ref().is_none_or(|s| &reminder.status == s)).collect::<Vec<_>>()
            })),
            QueryPayload::RunsList {} => Ok(json!({"runs": []})),
            QueryPayload::RunGet { id } => Err(ZodoError::not_found("run", id)),
            QueryPayload::HealthInfo {} => Ok(json!({
                "protocol": "zodo/1",
                "runtime_version": env!("CARGO_PKG_VERSION"),
                "instance_id": self.instance_id,
                "workspace_id": world.workspace.id,
                "revision": world.workspace.revision,
                "policy_revision": world.workspace.policy_revision,
                "now": self.clock.now(),
            })),
        }
    }

    fn create_proposal(
        &mut self,
        ctx: &crate::model::CommandContext,
        world: &mut World,
        intent: &str,
        operations: Vec<ChangeOp>,
        reads: Vec<crate::model::EntityRef>,
        assumptions: Vec<String>,
        questions: Vec<String>,
        ttl_seconds: Option<i64>,
        now: DateTime<Utc>,
        extra_preview: Option<Value>,
    ) -> Result<(Proposal, ProposalStatus)> {
        let base_revision = world.workspace.revision;
        let policy_revision = world.workspace.policy_revision;
        let hash = proposal_content_hash(
            &world.workspace.id,
            intent,
            &operations,
            base_revision,
            policy_revision,
        );
        let ttl = ttl_seconds.unwrap_or(3600).clamp(30, 24 * 3600);
        let expires_at = now + Duration::seconds(ttl);
        let mut diagnostics = Vec::new();
        let (status, preview) = {
            let mut preview_world = world.clone();
            match apply_operations(
                &mut preview_world,
                &operations,
                &ctx.principal_id,
                &ctx.channel,
                now,
                self.ids,
            ) {
                Ok(outcome) => (
                    ProposalStatus::Validated,
                    preview_diff(world, &preview_world, &outcome),
                ),
                Err(error) => {
                    diagnostics.push(crate::model::Diagnostic {
                        severity: "error".to_string(),
                        code: format!("{:?}", error.code).to_uppercase(),
                        message: error.message,
                        related_ids: Vec::new(),
                    });
                    (
                        ProposalStatus::Draft,
                        json!({"error": "proposal failed validation"}),
                    )
                }
            }
        };
        let proposal = Proposal {
            id: self.ids.next("prp"),
            workspace_id: ctx.workspace_id.clone(),
            intent: intent.to_string(),
            source: SourceRef {
                kind: ctx.channel.clone(),
                id: None,
                actor_id: Some(ctx.principal_id.clone()),
            },
            actor_id: ctx.principal_id.clone(),
            actor_kind: ctx.principal_kind.clone(),
            base_revision,
            policy_revision,
            operations: serde_json::to_value(&operations).expect("operations serialize"),
            reads,
            assumptions,
            questions,
            diagnostics,
            content_hash: hash,
            status: status.clone(),
            created_at: now,
            expires_at,
            validated_at: (status == ProposalStatus::Validated).then_some(now),
            applied_revision: None,
            change_set_id: None,
            preview: extra_preview
                .map(|extra| json!({"plan": extra, "diff": preview}))
                .unwrap_or(preview),
        };
        world.proposals.push(proposal.clone());
        Ok((proposal, status))
    }

    fn validate_proposal(
        &mut self,
        ctx: &crate::model::CommandContext,
        world: &mut World,
        proposal_id: &str,
        now: DateTime<Utc>,
    ) -> Result<(Proposal, ResultStatus)> {
        let (operations, base_revision, policy_revision, hash) = {
            let proposal = world
                .proposals
                .iter()
                .find(|proposal| proposal.id == proposal_id)
                .ok_or_else(|| ZodoError::not_found("proposal", proposal_id))?;
            let operations: Vec<ChangeOp> = serde_json::from_value(proposal.operations.clone())
                .map_err(|error| {
                    ZodoError::validation(format!("stored proposal is invalid: {error}"))
                })?;
            (
                operations,
                proposal.base_revision,
                proposal.policy_revision,
                proposal.content_hash.clone(),
            )
        };
        let current_hash = proposal_content_hash(
            &world.workspace.id,
            &world
                .proposals
                .iter()
                .find(|proposal| proposal.id == proposal_id)
                .expect("proposal exists")
                .intent,
            &operations,
            base_revision,
            policy_revision,
        );
        if current_hash != hash {
            return Err(ZodoError::new(
                ErrorCode::ValidationFailed,
                "stored proposal content does not match its approval hash",
            ));
        }
        let mut preview_world = world.clone();
        let mut diagnostics = Vec::new();
        let status = if base_revision != world.workspace.revision
            || policy_revision != world.workspace.policy_revision
        {
            diagnostics.push(crate::model::Diagnostic {
                severity: "warning".to_string(),
                code: if base_revision != world.workspace.revision {
                    "STALE"
                } else {
                    "POLICY_CHANGED"
                }
                .to_string(),
                message: "the proposal's foundation changed; revalidate before approving"
                    .to_string(),
                related_ids: vec![proposal_id.to_string()],
            });
            ProposalStatus::Stale
        } else {
            match apply_operations(
                &mut preview_world,
                &operations,
                &ctx.principal_id,
                &ctx.channel,
                now,
                self.ids,
            ) {
                Ok(outcome) => {
                    let preview = preview_diff(world, &preview_world, &outcome);
                    let proposal = world
                        .proposals
                        .iter_mut()
                        .find(|proposal| proposal.id == proposal_id)
                        .expect("proposal exists");
                    proposal.preview = preview;
                    proposal.status = ProposalStatus::Validated;
                    proposal.validated_at = Some(now);
                    proposal.diagnostics = diagnostics;
                    return Ok((proposal.clone(), ResultStatus::Validated));
                }
                Err(error) => {
                    diagnostics.push(crate::model::Diagnostic {
                        severity: "error".to_string(),
                        code: format!("{:?}", error.code).to_uppercase(),
                        message: error.message,
                        related_ids: vec![proposal_id.to_string()],
                    });
                    ProposalStatus::Draft
                }
            }
        };
        let proposal = world
            .proposals
            .iter_mut()
            .find(|proposal| proposal.id == proposal_id)
            .expect("proposal exists");
        proposal.status = status.clone();
        proposal.diagnostics = diagnostics;
        let result = if status == ProposalStatus::Validated {
            ResultStatus::Validated
        } else {
            ResultStatus::Proposed
        };
        Ok((proposal.clone(), result))
    }

    fn approve_proposal(
        &mut self,
        ctx: &crate::model::CommandContext,
        world: &mut World,
        proposal_id: &str,
        ttl_seconds: Option<i64>,
        now: DateTime<Utc>,
    ) -> Result<ApprovalGrant> {
        let (hash, base_revision, policy_revision, expires_at, operations) = {
            let proposal = world
                .proposals
                .iter()
                .find(|proposal| proposal.id == proposal_id)
                .ok_or_else(|| ZodoError::not_found("proposal", proposal_id))?;
            if proposal.status != ProposalStatus::Validated {
                return Err(ZodoError::new(
                    ErrorCode::ApprovalRequired,
                    format!("proposal is {:?}, not validated", proposal.status),
                ));
            }
            if proposal.expires_at <= now {
                return Err(ZodoError::new(
                    ErrorCode::ApprovalExpired,
                    "proposal has expired",
                ));
            }
            if proposal.base_revision != world.workspace.revision {
                return Err(ZodoError::new(
                    ErrorCode::RevisionConflict,
                    "proposal is stale; the workspace revision changed",
                )
                .with_details(json!({
                    "expected_revision": proposal.base_revision,
                    "current_revision": world.workspace.revision,
                })));
            }
            if proposal.policy_revision != world.workspace.policy_revision {
                return Err(ZodoError::new(
                    ErrorCode::PolicyChanged,
                    "the privacy or authorization policy changed since the proposal",
                ));
            }
            let operations: Vec<ChangeOp> = serde_json::from_value(proposal.operations.clone())
                .map_err(|error| {
                    ZodoError::validation(format!("stored proposal is invalid: {error}"))
                })?;
            (
                proposal.content_hash.clone(),
                proposal.base_revision,
                proposal.policy_revision,
                proposal.expires_at,
                operations,
            )
        };
        let ttl = ttl_seconds.unwrap_or(900).clamp(30, 24 * 3600);
        let grant_expiry = (now + Duration::seconds(ttl)).min(expires_at);
        let grant = ApprovalGrant {
            id: self.ids.next("apr"),
            workspace_id: world.workspace.id.clone(),
            proposal_id: proposal_id.to_string(),
            content_hash: hash,
            base_revision,
            policy_revision,
            principal_id: ctx.principal_id.clone(),
            channel: ctx.channel.clone(),
            effects: proposal_effects(&operations),
            issued_at: now,
            expires_at: grant_expiry,
            consumed_at: None,
            consumed_revision: None,
        };
        world.approval_grants.push(grant.clone());
        Ok(grant)
    }

    fn commit_proposal(
        &mut self,
        ctx: &crate::model::CommandContext,
        world: &mut World,
        proposal_id: &str,
        approval_grant: &str,
        outcome: &mut ApplyOutcome,
        now: DateTime<Utc>,
    ) -> Result<Value> {
        let (proposal_hash, base_revision, policy_revision, operations, expires_at) = {
            let proposal = world
                .proposals
                .iter()
                .find(|proposal| proposal.id == proposal_id)
                .ok_or_else(|| ZodoError::not_found("proposal", proposal_id))?;
            if proposal.status != ProposalStatus::Validated {
                return Err(ZodoError::new(
                    ErrorCode::ApprovalRequired,
                    "only a validated proposal can be committed",
                ));
            }
            if proposal.base_revision != world.workspace.revision {
                return Err(ZodoError::new(
                    ErrorCode::RevisionConflict,
                    "the proposal is stale; refresh and re-propose",
                )
                .with_details(json!({
                    "expected_revision": proposal.base_revision,
                    "current_revision": world.workspace.revision,
                })));
            }
            if proposal.policy_revision != world.workspace.policy_revision {
                return Err(ZodoError::new(
                    ErrorCode::PolicyChanged,
                    "the policy changed after approval",
                ));
            }
            if proposal.expires_at <= now {
                return Err(ZodoError::new(
                    ErrorCode::ApprovalExpired,
                    "proposal expired",
                ));
            }
            let operations: Vec<ChangeOp> = serde_json::from_value(proposal.operations.clone())
                .map_err(|error| {
                    ZodoError::validation(format!("stored proposal is invalid: {error}"))
                })?;
            (
                proposal.content_hash.clone(),
                proposal.base_revision,
                proposal.policy_revision,
                operations,
                proposal.expires_at,
            )
        };
        let grant = world
            .approval_grants
            .iter()
            .find(|grant| grant.id == approval_grant)
            .cloned()
            .ok_or_else(|| {
                ZodoError::new(ErrorCode::ApprovalRequired, "approval grant was not found")
            })?;
        if grant.consumed_at.is_some() {
            return Err(ZodoError::new(
                ErrorCode::ApprovalExpired,
                "approval grant was already used",
            ));
        }
        if grant.expires_at <= now || grant.expires_at > expires_at {
            return Err(ZodoError::new(
                ErrorCode::ApprovalExpired,
                "approval grant expired",
            ));
        }
        if grant.proposal_id != proposal_id
            || grant.content_hash != proposal_hash
            || grant.base_revision != base_revision
            || grant.policy_revision != policy_revision
        {
            return Err(ZodoError::new(
                ErrorCode::ApprovalRequired,
                "approval does not bind this proposal content and foundation",
            ));
        }
        if grant.principal_id != ctx.principal_id {
            return Err(ZodoError::new(
                ErrorCode::PermissionDenied,
                "approval belongs to a different principal",
            ));
        }
        let mut next_world = world.clone();
        let applied = apply_operations(
            &mut next_world,
            &operations,
            &ctx.principal_id,
            &ctx.channel,
            now,
            self.ids,
        )?;
        *outcome = applied;
        let change_set_id = self.ids.next("cs");
        let next_revision = world.workspace.revision.saturating_add(1);
        if let Some(proposal) = next_world
            .proposals
            .iter_mut()
            .find(|proposal| proposal.id == proposal_id)
        {
            proposal.status = ProposalStatus::Applied;
            proposal.applied_revision = Some(next_revision);
            proposal.change_set_id = Some(change_set_id.clone());
        }
        if let Some(grant) = next_world
            .approval_grants
            .iter_mut()
            .find(|grant| grant.id == approval_grant)
        {
            grant.consumed_at = Some(now);
            grant.consumed_revision = Some(next_revision);
        }
        next_world.workspace.updated_at = now;
        *world = next_world;
        Ok(json!({
            "proposal_id": proposal_id,
            "change_set_id": change_set_id,
            "revision": next_revision,
            "changed": outcome.changes,
            "warnings": outcome.warnings,
            "effects": outcome.effects.iter().map(|effect| json!({"id": effect.id, "status": "pending"})).collect::<Vec<_>>(),
        }))
    }

    fn finish_focus(
        &mut self,
        world: &mut World,
        now: DateTime<Utc>,
        note: Option<String>,
        completed: bool,
    ) -> Result<String> {
        let session_index = world
            .focus_sessions
            .iter()
            .position(|session| matches!(session.state, FocusState::Running | FocusState::Paused))
            .ok_or_else(|| ZodoError::validation("no active focus session"))?;
        let (task_id, seconds) = {
            let session = &mut world.focus_sessions[session_index];
            let seconds = if let Some(start) = session.active_started_at {
                (now - start).num_seconds().max(0) as u64
            } else {
                0
            };
            if let Some(start) = session.active_started_at.take() {
                session.segments.push(FocusSegment { start, end: now });
            }
            session.accumulated_seconds = session.accumulated_seconds.saturating_add(seconds);
            session.state = if completed {
                FocusState::Completed
            } else {
                FocusState::Cancelled
            };
            session.ended_at = Some(now);
            session.updated_at = now;
            (session.task_id.clone(), session.accumulated_seconds)
        };
        let minutes = (seconds / 60) as u32;
        let activity_id = self.ids.next("act");
        let source = SourceRef::system();
        if let Some(task_id) = &task_id {
            if completed && minutes > 0 {
                if let Some(task) = world.task_mut(task_id) {
                    let remaining = task.remaining_minutes.unwrap_or(0);
                    let applied = minutes.min(remaining);
                    task.actual_minutes = task.actual_minutes.saturating_add(applied);
                    task.remaining_minutes = Some(remaining.saturating_sub(applied));
                    task.status = if task.remaining_minutes == Some(0) {
                        TaskStatus::InProgress
                    } else {
                        TaskStatus::InProgress
                    };
                    task.updated_at = now;
                    task.version += 1;
                }
            }
        }
        world.activities.push(ActivityRecord {
            id: activity_id.clone(),
            workspace_id: world.workspace.id.clone(),
            kind: ActivityKind::Focus,
            task_id,
            habit_id: None,
            minutes,
            occurred_at: now,
            note,
            source,
            dedupe_key: None,
            created_at: now,
        });
        Ok(activity_id)
    }
}

/// Apply `recurrence.update` for the `future` scope: rewrite the rule and cascade the editable
/// fields onto the still-generated, not-yet-started instances from today onward. Occurrence dates,
/// occurrence keys, past/completed instances and started/locked allocations are never rewritten.
#[allow(clippy::too_many_arguments)]
fn update_recurrence_series(
    world: &mut World,
    rule_id: &str,
    scope: crate::commands::RecurrenceUpdateScope,
    title: Option<&str>,
    estimate_minutes: Option<u32>,
    priority: Option<i32>,
    time_of_day: Option<Option<&str>>,
    actor_id: &str,
    channel: &str,
    now: DateTime<Utc>,
    ids: &dyn IdSource,
    outcome: &mut ApplyOutcome,
) -> Result<Value> {
    let timezone = crate::apply::timezone(world);
    let today = now.with_timezone(&timezone).date_naive();
    let position = world
        .recurrence_rules
        .iter()
        .position(|rule| rule.id == rule_id)
        .ok_or_else(|| ZodoError::not_found("recurrence_rule", rule_id))?;

    // Resolve the new clock value before mutating anything so a bad value cannot half-apply.
    let parsed_time = match time_of_day {
        None => None,
        Some(None) => Some(None),
        Some(Some(text)) => Some(Some(crate::commands::parse_clock_time(text)?)),
    };

    // Candidate instances: generated occurrences from today onward that still link to a task.
    // Moved/skipped occurrences are deliberate single-occurrence exceptions and keep their task.
    let mut candidate_task_ids: Vec<String> = world
        .occurrences
        .iter()
        .filter(|occurrence| {
            occurrence.rule_id == rule_id
                && occurrence.status == OccurrenceStatus::Generated
                && (matches!(scope, crate::commands::RecurrenceUpdateScope::Series)
                    || occurrence.current_date >= today)
        })
        .filter_map(|occurrence| occurrence.task_id.clone())
        .collect();
    candidate_task_ids.sort();
    candidate_task_ids.dedup();

    {
        let rule = &mut world.recurrence_rules[position];
        if let Some(title) = title {
            rule.title = title.trim().to_string();
        }
        if let Some(estimate) = estimate_minutes {
            rule.estimate_minutes = Some(estimate);
        }
        if let Some(priority) = priority {
            rule.priority = priority;
        }
        if let Some(parsed_time) = parsed_time {
            rule.time_of_day = parsed_time;
        }
        rule.updated_at = now;
        rule.version = rule.version.saturating_add(1);
    }
    outcome.record("recurrence_rule", rule_id, "updated");

    let mut updated_task_ids = Vec::new();
    let mut skipped_locked_task_ids = Vec::new();
    for task_id in candidate_task_ids {
        let Some(task) = world.task(&task_id).cloned() else {
            continue;
        };
        // Closed instances are history; editing the series must not reopen or rewrite them.
        if matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled) {
            continue;
        }
        // Started work and locked allocations are fixed facts: leave that instance untouched.
        let has_fixed_allocation = world.allocations.iter().any(|allocation| {
            allocation.task_id == task_id
                && (allocation.locked || allocation.state == AllocationState::Started)
        });
        if has_fixed_allocation {
            skipped_locked_task_ids.push(task_id);
            continue;
        }
        // Reuse the normal task.update path with a complete spec so unrelated fields survive and
        // remaining minutes are recalculated exactly like a manual edit.
        let mut spec = task_spec_from(&task);
        if let Some(title) = title {
            spec.title = title.trim().to_string();
        }
        if let Some(estimate) = estimate_minutes {
            spec.estimate_minutes = Some(estimate);
        }
        if let Some(priority) = priority {
            spec.priority = priority;
        }
        apply_change(
            world,
            &ChangeOp::TaskUpdate {
                id: task_id.clone(),
                task: spec,
                expected_version: None,
            },
            actor_id,
            channel,
            now,
            ids,
            outcome,
        )?;
        updated_task_ids.push(task_id);
    }

    // One edit should not repeat the same estimate warning once per rewritten instance.
    let mut seen_warnings = std::collections::BTreeSet::new();
    outcome
        .warnings
        .retain(|warning| seen_warnings.insert(warning.clone()));

    let rule = world
        .recurrence_rules
        .iter()
        .find(|rule| rule.id == rule_id)
        .cloned()
        .expect("recurrence rule exists");
    let scope_label = match scope {
        crate::commands::RecurrenceUpdateScope::Future => "future",
        crate::commands::RecurrenceUpdateScope::Series => "series",
    };
    Ok(json!({
        "rule": rule,
        "scope": scope_label,
        "today": today,
        "updated_task_ids": updated_task_ids,
        "skipped_locked_task_ids": skipped_locked_task_ids,
    }))
}

/// Build the complete `TaskSpec` a manual `task.update` would send, so a series edit preserves
/// every field the command does not touch.
fn task_spec_from(task: &Task) -> crate::ops::TaskSpec {
    crate::ops::TaskSpec {
        client_ref: None,
        title: task.title.clone(),
        description: task.description.clone(),
        project_id: task.project_id.clone(),
        parent_id: task.parent_id.clone(),
        estimate_minutes: task.estimate_minutes,
        priority: task.priority,
        target_due: task.target_due,
        hard_deadline: task.hard_deadline,
        not_before: task.not_before,
        preferred_window: task.preferred_window.clone(),
        tags: task.tags.clone(),
        due: None,
    }
}

fn authorize(ctx: &crate::model::CommandContext, command: &CommandPayload) -> Result<()> {
    match command {
        CommandPayload::WorkspaceUpdate { .. } => ctx.require("admin"),
        CommandPayload::WorkspaceRename { .. } => ctx.require("admin"),
        CommandPayload::ProposalApprove { .. } => {
            ctx.require("approve")?;
            Ok(())
        }
        CommandPayload::ChangesCommit { .. } | CommandPayload::ProposalReject { .. } => {
            ctx.require("write")
        }
        CommandPayload::RecurrenceUpdate { .. } => ctx.require("write"),
        CommandPayload::ProposalCreate { .. } | CommandPayload::PlanPropose { .. } => {
            ctx.require("propose")
        }
        CommandPayload::ProposalValidate { .. } => ctx.require("read"),
        CommandPayload::CaptureCollect { .. }
        | CommandPayload::CaptureClarify { .. }
        | CommandPayload::CaptureConvert { .. }
        | CommandPayload::CaptureDismiss { .. } => {
            if ctx.has("write") || ctx.has("capture") {
                Ok(())
            } else {
                Err(ZodoError::new(
                    ErrorCode::PermissionDenied,
                    "this principal cannot capture inbox text",
                ))
            }
        }
        CommandPayload::ProgressRecord { .. }
        | CommandPayload::FocusStart { .. }
        | CommandPayload::FocusPause {}
        | CommandPayload::FocusResume {}
        | CommandPayload::FocusFinish { .. }
        | CommandPayload::FocusCancel {}
        | CommandPayload::HabitLog { .. } => {
            if ctx.has("write") || ctx.has("execute") {
                Ok(())
            } else {
                Err(ZodoError::new(
                    ErrorCode::PermissionDenied,
                    "this principal cannot record execution facts",
                ))
            }
        }
        CommandPayload::ReminderMarkDelivered { .. } => {
            if ctx.has("write") || ctx.has("execute") {
                Ok(())
            } else {
                Err(ZodoError::new(
                    ErrorCode::PermissionDenied,
                    "this principal cannot record reminder delivery",
                ))
            }
        }
        _ => ctx.require("write"),
    }
}

fn check_versions(parsed: &ParsedCommand, world: &World) -> Result<()> {
    if let Some(base) = parsed.envelope.base_revision {
        if base != world.workspace.revision {
            return Err(ZodoError::conflict(base, world.workspace.revision));
        }
    }
    if let Some(policy) = parsed.envelope.policy_revision {
        if policy != world.workspace.policy_revision {
            return Err(ZodoError::new(
                ErrorCode::PolicyChanged,
                "policy changed; refresh and revalidate the operation",
            )
            .with_details(json!({
                "expected_policy_revision": policy,
                "current_policy_revision": world.workspace.policy_revision,
            })));
        }
    }
    Ok(())
}

fn direct_change_op(command: CommandPayload) -> Result<ChangeOp> {
    Ok(match command {
        CommandPayload::TaskCreate { task } => ChangeOp::TaskCreate { task },
        CommandPayload::TaskUpdate {
            id,
            task,
            expected_version,
        } => ChangeOp::TaskUpdate {
            id,
            task,
            expected_version,
        },
        CommandPayload::TaskComplete { id, note } => ChangeOp::TaskStatus {
            id,
            status: TaskStatus::Done,
            note,
        },
        CommandPayload::TaskReopen { id } => ChangeOp::TaskStatus {
            id,
            status: TaskStatus::Open,
            note: None,
        },
        CommandPayload::TaskCancel { id, note } => ChangeOp::TaskStatus {
            id,
            status: TaskStatus::Cancelled,
            note,
        },
        CommandPayload::TaskArchive { id, archived } => ChangeOp::TaskArchive { id, archived },
        CommandPayload::TaskSetBlocked {
            id,
            blocked,
            reason,
        } => ChangeOp::TaskSetBlocked {
            id,
            blocked,
            reason,
        },
        CommandPayload::TaskSetTags { id, tags } => ChangeOp::TaskSetTags { task_id: id, tags },
        CommandPayload::GoalCreate { goal } => ChangeOp::GoalCreate { goal },
        CommandPayload::GoalUpdate { id, goal } => ChangeOp::GoalUpdate { id, goal },
        CommandPayload::GoalStatus {
            id,
            status,
            evidence,
        } => ChangeOp::GoalStatus {
            id,
            status,
            evidence,
        },
        CommandPayload::ProjectCreate { project } => ChangeOp::ProjectCreate { project },
        CommandPayload::ProjectUpdate { id, project } => ChangeOp::ProjectUpdate { id, project },
        CommandPayload::ProjectStatus { id, status } => ChangeOp::ProjectStatus { id, status },
        CommandPayload::DependencyAdd {
            predecessor_id,
            successor_id,
        } => ChangeOp::DependencyAdd {
            predecessor_id,
            successor_id,
        },
        CommandPayload::DependencyRemove { id } => ChangeOp::DependencyRemove { id },
        CommandPayload::NoteCreate { note } => ChangeOp::NoteCreate { note },
        CommandPayload::NoteUpdate {
            id,
            note,
            expected_version,
        } => ChangeOp::NoteUpdate {
            id,
            note,
            expected_version,
        },
        CommandPayload::CommitmentCreate { commitment } => {
            ChangeOp::CommitmentCreate { commitment }
        }
        CommandPayload::CommitmentUpdate { id, commitment } => {
            ChangeOp::CommitmentUpdate { id, commitment }
        }
        CommandPayload::CommitmentRemove { id } => ChangeOp::CommitmentRemove { id },
        CommandPayload::CapacityCreate { capacity } => ChangeOp::CapacityCreate { capacity },
        CommandPayload::CapacityUpdate { id, capacity } => {
            ChangeOp::CapacityUpdate { id, capacity }
        }
        CommandPayload::CapacityRemove { id } => ChangeOp::CapacityRemove { id },
        CommandPayload::AllocationCreate { allocation } => {
            ChangeOp::AllocationCreate { allocation }
        }
        CommandPayload::AllocationUpdate { id, allocation } => {
            ChangeOp::AllocationUpdate { id, allocation }
        }
        CommandPayload::AllocationRemove { id } => ChangeOp::AllocationRemove { id },
        CommandPayload::AllocationSetLocked { id, locked } => {
            ChangeOp::AllocationSetLocked { id, locked }
        }
        CommandPayload::ReminderCreate {
            task_id,
            message,
            fire_at,
            dedupe_key,
        } => ChangeOp::ReminderCreate {
            task_id,
            message,
            fire_at,
            dedupe_key,
        },
        CommandPayload::ReminderCancel { id } => ChangeOp::ReminderCancel { id },
        other => {
            return Err(ZodoError::new(
                ErrorCode::UnsupportedOperation,
                format!("{} is not a direct change", other.operation_name()),
            ));
        }
    })
}

fn operation_is_schedule(operation: &ChangeOp) -> bool {
    matches!(
        operation,
        ChangeOp::AllocationCreate { .. }
            | ChangeOp::AllocationUpdate { .. }
            | ChangeOp::AllocationRemove { .. }
            | ChangeOp::AllocationSetLocked { .. }
            | ChangeOp::CapacityCreate { .. }
            | ChangeOp::CapacityUpdate { .. }
            | ChangeOp::CapacityRemove { .. }
            | ChangeOp::CommitmentCreate { .. }
            | ChangeOp::CommitmentUpdate { .. }
            | ChangeOp::CommitmentRemove { .. }
            | ChangeOp::PlanApply { .. }
    )
}

fn active_focus_mut(world: &mut World) -> Result<&mut FocusSession> {
    world
        .focus_sessions
        .iter_mut()
        .find(|session| matches!(session.state, FocusState::Running | FocusState::Paused))
        .ok_or_else(|| ZodoError::validation("no active focus session"))
}

fn local_day_start(tz: Tz, date: NaiveDate) -> DateTime<Utc> {
    let local = date.and_hms_opt(0, 0, 0).expect("midnight");
    tz.from_local_datetime(&local)
        .single()
        .unwrap_or_else(|| tz.from_utc_datetime(&local))
        .with_timezone(&Utc)
}

fn local_day_end(tz: Tz, date: NaiveDate) -> DateTime<Utc> {
    let local = date.and_hms_opt(23, 59, 59).expect("end of day");
    tz.from_local_datetime(&local)
        .single()
        .unwrap_or_else(|| tz.from_utc_datetime(&local))
        .with_timezone(&Utc)
}

fn now_start(end: DateTime<Utc>) -> DateTime<Utc> {
    end - Duration::days(1)
}

fn today_risks(world: &World, _start: DateTime<Utc>) -> Vec<String> {
    let mut risks = Vec::new();
    if world
        .open_leaf_tasks()
        .iter()
        .any(|task| task.hard_deadline.is_some())
    {
        risks.push("hard deadlines require attention".to_string());
    }
    if world.tasks.iter().any(|task| {
        task.hard_deadline
            .is_some_and(|deadline| deadline < Utc::now())
    }) {
        risks.push("at least one hard deadline is overdue".to_string());
    }
    risks
}

fn normalize_duplicate_title(title: &str) -> String {
    title
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect()
}

fn capacity_candidates(
    world: &World,
    tz: &Tz,
    now: DateTime<Utc>,
    start: NaiveDate,
    end: NaiveDate,
    min_minutes: u32,
    max_candidates: u32,
) -> Vec<Value> {
    let mut busy: Vec<(DateTime<Utc>, DateTime<Utc>)> = Vec::new();
    for commitment in &world.commitments {
        busy.push((commitment.start, commitment.end));
    }
    for block in &world.capacity_blocks {
        busy.push((block.start, block.end));
    }
    for allocation in &world.allocations {
        busy.push((allocation.start, allocation.end));
    }
    for event in &world.calendar_events {
        if event.busy {
            busy.push((event.start, event.end));
        }
    }
    busy.sort_by_key(|interval| interval.0);
    let mut candidates = Vec::new();
    let mut date = start;
    while date <= end && candidates.len() < max_candidates as usize {
        let Some(local_start) = date.and_hms_opt(9, 0, 0) else {
            break;
        };
        let Some(local_end) = date.and_hms_opt(18, 0, 0) else {
            break;
        };
        let day_start = tz
            .from_local_datetime(&local_start)
            .single()
            .map(|value| value.with_timezone(&Utc));
        let day_end = tz
            .from_local_datetime(&local_end)
            .single()
            .map(|value| value.with_timezone(&Utc));
        if let (Some(mut cursor), Some(day_end)) = (day_start, day_end) {
            if date == now.with_timezone(tz).date_naive() {
                cursor = cursor.max(now + Duration::minutes(30));
            }
            if cursor < day_end {
                let mut free = vec![(cursor, day_end)];
                for (busy_start, busy_end) in &busy {
                    if *busy_end <= cursor || *busy_start >= day_end {
                        continue;
                    }
                    let mut next = Vec::new();
                    for (free_start, free_end) in free {
                        if *busy_end <= free_start || *busy_start >= free_end {
                            next.push((free_start, free_end));
                        } else {
                            if free_start < *busy_start {
                                next.push((free_start, *busy_start));
                            }
                            if *busy_end < free_end {
                                next.push((*busy_end, free_end));
                            }
                        }
                    }
                    free = next;
                }
                for (free_start, free_end) in free {
                    let minutes = (free_end - free_start).num_minutes().max(0) as u32;
                    if minutes >= min_minutes {
                        candidates.push(json!({
                            "start": free_start,
                            "end": free_end,
                            "minutes": minutes,
                            "date": date,
                            "timezone": tz.name(),
                        }));
                        if candidates.len() >= max_candidates as usize {
                            break;
                        }
                    }
                }
            }
        }
        date = match date.succ_opt() {
            Some(next) => next,
            None => break,
        };
    }
    candidates
}

fn filter_tasks(world: &World, filter: &TaskFilter, tz: &Tz, now: DateTime<Utc>) -> Vec<Value> {
    let mut tasks: Vec<_> = world
        .tasks
        .iter()
        .filter(|task| filter.include_archived || !task.archived)
        .filter(|task| {
            filter
                .status
                .as_ref()
                .is_none_or(|status| &task.status == status)
        })
        .filter(|task| {
            filter
                .project_id
                .as_ref()
                .is_none_or(|project| task.project_id.as_ref() == Some(project))
        })
        .filter(|task| {
            filter
                .tag
                .as_ref()
                .is_none_or(|tag| task.tags.iter().any(|value| value == tag))
        })
        .filter(|task| {
            filter.text.as_ref().is_none_or(|text| {
                let text = text.to_lowercase();
                task.title.to_lowercase().contains(&text)
                    || task
                        .description
                        .as_deref()
                        .is_some_and(|value| value.to_lowercase().contains(&text))
            })
        })
        .filter(|task| !filter.only_leaf || world.is_leaf(&task.id))
        .filter(|task| {
            !filter.overdue
                || task.status == TaskStatus::Open
                    && (task
                        .target_due
                        .is_some_and(|date| date < now.with_timezone(tz).date_naive())
                        || task.hard_deadline.is_some_and(|deadline| deadline < now))
        })
        .filter(|task| !task.archived || filter.include_archived || task.project_id.is_some())
        .collect();
    tasks.sort_by(|left, right| {
        left.status
            .clone()
            .cmp(&right.status)
            .then_with(|| left.hard_deadline.cmp(&right.hard_deadline))
            .then_with(|| left.target_due.cmp(&right.target_due))
            .then_with(|| right.priority.cmp(&left.priority))
            .then_with(|| left.id.cmp(&right.id))
    });
    let offset = filter.offset as usize;
    let limit = filter.limit.unwrap_or(200).min(1000) as usize;
    tasks
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|task| json!({
            "task": task,
            "blocked": derived_blocked(world, task),
            "allocated_minutes": world.allocations.iter().filter(|allocation| allocation.task_id == task.id).map(|a| a.minutes).sum::<u32>(),
        }))
        .collect()
}

fn derived_blocked(world: &World, task: &crate::model::Task) -> bool {
    task.blocked_manual
        || world.dependencies_of(&task.id).iter().any(|dependency| {
            world
                .task(&dependency.predecessor_id)
                .is_some_and(|predecessor| {
                    !matches!(predecessor.status, TaskStatus::Done | TaskStatus::Cancelled)
                })
        })
}

fn unplaced_summary(world: &World) -> Vec<Value> {
    let mut result = Vec::new();
    for task in world.open_leaf_tasks() {
        let remaining = task
            .remaining_minutes
            .or(task.estimate_minutes)
            .unwrap_or(0);
        let allocated: u32 = world
            .allocations
            .iter()
            .filter(|allocation| allocation.task_id == task.id)
            .map(|allocation| allocation.minutes)
            .sum();
        if remaining > allocated {
            result.push(json!({
                "task_id": task.id,
                "title": task.title,
                "unplaced_minutes": remaining - allocated,
            }));
        }
    }
    result
}

fn build_review_facts(
    world: &World,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    _now: DateTime<Utc>,
    tz: Tz,
) -> ReviewFacts {
    let mut facts = ReviewFacts::empty();
    facts.tasks_completed = world
        .tasks
        .iter()
        .filter(|task| task.completed_at.is_some_and(|at| at >= start && at <= end))
        .count() as u32;
    facts.tasks_opened = world
        .tasks
        .iter()
        .filter(|task| task.created_at >= start && task.created_at <= end)
        .count() as u32;
    facts.actual_minutes = world
        .activities
        .iter()
        .filter(|activity| activity.occurred_at >= start && activity.occurred_at <= end)
        .map(|activity| activity.minutes)
        .sum();
    facts.focus_minutes = world
        .activities
        .iter()
        .filter(|activity| {
            activity.kind == ActivityKind::Focus
                && activity.occurred_at >= start
                && activity.occurred_at <= end
        })
        .map(|activity| activity.minutes)
        .sum();
    facts.focus_sessions = world
        .focus_sessions
        .iter()
        .filter(|session| session.started_at >= start && session.started_at <= end)
        .count() as u32;
    facts.activity_records = world
        .activities
        .iter()
        .filter(|activity| activity.occurred_at >= start && activity.occurred_at <= end)
        .count() as u32;
    let today = end.with_timezone(&tz).date_naive();
    facts.overdue_open = world
        .open_leaf_tasks()
        .iter()
        .filter(|task| {
            task.target_due.is_some_and(|date| date < today)
                || task.hard_deadline.is_some_and(|deadline| deadline < end)
        })
        .count() as u32;
    facts.blocked_open = world
        .open_leaf_tasks()
        .iter()
        .filter(|task| derived_blocked(world, task))
        .count() as u32;
    facts.active_goals = world
        .goals
        .iter()
        .filter(|goal| goal.status == GoalStatus::Active)
        .count() as u32;
    facts.goals_achieved = world
        .goals
        .iter()
        .filter(|goal| goal.achieved_at.is_some_and(|at| at >= start && at <= end))
        .count() as u32;
    facts.habit_completions = world
        .activities
        .iter()
        .filter(|activity| {
            activity.kind == ActivityKind::Habit
                && activity.habit_id.is_some()
                && activity.occurred_at >= start
                && activity.occurred_at <= end
        })
        .count() as u32;
    let start_date = start.with_timezone(&tz).date_naive();
    let end_date = end.with_timezone(&tz).date_naive();
    let mut habit_target: u32 = 0;
    for habit in &world.habits {
        let active_start = habit.start_date.max(start_date);
        let active_end = habit.end_date.map_or(end_date, |date| date.min(end_date));
        if active_start > active_end {
            continue;
        }
        let mut expected = 0u32;
        let mut date = active_start;
        while date <= active_end {
            let weekday = date.weekday().number_from_monday() as u8;
            if habit.scheduled_weekdays.is_empty() || habit.scheduled_weekdays.contains(&weekday) {
                expected += 1;
            }
            date += chrono::Duration::days(1);
        }
        if habit.scheduled_weekdays.is_empty() {
            expected = habit
                .weekly_target
                .saturating_mul(((active_end - active_start).num_days() as u32 + 6) / 7);
        }
        habit_target = habit_target.saturating_add(expected);
    }
    facts.habit_target = habit_target;
    facts.habit_adherence_percent = if habit_target == 0 {
        0
    } else {
        facts
            .habit_completions
            .saturating_mul(100)
            .checked_div(habit_target)
            .unwrap_or(0)
            .min(100)
    };
    facts.unplaced_minutes = unplaced_summary(world)
        .iter()
        .filter_map(|value| value.get("unplaced_minutes").and_then(Value::as_u64))
        .sum::<u64>() as u32;
    facts
}

fn review_recommendations(facts: &ReviewFacts) -> Vec<String> {
    let mut result = Vec::new();
    if facts.overdue_open > 0 {
        result.push(format!("重新安排 {} 个逾期任务", facts.overdue_open));
    }
    if facts.unplaced_minutes > 0 {
        result.push(format!(
            "为 {} 分钟未排入工作增加容量或减少范围",
            facts.unplaced_minutes
        ));
    }
    if facts.focus_minutes == 0 {
        result.push("安排一段可执行的专注时间，而不是继续增加清单".to_string());
    } else if facts.focus_sessions > 0 {
        result.push(format!(
            "下一次专注建议沿用约 {} 分钟的节奏",
            (facts.focus_minutes / facts.focus_sessions.max(1)).max(15)
        ));
    }
    result
}

fn planned_diagnostics(planned: &PlanOutput) -> Vec<Value> {
    planned
        .diagnostics
        .iter()
        .map(|diagnostic| json!(diagnostic))
        .collect()
}

fn import_busy(
    world: &mut World,
    source_id: Option<String>,
    provider: String,
    display_name: String,
    external_id: String,
    events: Vec<BusyEvent>,
    replace_source: bool,
    cursor: Option<String>,
    now: DateTime<Utc>,
    ids: &dyn IdSource,
) -> Result<Value> {
    let index = if let Some(source_id) = source_id {
        world
            .calendar_sources
            .iter()
            .position(|source| source.id == source_id)
    } else {
        world
            .calendar_sources
            .iter()
            .position(|source| source.provider == provider && source.external_id == external_id)
    };
    let source_id = if let Some(index) = index {
        world.calendar_sources[index].display_name = display_name;
        world.calendar_sources[index].last_sync_at = now;
        if cursor.is_some() {
            world.calendar_sources[index].cursor = cursor.clone();
        }
        if replace_source {
            world
                .calendar_events
                .retain(|event| event.source_id != world.calendar_sources[index].id);
        }
        world.calendar_sources[index].id.clone()
    } else {
        let id = ids.next("cal");
        world.calendar_sources.push(CalendarSource {
            id: id.clone(),
            workspace_id: world.workspace.id.clone(),
            provider,
            display_name,
            external_id,
            writable: false,
            last_sync_at: now,
            freshness_seconds: 900,
            cursor,
        });
        id
    };
    for event in events {
        let existing = world.calendar_events.iter().position(|stored| {
            stored.source_id == source_id && stored.external_id == event.external_id
        });
        if let Some(position) = existing {
            let stored = &mut world.calendar_events[position];
            stored.title = event.title;
            stored.start = event.start;
            stored.end = event.end;
            stored.fetched_at = now;
            stored.updated_at = now;
        } else {
            world.calendar_events.push(CalendarEvent {
                id: ids.next("cev"),
                workspace_id: world.workspace.id.clone(),
                source_id: source_id.clone(),
                external_id: event.external_id,
                title: event.title,
                start: event.start,
                end: event.end,
                busy: true,
                updated_at: now,
                fetched_at: now,
            });
        }
    }
    Ok(json!({
        "source_id": source_id,
        "events": world.calendar_events.iter().filter(|event| event.source_id == source_id).count(),
        "fresh_at": now,
    }))
}

fn redact_data(data: &Value) -> Value {
    // Logs store a bounded, structured summary; private note bodies and attachment locations are
    // deliberately not copied into audit payloads.
    match data {
        Value::Object(values) => {
            let mut out = serde_json::Map::new();
            for (key, value) in values {
                if key == "body" || key == "location" || key == "text" {
                    out.insert(key.clone(), json!("[not logged]"));
                } else {
                    out.insert(key.clone(), redact_data(value));
                }
            }
            Value::Object(out)
        }
        Value::Array(values) => Value::Array(values.iter().map(redact_data).collect()),
        other => other.clone(),
    }
}

fn emit_effects_events(world: &mut World, effects: &[NewEffect]) {
    // Effects are persisted separately in the outbox. This hook exists so later event projections
    // can refer to an effect without inventing a second source of truth.
    let _ = (world, effects);
}

fn text_count(parsed: &ParsedQuery) -> String {
    match &parsed.payload {
        QueryPayload::SearchQuery { text, .. } => text.clone(),
        _ => String::new(),
    }
}
