# Evidence index

Every entry records the exact command, exit code, environment, source revision and artifact; raw
outputs are kept under `artifacts/evidence/` and are not edited. Secrets, private text and
authorization headers are never copied here.

## Baseline

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Result |
|---|---|---|---|---|---|
| E-0001 | 2026-10-24T23:04:00+08:00 | Verify supplied v1.3 bundle | `python design/v1.3/tools/verify_bundle.py` | 0 | 53 files verified; hashes/links/JSON passed |
| E-0002 | 2026-10-24T23:04:00+08:00 | Preserve original archive | `Get-FileHash Zodo-...-v1.3.zip -Algorithm SHA256` | 0 | `45E45CF0F21141572A987912E6FF5B3C1B3D3A616BE56A9ABC77A918D36454A5` |
| E-0003 | 2026-10-24T23:04:00+08:00 | Copy original archive | `design/v1.3-ORIGINAL.zip` | 0 | same SHA-256 as E-0002 |
| E-0004 | 2026-10-24T23:09:00+08:00 | Lock Harness source | `git ls-remote --tags https://github.com/deepseek-ai/deepseek-harness.git` | 0 | tag `dsh-v0.2.1-rc.3` -> `1f81c047fc48fa190a683e469daa699857e9afe8` |
| E-0005 | 2026-10-24T23:09:00+08:00 | Resolve installed archive | `Get-FileHash ...\resources\app.asar` | 0 | see `HARNESS_APP_ASAR_SHA256` below |
| E-0006 | 2026-10-24T23:12:00+08:00 | Inspect host package version | `Get-Item ...\DeepSeek Harness.exe`; `resources/runtime/runtime.json` | 0 | desktop `0.2.1-rc.3`, Node `24.21.0` |
| E-0007 | 2026-10-24T23:20:00+08:00 | Run host CLI through Electron Node | `ELECTRON_RUN_AS_NODE=1 "DeepSeek Harness.exe" "...app.asar\dsh\...\bin.js" --help` | 0 | profile/plugin CLI available |

`HARNESS_APP_ASAR_SHA256`: `04B32E3A872826BA15CFB2081C6A929A8D4ECAA65E09A2543E5A6B30E2EFF1F0`
(app.asar, 182,434,783 bytes).

## A0

Environment: isolated `DSH_HOME=D:\projects\zodo_dsh\.runtime\dsh-home-a0`, profile `zodo-a0`
created from the shipped `web` template, host on `127.0.0.1:3181`, Node via Electron
`v24.21.0`, local Ollama route `aravhawk/qwen3.5-opus-4.6:latest`. The real host and its original
desktop profile at port 61447 were not modified.

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| A0-0001 | 2026-10-24T23:17:00+08:00 | Build A0 probe and unit tests | `cargo test -p zodo-a0-probe`; `cargo build --release -p zodo-a0-probe` | 0 | `artifacts/evidence/a0/probe-cargo-test.txt`; 5 tests passed |
| A0-0002 | 2026-10-24T23:38:00+08:00 | Hash/pack the bundle | `Get-FileHash` on probe/plugin files | 0 | `artifacts/evidence/a0/plugin.sha256`; probe `38CC8BB9…` |
| A0-0003 | 2026-10-24T23:38:38+08:00 | Install tree-out bundle in clean profile | `dsh plugin --profile zodo-a0 add <plugin dir>` | 0 | `artifacts/evidence/a0/plugin-install.txt`; profile dependencies + bundle list updated |
| A0-0004 | 2026-10-24T23:34:49+08:00 | Real host model -> native tool -> Rust -> result | Browser prompt: “请调用 zodo_probe 工具，method 参数为 ping。调用完成后只回复 done。” | 0 | `artifacts/evidence/a0/native-session-events.json`, `session-dbc2253a.jsonl`, raw SHA-256 `4fa9bcbf…`; tool call `call_a1nzeuk2`; result `ok:true`, `zodo-a0-bridge/1`, `pong` |
| A0-0005 | 2026-10-24T23:39:56+08:00 | React Sidebar tab -> Host BFF -> Rust -> React, async cancel | `node tests/browser/a0-ui-flow.mjs` | 0 | `zodo-ui-flow.json`, `zodo-tab-ping.png`, `zodo-tab-cancel.png`; ping/echo HTTP 200, sleep -> cancel -> 409 `CANCELLED` |
| A0-0006 | 2026-10-24T23:39:00+08:00 | Host exit/restart and probe ownership | Stop port owner, restart host, rerun UI flow | 0 | `probe-instance-before-restart.txt` vs `probe-instance-after-restart.txt`; host-owned child did not survive; new bridge instance on restart; routes unavailable while host down |
| A0-0007 | 2026-10-24T23:36:00+08:00 | Second process with same state dir | `zodo-a0-probe.exe --state-dir <A0 state>` | 0 | `second-instance.txt`: existing healthy instance detected; second process exited without binding a second server |
| A0-0008 | 2026-10-24T23:39:20+08:00 | Cookie/Origin boundary | Same-origin browser session; route rejects missing session/Origin by implementation; UI flow shows mint then authorized RPC | 0 | `zodo-ui-flow.json`; `/session` 200, then `/rpc` 200/409 |
| A0-0009 | pending | Hot unload without host stop | — | — | not yet verified |
| A0-0010 | 2026-10-25T01:35:00+08:00 | Incompatible Core artifact refusal (H22) | Copy A0 probe over packaged Core binary; request `/zodo-bridge/v1/health` | 503 | `artifacts/evidence/a0/h22-incompatible.json`, `h22-ui.txt`, `h22-after-restore.txt`; error `CORE_INCOMPATIBLE`; host did not bind a second Core; good binary restored and reconnect verified |

Screenshots are real Chrome captures of the running host, not mock frames:
`zodo-tab-ping.png`, `zodo-tab-cancel.png`, `native-after.png`, `sidebar-menu.png`.

Secret handling: probe session tokens are ephemeral and were redacted from
`probe-lock.redacted.json`; the launch URL token was redacted from
`host-startup.redacted.log`; no authorization header, model credential, or private body is present
in the evidence set.

## A1 — Rust kernel/runtime

Environment: Rust 1.98.1, Windows x64, temporary SQLite data directory, no Harness. The test binary
calls the kernel through the same `Application` used by the daemon.

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Result / raw evidence |
|---|---|---|---|---|---|
| A1-0001 | 2026-10-25T00:25:00+08:00 | Format + full workspace test | `cargo fmt --all`; `cargo test --workspace` | 0 | 13 tests pass (2 store unit + 11 vertical); `artifacts/evidence/a1/cargo-test.txt` |
| A1-0002 | 2026-10-25T00:25:00+08:00 | Release build | `cargo build --release -p zodo-runtime` | 0 | `target/release/zodo.exe`; copied to plugin `bin/win32-x64/zodo-core.exe` |
| A1-0003 | 2026-10-24T23:50:00+08:00 | Manual host/CLI vertical slice | CLI `capture`, `plan`, `approve`, `commit`, `progress`, `review` against isolated daemon | 0 | `artifacts/evidence/a1/cli-vertical.txt`; receipts/revisions persisted |
| A1-0004 | 2026-10-25T00:26:00+08:00 | Tests include negative arms | `cargo test -p zodo-runtime --test a1_vertical` | 0 | idempotency conflict, stale revision, DAG cycle, agent approval denial, infeasible plan, locked allocation, expired approval, reminder outbox, recurrence/month-end |

A1 open: durable AI Runs are not implemented yet; notification delivery is not implemented yet;
formal JSON contract fixtures and full DST matrix remain.

## A2 — Real Harness Host + React workbench

Environment: isolated `DSH_HOME=D:\projects\zodo_dsh\.runtime\dsh-home-a0`, profile `zodo-a0`,
`ZODO_DATA_DIR=D:\projects\zodo_dsh\.runtime\zodo-a2`, host `127.0.0.1:3181`, local Ollama. The
desktop Harness at 61447 was not touched.

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| A2-0001 | 2026-10-25T00:10:00+08:00 | Host starts real Core and loads workbench | `node tests/browser/a2-smoke.mjs` | 0 | `artifacts/evidence/a2/a2-initial.png`, `a2-initial.json`; bridge session/health/rpc 200; no page/console errors |
| A2-0002 | 2026-10-25T00:10:06+08:00 | Manual path: capture -> task -> capacity -> plan proposal -> approve -> commit | `node tests/browser/a2-flow.mjs` | 0 | `flow-a2.json`, `flow-00..05*.png`; receipt `cs_1477ab490c734cedbc87`, allocation persisted at r4 |
| A2-0003 | 2026-10-25T00:15:00+08:00 | Real local model -> native tools -> Core proposal | `node tests/browser/a2-native-tools.mjs` | 0 | `native-agent-session.zstd` SHA-256 `207574da76fcdf35a1224954d510f03b8781bee65022ea3eccf5d495661fd1d2`; `native-tool-events.json`; tool calls and results |
| A2-0004 | 2026-10-25T00:17:00+08:00 | Agent proposal reviewed and approved in React | `node tests/browser/a2-agent-approval.mjs` | 0 | `agent-approval.json`, `agent-proposal-list/review/applied.png`; commit receipt `cs_38bfd4a261734f24a083`, revision r5 |
| A2-0005 | 2026-10-25T00:19:00+08:00 | Manual feature surface | `node tests/browser/a2-manual-flow.mjs` | 0 | 13 steps: goal/project, dependency, progress, focus, reminder, recurrence, busy import, search, review, audit, privacy; no console errors |
| A2-0006 | 2026-10-25T00:21:00+08:00 | Inbox conversion + goal evidence | `node tests/browser/a2-inbox-flow.mjs` | 0 | 5 steps pass; `inbox-*.png`, `inbox-flow.json`; no console errors |
| A2-0007 | 2026-10-25T00:24:00+08:00 | Core survives Host stop and reconnects | Stop port 3181 owner; CLI `workspace.get`; restart wrapper | 0 | Core PID alive during host downtime; CLI reads same revision; new host URL in `.runtime/a2-url.txt`; see `artifacts/evidence/a2/host-lifecycle.txt` |

A2 open: exhaustive state/dialog matrix, stale/infeasible UI branches, hot reload, actual uninstall,
IME composition stress, notification delivery, model failure/draft preservation, revocation and
the complete A2 flow fixture matrix.

Secret handling: A2 raw bridge/session evidence contains no Core token, cookie value, model
credential or launch token; runtime.json is under `.runtime` and is not committed.

## A3 — Core model, durable runs, MCP, privacy, revocation, notification

Environment: isolated test data directories under `.runtime`; local Ollama
`aravhawk/qwen3.5-opus-4.6:latest`; Rust 1.98.1; official MCP SDK 1.20.2. The desktop Harness at
61447 was not touched.

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| A3-0001 | 2026-10-25T00:40:00+08:00 | Core model gateway local goal decomposition | `python tools/test_a3_model_run.py` | 0 | `artifacts/evidence/a3/a3-model-run.json`, `a3-model-server.log`; 5–7 tasks, `provider=local-ollama`, `route=device`, actor `delegated_agent`, proposal validated with no diagnostics |
| A3-0002 | 2026-10-25T00:43:00+08:00 | Goal card -> UI run -> local model -> React diff -> approval -> commit | `node tests/browser/a3-ai-ui.mjs` | 0 | `ai-ui-flow.json`, `ai-00..04*.png`; run succeeded; 6 task additions shown; toast/receipt `cs_e49c8fbe691c49339d6b`, r26 |
| A3-0003 | 2026-10-25T00:44:00+08:00 | Official MCP SDK initialize/list/call | `cd tests/browser; node a3-mcp-sdk.mjs` | 0 | `a3-mcp-sdk.json`; server `zodo` 0.1.0; 4 tools; plan validated; proposal validated; get validated; no approve/commit tool |
| A3-0004 | 2026-10-25T00:45:00+08:00 | MCP authorization boundary | `python tools/test_a3_mcp_authz.py` | 0 | `a3-mcp-authz.json`; snapshot/approve/commit/cross-workspace all 403; proposal.create 200 |
| A3-0005 | 2026-10-25T00:46:00+08:00 | Revocation persisted/rejected/restored | `python tools/test_a3_revocation.py`; UI surfaces | 0 | `a3-revocation.json`, `a3-surfaces.json`, `a3-agents-*.png`; agent 200 -> 403 -> 200; auth.json revoked/restored |
| A3-0006 | 2026-10-25T00:45:00+08:00 | Reminder effect delivery | `python tools/test_a3_notification.py` | 0 | `a3-notification.json`; Windows toast backend delivered, reminder `delivered`, notifications.jsonl row |
| A3-0007 | 2026-10-25T00:45:00+08:00 | Run cancellation | `python tools/test_a3_cancel.py` | 0 | `a3-run-cancel.json`; queued run cancelled, final state `cancelled` |
| A3-0008 | 2026-10-25T00:49:00+08:00 | Core crash/restart recovery of running AI run | `python tools/test_a3_recovery.py` | 0 | `a3-run-recovery.json`, server logs; instance changed, run requeued and succeeded, exactly one AI proposal, steps queued/context/model/proposal |
| A3-0009 | 2026-10-25T00:52:00+08:00 | Privacy/no silent cloud fallback | `python tools/test_a3_privacy.py` | 0 | `a3-privacy.json`; broken local fails local-only; cloud denied for device-only; cloud missing key 401; no local fallback |
| A3-0010 | 2026-10-25T00:54:00+08:00 | Models/Agents/Runs surfaces | `node tests/browser/a3-ui-surfaces.mjs` | 0 | `a3-models.png`, `a3-runs.png`, `a3-agents-*.png`; no console/page errors |
| A3-0011 | 2026-10-25T00:55:00+08:00 | CLI run/model queries | `zodo query runs.list`; `zodo model-info` | 0 | runtime returns persisted run list and sanitized/editable model config |

A3 open: successful cloud call (no credential supplied), event cursor replay/filter, cancel-vs-commit
race, frozen eval set; cloud credential UI is environment-based. A3 evidence contains no API key or
channel token.

## A4 — P1 partial: habits, tags/views, milestones, data/backup/restore, automation, stats

Environment: isolated data directories under `.runtime`; Flutter not involved. Tests: Rust `a4_p1`.

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| A4-0001 | 2026-10-25T01:18:00+08:00 | Habit/tag/saved-view/milestone/automation tests | `cargo test -p zodo-runtime --test a4_p1` | 0 | 3 tests pass; `target` test log |
| A4-0002 | 2026-10-25T01:17:00+08:00 | Live UI P1 flow | `node tests/browser/a4-flow.mjs` | 0 | `a4-flow.json`, `a4-01..07*.png`; habit create/log, saved view/apply, milestone create/complete, backup, export download, model save; no console errors |
| A4-0003 | 2026-10-25T01:18:00+08:00 | Backup/export boundary | `GET /v1/data/export`, `POST /v1/data/backup` through Host | 0 | `a4/a4-flow.json`, `zodo-export.json`; schema `zodo.export/1`, counts, `contains_credentials:false`; backup file/hash |
| A4-0004 | 2026-10-25T01:20:00+08:00 | Offline preflight restore | `python tools/test_a4_restore.py` | 0 | `a4-restore.json`; before 2 tasks -> after 1, restore exit 0, pre-restore snapshot, auth.json absent |
| A4-0005 | 2026-10-25T01:18:00+08:00 | Automation budget test | `cargo test -p zodo-runtime --test a4_p1` | 0 | 2 manual runs create 2 tasks, rule auto-disables at budget, third run rejected |
| A4-0006 | 2026-10-25T01:19:00+08:00 | Flutter-visible workspace name repair | `zodo command workspace.rename` | 0 | current Core header now shows `我的事务`; command output verified |
| A4-0007 | 2026-10-25T01:32:00+08:00 | Batch preview + atomic apply UI | `node tests/browser/a4-batch.mjs` | 0 | `a4/a4-batch.json`, `batch-selected.png`, `batch-applied.png`; confirm dialog, r36 receipt, two tasks processed |
| A4-0008 | 2026-10-25T01:41:00+08:00 | Scheduled automation trigger | `python tools/test_a4_scheduler.py` | 0 | `a4/a4-automation-schedule.json`; Schedule rule ran once after 5s scheduler tick, created task, runs=1, last_run_at set; 60s interval + budget enforce loop prevention |
| A4-0009 | 2026-10-25T01:46:00+08:00 | JSON export/import into empty Core | `python tools/test_a4_import.py` | 0 | `a4/a4-import.json`; 22 tasks/1 goal/1 project/4 inbox/1 habit imported, schema/hash validated, credentials not imported, auth file absent |
| A4-0010 | 2026-10-25T01:49:00+08:00 | Actual-vs-estimate adjustment proposal | `node tests/browser/a4-estimate-adjust.mjs` | 0 | `a4/a4-estimate-adjust.json`, `estimate-proposal.png`; proposal bound to r40, user approval commit at r41; no permanent preference rule |
| A4-0012 | 2026-10-25T02:02:00+08:00 | Command palette keyboard path | Ctrl+K → search 提案 → Enter | 0 | `a4/command-palette.png`; modal keyboard opens, filters and navigates to Proposals; no automatic commit |
| A4-0013 | 2026-10-25T02:05:00+08:00 | Time-object detail and delete/lock actions | Schedule slot click | 0 | `a4/time-detail.png`; capacity/commitment/allocation detail, locked state and delete/close actions; no past allocation auto-move |
| A4-0014 | 2026-10-25T02:09:00+08:00 | Task CSV export | `node tests/browser/_csv_ui.mjs` | 0 | `a4/zodo-tasks.csv`, `data-csv.png`; 23 rows including 22 tasks, header and quoted Chinese titles, no credentials |

A4 open: automatic trigger scheduler/loop matrix, batch operations, JSON/CSV import, attachments,
adjustment/actual-vs-estimate proposals, incremental calendar sync, full screen/dialog state matrix.

## A5 — Flutter Windows partial

Environment: Flutter `3.47.8` stable / Dart `3.13.7`, Windows x64, Visual Studio 2022 Build Tools.
No Android SDK or Xcode. Flutter SDK is under `.runtime` and is not committed.

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| A5-0001 | 2026-10-25T01:22:00+08:00 | Flutter version/doctor | `flutter --version`; `flutter doctor -v` | 0 | `a5/flutter-version.txt`, `a5/flutter-doctor.txt`; Windows/Chrome OK; Android SDK missing |
| A5-0002 | 2026-10-25T01:22:00+08:00 | Widget test | `flutter test` | 0 | `a5/flutter-test.txt`; 1 test passed |
| A5-0003 | 2026-10-25T01:22:00+08:00 | Windows release build | `flutter build windows --release` | 0 | `a5/flutter-build-windows.txt`; `zodo_flutter.exe` built |
| A5-0004 | 2026-10-25T01:23:00+08:00 | Live client launch against Core | release exe + `ZODO_DATA_DIR=.runtime/zodo-a2` + `--enable-software-rendering` | 0 | `a5/flutter-windows-final.png`, `flutter-windows-habits.png`; real Today/Habits facts, workspace name; `r35` |
| A5-0005 | 2026-10-25T01:23:00+08:00 | Headless capture rendering limitation | same exe without software rendering | 0 | blank GPU surface captured; process ran; documented in `clients/flutter/README.md` |
| A5-0006 | 2026-10-25T01:40:00+08:00 | Flutter web build and live Core render | `flutter build web --release`; `python tools/flutter_web_proxy.py`; `node tests/browser/a5-flutter-web.mjs` | 0 | `a5/flutter-web.png`, `flutter-build-web.txt`; real Today facts, r36, workspace `我的事务`; URL-token proxy is test-only |
| A5-0006 | 2026-10-25T01:40:00+08:00 | Flutter web build and live Core render | `flutter build web --release`; `python tools/flutter_web_proxy.py`; `node tests/browser/a5-flutter-web.mjs` | 0 | `a5/flutter-web.png`; real Today facts, r36, workspace `我的事务`; URL-token proxy is test-only |

A5 open: production web authenticated transport (the validation proxy uses a URL token), Android/iOS, macOS/Linux, 30-screen/16-dialog parity,
accessibility, secure storage, deep links, platform notification permission and lifecycle. A5 is
partial and is not claimed complete.





## A6 — offline two-device sync foundation (partial)

Environment: two fresh independent Core data directories, same binary, local loopback.

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| A6-0001 | 2026-10-25T01:55:00+08:00 | Two-store export/import/conflict | `python tools/test_a6_sync.py` | 0 | `a6/a6-swap.json`, device logs; both devices reached 2 tasks; each import added 1; divergent shared task import returned 409 with one conflict; local title unchanged; device IDs differ |

A6 open: deletion/tombstones, automatic discovery, end-to-end encryption, revoked-device recovery,
operation-envelope transport, same-entity merge policy beyond reject-and-review, and UI sync error
matrix.


## A7 — build/release checkpoint (partial)

Environment: Windows x64, commit `ca1389b`, Rust 1.98.1, Flutter 3.47.8.

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| A7-0001 | 2026-10-25T01:58:00+08:00 | Release artifact assembly | `scripts/build-release.ps1` | 0 | `a7/release-sha256.txt`, `a7/release-build-info.json`; Core/Harness bundle/Flutter Windows/Web zips + SHA-256; Flutter test passed during build |

A7 open: complete final regression matrix, browser/MCP replay on the release bundle, installers/signing,
platform matrix, and closing all partial ledger rows.


## FND-04 run-state hardening

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FND04-0001 | 2026-10-25T02:07:00+08:00 | Durable run state machine and recovery | `cargo test -p zodo-runtime --test fnd04_runs` | 0 | queued→running claim, failed with error/ended_at/lease release, queued cancel, running recovery to queued, cancel-after-recovery to cancelled, invalid state rejected, list_runs |


## FND-11 event cursor hardening

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FND11-0001 | 2026-10-25T02:12:00+08:00 | Cursor replay, duplicate handling, gap resync | `python tools/test_fnd11_events.py` | 0 | `a3/fnd11-events.json`; first page 2 events/ev_2, incremental page 1 event, replay after advanced cursor 0, future cursor resync_required=true, limit=1 has_more=true, next_cursor=ev_1 |


## HARN-H-16 UI admin-identity forgery

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| H16-0001 | 2026-10-25T02:14:00+08:00 | Forged UI cookie, evil origin, agent admin attempt | `python tools/test_h16_admin_forgery.py` | 0 | `a3/h16-admin-forgery.json`; forged cookie 401 UI_SESSION_REQUIRED, evil origin 403 ORIGIN_DENIED, agent token data.backup/workspace.update 403 PERMISSION_DENIED, session body contains no token/secret/api_key |


## AI-13 frozen evaluation set

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| AIEVAL-0001 | 2026-10-25T02:22:00+08:00 | Frozen model-output set and offline scorer | `python tools/run_ai_eval.py` | 0 | `a3/ai-eval.json`; 1 recorded valid decomposition + 2 adversarial invalid cases; 3/3 decisions correct; live-model scoring/broader cases still open |


## HARN-H-21 cancel-vs-commit race

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| H21-0001 | 2026-10-25T02:26:00+08:00 | Cancel queued run; cancel after proposal; approve/commit | `python tools/test_h21_cancel_race.py` | 0 | `a3/h21-cancel-race.json`; queued cancel→cancelled with 0 proposals, no writes; second run reached proposal, cancel after success returned 200 and run stayed succeeded; approval+commit returned real revision 2 |


## AI-01 clarification run

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| AI01-0001 | 2026-10-25T02:28:00+08:00 | One-turn model clarification without facts | `python tools/test_ai01_clarify.py` | 0 | `a3/ai01-clarify.json`; run succeeded, 4 questions + suggested_kind task, 0 tasks written, checkpoint/output persisted; multi-turn conversation/screen still open |


## HARN-H-13 receipt-loss replay (H09)

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| H13-0001 | 2026-10-25T02:30:00+08:00 | Lost-response replay and key reuse with different content | `python tools/test_h09_receipt_replay.py` | 0 | `a3/h09-receipt-replay.json`; first commit r1/1 task, same key+content replayed true r1/1 task, same key+different content 409 IDEMPOTENCY_MISMATCH/1 task |


## Harness P0 dialogs (UI-L04/L11/L13) and AI clarify UI

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UI-L04-0001 | 2026-10-25T02:17:00+08:00 | Time/timezone editor | Playwright E2E on isolated Host | 0 | `a4/ui-l04-l11-l13-e2e.json` 15/15; `ui-l04-dialog.png`, `ui-l04-saved-detail.png`, `ui-l04-preview-hard.png`, `ui-l04-soft-utc-caveat.png`, `ui-l04-cleared.png`, `ui-l04-schedule-entry.png`, `ui-l04-time-detail-entry.png`; hard due+not_before saved/cleared/prefilled, UTC preview, focus return |
| UI-L11-0001 | 2026-10-25T02:17:00+08:00 | Data-reference/outbound preview | same E2E | 0 | `ui-l11-proposal-preview.png`, `ui-l11-run-preview*.png`; reads/data classes/truncated values/destination, no token rendered; summary-not-redaction warning |
| UI-L13-0001 | 2026-10-25T02:17:00+08:00 | Revoke/pause rules modal | same E2E + `auth.json` check | 0 | `ui-l13-rules-*.png`, `ui-l13-revoked.png`, `ui-l13-restored.png`; MCP revoke→restore verified, future-effect semantics stated; bridge has no auth-state read |
| AI01-UI-0001 | 2026-10-25T02:17:00+08:00 | Inbox AI clarification UI | Playwright on restarted Host | 0 | `ui-ai01-inbox-detail.png`, `ui-ai01-clarify.png`, `ui-ai01-clarify-e2e.json`; run succeeded, 4 questions, suggested_kind 任务, facts_written false; no proposal/approve |

Gaps: DST nonexistent/ambiguous branches implemented but only unit-reasoned in Asia/Shanghai; L11 shows only bridge-exposed facts (not full prompt); L13 cannot read current auth state before first revoke and revoke does not cancel queued outbox effects.


## A5 Flutter screen expansion

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| A5-0007 | 2026-10-25T02:35:00+08:00 | Flutter saved views/automation/sync/capture/time screens | `flutter analyze`; `flutter test`; `flutter build windows --release`; `flutter build web --release` | 0 | `a5/flutter-analyze.txt`, `flutter-test.txt`, `flutter-build-windows.txt`, `flutter-build-web.txt`; analyze clean, 1 widget test, Windows/Web builds; read-only live snapshot exposes saved_queries/automation_rules/inbox; no UI token in sync export |


| A7-0002 | 2026-10-25T02:27:00+08:00 | Clean-commit release rebuild after A2/A3/A5 changes | `scripts/build-release.ps1` from clean commit `9c1d6fa` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`; Core 70061b96…, plugin 51faa567…, Flutter Windows bd94bfe8…, Web 963e06db…; 5 Flutter tests passed during build |


## AI-12 host model selection

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| AI12-0001 | 2026-10-25T02:31:00+08:00 | Document host selector as unsupported override | Code/doc inspection | 0 | `adapters/deepseek-harness/A0_LOCK.md` section "Host model selection"; host selector is independent, Core `model.json`/policy remains authority; no silent injection |


## HARN-H-10 Core restart and cursor recovery

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| H10-0001 | 2026-10-25T02:33:00+08:00 | Restart Core and resume event cursor | `python tools/test_h10_restart_cursor.py` | 0 | `a3/h10-restart-cursor.json`; cursor ev_1 before restart, new instance after restart, one new event at ev_2, resync_required=false, same workspace |


## PLN-11 incremental calendar import

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN11-0001 | 2026-10-25T02:35:00+08:00 | Upsert by source/external id, replace, invalid rollback | `python tools/test_pln11_calendar.py` | 0 | `a4/a4-pln11-calendar.json`; 2→3 events on incremental import (same external id updated), replace_source →1, invalid event 422 and still 1 event; source last_sync updated; explicit cursor/provider mapping still pending |


## FND-16 C ABI foundation

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FND16-0001 | 2026-10-25T02:38:00+08:00 | FFI open/command/query/free with panic isolation | `cargo test -p zodo-ffi` | 0 | 2 tests: open existing ws_local store, create/list task via C ABI, free; null handle and malformed JSON return FFI_ERROR without panic. Poll/cancel run ABI and Flutter embedding still pending |


## DAT-05 browser offline draft

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| DAT05-0001 | 2026-10-25T02:40:00+08:00 | Offline capture draft, reload restore, reconnect commit | `node tests/browser/a4-offline-draft.mjs` | 0 | `a4/dat05-offline-draft.png`, `dat05-reconnected.png`, `ui-dat05-offline-draft.json`; aborted RPC leaves draft in localStorage and message "未提交到 Core"; reload restores exact text; reconnect commits r58 and clears draft |


## A7 regression checkpoint 2

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| A7-0003 | 2026-10-25T02:43:00+08:00 | Browser A2/A4 regression after UI expansion | `node tests/browser/a2-smoke.mjs`; `node tests/browser/a4-flow.mjs` | 0 | `a7/regression-smoke.txt`, `a7/regression-a4.txt`, `a4/a4-flow.json`; all A4 steps incl. backup/export/models save passed, 0 page/console errors |


| A7-0004 | 2026-10-25T02:46:00+08:00 | Final release with FFI, fixed export action | `scripts/build-release.ps1` | 0 | `a7/release-sha256.txt`, `a7/release-build-info.json`; artifacts include zodo.exe, zodo_ffi.dll, Harness plugin zip, Flutter Windows/Web zips; 5 Flutter tests passed |


| A7-0005 | 2026-10-25T02:48:00+08:00 | Final release daemon smoke | `node tests/browser/a2-smoke.mjs` against restarted Host/core | 0 | `a7/release-smoke.txt`; workbench loaded with final release binary; 0 page errors |


## A0 hot unload without host stop

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| A0-0011 | 2026-10-25T02:50:00+08:00 | Uninstall Zodo plugin while Host stays alive | Host plugin manager UI; then restore profile + restart | 0 | `a0/uninstall-confirm.png`, `a0/hot-uninstall.png`; plugin card/tab disappeared, Host still listened on 3181, `/zodo-bridge/v1/health`=404, Core PID 42664 alive at r68; profile restored with pnpm, Host restarted, Zodo workbench smoke passed |


## UI-L06 recurrence scope

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UI-L06-0001 | 2026-10-25T02:53:00+08:00 | Open/validation/focus-return/live submit dialog | `node artifacts/evidence/a4/ui-l06-live-check.cjs`; `cargo test -p zodo-runtime --test recurrence_scope` | 0 | 3/3 recurrence tests; `a4/ui-l06-live.json`, `ui-l06-dialog-single.png`, `ui-l06-dialog-future.png`, `ui-l06-validation.png`, `ui-l06-closed-focus-return.png`; single/未来系列 scopes, range line, Escape focus return |
| UI-L06-0002 | 2026-10-25T02:58:00+08:00 | Live future-series rewrite | UI submit on `rec_9fe393843e634514b73e` | 0 | `a4/ui-l06-series-updated.png`, `ui-l06-series-state.json`; rule v3 title/estimate updated, 5 open future tasks changed to 45m, 1 done past task stayed 30m, revision 70 |


## DAT-04 import preflight and idempotent retry

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| DAT04-0001 | 2026-10-25T02:57:00+08:00 | Fingerprint preflight, idempotent retry, changed-content mismatch | `python tools/test_dat04_import.py` | 0 | `a4/a4-dat04-import.json`; preflight can_import=true + SHA-256 fingerprint, first import r1/1 task, replay replayed=true with same revision/1 task, same key+changed facts 409 IDEMPOTENCY_MISMATCH and still 1 task |


| A7-0006 | 2026-10-25T02:59:00+08:00 | Release rebuild with recurrence/import ABI | `scripts/build-release.ps1` | 0 | `a7/release-sha256.txt`, `a7/release-build-info.json`; commit 38f83c5, Core 14765568 B, FFI dll, Harness/Flutter zips; 5 Flutter tests passed |


## PLN-15 flexible capacity candidates

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN15-0001 | 2026-10-25T03:05:00+08:00 | Candidate generation and explicit creation | `cargo test -p zodo-runtime --test a4_p1 capacity_candidates`; `node tests/browser/a4-capacity-suggest.mjs` | 0 | `a4/pln15-candidates.png`, `pln15-created.png`; 8 bounded candidates excluded fixed commitment, user clicked one → capacity created r71, list reduced to 7; no auto-write before click |


## AI-14 finite model catalog

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| AI14-0001 | 2026-10-25T03:08:00+08:00 | Provider/version/action catalog | `GET /v1/model/catalog` | 0 | `a3/ai14-catalog.json`; schema zodo.model-catalog/1, local+cloud providers with credential presence, chat/goal.decompose/capture.clarify actions with task/question/time budgets, authorization and failure lists |


## ORG-15 duplicate detection and merge

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| ORG15-0001 | 2026-10-25T03:12:00+08:00 | Detect and merge duplicates preserving links | `cargo test -p zodo-runtime --test a4_p1 duplicate_detection`; Harness merge | 0 | `a4/org15-duplicates.png`, `org15-merged.png`, `org15-merge-state.json`; normalized groups, dependency re-point test, duplicate archived with merged_into tag, survivor merged_from, r74 receipt; allocations refuse merge |


## PLN-16 meeting proposal (partial)

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN16-0001 | 2026-10-25T03:15:00+08:00 | Meeting proposal and approval | Harness Schedule → 提议会议 → approve/commit | 0 | `a4/pln16-meeting.png`, `pln16-meeting-state.json`; proposal created, user approved at r75, commitment has external_id; external calendar write and duplicate-external-id failure/idempotency still missing |


## HARN-H-19 session fork / disclosure

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| H19-0001 | 2026-10-25T03:18:00+08:00 | Fork real Host session; verify no Core replay | `node tests/browser/a3-h19-fork.mjs` | 0 | `a3/h19-fork.json`; fork at seq 1 succeeded with child session, Core revision/tasks/notes unchanged (r75, 28 tasks, 0 notes), no page errors; compression/restricted-class fail-closed matrix still pending |


## REC-07 mapped recurrence dependencies

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| REC07-0001 | 2026-10-25T03:22:00+08:00 | Chained occurrence dependencies and safe re-expansion | `cargo test -p zodo-runtime --test a4_p1 recurrence_chain` | 0 | 4 bounded occurrences → 3 dependency edges; same through re-expand returns 0 tasks and still 3 edges; Harness recurrence editor exposes chain checkbox; no duplicate/self edges |


| A7-0007 | 2026-10-25T03:17:00+08:00 | Round 3 release with recurrence/merge/capacity/catalog | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round3.txt`; commit e58bcc9, Core/FFI/Harness/Flutter zips, final Host smoke passed |


## REC-08 cross-period dependency planning

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| REC08-0001 | 2026-10-25T03:24:00+08:00 | Blocked cross-period predecessor and future-allocation diagnostic | `cargo test -p zodo-runtime --test a4_p1 cross_period` | 0 | Without predecessor allocation plan reports `dependency_blocked` + diagnostics; a future allocation beyond the horizon removes `DEPENDENCY_BLOCKED` while capacity shortfall remains truthfully reported; bounded horizon; not yet a full multi-period planner |


## DAT-08 encrypted sync bundles

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| DAT08-0001 | 2026-10-25T03:30:00+08:00 | AES-256-GCM/PBKDF2 encrypted sync envelope | `cargo test -p zodo-runtime --lib`; `python tools/test_dat08_encrypted_sync.py` | 0 | `a6/dat08-encrypted-sync.json`; schema zodo.sync.encrypted/1, plaintext task title absent from envelope, correct passphrase imports 1 task, wrong passphrase 403 PERMISSION_DENIED and 0 tasks; crypto unit round-trip passed |


## DAT-06 opt-in local content retrieval

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| DAT06-0001 | 2026-10-25T03:33:00+08:00 | Consent-gated local note retrieval | `cargo test -p zodo-runtime --test a4_p1 content_retrieval` | 0 | consent=false → PERMISSION_DENIED; consent=true returns 1 device_only note snippet with linked refs; scope local_workspace_only; personalisation not applied; empty query rejected; external retrieval/personalisation eval pending |


| A7-0008 | 2026-10-25T03:28:00+08:00 | Round 4 release and platform blocker check | `scripts/build-release.ps1`; `flutter devices` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round4.txt`; Windows/Chrome/Edge only, no adb/Xcode/macOS/Linux; see `docs/PLATFORM_BLOCKERS.md`, `a5/platform-checks.txt` |


## EXE-02 focus lifecycle

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| EXE02-0001 | 2026-10-25T03:38:00+08:00 | Single focus, pause/resume/finish, restart recovery | `cargo test -p zodo-runtime --test a4_p1 focus_lifecycle` | 0 | second focus rejected; 10m pause + 5m pause + 15m running finish; completed session with ≥2 segments; active focus persisted and finishable after restart with new Application instance |


## ORG-02/ORG-03/EXE-06 goal and project lifecycle

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| ORG02-0001 | 2026-10-25T03:40:00+08:00 | Goal evidence/reopen and project cancel isolation | `cargo test -p zodo-runtime --test a4_p1 goal_achieved` | 0 | achieved without evidence rejected; evidence achieves + achieved_at; update title; reopen keeps evidence and clears achieved_at; project update + cancel leaves child task open |


## ORG-05 parent/child hierarchy

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| ORG05-0001 | 2026-10-25T03:42:00+08:00 | Parent cycle refusal and rollup/leaf planning | `cargo test -p zodo-runtime --test a4_p1 parent_child` | 0 | child under parent; making parent child of child rejected; original graph preserved; parent not leaf/child leaf; planning rollup parent rejected rather than double-counted |


## PLN-01 distinct time strengths

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN01-0001 | 2026-10-25T03:45:00+08:00 | Soft/hard due, not_before, preferred window, ambiguity | `cargo test -p zodo-runtime --test a4_p1 due_strengths` | 0 | soft date → target_due only + not_before/preferred window preserved; hard instant → hard_deadline only; date+at+soft rejected as ambiguous |


## FND-02 versioned contract fixtures

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FND02-0001 | 2026-10-25T03:48:00+08:00 | Envelope fixtures parse/validate/round-trip | `cargo test -p zodo-kernel --test contracts` | 0 | 4 tests: command/query/error fixtures round-trip byte-equivalent JSON; command/query domain validation; bad schema and missing idempotency key rejected; fixtures under contracts/fixtures |


## PLN-05 allocation bounds

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN05-0001 | 2026-10-25T03:50:00+08:00 | Remaining/capacity bounds and locked behavior | `cargo test -p zodo-runtime --test a4_p1 allocations_respect`; A1 locked-allocation test | 0 | 40+30 over 60 remaining rejected; 40+20 totals 60 and capacity used 60; A1 test keeps locked allocation unmoved and capacity not double-charged |


## FND-07 grants/revocation/workspace isolation

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FND07-0001 | 2026-10-25T03:52:00+08:00 | Independent UI/Agent grants, revoke, cross-workspace denial | H16/A3 evidence | 0 | Agent token admin operations 403, forged UI cookie 401, evil Origin 403, revocation persists/rejects and MCP restore works, cross-workspace query 403; UI/Agent grants are distinct |


## HARN-H-20 replay does not redo facts

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| H20-0001 | 2026-10-25T03:54:00+08:00 | Fork/replay and receipt-loss replay | `node tests/browser/a3-h19-fork.mjs`; H13 receipt test | 0 | real Host fork at seq 1 created child with Core revision/tasks unchanged (no auto execute); same command key replay returns receipt and one fact (H13) |


| A7-0009 | 2026-10-25T03:58:00+08:00 | Final round5 clean release | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`; commit at HEAD, Core/FFI/Harness/Flutter artifacts; full Rust/Flutter tests passed before build |


## EXE-03 reported/derived facts and FND-06 outbox retry

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| EXE03-0001 | 2026-10-25T04:02:00+08:00 | Progress facts immutable/source; review derived | `cargo test -p zodo-runtime --test a4_p1 progress_facts` | 0 | 30+20 minutes → actual 50, remaining 70, InProgress; duplicate dedupe rejected without extra activity; activities carry user_ui source; stats/review derives actual_minutes=50 |
| FND06-0001 | 2026-10-25T04:02:00+08:00 | Outbox pending/backoff/terminal failure | `cargo test -p zodo-runtime --test a4_p1 outbox_retry` | 0 | reminder effect pending at fire time; attempts=4 with future availability invisible until due; attempts=5 failed removed from pending; durable store |


## AI-06 external structured proposal

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| AI06-0001 | 2026-10-25T04:04:00+08:00 | Official MCP client proposes typed candidate; no re-understanding | `tests/browser/a3-mcp-sdk.mjs`; `a3-mcp-authz.json` | 0 | official MCP SDK zodo_propose created a typed TaskCreate proposal with receipt; agent cannot approve/commit; revoked MCP channel denied 403; candidate accepted by Core validation |


## AI-03 typed plan candidate diagnostics

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| AI03-0001 | 2026-10-25T04:06:00+08:00 | Typed plan preview with hard/partial diagnostics | A1 `infeasible_plan_reports_gap...`; A2 plan modal flow | 0 | plan.propose returns typed assignments/unplaced/diagnostics; infeasible hard shortfall reported without mutating facts; partial/unplaced work visible; approval/commit separate |


## HARN-H-17 agent minimization

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| H17-0001 | 2026-10-25T04:08:00+08:00 | Agent/MCP projections minimized; no private snapshot/approval | `tests/browser/a3-mcp-sdk.mjs`; `a3-mcp-authz.json` | 0 | MCP list returns only id/title/status/estimate/priority/target_due; private snapshot 403; approve/commit 403; tool schemas expose no note bodies/tokens |


## HARN-H-06 IME composition separation

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| H06-0001 | 2026-10-25T04:12:00+08:00 | Ctrl+Enter during composition does not submit; after composition does | `node tests/browser/a2-ime.mjs` | 0 | composing keydown: dialog stayed open; non-composing keydown: dialog closed, toast/commit r76; 0 page errors; screenshots h25-ime-*.png |


## HARN-H-03 same-UID threat boundary

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| H03-0001 | 2026-10-25T04:15:00+08:00 | Actual same-UID read of Core DB/config | local file read as user <user> | 0 | `a0/threat-boundary.json`; SQLite header readable, runtime host/port and token field names readable, token values not recorded; documented no-OS-isolation limitation and separate-identity recovery; no strong isolation claim |


## AI-11 / FND-09 cloud route with minimized context

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| CLOUD-0001 | 2026-10-25T04:20:00+08:00 | Authorized cloud call via local OpenAI-compatible stub | `python tools/test_ai11_cloud_stub.py` | 0 | `a3/ai11-cloud-stub.json`; policy cloud_allowed + provider allowlist + credential; goal.decompose run succeeded provider cloud-stub/route cloud with 3 tasks; one request contained goal but not device-only note body; no fallback |


## PLN-08 deterministic plan order

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN08-0001 | 2026-10-25T04:25:00+08:00 | Same inputs identical; unrelated update does not reorder | `cargo test -p zodo-runtime --test a4_p1 planning_is_deterministic` | 0 | two equal-priority tasks planned twice produce identical assignment JSON; editing an unrelated task does not change assignments order/times |


## AI-13 frozen eval privacy extension

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| AIEVAL-0002 | 2026-10-25T04:30:00+08:00 | Add frozen cloud-context privacy case | `python tools/run_ai_eval.py` | 0 | `ai-eval.json` now 4 cases: recorded valid decomposition, 2 adversarial rejects, frozen cloud request must contain goal but not device_only note/api_key/Bearer; 4/4 passed |


## EXE-04 / AI-05 review facts and adjustments

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| REVIEW-0001 | 2026-10-25T04:36:00+08:00 | Fact-only review build; adjustment proposal | `cargo test -p zodo-runtime --test a4_p1 review_builds`; a4-estimate-adjust E2E | 0 | review.build without model: actual 25, tasks_opened 1, completed 0, notes/recommendations persisted; estimate adjustment proposal approved/committed earlier at r41 |


## PLN-10 busy snapshot import/freshness

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN10-0001 | 2026-10-25T04:42:00+08:00 | Busy/calendar upsert, freshness, read-only | `python tools/test_pln11_calendar.py` | 0 | 2→3 events by source/external id, replace →1, invalid batch 422 no write, source last_sync updated; imported events only block busy time and do not move commitments/allocations |


## HARN-H-22 / FND-11 Host event reconnect and revocation filter

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| H22-0001 | 2026-10-25T04:50:00+08:00 | BFF event cursor, reconnect, revocation | `node tests/browser/a3-events-reconnect.mjs` | 0 | session 200, first events 50/ev_50; new command then incremental 45 events; re-minted session resumes at same cursor with 45; agent events after revocation 403; restore 200; token values not recorded |


## AI-09 model failure local-only/manual fallback

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| AI09-0001 | 2026-10-25T04:55:00+08:00 | Provider failure preserves run input/no cloud fallback; manual path | `tools/test_a3_privacy.py` evidence | 0 | local_only broken provider run failed PROVIDER_UNAVAILABLE without cloud fallback; run input persisted; no proposal/facts from failure; manual task path remains independent |


| A7-0010 | 2026-10-25T05:00:00+08:00 | Round10 release with Host events route | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round10.txt`; final Host restarted and smoke passed |


## ORG-08 note attachment metadata

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| ORG08-0001 | 2026-10-25T05:08:00+08:00 | Note link, attachment hash/metadata, no binary in task | `cargo test -p zodo-runtime --test a4_p1 note_attachment` | 0 | note linked to task; attachment location/media_type/device_only/64-hex sha256 persisted; invalid hash rejected; task JSON has no attachments/hash; remove works; auth via normal command capabilities |


## PLN-13 reminder intent/cancel/delivery

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN13-0001 | 2026-10-25T05:15:00+08:00 | Durable reminder pending/cancel/delivered | `cargo test -p zodo-runtime --test a4_p1 reminder_intent` | 0 | fire_at UTC pending; cancel → Cancelled; mark_delivered → Delivered with delivered_at; no duplicate effect; explicit fire window semantics still pending |


| A7-0011 | 2026-10-25T05:20:00+08:00 | Round11 release with attachment metadata | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round11.txt`; Host restarted with final release and smoke passed |


## PLN-03 capacity lock/past immutability

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN03-0001 | 2026-10-25T05:25:00+08:00 | Locked/past capacity update/remove rejection | `cargo test -p zodo-runtime --test a4_p1 locked_and_past_capacity` | 0 | locked capacity update/remove rejected; capacity with past allocation cannot be edited; explicit unlock required; overlap-union matrix still pending |


## PLN-02 / UI-L04 DST boundary decisions

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN02-0001 | 2026-10-25T05:35:00+08:00 | IANA validation + NY DST hard deadline boundaries | `cargo test -p zodo-runtime --test a4_p1 iana_timezone` | 0 | invalid zone rejected; 2026-03-08 23:59 NY → 2026-03-09T03:59:59Z; 2026-11-01 23:59 NY → 2026-11-02T04:59:59Z |
| PLN02-0002 | 2026-10-25T05:35:00+08:00 | UI ambiguous/missing wall-clock branches | Playwright against live Host, workspace temporarily America/New_York | 0 | `a4/pln02-dst.png`; 2026-11-01 01:30 showed two-instant choice, save without choice rejected, choice saved r79; 2026-03-08 02:30 missing-time error; workspace restored Asia/Shanghai |


| A7-0012 | 2026-10-25T05:45:00+08:00 | Round12 release with capacity immutability and DST fixes | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round12.txt`; Host restarted and smoke passed |


## HARN-H-25 Host job and Core Run lifecycle separation

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| H25-0001 | 2026-10-25T05:55:00+08:00 | Start run via Host, stop Host, Core completes, restart and read | `node tests/browser/a3-h25-start.mjs` + Host stop/restart | 0 | Session 200, goal created, run 202 `run_3fe3...`; Host stopped immediately; Core run reached `succeeded` while Host down; Host restarted, smoke passed, run still readable/succeeded in `a3/h25-summary.json` |


## UI-L03 dependency cycle error branch

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIL03-0001 | 2026-10-25T06:02:00+08:00 | UI adds reverse dependency, expects atomic cycle error | Playwright live Host | 0 | A→B existing; TaskDetail B? selecting successor A/predecessor B produced `dependency would create a cycle; the original graph is unchanged`; dialog stayed open for correction, 0 page errors; screenshot `a4/ui-l03-cycle.png` |


## UI-L02 task editor validation

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIL02-0001 | 2026-10-25T06:05:00+08:00 | Empty title rejected; Escape closes | Playwright live Host | 0 | `a4/ui-l02-validation.png`; save empty title → `请填写标题`; Escape closed dialog, no page errors; full failure/focus matrix remains partial |


## AI-04 model-authored local replan

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| AI04-0001 | 2026-10-25T06:20:00+08:00 | MCP zodo_replan creates typed replan proposal; no commit | `node tests/browser/a4-replan-mcp.mjs` | 0 | tools include zodo_replan; proposal prp_d9d4... status draft, 55 assigned/0 unplaced; no approve/commit tool; locked/past facts preserved by planner; Harness UI shows 重新规划 entry |


| A7-0013 | 2026-10-25T06:30:00+08:00 | Round15 release with replan tools | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round15.txt`; final Core/plugin binary copied, Host restarted, smoke and MCP zodo_replan passed |


## UI-L08 focus pause/resume/cancel

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIL08-0001 | 2026-10-25T06:40:00+08:00 | Start → pause → resume → cancel focus in UI | Playwright live Host | 0 | 不关联任务 start; pause button visible → paused notice; resume → running; cancel confirmation accepted → ready-to-start state; 0 page errors; screenshot `a4/ui-l08-focus.png` |


## UI-L09 complete/archive/cancel lifecycle

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIL09-0001 | 2026-10-25T06:50:00+08:00 | Complete→reopen, archive→restore, cancel with confirmation | Playwright live Host | 0 | 标记完成→重新打开→标记完成; 归档→恢复→归档; 取消确认后显示已取消; 0 page errors; screenshot `a4/ui-l09-lifecycle.png` |


| A7-0016 | 2026-10-25T07:00:00+08:00 | Round16 release with focus and task lifecycle UI | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round16.txt`; focus pause/resume/cancel and task cancel UI included |


## UI-L11 outbound preview and actual payload

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIL11-0001 | 2026-10-25T07:05:00+08:00 | Read-only outbound preview + captured cloud request | A4 surfaces + `tools/test_ai11_cloud_stub.py` | 0 | preview shows source/destination/read fields/classes/truncation and no tokens; captured cloud request contains goal but no device_only note body; same facts match run input/checkpoint |


## UI-L13 auth-state read

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIL13-0001 | 2026-10-25T07:20:00+08:00 | Core/Host read revocations; UI shows current state | Playwright live Host | 0 | GET /zodo-bridge/v1/admin/revocations → 200 `{revoked_channels: []}`; Agents page shows 未撤销 badges without a preceding revoke action; 0 page errors; screenshot `a4/ui-l13-authstate.png` |


| A7-0017 | 2026-10-25T07:30:00+08:00 | Round17 release with auth-state read | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round17.txt`; Core/plugin restarted and smoke passed |


## UI-L10 reminder intent/delivery detail

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIL10-0001 | 2026-10-25T07:40:00+08:00 | Reminder pending and delivered detail | Playwright live Host | 0 | TaskDetail showed 等待送达; Core reminder.mark_delivered 200; detail then showed 送达 + timestamp; 0 page errors; screenshots `a4/ui-l10-pending.png`, `a4/ui-l10-delivered.png` |


| A7-0018 | 2026-10-25T07:50:00+08:00 | Round18 release with reminder delivery detail | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round18.txt`; Core/plugin restarted and smoke passed |


## UI-L02 failure branch

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIL02-0002 | 2026-10-25T08:00:00+08:00 | Simulated bridge 503 on task.create | Playwright route interception | 0 | dialog stayed open with `模拟桥接失败`; 0 page errors; screenshot `a4/ui-l02-failure.png`; focus restoration remains pending |


| A7-0019 | 2026-10-25T08:10:00+08:00 | Round19 release with modal focus fallback | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round19.txt`; Host/Core restarted and smoke passed |


## UI-L02 focus return

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIL02-0003 | 2026-10-25T08:20:00+08:00 | Escape closes dialog and returns focus to trigger | Playwright live Host | 0 | :focus is BUTTON 新建任务; dialogs 0; 0 page errors; `a4/ui-l02-focus.json`, `a4/ui-l02-focus.png` |


## UI-L05 capacity validation

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIL05-0001 | 2026-10-25T08:30:00+08:00 | Empty name and inverted range validation | Playwright live Host | 0 | 空名称 → 请填写容量名称; start>end → Core `capacity block end must be after start`; dialog stayed open; 0 page errors; `a4/ui-l05-capacity-validation.png`; overlap matrix pending |


| A7-0020 | 2026-10-25T08:40:00+08:00 | Round20 release with modal focus return | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round20.txt`; Host/Core restarted and smoke passed |


## UI-L15 command palette keyboard/overflow

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIL15-0001 | 2026-10-25T08:50:00+08:00 | Ctrl+K, filter, ArrowDown, Enter, Escape | Playwright live Host | 0 | active row changed 打开任务 → 新建任务; Enter opened task editor; list max-height 360 with scrollIntoView; Escape closed; 0 page errors; `a4/ui-l15-palette.json/png` |


| A7-0021 | 2026-10-25T09:00:00+08:00 | Round21 release with palette keyboard nav | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round21.txt`; Host/Core restarted and smoke passed |


## UI-L12 data import preflight/error

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIL12-0001 | 2026-10-25T09:15:00+08:00 | Invalid JSON error; valid export preflight modal | Playwright live Host | 0 | bad JSON → parse error in page; valid `zodo.export/1` → 导入预检 modal with preflight counts; cancel works; 0 page errors; `a4/ui-l12-import-preflight.png` |


| A7-0022 | 2026-10-25T09:25:00+08:00 | Round22 release with import preflight | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round22.txt`; Host/Core restarted and smoke passed |


## FND-06 unknown-result reconciliation

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FND06-0002 | 2026-10-25T09:35:00+08:00 | Mark delivered before effect retry; worker reconciles | `python tools/test_fnd06_reconcile.py` | 0 | reminder marked delivered; effect_outbox row status=delivered attempts=0 (no second toast/fact); `a1/fnd06-reconcile.json`; durable outbox/backoff/terminal failure already tested |


## UI-L14 busy calendar import/freshness

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIL14-0001 | 2026-10-25T09:45:00+08:00 | Invalid JSON error; valid busy snapshot import; freshness | Playwright live Host | 0 | bad JSON showed parse error in modal; valid manual snapshot closed modal; 日历新鲜度 shows UI-L14 快照 with provider/last_sync; 0 page errors; `a4/ui-l14-busy-import.png` |


## FND-04 run state/cancel race

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FND04-0002 | 2026-10-25T09:55:00+08:00 | waiting cancel, succeeded, interrupted, cancel-vs-success | `cargo test -p zodo-runtime --test fnd04_runs` | 0 | waiting run cancel → cancelled immediately; succeeded output/ended_at; interrupted terminal; running cancel_requested then success update rejected, final cancelled |


| A7-0025 | 2026-10-25T10:05:00+08:00 | Round25 release with run-state race fix | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round25.txt`; Core/plugin restarted and smoke passed |


## PLN-09 hard work windows/weekend policy

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN09-0001 | 2026-10-25T10:15:00+08:00 | Hard window excludes Tuesday; soft window does not; weekend Deny | `cargo test -p zodo-runtime --test a4_p1 work_windows` | 0 | hard Monday 09-12 → Tuesday capacity unplaced; soft-only window → Tuesday capacity still assigned; weekend Deny → Sunday capacity unplaced |


| A7-0026 | 2026-10-25T10:25:00+08:00 | Round26 release with hard work windows/weekend | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round26.txt`; Host/Core restarted and smoke passed |


## ORG-09 scoped search pagination/privacy

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| ORG09-0001 | 2026-10-25T10:40:00+08:00 | Pagination, filter, privacy, UI load-more | `cargo test -p zodo-runtime --test a4_p1 search_pagination`; Playwright | 0 | 26 fixture results paged 10/10/6 with no overlap; device_only note excluded, cloud_allowed included; live UI 60 results: 50 + 加载更多 → 60; stale response guard via request sequence; `a4/org09-search-pagination.json/png` |


| A7-0027 | 2026-10-25T10:50:00+08:00 | Round27 release with search pagination | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round27.txt`; Host/Core restarted and smoke passed |


## PLN-03 overlapping busy union

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN03-0002 | 2026-10-25T11:00:00+08:00 | Overlapping busy events subtract union, not sum | `cargo test -p zodo-runtime --test a4_p1 overlapping_busy` | 0 | 240-min capacity; 02:00-03:00 and 02:30-03:30 busy union 90 min → 150 assigned / 90 unplaced; locked/past capacity immutability already tested |


## UI-L05 capacity/commitment validation and overlap

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIL05-0002 | 2026-10-25T11:05:00+08:00 | Editors + validation + overlap union | Playwright + `overlapping_busy` test | 0 | capacity/commitment success paths; empty-name/inverted-range errors; capacity locked/past and busy overlap union verified; `ui-l05-capacity-validation.png` |


## HARN-H-09 foreground draft recovery

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| H09-0001 | 2026-10-25T11:15:00+08:00 | Capture draft survives page foreground reload | Playwright live Host | 0 | draft stored in localStorage; page reload → capture reopened with identical text + 已恢复 warning; no Core inbox fact written; `a2/h09-draft-recovery.json/png` |


## PLN-13 reminder timezone/cancel/delivery

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN13-0002 | 2026-10-25T11:25:00+08:00 | UI local reminder 09:30 → Core UTC; detail visible | Playwright live Host | 0 | reminder intent at 2026-10-27T09:30 Asia/Shanghai saved as 2026-10-27T01:30Z; TaskDetail shows message; cancel/delivery durability covered by reminder_intent test; `a4/pln13-reminder-timezone.json/png` |


## FLOW-F08 external agent to revocation/audit

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FLOWF08-0001 | 2026-10-25T11:35:00+08:00 | MCP propose → UI approve/commit → revoke → denied → restore | `node tests/browser/flow-f08.mjs` | 0 | proposal validated, commit committed, task found; MCP list after revoke 403 PERMISSION_DENIED `channel mcp_stdio has been revoked`; restore 200; change-set/audit event present; agent never self-approved; `a3/flow-f08.json` |


## FLOW-F01/F02 end-to-end capture flows

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FLOWF01-0001 | 2026-10-25T11:45:00+08:00 | First capture → converted task → completed; missing entry error | `cargo test -p zodo-runtime --test a4_p1 flow_f0` | 0 | capture entry processed into 1 task, task completed Done, inbox Processed; missing capture id rejected |
| FLOWF02-0001 | 2026-10-25T11:45:00+08:00 | Fragmented material → clarify → 3 grounded tasks | same test | 0 | clarify stored on entry; convert creates 3 Open tasks with estimates/target dates; inbox Processed with converted refs |


## FLOW-F03 goal→project→committed plan

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FLOWF03-0001 | 2026-10-25T11:55:00+08:00 | Goal→project→task→plan→approve→commit | `cargo test -p zodo-runtime --test a4_p1 flow_f03` | 0 | goal Active, project linked, task linked; plan validated, approval granted, commit produced allocation; empty horizon later returns unplaced without commit |


## FLOW-F04 schedule change → stable local replan

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FLOWF04-0001 | 2026-10-25T12:05:00+08:00 | Locked work fixed; unlocked work replanned after new meeting | `cargo test -p zodo-runtime --test a4_p1 flow_f04` | 0 | locked A allocation unchanged; B 60 min reallocated outside 02:00–02:30 meeting; approval/commit applied; no overlap |


## FLOW-F05 execute → progress → review → replan

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FLOWF05-0001 | 2026-10-25T12:15:00+08:00 | Progress 30/60 → review facts → replan remaining 30 | `cargo test -p zodo-runtime --test a4_p1 flow_f05` | 0 | task InProgress, remaining 30; review actual 30; second plan/commit replaces allocation with 30 min; task not auto-completed |


## FLOW-F06 recurring tasks/habit exception

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FLOWF06-0001 | 2026-10-25T12:25:00+08:00 | Habit log dedupe; recurrence skip/move single exception; no duplicate expansion | `cargo test -p zodo-runtime --test a4_p1 flow_f06` | 0 | habit log once, duplicate rejected; 3 weekly occurrences; skip first cancels only it; move second to 2026-11-05; re-expand adds no duplicates |


## FLOW-F07 review → reversible preference → next plan

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FLOWF07-0001 | 2026-10-25T12:40:00+08:00 | Review facts; toggle daily limit then revert; next-week plan/approve/commit | `cargo test -p zodo-runtime --test a4_p1 flow_f07` | 0 | review actual 30; daily_limit 120→null reversible; next-week capacity plan approved/committed with allocation |


## AI-01 multi-turn clarification

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| AI01-0001 | 2026-10-25T12:50:00+08:00 | Two clarification turns before conversion; no facts | `cargo test -p zodo-runtime --test a4_p1 flow_ai01` | 0 | 2 clarifications stored; 0 tasks/goals before explicit convert; convert → processed + 1 task |


## UI-L16 calendar source diagnostics

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIL16-0001 | 2026-10-25T13:05:00+08:00 | Source event count/staleness badges | Playwright live Host | 0 | 2 sources rendered with `N 个忙碌事件`, last sync, 新鲜/过期 badge, provider; 0 page errors; `a4/ui-l16-calendar-diagnostics.json/png` |


| A7-0036 | 2026-10-25T13:15:00+08:00 | Round36 release with calendar diagnostics | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round36.txt`; Host/Core restarted and smoke passed |


## UI screen family verification batch

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UISCREENS-0001 | 2026-10-25T13:25:00+08:00 | Loaded-state matrix for 21 views | Playwright live Host | 0 | all 21 views rendered correct headings, 0 page errors; screenshots `a4/screen-*.png`, `a4/screen-matrix.json`; Today/Search/Focus/Agents additionally covered by feature tests |


## UI screen family verification batch 2

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UISCREENS-0002 | 2026-10-25T13:35:00+08:00 | Feature-backed screens: Inbox/Goals/Tasks/Schedule/Proposals/Review/Models/Privacy/Audit | Playwright matrix + A2/A4 tests | 0 | each screen loaded via nav with correct heading and screenshot; feature tests cited per ledger row; 0 page errors |


## UI screen family verification batch 3

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UISCREENS-0003 | 2026-10-25T13:45:00+08:00 | First-use/Capture/Projects/TaskDetail/TimeDetail/Clarify/Diagnostics/Receipt | A2/A4 feature tests + screenshots | 0 | each screen/panel entered successfully with empty/first-use or real data; feature tests cited in ledger; 0 page errors |


## UI screen family verification batch 4

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UISCREENS-0004 | 2026-10-25T13:55:00+08:00 | Saved views/Habits/Habit detail/Data/Proposal compare | Playwright matrix + A4 tests | 0 | views/habits/data loaded; saved-view editor, habit create/log/detail, import/export/backup, plan diff all tested; 0 page errors |


## EXE-07/08 and ORG-13 adjustment/preference closure

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| EXESTATS-0001 | 2026-10-25T14:05:00+08:00 | Actual-vs-estimate proposal bound and reversible preference | A4 estimate-adjust E2E + F07 test | 0 | adjustment proposal validated→approved→committed from actual/remaining; daily_limit 120→null reversible; no automatic permanent rule |


## ORG-12 milestone CRUD/independent completion

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| ORG12-0001 | 2026-10-25T14:15:00+08:00 | Add/update/remove milestone; project/task independence | `cargo test -p zodo-runtime --test a4_p1 project_milestones` | 0 | milestone added due 2026-11-01; marking done leaves project Active and task Open; remove works |


## ORG-11 batch and ORG-14 blocked derivation

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| ORG11-0001 | 2026-10-25T14:25:00+08:00 | Batch preview/apply/unknown atomic | `cargo test -p zodo-runtime --test a4_p1 batch_update` | 0 | preview validated affected=2 no writes; apply archive+tags both; unknown id NotFound with no partial |
| ORG14-0001 | 2026-10-25T14:25:00+08:00 | Manual block reason clear; dependency-derived block | `cargo test -p zodo-runtime --test a4_p1 blocked_state` | 0 | manual blocked reason set/cleared; successor DEPENDENCY_BLOCKED until predecessor done, then assignable |


## FND-16 FFI run lifecycle ABI

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FND16-0001 | 2026-10-25T14:40:00+08:00 | C ABI run start/poll/cancel/free + panic isolation | `cargo test -p zodo-ffi` | 0 | 3/3 tests: open/command/query/free; run_start queued → poll queued → cancel cancelled+ended_at → poll cancelled; null/bad JSON errors no panic |


| A7-0040 | 2026-10-25T14:55:00+08:00 | Round40 release with FFI run ABI | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round40.txt`; Host/Core restarted and smoke passed |


## ORG-07 tag attach/filter composition

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| ORG07-0001 | 2026-10-25T15:05:00+08:00 | Tags on tasks and composed filters | `cargo test -p zodo-runtime --test a4_p1 tags_attach` | 0 | tag create; task A tags; task B set_tags [深度工作,重要]; filter tag+status returns 2; tag 重要 returns 1; tags persisted |


## UI-S06/S08 goal/project workspaces

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UISCREENS-0005 | 2026-10-25T15:15:00+08:00 | Goal detail/decomposition and project workspace | Playwright matrix + A3/A4 tests | 0 | goals/projects screens loaded; goal create/achieve, AI goal.decompose proposal, project create/tasks tested; screenshots screen-goals/projects; 0 page errors |


## DAT-01/FND-12 restore preflight/corrupt/duplicate

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| RESTORE-0001 | 2026-10-25T15:30:00+08:00 | Valid restore, pre-restore snapshot, duplicate restore, corrupt/missing fail closed | `python tools/test_restore_preflight.py` | 0 | first restore exit0 auth_reset; second creates snapshot; duplicate restore exit0 with `-1` snapshot; corrupt exit1 integrity fail and live DB hash unchanged; missing exit1; `a6/restore-preflight.json` |


| A7-0042 | 2026-10-25T15:40:00+08:00 | Round42 release with restore fixes | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round42.txt`; Host/Core restarted and smoke passed |


## UI-S19 stale proposal version comparison

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIS19-0001 | 2026-10-25T15:55:00+08:00 | Stale proposal badge after revision bump | Playwright live Host | 0 | proposal base r163 vs current r164; Proposals view shows 过期版本 badge + 当前 r164; 0 page errors; `a4/ui-s19-stale-proposal.png/json` |


| A7-0043 | 2026-10-25T16:05:00+08:00 | Round43 release with stale proposal UI | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round43.txt`; Host/Core restarted and smoke passed |


## UI-S25 settings/time connections

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| UIS25-0001 | 2026-10-25T16:15:00+08:00 | Settings loaded with calendar/time connections | Playwright live Host | 0 | 时间连接与日历源 section rendered; 2 calendar sources with provider/read-only/last sync; 0 page errors; `a4/ui-s25-settings-time.json/png` |


| A7-0044 | 2026-10-25T16:25:00+08:00 | Round44 release with settings time connections | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round44.txt`; Host/Core restarted and smoke passed |


## EXE-05 daily/weekly stats and habit adherence

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| EXE05-0001 | 2026-10-25T16:35:00+08:00 | Review/stats facts incl habit target/adherence | `cargo test -p zodo-runtime --test a4_p1 review_stats_include` | 0 | 3/5 scheduled completions → habit_completions 3, target 5, adherence 60%; stats.get same; Review UI shows 达成%/目标 |


| A7-0045 | 2026-10-25T16:45:00+08:00 | Round45 release with habit adherence | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round45.txt`; Host/Core restarted and smoke passed |


## FND-10 bounded automation

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FND10-0001 | 2026-10-25T16:55:00+08:00 | Bounded manual runs, budget auto-disable, toggle/delete | `cargo test -p zodo-runtime --test a4_p1 automation_manual_run` | 0 | 2 allowed runs, third budget-exceeded, rule auto-disabled; re-enable/delete; schedule worker has 60s min interval and task.create action |


## REC-05 habit quota/opportunity windows

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| REC05-0001 | 2026-10-25T17:05:00+08:00 | Weekly target + scheduled weekdays/time; 2/5 completion | `cargo test -p zodo-runtime --test a4_p1 habit_weekly_quota` | 0 | quota target 5 from Mon-Fri window; 2 completions counted; time_of_day 07:30 preserved; no task auto-created |


## PLN-11 calendar cursor/provider mapping

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN11-0001 | 2026-10-25T17:15:00+08:00 | Source cursor persisted/updated; provider external-id mapping; bad batch no write | `cargo test -p zodo-runtime --test a4_p1 calendar_source_cursor` | 0 | cursor-1→cursor-2 on same provider/external_id, one source, replaced event; invalid start/end batch rejected and source/event count unchanged |


## ORG-10 saved views/unresolved conditions

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| ORG10-0001 | 2026-10-25T17:25:00+08:00 | Saved view with unknown filter key shows unresolved | Playwright live Host | 0 | view `natural_language` key rendered `未解析条件：natural_language；应用时会被忽略，不会静默当作已支持`; CRUD/apply tested earlier; 0 page errors; `a4/ui-org10-unresolved.json/png` |


| A7-0048 | 2026-10-25T17:35:00+08:00 | Round48 release with calendar cursor/unresolved views | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round48.txt`; Host/Core restarted and smoke passed |


## REC-04 recurrence scopes

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| REC04-0001 | 2026-10-25T17:45:00+08:00 | future vs series scope backend; series UI | `cargo test -p zodo-runtime --test a4_p1 recurrence_scope_future`; Playwright | 0 | future updates 2 current/future instances; series updates 3 non-closed instances incl past; closed history untouched; UI 整个系列 submits and changes rule+3 task titles; `a4/rec04-series-scope.json` |


| A7-0049 | 2026-10-25T17:55:00+08:00 | Round49 release with whole-series recurrence scope | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round49.txt`; Host/Core restarted and smoke passed |


## PLN-12 sync conflict retry evidence

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN12-0001 | 2026-10-25T18:05:00+08:00 | Union import, divergence 409/no writes, retry | `python tools/test_a6_sync.py` | 0 | A/B union 2+2 tasks; divergence conflict 409 with 1 conflict and no writes; retry still 409 (explicit resolution required); `a6/a6-swap.json` |


## PLN-12 sync conflict resolution/retry

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN12-0002 | 2026-10-25T18:20:00+08:00 | Explicit incoming resolution then retry | `python tools/test_a6_sync.py` | 0 | conflict 409 with 1 entity/no writes; retry with resolutions {id: incoming} → 200, resolved=1, B title adopted A; union export/import duplicate-free; `a6/a6-swap.json` |


| A7-0051 | 2026-10-25T18:30:00+08:00 | Round51 release with sync conflict resolution | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round51.txt`; Host/Core restarted and smoke passed |


## DAT-07/FND-13 device revoke/recovery

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| DAT07-0001 | 2026-10-25T18:45:00+08:00 | Device identity, conflict resolution, revoke and unrevoke | `python tools/test_a6_sync.py` | 0 | distinct device IDs; union 2+2; conflict 409/no writes; resolution incoming 200; revoke device 200; revoked import 403 PERMISSION_DENIED; unrevoke 200 and import no longer 403; `a6/a6-swap.json` |


| A7-0052 | 2026-10-25T18:55:00+08:00 | Round52 release with device revocation | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round52.txt`; Host/Core restarted and smoke passed |


## DAT-02 JSON/CSV import/export

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| DAT02-0001 | 2026-10-25T19:05:00+08:00 | CSV import preview + task creation; JSON export/import | Playwright + A4 tests | 0 | CSV preview 2 rows, confirm creates CSV 五/六 with estimates/tags; JSON export/import preflight tested; no credentials in exported files; `a4/data-csv-import.json/png` |


| A7-0053 | 2026-10-25T19:15:00+08:00 | Round53 release with CSV import | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round53.txt`; Host/Core restarted and smoke passed |


## DAT-03 safe restore/pre-restore/auth reset

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| DAT03-0001 | 2026-10-25T19:25:00+08:00 | Restore preflight, snapshot, auth reset | `python tools/test_restore_preflight.py` | 0 | valid backup restored with auth_reset=true; second restore created pre-restore snapshot; duplicate snapshot unique; corrupted backup fails closed; runtime_restart_required in result |


## DAT-04 batch import/idempotency/no half-batch

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| DAT04-0001 | 2026-10-25T19:40:00+08:00 | Two-bundle batch import, idempotent replay, conflicting batch reject | `python tools/test_dat04_batch.py` | 0 | batch bundles A+B → tasks A,B; same bundles/key replay replayed=true; conflicting same-id bundles 409 and target remains 0 tasks; `a4/dat04-batch-import.json` |


| A7-0055 | 2026-10-25T19:50:00+08:00 | Round55 release with batch import | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round55.txt`; Host/Core restarted and smoke passed |


## AI-07 role routing/config next request

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| AI07-0001 | 2026-10-25T20:00:00+08:00 | Role pin local/cloud; local_only no fallback; config roles next run | `cargo test -p zodo-runtime --lib ai::tests`; `python tools/test_ai11_cloud_stub.py` | 0 | roles decompose→cloud persisted in editable config; goal.decompose run used cloud-stub, 3 tasks; device-only note absent; cloud-pinned role under local_only fails truthfully; `a3/ai11-cloud-stub.json` |


| A7-0056 | 2026-10-25T20:10:00+08:00 | Round56 release with model role routing | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round56.txt`; Host/Core restarted and smoke passed |


## REC-08 multi-period planner/capacity fix

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| REC08-0001 | 2026-10-25T20:25:00+08:00 | 3-week planning with per-block capacity; no overlap | `cargo test -p zodo-runtime --test a4_p1 multi_week_planning` | 0 | 3 tasks ×120min placed across 10-26/11-02 blocks; per-block 180min not exceeded; work split across blocks totals 120/task; adjacent allocations do not overlap; no unplaced; planner slot cursor fix |


| A7-0057 | 2026-10-25T20:40:00+08:00 | Round57 release with cross-period capacity overlap fix | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round57.txt`; Host/Core restarted and smoke passed |


## PLN-14 notification failure/retry/recovery

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN14-0001 | 2026-10-25T20:55:00+08:00 | Toast delivery denied then recovery | `python tools/test_pln14_notification_retry.py` | 0 | script exit 1 → notifications log delivered=false error "Windows toast exited Some(1)"; restore script → retry delivered=true backend windows_toast; effect retry bounded 5 attempts/backoff; `a4/pln14-notify-retry.json` |


| A7-0058 | 2026-10-25T21:00:00+08:00 | Round58 notification recovery evidence | `python tools/test_pln14_notification_retry.py` | 0 | `a4/pln14-notify-retry.json`; release unchanged from round57 |


## DAT-06 local content personalisation/eval

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| DAT06-0002 | 2026-10-25T21:10:00+08:00 | Consent-gated local ranking | `cargo test -p zodo-runtime --test a4_p1 content_retrieval` | 0 | personalise=false → not_applied; true → local_linked_recency, linked note first score≥1; consent=false denied; scope local_workspace_only; deterministic eval corpus in test |


| A7-0059 | 2026-10-25T21:20:00+08:00 | Round59 release with local content personalisation | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round59.txt`; Host/Core restarted and smoke passed |


## REC-03 complex rules/end conditions/after-completion

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| REC03-0001 | 2026-10-25T21:35:00+08:00 | Weekly/monthly intervals, end_date, max occurrences, after-completion interval | `cargo test -p zodo-runtime --test a4_p1 after_completion`; `complex_weekly` | 0 | weekly interval2 [1,3] end 11-05 → 4 dates; monthly interval2 end 02-15 → 3 dates; after_completion_days=3 creates only first then next on completion at 10-27; end_date suppresses next; same completion key not duplicated; UI fields added |


| A7-0060 | 2026-10-25T21:45:00+08:00 | Round60 release with after-completion recurrence | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round60.txt`; Host/Core restarted and smoke passed |


## REC-06 habit skip/undo/streak

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| REC06-0001 | 2026-10-25T22:00:00+08:00 | Dedupe, skip, undo, corrected counts, reconstructed streak | `cargo test -p zodo-runtime --test a4_p1 habit_skip_and_undo` | 0 | 3 consecutive completions + skip day; duplicate skip rejected; review habit_completions=3 (skip excluded); undo removes skip then completion, counts 0/2; UI derives current/longest streak from facts and exposes skip/undo |


| A7-0061 | 2026-10-25T22:10:00+08:00 | Round61 release with habit skip/undo/streak | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round61.txt`; Host/Core restarted and smoke passed |


## HARN-H-24 remote Core routing

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| H24-0001 | 2026-10-25T22:35:00+08:00 | Host attaches to remote Core via env; task lands in remote instance | Playwright + Core HTTP | 0 | Host with ZODO_CORE_URL/TOKEN; snapshot rev1→2; task created through Host; remote Core query returns REMOTE task; remote instance rt_fcb…; `a3/h24-remote-core*.json/png` |


| A7-0062 | 2026-10-25T22:45:00+08:00 | Round62 release with remote Core routing | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round62.txt`; normal Host restarted after remote test and smoke passed |


## AI-02 dependency inference

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| AI02-0001 | 2026-10-25T22:55:00+08:00 | 3-task proposal with model depends_on; ref dependencies after commit | `python tools/test_ai11_cloud_stub.py`; `cargo test -p zodo-runtime --test a4_p1 proposal_batch_creates_dependencies` | 0 | model tasks depends_on []/[0]/[1]; proposal reviewed; commit creates 2 dependencies 一→二→三; client_ref batch test; `a3/ai11-cloud-stub.json` |


| A7-0063 | 2026-10-25T23:05:00+08:00 | Round63 release with AI dependency inference | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round63.txt`; Host/Core restarted and smoke passed |


## PLN-16 external calendar write/idempotency

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| PLN16-0001 | 2026-10-25T23:20:00+08:00 | Commitment external_id → outbox calendar write; duplicate rejection | `python tools/test_pln16_calendar_write.py` | 0 | ext-meeting-1 written with Idempotency-Key; duplicate external_id command 422 VALIDATION_FAILED; ext-meeting-2 written; 2 POSTs with titles preserved; `a4/pln16-calendar-write.json` |


| A7-0064 | 2026-10-25T23:30:00+08:00 | Round64 release with external calendar write | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round64.txt`; Host/Core restarted and smoke passed |


## FLT-01 shared generated contract client/a11y

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FLT01-0001 | 2026-10-25T23:45:00+08:00 | Generated Dart contract from shared JSON; client uses it; semantics header | `node scripts/generate-contract-client.mjs`; `flutter test` | 0 | 41 operations/17 queries generated; Flutter imports generated contract for command/query envelopes; contract fixture test + ZodoPage header semantics test; 8/8 Flutter tests pass |


| A7-0065 | 2026-10-25T23:55:00+08:00 | Round65 release with generated Flutter contract | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round65.txt`; Flutter 8/8 tests and Host smoke passed |


## FLT-05 production BFF session auth

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| FLT05-0001 | 2026-10-26T00:10:00+08:00 | Flutter web uses Host BFF cookie session, no URL token | Flutter web + proxy + Playwright | 0 | session 200 sets HttpOnly Strict `zodo_ui` Path=/zodo-bridge; rpc 200 returns snapshot; URL has no token; Today rendered r1? (r171); `a5/flutter-web-bff-session.json/png` |


| A7-0066 | 2026-10-26T00:20:00+08:00 | Round66 release with Flutter web BFF session auth | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round66.txt`; Flutter 8/8 and Host smoke passed |


## HARN-H-05 plugin hot-reload/subscription cleanup

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| H05-0001 | 2026-11-06T23:50:00+08:00 | Three consecutive adapter reloads; stable subscription count and live Core | Playwright + `POST /zodo-bridge/v1/admin/reload` | 0 | reloads 200, generations 3→5; subscriptions 16→16 each; health 200; Core snapshot revision 172 with 103 tasks unchanged; no page errors; `a3/h05-hot-reload.json` |


| A7-0067 | 2026-11-06T23:58:00+08:00 | DSH-only release with plugin hot reload | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-round67-dsh.txt`; Host/Core restarted, smoke passed |


## HARN-H-18/19 Host history guard and fork disclosure

| ID | Time (Asia/Shanghai) | Action | Command | Exit | Raw evidence |
|---|---|---|---|---|---|
| H18H19-0001 | 2026-11-07T00:05:00+08:00 | Non-local provider blocked for restricted history; fork refused | Playwright + `/zodo-bridge/v1/admin/guard-probe`, `/history-guard`, `/fork-guard` | 0 | guard-probe `blocked=true` `ZODO_HISTORY_RESTRICTED`; real Host session flagged restricted; fork-guard 409 `ZODO_FORK_RESTRICTED`; plugin tags device_only tool results with `[[ZODO_DEVICE_ONLY]]`; `a3/h18-h19-guards.json/png` |


| A7-0068 | 2026-11-07T00:15:00+08:00 | Final DSH-only release with history guards | `scripts/build-release.ps1` | 0 | `a7/release-build-info.json`, `a7/release-sha256.txt`, `a7/release-smoke-dsh-final.txt`; DSH ledger 164/164 verified; Flutter excluded by scope decision |
