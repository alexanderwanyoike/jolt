use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use dweb_core::{ContentId, ContentManifest, IdentityId, UpdateLogEntry};

use crate::cache_entry::{CacheEntry, CacheIndex, CacheStats, ContentData};
use crate::config::CacheConfig;
use crate::error::StoreError;

pub struct ContentStore {
    published_dir: PathBuf,
    cache_dir: PathBuf,
    update_logs_dir: PathBuf,
    home_relay_pins_path: PathBuf,
    discovered_peer_hints_path: PathBuf,
    cache_index: CacheIndex,
    cache_config: CacheConfig,
    /// In-memory index of published content: content_id -> path to content file
    published_content: HashMap<String, PathBuf>,
}

const MAX_DISCOVERED_PEER_HINTS: usize = 64;
const DISCOVERED_PEER_HINT_TTL_SECS: u64 = 30 * 24 * 60 * 60;
const MAX_DISCOVERED_PEER_FAILURES: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedContentEntry {
    pub content_id: String,
    pub size: u64,
    pub content_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeRelayPinRecord {
    pub content_id: String,
    pub path: Option<String>,
    pub address: Option<String>,
    pub relay_peer_id: String,
    pub relay_multiaddr: String,
    pub relay_api_url: Option<String>,
    pub latest_sequence: u64,
    pub pinned_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredPeerHint {
    pub multiaddr: String,
    pub source: String,
    pub first_seen: u64,
    pub last_seen: u64,
    #[serde(default)]
    pub failure_count: u32,
}

impl ContentStore {
    /// Open or create a content store at the given base directory.
    pub fn open(base_dir: &Path, config: CacheConfig) -> Result<Self, StoreError> {
        let published_dir = base_dir.join("published");
        let cache_dir = base_dir.join("cache");
        let update_logs_dir = base_dir.join("update_logs");
        let home_relay_pins_path = base_dir.join("home_relay_pins.json");
        let discovered_peer_hints_path = base_dir.join("discovered_peer_hints.json");

        std::fs::create_dir_all(&published_dir)?;
        std::fs::create_dir_all(&cache_dir)?;
        std::fs::create_dir_all(&update_logs_dir)?;

        let cache_index = Self::load_index(&cache_dir)?;
        let published_content = Self::scan_published(&published_dir)?;

        info!(
            "Content store opened: {} published, {} cached",
            published_content.len(),
            cache_index.entries.len()
        );

        Ok(Self {
            published_dir,
            cache_dir,
            update_logs_dir,
            home_relay_pins_path,
            discovered_peer_hints_path,
            cache_index,
            cache_config: config,
            published_content,
        })
    }

    /// Publish content (user's own content). Stores in published/ directory.
    pub fn publish(
        &mut self,
        data: &[u8],
        manifest: &ContentManifest,
    ) -> Result<ContentId, StoreError> {
        let id_str = manifest.content_id.to_string();
        let content_dir = self.published_dir.join(&id_str);
        std::fs::create_dir_all(&content_dir)?;

        let content_path = content_dir.join("content");
        std::fs::write(&content_path, data)?;

        let manifest_json = serde_json::to_string_pretty(manifest)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        std::fs::write(content_dir.join("manifest.json"), manifest_json)?;

        self.published_content.insert(id_str, content_path);

        Ok(manifest.content_id.clone())
    }

    /// Cache content fetched from the network. Stores in cache/ directory.
    pub fn cache_content(
        &mut self,
        content_id: &str,
        data: &[u8],
        publisher_key: &[u8],
        signature: &[u8],
    ) -> Result<(), StoreError> {
        // Don't cache if we already have it (published or cached)
        if self.has_content(content_id) {
            return Ok(());
        }

        let size = data.len() as u64;

        // Evict if needed
        self.evict_if_needed(size)?;

        let content_dir = self.cache_dir.join(content_id);
        std::fs::create_dir_all(&content_dir)?;

        std::fs::write(content_dir.join("content"), data)?;

        // Store manifest for re-serving
        let manifest_data = serde_json::json!({
            "publisher_key": publisher_key,
            "signature": signature,
        });
        std::fs::write(
            content_dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest_data)
                .map_err(|e| StoreError::Serialization(e.to_string()))?,
        )?;

        let now = now_unix();
        let entry = CacheEntry {
            content_id: content_id.to_string(),
            size,
            cached_at: now,
            last_accessed: now,
            pinned: false,
            publisher_key: publisher_key.to_vec(),
            signature: signature.to_vec(),
        };

        self.cache_index.total_size += size;
        self.cache_index
            .entries
            .insert(content_id.to_string(), entry);
        self.save_index()?;

        debug!("Cached content: {content_id} ({size} bytes)");
        Ok(())
    }

    /// Get content by ID. Checks published first, then cache.
    /// Updates last_accessed for cache hits.
    pub fn get_content(&mut self, content_id: &str) -> Option<ContentData> {
        // Check published first
        if let Some(content_path) = self.published_content.get(content_id) {
            if let Ok(data) = std::fs::read(content_path) {
                // Read manifest for publisher_key + signature
                let manifest_path = content_path.parent().unwrap().join("manifest.json");
                if let Ok(manifest_str) = std::fs::read_to_string(&manifest_path) {
                    if let Ok(manifest) = serde_json::from_str::<ContentManifest>(&manifest_str) {
                        return Some(ContentData {
                            data,
                            publisher_key: manifest.publisher_key,
                            signature: manifest.signature,
                        });
                    }
                }
            }
        }

        // Check cache
        if self.cache_index.entries.contains_key(content_id) {
            let content_path = self.cache_dir.join(content_id).join("content");
            if let Ok(data) = std::fs::read(&content_path) {
                let entry = self.cache_index.entries.get_mut(content_id).unwrap();
                entry.last_accessed = now_unix();
                let result = ContentData {
                    data,
                    publisher_key: entry.publisher_key.clone(),
                    signature: entry.signature.clone(),
                };
                // Best-effort save index for access time update
                let _ = self.save_index();
                return Some(result);
            }
        }

        None
    }

    /// Check if content exists (published or cached) without updating access time.
    pub fn has_content(&self, content_id: &str) -> bool {
        self.published_content.contains_key(content_id)
            || self.cache_index.entries.contains_key(content_id)
    }

    /// List all published content IDs.
    pub fn published_ids(&self) -> Vec<String> {
        self.published_content.keys().cloned().collect()
    }

    /// List all locally published content with manifest metadata.
    pub fn list_published_content(&self) -> Vec<PublishedContentEntry> {
        let mut entries = Vec::new();
        for (content_id, content_path) in &self.published_content {
            let manifest_path = content_path.parent().unwrap().join("manifest.json");
            let Ok(manifest_str) = std::fs::read_to_string(manifest_path) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_str::<ContentManifest>(&manifest_str) else {
                continue;
            };
            entries.push(PublishedContentEntry {
                content_id: content_id.clone(),
                size: manifest.size,
                content_type: manifest.content_type,
            });
        }
        entries.sort_by(|a, b| a.content_id.cmp(&b.content_id));
        entries
    }

    /// Persist the authoritative update log for a local identity.
    pub fn save_update_log(
        &self,
        identity: &IdentityId,
        entries: &[UpdateLogEntry],
    ) -> Result<(), StoreError> {
        let path = self.update_log_path(identity);
        let tmp_path = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(entries)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(tmp_path, path)?;
        Ok(())
    }

    /// Load the authoritative update log for a local identity, if one exists.
    pub fn load_update_log(
        &self,
        identity: &IdentityId,
    ) -> Result<Option<Vec<UpdateLogEntry>>, StoreError> {
        let path = self.update_log_path(identity);
        if !path.exists() {
            return Ok(None);
        }

        let json = std::fs::read_to_string(path)?;
        let entries =
            serde_json::from_str(&json).map_err(|e| StoreError::Serialization(e.to_string()))?;
        Ok(Some(entries))
    }

    /// Persist a successful owner-directed home relay pin.
    pub fn save_home_relay_pin_record(&self, record: HomeRelayPinRecord) -> Result<(), StoreError> {
        let mut records = self.load_home_relay_pin_records()?;
        records.push(record);
        let tmp_path = self.home_relay_pins_path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(&records)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(tmp_path, &self.home_relay_pins_path)?;
        Ok(())
    }

    /// Load successful owner-directed home relay pins known by this node.
    pub fn load_home_relay_pin_records(&self) -> Result<Vec<HomeRelayPinRecord>, StoreError> {
        if !self.home_relay_pins_path.exists() {
            return Ok(Vec::new());
        }
        let json = std::fs::read_to_string(&self.home_relay_pins_path)?;
        serde_json::from_str(&json).map_err(|e| StoreError::Serialization(e.to_string()))
    }

    /// Persist a best-effort peer/relay address hint learned from the network.
    pub fn record_discovered_peer_hint(
        &self,
        multiaddr: &str,
        source: &str,
    ) -> Result<(), StoreError> {
        let now = now_unix();
        let mut hints = self.load_all_discovered_peer_hints()?;

        if let Some(existing) = hints.iter_mut().find(|hint| hint.multiaddr == multiaddr) {
            existing.source = source.to_string();
            existing.last_seen = now;
            existing.failure_count = 0;
        } else {
            hints.push(DiscoveredPeerHint {
                multiaddr: multiaddr.to_string(),
                source: source.to_string(),
                first_seen: now,
                last_seen: now,
                failure_count: 0,
            });
        }

        hints.sort_by_key(|hint| std::cmp::Reverse(hint.last_seen));
        hints.truncate(MAX_DISCOVERED_PEER_HINTS);
        self.save_discovered_peer_hints(&hints)
    }

    /// Load usable discovered peer hints. Stale or repeatedly failed hints are ignored.
    pub fn load_discovered_peer_hints(&self) -> Result<Vec<DiscoveredPeerHint>, StoreError> {
        let now = now_unix();
        Ok(self
            .load_all_discovered_peer_hints()?
            .into_iter()
            .filter(|hint| {
                hint.failure_count < MAX_DISCOVERED_PEER_FAILURES
                    && now.saturating_sub(hint.last_seen) <= DISCOVERED_PEER_HINT_TTL_SECS
            })
            .collect())
    }

    /// Mark a discovered hint as unreachable. After repeated failures it is ignored.
    pub fn mark_discovered_peer_hint_failure(&self, multiaddr: &str) -> Result<(), StoreError> {
        let mut hints = self.load_all_discovered_peer_hints()?;
        if let Some(existing) = hints.iter_mut().find(|hint| hint.multiaddr == multiaddr) {
            existing.failure_count = existing.failure_count.saturating_add(1);
            self.save_discovered_peer_hints(&hints)?;
        }
        Ok(())
    }

    fn load_all_discovered_peer_hints(&self) -> Result<Vec<DiscoveredPeerHint>, StoreError> {
        if !self.discovered_peer_hints_path.exists() {
            return Ok(Vec::new());
        }
        let json = std::fs::read_to_string(&self.discovered_peer_hints_path)?;
        serde_json::from_str(&json).map_err(|e| StoreError::Serialization(e.to_string()))
    }

    fn save_discovered_peer_hints(&self, hints: &[DiscoveredPeerHint]) -> Result<(), StoreError> {
        let tmp_path = self.discovered_peer_hints_path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(hints)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        std::fs::write(&tmp_path, json)?;
        std::fs::rename(tmp_path, &self.discovered_peer_hints_path)?;
        Ok(())
    }

    /// List all cached content IDs.
    pub fn cached_ids(&self) -> Vec<String> {
        self.cache_index.entries.keys().cloned().collect()
    }

    /// Pin a cached content item (prevent eviction).
    pub fn pin(&mut self, content_id: &str) -> Result<(), StoreError> {
        let entry = self
            .cache_index
            .entries
            .get_mut(content_id)
            .ok_or_else(|| StoreError::ContentNotFound(content_id.to_string()))?;
        entry.pinned = true;
        self.save_index()?;
        Ok(())
    }

    /// Unpin a cached content item.
    pub fn unpin(&mut self, content_id: &str) -> Result<(), StoreError> {
        let entry = self
            .cache_index
            .entries
            .get_mut(content_id)
            .ok_or_else(|| StoreError::ContentNotFound(content_id.to_string()))?;
        entry.pinned = false;
        self.save_index()?;
        Ok(())
    }

    /// Get aggregate statistics.
    pub fn stats(&self) -> CacheStats {
        let mut published_size: u64 = 0;
        for path in self.published_content.values() {
            if let Ok(meta) = std::fs::metadata(path) {
                published_size += meta.len();
            }
        }

        let pinned_items = self
            .cache_index
            .entries
            .values()
            .filter(|e| e.pinned)
            .count();
        let pinned_size: u64 = self
            .cache_index
            .entries
            .values()
            .filter(|e| e.pinned)
            .map(|e| e.size)
            .sum();

        CacheStats {
            total_cached: self.cache_index.total_size,
            total_published: published_size,
            cached_items: self.cache_index.entries.len(),
            published_items: self.published_content.len(),
            pinned_items,
            pinned_size,
            max_size: self.cache_config.max_size_bytes,
            available: self
                .cache_config
                .max_size_bytes
                .saturating_sub(self.cache_index.total_size),
        }
    }

    /// Get all cache entries for display.
    pub fn list_entries(&self) -> Vec<&CacheEntry> {
        self.cache_index.entries.values().collect()
    }

    /// Evict least recently accessed non-pinned items to free space.
    fn evict_if_needed(&mut self, required_bytes: u64) -> Result<(), StoreError> {
        if self.cache_index.total_size + required_bytes <= self.cache_config.max_size_bytes {
            return Ok(());
        }

        let space_needed =
            (self.cache_index.total_size + required_bytes) - self.cache_config.max_size_bytes;

        // Collect non-pinned entries sorted by last_accessed (oldest first)
        let mut evictable: Vec<(String, u64, u64)> = self
            .cache_index
            .entries
            .iter()
            .filter(|(_, e)| !e.pinned)
            .map(|(id, e)| (id.clone(), e.size, e.last_accessed))
            .collect();

        evictable.sort_by_key(|(_, _, accessed)| *accessed);

        let mut freed: u64 = 0;
        let mut to_remove = Vec::new();

        for (id, size, _) in &evictable {
            if freed >= space_needed {
                break;
            }
            to_remove.push(id.clone());
            freed += size;
        }

        if freed < space_needed {
            return Err(StoreError::CacheFull);
        }

        // Remove evicted entries
        for id in &to_remove {
            if let Some(entry) = self.cache_index.entries.remove(id) {
                self.cache_index.total_size -= entry.size;
                let content_dir = self.cache_dir.join(id);
                if let Err(e) = std::fs::remove_dir_all(&content_dir) {
                    warn!("Failed to remove evicted content dir: {e}");
                }
                debug!("Evicted cached content: {id} ({} bytes)", entry.size);
            }
        }

        self.save_index()?;
        Ok(())
    }

    fn save_index(&self) -> Result<(), StoreError> {
        let index_path = self.cache_dir.join("cache_index.json");
        let json = serde_json::to_string_pretty(&self.cache_index)
            .map_err(|e| StoreError::Serialization(e.to_string()))?;
        std::fs::write(index_path, json)?;
        Ok(())
    }

    fn load_index(cache_dir: &Path) -> Result<CacheIndex, StoreError> {
        let index_path = cache_dir.join("cache_index.json");
        if !index_path.exists() {
            return Ok(CacheIndex::default());
        }
        let json = std::fs::read_to_string(&index_path)?;
        let index: CacheIndex =
            serde_json::from_str(&json).map_err(|e| StoreError::Serialization(e.to_string()))?;
        Ok(index)
    }

    fn update_log_path(&self, identity: &IdentityId) -> PathBuf {
        self.update_logs_dir.join(format!("{identity}.json"))
    }

    fn scan_published(published_dir: &Path) -> Result<HashMap<String, PathBuf>, StoreError> {
        let mut map = HashMap::new();
        if !published_dir.exists() {
            return Ok(map);
        }
        for entry in std::fs::read_dir(published_dir)? {
            let entry = entry?;
            let content_path = entry.path().join("content");
            if content_path.exists() {
                let id_str = entry.file_name().to_string_lossy().to_string();
                map.insert(id_str, content_path);
            }
        }
        Ok(map)
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dweb_core::{UpdateAction, UpdateLogEntry, UpdateLogEntryBody};
    use tempfile::tempdir;

    fn make_manifest(data: &[u8], pubkey: &[u8], sig: &[u8]) -> ContentManifest {
        ContentManifest {
            content_id: ContentId::from_bytes(data),
            size: data.len() as u64,
            content_type: "application/octet-stream".to_string(),
            publisher_key: pubkey.to_vec(),
            signature: sig.to_vec(),
        }
    }

    fn small_config(max_bytes: u64) -> CacheConfig {
        CacheConfig {
            max_size_bytes: max_bytes,
        }
    }

    // --- Phase 1: ContentStore basics ---

    #[test]
    fn open_creates_directories() {
        let dir = tempdir().unwrap();
        let _store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        assert!(dir.path().join("published").exists());
        assert!(dir.path().join("cache").exists());
        assert!(dir.path().join("update_logs").exists());
    }

    #[test]
    fn empty_store_has_zero_stats() {
        let dir = tempdir().unwrap();
        let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let stats = store.stats();
        assert_eq!(stats.cached_items, 0);
        assert_eq!(stats.published_items, 0);
        assert_eq!(stats.total_cached, 0);
        assert_eq!(stats.total_published, 0);
    }

    #[test]
    fn publish_stores_content_on_disk() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let data = b"hello world";
        let manifest = make_manifest(data, &[1, 2, 3], &[4, 5, 6]);
        let id = store.publish(data, &manifest).unwrap();
        let content_path = dir
            .path()
            .join("published")
            .join(id.to_string())
            .join("content");
        assert!(content_path.exists());
        assert_eq!(std::fs::read(&content_path).unwrap(), data);
    }

    #[test]
    fn publish_stores_manifest_on_disk() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let data = b"manifest test";
        let manifest = make_manifest(data, &[1], &[2]);
        let id = store.publish(data, &manifest).unwrap();
        let manifest_path = dir
            .path()
            .join("published")
            .join(id.to_string())
            .join("manifest.json");
        assert!(manifest_path.exists());
    }

    #[test]
    fn published_content_appears_in_get_content() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let data = b"get test";
        let manifest = make_manifest(data, &[10, 20], &[30, 40]);
        let id = store.publish(data, &manifest).unwrap();
        let result = store.get_content(&id.to_string()).unwrap();
        assert_eq!(result.data, data);
        assert_eq!(result.publisher_key, vec![10, 20]);
        assert_eq!(result.signature, vec![30, 40]);
    }

    #[test]
    fn published_content_shows_in_stats() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let data = b"stats test data";
        let manifest = make_manifest(data, &[1], &[2]);
        store.publish(data, &manifest).unwrap();
        let stats = store.stats();
        assert_eq!(stats.published_items, 1);
        assert!(stats.total_published > 0);
    }

    #[test]
    fn cache_content_stores_on_disk() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let data = b"cached data";
        let id = ContentId::from_bytes(data);
        store
            .cache_content(&id.to_string(), data, &[1], &[2])
            .unwrap();
        let content_path = dir
            .path()
            .join("cache")
            .join(id.to_string())
            .join("content");
        assert!(content_path.exists());
        assert_eq!(std::fs::read(&content_path).unwrap(), data);
    }

    #[test]
    fn cached_content_appears_in_get_content() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let data = b"cached get test";
        let id = ContentId::from_bytes(data);
        store
            .cache_content(&id.to_string(), data, &[10], &[20])
            .unwrap();
        let result = store.get_content(&id.to_string()).unwrap();
        assert_eq!(result.data, data);
        assert_eq!(result.publisher_key, vec![10]);
        assert_eq!(result.signature, vec![20]);
    }

    #[test]
    fn get_content_returns_none_for_unknown() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        assert!(store.get_content("nonexistent").is_none());
    }

    #[test]
    fn has_content_works_for_published() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let data = b"has test";
        let manifest = make_manifest(data, &[1], &[2]);
        let id = store.publish(data, &manifest).unwrap();
        assert!(store.has_content(&id.to_string()));
    }

    #[test]
    fn has_content_works_for_cached() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let data = b"has cached";
        let id = ContentId::from_bytes(data);
        store
            .cache_content(&id.to_string(), data, &[1], &[2])
            .unwrap();
        assert!(store.has_content(&id.to_string()));
    }

    #[test]
    fn cache_shows_in_stats() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let data = b"cache stats test";
        let id = ContentId::from_bytes(data);
        store
            .cache_content(&id.to_string(), data, &[1], &[2])
            .unwrap();
        let stats = store.stats();
        assert_eq!(stats.cached_items, 1);
        assert_eq!(stats.total_cached, data.len() as u64);
    }

    #[test]
    fn get_content_prefers_published() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let data = b"prefer published";
        let manifest = make_manifest(data, &[1, 1], &[2, 2]);
        let id = store.publish(data, &manifest).unwrap();
        // Also cache with different keys
        store
            .cache_content(&id.to_string(), data, &[3, 3], &[4, 4])
            .unwrap();
        let result = store.get_content(&id.to_string()).unwrap();
        // Should get published version (publisher_key [1,1])
        assert_eq!(result.publisher_key, vec![1, 1]);
    }

    #[test]
    fn reopen_loads_published_content() {
        let dir = tempdir().unwrap();
        {
            let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
            let data = b"persistent published";
            let manifest = make_manifest(data, &[1], &[2]);
            store.publish(data, &manifest).unwrap();
        }
        let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let id = ContentId::from_bytes(b"persistent published");
        assert!(store.has_content(&id.to_string()));
    }

    #[test]
    fn reopen_loads_persisted_update_log() {
        let dir = tempdir().unwrap();
        let identity = IdentityId::from_public_key([7; 32]);
        let entries = vec![UpdateLogEntry {
            body: UpdateLogEntryBody {
                owner_public_key: vec![7; 32],
                sequence: 0,
                previous_entry_hash: None,
                action: UpdateAction::SetPath {
                    path: "/post".to_string(),
                    content_id: ContentId::from_bytes(b"persisted path"),
                },
            },
            signature: vec![8; 64],
        }];

        {
            let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
            store.save_update_log(&identity, &entries).unwrap();
        }

        let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        assert_eq!(store.load_update_log(&identity).unwrap(), Some(entries));
    }

    #[test]
    fn reopen_loads_cache_index() {
        let dir = tempdir().unwrap();
        let id_str;
        {
            let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
            let data = b"persistent cached";
            let id = ContentId::from_bytes(data);
            id_str = id.to_string();
            store.cache_content(&id_str, data, &[1], &[2]).unwrap();
        }
        let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        assert!(store.has_content(&id_str));
        assert_eq!(store.stats().cached_items, 1);
    }

    #[test]
    fn discovered_peer_hints_persist_reload_and_deduplicate() {
        let dir = tempdir().unwrap();
        let multiaddr =
            "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";

        {
            let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
            store
                .record_discovered_peer_hint(multiaddr, "bootstrap")
                .unwrap();
            store
                .record_discovered_peer_hint(multiaddr, "bootstrap")
                .unwrap();
        }

        let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let hints = store.load_discovered_peer_hints().unwrap();

        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].multiaddr, multiaddr);
        assert_eq!(hints[0].source, "bootstrap");
        assert_eq!(hints[0].failure_count, 0);
    }

    #[test]
    fn discovered_peer_hints_are_bounded() {
        let dir = tempdir().unwrap();
        let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();

        for port in 4000..4070 {
            store
                .record_discovered_peer_hint(
                    &format!(
                        "/ip4/127.0.0.1/tcp/{port}/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
                    ),
                    "routing",
                )
                .unwrap();
        }

        assert_eq!(store.load_discovered_peer_hints().unwrap().len(), 64);
    }

    #[test]
    fn discovered_peer_hints_are_ignored_after_repeated_failures() {
        let dir = tempdir().unwrap();
        let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let multiaddr =
            "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";

        store
            .record_discovered_peer_hint(multiaddr, "bootstrap")
            .unwrap();
        store.mark_discovered_peer_hint_failure(multiaddr).unwrap();
        store.mark_discovered_peer_hint_failure(multiaddr).unwrap();
        store.mark_discovered_peer_hint_failure(multiaddr).unwrap();

        assert!(store.load_discovered_peer_hints().unwrap().is_empty());
    }

    // --- Phase 2: LRU eviction ---

    #[test]
    fn eviction_removes_least_recently_accessed() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), small_config(100)).unwrap();

        let data_a = vec![0u8; 50];
        let id_a = ContentId::from_bytes(&data_a).to_string();
        store.cache_content(&id_a, &data_a, &[1], &[1]).unwrap();

        // Touch A's access time to be old
        store
            .cache_index
            .entries
            .get_mut(&id_a)
            .unwrap()
            .last_accessed = 1;

        let data_b = vec![1u8; 50];
        let id_b = ContentId::from_bytes(&data_b).to_string();
        store.cache_content(&id_b, &data_b, &[2], &[2]).unwrap();

        // Now cache C (50 bytes) -- should evict A (oldest accessed)
        let data_c = vec![2u8; 50];
        let id_c = ContentId::from_bytes(&data_c).to_string();
        store.cache_content(&id_c, &data_c, &[3], &[3]).unwrap();

        assert!(!store.has_content(&id_a), "A should be evicted");
        assert!(store.has_content(&id_b), "B should remain");
        assert!(store.has_content(&id_c), "C should be added");
    }

    #[test]
    fn eviction_removes_multiple_items() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), small_config(100)).unwrap();

        let data_a = vec![0u8; 40];
        let id_a = ContentId::from_bytes(&data_a).to_string();
        store.cache_content(&id_a, &data_a, &[1], &[1]).unwrap();
        store
            .cache_index
            .entries
            .get_mut(&id_a)
            .unwrap()
            .last_accessed = 1;

        let data_b = vec![1u8; 40];
        let id_b = ContentId::from_bytes(&data_b).to_string();
        store.cache_content(&id_b, &data_b, &[2], &[2]).unwrap();
        store
            .cache_index
            .entries
            .get_mut(&id_b)
            .unwrap()
            .last_accessed = 2;

        // Cache C (80 bytes) -- needs to evict both A and B to fit
        let data_c = vec![2u8; 80];
        let id_c = ContentId::from_bytes(&data_c).to_string();
        store.cache_content(&id_c, &data_c, &[3], &[3]).unwrap();

        assert!(!store.has_content(&id_a));
        assert!(!store.has_content(&id_b));
        assert!(store.has_content(&id_c));
    }

    #[test]
    fn eviction_does_not_remove_pinned() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), small_config(100)).unwrap();

        let data_a = vec![0u8; 50];
        let id_a = ContentId::from_bytes(&data_a).to_string();
        store.cache_content(&id_a, &data_a, &[1], &[1]).unwrap();
        store.pin(&id_a).unwrap();
        store
            .cache_index
            .entries
            .get_mut(&id_a)
            .unwrap()
            .last_accessed = 1;

        let data_b = vec![1u8; 50];
        let id_b = ContentId::from_bytes(&data_b).to_string();
        store.cache_content(&id_b, &data_b, &[2], &[2]).unwrap();

        // Cache C -- should evict B (unpinned), not A (pinned)
        let data_c = vec![2u8; 50];
        let id_c = ContentId::from_bytes(&data_c).to_string();
        store.cache_content(&id_c, &data_c, &[3], &[3]).unwrap();

        assert!(store.has_content(&id_a), "pinned A should remain");
        assert!(!store.has_content(&id_b), "unpinned B should be evicted");
        assert!(store.has_content(&id_c), "C should be added");
    }

    #[test]
    fn cache_fails_when_pinned_fills_cache() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), small_config(100)).unwrap();

        let data_a = vec![0u8; 60];
        let id_a = ContentId::from_bytes(&data_a).to_string();
        store.cache_content(&id_a, &data_a, &[1], &[1]).unwrap();
        store.pin(&id_a).unwrap();

        let data_b = vec![1u8; 60];
        let id_b = ContentId::from_bytes(&data_b).to_string();
        let result = store.cache_content(&id_b, &data_b, &[2], &[2]);
        assert!(matches!(result, Err(StoreError::CacheFull)));
        assert!(store.has_content(&id_a));
        assert!(!store.has_content(&id_b));
    }

    #[test]
    fn eviction_updates_stats() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), small_config(100)).unwrap();

        let data_a = vec![0u8; 50];
        let id_a = ContentId::from_bytes(&data_a).to_string();
        store.cache_content(&id_a, &data_a, &[1], &[1]).unwrap();
        store
            .cache_index
            .entries
            .get_mut(&id_a)
            .unwrap()
            .last_accessed = 1;

        let data_b = vec![1u8; 60];
        let id_b = ContentId::from_bytes(&data_b).to_string();
        store.cache_content(&id_b, &data_b, &[2], &[2]).unwrap();

        let stats = store.stats();
        assert_eq!(stats.cached_items, 1);
        assert_eq!(stats.total_cached, 60);
    }

    #[test]
    fn get_content_updates_last_accessed() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let data = b"access time test";
        let id = ContentId::from_bytes(data);
        let id_str = id.to_string();
        store.cache_content(&id_str, data, &[1], &[2]).unwrap();

        let before = store.cache_index.entries[&id_str].last_accessed;
        // Access it
        store.get_content(&id_str).unwrap();
        let after = store.cache_index.entries[&id_str].last_accessed;
        assert!(after >= before);
    }

    // --- Phase 3: Pinning ---

    #[test]
    fn pin_sets_pinned_true() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let data = b"pin test";
        let id = ContentId::from_bytes(data).to_string();
        store.cache_content(&id, data, &[1], &[2]).unwrap();
        store.pin(&id).unwrap();
        assert!(store.cache_index.entries[&id].pinned);
    }

    #[test]
    fn unpin_sets_pinned_false() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let data = b"unpin test";
        let id = ContentId::from_bytes(data).to_string();
        store.cache_content(&id, data, &[1], &[2]).unwrap();
        store.pin(&id).unwrap();
        store.unpin(&id).unwrap();
        assert!(!store.cache_index.entries[&id].pinned);
    }

    #[test]
    fn pin_nonexistent_returns_error() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let result = store.pin("nonexistent");
        assert!(matches!(result, Err(StoreError::ContentNotFound(_))));
    }

    #[test]
    fn pinned_items_show_in_stats() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let data = b"pinned stats test";
        let id = ContentId::from_bytes(data).to_string();
        store.cache_content(&id, data, &[1], &[2]).unwrap();
        store.pin(&id).unwrap();
        let stats = store.stats();
        assert_eq!(stats.pinned_items, 1);
        assert_eq!(stats.pinned_size, data.len() as u64);
    }

    #[test]
    fn pinned_content_survives_eviction() {
        let dir = tempdir().unwrap();
        let mut store = ContentStore::open(dir.path(), small_config(80)).unwrap();

        let data_a = vec![0u8; 40];
        let id_a = ContentId::from_bytes(&data_a).to_string();
        store.cache_content(&id_a, &data_a, &[1], &[1]).unwrap();
        store.pin(&id_a).unwrap();

        let data_b = vec![1u8; 40];
        let id_b = ContentId::from_bytes(&data_b).to_string();
        store.cache_content(&id_b, &data_b, &[2], &[2]).unwrap();
        store
            .cache_index
            .entries
            .get_mut(&id_b)
            .unwrap()
            .last_accessed = 1;

        // This should evict B, not A
        let data_c = vec![2u8; 40];
        let id_c = ContentId::from_bytes(&data_c).to_string();
        store.cache_content(&id_c, &data_c, &[3], &[3]).unwrap();

        assert!(store.has_content(&id_a), "pinned should survive");
        assert!(!store.has_content(&id_b), "unpinned should be evicted");
        assert!(store.has_content(&id_c));
    }
}
