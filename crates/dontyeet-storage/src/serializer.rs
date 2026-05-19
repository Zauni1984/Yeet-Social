//! Serialization traits and default JSON implementation.

use serde::{Serialize, de::DeserializeOwned};

use crate::error::{StorageError, StorageResult};

/// Serialize/deserialize typed values to/from bytes.
pub trait Serializer: Send + Sync {
    /// Serialize a value to bytes.
    ///
    /// # Errors
    /// Returns `StorageError::Serialization` if encoding fails.
    fn serialize<T: Serialize>(&self, value: &T) -> StorageResult<Vec<u8>>;

    /// Deserialize bytes back into a typed value.
    ///
    /// # Errors
    /// Returns `StorageError::Serialization` if decoding fails.
    fn deserialize<T: DeserializeOwned>(&self, bytes: &[u8]) -> StorageResult<T>;
}

/// JSON serializer (default).
pub struct JsonSerializer;

impl Serializer for JsonSerializer {
    fn serialize<T: Serialize>(&self, value: &T) -> StorageResult<Vec<u8>> {
        serde_json::to_vec(value)
            .map_err(|e| StorageError::Serialization(format!("JSON encode: {e}")))
    }

    fn deserialize<T: DeserializeOwned>(&self, bytes: &[u8]) -> StorageResult<T> {
        serde_json::from_slice(bytes)
            .map_err(|e| StorageError::Serialization(format!("JSON decode: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct TestData {
        name: String,
        value: u64,
    }

    #[test]
    fn json_round_trip() {
        let s = JsonSerializer;
        let data = TestData {
            name: "test".into(),
            value: 42,
        };
        let bytes = s.serialize(&data).expect("serialize");
        let decoded: TestData = s.deserialize(&bytes).expect("deserialize");
        assert_eq!(data, decoded);
    }
}

// Rust guideline compliant 2026-05-02
