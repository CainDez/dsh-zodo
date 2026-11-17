# Release build — Core + DSH plugin only

Updated 2026-11-17. Development builds support only the current configuration/schema; deprecated modes, fields, right-sidebar UI and A0 probe are not shipped. The sole maintained UI is the [DSH plugin](<../adapters/deepseek-harness/plugin/README.md>). Standalone UI clients are retired, not pending platform work; see the [scope decision](<../implementation/SCOPE_DECISION.md>). Historical release evidence is not a current acceptance certificate.

## Build

Windows x64, Rust/Cargo and PowerShell are required. From the repository root:

```powershell
powershell -NoProfile -File scripts/build-release.ps1
```

The [build script](<../scripts/build-release.ps1>) builds the workspace with its lockfile and replaces only generated release output. It stages the current plugin, including its privacy module and Core binary. The deprecated probe crate/binary and old right-sidebar registration have been removed. It does not install, reload or modify a running Host.

Generated output consists of the Core executable, non-UI native ABI library, plugin ZIP named from its package version, build metadata (including dirty-tree status), and SHA-256 checksums. No standalone UI bundle is built or distributed. Run the [regression runner](<../scripts/run-regression.ps1>) before packaging.

## Verification limits

- Unit/API/isolated React fixture checks do not prove full installed-Host integration.
- Real DSH authentication, desktop forwarding, hot lifecycle and model routing need an authorized installed-Host test.
- Windows x64 is the packaging target here; signing/installers and other Core platforms require separate verification.
- Full keyboard/screen-reader/dialog/error-state coverage is not implied by navigation smoke checks.
- Real cloud models and OS notification permission/delivery are not exercised by deterministic stubs.

## Data safety

Do not ship runtime descriptors, Host launch URLs, cookies, keys, test databases or caches. Same-UID Host/Shell is not a strong sandbox; use an independent identity or remote Core when isolation is required. Plaintext export/backup remains sensitive even when encrypted sync is available.
