# Frozen AI evaluation set

`cases.json` is a small, versioned regression set for model-output validation. It is derived from the
recorded A3 local-model run (`artifacts/evidence/a3/a3-model-run.json`) plus two adversarial cases that
must be rejected. It does not call a model, so it is deterministic and safe in CI.

`tools/run_ai_eval.py` validates:

- task array is 1..12 for normal output;
- each title is non-empty (<=200 chars) and unique when required;
- estimates 15..240 minutes, priorities 0..4;
- target_due is null or `YYYY-MM-DD`;
- assumptions/questions arrays are bounded.

The recorded output is only a regression fixture, not proof that a live model is good. A live-model
scoring run and broader cases (dependencies, capacity, ambiguous goals) remain open (ledger AI-13).
