//! Project-discovery settings: where to look for HPM-managed projects and
//! which directories to skip while traversing.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectsConfig {
    pub explicit_paths: Vec<PathBuf>,
    pub search_roots: Vec<PathBuf>,
    pub max_search_depth: usize,
    pub ignore_patterns: Vec<String>,
}

impl Default for ProjectsConfig {
    fn default() -> Self {
        Self {
            explicit_paths: vec![],
            search_roots: vec![],
            max_search_depth: 3,
            ignore_patterns: vec![
                ".git".to_string(),
                ".hg".to_string(),
                ".svn".to_string(),
                "node_modules".to_string(),
                "backup".to_string(),
                "archive".to_string(),
                ".cache".to_string(),
                "temp".to_string(),
                "tmp".to_string(),
            ],
        }
    }
}

impl ProjectsConfig {
    pub fn add_explicit_path(&mut self, path: PathBuf) {
        if !self.explicit_paths.contains(&path) {
            self.explicit_paths.push(path);
        }
    }

    pub fn add_search_root(&mut self, path: PathBuf) {
        if !self.search_roots.contains(&path) {
            self.search_roots.push(path);
        }
    }

    pub fn should_ignore(&self, dir_name: &str) -> bool {
        self.ignore_patterns
            .iter()
            .any(|pattern| dir_name == pattern || dir_name.starts_with(pattern))
    }
}
