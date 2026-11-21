# Plan approval replay safety

A workspace revision does not change just because the clock advances. Replacement planning
previously recomputed removable allocations at commit: an unlocked placement crossing its
start time could cease to be removable, silently changing an approved deletion into a no-op.
A planned future assignment could also become a historical insertion via the manual allocator.
Both effects are demonstrated with fixed-clock, same-revision command tests.

## New operation binding

`plan.propose` now includes `expected_removed_allocation_ids` in its serialized `plan.apply`
operation. The array is the exact sorted removal set at preview time, including an explicit
empty array for additive/no-removal plans. It is included in the existing proposal content hash.

The ordinary eligibility rule is unchanged: replacement scope, Planned state, unlocked, and
start >= command now. Before any plan deletion/insertion, Core requires that this rule still
produces the exact bound IDs. It also rejects assignments whose start < command now. At the
exact start instant the previous eligibility boundary remains unchanged; once it has passed,
a refreshed plan is required. This is deliberately scoped to plan application, not manual
historical allocation writes.

Plans are replayed on a cloned world before approval and through the normal cloned transaction
at commit. A changed effect returns `APPROVAL_REQUIRED` with a message to **regenerate the plan
and approve the new preview**. No allocation/task facts are changed, approval grants are not
consumed, and no success receipt is recorded. Existing revision, policy, expiry, principal and
hash/grant checks still apply. New timestamps or identities are not silently substituted into
already approved operation payloads.

## Serialized old proposals/grants

The added field is optional only for reading old stored operations and omitted when absent
on serialization. Consequently, reading an old operation does not inject fields into its
serialized content or rewrite its approval hash. No SQLite/entity migration or bulk mutation
of grants/proposals is performed.

Any legacy `plan.apply` lacking this binding requires regeneration before validation, approval
or commit, even with an unexpired matching grant. Core cannot determine whether an old task
replacement scope came from the old additive-plan bug or intentional replacement; guessing
would risk deleting retained work. The conservative rule covers all unbound legacy plans,
including old no-op/replacement plans. Old unrelated operation types remain usable. Previously
successful command receipts retain the ordinary idempotent replay path and do not rerun effects.

A UI should offer refresh/re-propose, not rewrite the stored operation, inject IDs into an old
grant, retry with permissive manual create, or automatically approve replacement content.
Plan immediately-starting work with sufficient future lead time for human review; a timestamp
that passes while approval is pending must be selected again.

## Evidence

- `crates/zodo-runtime/tests/plan_replay.rs`: clock-only deletion drift, newly past assignments,
  approval-time rejection, before/exact-boundary controls, old serialized matching-grant SQLite
  reload, unchanged facts/grants/hashes, no failed success receipt, regeneration, manual-history
  compatibility.
- `crates/zodo-kernel/tests/plan_replay_contract.rs`: legacy serialization and hash coverage of
  the bound deletion set.

This is not a generic semantic comparison of every proposal type or a reinterpretation of old
execution records. Other time-dependent operations retain their existing validation contracts.

Deployment limit: these checks require the upgraded Core. An older binary may ignore an unknown
payload field, so pending bound plans/grants must not be carried into a Core downgrade. This
change does not establish cross-version downgrade enforcement or claim old binaries gained the
new checks.
