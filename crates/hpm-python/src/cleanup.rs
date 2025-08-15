//! Python virtual environment cleanup integration

use crate::types::OrphanedVenv;
use crate::venv::VenvManager;
use anyhow::Result;
use tracing::{debug, info};

/// Python cleanup analyzer for virtual environments
pub struct PythonCleanupAnalyzer {
    venv_manager: VenvManager,
}

impl PythonCleanupAnalyzer {
    pub fn new() -> Self {
        Self {
            venv_manager: VenvManager::new(),
        }
    }

    /// Analyze orphaned virtual environments
    pub async fn analyze_orphaned_venvs(
        &self,
        active_packages: &[String],
    ) -> Result<Vec<OrphanedVenv>> {
        debug!(
            "Analyzing orphaned virtual environments for {} active packages",
            active_packages.len()
        );

        let orphaned = self
            .venv_manager
            .find_orphaned_venvs(active_packages)
            .await?;

        info!("Found {} orphaned virtual environments", orphaned.len());
        Ok(orphaned)
    }

    /// Clean up orphaned virtual environments
    pub async fn cleanup_orphaned_venvs(
        &self,
        orphaned_venvs: &[OrphanedVenv],
        dry_run: bool,
    ) -> Result<CleanupResult> {
        let mut result = CleanupResult::default();

        for venv in orphaned_venvs {
            if dry_run {
                result.would_remove.push(venv.path.clone());
                result.space_that_would_be_freed += venv.size;
            } else {
                self.venv_manager.remove_venv(&venv.path).await?;
                result.removed.push(venv.path.clone());
                result.space_freed += venv.size;
            }
        }

        Ok(result)
    }

    /// Get virtual environment usage statistics
    pub async fn get_venv_stats(&self) -> Result<VenvStats> {
        let all_venvs = self.venv_manager.list_all_venvs().await?;
        let mut stats = VenvStats::default();

        for venv_meta in all_venvs {
            stats.total_count += 1;

            if venv_meta.is_orphaned() {
                stats.orphaned_count += 1;
            } else {
                stats.active_count += 1;
            }

            if let Ok(size) = self.venv_manager.calculate_venv_size(&venv_meta.path).await {
                stats.total_size += size;

                if venv_meta.is_orphaned() {
                    stats.orphaned_size += size;
                } else {
                    stats.active_size += size;
                }
            }
        }

        Ok(stats)
    }
}

impl Default for PythonCleanupAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of cleanup operations
#[derive(Debug, Default)]
pub struct CleanupResult {
    pub removed: Vec<std::path::PathBuf>,
    pub would_remove: Vec<std::path::PathBuf>,
    pub space_freed: u64,
    pub space_that_would_be_freed: u64,
}

impl CleanupResult {
    pub fn items_cleaned(&self) -> usize {
        self.removed.len()
    }

    pub fn items_that_would_be_cleaned(&self) -> usize {
        self.would_remove.len()
    }

    /// Format the space freed in a human-readable way
    pub fn format_space_freed(&self) -> String {
        format_size(self.space_freed)
    }

    /// Format the space that would be freed in a human-readable way
    pub fn format_space_that_would_be_freed(&self) -> String {
        format_size(self.space_that_would_be_freed)
    }
}

/// Virtual environment statistics
#[derive(Debug, Default)]
pub struct VenvStats {
    pub total_count: usize,
    pub active_count: usize,
    pub orphaned_count: usize,
    pub total_size: u64,
    pub active_size: u64,
    pub orphaned_size: u64,
}

impl VenvStats {
    /// Format total size in a human-readable way
    pub fn format_total_size(&self) -> String {
        format_size(self.total_size)
    }

    /// Format active size in a human-readable way
    pub fn format_active_size(&self) -> String {
        format_size(self.active_size)
    }

    /// Format orphaned size in a human-readable way
    pub fn format_orphaned_size(&self) -> String {
        format_size(self.orphaned_size)
    }
}

/// Format byte size in human-readable format
fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    const THRESHOLD: u64 = 1024;

    if bytes == 0 {
        return "0 B".to_string();
    }

    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= THRESHOLD as f64 && unit_index < UNITS.len() - 1 {
        size /= THRESHOLD as f64;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
        assert_eq!(format_size(1073741824), "1.0 GB");
    }

    #[test]
    fn test_cleanup_result() {
        let result = CleanupResult {
            space_freed: 1536,
            space_that_would_be_freed: 2048,
            ..Default::default()
        };

        assert_eq!(result.format_space_freed(), "1.5 KB");
        assert_eq!(result.format_space_that_would_be_freed(), "2.0 KB");
    }

    #[tokio::test]
    async fn test_python_cleanup_analyzer_creation() {
        let _analyzer = PythonCleanupAnalyzer::new();
        // Just test that it can be created without errors
    }
}
