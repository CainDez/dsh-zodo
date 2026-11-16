# Isolated API integration regression

Run from the repository root with Python 3.9+:

```text
python -B -m unittest discover -s tests/integration -p test_runner_safety.py -v
python -B tools/run_isolated_integration.py
```

The [runner](<../../tools/run_isolated_integration.py>) accepts `--binary` for an already-built Core executable. It never builds or attaches to an existing Core. The default binary is Windows release; other platforms must select their executable explicitly. `--case` selects a reviewed legacy case, while `--api-only` selects the [fresh API suite](<../../tools/isolated_api_suite.py>); these flags are mutually exclusive.

## Safety design

- Every invocation creates a new unique fixture tree under the local ignored runs directory. In-memory AST transformation relocates ROOT, executable and fixed stub PORT; original legacy scripts remain unchanged.
- Eleven admitted legacy sources are SHA-256 pinned after full source review. Changed sources fail closed and require review before admission hashes are updated. This is a reviewed-source adapter, not a sandbox for arbitrary Python.
- Process creation admits only the selected Core, explicit data under this invocation, `serve --port 0`, or restore from the same fixture tree. Cleanup uses recorded owned handles only, never PID/name-wide kill. Successful cases explicitly verify all owned Core processes exited.
- Python HTTP admits only ports of verified owned Core instances (matching runtime PID) and registered owned stubs. Model and calendar stubs bind loopback port zero; no dependency on bridge 3181, model 11434, or fixed stub ports 39091/39162.
- Environment is allowlisted: no provider secrets, proxy, existing Core/model/calendar configuration or real user-profile paths. Only literal dummy credentials are admitted. Default local inference points to this invocation’s rejecting stub. AI11’s cloud-labelled route is loopback-only, not a paid provider.
- No live canonical Core or frozen archive access. Import, restore and sync use synthetic facts generated in this invocation.
- Real notification/retry/reconciliation cases are skipped because Core has no mock/disable Windows-toast setting. Starting empty Core prepares its own toast script, but these tests enqueue no reminders, so no toast is executed. Notification behavior is never patched to claim coverage.
- Raw evidence may contain only this invocation’s tokens and synthetic SQLite state; it remains local under the gitignored runs directory. Do not publish runtime/auth files or databases.

## Outputs

The runner prints JSON per scenario, returns nonzero on any failure and writes an aggregate JSON summary with binary/source hashes, timing and fifteen explicit skip reasons. Legacy stdout is retained per case; extra strong assertions prevent exit-zero observational scripts from falsely passing. [Safety-contract tests](<test_runner_safety.py>) cover admission, classification, environment, path/port/process/HTTP/restore guards without starting Core.

See [RESULTS.md](<RESULTS.md>) for independently recorded outcomes and limitations.
