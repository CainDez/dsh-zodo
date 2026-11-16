# Supported UI and verification boundaries

Updated 2026-11-07. Only the [DeepSeek Harness plugin](<../adapters/deepseek-harness/plugin/README.md>) UI is supported. Standalone Flutter, mobile, Web and native-client implementation/build instructions have been removed; these are retired scope, not platform blockers to reopen. See the [scope decision](<../implementation/SCOPE_DECISION.md>).

Current verification boundaries:

- Packaging is Windows x64; other Core targets and signing/installers require their own environment.
- The isolated React fixture substitutes Host registration/model services. Login, Electron forwarding, installed-plugin discovery and actual Host lifecycle still need authorized Host checks.
- DSH hot reload is constrained by Host capabilities; a Core-only or fixture test does not prove it.
- OS notifications and real remote model credentials are excluded from safe deterministic regression.

Historical platform measurements remain in the [evidence index](<../implementation/EVIDENCE_INDEX.md>) for traceability, not current product requirements.
