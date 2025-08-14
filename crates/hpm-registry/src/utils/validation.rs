//! Package validation utilities

use crate::types::RegistryError;
use sha2::{Digest, Sha256};

pub const MAX_PACKAGE_SIZE: u64 = 500 * 1024 * 1024; // 500MB
pub const MAX_PACKAGE_NAME_LENGTH: usize = 100;
pub const MAX_DESCRIPTION_LENGTH: usize = 1000;

pub fn validate_package_name(name: &str) -> Result<(), RegistryError> {
    if name.is_empty() {
        return Err(RegistryError::InvalidPackageData(
            "Package name cannot be empty".to_string(),
        ));
    }

    if name.len() > MAX_PACKAGE_NAME_LENGTH {
        return Err(RegistryError::InvalidPackageData(format!(
            "Package name too long: {} characters (max: {})",
            name.len(),
            MAX_PACKAGE_NAME_LENGTH
        )));
    }

    // Package names should contain only lowercase letters, numbers, and hyphens
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(RegistryError::InvalidPackageData(
            "Package name can only contain lowercase letters, numbers, and hyphens".to_string(),
        ));
    }

    // Cannot start or end with hyphen
    if name.starts_with('-') || name.ends_with('-') {
        return Err(RegistryError::InvalidPackageData(
            "Package name cannot start or end with hyphen".to_string(),
        ));
    }

    Ok(())
}

pub fn validate_package_version(version: &str) -> Result<(), RegistryError> {
    if version.is_empty() {
        return Err(RegistryError::InvalidPackageData(
            "Package version cannot be empty".to_string(),
        ));
    }

    // Basic semver validation (simplified)
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 {
        return Err(RegistryError::InvalidPackageData(
            "Package version must be in format X.Y.Z".to_string(),
        ));
    }

    for part in parts {
        if part.parse::<u32>().is_err() {
            return Err(RegistryError::InvalidPackageData(format!(
                "Invalid version component: {}",
                part
            )));
        }
    }

    Ok(())
}

pub fn validate_package_size(size: u64) -> Result<(), RegistryError> {
    if size > MAX_PACKAGE_SIZE {
        return Err(RegistryError::PackageTooLarge {
            size,
            max_size: MAX_PACKAGE_SIZE,
        });
    }
    Ok(())
}

pub fn calculate_checksum(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(&hasher.finalize())
}

pub fn verify_checksum(data: &[u8], expected_checksum: &str) -> Result<(), RegistryError> {
    let actual_checksum = calculate_checksum(data);
    if actual_checksum != expected_checksum {
        return Err(RegistryError::ChecksumMismatch {
            expected: expected_checksum.to_string(),
            actual: actual_checksum,
        });
    }
    Ok(())
}

// Simple hex encoding (in production, use hex crate)
mod hex {
    pub fn encode(data: &[u8]) -> String {
        data.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_package_name() {
        assert!(validate_package_name("valid-package").is_ok());
        assert!(validate_package_name("package123").is_ok());
        assert!(validate_package_name("my-package-name").is_ok());

        assert!(validate_package_name("").is_err());
        assert!(validate_package_name("Invalid-Name").is_err());
        assert!(validate_package_name("-invalid").is_err());
        assert!(validate_package_name("invalid-").is_err());
        assert!(validate_package_name("invalid_name").is_err());
    }

    #[test]
    fn test_validate_package_version() {
        assert!(validate_package_version("1.0.0").is_ok());
        assert!(validate_package_version("2.1.3").is_ok());
        assert!(validate_package_version("10.20.30").is_ok());

        assert!(validate_package_version("").is_err());
        assert!(validate_package_version("1.0").is_err());
        assert!(validate_package_version("1.0.0.1").is_err());
        assert!(validate_package_version("1.a.0").is_err());
    }

    #[test]
    fn test_checksum() {
        let data = b"test data";
        let checksum = calculate_checksum(data);

        assert!(verify_checksum(data, &checksum).is_ok());
        assert!(verify_checksum(b"different data", &checksum).is_err());
    }
}
