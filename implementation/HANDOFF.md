# Handoff

No prior chat history is needed to resume. Updated 2026-11-07 (Asia/Shanghai).

## Current delivery scope

Rust Core + DeepSeek Harness plugin only; **DSH is the only retained UI**. Flutter and all other
non-DSH UI adapters are retired/out_of_scope, not successful test completions. See
[SCOPE_DECISION.md](SCOPE_DECISION.md).

The active ledger retains 164 DSH/Core rows with existing historical `verified` labels and six
FLT-01..FLT-06 rows marked `out_of_scope` (170 total). The obsolete Flutter entry column and shared
Flutter route references are removed. DAT-05 describes DSH browser drafts; FND-16 is a Core backend
capability, not a standalone UI. These ledger counts are **not this regression's pass counts**.
Current Core/bridge/UI results must be measured and reported independently.

## Working directory and protected records

`D:\projects\zodo_dsh`

- `design/v1.3/` is read-only. Original archive `design/v1.3-ORIGINAL.zip` SHA-256:
  `45E45CF0F21141572A987912E6FF5B3C1B3D3A616BE56A9ABC77A918D36454A5`.
- Preserve the historical evidence index and artifacts, including A5/Flutter build and screenshot
  records. They are traceability data, not current delivery, startup instructions or fresh acceptance.
- Preserve the user's 2026-11-07 DSH model-catalog, migration and desktop bridge notes in
  [PROGRESS.md](PROGRESS.md).

## Historical host/toolchain checkpoint

These are recorded versions, not claims about the currently running GUI or services:

- DeepSeek Harness desktop `0.2.1-rc.3`, source tag `dsh-v0.2.1-rc.3`, commit
  `1f81c047fc48fa190a683e469daa699857e9afe8` (MIT). Recorded installed app archive SHA-256:
  `04B32E3A872826BA15CFB2081C6A929A8D4ECAA65E09A2543E5A6B30E2EFF1F0`.
- Rust/Cargo 1.98.1; Node v25.6.0; Host Node 24.21.0 through `ELECTRON_RUN_AS_NODE=1`.
- pnpm 11.28.5 and MCP SDK 1.20.2 were recorded for DSH testing. Re-observe local tools before use.
- Flutter tooling is retired from the active build/test route; do not restart client platform work.

## Safe build and regression continuation

1. Finish non-DSH UI adapter cleanup first; only Core and the DSH plugin belong in active releases.
2. Inspect existing changes and coordinate file scopes before editing. Do not overwrite the user's
   uncommitted DSH plugin/progress work.
3. Run Rust tests and bridge/UI regressions against newly created disposable fixture directories.
   Do not discover or connect to the canonical user Core for tests; avoid real model/network effects,
   notifications and clipboard access unless separately authorized and isolated.
4. Do not run an old Host launcher or point CLI commands at `.runtime/zodo-a2`. That directory is a
   frozen pre-migration archive; replaying it would fork history. Do not replace the user's GUI.
5. A release build is not permission to overwrite an active plugin binary, restart the user Host or
   mutate live workspace data. Coordinate any deployment separately with the Lead/user.
6. Report exact commands, passing/failing/skipped counts and platform/fixture limits. Historical
   `verified` entries and old release smoke/hash files do not prove current regression success.
7. Collect/stop all owned jobs, terminal sessions and fixture processes before handoff.

## Historical evidence

[Evidence index](EVIDENCE_INDEX.md) retains A0-A7 records and stable IDs; old A5 artifacts remain
historical only. Do not delete or rewrite those records as part of UI retirement. Existing Host
HARN-H-18/H19 privacy/fork guard evidence is preserved, but must not substitute for a new test run.

## Cautions

- Do not operate the user's active GUI/profile or canonical data for regression. Never commit tokens,
  runtime credentials or binaries.
- `local_only` must not fall back to cloud; Agent/MCP tools cannot approve/commit.
- Same-UID Host/Shell is not a strong OS sandbox.
- No Flutter parity, platform build, accessibility or full-product completion claim follows from
  removing an adapter. Scope retirement and test success are distinct outcomes.
