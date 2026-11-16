# Progress

Updated: 2026-11-07 (Asia/Shanghai)

## Current stage

**Active delivery: Rust Core + DeepSeek Harness plugin UI only.** Flutter and other non-DSH UI
adapters are retired/out_of_scope; no platform parity or Flutter test pass is claimed. See
[SCOPE_DECISION.md](SCOPE_DECISION.md). The ledger retains 164 DSH rows with historical `verified`
labels and six Flutter rows now marked `out_of_scope`; these are recorded evidence, not a claim that
the current regression is complete. Earlier stage checkpoints below are historical observations.

## Historical implementation checkpoints (not current regression results)

### A0 — Host bridge
Locked Harness 0.2.1-rc.3; out-of-tree bundle; real native tool; React Sidebar; BFF; restart evidence.
A0 H22 incompatible-artifact and hot unload remain open.

### A1 — Rust kernel/runtime
Typed commands/queries, deterministic planner, revisions, receipts, approvals, SQLite WAL, CLI,
restart persistence. 13 tests pass including negative arms.

### A2 — Harness workbench
Real independent Core, Host BFF, native tools, React workbench, manual and agent proposal paths,
approval receipts, narrow layout, lifecycle. A2 state/dialog matrix remains partial.

### A3 — Model/MCP/privacy/durability
Core ModelGateway (local Ollama/cloud routes, no silent fallback), durable runs with cancel/recovery,
real local goal decomposition, official MCP SDK client, revocation, Windows notification delivery,
privacy minimal context. Cloud success requires a real credential.
2026-11-07 Harness model catalog: the Models page reads the Host's DeepSeek Harness model routes via
read-only `GET /zodo-bridge/v1/harness/models` (same UI-session auth as the other bridge routes) and
`填入 Core 模型` fills the Core model-connection draft (local when the route host is 127.0.0.1,
cloud otherwise). Only provider/model names, endpoints and credential *environment names* cross the
bridge; no secret is copied, Harness configuration is never mutated, and saving remains an explicit
Core action. Evidence: `tests/browser/zodo-harness-models.mjs`, screenshot
`.runtime/zodo-harness-models.png`.

### A4 — P1 partial
- Habits: create/update, bounded weekly target, per-occurrence logs, dedupe, UI list/log; review
  facts count habit completions.
- Tags: CRUD, task tag filtering; saved views: filter builder, CRUD, apply.
- Milestones: add/update/remove and UI.
- Data: JSON export with no credentials, SQLite backup with SHA-256, offline restore with integrity
  and workspace compatibility preflight, pre-restore snapshot, auth/token reset. Browser and CLI
  evidence.
- Automation: manual and schedule triggers, task.create action, 60s minimum interval, 5s scheduler tick,
  budget/auto-disable; other triggers/conditions and full loop matrix remain partial.
- Weekly stats in Review; reminders delivered in A3.
- Batch preview/apply: `task.batch_update` and multi-select UI with confirmation; tested at r36.

### A5 — retired / out_of_scope (2026-11-07)
- The independent Flutter Windows/Web/mobile/desktop UI is no longer an active deliverable.
- FLT-01..FLT-06 are `out_of_scope`, not successful test or platform acceptance results.
- Historical A5 evidence/artifacts remain for traceability; do not use them as current launch steps.
- Rust FFI/backend capabilities and DSH browser draft behavior remain separately recorded.

## Previously recorded gaps / items to revalidate

- A0 hot unload and H22 incompatible-artifact refusal are verified; hot reload without Host restart is not a Host capability.
- A3 cloud success, event cursor replay/filter, cancel/commit race, frozen eval.
- A4 import/CSV, incremental calendar, remaining
  screen/dialog states and adversarial matrices.
- Retired non-DSH UI platform parity is not an open delivery requirement.
- A6: offline sync export/import and conflict rejection exist; encryption/revocation/automatic transport and A7 final regression remain.

## Historical service observations (2026-11-07; not current endpoints)

These observations preserve the DSH migration/bridge investigation. Ports, revisions and model
availability below are not a current service inventory and must be re-observed before use.
Do not launch an archived profile or use a frozen data directory for tests.
- Desktop GUI host (:55583, Electron, profile desktop): its @zodo/harness-plugin owns the
  canonical Core data dir %LOCALAPPDATA%\zodo. 2026-11-07 the a2 workspace (rev 172: 103 tasks,
  3 goals, 3 habits) was migrated there after a Host restart without ZODO_DATA_DIR silently
  spawned an empty Core; the GUI now shows Core connected at r172 with real data. The pre-migration
  empty dir is archived at .runtime/zodo-local-empty-backup-20261107.
- 2026-11-07 desktop-face fix: the Electron window (dsh-app://app) forwards bridge traffic through
  its shell, which strips request Origin/Cookie and response Set-Cookie. @zodo/harness-plugin now
  mints the UI session without Origin (non-browser transport) and returns session_token in the
  body for such callers; client.js replays it via X-Zodo-UI-Session. Verified on an isolated host
  (desktop-face simulation 200/200, foreign Origin still 403, bogus token 401).
- .runtime/zodo-a2 is a frozen pre-migration archive; launching run-a2-host.cmd again would
  fork history from rev 172 - prefer %LOCALAPPDATA%\zodo for live use.
- Isolated host 127.0.0.1:3181 (run-a2-host.cmd) is not currently running.
- Local Ollama 127.0.0.1:11434 is currently down; MCP SDK test deps under tests/browser.

## Build checkpoint

The active release contains Rust Core and the DSH plugin only. Old release hashes and Flutter
packages are historical artifacts, not current delivery or regression acceptance.

## Next action

Complete non-DSH UI retirement first, then continue isolated Rust Core / DSH bridge / DSH UI
regression. Use new disposable fixtures, not the canonical user workspace or frozen archives.
Report exact commands, passed/failed/skipped counts and remaining limits; do not promote historical
ledger labels to new test results.
