//! Layered manifest validation: [`ValidationLevel`] picks which checks run
//! and [`ValidationReport`] separates hard errors from advisory warnings.

/// Which layer of checks [`super::PackageManifest::validate_with`] should run.
///
/// `Strict` is the gate every manifest must pass to load — structural
/// invariants only. `Publish` adds advisory checks about
/// publish-quality metadata (description, authors, keywords,
/// `[compat].houdini`); those land in [`ValidationReport::warnings`]
/// so callers like `hpm check` show them, while a future `hpm publish`
/// can promote them to errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationLevel {
    /// Structural validity. Errors only.
    Strict,
    /// Structural validity plus publish-quality advisory warnings.
    Publish,
}

/// Outcome of [`super::PackageManifest::validate_with`].
///
/// `errors` are structural failures that block downstream operations;
/// `warnings` are advisory and only populated at higher validation
/// levels. `is_ok()` ignores warnings.
#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationReport {
    /// True when no structural errors were collected. Warnings are
    /// ignored — they're advisory by definition.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}
