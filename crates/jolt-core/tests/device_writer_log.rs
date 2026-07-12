use ed25519_dalek::{Signer, SigningKey};
use jolt_core::{
    merge_device_writer_logs, resolve_merged_device_jolt_address, verify_identity_authority_chain,
    DeviceAuthorizationOperation, DeviceAuthorizationRecord, DeviceWriterLogEntry,
    DeviceWriterLogEntryHash, DeviceWriterLogError, DeviceWriterOperation, DeviceWriterPathMode,
    DeviceWriterRejectionReason, IdentityId, JoltAddress, VerifiedIdentityAuthority,
};
use rand::rngs::OsRng;

fn signing_key() -> SigningKey {
    SigningKey::generate(&mut OsRng)
}

fn sign(key: &SigningKey, bytes: &[u8]) -> Vec<u8> {
    key.sign(bytes).to_bytes().to_vec()
}

fn content_id(bytes: &[u8]) -> jolt_core::ContentId {
    jolt_core::ContentId::from_bytes(bytes)
}

fn authority_for_devices(
    root: &SigningKey,
    devices: &[(&str, &SigningKey, &str)],
) -> (IdentityId, VerifiedIdentityAuthority) {
    let identity = IdentityId::from_public_key(root.verifying_key().to_bytes());
    let mut records: Vec<DeviceAuthorizationRecord> = Vec::new();

    for (index, (device_id, key, label)) in devices.iter().enumerate() {
        let operation = DeviceAuthorizationOperation::authorize_device(
            *device_id,
            key.verifying_key().to_bytes(),
            vec!["identity:write".to_string()],
            Some((*label).to_string()),
            1_780_579_200 + index as u64,
        );
        let record = match records.last() {
            Some(previous) => previous
                .append(operation, 1_780_579_200 + index as u64, |bytes| {
                    sign(root, bytes)
                })
                .unwrap(),
            None => DeviceAuthorizationRecord::genesis(
                root.verifying_key().to_bytes(),
                identity.clone(),
                operation,
                1_780_579_200,
                |bytes| sign(root, bytes),
            )
            .unwrap(),
        };
        records.push(record);
    }

    let authority = verify_identity_authority_chain(&identity, &records).unwrap();
    (identity, authority)
}

#[test]
fn merges_singleton_paths_deterministically_across_discovery_order() {
    let root = signing_key();
    let laptop = signing_key();
    let phone = signing_key();
    let (identity, authority) = authority_for_devices(
        &root,
        &[
            ("dev_laptop", &laptop, "Laptop"),
            ("dev_phone", &phone, "Phone"),
        ],
    );

    let laptop_log = vec![DeviceWriterLogEntry::genesis(
        identity.clone(),
        "dev_laptop",
        DeviceWriterOperation::set_path(
            "/profile",
            content_id(b"alice from laptop"),
            DeviceWriterPathMode::Singleton,
        ),
        100,
        |bytes| sign(&laptop, bytes),
    )
    .unwrap()];
    let phone_log = vec![DeviceWriterLogEntry::genesis(
        identity.clone(),
        "dev_phone",
        DeviceWriterOperation::set_path(
            "/profile",
            content_id(b"alice from phone"),
            DeviceWriterPathMode::Singleton,
        ),
        101,
        |bytes| sign(&phone, bytes),
    )
    .unwrap()];

    let first =
        merge_device_writer_logs(&authority, vec![laptop_log.clone(), phone_log.clone()]).unwrap();
    let second = merge_device_writer_logs(&authority, vec![phone_log, laptop_log]).unwrap();

    assert_eq!(first, second);
    assert_eq!(
        first
            .singleton_paths
            .get("/profile")
            .map(|entry| &entry.content_id),
        Some(&content_id(b"alice from phone"))
    );
    assert_eq!(first.singleton_conflicts.get("/profile").unwrap().len(), 1);
}

#[test]
fn resolves_jolt_address_from_merged_device_state() {
    let root = signing_key();
    let laptop = signing_key();
    let (identity, authority) = authority_for_devices(&root, &[("dev_laptop", &laptop, "Laptop")]);
    let profile_cid = content_id(b"profile from laptop");
    let laptop_log = vec![DeviceWriterLogEntry::genesis(
        identity.clone(),
        "dev_laptop",
        DeviceWriterOperation::set_path(
            "/profile",
            profile_cid.clone(),
            DeviceWriterPathMode::Singleton,
        ),
        100,
        |bytes| sign(&laptop, bytes),
    )
    .unwrap()];
    let merged = merge_device_writer_logs(&authority, vec![laptop_log]).unwrap();

    let resolved = resolve_merged_device_jolt_address(
        &JoltAddress::new(identity.clone(), "/profile").unwrap(),
        &merged,
    )
    .unwrap();

    assert_eq!(resolved.identity, identity);
    assert_eq!(resolved.path, "/profile");
    assert_eq!(resolved.content_id, profile_cid);
}

#[test]
fn preserves_append_records_from_multiple_devices_in_deterministic_order() {
    let root = signing_key();
    let laptop = signing_key();
    let phone = signing_key();
    let (identity, authority) = authority_for_devices(
        &root,
        &[
            ("dev_laptop", &laptop, "Laptop"),
            ("dev_phone", &phone, "Phone"),
        ],
    );

    let laptop_record = content_id(b"paste from laptop");
    let phone_record = content_id(b"paste from phone");
    let laptop_log = vec![DeviceWriterLogEntry::genesis(
        identity.clone(),
        "dev_laptop",
        DeviceWriterOperation::append_record("/apps/pastey/records", laptop_record.clone()),
        101,
        |bytes| sign(&laptop, bytes),
    )
    .unwrap()];
    let phone_log = vec![DeviceWriterLogEntry::genesis(
        identity,
        "dev_phone",
        DeviceWriterOperation::append_record("/apps/pastey/records", phone_record.clone()),
        100,
        |bytes| sign(&phone, bytes),
    )
    .unwrap()];

    let merged = merge_device_writer_logs(&authority, vec![laptop_log, phone_log]).unwrap();
    let records = merged.append_records.get("/apps/pastey/records").unwrap();

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].content_id, phone_record);
    assert_eq!(records[1].content_id, laptop_record);
    assert!(merged.singleton_paths.is_empty());
}

#[test]
fn enumerates_append_records_under_a_path_prefix_in_deterministic_order() {
    let root = signing_key();
    let laptop = signing_key();
    let (identity, authority) = authority_for_devices(&root, &[("dev_laptop", &laptop, "Laptop")]);

    let record_one = content_id(b"record one");
    let record_two = content_id(b"record two");
    let unrelated = content_id(b"unrelated record");
    let genesis = DeviceWriterLogEntry::genesis(
        identity.clone(),
        "dev_laptop",
        DeviceWriterOperation::append_record("/app/items/1", record_one.clone()),
        100,
        |bytes| sign(&laptop, bytes),
    )
    .unwrap();
    let second = genesis
        .append(
            DeviceWriterOperation::append_record("/app/items/2", record_two.clone()),
            101,
            |bytes| sign(&laptop, bytes),
        )
        .unwrap();
    let third = second
        .append(
            DeviceWriterOperation::append_record("/app/other/x", unrelated.clone()),
            102,
            |bytes| sign(&laptop, bytes),
        )
        .unwrap();
    let log = vec![genesis, second, third];

    let merged = merge_device_writer_logs(&authority, vec![log]).unwrap();
    let records = merged.append_records_under("/app/items/");

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].0, "/app/items/1");
    assert_eq!(records[0].1.content_id, record_one);
    assert_eq!(records[1].0, "/app/items/2");
    assert_eq!(records[1].1.content_id, record_two);
    // A prefix outside the collection enumerates nothing from it.
    assert_eq!(merged.append_records_under("/app/missing/").len(), 0);
}

#[test]
fn ignores_revoked_device_entries_after_accepted_sequence() {
    let root = signing_key();
    let laptop = signing_key();
    let identity = IdentityId::from_public_key(root.verifying_key().to_bytes());

    let laptop_authority = DeviceAuthorizationRecord::genesis(
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
    let revoked_authority = laptop_authority
        .append(
            DeviceAuthorizationOperation::revoke_device(
                "dev_laptop",
                Some(0),
                Some("rotated".to_string()),
                1_780_579_300,
            ),
            1_780_579_300,
            |bytes| sign(&root, bytes),
        )
        .unwrap();
    let authority =
        verify_identity_authority_chain(&identity, &[laptop_authority, revoked_authority]).unwrap();

    let accepted_profile = content_id(b"profile before revocation cutoff");
    let rejected_profile = content_id(b"profile after revocation cutoff");
    let accepted_entry = DeviceWriterLogEntry::genesis(
        identity,
        "dev_laptop",
        DeviceWriterOperation::set_path(
            "/profile",
            accepted_profile.clone(),
            DeviceWriterPathMode::Singleton,
        ),
        100,
        |bytes| sign(&laptop, bytes),
    )
    .unwrap();
    let rejected_entry = accepted_entry
        .append(
            DeviceWriterOperation::set_path(
                "/profile",
                rejected_profile.clone(),
                DeviceWriterPathMode::Singleton,
            ),
            101,
            |bytes| sign(&laptop, bytes),
        )
        .unwrap();

    let merged =
        merge_device_writer_logs(&authority, vec![vec![accepted_entry, rejected_entry]]).unwrap();

    assert_eq!(
        merged
            .singleton_paths
            .get("/profile")
            .map(|entry| &entry.content_id),
        Some(&accepted_profile)
    );
    assert_eq!(merged.rejected_entries.len(), 1);
    assert_eq!(
        merged.rejected_entries[0].reason,
        DeviceWriterRejectionReason::RevokedDevice
    );
    assert_eq!(merged.rejected_entries[0].content_id, rejected_profile);
}

#[test]
fn rejects_malformed_device_writer_paths() {
    let root = signing_key();
    let laptop = signing_key();
    let identity = IdentityId::from_public_key(root.verifying_key().to_bytes());

    let err = DeviceWriterLogEntry::genesis(
        identity,
        "dev_laptop",
        DeviceWriterOperation::set_path(
            "profile",
            content_id(b"relative profile path"),
            DeviceWriterPathMode::Singleton,
        ),
        100,
        |bytes| sign(&laptop, bytes),
    )
    .unwrap_err();

    assert_eq!(err, DeviceWriterLogError::InvalidPath);
}

#[test]
fn rejects_entries_signed_by_the_wrong_device_key() {
    let root = signing_key();
    let laptop = signing_key();
    let attacker = signing_key();
    let (identity, authority) = authority_for_devices(&root, &[("dev_laptop", &laptop, "Laptop")]);
    let entry = DeviceWriterLogEntry::genesis(
        identity,
        "dev_laptop",
        DeviceWriterOperation::set_path(
            "/profile",
            content_id(b"profile signed by wrong key"),
            DeviceWriterPathMode::Singleton,
        ),
        100,
        |bytes| sign(&attacker, bytes),
    )
    .unwrap();

    let err = merge_device_writer_logs(&authority, vec![vec![entry]]).unwrap_err();

    assert_eq!(err, DeviceWriterLogError::InvalidSignature);
}

#[test]
fn rejects_broken_device_writer_log_hash_chain() {
    let root = signing_key();
    let laptop = signing_key();
    let (identity, authority) = authority_for_devices(&root, &[("dev_laptop", &laptop, "Laptop")]);
    let first = DeviceWriterLogEntry::genesis(
        identity,
        "dev_laptop",
        DeviceWriterOperation::set_path(
            "/profile",
            content_id(b"profile v1"),
            DeviceWriterPathMode::Singleton,
        ),
        100,
        |bytes| sign(&laptop, bytes),
    )
    .unwrap();
    let mut second = first
        .append(
            DeviceWriterOperation::set_path(
                "/profile",
                content_id(b"profile v2"),
                DeviceWriterPathMode::Singleton,
            ),
            101,
            |bytes| sign(&laptop, bytes),
        )
        .unwrap();
    second.body.previous_entry_hash = Some(DeviceWriterLogEntryHash([9; 32]));
    second.signature = sign(&laptop, &second.body.canonical_bytes());

    let err = merge_device_writer_logs(&authority, vec![vec![first, second]]).unwrap_err();

    assert_eq!(err, DeviceWriterLogError::BrokenPreviousHash { index: 1 });
}

#[test]
fn rejects_out_of_order_device_writer_log_sequence() {
    let root = signing_key();
    let laptop = signing_key();
    let (identity, authority) = authority_for_devices(&root, &[("dev_laptop", &laptop, "Laptop")]);
    let first = DeviceWriterLogEntry::genesis(
        identity,
        "dev_laptop",
        DeviceWriterOperation::set_path(
            "/profile",
            content_id(b"profile v1"),
            DeviceWriterPathMode::Singleton,
        ),
        100,
        |bytes| sign(&laptop, bytes),
    )
    .unwrap();
    let mut second = first
        .append(
            DeviceWriterOperation::set_path(
                "/profile",
                content_id(b"profile v2"),
                DeviceWriterPathMode::Singleton,
            ),
            101,
            |bytes| sign(&laptop, bytes),
        )
        .unwrap();
    second.body.device_sequence = 3;
    second.signature = sign(&laptop, &second.body.canonical_bytes());

    let err = merge_device_writer_logs(&authority, vec![vec![first, second]]).unwrap_err();

    assert_eq!(
        err,
        DeviceWriterLogError::OutOfOrderSequence {
            index: 1,
            expected: 1,
            actual: 3,
        }
    );
}

#[test]
fn records_unknown_device_entries_as_rejected_diagnostics() {
    let root = signing_key();
    let laptop = signing_key();
    let unknown = signing_key();
    let (identity, authority) = authority_for_devices(&root, &[("dev_laptop", &laptop, "Laptop")]);
    let profile = content_id(b"profile from unknown device");
    let entry = DeviceWriterLogEntry::genesis(
        identity,
        "dev_unknown",
        DeviceWriterOperation::set_path(
            "/profile",
            profile.clone(),
            DeviceWriterPathMode::Singleton,
        ),
        100,
        |bytes| sign(&unknown, bytes),
    )
    .unwrap();

    let merged = merge_device_writer_logs(&authority, vec![vec![entry]]).unwrap();

    assert!(merged.singleton_paths.is_empty());
    assert_eq!(merged.rejected_entries.len(), 1);
    assert_eq!(
        merged.rejected_entries[0].reason,
        DeviceWriterRejectionReason::UnknownDevice
    );
    assert_eq!(merged.rejected_entries[0].content_id, profile);
}
