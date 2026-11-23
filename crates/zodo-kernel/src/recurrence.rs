use std::collections::BTreeSet;

use chrono::{Datelike, Duration, NaiveDate, Utc, Weekday};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::apply::{ApplyOutcome, apply_change};
use crate::error::{Result, ZodoError};
use crate::model::{
    Dependency, Occurrence, OccurrenceStatus, RecurrenceFrequency, RecurrenceRule, SourceRef,
    TaskStatus, TaskTree, World,
};
use crate::ops::{ChangeOp, TaskSpec, validate_title};
use crate::ports::IdSource;

pub const MAX_EXPANSION_DAYS: i64 = 180;
pub const MAX_OCCURRENCES: u32 = 366;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecurrenceSpec {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate_minutes: Option<u32>,
    #[serde(default)]
    pub priority: i32,
    pub timezone: String,
    pub start_date: NaiveDate,
    pub frequency: RecurrenceFrequency,
    #[serde(default = "one")]
    pub interval: u32,
    #[serde(default)]
    pub weekdays: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_of_day: Option<chrono::NaiveTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_occurrences: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_completion_days: Option<u32>,
    #[serde(default)]
    pub depends_on_previous: bool,
}

fn one() -> u32 {
    1
}

impl RecurrenceSpec {
    pub fn validate(&self) -> Result<()> {
        validate_title(&self.title)?;
        if self.frequency == RecurrenceFrequency::Weekly && self.weekdays.is_empty() {
            return Err(ZodoError::validation(
                "weekly recurrence needs at least one weekday",
            ));
        }
        if self.interval == 0 || self.interval > 52 {
            return Err(ZodoError::validation("recurrence interval must be 1..52"));
        }
        if self.timezone.parse::<Tz>().is_err() {
            return Err(ZodoError::validation("recurrence timezone must be IANA"));
        }
        if let Some(end) = self.end_date {
            if end < self.start_date {
                return Err(ZodoError::validation(
                    "recurrence end_date is before start_date",
                ));
            }
        }
        let mut weekdays = BTreeSet::new();
        for weekday in &self.weekdays {
            if !(1..=7).contains(weekday) {
                return Err(ZodoError::validation("weekday must be 1..7"));
            }
            if !weekdays.insert(*weekday) {
                return Err(ZodoError::validation("weekday is duplicated"));
            }
        }
        if self
            .after_completion_days
            .is_some_and(|value| value == 0 || value > 365)
        {
            return Err(ZodoError::validation(
                "after_completion_days must be 1..365",
            ));
        }
        if self
            .max_occurrences
            .is_some_and(|value| value == 0 || value > MAX_OCCURRENCES)
        {
            return Err(ZodoError::validation(
                "max_occurrences must be 1..366 for the bounded P0 expansion",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceAction {
    Skip,
    Move,
}

pub fn create_rule(
    world: &mut World,
    spec: &RecurrenceSpec,
    through: Option<NaiveDate>,
    actor_id: &str,
    channel: &str,
    now: chrono::DateTime<Utc>,
    ids: &dyn IdSource,
    outcome: &mut ApplyOutcome,
) -> Result<(RecurrenceRule, Vec<String>)> {
    spec.validate()?;
    if let Some(project_id) = &spec.project_id {
        if world.project(project_id).is_none() {
            return Err(ZodoError::not_found("project", project_id));
        }
    }
    let rule = RecurrenceRule {
        id: ids.next("rec"),
        workspace_id: world.workspace.id.clone(),
        title: spec.title.clone(),
        project_id: spec.project_id.clone(),
        estimate_minutes: spec.estimate_minutes,
        priority: spec.priority,
        timezone: spec.timezone.clone(),
        start_date: spec.start_date,
        frequency: spec.frequency.clone(),
        interval: spec.interval,
        weekdays: spec.weekdays.clone(),
        time_of_day: spec.time_of_day,
        end_date: spec.end_date,
        max_occurrences: spec.max_occurrences,
        after_completion_days: spec.after_completion_days,
        depends_on_previous: spec.depends_on_previous,
        created_at: now,
        updated_at: now,
        version: 1,
    };
    world.recurrence_rules.push(rule.clone());
    outcome.record("recurrence_rule", &rule.id, "created");
    let through = through.unwrap_or_else(|| spec.start_date + Duration::days(42));
    let task_ids = expand_rule(world, &rule, through, actor_id, channel, now, ids, outcome)?;
    Ok((rule, task_ids))
}

pub fn expand_existing(
    world: &mut World,
    rule_id: &str,
    through: NaiveDate,
    actor_id: &str,
    channel: &str,
    now: chrono::DateTime<Utc>,
    ids: &dyn IdSource,
    outcome: &mut ApplyOutcome,
) -> Result<Vec<String>> {
    let rule = world
        .recurrence_rules
        .iter()
        .find(|rule| rule.id == rule_id)
        .cloned()
        .ok_or_else(|| ZodoError::not_found("recurrence_rule", rule_id))?;
    expand_rule(world, &rule, through, actor_id, channel, now, ids, outcome)
}

pub fn apply_exception(
    world: &mut World,
    occurrence_id: &str,
    action: OccurrenceAction,
    new_date: Option<NaiveDate>,
    now: chrono::DateTime<Utc>,
    outcome: &mut ApplyOutcome,
) -> Result<Occurrence> {
    let position = world
        .occurrences
        .iter()
        .position(|occurrence| occurrence.id == occurrence_id)
        .ok_or_else(|| ZodoError::not_found("occurrence", occurrence_id))?;
    let occurrence = world.occurrences[position].clone();
    if !matches!(
        occurrence.status,
        OccurrenceStatus::Generated | OccurrenceStatus::Moved
    ) {
        return Err(ZodoError::validation(
            "only a generated or moved occurrence can receive an exception",
        ));
    }
    let task_id = occurrence
        .task_id
        .clone()
        .ok_or_else(|| ZodoError::validation("occurrence has no task"))?;
    let task = world
        .task(&task_id)
        .ok_or_else(|| ZodoError::not_found("task", &task_id))?;
    if task.status == TaskStatus::Done {
        return Err(ZodoError::validation(
            "a completed occurrence cannot be skipped or moved",
        ));
    }
    match action {
        OccurrenceAction::Skip => {
            TaskTree::new(world).require_finished_descendants(&task_id)?;
            let task = world.task_mut(&task_id).expect("task exists");
            task.status = TaskStatus::Cancelled;
            task.updated_at = now;
            task.version += 1;
            let occurrence = &mut world.occurrences[position];
            occurrence.status = OccurrenceStatus::Skipped;
            occurrence.updated_at = now;
            outcome.record("occurrence", occurrence_id, "skipped");
            outcome.record("task", &task_id, "cancelled_by_exception");
        }
        OccurrenceAction::Move => {
            let new_date = new_date
                .ok_or_else(|| ZodoError::validation("move exception requires new_date"))?;
            let duplicate = world.occurrences.iter().any(|other| {
                other.id != occurrence_id
                    && other.rule_id == occurrence.rule_id
                    && other.current_date == new_date
                    && !matches!(other.status, OccurrenceStatus::Skipped)
            });
            if duplicate {
                return Err(ZodoError::validation(
                    "another occurrence of this series already uses that date",
                ));
            }
            let task = world.task_mut(&task_id).expect("task exists");
            task.target_due = Some(new_date);
            task.updated_at = now;
            task.version += 1;
            let occurrence = &mut world.occurrences[position];
            occurrence.current_date = new_date;
            occurrence.status = OccurrenceStatus::Moved;
            occurrence.updated_at = now;
            outcome.record("occurrence", occurrence_id, "moved");
            outcome.record("task", &task_id, "moved_by_exception");
        }
    }
    Ok(world.occurrences[position].clone())
}

/// Create the next occurrence of an after-completion series. The next due date is measured from
/// the completion instant in the rule timezone, never from the previous schedule.
pub fn schedule_after_completion(
    world: &mut World,
    completed_task_id: &str,
    actor_id: &str,
    channel: &str,
    now: chrono::DateTime<Utc>,
    ids: &dyn IdSource,
    outcome: &mut ApplyOutcome,
) -> Result<Option<String>> {
    let Some(task) = world.task(completed_task_id).cloned() else {
        return Ok(None);
    };
    let Some(series_id) = task.series_id.clone() else {
        return Ok(None);
    };
    let Some(rule) = world
        .recurrence_rules
        .iter()
        .find(|rule| rule.id == series_id)
        .cloned()
    else {
        return Ok(None);
    };
    let Some(days) = rule.after_completion_days else {
        return Ok(None);
    };
    let tz = rule
        .timezone
        .parse::<Tz>()
        .map_err(|_| ZodoError::validation("recurrence rule has an invalid timezone"))?;
    let next_date = now.with_timezone(&tz).date_naive() + Duration::days(i64::from(days));
    if rule.end_date.is_some_and(|end| next_date > end) {
        return Ok(None);
    }
    let generated = world
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.rule_id == rule.id)
        .count() as u32;
    if rule
        .max_occurrences
        .is_some_and(|maximum| generated >= maximum)
    {
        return Ok(None);
    }
    let key = next_date.format("%Y-%m-%d").to_string();
    if world
        .occurrences
        .iter()
        .any(|occurrence| occurrence.rule_id == rule.id && occurrence.occurrence_key == key)
    {
        return Ok(None);
    }
    let spec = TaskSpec {
        client_ref: None,
        title: rule.title.clone(),
        description: None,
        project_id: rule.project_id.clone(),
        parent_id: None,
        estimate_minutes: rule.estimate_minutes,
        priority: rule.priority,
        target_due: Some(next_date),
        hard_deadline: None,
        not_before: None,
        preferred_window: None,
        tags: Vec::new(),
        due: None,
    };
    apply_change(
        world,
        &ChangeOp::TaskCreate { task: spec },
        actor_id,
        channel,
        now,
        ids,
        outcome,
    )?;
    let task_id = outcome
        .changes
        .iter()
        .rev()
        .find(|change| change.kind == "task")
        .map(|change| change.id.clone())
        .ok_or_else(|| {
            ZodoError::new(
                crate::error::ErrorCode::StorageFailure,
                "after-completion task was not created",
            )
        })?;
    if let Some(task) = world.task_mut(&task_id) {
        task.series_id = Some(rule.id.clone());
        task.occurrence_key = Some(key.clone());
    }
    world.occurrences.push(Occurrence {
        id: ids.next("occ"),
        workspace_id: world.workspace.id.clone(),
        rule_id: rule.id.clone(),
        occurrence_key: key.clone(),
        original_date: next_date,
        current_date: next_date,
        task_id: Some(task_id.clone()),
        status: OccurrenceStatus::Generated,
        created_at: now,
        updated_at: now,
    });
    if rule.depends_on_previous {
        let dependency_id = ids.next("dep");
        world.dependencies.push(Dependency {
            id: dependency_id.clone(),
            workspace_id: world.workspace.id.clone(),
            predecessor_id: completed_task_id.to_string(),
            successor_id: task_id.clone(),
            created_at: now,
        });
        outcome.record("dependency", &dependency_id, "created");
    }
    outcome.record("recurrence_occurrence", &key, "created_after_completion");
    Ok(Some(task_id))
}

fn expand_rule(
    world: &mut World,
    rule: &RecurrenceRule,
    through: NaiveDate,
    actor_id: &str,
    channel: &str,
    now: chrono::DateTime<Utc>,
    ids: &dyn IdSource,
    outcome: &mut ApplyOutcome,
) -> Result<Vec<String>> {
    if through < rule.start_date {
        return Ok(Vec::new());
    }
    let span = (through - rule.start_date).num_days();
    if span > MAX_EXPANSION_DAYS {
        return Err(ZodoError::validation(format!(
            "recurrence expansion is bounded to {MAX_EXPANSION_DAYS} days"
        )));
    }
    let existing_keys: BTreeSet<(String, String)> = world
        .occurrences
        .iter()
        .map(|occurrence| {
            (
                occurrence.rule_id.clone(),
                occurrence.occurrence_key.clone(),
            )
        })
        .collect();
    let mut dates = Vec::new();
    let mut cursor = rule.start_date;
    while cursor <= through {
        if within_rule(rule, cursor) {
            dates.push(cursor);
            if rule.after_completion_days.is_some() {
                break;
            }
            if dates.len() as u32 >= rule.max_occurrences.unwrap_or(MAX_OCCURRENCES) {
                break;
            }
        }
        cursor += Duration::days(1);
    }
    let mut created = Vec::new();
    for date in dates {
        let key = date.format("%Y-%m-%d").to_string();
        if existing_keys.contains(&(rule.id.clone(), key.clone())) {
            continue;
        }
        let spec = TaskSpec {
            client_ref: None,
            title: rule.title.clone(),
            description: None,
            project_id: rule.project_id.clone(),
            parent_id: None,
            estimate_minutes: rule.estimate_minutes,
            priority: rule.priority,
            target_due: Some(date),
            hard_deadline: None,
            not_before: None,
            preferred_window: None,
            tags: Vec::new(),
            due: None,
        };
        apply_change(
            world,
            &ChangeOp::TaskCreate { task: spec },
            actor_id,
            channel,
            now,
            ids,
            outcome,
        )?;
        let task_id = outcome
            .changes
            .iter()
            .rev()
            .find(|change| change.kind == "task")
            .map(|change| change.id.clone())
            .ok_or_else(|| {
                ZodoError::new(
                    crate::error::ErrorCode::StorageFailure,
                    "recurrence task was not created",
                )
            })?;
        if let Some(task) = world.task_mut(&task_id) {
            task.series_id = Some(rule.id.clone());
            task.occurrence_key = Some(key.clone());
        }
        world.occurrences.push(Occurrence {
            id: ids.next("occ"),
            workspace_id: world.workspace.id.clone(),
            rule_id: rule.id.clone(),
            occurrence_key: key,
            original_date: date,
            current_date: date,
            task_id: Some(task_id.clone()),
            status: OccurrenceStatus::Generated,
            created_at: now,
            updated_at: now,
        });
        created.push(task_id);
    }
    if rule.depends_on_previous {
        let mut chain = world
            .occurrences
            .iter()
            .filter(|occurrence| occurrence.rule_id == rule.id)
            .filter_map(|occurrence| {
                occurrence
                    .task_id
                    .as_ref()
                    .map(|task_id| (occurrence.original_date, task_id.clone()))
            })
            .collect::<Vec<_>>();
        chain.sort_by_key(|(date, _)| *date);
        for window in chain.windows(2) {
            let predecessor_id = window[0].1.clone();
            let successor_id = window[1].1.clone();
            if predecessor_id == successor_id {
                continue;
            }
            let exists = world.dependencies.iter().any(|dependency| {
                dependency.predecessor_id == predecessor_id
                    && dependency.successor_id == successor_id
            });
            if !exists {
                let dependency_id = ids.next("dep");
                world.dependencies.push(Dependency {
                    id: dependency_id.clone(),
                    workspace_id: world.workspace.id.clone(),
                    predecessor_id,
                    successor_id,
                    created_at: now,
                });
                outcome.record("dependency", &dependency_id, "created");
            }
        }
    }
    let _ = SourceRef::system();
    Ok(created)
}

fn within_rule(rule: &RecurrenceRule, date: NaiveDate) -> bool {
    if date < rule.start_date {
        return false;
    }
    if rule.end_date.is_some_and(|end| date > end) {
        return false;
    }
    match rule.frequency {
        RecurrenceFrequency::Daily => {
            (date - rule.start_date).num_days() % i64::from(rule.interval) == 0
        }
        RecurrenceFrequency::Weekly => {
            let weekdays = if rule.weekdays.is_empty() {
                vec![rule.start_date.weekday().number_from_monday() as u8]
            } else {
                rule.weekdays.clone()
            };
            if !weekdays.contains(&(date.weekday().number_from_monday() as u8)) {
                return false;
            }
            let start_monday = rule.start_date
                - Duration::days(i64::from(rule.start_date.weekday().num_days_from_monday()));
            let date_monday =
                date - Duration::days(i64::from(date.weekday().num_days_from_monday()));
            let weeks = (date_monday - start_monday).num_weeks();
            weeks % i64::from(rule.interval) == 0
        }
        RecurrenceFrequency::Monthly => {
            let months = (i32::from(date.year()) - i32::from(rule.start_date.year())) * 12
                + date.month() as i32
                - rule.start_date.month() as i32;
            months >= 0 && months % rule.interval as i32 == 0 && date.day() == rule.start_date.day()
        }
    }
}

#[allow(dead_code)]
fn weekday_from_number(value: u8) -> Option<Weekday> {
    Weekday::try_from(value.saturating_sub(1)).ok()
}
