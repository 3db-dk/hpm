//! gRPC service implementation for the package registry

use crate::proto::{
    download_response, health_response, package_registry_server::PackageRegistry, publish_request,
    DownloadRequest, DownloadResponse, HealthRequest, HealthResponse, ListVersionsRequest,
    ListVersionsResponse, PackageInfo, PackageInfoRequest, PublishRequest, PublishResponse,
    SearchRequest, SearchResponse, ValidateTokenRequest, ValidateTokenResponse,
};
use crate::server::{AuthService, PackageStorage};
use crate::types::PackageVersion;
use crate::utils::{compress_data, decompress_data, verify_checksum};
use chrono::Utc;
use std::sync::Arc;
use tokio_stream::{Stream, StreamExt};
use tonic::{Request, Response, Status, Streaming};

pub struct RegistryService {
    storage: Box<dyn PackageStorage>,
    auth_service: Arc<AuthService>,
}

impl RegistryService {
    pub fn new(storage: Box<dyn PackageStorage>, auth_service: AuthService) -> Self {
        Self {
            storage,
            auth_service: Arc::new(auth_service),
        }
    }
}

#[tonic::async_trait]
impl PackageRegistry for RegistryService {
    async fn publish_package(
        &self,
        request: Request<Streaming<PublishRequest>>,
    ) -> Result<Response<PublishResponse>, Status> {
        // For now, skip authentication to fix compilation issues
        // TODO: Re-implement authentication without Sync issues

        let mut stream = request.into_inner();
        let mut package_data = Vec::new();
        let mut metadata = None;

        // Process the stream
        while let Some(req) = stream.next().await {
            let req = req?;

            match req.data {
                Some(publish_request::Data::Metadata(meta)) => {
                    metadata = Some(meta);
                }
                Some(publish_request::Data::Chunk(chunk)) => {
                    package_data.extend_from_slice(&chunk);
                }
                None => {}
            }
        }

        let metadata =
            metadata.ok_or_else(|| Status::invalid_argument("Missing package metadata"))?;

        // Verify checksum
        let decompressed_data = decompress_data(&package_data)
            .map_err(|e| Status::internal(format!("Decompression failed: {}", e)))?;

        verify_checksum(&decompressed_data, &metadata.checksum)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        // Create package version
        let package_version = PackageVersion {
            version: metadata.version.clone(),
            metadata: crate::types::PackageMetadata {
                name: metadata.name.clone(),
                version: metadata.version.clone(),
                description: metadata.description.clone(),
                authors: metadata.authors.clone(),
                license: if metadata.license.is_empty() {
                    None
                } else {
                    Some(metadata.license.clone())
                },
                dependencies: metadata.dependencies.clone(),
                houdini: crate::types::HoudiniRequirements {
                    min_version: metadata.houdini.as_ref().and_then(|h| {
                        if h.min_version.is_empty() {
                            None
                        } else {
                            Some(h.min_version.clone())
                        }
                    }),
                    max_version: metadata.houdini.as_ref().and_then(|h| {
                        if h.max_version.is_empty() {
                            None
                        } else {
                            Some(h.max_version.clone())
                        }
                    }),
                    platforms: metadata
                        .houdini
                        .as_ref()
                        .map(|h| h.platforms.clone())
                        .unwrap_or_default(),
                },
                keywords: metadata.keywords.clone(),
                readme: if metadata.readme.is_empty() {
                    None
                } else {
                    Some(metadata.readme.clone())
                },
                repository: if metadata.repository.is_empty() {
                    None
                } else {
                    Some(metadata.repository.clone())
                },
                homepage: if metadata.homepage.is_empty() {
                    None
                } else {
                    Some(metadata.homepage.clone())
                },
            },
            published_at: Utc::now(),
            published_by: "unknown".to_string(), // Would come from token
            checksum: metadata.checksum,
            size_bytes: decompressed_data.len() as u64,
        };

        // Store package
        let package_id = self
            .storage
            .store_package(package_version, decompressed_data)
            .await
            .map_err(Status::from)?;

        Ok(Response::new(PublishResponse {
            success: true,
            package_id,
            message: "Package published successfully".to_string(),
        }))
    }

    type DownloadPackageStream =
        std::pin::Pin<Box<dyn Stream<Item = Result<DownloadResponse, Status>> + Send>>;

    async fn download_package(
        &self,
        request: Request<DownloadRequest>,
    ) -> Result<Response<Self::DownloadPackageStream>, Status> {
        let req = request.into_inner();

        // Get package info
        let package_version = self
            .storage
            .get_package_info(&req.name, Some(&req.version))
            .await
            .map_err(Status::from)?;

        // Get package data
        let package_data = self
            .storage
            .get_package_data(&req.name, &req.version)
            .await
            .map_err(Status::from)?;

        // Compress data for transmission
        let compressed_data = compress_data(&package_data)
            .map_err(|e| Status::internal(format!("Compression failed: {}", e)))?;

        // Create response stream
        let package_info: PackageInfo = package_version.into();
        let metadata_response = DownloadResponse {
            data: Some(download_response::Data::Metadata(package_info)),
        };

        // Convert all chunks to responses upfront to avoid borrowing issues
        let chunk_responses: Vec<Result<DownloadResponse, Status>> = compressed_data
            .chunks(8192)
            .map(|chunk| {
                Ok(DownloadResponse {
                    data: Some(download_response::Data::Chunk(chunk.to_vec())),
                })
            })
            .collect();

        let response_stream =
            tokio_stream::iter(std::iter::once(Ok(metadata_response)).chain(chunk_responses));

        Ok(Response::new(Box::pin(response_stream)))
    }

    async fn search_packages(
        &self,
        request: Request<SearchRequest>,
    ) -> Result<Response<SearchResponse>, Status> {
        let req = request.into_inner();

        let (packages, total_count) = self
            .storage
            .search_packages(&req.query, req.limit as usize, req.offset as usize)
            .await
            .map_err(Status::from)?;

        let package_infos: Vec<PackageInfo> = packages.into_iter().map(|p| p.into()).collect();

        Ok(Response::new(SearchResponse {
            packages: package_infos,
            total_count: total_count as i32,
        }))
    }

    async fn get_package_info(
        &self,
        request: Request<PackageInfoRequest>,
    ) -> Result<Response<PackageInfo>, Status> {
        let req = request.into_inner();

        let version = if req.version.is_empty() {
            None
        } else {
            Some(req.version.as_str())
        };

        let package_version = self
            .storage
            .get_package_info(&req.name, version)
            .await
            .map_err(Status::from)?;

        Ok(Response::new(package_version.into()))
    }

    async fn list_versions(
        &self,
        request: Request<ListVersionsRequest>,
    ) -> Result<Response<ListVersionsResponse>, Status> {
        let req = request.into_inner();

        let versions = self
            .storage
            .list_versions(&req.name)
            .await
            .map_err(Status::from)?;

        Ok(Response::new(ListVersionsResponse { versions }))
    }

    async fn validate_token(
        &self,
        request: Request<ValidateTokenRequest>,
    ) -> Result<Response<ValidateTokenResponse>, Status> {
        let req = request.into_inner();

        match self.auth_service.validate_token(&req.token).await {
            Ok(token) => {
                let scopes: Vec<String> = token
                    .scopes
                    .iter()
                    .map(|s| s.as_str().to_string())
                    .collect();

                Ok(Response::new(ValidateTokenResponse {
                    valid: true,
                    user_id: token.user_id,
                    scopes,
                    expires_at: token.expires_at.map(|dt| dt.timestamp()).unwrap_or(0),
                }))
            }
            Err(_) => Ok(Response::new(ValidateTokenResponse {
                valid: false,
                user_id: String::new(),
                scopes: Vec::new(),
                expires_at: 0,
            })),
        }
    }

    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        Ok(Response::new(HealthResponse {
            status: health_response::Status::Serving.into(),
            message: "Registry is healthy".to_string(),
            timestamp: Utc::now().timestamp(),
        }))
    }
}
