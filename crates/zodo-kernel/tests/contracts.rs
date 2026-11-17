//! Versioned contract fixtures: parse, validate and round-trip the same envelope bytes a client sends.

use serde_json::{Value, json};
use zodo_kernel::commands::{CommandEnvelope, ErrorEnvelope, QueryEnvelope};
use zodo_kernel::error::ErrorCode;
use zodo_kernel::model::{DataClass, ModelAccess, PrivacyPolicy};

const COMMAND: &str = include_str!("../../../contracts/fixtures/command.json");
const QUERY: &str = include_str!("../../../contracts/fixtures/query.json");
const ERROR: &str = include_str!("../../../contracts/fixtures/error.json");

#[test]
fn command_fixture_parses_validates_and_round_trips() {
    let envelope: CommandEnvelope = serde_json::from_str(COMMAND).expect("command fixture");
    assert_eq!(envelope.schema, "zodo.command/1");
    let parsed = envelope.clone().parse().expect("command parse/validate");
    assert!(matches!(
        parsed.payload,
        zodo_kernel::commands::CommandPayload::TaskCreate { .. }
    ));
    let value: Value = serde_json::from_str(COMMAND).unwrap();
    let round_trip = serde_json::to_value(&envelope).unwrap();
    assert_eq!(round_trip, value);
    assert_eq!(value["schema"], "zodo.command/1");
}

#[test]
fn query_fixture_parses_and_round_trips() {
    let envelope: QueryEnvelope = serde_json::from_str(QUERY).expect("query fixture");
    assert_eq!(envelope.schema, "zodo.query/1");
    let parsed = envelope.clone().parse().expect("query parse/validate");
    assert!(matches!(
        parsed.payload,
        zodo_kernel::commands::QueryPayload::TasksList { .. }
    ));
    let value: Value = serde_json::from_str(QUERY).unwrap();
    assert_eq!(serde_json::to_value(&envelope).unwrap(), value);
}

#[test]
fn error_fixture_parses_and_round_trips() {
    let envelope: ErrorEnvelope = serde_json::from_str(ERROR).expect("error fixture");
    assert_eq!(envelope.schema, "zodo.error/1");
    assert_eq!(
        envelope.error.code,
        zodo_kernel::error::ErrorCode::ValidationFailed
    );
    let value: Value = serde_json::from_str(ERROR).unwrap();
    assert_eq!(serde_json::to_value(&envelope).unwrap(), value);
}

#[test]
fn invalid_contracts_are_rejected() {
    let mut bad: Value = serde_json::from_str(COMMAND).unwrap();
    bad["schema"] = json!("zodo.command/2");
    let envelope: CommandEnvelope = serde_json::from_value(bad).unwrap();
    assert!(envelope.parse().is_err());
    let mut missing: Value = serde_json::from_str(COMMAND).unwrap();
    missing.as_object_mut().unwrap().remove("idempotency_key");
    assert!(serde_json::from_value::<CommandEnvelope>(missing).is_err());
}

#[test]
fn command_identifiers_enforce_byte_limits_and_reject_blank_values() {
    let original: CommandEnvelope = serde_json::from_str(COMMAND).unwrap();
    for field in ["request_id", "idempotency_key"] {
        for text in ["x".repeat(200), "界".repeat(66)] {
            let mut value = serde_json::to_value(&original).unwrap();
            value[field] = json!(text);
            assert!(
                serde_json::from_value::<CommandEnvelope>(value)
                    .unwrap()
                    .parse()
                    .is_ok()
            );
        }
        for text in [
            String::new(),
            " \t\n".to_string(),
            "x".repeat(201),
            "界".repeat(67),
        ] {
            let mut value = serde_json::to_value(&original).unwrap();
            value[field] = json!(text);
            let error = serde_json::from_value::<CommandEnvelope>(value)
                .unwrap()
                .parse()
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::ValidationFailed, "{field}");
        }
    }
    let mut blank_workspace = original;
    blank_workspace.workspace_id = " \t".to_string();
    assert_eq!(
        blank_workspace.parse().unwrap_err().code,
        ErrorCode::ValidationFailed
    );
}

#[test]
fn command_operation_version_is_required() {
    let mut missing: Value = serde_json::from_str(COMMAND).unwrap();
    missing.as_object_mut().unwrap().remove("operation_version");
    let error = serde_json::from_value::<CommandEnvelope>(missing).unwrap_err();
    assert!(error.to_string().contains("missing field"), "{error}");
    assert!(error.to_string().contains("operation_version"), "{error}");
}

#[test]
fn unsupported_versions_and_unknown_envelope_fields_are_rejected() {
    for version in [0, 2, u8::MAX] {
        let mut envelope: CommandEnvelope = serde_json::from_str(COMMAND).unwrap();
        envelope.operation_version = version;
        assert_eq!(
            envelope.parse().unwrap_err().code,
            ErrorCode::UnsupportedOperation
        );
    }
    let mut command: Value = serde_json::from_str(COMMAND).unwrap();
    command["unexpected"] = json!(true);
    assert!(serde_json::from_value::<CommandEnvelope>(command).is_err());
    let mut query: Value = serde_json::from_str(QUERY).unwrap();
    query["unexpected"] = json!(true);
    assert!(serde_json::from_value::<QueryEnvelope>(query).is_err());
}

#[test]
fn query_request_id_has_the_same_byte_boundary_as_commands() {
    for text in ["x".repeat(200), "界".repeat(66)] {
        let mut envelope: QueryEnvelope = serde_json::from_str(QUERY).unwrap();
        envelope.request_id = text;
        assert!(envelope.parse().is_ok());
    }
    for text in [
        String::new(),
        " \t\n".to_string(),
        "x".repeat(201),
        "界".repeat(67),
    ] {
        let mut envelope: QueryEnvelope = serde_json::from_str(QUERY).unwrap();
        envelope.request_id = text;
        assert_eq!(
            envelope.parse().unwrap_err().code,
            ErrorCode::ValidationFailed
        );
    }
}

#[test]
fn canonical_command_hash_ignores_transport_ids_but_binds_revision_and_payload() {
    let original: CommandEnvelope = serde_json::from_str(COMMAND).unwrap();
    let expected = original.clone().parse().unwrap().canonical_hash;
    let mut retry = original.clone();
    retry.request_id = "different-request".to_string();
    retry.idempotency_key = "different-key".to_string();
    assert_eq!(retry.parse().unwrap().canonical_hash, expected);

    let mut changed = original.clone();
    changed.payload["task"]["title"] = json!("Different title");
    assert_ne!(changed.parse().unwrap().canonical_hash, expected);
    let mut changed = original.clone();
    changed.base_revision = Some(original.base_revision.unwrap_or(0) + 1);
    assert_ne!(changed.parse().unwrap().canonical_hash, expected);
    let mut changed = original.clone();
    changed.policy_revision = Some(original.policy_revision.unwrap_or(0) + 1);
    assert_ne!(changed.parse().unwrap().canonical_hash, expected);
    let mut changed = original;
    changed.workspace_id = "different-workspace".to_string();
    assert_ne!(changed.parse().unwrap().canonical_hash, expected);
}

#[test]
fn privacy_policy_defaults_deny_all_model_access_and_diagnostic_capture() {
    let denied = ModelAccess {
        goals: false,
        projects: false,
        captures: false,
    };
    assert_eq!(ModelAccess::default(), denied);
    assert_eq!(
        PrivacyPolicy::default(),
        PrivacyPolicy {
            model_access: denied,
            diagnostic_capture: false,
        }
    );
    assert_eq!(
        serde_json::to_value(PrivacyPolicy::default()).unwrap(),
        json!({
            "model_access": {"goals": false, "projects": false, "captures": false},
            "diagnostic_capture": false
        })
    );
}

#[test]
fn privacy_policy_round_trips_independent_explicit_permissions() {
    for goals in [false, true] {
        for projects in [false, true] {
            for captures in [false, true] {
                for diagnostic_capture in [false, true] {
                    let current = json!({
                        "model_access": {"goals": goals, "projects": projects, "captures": captures},
                        "diagnostic_capture": diagnostic_capture
                    });
                    let policy: PrivacyPolicy = serde_json::from_value(current.clone()).unwrap();
                    assert_eq!(
                        policy,
                        PrivacyPolicy {
                            model_access: ModelAccess {
                                goals,
                                projects,
                                captures,
                            },
                            diagnostic_capture,
                        }
                    );
                    assert_eq!(serde_json::to_value(policy).unwrap(), current);
                }
            }
        }
    }
}

#[test]
fn privacy_policy_rejects_obsolete_unknown_missing_and_non_boolean_fields() {
    let current = json!({
        "model_access": {"goals": false, "projects": false, "captures": false},
        "diagnostic_capture": false
    });
    for old in [
        json!({"model_route": "local_only", "allowed_cloud_providers": [], "diagnostic_capture": false}),
        json!({"model_route": "cloud_assisted", "allowed_cloud_providers": ["deepseek-cloud"], "diagnostic_capture": false}),
        json!({"model_route": "cloud_allowed", "allowed_cloud_providers": [], "diagnostic_capture": false}),
    ] {
        assert!(serde_json::from_value::<PrivacyPolicy>(old).is_err());
    }
    for (field, value) in [
        ("model_route", json!("local_only")),
        ("allowed_cloud_providers", json!([])),
        ("private_notes", json!("device_only")),
        ("unknown_policy_option", json!(true)),
    ] {
        let mut invalid = current.clone();
        invalid[field] = value;
        let error = serde_json::from_value::<PrivacyPolicy>(invalid).unwrap_err();
        assert!(error.to_string().contains("unknown field"), "{error}");
    }
    for field in ["model_access", "diagnostic_capture"] {
        let mut missing = current.clone();
        missing.as_object_mut().unwrap().remove(field);
        let error = serde_json::from_value::<PrivacyPolicy>(missing).unwrap_err();
        assert!(error.to_string().contains("missing field"), "{error}");
    }
    for field in ["goals", "projects", "captures"] {
        let mut missing = current.clone();
        missing["model_access"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        let error = serde_json::from_value::<PrivacyPolicy>(missing).unwrap_err();
        assert!(error.to_string().contains("missing field"), "{error}");
        for invalid_value in [Value::Null, json!(0), json!("false"), json!([]), json!({})] {
            let mut invalid = current.clone();
            invalid["model_access"][field] = invalid_value;
            assert!(serde_json::from_value::<PrivacyPolicy>(invalid).is_err());
        }
    }
    for invalid_value in [Value::Null, json!(0), json!("false"), json!([]), json!({})] {
        let mut invalid = current.clone();
        invalid["diagnostic_capture"] = invalid_value;
        assert!(serde_json::from_value::<PrivacyPolicy>(invalid).is_err());
    }
    for field in ["notes", "tasks", "unknown_access_option"] {
        let mut invalid = current.clone();
        invalid["model_access"][field] = json!(true);
        let error = serde_json::from_value::<PrivacyPolicy>(invalid).unwrap_err();
        assert!(error.to_string().contains("unknown field"), "{error}");
    }
    for invalid_access in [
        Value::Null,
        json!(false),
        json!("false"),
        json!([]),
        json!({}),
    ] {
        let mut invalid = current.clone();
        invalid["model_access"] = invalid_access;
        assert!(serde_json::from_value::<PrivacyPolicy>(invalid).is_err());
    }
    assert!(serde_json::from_value::<PrivacyPolicy>(json!({})).is_err());
}

#[test]
fn data_class_keeps_all_existing_classifications() {
    for (value, expected) in [
        ("device_only", DataClass::DeviceOnly),
        ("trusted_workspace", DataClass::TrustedWorkspace),
        ("cloud_allowed", DataClass::CloudAllowed),
    ] {
        let parsed: DataClass = serde_json::from_value(json!(value)).unwrap();
        assert_eq!(parsed, expected);
        assert_eq!(serde_json::to_value(parsed).unwrap(), json!(value));
    }
}
