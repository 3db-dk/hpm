# HPM Package Registry Architecture

## Executive Summary

This document outlines the server/client architecture for the HPM package registry, implementing a high-performance, secure system optimized for package distribution. Based on comprehensive research, we recommend **QUIC with HTTP/3** as the transport layer, leveraging **gRPC** for RPC communication and **API tokens** for authentication.

**Key Decision: QUIC over WebSocket**
- QUIC provides 3.69x performance improvement for large file transfers
- 50% latency reduction for initial connections
- Superior handling of network instability and connection migration
- Optimized for package registry use cases (large, infrequent transfers)

## Architecture Overview

### High-Level Components

```
┌─────────────────┐    QUIC/HTTP3+gRPC    ┌─────────────────┐
│   HPM Client    │ ◄─────────────────────► │ Registry Server │
│                 │                        │                 │
│ • CLI Interface │                        │ • API Gateway   │
│ • Auth Manager  │                        │ • Package Store │
│ • Cache Layer   │                        │ • Auth Service  │
│ • Download Mgr  │                        │ • Search Engine │
└─────────────────┘                        └─────────────────┘
                                                     │
                                           ┌─────────────────┐
                                           │   Database      │
                                           │                 │
                                           │ • Package Meta  │
                                           │ • User Accounts │
                                           │ • API Tokens    │
                                           │ • Audit Logs    │
                                           └─────────────────┘
```

### Data Flow

1. **Client Authentication**: API token validation via gRPC metadata
2. **Package Operations**: Upload/download via streaming gRPC calls
3. **Metadata Queries**: Search and package info via unary gRPC calls
4. **Caching**: Client-side cache for metadata, server-side CDN for packages

## Technology Stack

### Transport Layer
- **Primary**: QUIC with s2n-quic (AWS-backed, production-proven)
- **Fallback**: HTTP/2 with TLS 1.3 for compatibility
- **Rationale**: s2n-quic provides formal verification, interoperability testing, and production deployment at CloudFront scale

### RPC Framework
- **Framework**: tonic (native Rust gRPC implementation)
- **HTTP/3 Extension**: tonic-h3 for gRPC over QUIC
- **Serialization**: Protocol Buffers for efficient binary serialization
- **Compression**: zstd compression for package data

### Client Libraries
- **QUIC Client**: s2n-quic with async/await support
- **HTTP Client**: reqwest with HTTP/2 fallback
- **Streaming**: tokio-stream for large file transfers

### Server Components
- **HTTP/3 Server**: s2n-quic with tonic integration
- **Database**: PostgreSQL with tokio-postgres driver
- **Storage**: Object storage (S3-compatible) for package artifacts
- **Caching**: Redis for metadata caching

## RPC Protocol Design

### Service Definitions

```protobuf
syntax = "proto3";

package hpm.registry.v1;

service PackageRegistry {
  // Package Management
  rpc PublishPackage(stream PublishPackageRequest) returns (PublishPackageResponse);
  rpc DownloadPackage(DownloadPackageRequest) returns (stream DownloadPackageResponse);
  rpc GetPackageInfo(GetPackageInfoRequest) returns (PackageInfo);
  rpc SearchPackages(SearchPackagesRequest) returns (SearchPackagesResponse);
  rpc ListPackageVersions(ListPackageVersionsRequest) returns (ListPackageVersionsResponse);
  
  // Version Management
  rpc GetLatestVersion(GetLatestVersionRequest) returns (VersionInfo);
  rpc DeprecateVersion(DeprecateVersionRequest) returns (DeprecateVersionResponse);
  
  // Authentication & Authorization
  rpc ValidateToken(ValidateTokenRequest) returns (ValidateTokenResponse);
  rpc RefreshToken(RefreshTokenRequest) returns (RefreshTokenResponse);
  
  // Registry Health
  rpc GetRegistryStatus(GetRegistryStatusRequest) returns (RegistryStatus);
}
```

### Message Formats

```protobuf
message PackageInfo {
  string name = 1;
  string version = 2;
  string description = 3;
  repeated string authors = 4;
  string license = 5;
  map<string, string> dependencies = 6;
  HoudiniCompatibility houdini = 7;
  PackageMetrics metrics = 8;
  google.protobuf.Timestamp published_at = 9;
}

message HoudiniCompatibility {
  string min_version = 1;
  string max_version = 2;
  repeated string platforms = 3;
}

message PublishPackageRequest {
  oneof data {
    PackageInfo metadata = 1;
    bytes chunk = 2;
  }
}

message DownloadPackageRequest {
  string name = 1;
  string version = 2;
  bool include_dependencies = 3;
}
```

## Authentication System

### API Token Architecture

**Token Types:**
- **Personal Access Tokens (PAT)**: Individual developer access
- **Organization Tokens**: Team-based publishing
- **CI/CD Tokens**: Automated pipeline integration
- **Read-Only Tokens**: Private package access

**Token Format:**
```
hpm_[type]_[random]
Examples:
- hpm_pat_1a2b3c4d5e6f7g8h9i0j
- hpm_org_9i8h7g6f5e4d3c2b1a0j
- hpm_ci_0j1i2h3g4f5e6d7c8b9a
```

### Security Features

**Token Validation:**
```rust
pub struct TokenClaims {
    pub token_id: String,
    pub user_id: String,
    pub scopes: Vec<TokenScope>,
    pub expires_at: Option<DateTime<Utc>>,
    pub ip_restrictions: Option<Vec<IpNetwork>>,
}

pub enum TokenScope {
    PackageRead,
    PackagePublish,
    PackageDelete,
    UserRead,
    OrganizationAdmin,
}
```

**Authentication Middleware:**
```rust
impl<S> Service<Request<Body>> for AuthMiddleware<S> {
    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        let token = extract_token_from_metadata(&req);
        let validation_result = self.token_validator.validate(token).await;
        
        match validation_result {
            Ok(claims) => {
                req.extensions_mut().insert(claims);
                self.inner.call(req)
            }
            Err(error) => {
                Box::pin(async { Err(AuthError::InvalidToken(error).into()) })
            }
        }
    }
}
```

### Token Storage & Management

**Server-Side Storage:**
```sql
CREATE TABLE api_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    token_hash VARCHAR(64) NOT NULL UNIQUE, -- SHA-256 hash
    user_id UUID NOT NULL REFERENCES users(id),
    name VARCHAR(255) NOT NULL,
    scopes TEXT[] NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT now(),
    expires_at TIMESTAMP WITH TIME ZONE,
    last_used_at TIMESTAMP WITH TIME ZONE,
    ip_restrictions INET[],
    is_active BOOLEAN DEFAULT true
);
```

**Client-Side Storage:**
```toml
# ~/.hpm/credentials.toml
[auth]
default_registry = "https://registry.hpm.dev"

[registries]
"https://registry.hpm.dev" = { token = "hpm_pat_1a2b3c4d5e6f7g8h9i0j" }
"https://private.company.com" = { token = "hmp_org_9i8h7g6f5e4d3c2b1a0j" }
```

## Performance Considerations

### QUIC Optimizations

**Connection Management:**
- Connection pooling with automatic reconnection
- 0-RTT resumption for frequent operations
- Connection migration for mobile clients
- Adaptive congestion control

**Streaming Efficiency:**
```rust
impl PackageRegistryService {
    async fn download_package(&self, request: DownloadPackageRequest) 
        -> Result<impl Stream<Item = DownloadPackageResponse>, Status> {
        
        let package_stream = self.storage
            .get_package_stream(&request.name, &request.version)
            .await?;
            
        let compressed_stream = package_stream
            .compress(CompressionAlgorithm::Zstd)
            .chunks(CHUNK_SIZE);
            
        Ok(compressed_stream.map(|chunk| DownloadPackageResponse {
            chunk: chunk.into(),
        }))
    }
}
```

### Caching Strategy

**Multi-Level Caching:**
1. **Client Cache**: Metadata and dependency resolution cache
2. **CDN**: Package artifacts distributed globally
3. **Application Cache**: Redis for frequently accessed metadata
4. **Database Cache**: PostgreSQL query result caching

## Security Considerations

### Threat Model

**Attack Vectors:**
- Package tampering during upload/download
- Dependency confusion attacks
- API token compromise
- DDoS attacks on registry infrastructure

**Mitigations:**

**Package Integrity:**
```rust
pub struct PackageChecksum {
    pub sha256: String,
    pub size: u64,
    pub signature: Option<String>, // Future: cryptographic signatures
}

impl PackageValidator {
    pub async fn verify_package(&self, data: &[u8], checksum: &PackageChecksum) -> Result<()> {
        let computed_hash = sha2::Sha256::digest(data);
        let expected_hash = hex::decode(&checksum.sha256)?;
        
        if computed_hash.as_slice() != expected_hash {
            return Err(ValidationError::ChecksumMismatch);
        }
        
        if data.len() as u64 != checksum.size {
            return Err(ValidationError::SizeMismatch);
        }
        
        Ok(())
    }
}
```

**Rate Limiting:**
```rust
pub struct RateLimiter {
    redis: RedisConnection,
}

impl RateLimiter {
    pub async fn check_rate_limit(&self, token: &str, operation: Operation) -> Result<()> {
        let key = format!("rate_limit:{}:{:?}", token, operation);
        let current = self.redis.incr(&key, 1).await?;
        
        if current == 1 {
            self.redis.expire(&key, operation.window_seconds()).await?;
        }
        
        if current > operation.max_requests() {
            return Err(RateLimitError::Exceeded);
        }
        
        Ok(())
    }
}
```

### Transport Security

**TLS Configuration:**
- TLS 1.3 minimum for all connections
- Certificate pinning for client connections
- HSTS headers for web interfaces
- Perfect Forward Secrecy

**QUIC Security:**
- Mandatory encryption (no plaintext mode)
- Connection ID rotation
- Anti-amplification protection
- Path validation

## Implementation Plan

### Phase 1: Foundation (4-6 weeks)
1. **Core Infrastructure**
   - s2n-quic server setup with basic HTTP/3
   - PostgreSQL schema design and migrations
   - Basic gRPC service definitions
   - Token-based authentication

2. **Basic Operations**
   - Package upload/download with checksum validation
   - Simple metadata storage and retrieval
   - Client CLI with basic commands

### Phase 2: Enhanced Features (4-6 weeks)
1. **Advanced Package Management**
   - Dependency resolution API
   - Package search with full-text indexing
   - Version management and deprecation
   - Package statistics and metrics

2. **Performance & Reliability**
   - Connection pooling and reuse
   - Streaming upload/download optimization
   - Redis caching layer
   - Comprehensive error handling

### Phase 3: Production Readiness (4-6 weeks)
1. **Security & Monitoring**
   - Rate limiting and abuse prevention
   - Audit logging and security monitoring
   - Package signature verification
   - Automated security scanning

2. **Scalability & Operations**
   - Horizontal scaling architecture
   - CDN integration for package distribution
   - Health checks and monitoring
   - Deployment automation

### Phase 4: Advanced Features (Ongoing)
1. **Registry Federation**
   - Multi-registry support
   - Private registry mirroring
   - Cross-registry package resolution

2. **Developer Experience**
   - Web dashboard for package management
   - Webhook notifications
   - Analytics and usage reporting
   - Advanced search capabilities

## Deployment Architecture

### Server Infrastructure

**Recommended Stack:**
- **Load Balancer**: ALB with HTTP/3 support
- **Application Servers**: ECS/EKS with auto-scaling
- **Database**: Amazon RDS PostgreSQL with read replicas
- **Object Storage**: S3 with CloudFront CDN
- **Cache**: ElastiCache Redis cluster
- **Monitoring**: CloudWatch + Prometheus + Grafana

**Scaling Considerations:**
- Stateless server design for horizontal scaling
- Database read replicas for query performance
- CDN for global package distribution
- Connection pooling for database efficiency

### Resource Requirements

**Minimum Production Setup:**
- **Server**: 4 vCPU, 8GB RAM, 100GB SSD
- **Database**: 2 vCPU, 4GB RAM, 100GB storage
- **Cache**: 1GB Redis instance
- **Network**: 1Gbps bandwidth

**High-Availability Setup:**
- **Servers**: 3+ instances across AZs
- **Database**: Primary + 2 read replicas
- **Cache**: Redis cluster with failover
- **Storage**: Multi-region replication

## Future Considerations

### Scalability Extensions

**Registry Federation:**
- Multi-registry package resolution
- Private registry mirroring and synchronization
- Cross-registry dependency resolution

**Performance Optimizations:**
- Package deduplication and delta updates
- Parallel download streams
- Smart caching based on usage patterns

### Protocol Evolution

**HTTP/4 and Beyond:**
- Monitor evolution of HTTP protocols
- WebTransport integration possibilities
- Enhanced streaming capabilities

**Security Enhancements:**
- Package signing with cryptographic verification
- Zero-trust architecture implementation
- Advanced threat detection and mitigation

This architecture provides a robust foundation for the HPM package registry, optimized for performance, security, and scalability while leveraging the latest networking technologies available in the Rust ecosystem.