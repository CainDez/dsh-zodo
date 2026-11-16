# Final isolated Harness regression

- Final run: 2026-11-07 22:52 Asia/Shanghai. Exit 0; **20 groups passed, 0 failed**.
- 21 desktop navigation pages mounted; 63 additional real navigation clicks passed across 390px, 800px and a 600px container in a 1440px viewport. Every responsive page asserted active navigation, nonempty heading and no document/nav horizontal overflow.
- 0 client JavaScript errors; 0 unexpected external browser origins. Core environment is allowlisted, home/temp are isolated, cloud is disabled and the model endpoint is 127.0.0.1:9. Bridge detached autostart is deliberately disabled: only the owned Core child may serve.
- Core/server/browser closed; fixture data removed. No live DSH GUI/profile/runtime was read or modified by these tests.

## Attribution

| Artifact | SHA-256 |
|---|---|
| Core release | 9702ccedaa0511b492bb5e69c8e380f2d12a7df3646e4043b568ab03a0e7af17 |
| Host bridge source | 2fbf2f290c6dd12ca63a28a1cb7095448398d412b7a4e2f6fe7bed4748ab61a3 |
| React client source | f80175b6e9d8b6fc7808e1eb835a0ffe08b9c6bba6f2b43ab2754e13cea12244 |

## Evidence

[Sanitized final report](<harness-fixture-runtime/results/2026-11-07T14-52-54-050Z/report.json>)

- [800px full navigation](<harness-fixture-runtime/results/2026-11-07T14-52-54-050Z/navigation-800.png>)
- [390px navigation](<harness-fixture-runtime/results/2026-11-07T14-52-54-050Z/navigation-390.png>)
- [600px container](<harness-fixture-runtime/results/2026-11-07T14-52-54-050Z/navigation-1440-container600.png>)
- [Narrow capture dialog](<harness-fixture-runtime/results/2026-11-07T14-52-54-050Z/narrow-capture.png>)
- [Plan preview](<harness-fixture-runtime/results/2026-11-07T14-52-54-050Z/plan-preview.png>)
- [Goal/project editors](<harness-fixture-runtime/results/2026-11-07T14-52-54-050Z/goal-project-editors.png>)
- [Habit logged](<harness-fixture-runtime/results/2026-11-07T14-52-54-050Z/habit-logged.png>)
- [Saved view applied](<harness-fixture-runtime/results/2026-11-07T14-52-54-050Z/saved-view-applied.png>)
- [Automation budget exhausted](<harness-fixture-runtime/results/2026-11-07T14-52-54-050Z/automation-budget-exhausted.png>)
- [Focus completed](<harness-fixture-runtime/results/2026-11-07T14-52-54-050Z/focus-completed.png>)
- [Data backup](<harness-fixture-runtime/results/2026-11-07T14-52-54-050Z/data-backup.png>)
- [Models](<harness-fixture-runtime/results/2026-11-07T14-52-54-050Z/models.png>)
- [Catalog error/retry](<harness-fixture-runtime/results/2026-11-07T14-52-54-050Z/catalog-error.png>)
- [Offline draft](<harness-fixture-runtime/results/2026-11-07T14-52-54-050Z/offline-draft.png>)

## Scope boundaries

This is real Core + real Host bridge + real React client in a fixture container, not a complete DSH Host. Host registration, model catalog and session-projection services are test doubles. It does not cover real DSH authentication/login, Host router/sidebar composition, native desktop shell, real model/cloud dispatch, or every state/control on all 21 pages. Shared browser lane was busy; screenshots were produced by existing playwright-core automation and can be reviewed with read_image. Navigation smoke coverage is distinct from the functional cases enumerated in the report.

[Reproduction and coverage details](<harness-isolated-README.md>)
