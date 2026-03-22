use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentRequest {
    pub content_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentResponse {
    pub data: Vec<u8>,
    pub signature: Vec<u8>,
    pub publisher_key: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_request_cbor_round_trip() {
        let request = ContentRequest {
            content_id: "bafk_test_id_123".to_string(),
        };

        let mut buf = Vec::new();
        ciborium::into_writer(&request, &mut buf).unwrap();
        let decoded: ContentRequest = ciborium::from_reader(&buf[..]).unwrap();
        assert_eq!(request.content_id, decoded.content_id);
    }

    #[test]
    fn content_response_cbor_round_trip() {
        let response = ContentResponse {
            data: vec![1, 2, 3, 4, 5],
            signature: vec![10, 20, 30],
            publisher_key: vec![40, 50, 60],
        };

        let mut buf = Vec::new();
        ciborium::into_writer(&response, &mut buf).unwrap();
        let decoded: ContentResponse = ciborium::from_reader(&buf[..]).unwrap();
        assert_eq!(response.data, decoded.data);
        assert_eq!(response.signature, decoded.signature);
        assert_eq!(response.publisher_key, decoded.publisher_key);
    }
}
