# Exact additive scheduling contract

`allocation.schedule` is a **proposal ChangeOp**, not a standalone command. It is intended for
an explicit user-selected future slot. Do not route this UI through permissive manual/history
`allocation.create`, and do not reinterpret existing create/update/undo semantics.

## Wire path

Send `proposal.create` (normal command envelope, current workspace/base revision) with:

```json
{
  "intent": "Schedule this task in the selected slot",
  "operations": [{
    "operation": "allocation.schedule",
    "payload": {
      "allocation": {
        "task_id": "tsk_example",
        "expected_task_version": 1,
        "capacity_block_id": "cap_example",
        "start": "2026-11-18T02:00:00Z",
        "end": "2026-11-18T02:30:00Z"
      }
    }
  }]
}
```

These timestamps are illustrative, not a reusable future default. The expected version must be
read from the task, not guessed. The UI should use exactly one operation for its quick-slot flow.

Inspect a validated proposal and its authoritative preview. Only after the user confirms that
preview, send `proposal.approve`, then `changes.commit` with the returned grant. Existing
principal, revision, policy, content-hash, expiry and receipt/idempotency rules still apply.
Opening/canceling the form must not send a mutation. Creating/rejecting a proposal may persist
proposal/audit metadata but never allocates time until commit. On a stale or invalid proposal,
refresh and re-propose; do not silently change dates or use the manual fallback.

## Strict checks, repeated at preview and commit

- Required existing task/capacity IDs and positive `expected_task_version`; stale versions fail.
- Both instants use whole-minute boundaries; positive duration is limited to 1–1440 minutes.
- Start must be strictly later than the command clock, including at commit.
- Task must be open, non-archived, a leaf and not manually blocked, with sufficient remaining work.
- Interval respects `not_before` and hard deadline, including exact boundary equality.
- Each predecessor is completed/cancelled, or has its full remaining work reserved in outstanding
  allocations whose latest finish is no later than this start. Partial or expired reservations
  are not evidence of completion. Archived/manually blocked open predecessors are rejected.
- Entire interval belongs to the selected existing capacity and fits its remaining budget.
- No overlap with allocations (including locked/started), fixed commitments or imported busy time.
- Valid workspace timezone; entire interval allowed by hard work windows and weekend-deny policy.
  Soft windows remain preferences. Weekend-ask is satisfied by this explicit approval flow.

Minutes are derived. Caller-supplied allocation identity, minutes, locked flag, or override fields
are rejected. The new row is unlocked and Planned. The operation is additive: it never moves,
removes, locks or rewrites another allocation, creates capacity, records progress, starts focus,
changes task facts, or calls an external calendar/provider. Any failed batch or commit leaves
allocation facts untouched through the existing cloned-world transaction path.

## Compatibility and limits

No SQLite/entity migration; stored allocation shape is unchanged. Old manual/history create and
update APIs retain their previous behavior. Old Core builds reject the unknown new operation,
so UI integration must verify support rather than fall back to `allocation.create`.

This does not implement daily-limit enforcement, optimal planning, provider freshness guarantees,
a new allocation execution lifecycle, or recurrence expansion. Imported busy data can be stale;
a valid slot is not a real-time external-calendar availability guarantee.

The Core takes unambiguous instants. The UI must convert the selected display date in the
workspace timezone, explicitly handle missing/repeated DST local times, show the actual task,
interval and capacity in the preview, and never infer a date from the computer timezone.

## Tests

- `crates/zodo-kernel/tests/strict_schedule_contract.rs`: shape, round-trip, effects and bounds.
- `crates/zodo-runtime/tests/strict_schedule.rs`: real SQLite command/proposal/approval/commit,
  version/time/dependency guards, windows, conflicts, atomicity and preserved manual semantics.

## Move an existing allocation

`allocation.move` is also proposal-only. Its flat payload contains `id`,
`expected_task_version`, `capacity_block_id`, `start`, and `end`. It moves one
unlocked planned allocation that has not reached its start time, to an existing
capacity block and future whole-minute interval. It preserves the allocation
ID, task, duration, and source. An unchanged destination is rejected.

Core checks task constraints, dependencies, workspace availability, capacity
and overlaps again at commit; it excludes the original slot from destination
accounting. If the move would make a retained successor interval invalid, Core
rejects the move without changing either allocation. Preview and approval do
not change the schedule. The preview shows a single changed allocation, not a
deletion and recreation. The DSH calendar move control is not implemented yet.
