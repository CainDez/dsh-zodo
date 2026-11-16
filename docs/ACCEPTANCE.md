# Acceptance snapshot

> Current scope (2026-11-07): Rust Core + DSH plugin UI only. The phase/flow table below is historical context, not the result of the current regression. See the [scope decision](<../implementation/SCOPE_DECISION.md>); retired client rows must not count as verified DSH coverage.

This is an honest checkpoint, not a completion certificate. The authoritative per-feature state is
`implementation/FEATURE_LEDGER.csv`; evidence IDs are in `implementation/EVIDENCE_INDEX.md`.

## Phase status

| Phase | Status | Evidence / gaps |
|---|---|---|
| A0 bridge | Verified for tested lifecycle | Tree-out plugin, native tool, React tab, BFF, restart, H22 refusal and live uninstall with independent Core alive. Hot reload without Host restart is not offered. |
| A1 kernel/runtime | Working | 13 A1 tests + store tests; SQLite WAL, receipts, approvals, planner, recurrence. Formal contract fixtures still pending. |
| A2 Harness workbench | Working, matrix partial | Real manual and agent proposal→approval→commit; all main views; exhaustive state/dialog matrix pending. |
| A3 model/MCP/privacy/runs | Partial | Local model, durable runs/cancel/recovery, MCP SDK, revocation, notification, no silent cloud fallback. Cloud success requires a real credential; event cursor and cancel/commit race pending. |
| A4 P1 | Partial | Habits/logs, tags, saved views, milestones, batch, automation manual/schedule, JSON export/import, backup/restore, estimate adjustment, weekly stats. CSV/selection import, incremental calendar, full matrix pending. |
| A5 standalone UI | Retired / out of scope | Removed per 2026-11-07 direction: only the DSH plugin UI is maintained. Historical client evidence is not counted as current acceptance. |
| A6 sync | Partial foundation | Two independent stores round-trip facts; divergence is rejected with 409 conflicts and no partial writes. Encryption/revocation/automatic transport/tombstones pending. |
| A7 final | Not complete | Clean-commit release artifacts + SHA-256 generated (A7-0007); full regression, installers, platform matrix and acceptance close-out not finished. |

## Eight core flows

| Flow | State |
|---|---|
| F01 首次进入到第一项事务 | Partial: first-use panel, capture, task/goal creation; full error/exit branches pending |
| F02 零碎材料到有依据的多个任务 | Partial: Inbox capture/clarify/convert and AI decomposition proposals; full multi-item acceptance pending |
| F03 模糊目标到项目和可执行计划 | Partial: goals/projects, AI plan proposal, approve/commit; full fixture matrix pending |
| F04 日程变化到稳定局部重排 | Partial: capacity/commitment/busy, planner locks/infeasibility; full change/replan acceptance pending |
| F05 从下一步执行到反馈与再次安排 | Partial: focus/progress/review/adjustment proposal tested |
| F06 重复事务与习惯的一次例外 | Partial: habits/logs/recurrence generation; dedicated exception/undo flow pending |
| F07 周回顾到可撤销偏好与下周计划 | Partial: weekly facts, estimate adjustment proposal; reversible preference matrix pending |
| F08 外部 agent 协作到撤权和可审计结果 | Partial: MCP/native tools, revocation, audit; encrypted/automatic sync and full recovery pending |

## Screen/dialog coverage

Implemented views: Today, Inbox, Goals/Projects, Tasks, Schedule, Habits, Saved views, Automation,
Focus, Proposals, AI Runs, Search, Models, Data, Sync, Settings, Privacy, Agents, Audit.
Implemented dialogs include capture, task/goal/project editors, dependency, progress, reminder,
commitment/capacity, busy import, recurrence, plan/proposal approval, occurrence move, goal evidence,
habit editor/log/detail, saved view, milestone, automation.

Not fully covered: onboarding as a standalone screen family, time-object detail/edit screen, full
clarification/state matrix, attachment/reference dialogs, calendar source diagnostics, draft recovery,
command menu states, mobile/responsive/accessibility state matrix.

## Ledger snapshot

Run:

```powershell
Import-Csv implementation\FEATURE_LEDGER.csv | Group-Object status | Select-Object Name,Count
```

Do not claim completion while any P0/P1/P2 row remains `partial`, `not_started` or `designed`.
