# Synthetic whole-week Core journey

Baseline: `824e056806bb1ddd5cfd48bc40aca263213ac8e0`.
Test: `crates/zodo-runtime/tests/week_journey.rs`.
No production code changes, real data, accounts, models, providers, or desktop actions.

## Why one compound test

The existing `a4_p1::multi_week_planning_spans_capacity_blocks_across_periods` checks one
multi-period preview, not approval/commit or storage reopen. `retained_plan` and `plan_replay`
cover isolated additive/replacement and clock/grant cases. This new single regression does
not duplicate those negative matrices; it checks their composition over one persisted week.

## Exercised sequence

1. Build a synthetic Monday–Sunday Asia/Shanghai week with five daytime capacity blocks,
   one night block, fixed meetings, imported busy, and a locked selected-task allocation
   spanning local midnight. All inputs are explicit Core commands, no model use.
2. Preview, approve and commit the initial 420-minute task, genuinely spread over at least
   four local dates. The 30-minute locked allocation is included, not duplicated.
3. Add a 150-minute task and run the UI-default additive intent (`move_unlocked:false`
   explicitly supplied). Existing allocation identities and complete contents survive.
4. Approve an explicit replan, then introduce a fixed appointment which genuinely overlaps
   existing movable work. The now-stale grant fails with `RevisionConflict`; allocations,
   commitments, workspace revision and grant state are unchanged by the rejected commit.
5. Close the actual SQLite connection and reopen the same isolated database. Verify the
   complete serialized World, including pending grant/conflict, is preserved.
6. Generate a fresh explicit replan, verify its bound deletion IDs exactly match movable
   allocations, approve and commit. The new appointment remains, all conflicts are resolved,
   locked/fixed/imported/capacity facts remain unchanged, and all 570 task minutes are placed.
7. Query adjacent local days: the same locked cross-midnight allocation is discoverable on
   both sides, and the overnight fixed appointment remains visible on its following day.
8. Close/reopen SQLite again; complete World and the queried schedule are unchanged.

At stable stages, assertions enforce global non-overlap, commitment/busy exclusion, capacity
membership/budgets, exact task totals, and zero fabricated activity/focus/progress facts.

## Verification

```sh
cargo test -p zodo-runtime --test week_journey --locked
cargo test --workspace --locked
```

Focused journey: **1/1 passed**. Workspace: **160/160 passed** (159 prior + this one test).
Touched-file rustfmt and `git diff --check` pass. The existing Linux-only unused import warning
in `notify.rs` is unrelated and unchanged.

## Acceptance still missing

This is real Rust/SQLite command execution, not a browser, HTTP, running Core-process restart,
or DSH Desktop lifecycle test. It does not prove that a person can complete the journey in the
actual UI, keyboard/narrow layout, lost-response recovery, plugin installation/upgrade/uninstall,
Windows binary/profile isolation, or Host-to-Core rediscovery after restart. It uses synthetic
busy input, not provider freshness/connectivity. DST and no-busy UI matrices remain in their
existing separate suites; this one week does not substitute for them.

The roadmap's whole-week **Core composition** has one durable regression. It does not
substitute for the whole-week journey in official DSH Web. Native Windows Desktop and
cross-version compatibility are deferred for this Linux-first stage.
