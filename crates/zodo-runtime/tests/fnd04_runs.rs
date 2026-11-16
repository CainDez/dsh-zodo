use serde_json::json;
use zodo_kernel::model::{Workspace, World};
use zodo_runtime::SqliteStore;

fn store() -> SqliteStore {
    let dir = tempfile::tempdir().unwrap().keep();
    let mut store = SqliteStore::open(dir.join("fnd04.sqlite3")).unwrap();
    let now = chrono::Utc::now();
    let world = World::new(Workspace::new(
        "ws_test".to_string(),
        "Test".to_string(),
        now,
    ));
    store.create_workspace(&world).unwrap();
    store
}

#[test]
fn run_state_machine_and_recovery_are_durable() {
    let store = store();
    let created = store
        .create_run("ws_test", "goal.decompose", &json!({"goal_id": "g1"}))
        .unwrap();
    assert_eq!(created.state, "queued");
    assert!(!created.cancel_requested);

    let claimed = store
        .claim_next_run("ws_test", "worker_1")
        .unwrap()
        .expect("queued run claimed");
    assert_eq!(claimed.id, created.id);
    assert_eq!(claimed.state, "running");
    assert_eq!(claimed.lease_owner.as_deref(), Some("worker_1"));

    store
        .append_run_step(&created.id, "context.loaded", &json!({"rev": 1}))
        .unwrap();
    store
        .checkpoint_run(&created.id, &json!({"tasks": []}))
        .unwrap();
    store
        .update_run_state(&created.id, "failed", None, Some(&json!({"code": "TEST"})))
        .unwrap();
    let failed = store.get_run(&created.id).unwrap().unwrap();
    assert_eq!(failed.state, "failed");
    assert!(failed.error.is_some());
    assert!(failed.ended_at.is_some());
    assert!(failed.lease_owner.is_none());

    // A cancelled queued run never gets claimed.
    let queued_cancel = store
        .create_run("ws_test", "goal.decompose", &json!({"goal_id": "g2"}))
        .unwrap();
    let cancelled = store
        .request_run_cancel(&queued_cancel.id)
        .unwrap()
        .unwrap();
    assert_eq!(cancelled.state, "cancelled");
    assert!(
        store
            .claim_next_run("ws_test", "worker_1")
            .unwrap()
            .is_none()
    );

    // An interrupted worker returns running work to queued, and a cancel request wins on recovery.
    let running = store
        .create_run("ws_test", "goal.decompose", &json!({"goal_id": "g3"}))
        .unwrap();
    store
        .claim_next_run("ws_test", "worker_dead")
        .unwrap()
        .expect("running run claimed");
    let recovered = store.recover_runs("ws_test").unwrap();
    assert_eq!(recovered, 1);
    assert_eq!(store.get_run(&running.id).unwrap().unwrap().state, "queued");

    store
        .claim_next_run("ws_test", "worker_dead")
        .unwrap()
        .expect("recovered run re-claimed");
    store.request_run_cancel(&running.id).unwrap();
    store.recover_runs("ws_test").unwrap();
    assert_eq!(
        store.get_run(&running.id).unwrap().unwrap().state,
        "cancelled"
    );

    let all = store.list_runs("ws_test", 10).unwrap();
    assert_eq!(all.len(), 3);
    assert!(
        store
            .update_run_state(&created.id, "not_a_state", None, None)
            .is_err()
    );
}

#[test]
fn waiting_cancel_success_and_interrupted_transitions_are_defined() {
    let store = store();
    // waiting runs can be cancelled immediately instead of waiting for a worker.
    let waiting = store
        .create_run("ws_test", "goal.decompose", &json!({"goal_id": "w1"}))
        .unwrap();
    store
        .claim_next_run("ws_test", "worker_w")
        .unwrap()
        .expect("claim waiting");
    store
        .update_run_state(&waiting.id, "waiting", None, None)
        .unwrap();
    let cancelled = store.request_run_cancel(&waiting.id).unwrap().unwrap();
    assert_eq!(cancelled.state, "cancelled");
    assert!(cancelled.ended_at.is_some());

    // succeeded runs are terminal and expose output.
    let succeeded = store
        .create_run("ws_test", "goal.decompose", &json!({"goal_id": "w2"}))
        .unwrap();
    store
        .claim_next_run("ws_test", "worker_s")
        .unwrap()
        .expect("claim succeeded");
    store
        .update_run_state(&succeeded.id, "succeeded", Some(&json!({"tasks": 2})), None)
        .unwrap();
    let done = store.get_run(&succeeded.id).unwrap().unwrap();
    assert_eq!(done.state, "succeeded");
    assert_eq!(done.output.as_ref().unwrap()["tasks"], 2);
    assert!(done.ended_at.is_some());
    assert!(done.lease_owner.is_none());

    // interrupted is a defined terminal/failure state.
    let interrupted = store
        .create_run("ws_test", "goal.decompose", &json!({"goal_id": "w3"}))
        .unwrap();
    store
        .claim_next_run("ws_test", "worker_i")
        .unwrap()
        .expect("claim interrupted");
    store
        .update_run_state(&interrupted.id, "interrupted", None, None)
        .unwrap();
    assert_eq!(
        store.get_run(&interrupted.id).unwrap().unwrap().state,
        "interrupted"
    );

    // cancel requested while running wins over a later success write.
    let race = store
        .create_run("ws_test", "goal.decompose", &json!({"goal_id": "w4"}))
        .unwrap();
    store
        .claim_next_run("ws_test", "worker_r")
        .unwrap()
        .expect("claim race");
    store.request_run_cancel(&race.id).unwrap();
    assert!(
        store
            .update_run_state(&race.id, "succeeded", Some(&json!({"tasks": 1})), None)
            .is_err()
    );
    store
        .update_run_state(&race.id, "cancelled", None, None)
        .unwrap();
    assert_eq!(store.get_run(&race.id).unwrap().unwrap().state, "cancelled");
}
