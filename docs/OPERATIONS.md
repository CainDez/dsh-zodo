# Operations — DSH plugin UI

The maintained product is the independent Rust Core plus the [DeepSeek Harness plugin](<../adapters/deepseek-harness/plugin/README.md>). There is no standalone UI client, validation proxy or Dart client generator.

## Runtime ownership

Core data uses `ZODO_DATA_DIR`, otherwise the Windows user-local Zodo directory. The Core owns the workspace writer lock and can outlive the Host. Never start two writers on one directory or silently switch an installed plugin to a test archive.

Runtime descriptors, auth/model configuration, device identity, SQLite databases and notification logs may contain secrets or personal facts. Do not publish them or include them in a release. Models read credential environment variable names from configuration; values stay in the Core process.

Build/package instructions are in [release notes](<RELEASE.md>); actual plugin installation and connection follow the [plugin instructions](<../adapters/deepseek-harness/plugin/README.md>). Packaging does not reload the running GUI.

## Safe regression

```powershell
powershell -NoProfile -File scripts/run-regression.ps1 -WithIntegration -WithBrowser
```

The [regression entrypoint](<../scripts/run-regression.ps1>) no longer reads old launch-URL files. The [API runner](<../tools/run_isolated_integration.py>) creates a unique data tree with owned ephemeral ports and dummy model/calendar endpoints; the [React workbench fixture](<../tests/browser/harness-isolated-fixture.mjs>) uses a fresh Core and substitute Host services.

Do not run legacy evidence scripts directly: some reference an archived workspace or an existing Host, or deliver a real OS notification. The isolated API runner admits only reviewed sources and adds result assertions. Do not change admission hashes without reviewing the corresponding code.

## Backup, import and sync

- UI Data offers export, import preflight and local backup. Treat exported facts and backups as sensitive.
- Offline restore requires a stopped Core and a valid compatible backup; never restore a test fixture into a real workspace.
- Sync conflicts must be reviewed; a rejected import must not partially overwrite local facts.
- Encryption tests use synthetic facts and dummy passphrases. Automatic transport and device lifecycle need their own acceptance checks.

## Model and permission boundaries

- [Model and privacy settings](<MODEL_SETTINGS.md>) are independent: one explicit DSH/custom source, with deny-by-default goal/project/capture context permissions. DSH models execute through the authenticated Host llm service without copied credentials or chat history; custom sources call the selected endpoint. No cross-model fallback or legacy migration is maintained.
- An AI run produces proposals, not approval. Host tool permission is not business approval.
- Agent/MCP channels cannot approve or commit. Revocation is checked at runtime.
- Reading the DSH catalog or editing the model draft does not dispatch inference. Saving a Zodo choice does not change DSH chat selection or privacy. Host invocation credentials remain in memory and must never be copied to model configuration, browser responses, logs or release assets.
- Same-UID Host/Shell is not a strong OS sandbox. Use another OS identity or remote Core when stronger isolation is needed.

## Verification not covered by a fixture

Real DSH login, installed-plugin discovery, Electron forwarding, Host reload/unload, real model-provider success and OS notification permissions/delivery require separate authorized environment tests. Historical observations remain in the [evidence index](<../implementation/EVIDENCE_INDEX.md>) but must not be reported as fresh passes.
