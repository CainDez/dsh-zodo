use serde_json::json;
use zodo_kernel::ops::ChangeOp;
use zodo_kernel::proposal::proposal_content_hash;

#[test]
fn legacy_plan_round_trips_without_new_hash_fields() {
    let value = json!({"operation": "plan.apply", "payload": {"replace_task_ids": ["task"], "assignments": [], "unplaced": []}});
    let op: ChangeOp = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&op).unwrap(), value);
    let restored: ChangeOp = serde_json::from_value(serde_json::to_value(&op).unwrap()).unwrap();
    assert_eq!(
        proposal_content_hash("ws", "Original intent", &[op], 4, 1),
        proposal_content_hash("ws", "Original intent", &[restored], 4, 1)
    );
}

#[test]
fn deletion_binding_is_in_the_approved_content_hash() {
    let make = |ids| {
        serde_json::from_value::<ChangeOp>(json!({"operation": "plan.apply", "payload": {"replace_task_ids": ["task"], "expected_removed_allocation_ids": ids, "assignments": [], "unplaced": []}})).unwrap()
    };
    let empty = make(json!([]));
    let first = make(json!(["allocation_1"]));
    let second = make(json!(["allocation_2"]));
    let hash = |op| proposal_content_hash("ws", "Replan", &[op], 4, 1);
    assert_ne!(hash(empty), hash(first.clone()));
    assert_ne!(hash(first), hash(second));
    assert!(
        make(json!(["duplicate", "duplicate"]))
            .validate_shape()
            .is_err()
    );
}
