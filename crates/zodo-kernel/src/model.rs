use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{Result, ZodoError};

pub const SCHEMA_COMMAND: &str = "zodo.command/1";
pub const SCHEMA_QUERY: &str = "zodo.query/1";
pub const SCHEMA_RESULT: &str = "zodo.result/1";
pub const SCHEMA_ERROR: &str = "zodo.error/1";
pub const SCHEMA_EVENT: &str = "zodo.event/1";
pub const OPERATION_VERSION: u8 = 1;
pub const HASH_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    Owner,
    InteractiveUi,
    Cli,
    DelegatedAgent,
    McpClient,
    Automation,
    System,
}

impl PrincipalKind {
    pub fn can_approve(&self) -> bool {
        matches!(self, Self::Owner | Self::InteractiveUi | Self::Cli)
    }

    pub fn is_agent(&self) -> bool {
        matches!(
            self,
            Self::DelegatedAgent | Self::McpClient | Self::Automation
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandContext {
    pub principal_id: String,
    pub principal_kind: PrincipalKind,
    pub channel: String,
    pub capabilities: Vec<String>,
    pub workspace_id: String,
    pub request_id: String,
    pub received_at: DateTime<Utc>,
    pub runtime_instance: String,
}

impl CommandContext {
    pub fn has(&self, capability: &str) -> bool {
        self.capabilities
            .iter()
            .any(|value| value == capability || value == "*")
    }

    pub fn require(&self, capability: &str) -> Result<()> {
        if self.has(capability) {
            Ok(())
        } else {
            Err(ZodoError::new(
                crate::error::ErrorCode::PermissionDenied,
                format!(
                    "principal {} lacks capability {capability}",
                    self.principal_id
                ),
            ))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DataClass {
    DeviceOnly,
    TrustedWorkspace,
    CloudAllowed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WeekendPolicy {
    Ask,
    Allow,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkWindow {
    /// ISO weekday, Monday = 1.
    pub weekday: u8,
    pub start_minute: u16,
    pub end_minute: u16,
    pub hard: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelAccess {
    pub goals: bool,
    pub projects: bool,
    pub captures: bool,
}

/// Content consent is independent of the selected model connection.
/// Notes and attachments are not automatically included in any model request.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PrivacyPolicy {
    pub model_access: ModelAccess,
    pub diagnostic_capture: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceSettings {
    pub timezone: String,
    pub week_start: u8,
    pub weekend_policy: WeekendPolicy,
    pub work_windows: Vec<WorkWindow>,
    pub default_estimate_minutes: u32,
    pub daily_limit_minutes: Option<u32>,
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            timezone: "Asia/Shanghai".to_string(),
            week_start: 1,
            weekend_policy: WeekendPolicy::Ask,
            work_windows: vec![
                WorkWindow {
                    weekday: 1,
                    start_minute: 9 * 60,
                    end_minute: 12 * 60,
                    hard: false,
                },
                WorkWindow {
                    weekday: 1,
                    start_minute: 14 * 60,
                    end_minute: 18 * 60,
                    hard: false,
                },
                WorkWindow {
                    weekday: 2,
                    start_minute: 9 * 60,
                    end_minute: 12 * 60,
                    hard: false,
                },
                WorkWindow {
                    weekday: 2,
                    start_minute: 14 * 60,
                    end_minute: 18 * 60,
                    hard: false,
                },
                WorkWindow {
                    weekday: 3,
                    start_minute: 9 * 60,
                    end_minute: 12 * 60,
                    hard: false,
                },
                WorkWindow {
                    weekday: 3,
                    start_minute: 14 * 60,
                    end_minute: 18 * 60,
                    hard: false,
                },
                WorkWindow {
                    weekday: 4,
                    start_minute: 9 * 60,
                    end_minute: 12 * 60,
                    hard: false,
                },
                WorkWindow {
                    weekday: 4,
                    start_minute: 14 * 60,
                    end_minute: 18 * 60,
                    hard: false,
                },
                WorkWindow {
                    weekday: 5,
                    start_minute: 9 * 60,
                    end_minute: 12 * 60,
                    hard: false,
                },
                WorkWindow {
                    weekday: 5,
                    start_minute: 14 * 60,
                    end_minute: 18 * 60,
                    hard: false,
                },
            ],
            default_estimate_minutes: 30,
            daily_limit_minutes: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revision: u64,
    pub policy_revision: u64,
    pub settings: WorkspaceSettings,
    pub policy: PrivacyPolicy,
    pub is_demo: bool,
}

impl Workspace {
    pub fn new(id: String, name: String, now: DateTime<Utc>) -> Self {
        Self {
            id,
            name,
            created_at: now,
            updated_at: now,
            revision: 0,
            policy_revision: 1,
            settings: WorkspaceSettings::default(),
            policy: PrivacyPolicy::default(),
            is_demo: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceRef {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
}

impl SourceRef {
    pub fn user() -> Self {
        Self {
            kind: "user".to_string(),
            id: None,
            actor_id: None,
        }
    }

    pub fn system() -> Self {
        Self {
            kind: "system".to_string(),
            id: None,
            actor_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityRef {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InboxStatus {
    Unprocessed,
    NeedsClarification,
    Processed,
    Dismissed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Clarification {
    pub id: String,
    pub question: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    pub asked_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answered_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InboxEntry {
    pub id: String,
    pub workspace_id: String,
    pub text: String,
    pub source: SourceRef,
    pub status: InboxStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub clarifications: Vec<Clarification>,
    #[serde(default)]
    pub converted_refs: Vec<EntityRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dismissed_reason: Option<String>,
    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Paused,
    Achieved,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalEvidence {
    pub text: String,
    pub recorded_at: DateTime<Utc>,
    pub source: SourceRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Goal {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success_criteria: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_due: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hard_deadline: Option<DateTime<Utc>>,
    pub status: GoalStatus,
    #[serde(default)]
    pub evidence: Vec<GoalEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub achieved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Active,
    Paused,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Milestone {
    pub id: String,
    pub title: String,
    pub due: Option<NaiveDate>,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
    pub status: ProjectStatus,
    #[serde(default)]
    pub milestones: Vec<Milestone>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Open,
    InProgress,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimeWindow {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub status: TaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_minutes: Option<u32>,
    #[serde(default)]
    pub actual_minutes: u32,
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
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub blocked_manual: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occurrence_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dependency {
    pub id: String,
    pub workspace_id: String,
    pub predecessor_id: String,
    pub successor_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Commitment {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub locked: bool,
    pub source: SourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calendar_source_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapacityBlock {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub capacity_minutes: u32,
    pub locked: bool,
    pub source: SourceRef,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AllocationState {
    Planned,
    Started,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Allocation {
    pub id: String,
    pub workspace_id: String,
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity_block_id: Option<String>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub minutes: u32,
    pub locked: bool,
    pub state: AllocationState,
    pub source: SourceRef,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Draft,
    Validated,
    Applied,
    Rejected,
    Stale,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Proposal {
    pub id: String,
    pub workspace_id: String,
    pub intent: String,
    pub source: SourceRef,
    pub actor_id: String,
    pub actor_kind: PrincipalKind,
    pub base_revision: u64,
    pub policy_revision: u64,
    pub operations: serde_json::Value,
    #[serde(default)]
    pub reads: Vec<EntityRef>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub questions: Vec<String>,
    #[serde(default)]
    pub diagnostics: Vec<Diagnostic>,
    pub content_hash: String,
    pub status: ProposalStatus,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_set_id: Option<String>,
    #[serde(default)]
    pub preview: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalGrant {
    pub id: String,
    pub workspace_id: String,
    pub proposal_id: String,
    pub content_hash: String,
    pub base_revision: u64,
    pub policy_revision: u64,
    pub principal_id: String,
    pub channel: String,
    #[serde(default)]
    pub effects: Vec<String>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_revision: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub related_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FocusState {
    Running,
    Paused,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FocusSegment {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FocusSession {
    pub id: String,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub state: FocusState,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_at: Option<DateTime<Utc>>,
    /// Start of the currently running segment; absent while paused or ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub segments: Vec<FocusSegment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,
    pub accumulated_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    Focus,
    Progress,
    Manual,
    Habit,
    HabitSkip,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActivityRecord {
    pub id: String,
    pub workspace_id: String,
    pub kind: ActivityKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub habit_id: Option<String>,
    pub minutes: u32,
    pub occurred_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub source: SourceRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReminderStatus {
    Pending,
    Delivered,
    Failed,
    Cancelled,
    NeedsReconciliation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Reminder {
    pub id: String,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub message: String,
    pub fire_at: DateTime<Utc>,
    pub status: ReminderStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Review {
    pub id: String,
    pub workspace_id: String,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub facts: ReviewFacts,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub recommendations: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub source: SourceRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewFacts {
    pub tasks_completed: u32,
    pub tasks_opened: u32,
    pub focus_minutes: u32,
    pub actual_minutes: u32,
    pub overdue_open: u32,
    pub blocked_open: u32,
    pub active_goals: u32,
    pub goals_achieved: u32,
    pub habit_completions: u32,
    #[serde(default)]
    pub habit_target: u32,
    #[serde(default)]
    pub habit_adherence_percent: u32,
    pub unplaced_minutes: u32,
    pub focus_sessions: u32,
    pub activity_records: u32,
}

impl ReviewFacts {
    pub fn empty() -> Self {
        Self {
            tasks_completed: 0,
            tasks_opened: 0,
            focus_minutes: 0,
            actual_minutes: 0,
            overdue_open: 0,
            blocked_open: 0,
            active_goals: 0,
            goals_achieved: 0,
            habit_completions: 0,
            habit_target: 0,
            habit_adherence_percent: 0,
            unplaced_minutes: 0,
            focus_sessions: 0,
            activity_records: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tag {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Note {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub body: String,
    pub source: SourceRef,
    pub data_class: DataClass,
    #[serde(default)]
    pub linked_tasks: Vec<String>,
    #[serde(default)]
    pub linked_goals: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachmentRef {
    pub id: String,
    pub workspace_id: String,
    pub note_id: String,
    pub location: String,
    pub media_type: String,
    pub data_class: DataClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedQuery {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub filter: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalendarSource {
    pub id: String,
    pub workspace_id: String,
    pub provider: String,
    pub display_name: String,
    pub external_id: String,
    pub writable: bool,
    pub last_sync_at: DateTime<Utc>,
    pub freshness_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CalendarEvent {
    pub id: String,
    pub workspace_id: String,
    pub source_id: String,
    pub external_id: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub busy: bool,
    pub updated_at: DateTime<Utc>,
    pub fetched_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTrigger {
    Schedule,
    TaskStatus,
    Reminder,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationRule {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub trigger: AutomationTrigger,
    pub conditions: serde_json::Value,
    pub actions: serde_json::Value,
    pub enabled: bool,
    pub budget: u32,
    pub runs: u32,
    pub last_run_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Habit {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub timezone: String,
    pub start_date: NaiveDate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<NaiveDate>,
    pub weekly_target: u32,
    pub scheduled_weekdays: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_of_day: Option<NaiveTime>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecurrenceFrequency {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecurrenceRule {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate_minutes: Option<u32>,
    pub priority: i32,
    pub timezone: String,
    pub start_date: NaiveDate,
    pub frequency: RecurrenceFrequency,
    pub interval: u32,
    #[serde(default)]
    pub weekdays: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_of_day: Option<NaiveTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_occurrences: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_completion_days: Option<u32>,
    #[serde(default)]
    pub depends_on_previous: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceStatus {
    Generated,
    Skipped,
    Moved,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Occurrence {
    pub id: String,
    pub workspace_id: String,
    pub rule_id: String,
    /// Stable local-date key. A move changes `current_date` but never this key.
    pub occurrence_key: String,
    pub original_date: NaiveDate,
    pub current_date: NaiveDate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub status: OccurrenceStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Complete authoritative facts for one workspace. Model and transport types never enter this
/// aggregate; it serializes only domain values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct World {
    pub workspace: Workspace,
    #[serde(default)]
    pub inbox: Vec<InboxEntry>,
    #[serde(default)]
    pub goals: Vec<Goal>,
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub commitments: Vec<Commitment>,
    #[serde(default)]
    pub capacity_blocks: Vec<CapacityBlock>,
    #[serde(default)]
    pub allocations: Vec<Allocation>,
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub notes: Vec<Note>,
    #[serde(default)]
    pub attachments: Vec<AttachmentRef>,
    #[serde(default)]
    pub saved_queries: Vec<SavedQuery>,
    #[serde(default)]
    pub reminders: Vec<Reminder>,
    #[serde(default)]
    pub focus_sessions: Vec<FocusSession>,
    #[serde(default)]
    pub activities: Vec<ActivityRecord>,
    #[serde(default)]
    pub reviews: Vec<Review>,
    #[serde(default)]
    pub habits: Vec<Habit>,
    #[serde(default)]
    pub recurrence_rules: Vec<RecurrenceRule>,
    #[serde(default)]
    pub occurrences: Vec<Occurrence>,
    #[serde(default)]
    pub proposals: Vec<Proposal>,
    #[serde(default)]
    pub approval_grants: Vec<ApprovalGrant>,
    #[serde(default)]
    pub calendar_sources: Vec<CalendarSource>,
    #[serde(default)]
    pub calendar_events: Vec<CalendarEvent>,
    #[serde(default)]
    pub automation_rules: Vec<AutomationRule>,
}

impl World {
    pub fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            inbox: Vec::new(),
            goals: Vec::new(),
            projects: Vec::new(),
            tasks: Vec::new(),
            dependencies: Vec::new(),
            commitments: Vec::new(),
            capacity_blocks: Vec::new(),
            allocations: Vec::new(),
            tags: Vec::new(),
            notes: Vec::new(),
            attachments: Vec::new(),
            saved_queries: Vec::new(),
            reminders: Vec::new(),
            focus_sessions: Vec::new(),
            activities: Vec::new(),
            reviews: Vec::new(),
            habits: Vec::new(),
            recurrence_rules: Vec::new(),
            occurrences: Vec::new(),
            proposals: Vec::new(),
            approval_grants: Vec::new(),
            calendar_sources: Vec::new(),
            calendar_events: Vec::new(),
            automation_rules: Vec::new(),
        }
    }

    pub fn task(&self, id: &str) -> Option<&Task> {
        self.tasks.iter().find(|task| task.id == id)
    }

    pub fn task_mut(&mut self, id: &str) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|task| task.id == id)
    }

    pub fn goal(&self, id: &str) -> Option<&Goal> {
        self.goals.iter().find(|goal| goal.id == id)
    }

    pub fn project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|project| project.id == id)
    }

    pub fn inbox_entry(&self, id: &str) -> Option<&InboxEntry> {
        self.inbox.iter().find(|entry| entry.id == id)
    }

    pub fn children_of(&self, task_id: &str) -> Vec<&Task> {
        self.tasks
            .iter()
            .filter(|task| task.parent_id.as_deref() == Some(task_id))
            .collect()
    }

    pub fn is_leaf(&self, task_id: &str) -> bool {
        !self
            .tasks
            .iter()
            .any(|task| task.parent_id.as_deref() == Some(task_id) && !task.archived)
    }

    pub fn dependencies_of(&self, task_id: &str) -> Vec<&Dependency> {
        self.dependencies
            .iter()
            .filter(|dep| dep.successor_id == task_id)
            .collect()
    }

    pub fn successors_of(&self, task_id: &str) -> Vec<&Dependency> {
        self.dependencies
            .iter()
            .filter(|dep| dep.predecessor_id == task_id)
            .collect()
    }

    pub fn completed(&self, task_id: &str) -> bool {
        self.task(task_id)
            .is_some_and(|task| task.status == TaskStatus::Done)
    }

    pub fn would_create_dependency_cycle(&self, predecessor: &str, successor: &str) -> bool {
        if predecessor == successor {
            return true;
        }
        // A new edge predecessor -> successor is a cycle iff predecessor is reachable from successor.
        let mut stack = vec![successor.to_string()];
        let mut seen = BTreeSet::new();
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            if current == predecessor {
                return true;
            }
            for edge in self
                .dependencies
                .iter()
                .filter(|edge| edge.predecessor_id == current)
            {
                stack.push(edge.successor_id.clone());
            }
        }
        false
    }

    pub fn ancestor_chain(&self, task_id: &str) -> Vec<String> {
        let mut chain = Vec::new();
        let mut seen = BTreeSet::new();
        let mut current = self.task(task_id).and_then(|task| task.parent_id.clone());
        while let Some(id) = current {
            if !seen.insert(id.clone()) {
                break;
            }
            chain.push(id.clone());
            current = self.task(&id).and_then(|task| task.parent_id.clone());
        }
        chain
    }

    pub fn open_leaf_tasks(&self) -> Vec<&Task> {
        self.tasks
            .iter()
            .filter(|task| {
                !task.archived
                    && !matches!(task.status, TaskStatus::Done | TaskStatus::Cancelled)
                    && self.is_leaf(&task.id)
            })
            .collect()
    }

    pub fn active_focus(&self) -> Option<&FocusSession> {
        self.focus_sessions
            .iter()
            .find(|session| matches!(session.state, FocusState::Running | FocusState::Paused))
    }

    pub fn allocation_minutes_for_task(&self, task_id: &str, after: Option<DateTime<Utc>>) -> u32 {
        self.allocations
            .iter()
            .filter(|allocation| allocation.task_id == task_id)
            .filter(|allocation| after.is_none_or(|time| allocation.end > time))
            .map(|allocation| allocation.minutes)
            .sum()
    }

    pub fn entity_json_map(&self) -> BTreeMap<(String, String), serde_json::Value> {
        fn insert<T: Serialize>(
            map: &mut BTreeMap<(String, String), serde_json::Value>,
            kind: &str,
            items: &[T],
        ) {
            for item in items {
                let value = serde_json::to_value(item).expect("domain value serializes");
                let id = value
                    .get("id")
                    .and_then(|id| id.as_str())
                    .unwrap_or_default()
                    .to_string();
                map.insert((kind.to_string(), id), value);
            }
        }
        let mut map = BTreeMap::new();
        insert(&mut map, "inbox", &self.inbox);
        insert(&mut map, "goal", &self.goals);
        insert(&mut map, "project", &self.projects);
        insert(&mut map, "task", &self.tasks);
        insert(&mut map, "dependency", &self.dependencies);
        insert(&mut map, "commitment", &self.commitments);
        insert(&mut map, "capacity_block", &self.capacity_blocks);
        insert(&mut map, "allocation", &self.allocations);
        insert(&mut map, "tag", &self.tags);
        insert(&mut map, "note", &self.notes);
        insert(&mut map, "attachment", &self.attachments);
        insert(&mut map, "saved_query", &self.saved_queries);
        insert(&mut map, "reminder", &self.reminders);
        insert(&mut map, "focus_session", &self.focus_sessions);
        insert(&mut map, "activity", &self.activities);
        insert(&mut map, "review", &self.reviews);
        insert(&mut map, "habit", &self.habits);
        insert(&mut map, "recurrence_rule", &self.recurrence_rules);
        insert(&mut map, "occurrence", &self.occurrences);
        insert(&mut map, "proposal", &self.proposals);
        insert(&mut map, "approval_grant", &self.approval_grants);
        insert(&mut map, "calendar_source", &self.calendar_sources);
        insert(&mut map, "calendar_event", &self.calendar_events);
        insert(&mut map, "automation_rule", &self.automation_rules);
        map
    }

    pub fn from_entities(
        workspace: Workspace,
        entities: BTreeMap<(String, String), serde_json::Value>,
    ) -> Result<Self> {
        let mut world = Self::new(workspace);
        for ((kind, _id), value) in entities {
            match kind.as_str() {
                "inbox" => world.inbox.push(parse_entity::<InboxEntry>(&kind, value)?),
                "goal" => world.goals.push(parse_entity::<Goal>(&kind, value)?),
                "project" => world.projects.push(parse_entity::<Project>(&kind, value)?),
                "task" => world.tasks.push(parse_entity::<Task>(&kind, value)?),
                "dependency" => world
                    .dependencies
                    .push(parse_entity::<Dependency>(&kind, value)?),
                "commitment" => world
                    .commitments
                    .push(parse_entity::<Commitment>(&kind, value)?),
                "capacity_block" => world
                    .capacity_blocks
                    .push(parse_entity::<CapacityBlock>(&kind, value)?),
                "allocation" => world
                    .allocations
                    .push(parse_entity::<Allocation>(&kind, value)?),
                "tag" => world.tags.push(parse_entity::<Tag>(&kind, value)?),
                "note" => world.notes.push(parse_entity::<Note>(&kind, value)?),
                "attachment" => world
                    .attachments
                    .push(parse_entity::<AttachmentRef>(&kind, value)?),
                "saved_query" => world
                    .saved_queries
                    .push(parse_entity::<SavedQuery>(&kind, value)?),
                "reminder" => world
                    .reminders
                    .push(parse_entity::<Reminder>(&kind, value)?),
                "focus_session" => world
                    .focus_sessions
                    .push(parse_entity::<FocusSession>(&kind, value)?),
                "activity" => world
                    .activities
                    .push(parse_entity::<ActivityRecord>(&kind, value)?),
                "review" => world.reviews.push(parse_entity::<Review>(&kind, value)?),
                "habit" => world.habits.push(parse_entity::<Habit>(&kind, value)?),
                "recurrence_rule" => world
                    .recurrence_rules
                    .push(parse_entity::<RecurrenceRule>(&kind, value)?),
                "occurrence" => world
                    .occurrences
                    .push(parse_entity::<Occurrence>(&kind, value)?),
                "proposal" => world
                    .proposals
                    .push(parse_entity::<Proposal>(&kind, value)?),
                "approval_grant" => world
                    .approval_grants
                    .push(parse_entity::<ApprovalGrant>(&kind, value)?),
                "calendar_source" => world
                    .calendar_sources
                    .push(parse_entity::<CalendarSource>(&kind, value)?),
                "calendar_event" => world
                    .calendar_events
                    .push(parse_entity::<CalendarEvent>(&kind, value)?),
                "automation_rule" => world
                    .automation_rules
                    .push(parse_entity::<AutomationRule>(&kind, value)?),
                _ => {}
            }
        }
        Ok(world)
    }

    pub fn next_task_id(&self) -> usize {
        self.tasks.len() + 1
    }
}

fn parse_entity<T: serde::de::DeserializeOwned>(kind: &str, value: serde_json::Value) -> Result<T> {
    serde_json::from_value(value)
        .map_err(|error| ZodoError::validation(format!("stored {kind} is invalid: {error}")))
}
