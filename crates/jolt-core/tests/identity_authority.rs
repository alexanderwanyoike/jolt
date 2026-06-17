use ed25519_dalek::{Signer, SigningKey};
use jolt_core::{
    verify_identity_authority_chain, AuthorizedDeviceStatus, DeviceAuthorizationOperation,
    DeviceAuthorizationRecord, DeviceAuthorizationRecordHash, DeviceEncryptionPublicKey,
    IdentityAuthorityError, IdentityId,
};
use rand::rngs::OsRng;

fn sign(key: &SigningKey, bytes: &[u8]) -> Vec<u8> {
    key.sign(bytes).to_bytes().to_vec()
}

fn signing_key() -> SigningKey {
    SigningKey::generate(&mut OsRng)
}

#[test]
fn verifies_authorized_and_revoked_device_records() {
    let root = signing_key();
    let laptop = signing_key();
    let phone = signing_key();
    let identity = IdentityId::from_public_key(root.verifying_key().to_bytes());

    let laptop_record = DeviceAuthorizationRecord::genesis(
        root.verifying_key().to_bytes(),
        identity.clone(),
        DeviceAuthorizationOperation::authorize_device(
            "dev_laptop",
            laptop.verifying_key().to_bytes(),
            vec!["identity:write".to_string(), "app:grant".to_string()],
            Some("Laptop".to_string()),
            1_780_579_200,
        ),
        1_780_579_200,
        |bytes| sign(&root, bytes),
    )
    .unwrap();
    let phone_record = laptop_record
        .append(
            DeviceAuthorizationOperation::authorize_device(
                "dev_phone",
                phone.verifying_key().to_bytes(),
                vec!["identity:write".to_string()],
                Some("Phone".to_string()),
                1_780_579_300,
            ),
            1_780_579_300,
            |bytes| sign(&root, bytes),
        )
        .unwrap();
    let revoke_phone = phone_record
        .append(
            DeviceAuthorizationOperation::revoke_device(
                "dev_phone",
                Some(7),
                Some("lost_device".to_string()),
                1_780_579_400,
            ),
            1_780_579_400,
            |bytes| sign(&root, bytes),
        )
        .unwrap();

    let verified =
        verify_identity_authority_chain(&identity, &[laptop_record, phone_record, revoke_phone])
            .unwrap();

    assert_eq!(verified.latest_sequence, 2);
    let laptop = verified.devices.get("dev_laptop").unwrap();
    assert_eq!(laptop.status, AuthorizedDeviceStatus::Active);
    assert!(verified.device_can_write("dev_laptop", 500));

    let phone = verified.devices.get("dev_phone").unwrap();
    assert_eq!(phone.status, AuthorizedDeviceStatus::Revoked);
    assert_eq!(phone.accepted_through_device_sequence, Some(7));
    assert!(verified.device_can_write("dev_phone", 7));
    assert!(!verified.device_can_write("dev_phone", 8));
}

#[test]
fn rejects_authority_chain_with_non_root_signature() {
    let root = signing_key();
    let attacker = signing_key();
    let device = signing_key();
    let identity = IdentityId::from_public_key(root.verifying_key().to_bytes());

    let record = DeviceAuthorizationRecord::genesis(
        root.verifying_key().to_bytes(),
        identity.clone(),
        DeviceAuthorizationOperation::authorize_device(
            "dev_attacker",
            device.verifying_key().to_bytes(),
            vec!["identity:write".to_string()],
            None,
            1_780_579_200,
        ),
        1_780_579_200,
        |bytes| sign(&attacker, bytes),
    )
    .unwrap();

    assert!(verify_identity_authority_chain(&identity, &[record]).is_err());
}

#[test]
fn rejects_authority_chain_with_broken_previous_record_hash() {
    let root = signing_key();
    let laptop = signing_key();
    let phone = signing_key();
    let identity = IdentityId::from_public_key(root.verifying_key().to_bytes());

    let laptop_record = DeviceAuthorizationRecord::genesis(
        root.verifying_key().to_bytes(),
        identity.clone(),
        DeviceAuthorizationOperation::authorize_device(
            "dev_laptop",
            laptop.verifying_key().to_bytes(),
            vec!["identity:write".to_string()],
            Some("Laptop".to_string()),
            1_780_579_200,
        ),
        1_780_579_200,
        |bytes| sign(&root, bytes),
    )
    .unwrap();
    let mut phone_record = laptop_record
        .append(
            DeviceAuthorizationOperation::authorize_device(
                "dev_phone",
                phone.verifying_key().to_bytes(),
                vec!["identity:write".to_string()],
                Some("Phone".to_string()),
                1_780_579_300,
            ),
            1_780_579_300,
            |bytes| sign(&root, bytes),
        )
        .unwrap();
    phone_record.body.previous_record_hash = Some(DeviceAuthorizationRecordHash([7; 32]));
    phone_record.signature = sign(&root, &phone_record.body.canonical_bytes());

    let err =
        verify_identity_authority_chain(&identity, &[laptop_record, phone_record]).unwrap_err();

    assert_eq!(err, IdentityAuthorityError::BrokenPreviousHash { index: 1 });
}

#[test]
fn rejects_authority_chain_with_out_of_order_sequence() {
    let root = signing_key();
    let laptop = signing_key();
    let phone = signing_key();
    let identity = IdentityId::from_public_key(root.verifying_key().to_bytes());

    let laptop_record = DeviceAuthorizationRecord::genesis(
        root.verifying_key().to_bytes(),
        identity.clone(),
        DeviceAuthorizationOperation::authorize_device(
            "dev_laptop",
            laptop.verifying_key().to_bytes(),
            vec!["identity:write".to_string()],
            Some("Laptop".to_string()),
            1_780_579_200,
        ),
        1_780_579_200,
        |bytes| sign(&root, bytes),
    )
    .unwrap();
    let mut phone_record = laptop_record
        .append(
            DeviceAuthorizationOperation::authorize_device(
                "dev_phone",
                phone.verifying_key().to_bytes(),
                vec!["identity:write".to_string()],
                Some("Phone".to_string()),
                1_780_579_300,
            ),
            1_780_579_300,
            |bytes| sign(&root, bytes),
        )
        .unwrap();
    phone_record.body.sequence = 3;
    phone_record.signature = sign(&root, &phone_record.body.canonical_bytes());

    let err =
        verify_identity_authority_chain(&identity, &[laptop_record, phone_record]).unwrap_err();

    assert_eq!(
        err,
        IdentityAuthorityError::OutOfOrderSequence {
            index: 1,
            expected: 1,
            actual: 3,
        }
    );
}

#[test]
fn rejects_unknown_device_revocation() {
    let root = signing_key();
    let identity = IdentityId::from_public_key(root.verifying_key().to_bytes());

    let record = DeviceAuthorizationRecord::genesis(
        root.verifying_key().to_bytes(),
        identity.clone(),
        DeviceAuthorizationOperation::revoke_device(
            "dev_missing",
            Some(9),
            Some("not_authorized".to_string()),
            1_780_579_200,
        ),
        1_780_579_200,
        |bytes| sign(&root, bytes),
    )
    .unwrap();

    let err = verify_identity_authority_chain(&identity, &[record]).unwrap_err();

    assert_eq!(
        err,
        IdentityAuthorityError::UnknownDeviceRevocation("dev_missing".to_string())
    );
}

#[test]
fn revoked_device_without_cutoff_cannot_write_any_sequence() {
    let root = signing_key();
    let laptop = signing_key();
    let identity = IdentityId::from_public_key(root.verifying_key().to_bytes());

    let laptop_record = DeviceAuthorizationRecord::genesis(
        root.verifying_key().to_bytes(),
        identity.clone(),
        DeviceAuthorizationOperation::authorize_device(
            "dev_laptop",
            laptop.verifying_key().to_bytes(),
            vec!["identity:write".to_string()],
            Some("Laptop".to_string()),
            1_780_579_200,
        ),
        1_780_579_200,
        |bytes| sign(&root, bytes),
    )
    .unwrap();
    let revoke_laptop = laptop_record
        .append(
            DeviceAuthorizationOperation::revoke_device(
                "dev_laptop",
                None,
                Some("compromised".to_string()),
                1_780_579_300,
            ),
            1_780_579_300,
            |bytes| sign(&root, bytes),
        )
        .unwrap();

    let verified =
        verify_identity_authority_chain(&identity, &[laptop_record, revoke_laptop]).unwrap();

    assert!(!verified.device_can_write("dev_laptop", 0));
    assert!(!verified.device_can_write("dev_laptop", 1));
    assert!(!verified.device_can_write("dev_unknown", 0));
}

#[test]
fn preserves_authorized_device_encryption_keys() {
    let root = signing_key();
    let laptop = signing_key();
    let identity = IdentityId::from_public_key(root.verifying_key().to_bytes());
    let encryption_key = DeviceEncryptionPublicKey {
        key_id: "enc_laptop_1".to_string(),
        suite_family: "x25519-hkdf-chacha20poly1305".to_string(),
        public_key: [4; 32].to_vec(),
        created_at: 1_780_579_200,
    };

    let record = DeviceAuthorizationRecord::genesis(
        root.verifying_key().to_bytes(),
        identity.clone(),
        DeviceAuthorizationOperation::authorize_device_with_encryption_keys(
            "dev_laptop",
            laptop.verifying_key().to_bytes(),
            vec![encryption_key.clone()],
            vec!["identity:write".to_string(), "encrypt:receive".to_string()],
            Some("Laptop".to_string()),
            1_780_579_200,
        ),
        1_780_579_200,
        |bytes| sign(&root, bytes),
    )
    .unwrap();

    let verified = verify_identity_authority_chain(&identity, &[record]).unwrap();
    let laptop = verified.devices.get("dev_laptop").unwrap();

    assert_eq!(laptop.encryption_keys, vec![encryption_key]);
    assert_eq!(
        laptop.capabilities,
        vec!["identity:write".to_string(), "encrypt:receive".to_string()]
    );
}
