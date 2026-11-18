use serde_json::{Value, json};
use zodo_kernel::ops::ChangeOp;
fn slot() -> Value {
    json!({"operation": "allocation.schedule", "payload": {"allocation": {"task_id": "task", "expected_task_version": 1, "capacity_block_id": "capacity", "start": "2026-10-25T02:00:00Z", "end": "2026-10-25T02:30:00Z"}}})
}
#[test]
fn strict_schedule_round_trip_and_schedule_effect() {
    let value = slot();
    let op: ChangeOp = serde_json::from_value(value.clone()).unwrap();
    op.validate_shape().unwrap();
    assert_eq!(serde_json::to_value(&op).unwrap(), value);
    assert_eq!(op.effects(), vec!["schedule"]);
}
#[test]
fn strict_schedule_rejects_missing_fields_and_override_knobs() {
    for field in [
        "task_id",
        "expected_task_version",
        "capacity_block_id",
        "start",
        "end",
    ] {
        let mut value = slot();
        value["payload"]["allocation"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(serde_json::from_value::<ChangeOp>(value).is_err());
    }
    for field in ["id", "minutes", "locked", "ignore_conflicts"] {
        let mut value = slot();
        value["payload"]["allocation"][field] = json!(true);
        assert!(serde_json::from_value::<ChangeOp>(value).is_err());
    }
}
#[test]
fn strict_schedule_requires_minute_precision_positive_bounded_interval_and_version() {
    for (field, invalid) in [
        ("start", json!("2026-10-25T02:00:01Z")),
        ("end", json!("2026-10-25T02:00:00Z")),
        ("end", json!("2026-10-27T02:00:00Z")),
        ("expected_task_version", json!(0)),
        ("capacity_block_id", json!(" ")),
    ] {
        let mut value = slot();
        value["payload"]["allocation"][field] = invalid;
        let op: ChangeOp = serde_json::from_value(value).unwrap();
        assert!(op.validate_shape().is_err());
    }
}
