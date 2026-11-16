//! Zodo kernel.
//!
//! This crate owns the personal-operations domain, deterministic validation, planning proposals,
//! the command/query application surface, and transport-neutral ports. It must not depend on
//! Cordis, React, Flutter, MCP, SQLite or a model SDK.

pub mod app;
pub mod apply;
pub mod commands;
pub mod error;
pub mod model;
pub mod ops;
pub mod planner;
pub mod ports;
pub mod proposal;
pub mod recurrence;

pub use app::Application;
pub use commands::{
    CommandEnvelope, CommandPayload, CommandResult, QueryEnvelope, QueryPayload, QueryResult,
    ResultStatus,
};
pub use error::{ErrorCode, Result, ZodoError};
pub use model::{
    ApprovalGrant, CommandContext, InboxEntry, InboxStatus, PrincipalKind, Proposal,
    ProposalStatus, Task, TaskStatus, Workspace, World,
};
pub use ports::{Clock, FixedClock, IdSource, SequentialIds, Store, SystemClock, UuidIds};
