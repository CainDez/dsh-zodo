use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::error::{ErrorCode, Result, ZodoError};
use crate::model::{
    self, GoalStatus, PrivacyPolicy, ProjectStatus, ProposalStatus, SCHEMA_COMMAND, SCHEMA_QUERY,
    SCHEMA_RESULT, TaskStatus, WorkspaceSettings,
};
use crate::ops::{
    AllocationDraft, CapacityInput, ChangeOp, CommitmentInput, MAX_BATCH, NoteSpec, ProjectSpec,
    TaskSpec, validate_title,
};
use crate::recurrence::{OccurrenceAction, RecurrenceSpec};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEnvelope {
    pub schema: String,
    pub request_id: String,
    pub idempotency_key: String,
    pub workspace_id: String,
    pub operation: String,
    #[serde(default = "default_operation_version")]
    pub operation_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_revision: Option<u64>,
    pub payload: Value,
}

fn default_operation_version() -> u8 {
    model::OPERATION_VERSION
}

#[derive(Debug, Clone)]
pub struct ParsedCommand {
    pub envelope: CommandEnvelope,
    pub payload: CommandPayload,
    pub canonical_hash: String,
}

impl CommandEnvelope {
    pub fn parse(self) -> Result<ParsedCommand> {
        if self.schema != SCHEMA_COMMAND {
            return Err(ZodoError::new(
                ErrorCode::UnsupportedOperation,
                format!("unsupported command schema {:?}", self.schema),
            ));
        }
        if self.request_id.trim().is_empty() || self.request_id.len() > 200 {
            return Err(ZodoError::validation("request_id must be 1..200 bytes"));
        }
        if self.idempotency_key.trim().is_empty() || self.idempotency_key.len() > 200 {
            return Err(ZodoError::validation(
                "idempotency_key must be 1..200 bytes",
            ));
        }
        if self.workspace_id.trim().is_empty() {
            return Err(ZodoError::validation("workspace_id is required"));
        }
        if self.operation_version != model::OPERATION_VERSION {
            return Err(ZodoError::new(
                ErrorCode::UnsupportedOperation,
                format!("unsupported operation_version {}", self.operation_version),
            ));
        }
        let wrapped = json!({"operation": self.operation, "payload": self.payload});
        let payload: CommandPayload = serde_json::from_value(wrapped)
            .map_err(|error| ZodoError::validation(format!("invalid command payload: {error}")))?;
        payload.validate()?;
        let mut hash_input = BTreeMap::new();
        hash_input.insert("hash_version", json!(model::HASH_VERSION));
        hash_input.insert("workspace_id", json!(self.workspace_id));
        hash_input.insert("operation", json!(self.operation));
        hash_input.insert("operation_version", json!(self.operation_version));
        hash_input.insert(
            "payload",
            serde_json::to_value(&payload).unwrap_or(Value::Null),
        );
        hash_input.insert("base_revision", json!(self.base_revision));
        hash_input.insert("policy_revision", json!(self.policy_revision));
        let canonical = canonical_json(&Value::Object(
            hash_input
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        ));
        let canonical_hash = hex::encode(Sha256::digest(canonical.as_bytes()));
        Ok(ParsedCommand {
            envelope: self,
            payload,
            canonical_hash,
        })
    }
}

/// Supported scopes for `recurrence.update`. The P0 contract only commits series changes from
/// today onward into the future; "this occurrence only" is served by `recurrence.exception`, and
/// whole-series rewriting is not implemented, so other values are rejected instead of silently
/// being treated as `future`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecurrenceUpdateScope {
    Future,
    Series,
}

/// Tell an omitted `time_of_day` (keep the rule value) apart from an explicit JSON `null` (clear
/// it). Plain `Option<Option<T>>` cannot distinguish those two cases through serde.
fn deserialize_optional_clock<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct OptionalClock;

    impl<'de> serde::de::Visitor<'de> for OptionalClock {
        type Value = Option<Option<String>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("null or an HH:MM[:SS] clock string")
        }

        fn visit_none<E>(self) -> std::result::Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(None))
        }

        fn visit_unit<E>(self) -> std::result::Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(None))
        }

        fn visit_some<D2>(self, deserializer: D2) -> std::result::Result<Self::Value, D2::Error>
        where
            D2: serde::Deserializer<'de>,
        {
            String::deserialize(deserializer).map(|text| Some(Some(text)))
        }
    }

    deserializer.deserialize_option(OptionalClock)
}

/// Parse the contract's `HH:MM` and `HH:MM:SS` wall-clock forms. Shared with execution so command
/// validation and the applied rule value can never disagree.
pub(crate) fn parse_clock_time(text: &str) -> Result<NaiveTime> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    let invalid = || ZodoError::validation("time_of_day must be HH:MM or HH:MM:SS");
    let (hour, minute, second) = match parts.as_slice() {
        [hour, minute] => (*hour, *minute, "0"),
        [hour, minute, second] => (*hour, *minute, *second),
        _ => return Err(invalid()),
    };
    let parse_number = |value: &str| -> Result<u32> {
        if value.is_empty() || value.len() > 2 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid());
        }
        value.parse::<u32>().map_err(|_| invalid())
    };
    let hour = parse_number(hour)?;
    let minute = parse_number(minute)?;
    let second = parse_number(second)?;
    NaiveTime::from_hms_opt(hour, minute, second).ok_or_else(invalid)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", content = "payload", rename_all = "snake_case")]
pub enum CommandPayload {
    #[serde(rename = "workspace.update")]
    WorkspaceUpdate {
        settings: Option<WorkspaceSettings>,
        policy: Option<PrivacyPolicy>,
    },
    #[serde(rename = "workspace.rename")]
    WorkspaceRename { name: String },
    #[serde(rename = "capture.collect")]
    CaptureCollect {
        text: String,
        source_kind: Option<String>,
    },
    #[serde(rename = "capture.clarify")]
    CaptureClarify {
        id: String,
        question: String,
        answer: Option<String>,
    },
    #[serde(rename = "capture.convert")]
    CaptureConvert {
        id: String,
        tasks: Vec<TaskSpec>,
        goal: Option<crate::ops::GoalSpec>,
        project: Option<ProjectSpec>,
    },
    #[serde(rename = "capture.dismiss")]
    CaptureDismiss { id: String, reason: Option<String> },
    #[serde(rename = "task.create")]
    TaskCreate { task: TaskSpec },
    #[serde(rename = "task.update")]
    TaskUpdate {
        id: String,
        task: TaskSpec,
        expected_version: Option<u64>,
    },
    #[serde(rename = "task.complete")]
    TaskComplete { id: String, note: Option<String> },
    #[serde(rename = "task.reopen")]
    TaskReopen { id: String },
    #[serde(rename = "task.cancel")]
    TaskCancel { id: String, note: Option<String> },
    #[serde(rename = "task.archive")]
    TaskArchive { id: String, archived: bool },
    #[serde(rename = "task.set_blocked")]
    TaskSetBlocked {
        id: String,
        blocked: bool,
        reason: Option<String>,
    },
    #[serde(rename = "task.set_tags")]
    TaskSetTags { id: String, tags: Vec<String> },
    #[serde(rename = "task.merge")]
    TaskMerge {
        survivor_id: String,
        duplicate_ids: Vec<String>,
    },
    #[serde(rename = "goal.create")]
    GoalCreate { goal: crate::ops::GoalSpec },
    #[serde(rename = "goal.update")]
    GoalUpdate {
        id: String,
        goal: crate::ops::GoalSpec,
    },
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
    #[serde(rename = "dependency.remove")]
    DependencyRemove { id: String },
    #[serde(rename = "note.create")]
    NoteCreate { note: NoteSpec },
    #[serde(rename = "note.update")]
    NoteUpdate {
        id: String,
        note: NoteSpec,
        expected_version: Option<u64>,
    },
    #[serde(rename = "attachment.add")]
    AttachmentAdd {
        note_id: String,
        location: String,
        media_type: String,
        data_class: crate::model::DataClass,
        sha256: Option<String>,
        summary: Option<String>,
    },
    #[serde(rename = "attachment.remove")]
    AttachmentRemove { id: String },
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
    #[serde(rename = "plan.propose")]
    PlanPropose {
        task_ids: Vec<String>,
        horizon_start: DateTime<Utc>,
        horizon_end: DateTime<Utc>,
        move_unlocked: bool,
        intent: Option<String>,
        ttl_seconds: Option<i64>,
    },
    #[serde(rename = "proposal.create")]
    ProposalCreate {
        intent: String,
        operations: Vec<ChangeOp>,
        reads: Option<Vec<model::EntityRef>>,
        assumptions: Option<Vec<String>>,
        questions: Option<Vec<String>>,
        ttl_seconds: Option<i64>,
    },
    #[serde(rename = "proposal.validate")]
    ProposalValidate { proposal_id: String },
    #[serde(rename = "proposal.approve")]
    ProposalApprove {
        proposal_id: String,
        ttl_seconds: Option<i64>,
    },
    #[serde(rename = "proposal.reject")]
    ProposalReject {
        proposal_id: String,
        reason: Option<String>,
    },
    #[serde(rename = "changes.commit")]
    ChangesCommit {
        proposal_id: String,
        approval_grant: String,
    },
    #[serde(rename = "progress.record")]
    ProgressRecord {
        task_id: String,
        minutes: u32,
        remaining_minutes: Option<u32>,
        note: Option<String>,
        occurred_at: Option<DateTime<Utc>>,
        dedupe_key: Option<String>,
    },
    #[serde(rename = "focus.start")]
    FocusStart { task_id: Option<String> },
    #[serde(rename = "focus.pause")]
    FocusPause {},
    #[serde(rename = "focus.resume")]
    FocusResume {},
    #[serde(rename = "focus.finish")]
    FocusFinish { note: Option<String> },
    #[serde(rename = "focus.cancel")]
    FocusCancel {},
    #[serde(rename = "review.build")]
    ReviewBuild {
        period_start: NaiveDate,
        period_end: NaiveDate,
        notes: Option<Vec<String>>,
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
    #[serde(rename = "reminder.mark_delivered")]
    ReminderMarkDelivered {
        id: String,
        delivered_at: Option<DateTime<Utc>>,
        failure: Option<String>,
    },
    #[serde(rename = "recurrence.create")]
    RecurrenceCreate {
        rule: RecurrenceSpec,
        through: Option<NaiveDate>,
    },
    #[serde(rename = "recurrence.expand")]
    RecurrenceExpand { rule_id: String, through: NaiveDate },
    #[serde(rename = "recurrence.exception")]
    RecurrenceException {
        occurrence_id: String,
        action: OccurrenceAction,
        new_date: Option<NaiveDate>,
    },
    #[serde(rename = "recurrence.update")]
    RecurrenceUpdate {
        rule_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<RecurrenceUpdateScope>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        estimate_minutes: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        priority: Option<i32>,
        #[serde(
            default,
            deserialize_with = "deserialize_optional_clock",
            skip_serializing_if = "Option::is_none"
        )]
        time_of_day: Option<Option<String>>,
    },
    #[serde(rename = "calendar.import_busy")]
    CalendarImportBusy {
        source_id: Option<String>,
        provider: String,
        display_name: String,
        external_id: String,
        events: Vec<BusyEvent>,
        replace_source: bool,
        cursor: Option<String>,
    },
    #[serde(rename = "habit.create")]
    HabitCreate { habit: crate::ops::HabitSpec },
    #[serde(rename = "habit.update")]
    HabitUpdate {
        id: String,
        habit: crate::ops::HabitSpec,
    },
    #[serde(rename = "habit.skip")]
    HabitSkip {
        habit_id: String,
        date: chrono::NaiveDate,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    #[serde(rename = "habit.undo")]
    HabitUndo {
        habit_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        activity_id: Option<String>,
    },
    #[serde(rename = "habit.log")]
    HabitLog {
        habit_id: String,
        #[serde(default)]
        occurred_at: Option<DateTime<Utc>>,
        minutes: u32,
        #[serde(default)]
        note: Option<String>,
        #[serde(default)]
        dedupe_key: Option<String>,
    },
    #[serde(rename = "tag.create")]
    TagCreate { tag: crate::ops::TagSpec },
    #[serde(rename = "tag.update")]
    TagUpdate {
        id: String,
        tag: crate::ops::TagSpec,
    },
    #[serde(rename = "tag.delete")]
    TagDelete { id: String },
    #[serde(rename = "saved_query.create")]
    SavedQueryCreate {
        saved_query: crate::ops::SavedQuerySpec,
    },
    #[serde(rename = "saved_query.update")]
    SavedQueryUpdate {
        id: String,
        saved_query: crate::ops::SavedQuerySpec,
    },
    #[serde(rename = "saved_query.delete")]
    SavedQueryDelete { id: String },
    #[serde(rename = "milestone.add")]
    MilestoneAdd {
        project_id: String,
        milestone: crate::ops::MilestoneSpec,
    },
    #[serde(rename = "milestone.update")]
    MilestoneUpdate {
        project_id: String,
        milestone_id: String,
        milestone: crate::ops::MilestoneSpec,
    },
    #[serde(rename = "milestone.remove")]
    MilestoneRemove {
        project_id: String,
        milestone_id: String,
    },
    #[serde(rename = "task.batch_update")]
    TaskBatchUpdate {
        ids: Vec<String>,
        #[serde(default)]
        status: Option<TaskStatus>,
        #[serde(default)]
        archived: Option<bool>,
        #[serde(default)]
        add_tags: Vec<String>,
        #[serde(default)]
        remove_tags: Vec<String>,
        #[serde(default)]
        preview: bool,
    },
    #[serde(rename = "automation.create")]
    AutomationCreate {
        automation: crate::ops::AutomationSpec,
    },
    #[serde(rename = "automation.update")]
    AutomationUpdate {
        id: String,
        automation: crate::ops::AutomationSpec,
    },
    #[serde(rename = "automation.delete")]
    AutomationDelete { id: String },
    #[serde(rename = "automation.toggle")]
    AutomationToggle { id: String, enabled: bool },
    #[serde(rename = "automation.run")]
    AutomationRun { id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BusyEvent {
    pub external_id: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl CommandPayload {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::WorkspaceUpdate { settings, policy } => {
                if let Some(settings) = settings {
                    if settings.work_windows.len() > 64 {
                        return Err(ZodoError::validation("at most 64 work windows are allowed"));
                    }
                    for window in &settings.work_windows {
                        if !(1..=7).contains(&window.weekday) {
                            return Err(ZodoError::validation("work-window weekday must be 1..7"));
                        }
                        if window.start_minute >= window.end_minute || window.end_minute > 24 * 60 {
                            return Err(ZodoError::validation("invalid work-window minutes"));
                        }
                    }
                    if settings.timezone.parse::<chrono_tz::Tz>().is_err() {
                        return Err(ZodoError::validation("timezone must be an IANA zone"));
                    }
                }
                if let Some(policy) = policy {
                    if policy.allowed_cloud_providers.len() > 16 {
                        return Err(ZodoError::validation("too many cloud providers"));
                    }
                }
                Ok(())
            }
            Self::WorkspaceRename { name } => {
                if name.trim().is_empty() || name.chars().count() > 80 {
                    return Err(ZodoError::validation("workspace name must be 1..80 chars"));
                }
                Ok(())
            }
            Self::CaptureCollect { text, .. } => {
                if text.trim().is_empty() {
                    return Err(ZodoError::validation("captured text is required"));
                }
                if text.chars().count() > 20_000 {
                    return Err(ZodoError::validation("captured text is too long"));
                }
                Ok(())
            }
            Self::CaptureClarify { id, question, .. } => {
                if id.trim().is_empty() || question.trim().is_empty() {
                    return Err(ZodoError::validation(
                        "capture id and question are required",
                    ));
                }
                Ok(())
            }
            Self::CaptureConvert { tasks, .. } => {
                if tasks.len() > MAX_BATCH {
                    return Err(ZodoError::validation("too many converted tasks"));
                }
                for task in tasks {
                    task.validate()?;
                }
                Ok(())
            }
            Self::CaptureDismiss { .. } => Ok(()),
            Self::TaskCreate { task } | Self::TaskUpdate { task, .. } => task.validate(),
            Self::TaskComplete { .. }
            | Self::TaskReopen { .. }
            | Self::TaskCancel { .. }
            | Self::TaskArchive { .. }
            | Self::TaskSetBlocked { .. } => Ok(()),
            Self::TaskSetTags { tags, .. } => {
                if tags.len() > 32 {
                    return Err(ZodoError::validation("at most 32 tags are allowed"));
                }
                Ok(())
            }
            Self::GoalCreate { goal } | Self::GoalUpdate { goal, .. } => goal.validate(),
            Self::GoalStatus { .. } => Ok(()),
            Self::ProjectCreate { project } | Self::ProjectUpdate { project, .. } => {
                project.validate()
            }
            Self::ProjectStatus { .. } => Ok(()),
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
            Self::NoteCreate { note } | Self::NoteUpdate { note, .. } => note.validate(),
            Self::AttachmentAdd {
                note_id,
                location,
                media_type,
                sha256,
                summary,
                ..
            } => {
                if note_id.trim().is_empty() {
                    return Err(ZodoError::validation("attachment note_id is required"));
                }
                if location.trim().is_empty() || location.chars().count() > 2000 {
                    return Err(ZodoError::validation(
                        "attachment location must be 1..2000 chars",
                    ));
                }
                if media_type.trim().is_empty() || media_type.chars().count() > 200 {
                    return Err(ZodoError::validation(
                        "attachment media_type must be 1..200 chars",
                    ));
                }
                if let Some(hash) = sha256 {
                    if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Err(ZodoError::validation(
                            "attachment sha256 must be 64 hex chars",
                        ));
                    }
                }
                if summary
                    .as_ref()
                    .is_some_and(|text| text.chars().count() > 1000)
                {
                    return Err(ZodoError::validation("attachment summary is too long"));
                }
                Ok(())
            }
            Self::AttachmentRemove { .. } => Ok(()),
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
            Self::PlanPropose {
                horizon_start,
                horizon_end,
                task_ids,
                ..
            } => {
                if horizon_end <= horizon_start {
                    return Err(ZodoError::validation(
                        "planning horizon end must be after start",
                    ));
                }
                if (*horizon_end - *horizon_start).num_days() > 60 {
                    return Err(ZodoError::validation(
                        "planning horizon is limited to 60 days",
                    ));
                }
                if task_ids.len() > MAX_BATCH {
                    return Err(ZodoError::validation(
                        "at most 200 tasks can be planned at once",
                    ));
                }
                Ok(())
            }
            Self::ProposalCreate {
                intent, operations, ..
            } => {
                if intent.trim().is_empty() {
                    return Err(ZodoError::validation("proposal intent is required"));
                }
                if operations.is_empty() || operations.len() > MAX_BATCH {
                    return Err(ZodoError::validation("proposal needs 1..200 operations"));
                }
                for operation in operations {
                    operation.validate_shape()?;
                }
                Ok(())
            }
            Self::ProposalValidate { .. }
            | Self::ProposalApprove { .. }
            | Self::ProposalReject { .. } => Ok(()),
            Self::ChangesCommit {
                proposal_id,
                approval_grant,
            } => {
                if proposal_id.trim().is_empty() || approval_grant.trim().is_empty() {
                    return Err(ZodoError::validation(
                        "proposal_id and approval_grant are required",
                    ));
                }
                Ok(())
            }
            Self::ProgressRecord {
                minutes, task_id, ..
            } => {
                if task_id.trim().is_empty() {
                    return Err(ZodoError::validation("task_id is required"));
                }
                if *minutes == 0 || *minutes > 24 * 60 {
                    return Err(ZodoError::validation("progress minutes must be 1..1440"));
                }
                Ok(())
            }
            Self::FocusStart { .. }
            | Self::FocusPause {}
            | Self::FocusResume {}
            | Self::FocusFinish { .. }
            | Self::FocusCancel {} => Ok(()),
            Self::ReviewBuild {
                period_start,
                period_end,
                ..
            } => {
                if period_end < period_start {
                    return Err(ZodoError::validation("review period end is before start"));
                }
                if (*period_end - *period_start).num_days() > 366 {
                    return Err(ZodoError::validation(
                        "review period is limited to one year",
                    ));
                }
                Ok(())
            }
            Self::ReminderCreate { message, .. } => {
                if message.trim().is_empty() {
                    return Err(ZodoError::validation("reminder message is required"));
                }
                Ok(())
            }
            Self::ReminderCancel { .. } => Ok(()),
            Self::ReminderMarkDelivered { id, .. } => {
                if id.trim().is_empty() {
                    return Err(ZodoError::validation("reminder id is required"));
                }
                Ok(())
            }
            Self::RecurrenceCreate { rule, .. } => rule.validate(),
            Self::RecurrenceExpand { rule_id, .. } => {
                if rule_id.trim().is_empty() {
                    return Err(ZodoError::validation("rule_id is required"));
                }
                Ok(())
            }
            Self::RecurrenceException {
                occurrence_id,
                action,
                new_date,
            } => {
                if occurrence_id.trim().is_empty() {
                    return Err(ZodoError::validation("occurrence_id is required"));
                }
                if matches!(action, OccurrenceAction::Move) && new_date.is_none() {
                    return Err(ZodoError::validation("move requires new_date"));
                }
                Ok(())
            }
            Self::RecurrenceUpdate {
                rule_id,
                title,
                estimate_minutes,
                priority,
                time_of_day,
                ..
            } => {
                if rule_id.trim().is_empty() {
                    return Err(ZodoError::validation("rule_id is required"));
                }
                if let Some(title) = title {
                    validate_title(title)?;
                }
                if let Some(estimate) = estimate_minutes {
                    if !(15..=1440).contains(estimate) {
                        return Err(ZodoError::validation(
                            "estimate_minutes must be 15..1440 minutes",
                        ));
                    }
                }
                if let Some(priority) = priority {
                    if !(0..=4).contains(priority) {
                        return Err(ZodoError::validation("priority must be 0..4"));
                    }
                }
                if let Some(Some(time_of_day)) = time_of_day {
                    parse_clock_time(time_of_day)?;
                }
                Ok(())
            }
            Self::CalendarImportBusy {
                provider, events, ..
            } => {
                if provider.trim().is_empty() || events.len() > 10_000 {
                    return Err(ZodoError::validation("invalid busy import"));
                }
                for event in events {
                    if event.end <= event.start {
                        return Err(ZodoError::validation("busy event end must be after start"));
                    }
                }
                Ok(())
            }
            Self::HabitCreate { habit } | Self::HabitUpdate { habit, .. } => habit.validate(),
            Self::HabitSkip { habit_id, .. } | Self::HabitUndo { habit_id, .. } => {
                if habit_id.trim().is_empty() {
                    return Err(ZodoError::validation("habit_id is required"));
                }
                Ok(())
            }
            Self::HabitLog {
                habit_id, minutes, ..
            } => {
                if habit_id.trim().is_empty() {
                    return Err(ZodoError::validation("habit_id is required"));
                }
                if *minutes == 0 || *minutes > 24 * 60 {
                    return Err(ZodoError::validation("habit minutes must be 1..1440"));
                }
                Ok(())
            }
            Self::TagCreate { tag } | Self::TagUpdate { tag, .. } => tag.validate(),
            Self::TagDelete { .. } => Ok(()),
            Self::SavedQueryCreate { saved_query } | Self::SavedQueryUpdate { saved_query, .. } => {
                saved_query.validate()
            }
            Self::SavedQueryDelete { .. } => Ok(()),
            Self::MilestoneAdd { milestone, .. } | Self::MilestoneUpdate { milestone, .. } => {
                milestone.validate()
            }
            Self::MilestoneRemove { .. } => Ok(()),
            Self::TaskBatchUpdate {
                ids,
                add_tags,
                remove_tags,
                ..
            } => {
                if ids.is_empty() || ids.len() > MAX_BATCH {
                    return Err(ZodoError::validation("batch needs 1..200 task ids"));
                }
                let unique = ids.iter().collect::<std::collections::BTreeSet<_>>();
                if unique.len() != ids.len() {
                    return Err(ZodoError::validation("batch task ids must be unique"));
                }
                if add_tags.len() > 32 || remove_tags.len() > 32 {
                    return Err(ZodoError::validation("batch tag lists are limited to 32"));
                }
                Ok(())
            }
            Self::TaskMerge {
                survivor_id,
                duplicate_ids,
            } => {
                if survivor_id.trim().is_empty()
                    || duplicate_ids.is_empty()
                    || duplicate_ids.len() > 50
                {
                    return Err(ZodoError::validation(
                        "task.merge needs a survivor and 1..50 duplicates",
                    ));
                }
                let unique = duplicate_ids
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>();
                if unique.len() != duplicate_ids.len() {
                    return Err(ZodoError::validation(
                        "task.merge duplicate ids must be unique",
                    ));
                }
                if duplicate_ids.iter().any(|id| id == survivor_id) {
                    return Err(ZodoError::validation(
                        "survivor cannot be one of the duplicates",
                    ));
                }
                Ok(())
            }
            Self::AutomationCreate { automation } | Self::AutomationUpdate { automation, .. } => {
                automation.validate()
            }
            Self::AutomationDelete { .. } | Self::AutomationToggle { .. } => Ok(()),
            Self::AutomationRun { id } => {
                if id.trim().is_empty() {
                    return Err(ZodoError::validation("automation id is required"));
                }
                Ok(())
            }
        }
    }

    pub fn operation_name(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|value| {
                value
                    .get("operation")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "unknown".to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResultStatus {
    Committed,
    Proposed,
    Validated,
    Approved,
    Rejected,
    Accepted,
    Ok,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub schema: String,
    pub request_id: String,
    pub operation_id: String,
    pub status: ResultStatus,
    pub revision: u64,
    pub data: Value,
    pub event_cursor: String,
    pub replayed: bool,
}

impl CommandResult {
    pub fn new(
        request_id: String,
        operation_id: String,
        status: ResultStatus,
        revision: u64,
        data: Value,
        event_cursor: String,
        replayed: bool,
    ) -> Self {
        Self {
            schema: SCHEMA_RESULT.to_string(),
            request_id,
            operation_id,
            status,
            revision,
            data,
            event_cursor,
            replayed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCommandResult {
    pub operation_id: String,
    pub status: ResultStatus,
    pub revision: u64,
    pub data: Value,
    pub event_cursor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryEnvelope {
    pub schema: String,
    pub request_id: String,
    pub workspace_id: String,
    pub query: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedQuery {
    pub envelope: QueryEnvelope,
    pub payload: QueryPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "query", content = "payload", rename_all = "snake_case")]
pub enum QueryPayload {
    #[serde(rename = "workspace.get")]
    WorkspaceGet {},
    #[serde(rename = "workspace.snapshot")]
    WorkspaceSnapshot {},
    #[serde(rename = "today.get")]
    TodayGet { date: Option<NaiveDate> },
    #[serde(rename = "inbox.list")]
    InboxList {
        status: Option<model::InboxStatus>,
        limit: Option<u32>,
    },
    #[serde(rename = "goals.list")]
    GoalsList { include_archived: Option<bool> },
    #[serde(rename = "projects.list")]
    ProjectsList {},
    #[serde(rename = "tasks.list")]
    TasksList { filter: Option<TaskFilter> },
    #[serde(rename = "task.get")]
    TaskGet { id: String },
    #[serde(rename = "tasks.duplicates")]
    TasksDuplicates {},
    #[serde(rename = "content.retrieve")]
    ContentRetrieve {
        text: String,
        #[serde(default)]
        consent: bool,
        #[serde(default)]
        personalise: bool,
        limit: Option<u32>,
    },
    #[serde(rename = "schedule.get")]
    ScheduleGet {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    },
    #[serde(rename = "capacity.candidates")]
    CapacityCandidates {
        start: NaiveDate,
        end: NaiveDate,
        min_minutes: Option<u32>,
        max_candidates: Option<u32>,
    },
    #[serde(rename = "plan.context")]
    PlanContext {
        task_ids: Vec<String>,
        horizon_start: DateTime<Utc>,
        horizon_end: DateTime<Utc>,
    },
    #[serde(rename = "proposals.list")]
    ProposalsList { status: Option<ProposalStatus> },
    #[serde(rename = "proposal.get")]
    ProposalGet { id: String },
    #[serde(rename = "focus.current")]
    FocusCurrent {},
    #[serde(rename = "reviews.list")]
    ReviewsList {},
    #[serde(rename = "review.get")]
    ReviewGet { id: String },
    #[serde(rename = "search.query")]
    SearchQuery {
        text: String,
        kinds: Option<Vec<String>>,
        limit: Option<u32>,
        offset: Option<u32>,
    },
    #[serde(rename = "stats.get")]
    StatsGet { start: NaiveDate, end: NaiveDate },
    #[serde(rename = "events.list")]
    EventsList {
        after_cursor: Option<String>,
        limit: Option<u32>,
    },
    #[serde(rename = "audit.list")]
    AuditList {
        after_seq: Option<u64>,
        limit: Option<u32>,
    },
    #[serde(rename = "reminders.list")]
    RemindersList {
        status: Option<model::ReminderStatus>,
    },
    #[serde(rename = "recurrences.list")]
    RecurrencesList {},
    #[serde(rename = "occurrences.list")]
    OccurrencesList { rule_id: Option<String> },
    #[serde(rename = "runs.list")]
    RunsList {},
    #[serde(rename = "run.get")]
    RunGet { id: String },
    #[serde(rename = "health.info")]
    HealthInfo {},
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default)]
    pub only_leaf: bool,
    #[serde(default)]
    pub overdue: bool,
    pub limit: Option<u32>,
    #[serde(default)]
    pub offset: u32,
}

impl QueryEnvelope {
    pub fn parse(self) -> Result<ParsedQuery> {
        if self.schema != SCHEMA_QUERY {
            return Err(ZodoError::new(
                ErrorCode::UnsupportedOperation,
                "unsupported query schema",
            ));
        }
        if self.request_id.trim().is_empty() || self.request_id.len() > 200 {
            return Err(ZodoError::validation("request_id must be 1..200 bytes"));
        }
        let wrapped = json!({"query": self.query, "payload": self.payload});
        let payload: QueryPayload = serde_json::from_value(wrapped)
            .map_err(|error| ZodoError::validation(format!("invalid query payload: {error}")))?;
        Ok(ParsedQuery {
            envelope: self,
            payload,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub schema: String,
    pub request_id: String,
    pub revision: u64,
    pub event_cursor: String,
    pub data: Value,
}

impl QueryResult {
    pub fn new(request_id: String, revision: u64, event_cursor: String, data: Value) -> Self {
        Self {
            schema: "zodo.query-result/1".to_string(),
            request_id,
            revision,
            event_cursor,
            data,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEnvelope {
    pub schema: String,
    pub request_id: String,
    pub error: ErrorBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    pub details: Value,
}

impl ErrorEnvelope {
    pub fn new(request_id: impl Into<String>, error: &ZodoError) -> Self {
        Self {
            schema: model::SCHEMA_ERROR.to_string(),
            request_id: request_id.into(),
            error: ErrorBody {
                code: error.code.clone(),
                message: error.message.clone(),
                retryable: error.retryable,
                details: error.details.clone(),
            },
        }
    }
}

pub fn canonical_json(value: &Value) -> String {
    fn write(value: &Value, out: &mut String) {
        match value {
            Value::Null => out.push_str("null"),
            Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Value::Number(value) => out.push_str(&value.to_string()),
            Value::String(value) => {
                out.push_str(&serde_json::to_string(value).expect("string serializes"))
            }
            Value::Array(values) => {
                out.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write(value, out);
                }
                out.push(']');
            }
            Value::Object(values) => {
                let sorted: BTreeMap<&String, &Value> = values.iter().collect();
                out.push('{');
                for (index, (key, value)) in sorted.into_iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(key).expect("key serializes"));
                    out.push(':');
                    write(value, out);
                }
                out.push('}');
            }
        }
    }
    let mut out = String::new();
    write(value, &mut out);
    out
}

pub fn object_without_nulls(value: Value) -> Value {
    match value {
        Value::Object(values) => {
            let mut out = Map::new();
            for (key, value) in values {
                if !value.is_null() {
                    out.insert(key, object_without_nulls(value));
                }
            }
            Value::Object(out)
        }
        Value::Array(values) => {
            Value::Array(values.into_iter().map(object_without_nulls).collect())
        }
        other => other,
    }
}
