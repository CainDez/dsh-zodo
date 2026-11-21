# Release build — Core + DSH plugin only

Updated 2026-11-17. Development builds support only the current configuration/schema; deprecated modes, fields, right-sidebar UI and A0 probe are not shipped. The sole maintained UI is the [DSH plugin](<../adapters/deepseek-harness/plugin/README.md>). Standalone UI clients are retired, not pending platform work; see the [scope decision](<../implementation/SCOPE_DECISION.md>). Historical release evidence is not a current acceptance certificate.

## Windows build

Windows x64, the Rust `x86_64-pc-windows-msvc` target and its MSVC linker, Cargo and PowerShell are required. From the repository root:

```powershell
powershell -NoProfile -File scripts/build-release.ps1
```

The [build script](<../scripts/build-release.ps1>) builds the workspace with its lockfile and replaces only generated release output. It stages the current plugin, including its privacy module and Core binary. The deprecated probe crate/binary and old right-sidebar registration have been removed. It does not install, reload or modify a running Host.

Generated output consists of the Core executable, non-UI native ABI library, plugin ZIP named from its package version, build metadata (including dirty-tree status), and SHA-256 checksums. No standalone UI bundle is built or distributed. Run the [regression runner](<../scripts/run-regression.ps1>) before packaging.

## Linux x64 development bundle

On native Linux x86_64, install Python 3 (standard library only), Cargo and the GNU x86_64 Rust target/linker. Use a new output directory with an existing parent:

```sh
python3 scripts/build-linux-plugin.py artifacts/linux-x64
python3 scripts/verify-plugin-package.py --target linux-x64 \
  artifacts/linux-x64/zodo-harness-plugin-linux-x64.zip artifacts/linux-x64/zodo
```

The script builds the locked Rust Core in a new temporary Cargo target directory. It writes one platform-specific plugin ZIP, the matching Core executable, and metadata with the source revision, dirty-tree flag and SHA-256 values. It does not install or publish anything. Unpack with a tool that preserves the Linux executable bit, then check `test -x bin/linux-x64/zodo-core` in the unpacked package. A build or fixture success does not establish official DSH Desktop availability or acceptance. For all tests keep `DSH_HOME` and `ZODO_DATA_DIR` scoped to isolated test directories and do not sign in or use real data.

## Verify a locally built plugin package

From the same checkout, use Python 3 (standard library only). Substitute the ZIP filename printed by the build.

```text
python scripts/verify-plugin-package.py artifacts/release/zodo-harness-plugin-<version>.zip artifacts/release/zodo.exe
python -B -m unittest discover -s tests/packaging
```

The validator checks archive members and exact source/Core bytes, and checks the Core executable header and target architecture (PE for Windows, ELF and ZIP executable mode for Linux). Run it against a package and executable from the same build; it does not establish provenance, signing, execution, installability or Host compatibility. The validator's unit tests use temporary synthetic archives, not native builds.

## Verification limits

- Unit/API/isolated React fixture checks do not prove full installed-Host integration.
- Real DSH authentication, desktop forwarding, hot lifecycle and model routing need an authorized installed-Host test.
- Windows x64 release assembly and Linux x64 development assembly are separate; signing/installers, installed Host behavior and other Core targets require separate verification.
- Full keyboard/screen-reader/dialog/error-state coverage is not implied by navigation smoke checks.
- Real cloud models and OS notification permission/delivery are not exercised by deterministic stubs.

## Data safety

Do not ship runtime descriptors, Host launch URLs, cookies, keys, test databases or caches. Same-UID Host/Shell is not a strong sandbox; use an independent identity or remote Core when isolation is required. Plaintext export/backup remains sensitive even when encrypted sync is available.
