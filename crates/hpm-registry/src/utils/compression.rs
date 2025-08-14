//! Package compression utilities

use crate::types::RegistryError;
use bytes::Bytes;
use std::io::{Read, Write};

pub const DEFAULT_COMPRESSION_LEVEL: i32 = 3;
pub const MAX_CHUNK_SIZE: usize = 8192;

pub fn compress_data(data: &[u8]) -> Result<Vec<u8>, RegistryError> {
    let mut encoder = zstd::Encoder::new(Vec::new(), DEFAULT_COMPRESSION_LEVEL)
        .map_err(|e| RegistryError::Compression(e.to_string()))?;

    encoder
        .write_all(data)
        .map_err(|e| RegistryError::Compression(e.to_string()))?;

    encoder
        .finish()
        .map_err(|e| RegistryError::Compression(e.to_string()))
}

pub fn decompress_data(compressed: &[u8]) -> Result<Vec<u8>, RegistryError> {
    let mut decoder =
        zstd::Decoder::new(compressed).map_err(|e| RegistryError::Compression(e.to_string()))?;

    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| RegistryError::Compression(e.to_string()))?;

    Ok(decompressed)
}

pub fn chunk_data(data: &[u8], chunk_size: usize) -> impl Iterator<Item = Bytes> + '_ {
    data.chunks(chunk_size).map(Bytes::copy_from_slice)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_roundtrip() {
        // Use larger data that will actually compress well
        let original_data = b"Hello, world! This is test data for compression. ".repeat(100);

        let compressed = compress_data(&original_data).unwrap();
        assert!(
            compressed.len() < original_data.len(),
            "Compressed size {} should be less than original size {}",
            compressed.len(),
            original_data.len()
        );

        let decompressed = decompress_data(&compressed).unwrap();
        assert_eq!(decompressed, original_data);
    }

    #[test]
    fn test_chunking() {
        let data = b"0123456789";
        let chunks: Vec<_> = chunk_data(data, 3).collect();

        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks[0].as_ref(), b"012");
        assert_eq!(chunks[1].as_ref(), b"345");
        assert_eq!(chunks[2].as_ref(), b"678");
        assert_eq!(chunks[3].as_ref(), b"9");
    }
}
