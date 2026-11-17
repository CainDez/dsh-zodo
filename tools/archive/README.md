# Historical integration scripts — do not run

These 15 integration scripts and three installation-specific ASAR helpers are retained only to explain earlier evidence. They are unmaintained, excluded from current regression, and are not compatibility requirements. They may read frozen or live workspaces, inherit credentials, call real models, alter authorization, or deliver real OS notifications. Moving them here does not make them safe; do not run or import them. Existing evidence is preserved unchanged.

The maintained entry point is [the isolated runner](<../run_isolated_integration.py>), with its reviewed-source allowlist, source hashes, fresh owned data, ephemeral ports, dummy credentials, and process/network guards. Its eleven admitted sources remain in the parent tools directory; those source files must not be run directly either. Current safety checks are in [test_runner_safety.py](<../../tests/integration/test_runner_safety.py>).

The runner reports these archive-relative paths with their original exclusion reasons. Any future replacement must be a new deterministic fixture, not a revival of old profile paths or a request to preserve obsolete product schemas.
