# Zodo

Zodo is a Rust personal-operations Core with **one supported UI: the DeepSeek Harness plugin**.
The plugin contains the React workbench and authenticated Host bridge. Standalone Flutter/Web/native UI adapters and their build/test tooling have been removed per the 2026-11-07 scope decision.

The Rust Core remains independent: facts, validation, proposals, approvals, revisions, receipts, durable runs and outbox are not owned by the UI. CLI, MCP and the native C ABI remain non-UI interfaces.

## Repository map

| Area | Purpose |
|---|---|
| [Kernel](<crates/zodo-kernel/Cargo.toml>) | Domain contracts, deterministic planning and rules |
| [Runtime](<crates/zodo-runtime/Cargo.toml>) | SQLite, HTTP API, AI runs, effects, CLI and MCP |
| [Native ABI](<crates/zodo-ffi/Cargo.toml>) | Non-UI C ABI |
| [DSH plugin](<adapters/deepseek-harness/plugin/README.md>) | Sole maintained UI and Host adapter |
| [Regression runner](<scripts/run-regression.ps1>) | Core and isolated DSH UI/API checks |
| [Scope decision](<implementation/SCOPE_DECISION.md>) | Current delivery scope |
| [Historical evidence](<implementation/EVIDENCE_INDEX.md>) | Previous evidence, not proof of the current tree |

The original v1.3 design and historical evidence are retained for traceability; historical standalone-client claims do not describe the supported product.

最新执行结果与未覆盖范围见 [测试报告](<docs/TEST_REPORT.md>)；不要把历史台账当作本轮结果。

## Development policy

This project is in active development: only the current schema and two AI modes are supported. Deprecated modes/fields are rejected, not migrated or aliased. Historical design/evidence remains an archive, not an obligation to keep old APIs, UI entry points, probes or scripts working. Safety checks and current external protocol contracts remain mandatory.

## Build and test

```powershell
cargo test --workspace --locked
cargo build --release -p zodo-runtime --locked

# Pure adapter tests (no live data)
node --test tests/adapter/privacy.test.mjs

# Full isolated regression, after installing the test dependencies below
powershell -NoProfile -File scripts/run-regression.ps1 -WithIntegration -WithBrowser

# Package Core + native ABI + DSH plugin only
powershell -NoProfile -File scripts/build-release.ps1
```

Browser dependencies: install the locked packages in the [browser manifest](<tests/browser/package.json>) and [fixture runtime manifest](<tests/browser/harness-fixture-runtime/package.json>), using the matching lockfiles:

```powershell
pnpm --dir tests/browser install --frozen-lockfile
pnpm --dir tests/browser/harness-fixture-runtime install --frozen-lockfile
```

The [isolated API runner](<tools/run_isolated_integration.py>) and [workbench regression](<tests/browser/harness-isolated-regression.mjs>) create fresh Core instances. They must not point at a user workspace or a frozen archive. The workbench fixture uses the real plugin bridge and React UI with substitute Host services; it does not prove DSH login, installation, Electron forwarding or Host lifecycle compatibility.

For deployment see the [plugin instructions](<adapters/deepseek-harness/plugin/README.md>), [operations](<docs/OPERATIONS.md>) and [release notes](<docs/RELEASE.md>). Do not use an old test-profile launcher against archived data.

## Safety rules

- Previewing never writes authoritative facts. Models and agents only propose candidates.
- Approval is explicit and bound to content, revision, policy and expiry. Host tool permission is not business approval.
- Every mutation is revisioned and idempotent; one writer owns each workspace.
- Past, started and locked allocations are not moved automatically. Infeasibility is reported.
- [Two AI modes](<docs/MODEL_MODES.md>): `local_only` never falls back to cloud; `cloud_assisted` keeps device-only content local and checks cloud authorization/credentials before dispatch.
- Credentials, runtime descriptors, launch URLs and real user data are not test fixtures or release assets.
- Same-UID Host/Shell is not a strong OS sandbox.
