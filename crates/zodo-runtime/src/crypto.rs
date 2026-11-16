//! Passphrase-based encryption for offline sync bundles.
//!
//! This is an envelope for the existing `zodo.sync/1` bundle: PBKDF2-HMAC-SHA256 derives a 256-bit
//! key, AES-256-GCM authenticates the ciphertext with the sync schema as AAD, and only the salt,
//! nonce and ciphertext are stored/transported. The plaintext bundle exists only in memory on the
//! exporting/importing device.

use std::num::NonZeroU32;

use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::pbkdf2;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{Value, json};
use zodo_kernel::error::{ErrorCode, Result, ZodoError};

const ITERATIONS: u32 = 100_000;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const AAD: &[u8] = b"zodo.sync.encrypted/1";
pub const ENCRYPTED_SCHEMA: &str = "zodo.sync.encrypted/1";

fn invalid(message: impl Into<String>) -> ZodoError {
    ZodoError::new(ErrorCode::ValidationFailed, message)
}

fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32]> {
    if passphrase.chars().count() < 12 {
        return Err(invalid("sync passphrase must be at least 12 characters"));
    }
    let mut key = [0u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(ITERATIONS).expect("nonzero iterations"),
        salt,
        passphrase.as_bytes(),
        &mut key,
    );
    Ok(key)
}

pub fn encrypt_bundle(bundle: &Value, passphrase: &str) -> Result<Value> {
    let plaintext = serde_json::to_vec(bundle)
        .map_err(|error| invalid(format!("serialize sync bundle: {error}")))?;
    let rng = SystemRandom::new();
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    rng.fill(&mut salt)
        .map_err(|_| invalid("random source failed for sync salt"))?;
    rng.fill(&mut nonce)
        .map_err(|_| invalid("random source failed for sync nonce"))?;
    let key = derive_key(passphrase, &salt)?;
    let key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, &key)
            .map_err(|_| invalid("failed to initialize sync cipher"))?,
    );
    let mut in_out = plaintext;
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(AAD),
        &mut in_out,
    )
    .map_err(|_| invalid("sync bundle encryption failed"))?;
    Ok(json!({
        "schema": ENCRYPTED_SCHEMA,
        "kdf": "pbkdf2-hmac-sha256",
        "iterations": ITERATIONS,
        "salt_hex": hex::encode(salt),
        "nonce_hex": hex::encode(nonce),
        "ciphertext_hex": hex::encode(in_out),
        "contains_credentials": false,
    }))
}

pub fn decrypt_bundle(envelope: &Value, passphrase: &str) -> Result<Value> {
    if envelope.get("schema").and_then(Value::as_str) != Some(ENCRYPTED_SCHEMA) {
        return Err(invalid("encrypted sync envelope has the wrong schema"));
    }
    if envelope.get("kdf").and_then(Value::as_str) != Some("pbkdf2-hmac-sha256") {
        return Err(invalid("unsupported sync KDF"));
    }
    let iterations = envelope
        .get("iterations")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if iterations != u64::from(ITERATIONS) {
        return Err(invalid("unsupported sync KDF iteration count"));
    }
    let salt = hex::decode(
        envelope
            .get("salt_hex")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
    .map_err(|_| invalid("invalid sync salt"))?;
    let nonce = hex::decode(
        envelope
            .get("nonce_hex")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
    .map_err(|_| invalid("invalid sync nonce"))?;
    let mut ciphertext = hex::decode(
        envelope
            .get("ciphertext_hex")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
    .map_err(|_| invalid("invalid sync ciphertext"))?;
    if salt.len() != SALT_LEN || nonce.len() != NONCE_LEN || ciphertext.len() < 16 {
        return Err(invalid("encrypted sync envelope is malformed"));
    }
    let key = derive_key(passphrase, &salt)?;
    let key = LessSafeKey::new(
        UnboundKey::new(&AES_256_GCM, &key)
            .map_err(|_| invalid("failed to initialize sync cipher"))?,
    );
    let nonce_array: [u8; NONCE_LEN] = nonce
        .try_into()
        .map_err(|_| invalid("invalid sync nonce"))?;
    let plaintext = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce_array),
            Aad::from(AAD),
            &mut ciphertext,
        )
        .map_err(|_| {
            ZodoError::new(
                ErrorCode::PermissionDenied,
                "sync passphrase is wrong or the encrypted bundle was tampered with",
            )
        })?;
    let value: Value = serde_json::from_slice(plaintext)
        .map_err(|error| invalid(format!("decrypted sync bundle is not JSON: {error}")))?;
    let schema = value
        .get("schema")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if schema != "zodo.sync/1" && schema != "zodo.export/1" {
        return Err(invalid("decrypted bundle has an unsupported schema"));
    }
    if value.get("contains_credentials").and_then(Value::as_bool) != Some(false) {
        return Err(invalid("decrypted bundle must not contain credentials"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle() -> Value {
        json!({
            "schema": "zodo.sync/1",
            "contains_credentials": false,
            "facts": {"workspace": {"id": "ws_local"}},
        })
    }

    #[test]
    fn round_trip_and_wrong_passphrase() {
        let envelope = encrypt_bundle(&bundle(), "correct horse battery staple").unwrap();
        assert_eq!(envelope["schema"], ENCRYPTED_SCHEMA);
        assert!(
            !envelope["ciphertext_hex"]
                .as_str()
                .unwrap()
                .contains("ws_local")
        );
        let decrypted = decrypt_bundle(&envelope, "correct horse battery staple").unwrap();
        assert_eq!(decrypted, bundle());
        assert!(decrypt_bundle(&envelope, "wrong passphrase 123").is_err());
    }

    #[test]
    fn tampered_ciphertext_salt_and_nonce_fail_authentication() {
        let passphrase = "correct horse battery staple";
        let envelope = encrypt_bundle(&bundle(), passphrase).unwrap();
        for field in ["ciphertext_hex", "salt_hex", "nonce_hex"] {
            let mut tampered = envelope.clone();
            let mut bytes = hex::decode(tampered[field].as_str().unwrap()).unwrap();
            bytes[0] ^= 1;
            tampered[field] = json!(hex::encode(bytes));
            assert_eq!(
                decrypt_bundle(&tampered, passphrase).unwrap_err().code,
                ErrorCode::PermissionDenied,
                "{field}"
            );
        }
    }

    #[test]
    fn malformed_envelopes_fail_validation_without_panicking() {
        let passphrase = "correct horse battery staple";
        let envelope = encrypt_bundle(&bundle(), passphrase).unwrap();
        for (field, value) in [
            ("schema", json!("zodo.sync.encrypted/2")),
            ("kdf", json!("unsupported")),
            ("iterations", json!(0)),
            ("iterations", json!(100_001)),
            ("iterations", json!("100000")),
            ("salt_hex", json!("zz")),
            ("salt_hex", json!("00")),
            ("nonce_hex", json!("zz")),
            ("nonce_hex", json!("00")),
            ("ciphertext_hex", json!("zz")),
            ("ciphertext_hex", json!("00".repeat(15))),
        ] {
            let mut malformed = envelope.clone();
            malformed[field] = value;
            assert_eq!(
                decrypt_bundle(&malformed, passphrase).unwrap_err().code,
                ErrorCode::ValidationFailed,
                "{field}"
            );
        }
        for field in [
            "schema",
            "kdf",
            "iterations",
            "salt_hex",
            "nonce_hex",
            "ciphertext_hex",
        ] {
            let mut missing = envelope.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert_eq!(
                decrypt_bundle(&missing, passphrase).unwrap_err().code,
                ErrorCode::ValidationFailed,
                "missing {field}"
            );
        }
    }

    #[test]
    fn decrypted_payload_requires_supported_schema_and_explicit_credential_exclusion() {
        let passphrase = "correct horse battery staple";
        let mut export = bundle();
        export["schema"] = json!("zodo.export/1");
        let envelope = encrypt_bundle(&export, passphrase).unwrap();
        assert_eq!(decrypt_bundle(&envelope, passphrase).unwrap(), export);

        for (field, value) in [
            ("schema", json!("zodo.sync/2")),
            ("contains_credentials", json!(true)),
            ("contains_credentials", Value::Null),
            ("contains_credentials", json!("false")),
        ] {
            let mut invalid = bundle();
            invalid[field] = value;
            let envelope = encrypt_bundle(&invalid, passphrase).unwrap();
            assert_eq!(
                decrypt_bundle(&envelope, passphrase).unwrap_err().code,
                ErrorCode::ValidationFailed,
                "{field}"
            );
        }
        let mut missing = bundle();
        missing
            .as_object_mut()
            .unwrap()
            .remove("contains_credentials");
        let envelope = encrypt_bundle(&missing, passphrase).unwrap();
        assert_eq!(
            decrypt_bundle(&envelope, passphrase).unwrap_err().code,
            ErrorCode::ValidationFailed
        );
    }

    #[test]
    fn short_passphrase_is_rejected() {
        assert!(encrypt_bundle(&bundle(), "short").is_err());
    }
}
