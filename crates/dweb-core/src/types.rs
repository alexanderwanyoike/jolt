use serde::{Deserialize, Serialize};

use crate::content_id::ContentId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentManifest {
    pub content_id: ContentId,
    pub size: u64,
    pub content_type: String,
    pub publisher_key: Vec<u8>,
    pub signature: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_manifest_serializes_to_json() {
        let manifest = ContentManifest {
            content_id: ContentId::from_bytes(b"test"),
            size: 1234,
            content_type: "application/octet-stream".to_string(),
            publisher_key: vec![1, 2, 3],
            signature: vec![4, 5, 6],
        };

        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: ContentManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(manifest.content_id, parsed.content_id);
        assert_eq!(manifest.size, parsed.size);
        assert_eq!(manifest.content_type, parsed.content_type);
        assert_eq!(manifest.publisher_key, parsed.publisher_key);
        assert_eq!(manifest.signature, parsed.signature);
    }
}
