use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::error::{Result, ZodoError};
use crate::model::{
    AllocationState, AutomationTrigger, DataClass, GoalStatus, ProjectStatus, SourceRef,
    TaskStatus, TimeWindow,
};

pub const MAX_TITLE_CHARS: usize = 500;
pub const MAX_TEXT_CHARS: usize = 40_000;
pub const MAX_BATCH: usize = 200;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DueStrength {
    Soft,
    Hard,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DueInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<DateTime<Utc>>,
    pub strength: DueStrength,
}

impl DueInput {
    pub fn validate(&self) -> Result<()> {
        match (self.date.is_some(), self.at.is_some()) {
            (true, false) | (false, true) => Ok(()),
            _ => Err(ZodoError::validation(
                "due must carry exactly one of date or at",
            )),
        }
    }

    pub fn hard_deadline(&self, timezone: Tz) -> Option<DateTime<Utc>> {
        if self.strength != DueStrength::Hard {
            return None;
        }
        if let Some(at) = self.at {
            return Some(at);
        }
        self.date.map(|date| {
            let local = date
                .and_hms_opt(23, 59, 59)
                .expect("midnight arithmetic is valid");
            timezone
                .from_local_datetime(&local)
                .single()
                .map(|value| value.with_timezone(&Utc))
                .unwrap_or_else(|| local.and_utc())
        })
    }

    pub fn soft_target(&self) -> Option<NaiveDate> {
        if self.strength != DueStrength::Soft {
            return None;
        }
        self.date
            .or_else(|| self.at.map(|value| value.date_naive()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskSpec {
    pub title: String,
    /// Proposal-local reference used by `dependency.add_ref` operations in the same batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate_minutes: Option<u32>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_due: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hard_deadline: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_before: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_window: Option<TimeWindow>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<DueInput>,
}

impl TaskSpec {
    pub fn validate(&self) -> Result<()> {
        validate_title(&self.title)?;
        if self.priority < 0 || self.priority > 4 {
            return Err(ZodoError::validation("priority must be 0..4"));
        }
        if let Some(estimate) = self.estimate_minutes {
            if estimate == 0 || estimate > 7 * 24 * 60 {
                return Err(ZodoError::validation("estimate_minutes must be 1..10080"));
            }
        }
        if let Some(description) = &self.description {
            if description.chars().count() > MAX_TEXT_CHARS {
                return Err(ZodoError::validation("description is too long"));
            }
        }
        if self.due.is_some() && (self.target_due.is_some() || self.hard_deadline.is_some()) {
            return Err(ZodoError::validation(
                "use due with a strength, or target_due/hard_deadline, not both",
            ));
        }
        if let Some(due) = &self.due {
            due.validate()?;
        }
        if let Some(window) = &self.preferred_window {
            if window.end <= window.start {
                return Err(ZodoError::validation(
                    "preferred_window end must be after start",
                ));
            }
        }
        if let Some(tags) = Some(&self.tags) {
            if tags.len() > 32 {
                return Err(ZodoError::validation("at most 32 tags are allowed"));
            }
            for tag in tags {
                if tag.trim().is_empty() || tag.chars().count() > 64 {
                    return Err(ZodoError::validation("tags must be 1..64 characters"));
                }
            }
        }
        Ok(())
    }

    pub fn effective_target_due(&self) -> Option<NaiveDate> {
        self.target_due
            .or_else(|| self.due.as_ref().and_then(DueInput::soft_target))
    }

    pub fn effective_hard_deadline(&self, timezone: Tz) -> Option<DateTime<Utc>> {
        self.hard_deadline.or_else(|| {
            self.due
                .as_ref()
                .and_then(|due| due.hard_deadline(timezone))
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GoalSpec {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success_criteria: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_due: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hard_deadline: Option<DateTime<Utc>>,
}

impl GoalSpec {
    pub fn validate(&self) -> Result<()> {
        validate_title(&self.title)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectSpec {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
}

impl ProjectSpec {
    pub fn validate(&self) -> Result<()> {
        validate_title(&self.title)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HabitSpec {
    pub title: String,
    pub timezone: String,
    pub start_date: NaiveDate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<NaiveDate>,
    pub weekly_target: u32,
    #[serde(default)]
    pub scheduled_weekdays: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_of_day: Option<NaiveTime>,
}

impl HabitSpec {
    pub fn validate(&self) -> Result<()> {
        validate_title(&self.title)?;
        if self.timezone.parse::<Tz>().is_err() {
            return Err(ZodoError::validation("habit timezone must be IANA"));
        }
        if self.weekly_target == 0 || self.weekly_target > 100 {
            return Err(ZodoError::validation("weekly_target must be 1..100"));
        }
        if self.scheduled_weekdays.len() > 7
            || self
                .scheduled_weekdays
                .iter()
                .any(|day| !(1..=7).contains(day))
        {
            return Err(ZodoError::validation("habit weekdays must be 1..7"));
        }
        let mut seen = std::collections::BTreeSet::new();
        if !self.scheduled_weekdays.iter().all(|day| seen.insert(*day)) {
            return Err(ZodoError::validation("habit weekdays must be unique"));
        }
        if let Some(end) = self.end_date {
            if end < self.start_date {
                return Err(ZodoError::validation("habit end_date is before start_date"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TagSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

impl TagSpec {
    pub fn validate(&self) -> Result<()> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 64 {
            return Err(ZodoError::validation("tag name must be 1..64 chars"));
        }
        if self
            .color
            .as_ref()
            .is_some_and(|color| color.chars().count() > 32)
        {
            return Err(ZodoError::validation("tag color is too long"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SavedQuerySpec {
    pub name: String,
    #[serde(default)]
    pub filter: serde_json::Value,
}

impl SavedQuerySpec {
    pub fn validate(&self) -> Result<()> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 120 {
            return Err(ZodoError::validation(
                "saved view name must be 1..120 chars",
            ));
        }
        if !self.filter.is_object() {
            return Err(ZodoError::validation(
                "saved view filter must be a JSON object",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MilestoneSpec {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<NaiveDate>,
    #[serde(default)]
    pub done: bool,
}

impl MilestoneSpec {
    pub fn validate(&self) -> Result<()> {
        validate_title(&self.title)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AutomationSpec {
    pub name: String,
    pub trigger: AutomationTrigger,
    #[serde(default)]
    pub conditions: serde_json::Value,
    pub actions: serde_json::Value,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub budget: u32,
}

fn default_true() -> bool {
    true
}

impl AutomationSpec {
    pub fn validate(&self) -> Result<()> {
        validate_title(&self.name)?;
        if self.budget == 0 || self.budget > 100 {
            return Err(ZodoError::validation("automation budget must be 1..100"));
        }
        if !self.conditions.is_object() {
            return Err(ZodoError::validation(
                "automation conditions must be an object",
            ));
        }
        let actions = self
            .actions
            .as_array()
            .ok_or_else(|| ZodoError::validation("automation actions must be an array"))?;
        if actions.is_empty() || actions.len() > 20 {
            return Err(ZodoError::validation("automation actions must be 1..20"));
        }
        for action in actions {
            if action.get("kind").and_then(|value| value.as_str()) != Some("task.create") {
                return Err(ZodoError::validation(
                    "only task.create automation actions are currently supported",
                ));
            }
            let task: TaskSpec = serde_json::from_value(
                action
                    .get("task")
                    .cloned()
                    .ok_or_else(|| ZodoError::validation("automation task action needs task"))?,
            )
            .map_err(|error| {
                ZodoError::validation(format!("invalid automation task action: {error}"))
            })?;
            task.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CommitmentInput {
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    #[serde(default)]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calendar_source_id: Option<String>,
}

impl CommitmentInput {
    pub fn validate(&self) -> Result<()> {
        validate_title(&self.title)?;
        if self.end <= self.start {
            return Err(ZodoError::validation("commitment end must be after start"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CapacityInput {
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub capacity_minutes: u32,
    #[serde(default)]
    pub locked: bool,
}

impl CapacityInput {
    pub fn validate(&self) -> Result<()> {
        validate_title(&self.title)?;
        if self.end <= self.start {
            return Err(ZodoError::validation(
                "capacity block end must be after start",
            ));
        }
        let minutes = (self.end - self.start).num_minutes();
        if minutes <= 0 {
            return Err(ZodoError::validation(
                "capacity block must have a positive duration",
            ));
        }
        if self.capacity_minutes == 0
            || u64::from(self.capacity_minutes) > u64::try_from(minutes).unwrap_or(0)
        {
            return Err(ZodoError::validation(
                "capacity_minutes must be positive and not exceed the block duration",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AllocationDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity_block_id: Option<String>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub minutes: u32,
    #[serde(default)]
    pub locked: bool,
}

impl AllocationDraft {
    pub fn validate(&self) -> Result<()> {
        if self.end <= self.start {
            return Err(ZodoError::validation("allocation end must be after start"));
        }
        let duration = (self.end - self.start).num_minutes();
        if duration <= 0 {
            return Err(ZodoError::validation(
                "allocation must have a positive duration",
            ));
        }
        if self.minutes == 0 || u64::from(self.minutes) != u64::try_from(duration).unwrap_or(0) {
            return Err(ZodoError::validation(
                "allocation minutes must equal its wall-clock duration",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NoteSpec {
    pub title: String,
    pub body: String,
    #[serde(default = "default_data_class")]
    pub data_class: DataClass,
    #[serde(default)]
    pub linked_tasks: Vec<String>,
    #[serde(default)]
    pub linked_goals: Vec<String>,
}

fn default_data_class() -> DataClass {
    DataClass::DeviceOnly
}

impl NoteSpec {
    pub fn validate(&self) -> Result<()> {
        validate_title(&self.title)?;
        if self.body.chars().count() > 200_000 {
            return Err(ZodoError::validation("note body is too long"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "operation", content = "payload", rename_all = "snake_case")]
pub enum ChangeOp {
    #[serde(rename = "task.create")]
    TaskCreate { task: TaskSpec },
    #[serde(rename = "task.update")]
    TaskUpdate {
        id: String,
        task: TaskSpec,
        expected_version: Option<u64>,
    },
    #[serde(rename = "task.status")]
    TaskStatus {
        id: String,
        status: TaskStatus,
        note: Option<String>,
    },
    #[serde(rename = "task.archive")]
    TaskArchive { id: String, archived: bool },
    #[serde(rename = "task.set_blocked")]
    TaskSetBlocked {
        id: String,
        blocked: bool,
        reason: Option<String>,
    },
    #[serde(rename = "goal.create")]
    GoalCreate { goal: GoalSpec },
    #[serde(rename = "goal.update")]
    GoalUpdate { id: String, goal: GoalSpec },
    #[serde(rename = "goal.status")]
    GoalStatus {
        id: String,
        status: GoalStatus,
        evidence: Option<String>,
    },
    #[serde(rename = "project.create")]
    ProjectCreate { project: ProjectSpec },
    #[serde(rename = "project.update")]
    ProjectUpdate { id: String, project: ProjectSpec },
    #[serde(rename = "project.status")]
    ProjectStatus { id: String, status: ProjectStatus },
    #[serde(rename = "dependency.add")]
    DependencyAdd {
        predecessor_id: String,
        successor_id: String,
    },
    #[serde(rename = "dependency.add_ref")]
    DependencyAddRef {
        predecessor_ref: String,
        successor_ref: String,
    },
    #[serde(rename = "dependency.remove")]
    DependencyRemove { id: String },
    #[serde(rename = "task.set_tags")]
    TaskSetTags { task_id: String, tags: Vec<String> },
    #[serde(rename = "commitment.create")]
    CommitmentCreate { commitment: CommitmentInput },
    #[serde(rename = "commitment.update")]
    CommitmentUpdate {
        id: String,
        commitment: CommitmentInput,
    },
    #[serde(rename = "commitment.remove")]
    CommitmentRemove { id: String },
    #[serde(rename = "capacity.create")]
    CapacityCreate { capacity: CapacityInput },
    #[serde(rename = "capacity.update")]
    CapacityUpdate { id: String, capacity: CapacityInput },
    #[serde(rename = "capacity.remove")]
    CapacityRemove { id: String },
    #[serde(rename = "allocation.create")]
    AllocationCreate { allocation: AllocationDraft },
    #[serde(rename = "allocation.update")]
    AllocationUpdate {
        id: String,
        allocation: AllocationDraft,
    },
    #[serde(rename = "allocation.remove")]
    AllocationRemove { id: String },
    #[serde(rename = "allocation.set_locked")]
    AllocationSetLocked { id: String, locked: bool },
    #[serde(rename = "note.create")]
    NoteCreate { note: NoteSpec },
    #[serde(rename = "note.update")]
    NoteUpdate {
        id: String,
        note: NoteSpec,
        expected_version: Option<u64>,
    },
    #[serde(rename = "reminder.create")]
    ReminderCreate {
        task_id: Option<String>,
        message: String,
        fire_at: DateTime<Utc>,
        dedupe_key: Option<String>,
    },
    #[serde(rename = "reminder.cancel")]
    ReminderCancel { id: String },
    #[serde(rename = "plan.apply")]
    PlanApply {
        replace_task_ids: Vec<String>,
        assignments: Vec<AllocationDraft>,
        unplaced: Vec<UnplacedWork>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnplacedWork {
    pub task_id: String,
    pub minutes: u32,
    pub reason: String,
}

impl ChangeOp {
    pub fn validate_shape(&self) -> Result<()> {
        match self {
            Self::TaskCreate { task } | Self::TaskUpdate { task, .. } => task.validate(),
            Self::TaskStatus { status, .. } => {
                if matches!(status, TaskStatus::Open) {
                    return Err(ZodoError::validation(
                        "use task.reopen to move a task back to open",
                    ));
                }
                Ok(())
            }
            Self::GoalCreate { goal } | Self::GoalUpdate { goal, .. } => goal.validate(),
            Self::GoalStatus { .. } => Ok(()),
            Self::ProjectCreate { project } | Self::ProjectUpdate { project, .. } => {
                project.validate()
            }
            Self::ProjectStatus { .. } => Ok(()),
            Self::DependencyAddRef {
                predecessor_ref,
                successor_ref,
            } => {
                if predecessor_ref.trim().is_empty() || successor_ref.trim().is_empty() {
                    return Err(ZodoError::validation("dependency refs must not be empty"));
                }
                Ok(())
            }
            Self::DependencyAdd {
                predecessor_id,
                successor_id,
            } => {
                if predecessor_id == successor_id {
                    return Err(ZodoError::validation("a task cannot depend on itself"));
                }
                Ok(())
            }
            Self::DependencyRemove { .. } => Ok(()),
            Self::TaskSetTags { tags, .. } => {
                if tags.len() > 32 {
                    return Err(ZodoError::validation("at most 32 tags are allowed"));
                }
                Ok(())
            }
            Self::CommitmentCreate { commitment } | Self::CommitmentUpdate { commitment, .. } => {
                commitment.validate()
            }
            Self::CommitmentRemove { .. } => Ok(()),
            Self::CapacityCreate { capacity } | Self::CapacityUpdate { capacity, .. } => {
                capacity.validate()
            }
            Self::CapacityRemove { .. } => Ok(()),
            Self::AllocationCreate { allocation } | Self::AllocationUpdate { allocation, .. } => {
                allocation.validate()
            }
            Self::AllocationRemove { .. } | Self::AllocationSetLocked { .. } => Ok(()),
            Self::NoteCreate { note } | Self::NoteUpdate { note, .. } => note.validate(),
            Self::TaskArchive { .. } | Self::TaskSetBlocked { .. } => Ok(()),
            Self::ReminderCreate { message, .. } => {
                if message.trim().is_empty() {
                    return Err(ZodoError::validation("reminder message is required"));
                }
                Ok(())
            }
            Self::ReminderCancel { .. } => Ok(()),
            Self::PlanApply {
                replace_task_ids,
                assignments,
                ..
            } => {
                if replace_task_ids.len() > MAX_BATCH || assignments.len() > 2000 {
                    return Err(ZodoError::validation("plan proposal is too large"));
                }
                for assignment in assignments {
                    assignment.validate()?;
                }
                Ok(())
            }
        }
    }

    pub fn effects(&self) -> Vec<String> {
        match self {
            Self::ReminderCreate { .. } => vec!["reminder".to_string()],
            Self::PlanApply { .. } => vec!["schedule".to_string()],
            _ => Vec::new(),
        }
    }
}

pub fn validate_title(title: &str) -> Result<()> {
    let count = title.trim().chars().count();
    if count == 0 {
        return Err(ZodoError::validation("title is required"));
    }
    if count > MAX_TITLE_CHARS {
        return Err(ZodoError::validation(format!(
            "title must be at most {MAX_TITLE_CHARS} characters"
        )));
    }
    Ok(())
}

pub fn default_source() -> SourceRef {
    SourceRef::user()
}

pub fn allocation_state_default() -> AllocationState {
    AllocationState::Planned
}
