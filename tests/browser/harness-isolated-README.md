# Isolated Harness regression

This fixture runs a fresh Core child in an OS temporary directory, the unmodified plugin Host bridge, and the unmodified React client with pinned React 18.3.1. Only the Host service-registration/model-catalog container is a test double. It never starts a DSH server, reads a DSH profile, accepts a live URL, or reads archived runtime data. Core receives a minimal environment, isolated home/temp directories, a local-only rejecting model endpoint (127.0.0.1:9), and initially no cloud model or credentials. Settings cases may save a cloud connection draft/configuration, but never dispatch a model run and never provide real credentials. Browser automation uses the existing playwright-core installation.

## Run

```sh
pnpm --dir tests/browser/harness-fixture-runtime install --frozen-lockfile --ignore-scripts
node tests/browser/harness-isolated-regression.mjs
```

Requires a current `target/release/zodo.exe` (`cargo build --release --locked -p zodo-runtime`), Chrome, and the existing browser test dependencies. Optional `ZODO_FIXTURE_CORE_BIN` selects a binary, never a data directory; `ZODO_CHROME` selects Chrome. Tests create their own origin and data directory and always stop the Core/server/browser and delete data in `finally`. The bridge discovers the already-owned Core; its detached autostart fallback is deliberately disabled with an absent binary path, so a fixture crash cannot leave an untracked replacement Core. Per-run screenshots and the sanitized JSON report are under `harness-fixture-runtime/results/<timestamp>/`, ignored by Git. Results include cleanup confirmation.

For an explicitly managed manual fixture, `node tests/browser/harness-isolated-fixture.mjs --serve` prints its unique loopback URL. POST `/__fixture/shutdown` stops the owned server and Core and removes its data. It also has a 30-minute maximum lifetime. The optional `?fixture_view=models` query navigates the throwaway client to that page for visual checks without writing any settings. Never use it as a replacement for the current DSH GUI. In DSH run_code, importing `startFixture()` and keeping the program alive through browser inspection before `finally { await fixture.stop() }` is preferable. The run_code process starts with an empty environment, so provide a valid SystemRoot and TEMP/TMP before startup.

## Coverage and limits

28 cases cover same-origin/cookie gates, credential non-exposure, empty Core boot, all 21 navigation pages, capture/manual clarification/conversion to task+goal+project, task validation/create/edit/complete/reopen/archive/restore, explicit goal/project editors, habit create/log, saved-view create/apply, one-run automation budget exhaustion, focus start/pause/resume/finish without completing the task, JSON/CSV downloads and SQLite backup hashes, capacity and preview-before-approval plan commit, search, model draft filling without save, catalog failure/retry, responsive navigation/dialogs, offline draft reload/recovery, JavaScript errors, and unexpected external browser requests. Responsive coverage clicks all 21 navigation entries at 390px and 800px viewports, and a 600px workbench container inside a 1440px viewport, checking active state, headings and absence of document/nav horizontal overflow. Navigation tests confirm mounting/active state/headings, not every control on each page. UI uses archive/restore rather than permanent task deletion.


### Dual-mode (local_only / cloud_assisted) settings surface

Eight cases drive the shared ModelsView that both the 模型 and 隐私 navigation entries render, through the real bridge and a fresh fixture Core (`cloud: null`, local endpoint 127.0.0.1:9, no credentials, no dispatch):

- Defaults: radios `ai-mode-local_only` / `ai-mode-cloud_assisted`, `ai-effective-status` rows from Core's `GET /model` `effective` projection (`connection_tested` false), `ai-saved-mode`, advanced `ai-advanced` collapsed by default, the old data-level dropdown absent, the host catalog and endpoint/env/role/diagnostics fields only inside advanced.
- Consent flow: switching to cloud_assisted and `ai-add-cloud` send no commands; a dirty draft blocks the mode save client-side; `model-save` persists the connection without touching policy (`workspace.update` is the only mode command); the first authorizing save needs the `ai-cloud-consent` checkbox and writes exactly one explicit provider id into `allowed_cloud_providers`; the saved policy contains only current fields (no `private_notes`) and the `snapshot.notes` content levels stay unchanged (a fixture-created device_only note is verified before and after); the missing cloud key keeps `effective.workspace` on device with reason `cloud_credentials_missing`; no run is dispatched (the actual bridge `GET /runs` list stays empty).
- Failure: an injected 502 on `workspace.update` surfaces the error without any success notice and leaves policy unchanged; retrying saves 仅本地 while preserving the explicit allowlist.
- Persistence: saved mode and saved connection survive privacy/models navigation and a full reload.
- Removed surfaces stay rejected: fixture-only `workspace.update` submissions (through the bridge, never a real profile) carrying the old `private_notes` field or the old `cloud_allowed` model_route are rejected by Core as unknown field / unknown variant, leave the saved policy and `policy_revision` untouched, and the surface keeps showing 仅本地. These strings appear only as rejected negative submissions; no compatibility or migration path is exercised.
- Cross-window connections: an outside save through the real `POST /model` followed by a clean `ai-settings-refresh` syncs the draft (a later edit cannot roll the outside change back); while a draft is dirty, an outside update makes `model-save` re-check the baseline via GET, refuse to POST, keep the draft behind `ai-connection-stale`, and `ai-discard-connection` reloads the server state.
- Advanced: keyboard Enter toggles the details element; the 诊断与授权信息 section shows the diagnostics switch plus only the explicitly authorized provider list; at 390px the open advanced section causes no horizontal document overflow.


**Not covered:** real DSH authentication/login, Host routing and sidebar composition, real Host privacy session projection persistence, native desktop shell, real model/cloud dispatch. Host projection APIs and model directory are test doubles. Do not describe these tests as a full-Host end-to-end proof. Browser-lane visual review is subject to shared desktop availability; the lead may review these isolated Playwright screenshot artifacts instead. Neither method covers real Host authentication/routes.
