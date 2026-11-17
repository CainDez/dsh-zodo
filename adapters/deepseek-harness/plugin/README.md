# Zodo — DeepSeek Harness plugin

The maintained Zodo UI is a native DSH left-menu entry and main workbench. Core owns SQLite business facts, proposals, planner, and permissions. The Host bridge supplies authenticated browser/tool access and optional DSH model execution.

## Model selection and privacy are separate

- **Models:** select exactly one DSH provider/model, or a custom OpenAI-compatible endpoint/model. No local/cloud pair, role slots, current-chat-model inheritance or automatic cross-model fallback.
- **Privacy:** independently authorize goal content, optional project background, and captured text. All are denied by default. Model saves cannot grant permissions; privacy saves cannot change model choice.
- DSH models are invoked by the real Host llm service, including supported subscription/OAuth providers. Catalog entries are not converted into copied endpoints or environment variables. No provider credentials or chat history are copied to Core.
- A custom model calls its selected API endpoint directly. Localhost is a special endpoint configuration, not a separate model type. Optional API authentication references a Core environment variable name.

Details: [model and privacy settings](<../../../docs/MODEL_SETTINGS.md>).

## Package

The package includes the Host entry, browser UI, privacy guards, authenticated Host model gateway, locale/icon, manifest, bundle patch and a Windows Core binary. No retired A0 probe or old right-sidebar tab is shipped. Version and Host requirement are reported from the package manifest rather than hardcoded historical locks.

Build from the repository root with `powershell -NoProfile -File scripts/build-release.ps1`. The release script packages only the explicit file list; runtime directories, database, backups, invocation tokens and model credentials are not release assets.

Install the generated ZIP through DSH bundle management or use the repository development link. Loading new Host modules may require a full DSH restart when no development watcher is active. Rebuilding a fixture or starting another server does not update the actual desktop Host.

## Host model boundary

Core and Host establish an authenticated loopback invocation channel. Its credentials are memory-only; per-run authorization binds the Core instance, exact selected model, policy revision, model revision and context scope. Restarting the Host re-establishes this channel. The public catalog/config does not expose invocation credentials.

Only Core-constructed system/user context is sent to the selected provider. No sessionId, tools, DSH conversation history or current chat model is supplied. DSH destinations are not assumed device-local based on names or proxy addresses. Explicit DeviceOnly data requires a verified custom loopback endpoint. Content access gates apply even to local endpoints.

Agent tools can create candidates but cannot approve/commit or change model/privacy administration. Structured private export and restricted-history/fork dispatch guards remain mandatory. Same-UID application auth is not an OS sandbox.

## Verification

The isolated fixture loads the real plugin and a disposable Core with deterministic Host services; it does not exercise paid models or prove live desktop authentication. Browser settings tests verify that catalog reads, selections and saves never invoke inference. Separate Host/Core tests verify actual run → Host llm stub → proposal, denied scopes, subscription-shaped providers and callback authorization.

See [isolated browser regression](<../../../tests/browser/harness-isolated-README.md>) and [current test report](<../../../docs/TEST_REPORT.md>). Historical scripts and reports are archives, not current interfaces or compatibility promises.
