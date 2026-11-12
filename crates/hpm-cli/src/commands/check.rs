//! `hpm check`: render [`hpm_core::check::check_package`]'s report.

use crate::console::Console;
use crate::output::OutputFormat;
use anyhow::Result;
use hpm_core::check::{Report, Severity};

/// CLI entry for `hpm check`: validate, display the results through the
/// console (or as JSON), and fail when errors were found.
pub fn check_package(
    manifest: Option<std::path::PathBuf>,
    console: &mut Console,
    output: OutputFormat,
) -> Result<()> {
    let directory = super::manifest_utils::package_dir(manifest)?;
    let report = hpm_core::check::check_package(&directory);
    display_results(&report, console, output);
    if !report.is_valid() {
        anyhow::bail!(
            "Package validation failed with {} error(s)",
            report.messages(Severity::Error).count()
        );
    }
    Ok(())
}

/// The informational lines, in the order the checks ran: passed checks
/// carry an `[OK]` prefix, neutral information none.
fn info_lines(report: &Report) -> Vec<String> {
    report
        .findings
        .iter()
        .filter_map(|f| match f.severity {
            Severity::Passed => Some(format!("[OK] {}", f.message)),
            Severity::Info => Some(f.message.clone()),
            Severity::Error | Severity::Warning => None,
        })
        .collect()
}

/// Emit the validation findings. Human output goes through the console —
/// info lines are suppressed by `--quiet` while warnings, errors, and the
/// final verdict always show; JSON output is a single document on stdout.
fn display_results(report: &Report, console: &mut Console, output: OutputFormat) {
    let errors: Vec<&str> = report.messages(Severity::Error).collect();
    let warnings: Vec<&str> = report.messages(Severity::Warning).collect();
    if output.is_json() {
        let doc = serde_json::json!({
            "valid": report.is_valid(),
            "errors": errors,
            "warnings": warnings,
            "info": info_lines(report),
        });
        console.stdout(output.render_json(&doc));
        return;
    }

    for info in info_lines(report) {
        console.info(info);
    }
    for warning in &warnings {
        console.warn(warning);
    }
    for error in &errors {
        console.error(error);
    }

    if report.is_valid() {
        console.success("Package validation completed successfully");
        if !warnings.is_empty() {
            console.status(format!(
                "  {} warning(s) found - consider addressing them",
                warnings.len()
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hpm_core::check::Finding;

    #[test]
    fn info_lines_prefix_passed_checks_and_keep_the_run_order() {
        let finding = |severity, message: &str| Finding {
            severity,
            message: message.to_string(),
        };
        let report = Report {
            findings: vec![
                finding(Severity::Passed, "hpm.toml found"),
                finding(Severity::Warning, "w"),
                finding(Severity::Info, "No version control detected"),
                finding(Severity::Error, "e"),
                finding(Severity::Passed, "Found otls directory"),
            ],
        };
        assert_eq!(
            info_lines(&report),
            [
                "[OK] hpm.toml found",
                "No version control detected",
                "[OK] Found otls directory"
            ]
        );
    }
}
