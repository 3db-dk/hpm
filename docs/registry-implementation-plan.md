# HPM Registry Implementation Plan

## Overview

This document outlines the implementation plan for the HPM package registry using QUIC (s2n-quic) for transport and gRPC for RPC communication.

## Architecture Design

### Registry Crate Structure

```
crates/hpm-registry/
├── Cargo.toml
├── src/
│   ├── lib.rs                    # Public API and re-exports
│   ├── client/                   # Registry client implementation
│   │   ├── mod.rs
│   │   ├── auth.rs              # Authentication handling
│   │   ├── connection.rs        # QUIC connection management
│   │   └── operations.rs        # Package operations (upload/download/search)
│   ├── server/                   # Registry server implementation
│   │   ├── mod.rs
│   │   ├── auth.rs              # Authentication middleware
│   │   ├── service.rs           # gRPC service implementation
│   │   ├── storage.rs           # Package storage backend
│   │   └── main.rs              # Server binary entry point
│   ├── proto/                    # Protocol buffer definitions
│   │   ├── mod.rs
│   │   └── registry.proto        # gRPC service and message definitions
│   ├── types/                    # Shared types and utilities
│   │   ├── mod.rs
│   │   ├── package.rs           # Package metadata types
│   │   ├── auth.rs              # Authentication types
│   │   └── error.rs             # Registry-specific errors
│   └── utils/
│       ├── mod.rs
│       ├── compression.rs       # Package compression utilities
│       └── validation.rs       # Package validation
├── proto/                       # Proto files for build
│   └── registry.proto
└── build.rs                     # Protocol buffer build script
```

### Technology Stack

**Core Dependencies:**
- `s2n-quic` - QUIC transport implementation
- `tonic` - gRPC framework for Rust
- `tonic-h3` - gRPC over HTTP/3 support
- `prost` - Protocol buffer serialization
- `tokio` - Async runtime
- `anyhow`/`thiserror` - Error handling

**Additional Dependencies:**
- `sha2` - Package checksums
- `zstd` - Package compression
- `serde` - Configuration serialization
- `tracing` - Logging and observability
- `uuid` - Token generation
- `ring` - Cryptographic operations

## Protocol Design

### gRPC Service Definition

```protobuf
syntax = "proto3";

package hpm.registry.v1;

service PackageRegistry {
  // Package operations
  rpc PublishPackage(stream PublishRequest) returns (PublishResponse);
  rpc DownloadPackage(DownloadRequest) returns (stream DownloadResponse);
  rpc SearchPackages(SearchRequest) returns (SearchResponse);
  rpc GetPackageInfo(PackageInfoRequest) returns (PackageInfo);
  rpc ListVersions(ListVersionsRequest) returns (ListVersionsResponse);
  
  // Authentication
  rpc ValidateToken(ValidateTokenRequest) returns (ValidateTokenResponse);
  
  // Health checks
  rpc Health(HealthRequest) returns (HealthResponse);
}

message PackageInfo {
  string name = 1;
  string version = 2;
  string description = 3;
  repeated string authors = 4;
  string license = 5;
  map<string, string> dependencies = 6;
  HoudiniRequirements houdini = 7;
  int64 size_bytes = 8;
  string checksum = 9;
  int64 published_at = 10;
}

message HoudiniRequirements {
  string min_version = 1;
  string max_version = 2;
  repeated string platforms = 3;
}

message PublishRequest {
  oneof data {
    PackageMetadata metadata = 1;
    bytes chunk = 2;
  }
}

message DownloadRequest {
  string name = 1;
  string version = 2;
}

message SearchRequest {
  string query = 1;
  int32 limit = 2;
  int32 offset = 3;
  SearchFilter filter = 4;
}
```

## Implementation Phases

### Phase 1: Foundation (Week 1-2)

**Goals:**
- Set up basic QUIC + gRPC infrastructure
- Implement core protocol buffer definitions
- Create basic client/server structure

**Deliverables:**
1. Update `hpm-registry/Cargo.toml` with dependencies
2. Implement protocol buffer definitions
3. Create basic server with health check endpoint
4. Create basic client with connection management
5. Add integration tests for client/server communication

### Phase 2: Core Operations (Week 3-4)

**Goals:**
- Implement package upload/download
- Add basic authentication
- Integrate with existing package storage

**Deliverables:**
1. Package publish functionality with streaming
2. Package download with checksum verification
3. Token-based authentication system
4. Integration with `hpm-core` storage system
5. CLI commands: `hpm publish`, `hpm install <pkg>`

### Phase 3: Search and Discovery (Week 5)

**Goals:**
- Implement package search and metadata queries
- Add package versioning support

**Deliverables:**
1. Package search functionality
2. Version listing and management
3. CLI commands: `hpm search <query>`, `hpm info <pkg>`
4. Package metadata caching

### Phase 4: Security and Production (Week 6-7)

**Goals:**
- Enhanced security features
- Production deployment preparation
- Performance optimization

**Deliverables:**
1. Advanced authentication (token scopes, expiration)
2. Rate limiting and abuse prevention
3. Package signature verification
4. Comprehensive logging and metrics
5. Docker containers for server deployment

## Implementation Details

### Authentication System

```rust
#[derive(Debug, Clone)]
pub struct AuthToken {
    pub token: String,
    pub user_id: String,
    pub scopes: Vec<TokenScope>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenScope {
    Read,
    Publish,
    Admin,
}

pub struct AuthService {
    tokens: Arc<RwLock<HashMap<String, AuthToken>>>,
}

impl AuthService {
    pub async fn validate_token(&self, token: &str) -> Result<AuthToken> {
        // Token validation logic
    }
    
    pub async fn check_permission(&self, token: &AuthToken, scope: TokenScope) -> Result<()> {
        if token.scopes.contains(&scope) {
            Ok(())
        } else {
            Err(AuthError::InsufficientPermissions)
        }
    }
}
```

### Server Implementation

```rust
use tonic::{transport::Server, Request, Response, Status, Streaming};
use s2n_quic::Server as QuicServer;

#[derive(Debug)]
pub struct RegistryService {
    storage: Arc<dyn PackageStorage>,
    auth: Arc<AuthService>,
}

#[tonic::async_trait]
impl PackageRegistry for RegistryService {
    async fn publish_package(
        &self,
        request: Request<Streaming<PublishRequest>>,
    ) -> Result<Response<PublishResponse>, Status> {
        let auth_token = self.extract_auth_token(&request)?;
        self.auth.check_permission(&auth_token, TokenScope::Publish).await?;
        
        let mut stream = request.into_inner();
        let mut package_data = Vec::new();
        let mut metadata = None;
        
        while let Some(req) = stream.message().await? {
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
        
        let metadata = metadata.ok_or_else(|| Status::invalid_argument("Missing metadata"))?;
        let package_info = self.storage.store_package(metadata, package_data).await?;
        
        Ok(Response::new(PublishResponse {
            success: true,
            package_id: package_info.id,
        }))
    }
}
```

### Client Implementation

```rust
use tonic::transport::Channel;
use s2n_quic::client::Connect;

pub struct RegistryClient {
    client: PackageRegistryClient<Channel>,
    auth_token: Option<String>,
}

impl RegistryClient {
    pub async fn connect(endpoint: &str) -> Result<Self> {
        // Create QUIC connection
        let channel = Channel::from_shared(endpoint)?
            .connect()
            .await?;
            
        let client = PackageRegistryClient::new(channel);
        
        Ok(Self {
            client,
            auth_token: None,
        })
    }
    
    pub async fn publish_package(
        &mut self, 
        metadata: PackageMetadata, 
        data: Vec<u8>
    ) -> Result<PublishResponse> {
        let metadata_request = PublishRequest {
            data: Some(publish_request::Data::Metadata(metadata)),
        };
        
        let chunk_requests = data
            .chunks(8192)
            .map(|chunk| PublishRequest {
                data: Some(publish_request::Data::Chunk(chunk.to_vec())),
            });
            
        let requests = std::iter::once(metadata_request).chain(chunk_requests);
        let request_stream = tokio_stream::iter(requests);
        
        let request = self.add_auth_metadata(tonic::Request::new(request_stream))?;
        let response = self.client.publish_package(request).await?;
        
        Ok(response.into_inner())
    }
}
```

## CLI Integration

### New CLI Commands

```rust
// crates/hpm-cli/src/commands/publish.rs
#[derive(Debug, clap::Args)]
pub struct PublishCommand {
    /// Package directory to publish
    #[arg(default_value = ".")]
    path: PathBuf,
    
    /// Registry URL
    #[arg(long)]
    registry: Option<String>,
    
    /// Dry run - validate but don't publish
    #[arg(long)]
    dry_run: bool,
}

impl PublishCommand {
    pub async fn execute(&self) -> Result<()> {
        let package_info = PackageInfo::from_directory(&self.path).await?;
        let registry_url = self.registry_url().await?;
        
        let mut client = RegistryClient::connect(&registry_url).await?;
        client.set_auth_token(self.load_auth_token(&registry_url)?);
        
        if self.dry_run {
            println!("Would publish {} v{}", package_info.name, package_info.version);
            return Ok(());
        }
        
        let package_data = self.create_package_archive(&self.path).await?;
        let response = client.publish_package(package_info.into(), package_data).await?;
        
        println!("Published {} v{}", package_info.name, package_info.version);
        Ok(())
    }
}
```

## Testing Strategy

### Unit Tests
- Protocol buffer serialization/deserialization
- Authentication token validation
- Package compression and checksums
- Error handling and edge cases

### Integration Tests
- End-to-end client/server communication
- Package publish and download workflows
- Search functionality
- Authentication flows

### Performance Tests
- Large package upload/download
- Concurrent client connections
- Memory usage under load
- QUIC vs HTTP/2 performance comparison

## Deployment Considerations

### Server Requirements
- Rust 1.70+ with stable toolchain
- PostgreSQL for metadata storage
- Object storage (S3-compatible) for packages
- Redis for caching (optional)
- TLS certificates for HTTPS/QUIC

### Configuration
```toml
# registry-server.toml
[server]
host = "0.0.0.0"
port = 443
max_connections = 1000

[storage]
type = "s3"
bucket = "hpm-packages"
region = "us-east-1"

[database]
url = "postgres://user:pass@localhost/hpm_registry"
max_connections = 10

[auth]
token_secret = "your-secret-key"
token_expiry = "30d"
```

## Security Considerations

### Transport Security
- QUIC provides mandatory encryption
- TLS 1.3 minimum for all connections
- Certificate pinning for client connections

### Package Security
- SHA-256 checksums for all packages
- Package size limits to prevent DoS
- Malware scanning integration (future)
- Signature verification (future)

### Authentication Security
- Secure token generation using `ring`
- Token hashing for storage
- Rate limiting per token
- Audit logging for all operations

This implementation plan provides a structured approach to building the HPM registry with QUIC and gRPC, ensuring security, performance, and maintainability.