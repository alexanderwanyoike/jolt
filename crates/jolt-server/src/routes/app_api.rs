use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::Multipart;
use jolt_core::{
    verify_identity_encryption_key_record_for_identity, EncryptedObjectEnvelope,
    EncryptedObjectRecipient, IdentityEncryptionKey, IdentityEncryptionKeyRecord, IdentityId,
    JoltAddress, IDENTITY_ENCRYPTION_KEYS_PATH,
};
use jolt_network::{
    AppendRecordInfo, EncryptedObjectResponse, FetchResult, NetworkError, PublishResponse,
    PublishedContentInfo, ResolveResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::ApiError;
use crate::routes::fetch::{fetch_error_for_target, FetchRequest};
use crate::routes::home_relay::HomeRelayPinRequest;
use crate::routes::resolve::ResolveRequest;
use crate::session_store::{AppSessionStoreError, AppSessionView};
use crate::state::AppState;

static APP_TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub async fn resolve_address(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ResolveRequest>,
) -> Result<Json<ResolveResponse>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_capability(&session, "resolve:public")?;

    let result = state.daemon.resolve(req.address).await?;
    Ok(Json(result))
}

pub async fn fetch_content(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<FetchRequest>,
) -> Result<Json<FetchResult>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_capability(&session, "fetch:public")?;

    let target = req.target.or(req.content_id).ok_or_else(|| {
        AppApiError::Network(NetworkError::InvalidInput(
            "missing fetch target".to_string(),
        ))
    })?;
    let content_id = match jolt_core::JoltAddress::from_str(&target) {
        Ok(_) => state.daemon.resolve(target).await?.content_id,
        Err(e) if target.contains(".jolt") => {
            return Err(AppApiError::Network(NetworkError::InvalidInput(
                e.to_string(),
            )));
        }
        Err(_) => target,
    };
    let result = state
        .daemon
        .fetch(content_id.clone())
        .await
        .map_err(|err| AppApiError::Network(fetch_error_for_target(err, &content_id)))?;
    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
pub struct EncryptedPublishRequest {
    pub path: String,
    pub plaintext: Vec<u8>,
    pub content_type: Option<String>,
    pub recipients: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct EncryptedPublishResponse {
    pub content_id: String,
    pub size: u64,
    pub path: Option<String>,
    pub address: Option<String>,
    pub latest_sequence: Option<u64>,
    pub recipient_count: usize,
}

#[derive(Debug, Deserialize)]
pub struct EncryptedDecryptRequest {
    pub target: String,
    pub path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EncryptedDecryptResponse {
    pub content_id: String,
    pub path: String,
    pub plaintext: Vec<u8>,
    pub size: u64,
    pub content_type: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EncryptedOpenStatus {
    Decrypted,
    Ciphertext,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EncryptedAccessStatus {
    Available,
    NeedsRewrap,
    NotAccessible,
}

#[derive(Debug, Serialize)]
pub struct EncryptedOpenResponse {
    pub content_id: String,
    pub path: String,
    pub status: EncryptedOpenStatus,
    pub access_status: EncryptedAccessStatus,
    pub plaintext: Option<Vec<u8>>,
    pub ciphertext: Option<Vec<u8>>,
    pub size: u64,
    pub content_type: Option<String>,
    pub decrypt_error: Option<String>,
}

pub async fn publish_encrypted(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<EncryptedPublishRequest>,
) -> Result<Json<EncryptedPublishResponse>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;
    let (path, encrypted) = encrypt_app_request(&state, &session, req).await?;
    let published = publish_app_bytes(&state, encrypted.data, path).await?;

    Ok(Json(EncryptedPublishResponse {
        content_id: published.content_id,
        size: published.size,
        path: published.path,
        address: published.address,
        latest_sequence: published.latest_sequence,
        recipient_count: encrypted.recipient_count,
    }))
}

async fn encrypt_app_request(
    state: &AppState,
    session: &AppSessionView,
    req: EncryptedPublishRequest,
) -> Result<(String, EncryptedObjectResponse), AppApiError> {
    let path = normalize_path(&req.path)?;
    require_path_capability(session, "encrypt:", &path)?;
    require_path_capability(session, "publish:encrypted:", &path)?;

    let recipient_keys = resolve_app_encryption_recipients(state, &req.recipients).await?;
    let encrypted = state
        .daemon
        .encrypt_object(
            req.plaintext,
            req.content_type
                .unwrap_or_else(|| "application/octet-stream".to_string()),
            recipient_keys,
        )
        .await?;
    Ok((path, encrypted))
}

pub async fn append_encrypted(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<EncryptedPublishRequest>,
) -> Result<Json<EncryptedPublishResponse>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;
    let (path, encrypted) = encrypt_app_request(&state, &session, req).await?;
    let published = publish_app_append_bytes(&state, encrypted.data, path).await?;

    Ok(Json(EncryptedPublishResponse {
        content_id: published.content_id,
        size: published.size,
        path: published.path,
        address: published.address,
        latest_sequence: published.latest_sequence,
        recipient_count: encrypted.recipient_count,
    }))
}

async fn encrypted_target(
    state: &AppState,
    req: &EncryptedDecryptRequest,
    operation: &str,
) -> Result<(String, String), AppApiError> {
    match JoltAddress::from_str(&req.target) {
        Ok(address) => {
            let resolved = state.daemon.resolve(address.to_string()).await?;
            if let Some(raw_path) = &req.path {
                let requested_path = normalize_path(raw_path)?;
                if requested_path != resolved.path {
                    return Err(AppApiError::Network(NetworkError::InvalidInput(format!(
                        "encrypted {operation} path {requested_path} does not match target path {}",
                        resolved.path
                    ))));
                }
            }
            Ok((resolved.content_id, resolved.path))
        }
        Err(e) if req.target.contains(".jolt") => {
            Err(AppApiError::Network(NetworkError::InvalidInput(format!(
                "encrypted {operation} target must be a valid .jolt address or content id: {e}"
            ))))
        }
        Err(_) => {
            let path = req.path.as_deref().ok_or_else(|| {
                AppApiError::Network(NetworkError::InvalidInput(format!(
                    "encrypted {operation} by content id requires a path"
                )))
            })?;
            Ok((req.target.clone(), normalize_path(path)?))
        }
    }
}

pub async fn decrypt_encrypted(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<EncryptedDecryptRequest>,
) -> Result<Json<EncryptedDecryptResponse>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;

    let (content_id, path) = encrypted_target(&state, &req, "decrypt").await?;
    require_path_capability(&session, "decrypt:", &path)?;
    let fetched = state
        .daemon
        .fetch(content_id.clone())
        .await
        .map_err(|err| AppApiError::Network(fetch_error_for_target(err, &content_id)))?;
    let decrypted = state.daemon.decrypt_object(fetched.data).await?;

    Ok(Json(EncryptedDecryptResponse {
        content_id,
        path,
        plaintext: decrypted.plaintext,
        size: decrypted.size,
        content_type: decrypted.content_type,
    }))
}

pub async fn open_encrypted(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<EncryptedDecryptRequest>,
) -> Result<Json<EncryptedOpenResponse>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;

    let (content_id, path) = encrypted_target(&state, &req, "open").await?;
    require_path_capability(&session, "decrypt:", &path)?;
    let fetched = state
        .daemon
        .fetch(content_id.clone())
        .await
        .map_err(|err| AppApiError::Network(fetch_error_for_target(err, &content_id)))?;
    let ciphertext = fetched.data;
    let ciphertext_size = fetched.size;
    let access_status = encrypted_access_status(&state, &ciphertext).await?;

    match state.daemon.decrypt_object(ciphertext.clone()).await {
        Ok(decrypted) => Ok(Json(EncryptedOpenResponse {
            content_id,
            path,
            status: EncryptedOpenStatus::Decrypted,
            access_status,
            plaintext: Some(decrypted.plaintext),
            ciphertext: None,
            size: decrypted.size,
            content_type: Some(decrypted.content_type),
            decrypt_error: None,
        })),
        Err(err) => Ok(Json(EncryptedOpenResponse {
            content_id,
            path,
            status: EncryptedOpenStatus::Ciphertext,
            access_status: EncryptedAccessStatus::NotAccessible,
            plaintext: None,
            ciphertext: Some(ciphertext),
            size: ciphertext_size,
            content_type: None,
            decrypt_error: Some(err.to_string()),
        })),
    }
}

pub async fn rewrap_encrypted(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<EncryptedDecryptRequest>,
) -> Result<Json<EncryptedPublishResponse>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;

    let (content_id, path) = encrypted_target(&state, &req, "rewrap").await?;
    require_path_capability(&session, "decrypt:", &path)?;
    require_path_capability(&session, "encrypt:", &path)?;
    require_path_capability(&session, "publish:encrypted:", &path)?;

    let fetched = state
        .daemon
        .fetch(content_id.clone())
        .await
        .map_err(|err| AppApiError::Network(fetch_error_for_target(err, &content_id)))?;
    let decrypted = state.daemon.decrypt_object(fetched.data).await?;
    let recipient_keys = resolve_app_encryption_recipients(&state, &[]).await?;
    let encrypted = state
        .daemon
        .encrypt_object(
            decrypted.plaintext,
            decrypted.content_type.clone(),
            recipient_keys,
        )
        .await?;
    let published = publish_app_bytes(&state, encrypted.data, path).await?;

    Ok(Json(EncryptedPublishResponse {
        content_id: published.content_id,
        size: published.size,
        path: published.path,
        address: published.address,
        latest_sequence: published.latest_sequence,
        recipient_count: encrypted.recipient_count,
    }))
}

async fn encrypted_access_status(
    state: &AppState,
    encrypted_object: &[u8],
) -> Result<EncryptedAccessStatus, AppApiError> {
    let envelope = EncryptedObjectEnvelope::from_bytes(encrypted_object)
        .map_err(|err| AppApiError::Network(NetworkError::InvalidInput(err.to_string())))?;
    let active_device_recipients = state
        .device_authority
        .active_authorized_device_encryption_recipients(&state.daemon)
        .await
        .map_err(|err| AppApiError::Network(NetworkError::Protocol(err.to_string())))?;

    let missing_active_device_key = active_device_recipients.iter().any(|active| {
        !envelope.body.recipients.iter().any(|recipient| {
            recipient.recipient_identity == active.identity
                && recipient.recipient_key_id == active.key.key_id
        })
    });

    Ok(if missing_active_device_key {
        EncryptedAccessStatus::NeedsRewrap
    } else {
        EncryptedAccessStatus::Available
    })
}

pub async fn publish_file(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;

    let (data, path) = read_file_and_path_fields(multipart).await?;
    require_path_capability(&session, "publish:", &path)?;

    let temp_path = persist_app_temp_file("publish", &data)?;
    let result = state.daemon.publish(temp_path.clone(), Some(path)).await;
    let _ = std::fs::remove_file(&temp_path);

    match result {
        Ok(response) => Ok((StatusCode::OK, Json(json!(response))).into_response()),
        Err(e) => Err(AppApiError::Network(e)),
    }
}

/// Publish content as an append record bound to a path the app owns. Unlike
/// `publish_file`, append records coexist: this is the write seam for elements
/// of a growing collection, where every element must survive concurrent writes
/// rather than overwriting a single current value.
pub async fn append_record(
    State(state): State<AppState>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Response, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;

    let (data, path) = read_file_and_path_fields(multipart).await?;
    require_path_capability(&session, "publish:", &path)?;

    let temp_path = persist_app_temp_file("append", &data)?;
    let result = state.daemon.publish_append(temp_path.clone(), path).await;
    let _ = std::fs::remove_file(&temp_path);

    match result {
        Ok(response) => Ok((StatusCode::OK, Json(json!(response))).into_response()),
        Err(e) => Err(AppApiError::Network(e)),
    }
}

/// Read the `file` and `path` fields shared by the multipart publish endpoints.
/// The path is normalized and required; the file bytes are required.
async fn read_file_and_path_fields(
    mut multipart: Multipart,
) -> Result<(Vec<u8>, String), AppApiError> {
    let mut file_data = None;
    let mut path = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name() {
            Some("file") => {
                let bytes = field.bytes().await.map_err(|e| {
                    AppApiError::Network(NetworkError::InvalidInput(format!(
                        "failed to read file field: {e}"
                    )))
                })?;
                file_data = Some(bytes.to_vec());
            }
            Some("path") => {
                let value = field.text().await.map_err(|e| {
                    AppApiError::Network(NetworkError::InvalidInput(format!(
                        "failed to read path field: {e}"
                    )))
                })?;
                if !value.trim().is_empty() {
                    path = Some(normalize_path(&value)?);
                }
            }
            _ => {}
        }
    }

    let path = path.ok_or_else(|| {
        AppApiError::Network(NetworkError::InvalidInput("a path is required".to_string()))
    })?;
    let data = file_data.ok_or_else(|| {
        AppApiError::Network(NetworkError::InvalidInput(
            "no file field in multipart request".to_string(),
        ))
    })?;
    Ok((data, path))
}

/// Write `data` to a uniquely named temp file the daemon can read. The caller
/// removes it after publishing.
fn persist_app_temp_file(label: &str, data: &[u8]) -> Result<std::path::PathBuf, AppApiError> {
    let temp_dir = std::env::temp_dir();
    let unique_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = APP_TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp_path = temp_dir.join(format!(
        "jolt_app_{label}_{}_{unique_id}_{sequence}",
        std::process::id(),
    ));
    std::fs::write(&temp_path, data).map_err(|e| AppApiError::Network(NetworkError::Io(e)))?;
    Ok(temp_path)
}

#[derive(Debug, Deserialize)]
pub struct EnumerateRequest {
    pub identity: String,
    pub path_prefix: String,
}

/// List an identity's append records whose path starts with `path_prefix`. This
/// is the read seam a Collection is assembled from. Reads cached merged
/// device-writer state, never a rewritten blob.
pub async fn enumerate_append_records(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<EnumerateRequest>,
) -> Result<Json<Vec<AppendRecordInfo>>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_capability(&session, "resolve:public")?;

    let identity = recipient_identity(&req.identity)?;
    let prefix = normalize_path(&req.path_prefix)?;
    let records = state
        .daemon
        .enumerate_append_records(identity, prefix)
        .await?;
    Ok(Json(records))
}

async fn publish_app_bytes(
    state: &AppState,
    data: Vec<u8>,
    path: String,
) -> Result<PublishResponse, AppApiError> {
    let temp_path = persist_app_temp_file("publish", &data)?;
    let result = state.daemon.publish(temp_path.clone(), Some(path)).await;
    let _ = std::fs::remove_file(&temp_path);
    result.map_err(AppApiError::Network)
}

async fn publish_app_append_bytes(
    state: &AppState,
    data: Vec<u8>,
    path: String,
) -> Result<PublishResponse, AppApiError> {
    let temp_path = persist_app_temp_file("append", &data)?;
    let result = state.daemon.publish_append(temp_path.clone(), path).await;
    let _ = std::fs::remove_file(&temp_path);
    result.map_err(AppApiError::Network)
}

async fn resolve_app_encryption_recipients(
    state: &AppState,
    recipients: &[String],
) -> Result<Vec<EncryptedObjectRecipient>, AppApiError> {
    let local_key = state.daemon.ensure_local_identity_encryption_key().await?;
    let local_identity = JoltAddress::from_str(&state.daemon.status().await?.identity_address)
        .map_err(|e| AppApiError::Network(NetworkError::Protocol(e.to_string())))?
        .identity()
        .clone();
    let local_device_keys = state
        .device_authority
        .active_authorized_device_encryption_recipients(&state.daemon)
        .await
        .map_err(|err| AppApiError::Network(NetworkError::Protocol(err.to_string())))?;
    resolve_recipient_keys(
        state,
        recipients,
        &local_identity,
        local_key,
        local_device_keys,
    )
    .await
}

async fn resolve_recipient_keys(
    state: &AppState,
    recipients: &[String],
    local_identity: &IdentityId,
    local_key: IdentityEncryptionKey,
    local_device_keys: Vec<EncryptedObjectRecipient>,
) -> Result<Vec<EncryptedObjectRecipient>, AppApiError> {
    let mut keys = Vec::with_capacity(recipients.len() + local_device_keys.len() + 1);
    let mut includes_local_identity = false;
    for recipient in recipients {
        let identity = recipient_identity(recipient)?;
        if &identity == local_identity {
            includes_local_identity = true;
            push_local_recipient_keys(&mut keys, local_identity, &local_key, &local_device_keys);
            continue;
        }

        let address = JoltAddress::new(identity.clone(), IDENTITY_ENCRYPTION_KEYS_PATH)
            .map_err(|e| AppApiError::Network(NetworkError::InvalidInput(e.to_string())))?;
        let resolved = state.daemon.resolve(address.to_string()).await?;
        let fetched = state
            .daemon
            .fetch(resolved.content_id.clone())
            .await
            .map_err(|err| {
                AppApiError::Network(fetch_error_for_target(err, &resolved.content_id))
            })?;
        let record: IdentityEncryptionKeyRecord = serde_json::from_slice(&fetched.data)
            .map_err(|e| AppApiError::Network(NetworkError::InvalidInput(e.to_string())))?;
        let verified =
            verify_identity_encryption_key_record_for_identity(&identity, &record, unix_now())
                .map_err(|e| AppApiError::Network(NetworkError::InvalidInput(e.to_string())))?;
        keys.extend(
            verified
                .keys
                .into_iter()
                .map(|key| EncryptedObjectRecipient {
                    identity: identity.clone(),
                    key,
                }),
        );
    }

    if !includes_local_identity {
        push_local_recipient_keys(&mut keys, local_identity, &local_key, &local_device_keys);
    }

    Ok(keys)
}

fn push_local_recipient_keys(
    keys: &mut Vec<EncryptedObjectRecipient>,
    local_identity: &IdentityId,
    local_key: &IdentityEncryptionKey,
    local_device_keys: &[EncryptedObjectRecipient],
) {
    push_unique_recipient(
        keys,
        EncryptedObjectRecipient {
            identity: local_identity.clone(),
            key: local_key.clone(),
        },
    );
    for recipient in local_device_keys {
        push_unique_recipient(keys, recipient.clone());
    }
}

fn push_unique_recipient(
    keys: &mut Vec<EncryptedObjectRecipient>,
    recipient: EncryptedObjectRecipient,
) {
    if keys.iter().any(|existing| {
        existing.identity == recipient.identity && existing.key.key_id == recipient.key.key_id
    }) {
        return;
    }
    keys.push(recipient);
}

fn recipient_identity(raw: &str) -> Result<IdentityId, AppApiError> {
    match JoltAddress::from_str(raw) {
        Ok(address) => Ok(address.identity().clone()),
        Err(_) => IdentityId::from_str(raw.strip_suffix(".jolt").unwrap_or(raw)).map_err(|e| {
            AppApiError::Network(NetworkError::InvalidInput(format!(
                "invalid recipient identity {raw}: {e}"
            )))
        }),
    }
}

pub async fn list_published(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<PublishedContentInfo>>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;
    let prefixes = capability_patterns(&session, "inventory:");
    if prefixes.is_empty() {
        return Err(AppApiError::Forbidden(
            "missing required capability: inventory:<path>".to_string(),
        ));
    }

    let entries = state
        .daemon
        .list_published_content()
        .await?
        .into_iter()
        .filter(|entry| {
            entry
                .path
                .as_deref()
                .is_some_and(|path| prefixes.iter().any(|pattern| path_matches(pattern, path)))
        })
        .collect();
    Ok(Json(entries))
}

pub async fn pin_home_relay(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<HomeRelayPinRequest>,
) -> Result<impl IntoResponse, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;
    let entries = state.daemon.list_published_content().await?;
    let Some(entry) = entries
        .iter()
        .find(|entry| entry.content_id == request.content_id)
    else {
        return Err(AppApiError::Forbidden(
            "app can only pin own published content".to_string(),
        ));
    };
    let Some(path) = entry.path.as_deref() else {
        return Err(AppApiError::Forbidden(
            "app can only pin path-bound content".to_string(),
        ));
    };
    require_path_capability(&session, "pin:own:", path)?;

    crate::routes::home_relay::pin(State(state), Json(request))
        .await
        .map_err(AppApiError::from)
}

pub(crate) async fn authenticated_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AppSessionView, AppApiError> {
    let token = bearer_token(headers)?.to_string();
    Ok(state.sessions.session_for_token(&token).await?)
}

pub(crate) async fn require_local_identity(
    state: &AppState,
    session: &AppSessionView,
) -> Result<(), AppApiError> {
    let granted = session
        .identity
        .as_deref()
        .ok_or_else(|| AppApiError::Forbidden("app session has no granted identity".to_string()))?;
    let local = state.daemon.status().await?.identity_address;
    if granted == local {
        Ok(())
    } else {
        Err(AppApiError::Forbidden(format!(
            "session identity {granted} does not match local daemon identity {local}"
        )))
    }
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, AppSessionStoreError> {
    let value = headers
        .get("authorization")
        .ok_or(AppSessionStoreError::MissingBearerToken)?;
    let raw = value
        .to_str()
        .map_err(|_| AppSessionStoreError::InvalidToken)?;
    let token = raw
        .strip_prefix("Bearer ")
        .ok_or(AppSessionStoreError::InvalidToken)?;
    if token.is_empty() {
        return Err(AppSessionStoreError::InvalidToken);
    }
    Ok(token)
}

pub(crate) fn require_capability(
    session: &AppSessionView,
    capability: &str,
) -> Result<(), AppApiError> {
    if session
        .granted_capabilities
        .iter()
        .any(|granted| granted == capability)
    {
        Ok(())
    } else {
        Err(AppApiError::Forbidden(format!(
            "missing required capability: {capability}"
        )))
    }
}

fn require_path_capability(
    session: &AppSessionView,
    capability_prefix: &str,
    path: &str,
) -> Result<(), AppApiError> {
    if capability_patterns(session, capability_prefix)
        .iter()
        .any(|pattern| path_matches(pattern, path))
    {
        Ok(())
    } else {
        Err(AppApiError::Forbidden(format!(
            "path {path} is outside granted capability {capability_prefix}<path>"
        )))
    }
}

fn capability_patterns(session: &AppSessionView, capability_prefix: &str) -> Vec<String> {
    session
        .granted_capabilities
        .iter()
        .filter_map(|capability| capability.strip_prefix(capability_prefix))
        .map(str::to_string)
        .collect()
}

fn path_matches(pattern: &str, path: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        path.starts_with(prefix)
    } else {
        path == pattern
    }
}

fn normalize_path(path: &str) -> Result<String, AppApiError> {
    let path = path.trim();
    if path.is_empty() {
        return Err(AppApiError::Network(NetworkError::InvalidInput(
            "path must not be empty".to_string(),
        )));
    }
    if path.contains('?') || path.contains('#') || path.chars().any(char::is_whitespace) {
        return Err(AppApiError::Network(NetworkError::InvalidInput(
            "path must not contain whitespace, query, or fragment".to_string(),
        )));
    }

    let normalized = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    if normalized
        .split('/')
        .any(|segment| segment == "." || segment == "..")
    {
        return Err(AppApiError::Network(NetworkError::InvalidInput(
            "path must not contain . or .. segments".to_string(),
        )));
    }
    Ok(normalized)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub enum AppApiError {
    Session(AppSessionStoreError),
    Forbidden(String),
    Network(NetworkError),
}

impl IntoResponse for AppApiError {
    fn into_response(self) -> Response {
        match self {
            AppApiError::Session(err) => {
                crate::routes::app_sessions::AppSessionApiError::from(err).into_response()
            }
            AppApiError::Forbidden(message) => (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "code": "app_session_forbidden",
                    "error": message,
                })),
            )
                .into_response(),
            AppApiError::Network(err) => ApiError(err).into_response(),
        }
    }
}

impl From<AppSessionStoreError> for AppApiError {
    fn from(err: AppSessionStoreError) -> Self {
        Self::Session(err)
    }
}

impl From<NetworkError> for AppApiError {
    fn from(err: NetworkError) -> Self {
        Self::Network(err)
    }
}

impl From<ApiError> for AppApiError {
    fn from(err: ApiError) -> Self {
        Self::Network(err.0)
    }
}
