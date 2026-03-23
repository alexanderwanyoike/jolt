use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub content_id: String,
    pub size: u64,
    pub cached_at: u64,
    pub last_accessed: u64,
    pub pinned: bool,
    pub publisher_key: Vec<u8>,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheIndex {
    pub entries: HashMap<String, CacheEntry>,
    pub total_size: u64,
}

/// Data returned from ContentStore::get_content
#[derive(Debug, Clone)]
pub struct ContentData {
    pub data: Vec<u8>,
    pub publisher_key: Vec<u8>,
    pub signature: Vec<u8>,
}

/// Aggregate cache statistics for display
#[derive(Debug)]
pub struct CacheStats {
    pub total_cached: u64,
    pub total_published: u64,
    pub cached_items: usize,
    pub published_items: usize,
    pub pinned_items: usize,
    pub pinned_size: u64,
    pub max_size: u64,
    pub available: u64,
}
