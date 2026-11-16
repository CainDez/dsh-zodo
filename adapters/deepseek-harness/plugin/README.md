# Zodo for DeepSeek Harness

Status: A2 Host + Client plugin plus A3 Core model/MCP/durable-run integration. A0 bridge evidence
remains in `../A0_LOCK.md`; the A0 probe binary is retained only as historical bridge evidence and is
no longer used by the product surface.

## Locked host

DeepSeek Harness Desktop `0.2.1-rc.3`, source tag `dsh-v0.2.1-rc.3`, source commit
`1f81c047fc48fa190a683e469daa699857e9afe8`, MIT. The installed `app.asar` SHA-256 is recorded in
`implementation/EVIDENCE_INDEX.md`.

## What this bundle does

| File | Role |
|---|---|
| `package.json` | Harness bundle and `dsh.client` manifest |
| `cordis.patch.yml` | Inserts the Host adapter row |
| `index.js` | Core discovery/start, native Zodo tools, same-origin UI BFF, agent/UI credential separation |
| `client.js` | Left-menu Zodo entry and main-area React workbench (existing right Sidebar tabs remain compatible): today, inbox, goals/projects, tasks, schedule, proposals, review, focus, search, settings, privacy, audit |
| `bin/win32-x64/zodo-core.exe` | Prebuilt independent Core runtime (built from this repository) |

The Host adapter owns no Zodo facts. It forwards commands/queries to the Core HTTP API and never
returns a Core token to the browser or model. A Host tool execution is a separate gate from a Core
business approval.

## Build the Core and plugin binary

```powershell
cargo build --release -p zodo-runtime
New-Item -ItemType Directory -Force adapters/deepseek-harness/plugin/bin/win32-x64
Copy-Item target/release/zodo.exe adapters/deepseek-harness/plugin/bin/win32-x64/zodo-core.exe
```

## Install into an isolated profile

Run the bundled CLI through Electron so it can read `app.asar`:

```powershell
$exe = 'C:\Users\<user>\AppData\Local\Programs\DeepSeek Harness\DeepSeek Harness.exe'
$bin = 'C:\Users\<user>\AppData\Local\Programs\DeepSeek Harness\resources\app.asar\dsh\node_modules\@deepseek-ai\dsh\lib\bin.js'
$env:ELECTRON_RUN_AS_NODE = '1'
$env:DSH_HOME = 'D:\projects\zodo_dsh\.runtime\dsh-home-a0'
$env:PATH = "D:\projects\zodo_dsh\.runtime\bin;$env:PATH"
& $exe $bin --profile zodo-a0 --from-default-profile web --dump-config
& $exe $bin plugin --profile zodo-a0 add 'D:\projects\zodo_dsh\adapters\deepseek-harness\plugin'
```

The runtime data directory is selected by `ZODO_DATA_DIR` (or `%LOCALAPPDATA%\zodo`). The Host
discovers `runtime.json`; if no healthy Core exists it starts the packaged `zodo-core.exe` detached
and then connects. The Core holds the single-writer lock, SQLite facts, receipts, approvals and
outbox. Stopping/unloading the Host does not stop the shared Core.

## Trust faces

- **Interactive UI:** `/zodo-bridge/v1/session` mints an HttpOnly SameSite cookie. `/rpc` accepts only
  same-origin calls with that cookie and forwards Core commands/queries with the UI token. The browser
  never receives a Core token.
- **Agent tools:** `zodo_capture`, `zodo_list_tasks`, `zodo_propose`, `zodo_plan_propose`, and
  `zodo_get_proposal` use the agent token. They expose structured summaries, never approval/commit
  capability. The Core rejects agent approval/commit by principal kind.
- **Core:** domain validation, actor resolution, proposal hash, approval binding, revision and
  idempotency are all in Rust.

## A3 additions

The Host BFF also proxies Core `/v1/runs`, `/v1/model` and `/v1/admin/revoke` with the interactive UI
session. The React workbench adds an AI Run center, Models page, and revoke/restore controls.

Core-owned A3 flow:

1. A goal card starts a durable `goal.decompose` run.
2. The Core Run worker calls the configured local/cloud model outside the fact transaction.
3. The model output is checkpointed, then Core creates a validated proposal as `delegated_agent`.
4. The React workbench shows the task additions/diagnostics; a user approval plus commit consumes the
   grant and writes a receipt.
5. Cancellation, crash/restart recovery, revocation and reminder delivery are Core/worker concerns,
   not Host state.

MCP stdio (`zodo --data-dir <dir> mcp`) is independent of the Host and uses the MCP token. It exposes
read/list/plan/propose/get only; it cannot approve or commit, and cannot read the private snapshot.
`@modelcontextprotocol/sdk` 1.20.2 is used for the installed-client proof.

Model credentials are read from environment names recorded in `model.json`; no API key is exposed to
the browser, Host tools, MCP, or evidence. `local_only` never silently falls back to cloud.

The Models page also shows a read-only Harness model catalog (Host BFF `GET
/zodo-bridge/v1/harness/models`, interactive UI session only); `填入 Core 模型` fills the Core
model-connection draft from a Harness route, but only provider/model names, endpoints and credential
reference names cross the bridge — no secret is copied, Harness configuration is never mutated, and
the Core save stays an explicit user action. Verified by `tests/browser/zodo-harness-models.mjs`.

## Verification

Browser evidence scripts live in `tests/browser/`:

```powershell
$env:ZODO_A2_URL = Get-Content .runtime\a2-url.txt -Raw
node tests/browser/a2-flow.mjs
node tests/browser/a2-native-tools.mjs
node tests/browser/a2-agent-approval.mjs
node tests/browser/a2-manual-flow.mjs
node tests/browser/a2-inbox-flow.mjs
node tests/browser/a3-ai-ui.mjs
node tests/browser/a3-ui-surfaces.mjs
cd tests/browser; node a3-mcp-sdk.mjs
```

A3 Python reproduction scripts are under `tools/test_a3_*.py`; evidence is under
`artifacts/evidence/a3/`.

Results and screenshots are in `artifacts/evidence/a2/` and indexed in
`implementation/EVIDENCE_INDEX.md`.

## Known limits at A2

- Core model routing, MCP, durable AI Runs, actual notification delivery and cross-device recovery
  are A3/A4 work.
- The A0 incompatible-artifact and hot-unload arms remain open.
- The Host is trusted code under the same OS user; the UI cookie and Core token separation is not an
  OS sandbox.
- UI P1 screens (habits, import/export, incremental calendar, automation, sync) are not yet exposed.
