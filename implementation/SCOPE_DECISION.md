# Scope decision (2026-11-07)

The user reaffirmed that **Rust Core + DeepSeek Harness plugin (Host workbench)** is the delivery
scope. DSH is the only retained UI. Flutter and other non-DSH UI adapters are retired/out_of_scope;
this is a scope removal, not completion through testing.

## Active accounting

- The active ledger has 170 rows and 14 columns; the obsolete `flutter_entry` column is removed.
- FLT-01..FLT-06 remain as six `out_of_scope` traceability rows. Their old observations and evidence
  are historical only; no current build, platform, accessibility or regression pass is claimed.
- The other 164 DSH/Core rows retain their existing `verified` labels. These are historical ledger
  assertions, not results of the current isolated regression, which must be reported separately.
- Shared screen/dialog implementation entries now name the DSH plugin client only. DAT-05 refers to
  DSH browser draft behavior and depends on HARN-H-09, not the retired FLT-06 client lifecycle.
- FND-16 remains a Rust Core FFI/backend entry, not an active standalone UI or platform route.
- Do not count the six excluded rows as verified/partial/not_started delivery work or revive their
  Windows/Web/mobile/macOS/Linux acceptance requirements.

## Preservation and safe continuation

- Leave the read-only design handoff, historical evidence index and old artifacts intact.
- Preserve the 2026-11-07 DSH model-catalog, migration and desktop bridge observations in the progress
  record; archived service endpoints and revisions are not a current inventory or startup guide.
- Never relaunch a frozen pre-migration workspace for testing. Use new disposable Core/Host fixtures;
  do not operate the user's GUI, production data or canonical Core workspace for regression.
- HARN-H-18/H19 guard evidence remains recorded historically: restricted non-local model history and
  restricted inherited forks are refused by the Host adapter. Revalidation is a separate test result.
- Complete UI-adapter cleanup before continuing Core/DSH-only regression; report accurate counts,
  failures and coverage limits rather than inferring success from the ledger.
