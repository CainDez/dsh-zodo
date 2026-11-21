# Isolated Harness regression

This fixture runs a fresh Core child in an OS temporary directory, the unmodified plugin Host bridge, and the unmodified React client with pinned React 18.3.1. Host registration, the model catalog and the LLM service are test doubles. It never starts a DSH server, reads a DSH profile, accepts a live URL, or reads archived runtime data. Core receives a minimal environment and isolated home/temp directories. It starts with an explicit **unselected DSH source**, not a local/cloud route and not the current chat model. Custom settings use a rejecting loopback endpoint (127.0.0.1:9) and no real key. Settings cases never dispatch a model run. Browser automation uses the existing playwright-core installation.

## Run (integration owner)

```sh
pnpm --dir tests/browser/harness-fixture-runtime install --frozen-lockfile --ignore-scripts
node tests/browser/harness-isolated-regression.mjs
```

Requires a newly integrated release Core, Chrome, and the existing browser test dependencies. Optional `ZODO_FIXTURE_CORE_BIN` selects a binary, never a data directory; `ZODO_CHROME` selects Chrome. An older Core that only accepts v1 local/cloud configuration is not a valid test target. Run the suite only after the integrated release is ready. The fixture itself neither builds Core nor installs anything into a real profile.

Tests create their own origin and data directory and always stop the Core/server/browser and delete data in `finally`. The bridge discovers the already-owned Core; detached autostart is deliberately disabled with an absent binary path, so a fixture crash cannot leave an untracked replacement Core. The HTTP server is listening and `ctx.webServer.getOrigin()` returns its exact fixture URL before plugin registration. Plugin effects own their callback-channel cleanup. Per-run screenshots and a sanitized JSON report are written under the ignored runtime results directory. Results record release Core/bridge/client hashes, every case outcome, navigation checks and cleanup confirmation.

For an explicitly managed manual fixture, `node tests/browser/harness-isolated-fixture.mjs --serve` prints its unique loopback URL. POST `/__fixture/shutdown` stops the owned server and Core and removes its data. It also has a 30-minute maximum lifetime. The optional `?fixture_view=models` query navigates the throwaway client to that page without writing settings. Never use it as a replacement for the current DSH GUI. In DSH run_code, import `startFixture()` and keep the process alive through browser inspection before `finally { await fixture.stop() }`; that process starts with an empty environment, so provide valid SystemRoot and TEMP/TMP values before startup.

## Supported runner and reproducible evidence

Use a disposable development/CI machine with working local TCP and Unix sockets,
an already provisioned supported Chrome/Chromium, Node, pnpm, and Rust. The normal
Playwright browser launch must work under that runner's existing security policy.
On Linux the fixture explicitly requests Playwright's Chromium sandbox; it does
not fall back to a weaker launch. The branch-scoped [Linux x64 workflow](../../.github/workflows/zodo-linux-x64-browser.yml)
uses the hosted `ubuntu-22.04` image's existing Chrome, builds the exact triggering
revision, and verifies the same-build Core. It publishes no screenshots, raw logs
or artifacts. A passing run covers the React/bridge/Core fixture only, never
official DSH Desktop or account-backed behavior.

A Chromium `socket() failed: Operation not permitted`, `ptrace: Operation not
permitted`, or `No usable sandbox` startup error is an **environment blocker**,
not a failed UI assertion and not a successful test. Do not disable host syscall
restrictions, change security settings, add browser-security bypass flags, attach
to an authenticated browser profile, or use a real DSH installation to work around
it. Move the unchanged isolated fixture to a properly provisioned runner instead.
A connected computer or a working web browser alone is not proof it supports this
local Core/Playwright fixture. Cloud-browser UI tools cannot substitute for the
suite unless they can actually reach its owned loopback origin and run its checks.

Example Linux run from the repository root, after checking out the exact commit
being validated (no live Host or calendar is required):

```sh
# Use the checked-in lockfiles for BOTH dependency trees.
pnpm --dir tests/browser install --frozen-lockfile --ignore-scripts
pnpm --dir tests/browser/harness-fixture-runtime install --frozen-lockfile --ignore-scripts
# Keep this target separate from concurrent builds/checks.
export CARGO_TARGET_DIR="$(mktemp -d /tmp/zodo-fixture-target.XXXXXX)"
cargo build --locked --release --bin zodo
export ZODO_FIXTURE_CORE_BIN="$CARGO_TARGET_DIR/release/zodo"
export ZODO_CHROME="$(command -v google-chrome || command -v chromium)"
test -x "$ZODO_CHROME" && test -x "$ZODO_FIXTURE_CORE_BIN"
node --test tests/browser/harness-isolated-diagnostics.test.mjs
node tests/browser/harness-isolated-regression.mjs
```

Record `git rev-parse HEAD`, `git status --short`, the runner OS, and the resulting
`report.json` alongside screenshots. The report includes Node/browser versions
(when launch succeeds), timezone, Core/client/bridge hashes, executed cases and
cleanup confirmation. The supplied Core must be built from the same checkout;
a binary hash alone does not prove source equivalence. Do not share raw stderr
without review, because browser launcher output can include local paths.

Report statuses are `passed`, `failed` (assertions), `blocked` (recognized browser
prerequisites/security limitations), `error` (other fatal execution failures), or
`not-run`. A fatal startup reports a sanitized `fatal.stage`, `fatal.code` and
next-step guidance even when `executed` is zero. Zero failed assertions alone is
never a passing result. Case failures remain in `cases`; unknown startup errors
are not silently labeled security limitations. Failures produce a nonzero exit.
If dependency imports fail before the runner starts, Node stderr is the evidence;
no report is guaranteed for that earlier failure.

Agenda coverage additionally checks previous/next/today date windows in
Asia/Shanghai, row visibility and keyboard/click drilldown, no command RPC during
navigation, and unchanged Core revision/data. Pure projection tests cover DST,
midnight endings and offset chronology separately; neither substitutes for a
successful rendered browser run.

## Coverage and limits

**21 navigation entries; the report records the executed case count.** The original non-settings coverage remains: same-origin/cookie gates, credential non-exposure, empty Core boot, navigation mounting, capture/manual clarification/conversion to task+goal+project, task validation/create/edit/complete/reopen/archive/restore, explicit goal/project editors, habit create/log, saved-view create/apply, one-run automation budget exhaustion, focus start/pause/resume/finish without completing the task, JSON/CSV downloads and SQLite backup hashes, capacity and preview-before-approval plan commit, search, responsive navigation/dialogs, offline draft reload/recovery, JavaScript errors and unexpected external browser requests. Archive/restore is used rather than permanent task deletion.

Responsive coverage clicks all 21 entries at 390px and 800px viewports and a 600px workbench container inside a 1440px viewport. It checks active state, headings and document/nav horizontal overflow. These are navigation/action checks, not exhaustive proofs of every control on every page.

### Independent model-source and privacy settings

The two original catalog cases are updated for source selection. Eight former dual-mode cases are replaced:

1. **Catalog selection is draft-only.** A mocked subscription provider/model is supported and selectable through the DSH dropdown without copying an HTTP endpoint or requesting a key. The catalog has schema `zodo.harness-model-catalog/1`, `groups` with supported provider/model rows, and no `default_route`. Discard restores the unselected saved source.
2. **Catalog failure/retry.** An injected 503 shows a visible error, retains saved model configuration and recovers using the refresh control.
3. **Distinct pages/default deny.** Models and Privacy retain their navigation IDs but have separate headings “模型” and “隐私” and independent editors. DSH/custom are the only model-source radios. The initial DSH selection is null/null, does not read the chat default, and effective status is preflight with `connection_tested: false`. Privacy initially denies goals, projects, captures and diagnostic capture.
4. **Model-only saves.** Saving the subscription DSH selection, then a custom loopback config with an optional null environment-variable name, changes neither privacy/policy revision nor note content state. Editing fields without saving remains draft-only; settings do not dispatch a run.
5. **Privacy-only saves/failure.** Goals/projects/captures are independent booleans, not a global model-routing mode. A failed policy save changes nothing and displays no success; retry persists the exact privacy policy without replacing model config. Diagnostics stays inside a separate advanced section.
6. **Independent persistence.** Model source and privacy survive switching between pages and reloading the client.
7. **Strict rejection.** Fixture-only negative submissions reject v1 model config, editable wrappers, local/cloud/roles fields, a raw key, old global model-route/allowlist/private-notes policies and unknown context fields. Rejections change neither saved config nor policy revision. Old strings occur only in deliberate rejection payloads; no compatibility/migration path is exercised.
8. **Clean refresh.** An outside model save is adopted before a later local edit, preserving another changed field rather than rolling it back.
9. **Dirty conflict.** Save rechecks the server baseline by GET, refuses to POST a stale draft, preserves the local draft, disables save, and discard reloads the authoritative config.
10. **Independent advanced UI.** Model advanced details toggle via keyboard; the environment-variable name is not a password input. Open advanced sections on both settings pages stay within a 390px viewport.

The model API GET/POST result is schema `zodo.model/1`; `config` is the direct value, never `config.editable`:

```json
{"version":2,"source":{"kind":"dsh","provider":null,"model":null}}
```

A selected DSH source fills provider/model. Custom config is `{version:2,source:{kind:"custom",base_url,model,api_key_env:null|ENV_NAME}}`. Effective state contains `status: "preflight"`, `connection_tested: false`, `selected: {source,provider,model,available,reason}` and operation availability/reasons for `decompose` and `clarify`; preflight availability never claims a successful inference.

PrivacyPolicy is strictly:

```json
{"model_access":{"goals":false,"projects":false,"captures":false},"diagnostic_capture":false}
```

### Host LLM mock boundary

The fixture exposes local-only deterministic `ctx.llm.stream(options)` as an async iterable, in addition to provider/model discovery. It emits real-service-shaped `text-delta` and `finish` chunks, accepts an AbortSignal, and records the complete options object in in-memory `fixture.llmCalls.streams` for Host callback-chain tests. It does **not** implement its own callback HTTP protocol or replace the production callback transport. The Core/Host callback integration tests own dispatch coverage; this browser suite asserts no streams during all settings/navigation work. The mock subscription route deliberately exposes no reusable endpoint/key and the chat-default accessor throws and is counted; the browser suite asserts it was never read. No paid provider or actual subscription request is made.

**Not covered:** real DSH authentication/login, Host routing/sidebar composition, real Host privacy-session projection persistence, native desktop shell, paid/subscription inference, or non-mocked provider network behavior. Host projection/catalog/LLM services are doubles. Do not describe this as full-Host end-to-end proof. Isolated screenshots support UI review but do not establish real Host authentication or routing correctness.
