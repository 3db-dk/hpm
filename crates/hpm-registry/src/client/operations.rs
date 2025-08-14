//! Client operations for interacting with the registry

use crate::client::AuthManager;
use crate::proto::{
    download_response, package_registry_client::PackageRegistryClient, publish_request,
    DownloadRequest, HealthRequest, PackageInfo, PackageInfoRequest, PublishRequest,
    PublishResponse, SearchRequest, SearchResponse,
};
use crate::types::RegistryError;
use crate::utils::{calculate_checksum, compress_data, decompress_data, validate_package_size};
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio_stream::{Stream, StreamExt};
use tonic::transport::Channel;

const CHUNK_SIZE: usize = 8192;

pub async fn publish_package<P: AsRef<Path>>(
    client: &mut PackageRegistryClient<Channel>,
    auth_manager: &AuthManager,
    package_path: P,
) -> Result<PublishResponse, RegistryError> {
    let package_path = package_path.as_ref();

    // Read package data
    let package_data = read_package_directory(package_path).await?;
    let package_metadata = extract_package_metadata(package_path).await?;

    // Validate package size
    validate_package_size(package_data.len() as u64)?;

    // Calculate checksum
    let checksum = calculate_checksum(&package_data);

    // Compress package data
    let compressed_data = compress_data(&package_data)?;

    // Create request stream
    let requests = create_publish_stream(package_metadata, compressed_data, checksum);
    let request = auth_manager.add_auth_metadata(tonic::Request::new(requests));

    // Send publish request
    let response = client.publish_package(request).await?;
    Ok(response.into_inner())
}

pub async fn download_package(
    client: &mut PackageRegistryClient<Channel>,
    name: &str,
    version: &str,
    output_path: &Path,
) -> Result<(), RegistryError> {
    let request = tonic::Request::new(DownloadRequest {
        name: name.to_string(),
        version: version.to_string(),
    });

    let mut response_stream = client.download_package(request).await?.into_inner();
    let mut package_data = Vec::new();
    let mut metadata = None;

    while let Some(response) = response_stream.next().await {
        let response = response?;

        match response.data {
            Some(download_response::Data::Metadata(meta)) => {
                metadata = Some(meta);
            }
            Some(download_response::Data::Chunk(chunk)) => {
                package_data.extend_from_slice(&chunk);
            }
            None => {}
        }
    }

    let metadata = metadata
        .ok_or_else(|| RegistryError::InvalidPackageData("Missing package metadata".to_string()))?;

    // Verify checksum
    let expected_checksum = metadata.checksum;
    let decompressed_data = decompress_data(&package_data)?;
    let actual_checksum = calculate_checksum(&decompressed_data);

    if actual_checksum != expected_checksum {
        return Err(RegistryError::ChecksumMismatch {
            expected: expected_checksum,
            actual: actual_checksum,
        });
    }

    // Write package to output path
    write_package_to_directory(output_path, &decompressed_data).await?;

    Ok(())
}

pub async fn search_packages(
    client: &mut PackageRegistryClient<Channel>,
    query: &str,
    limit: Option<i32>,
    offset: Option<i32>,
) -> Result<SearchResponse, RegistryError> {
    let request = tonic::Request::new(SearchRequest {
        query: query.to_string(),
        limit: limit.unwrap_or(50),
        offset: offset.unwrap_or(0),
        filter: None, // Could be extended later
    });

    let response = client.search_packages(request).await?;
    Ok(response.into_inner())
}

pub async fn get_package_info(
    client: &mut PackageRegistryClient<Channel>,
    name: &str,
    version: Option<&str>,
) -> Result<PackageInfo, RegistryError> {
    let request = tonic::Request::new(PackageInfoRequest {
        name: name.to_string(),
        version: version.unwrap_or_default().to_string(),
    });

    let response = client.get_package_info(request).await?;
    Ok(response.into_inner())
}

pub async fn health_check(
    client: &mut PackageRegistryClient<Channel>,
) -> Result<bool, RegistryError> {
    let request = tonic::Request::new(HealthRequest {});

    let response = client.health(request).await?;
    let health_response = response.into_inner();

    Ok(matches!(
        health_response.status(),
        crate::proto::health_response::Status::Serving
    ))
}

// Helper functions

async fn read_package_directory(path: &Path) -> Result<Vec<u8>, RegistryError> {
    // In a real implementation, this would create a tar/zip archive of the directory
    // For now, we'll just read a dummy file
    let manifest_path = path.join("hpm.toml");
    fs::read(&manifest_path).await.map_err(RegistryError::Io)
}

async fn extract_package_metadata(
    _path: &Path,
) -> Result<crate::proto::PackageMetadata, RegistryError> {
    // In a real implementation, this would parse the hpm.toml file
    // For now, return a dummy metadata
    Ok(crate::proto::PackageMetadata {
        name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        description: "Test package".to_string(),
        authors: vec!["Test Author".to_string()],
        license: "MIT".to_string(),
        dependencies: std::collections::HashMap::new(),
        houdini: Some(crate::proto::HoudiniRequirements {
            min_version: "19.0".to_string(),
            max_version: "20.0".to_string(),
            platforms: vec!["linux".to_string(), "windows".to_string()],
        }),
        keywords: vec!["test".to_string()],
        readme: "Test package readme".to_string(),
        repository: "https://github.com/test/test-package".to_string(),
        homepage: "https://test-package.dev".to_string(),
        size_bytes: 0,            // Will be filled in later
        checksum: "".to_string(), // Will be filled in later
    })
}

fn create_publish_stream(
    mut metadata: crate::proto::PackageMetadata,
    compressed_data: Vec<u8>,
    checksum: String,
) -> impl Stream<Item = PublishRequest> {
    // Update metadata with actual size and checksum
    metadata.size_bytes = compressed_data.len() as i64;
    metadata.checksum = checksum;

    let metadata_request = PublishRequest {
        data: Some(publish_request::Data::Metadata(metadata)),
    };

    // Convert all chunks to requests upfront to avoid borrowing issues
    let chunk_requests: Vec<PublishRequest> = compressed_data
        .chunks(CHUNK_SIZE)
        .map(|chunk| PublishRequest {
            data: Some(publish_request::Data::Chunk(chunk.to_vec())),
        })
        .collect();

    tokio_stream::iter(std::iter::once(metadata_request).chain(chunk_requests))
}

async fn write_package_to_directory(path: &Path, data: &[u8]) -> Result<(), RegistryError> {
    // In a real implementation, this would extract a tar/zip archive
    // For now, just write to a single file
    fs::create_dir_all(path).await.map_err(RegistryError::Io)?;
    let output_file = path.join("package.data");
    let mut file = fs::File::create(output_file)
        .await
        .map_err(RegistryError::Io)?;
    file.write_all(data).await.map_err(RegistryError::Io)
}
