# Zodo for DeepSeek Harness

Development plugin `0.2.0-dev.1`. The Rust Core owns facts, validation, proposals, approvals, revisions, durable runs and outbox; the Host is an authenticated adapter.

## Maintained surface

- Native left-sidebar Zodo entry and main-area React workbench. There is no old right-sidebar tab or A0 probe.
- Two Core AI modes: **仅本地** / **允许云端辅助**, with advanced options collapsed. See [mode and privacy rules](<../../../docs/MODEL_MODES.md>).
- Agent tools may capture, read scoped summaries and propose; they cannot approve or commit.
- CLI, MCP and the native C ABI remain non-UI interfaces.

## Development contract

Only the current schema and fields are supported. Old model modes, global note policy, field aliases and implicit operation versions are not migrated or accepted. Content-level note/attachment classifications remain enforced. Current protocol/version checks, backup integrity checks, structured private export prevention and business-approval boundaries are security rules, not compatibility shims.

Core and client must be updated together. Outdated development settings should be explicitly reset after a consistent backup; this is an administrative action, not an automatic runtime migration. Never erase tasks or notes to fix obsolete settings.

## Build and install

```powershell
cargo build --release -p zodo-runtime --locked
# Copy target/release/zodo.exe into bin/win32-x64/zodo-core.exe only after stopping its running instance.
```

Add this directory through the DSH plugin manager or install a package built by the [release script](<../../../scripts/build-release.ps1>). A source-directory link loads this source; a ZIP install uses its packaged files. The package declares the required current DSH services in its manifest. No probe executable is built or shipped.

Core data uses `ZODO_DATA_DIR`, otherwise the Windows user-local Zodo directory. Host discovers the runtime descriptor and connects to a healthy Core, or starts the packaged executable. Core runs detached and holds the single-writer lock; unloading Host does not stop it. Never start a second writer on the same data directory.

## Trust boundaries

- **UI:** authenticated same-origin bridge requests use a short-lived UI session; the browser does not receive Core credentials. Desktop forwarding uses the current Host UI-session transport.
- **Agent/MCP:** separate principals, scoped responses, no approval/commit capabilities, runtime revocation enforcement.
- **Core:** domain validation, proposal hash, approval binding, revision and idempotency.
- **Models:** credentials are referenced by environment-variable name and remain in the Core process. Model pages expose only presence/readiness, never values.
- **Privacy:** device-only exports are rejected before entering Host chat. Verified restricted history blocks subsequent dispatch/fork; content already sent cannot be retroactively recalled. Same-UID Host/Shell is not a strong OS sandbox.

Cloud assistance requires explicit provider authorization and credential preflight. Restricted input stays local; local failure never silently becomes a cloud request. Connection readiness is not a successful network/inference test.

## Verification

Use only the [current isolated browser regression](<../../../tests/browser/harness-isolated-regression.mjs>), [API runner](<../../../tools/run_isolated_integration.py>) and [regression entrypoint](<../../../scripts/run-regression.ps1>) for automated mutation tests. They create synthetic workspaces and owned services instead of using real task data or an existing Host profile.

Historical design and evidence remain documentation, not active product contracts. Archived scripts are not supported entrypoints and must not be run against user data.

## Updating a running desktop

Build/test first, back up data, stop the shared Core, replace its executable, and verify the new Core against the same workspace. Client resources also need rebuilding/reloading; changing source alone is not proof that the existing GUI has updated. Keep the current Host alive where possible; do not replace the GUI with another server. See [operations](<../../../docs/OPERATIONS.md>) and [release notes](<../../../docs/RELEASE.md>).
