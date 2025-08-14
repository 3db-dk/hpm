//! Package-related types and utilities

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub versions: HashMap<String, PackageVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageVersion {
    pub version: String,
    pub metadata: PackageMetadata,
    pub published_at: DateTime<Utc>,
    pub published_by: String,
    pub checksum: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub dependencies: HashMap<String, String>,
    pub houdini: HoudiniRequirements,
    pub keywords: Vec<String>,
    pub readme: Option<String>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoudiniRequirements {
    pub min_version: Option<String>,
    pub max_version: Option<String>,
    pub platforms: Vec<String>,
}

impl From<crate::proto::PackageInfo> for PackageVersion {
    fn from(info: crate::proto::PackageInfo) -> Self {
        Self {
            version: info.version.clone(),
            metadata: PackageMetadata {
                name: info.name,
                version: info.version,
                description: info.description,
                authors: info.authors,
                license: if info.license.is_empty() {
                    None
                } else {
                    Some(info.license)
                },
                dependencies: info.dependencies,
                houdini: HoudiniRequirements {
                    min_version: info.houdini.as_ref().and_then(|h| {
                        if h.min_version.is_empty() {
                            None
                        } else {
                            Some(h.min_version.clone())
                        }
                    }),
                    max_version: info.houdini.as_ref().and_then(|h| {
                        if h.max_version.is_empty() {
                            None
                        } else {
                            Some(h.max_version.clone())
                        }
                    }),
                    platforms: info.houdini.map(|h| h.platforms).unwrap_or_default(),
                },
                keywords: info.keywords,
                readme: if info.readme.is_empty() {
                    None
                } else {
                    Some(info.readme)
                },
                repository: if info.repository.is_empty() {
                    None
                } else {
                    Some(info.repository)
                },
                homepage: if info.homepage.is_empty() {
                    None
                } else {
                    Some(info.homepage)
                },
            },
            published_at: DateTime::from_timestamp(info.published_at, 0).unwrap_or_else(Utc::now),
            published_by: "unknown".to_string(), // This would come from auth context
            checksum: info.checksum,
            size_bytes: info.size_bytes as u64,
        }
    }
}

impl From<PackageVersion> for crate::proto::PackageInfo {
    fn from(version: PackageVersion) -> Self {
        Self {
            name: version.metadata.name,
            version: version.metadata.version,
            description: version.metadata.description,
            authors: version.metadata.authors,
            license: version.metadata.license.unwrap_or_default(),
            dependencies: version.metadata.dependencies,
            houdini: Some(crate::proto::HoudiniRequirements {
                min_version: version.metadata.houdini.min_version.unwrap_or_default(),
                max_version: version.metadata.houdini.max_version.unwrap_or_default(),
                platforms: version.metadata.houdini.platforms,
            }),
            size_bytes: version.size_bytes as i64,
            checksum: version.checksum,
            published_at: version.published_at.timestamp(),
            keywords: version.metadata.keywords,
            readme: version.metadata.readme.unwrap_or_default(),
            repository: version.metadata.repository.unwrap_or_default(),
            homepage: version.metadata.homepage.unwrap_or_default(),
        }
    }
}
