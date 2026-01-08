//! Machine-readable output formats for HPM CLI
//!
//! This module provides structured output formats for programmatic consumption
//! of HPM command results. It supports multiple formats suitable for different
//! use cases:
//!
//! - **JSON**: Pretty-printed JSON for human-readable automation
//! - **JSON Lines**: One JSON object per line for streaming processing
//! - **JSON Compact**: Minified JSON for bandwidth-sensitive applications
//! - **Human**: Traditional human-readable output (default)
//!
//! # Design Goals
//!
//! 1. **Automation Friendly**: Enable easy parsing by scripts and tools
//! 2. **Consistent Schema**: Predictable structure across all commands
//! 3. **Bandwidth Efficient**: Multiple formatting options for different needs
//! 4. **Future Extensible**: Foundation for additional formats (CSV, YAML, etc.)
//!
//! # Usage Patterns
//!
//! ## Continuous Integration
//! ```bash
//! # Parse results in CI/CD pipelines
//! hpm --output json install | jq '.success'
//! ```
//!
//! ## Log Processing  
//! ```bash
//! # Stream results for log analysis
//! hpm --output json-lines clean | while read line; do
//!   echo "$line" | jq '.packages_removed[]'
//! done
//! ```
//!
//! ## API Integration
//! ```bash
//! # Compact output for network efficiency
//! hpm --output json-compact list --package project.toml
//! ```
//!
//! # Examples
//!
//! ```rust
//! use hpm_cli::output::{OutputFormat, CommandResult, Output};
//!
//! // Create a success result
//! let result = CommandResult::success("install");
//!
//! // Output in different formats
//! result.print(OutputFormat::Json).unwrap();      // Pretty JSON
//! result.print(OutputFormat::JsonLines).unwrap(); // Single line JSON
//! result.print(OutputFormat::Human).unwrap();     // Human readable
//! ```

use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::io::{self, Write};

/// Output format options for HPM commands
///
/// Determines how command results are formatted and displayed.
/// Each format serves different use cases and consumption patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable output (default)
    ///
    /// Traditional CLI output with colors, formatting, and user-friendly messages.
    /// Best for interactive terminal use.
    Human,

    /// JSON output for programmatic use
    ///
    /// Pretty-printed JSON with indentation and line breaks.
    /// Good balance between readability and machine processing.
    Json,

    /// JSON Lines output for streaming
    ///
    /// One JSON object per line without pretty printing.
    /// Ideal for streaming, logging, and line-by-line processing.
    JsonLines,

    /// Compact JSON without pretty printing
    ///
    /// Minified JSON without whitespace or line breaks.
    /// Optimal for network transmission and storage efficiency.
    JsonCompact,
}

impl OutputFormat {
    /// Parse an output format from a string
    ///
    /// Supports case-insensitive matching of format names.
    ///
    /// # Arguments
    ///
    /// * `s` - The format name to parse
    ///
    /// # Returns
    ///
    /// Some(OutputFormat) if the string matches a known format, None otherwise.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use hpm_cli::output::OutputFormat;
    ///
    /// assert_eq!(OutputFormat::from_str("json"), Some(OutputFormat::Json));
    /// assert_eq!(OutputFormat::from_str("JSON"), Some(OutputFormat::Json));
    /// assert_eq!(OutputFormat::from_str("jsonl"), Some(OutputFormat::JsonLines));
    /// assert_eq!(OutputFormat::from_str("invalid"), None);
    /// ```
    #[allow(dead_code)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "human" => Some(Self::Human),
            "json" => Some(Self::Json),
            "json-lines" | "jsonl" => Some(Self::JsonLines),
            "json-compact" => Some(Self::JsonCompact),
            _ => None,
        }
    }
}

impl Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Human => write!(f, "human"),
            Self::Json => write!(f, "json"),
            Self::JsonLines => write!(f, "json-lines"),
            Self::JsonCompact => write!(f, "json-compact"),
        }
    }
}

/// Trait for objects that can be output in different formats
///
/// This trait enables consistent formatting across different HPM data types.
/// Implementors can support multiple output formats while maintaining a
/// unified interface.
///
/// # Examples
///
/// ```rust
/// use hpm_cli::output::{Output, OutputFormat};
/// use std::io::Write;
///
/// struct MyData {
///     name: String,
///     value: i32,
/// }
///
/// impl Output for MyData {
///     fn output(&self, format: OutputFormat, writer: &mut dyn Write) -> std::io::Result<()> {
///         match format {
///             OutputFormat::Human => writeln!(writer, "{}: {}", self.name, self.value),
///             OutputFormat::Json => writeln!(writer, "{{\"name\":\"{}\",\"value\":{}}}", self.name, self.value),
///             _ => writeln!(writer, "{}={}", self.name, self.value),
///         }
///     }
/// }
/// ```
#[allow(dead_code)]
trait Output {
    /// Output the data in the specified format to a writer
    ///
    /// # Arguments
    ///
    /// * `format` - The output format to use
    /// * `writer` - The writer to output to
    ///
    /// # Returns
    ///
    /// Ok(()) on success, or an IO error if writing fails
    fn output(&self, format: OutputFormat, writer: &mut dyn Write) -> io::Result<()>;

    /// Output to stdout with the specified format
    ///
    /// Convenience method that writes to stdout. Uses a locked stdout
    /// handle for thread safety.
    ///
    /// # Arguments
    ///
    /// * `format` - The output format to use
    ///
    /// # Returns
    ///
    /// Ok(()) on success, or an IO error if writing fails
    fn print(&self, format: OutputFormat) -> io::Result<()> {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        self.output(format, &mut handle)
    }
}

/// Command execution result for structured output
///
/// Represents the outcome of a CLI command in a consistent format
/// suitable for machine processing. All HPM commands can produce
/// CommandResult objects for JSON output.
///
/// # JSON Schema
///
/// ```json
/// {
///   "success": boolean,        // true if command succeeded
///   "command": string,         // name of the command executed
///   "message": string|null,    // optional human-readable message
///   "data": object|null,       // optional structured result data
///   "elapsed_ms": number|null  // execution time in milliseconds
/// }
/// ```
///
/// # Examples
///
/// ```rust
/// use hpm_cli::output::CommandResult;
/// use serde_json::json;
///
/// // Simple success
/// let result = CommandResult::success("install");
///
/// // Success with structured data
/// let result = CommandResult::success_with_data(
///     "list",
///     json!({"packages": ["foo", "bar"]})
/// );
///
/// // Failure with error message
/// let result = CommandResult::failure("install", "Package not found");
/// ```
#[derive(Debug, Serialize, Deserialize)]
pub struct CommandResult {
    /// Whether the command completed successfully
    pub success: bool,

    /// The name of the command that was executed
    pub command: String,

    /// Optional human-readable message
    pub message: Option<String>,

    /// Optional structured data (command-specific)
    pub data: Option<serde_json::Value>,

    /// Optional execution time in milliseconds
    pub elapsed_ms: Option<u64>,
}

impl CommandResult {
    /// Create a successful command result
    ///
    /// # Arguments
    ///
    /// * `command` - The name of the command that succeeded
    ///
    /// # Examples
    ///
    /// ```rust
    /// let result = CommandResult::success("install");
    /// assert!(result.success);
    /// assert_eq!(result.command, "install");
    /// ```
    #[allow(dead_code)]
    pub fn success(command: &str) -> Self {
        Self {
            success: true,
            command: command.to_string(),
            message: None,
            data: None,
            elapsed_ms: None,
        }
    }

    /// Create a successful command result with structured data
    ///
    /// # Arguments
    ///
    /// * `command` - The name of the command that succeeded
    /// * `data` - Structured result data
    ///
    /// # Examples
    ///
    /// ```rust
    /// use serde_json::json;
    ///
    /// let result = CommandResult::success_with_data(
    ///     "list",
    ///     json!({"packages": ["foo", "bar"]})
    /// );
    /// ```
    #[allow(dead_code)]
    pub fn success_with_data(command: &str, data: serde_json::Value) -> Self {
        Self {
            success: true,
            command: command.to_string(),
            message: None,
            data: Some(data),
            elapsed_ms: None,
        }
    }

    /// Create a failed command result
    ///
    /// # Arguments
    ///
    /// * `command` - The name of the command that failed
    /// * `message` - Error message describing the failure
    ///
    /// # Examples
    ///
    /// ```rust
    /// let result = CommandResult::failure("install", "Package not found");
    /// assert!(!result.success);
    /// assert_eq!(result.message, Some("Package not found".to_string()));
    /// ```
    #[allow(dead_code)]
    pub fn failure(command: &str, message: &str) -> Self {
        Self {
            success: false,
            command: command.to_string(),
            message: Some(message.to_string()),
            data: None,
            elapsed_ms: None,
        }
    }

    /// Add execution time to the result (builder pattern)
    ///
    /// # Arguments
    ///
    /// * `elapsed_ms` - Execution time in milliseconds
    ///
    /// # Examples
    ///
    /// ```rust
    /// let result = CommandResult::success("install").with_elapsed(1250);
    /// assert_eq!(result.elapsed_ms, Some(1250));
    /// ```
    #[allow(dead_code)]
    pub fn with_elapsed(mut self, elapsed_ms: u64) -> Self {
        self.elapsed_ms = Some(elapsed_ms);
        self
    }

    /// Add a message to the result (builder pattern)
    ///
    /// # Arguments
    ///
    /// * `message` - Human-readable message
    ///
    /// # Examples
    ///
    /// ```rust
    /// let result = CommandResult::success("install")
    ///     .with_message("3 packages installed");
    /// ```
    #[allow(dead_code)]
    pub fn with_message(mut self, message: &str) -> Self {
        self.message = Some(message.to_string());
        self
    }
}

impl Output for CommandResult {
    fn output(&self, format: OutputFormat, writer: &mut dyn Write) -> io::Result<()> {
        match format {
            OutputFormat::Human => {
                if self.success {
                    writeln!(
                        writer,
                        "\u{2713} Command '{}' completed successfully", // Unicode check mark - CLI success symbol
                        self.command
                    )?;
                    if let Some(ref message) = self.message {
                        writeln!(writer, "  {}", message)?;
                    }
                } else {
                    writeln!(
                        writer,
                        "\u{2717} Command '{}' failed", // Unicode ballot X - CLI error symbol
                        self.command
                    )?;
                    if let Some(ref message) = self.message {
                        writeln!(writer, "  Error: {}", message)?;
                    }
                }
                if let Some(elapsed) = self.elapsed_ms {
                    writeln!(writer, "  Completed in {}ms", elapsed)?;
                }
            }
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(self)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                writeln!(writer, "{}", json)?;
            }
            OutputFormat::JsonLines => {
                let json = serde_json::to_string(self)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                writeln!(writer, "{}", json)?;
            }
            OutputFormat::JsonCompact => {
                let json = serde_json::to_string(self)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                write!(writer, "{}", json)?;
            }
        }
        Ok(())
    }
}

//
// Future data structures for structured output
// These will be used when implementing full JSON output for specific commands
//

/// Package information for list command (future feature)
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
struct PackageInfo {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub houdini_min: Option<String>,
    pub houdini_max: Option<String>,
    pub dependencies: Vec<DependencyInfo>,
    pub python_dependencies: Vec<PythonDependencyInfo>,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
struct DependencyInfo {
    pub name: String,
    pub version: String,
    pub optional: bool,
    pub source: DependencySource,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
enum DependencySource {
    Git {
        url: String,
        commit: String,
    },
    Path {
        path: String,
    },
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
struct PythonDependencyInfo {
    pub name: String,
    pub version: String,
    pub extras: Vec<String>,
    pub optional: bool,
}

#[allow(dead_code)]
impl Output for PackageInfo {
    fn output(&self, format: OutputFormat, writer: &mut dyn Write) -> io::Result<()> {
        match format {
            OutputFormat::Human => {
                writeln!(writer, "Package: {} v{}", self.name, self.version)?;
                if let Some(ref desc) = self.description {
                    writeln!(writer, "Description: {}", desc)?;
                }
                if !self.authors.is_empty() {
                    writeln!(writer, "Authors: {}", self.authors.join(", "))?;
                }
                if let Some(ref license) = self.license {
                    writeln!(writer, "License: {}", license)?;
                }

                // Houdini compatibility
                match (&self.houdini_min, &self.houdini_max) {
                    (Some(min), Some(max)) => {
                        writeln!(writer, "Houdini compatibility: {} - {}", min, max)?
                    }
                    (Some(min), None) => {
                        writeln!(writer, "Houdini compatibility: {} or later", min)?
                    }
                    (None, Some(max)) => {
                        writeln!(writer, "Houdini compatibility: {} or earlier", max)?
                    }
                    (None, None) => {}
                }

                if !self.dependencies.is_empty() {
                    writeln!(writer, "\nHPM Dependencies:")?;
                    for dep in &self.dependencies {
                        write!(writer, "  {} {}", dep.name, dep.version)?;
                        if dep.optional {
                            write!(writer, " (optional)")?;
                        }
                        match &dep.source {
                            DependencySource::Git { url, commit } => {
                                write!(writer, " git: {} ({})", url, &commit[..commit.len().min(12)])?;
                            }
                            DependencySource::Path { path } => {
                                write!(writer, " path: {}", path)?;
                            }
                        }
                        writeln!(writer)?;
                    }
                }

                if !self.python_dependencies.is_empty() {
                    writeln!(writer, "\nPython Dependencies:")?;
                    for dep in &self.python_dependencies {
                        write!(writer, "  {} {}", dep.name, dep.version)?;
                        if dep.optional {
                            write!(writer, " (optional)")?;
                        }
                        if !dep.extras.is_empty() {
                            write!(writer, " [{}]", dep.extras.join(","))?;
                        }
                        writeln!(writer)?;
                    }
                }
            }
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(self)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                writeln!(writer, "{}", json)?;
            }
            OutputFormat::JsonLines => {
                let json = serde_json::to_string(self)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                writeln!(writer, "{}", json)?;
            }
            OutputFormat::JsonCompact => {
                let json = serde_json::to_string(self)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                write!(writer, "{}", json)?;
            }
        }
        Ok(())
    }
}

/// Clean command result (future feature)
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
struct CleanResult {
    pub packages_removed: Vec<String>,
    pub packages_kept: Vec<String>,
    pub python_envs_removed: Vec<String>,
    pub python_envs_kept: Vec<String>,
    pub total_space_freed: u64,
    pub dry_run: bool,
}

#[allow(dead_code)]
impl Output for CleanResult {
    fn output(&self, format: OutputFormat, writer: &mut dyn Write) -> io::Result<()> {
        match format {
            OutputFormat::Human => {
                if self.dry_run {
                    writeln!(writer, "Dry run - no changes made")?;
                }

                writeln!(writer, "Packages removed: {}", self.packages_removed.len())?;
                for pkg in &self.packages_removed {
                    writeln!(writer, "  - {}", pkg)?;
                }

                if !self.python_envs_removed.is_empty() {
                    writeln!(
                        writer,
                        "Python environments removed: {}",
                        self.python_envs_removed.len()
                    )?;
                    for env in &self.python_envs_removed {
                        writeln!(writer, "  - {}", env)?;
                    }
                }

                if self.total_space_freed > 0 {
                    writeln!(
                        writer,
                        "Total space freed: {} bytes",
                        self.total_space_freed
                    )?;
                }
            }
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(self)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                writeln!(writer, "{}", json)?;
            }
            OutputFormat::JsonLines => {
                let json = serde_json::to_string(self)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                writeln!(writer, "{}", json)?;
            }
            OutputFormat::JsonCompact => {
                let json = serde_json::to_string(self)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                write!(writer, "{}", json)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_output_format_from_str() {
        assert_eq!(OutputFormat::from_str("human"), Some(OutputFormat::Human));
        assert_eq!(OutputFormat::from_str("json"), Some(OutputFormat::Json));
        assert_eq!(OutputFormat::from_str("JSON"), Some(OutputFormat::Json));
        assert_eq!(
            OutputFormat::from_str("json-lines"),
            Some(OutputFormat::JsonLines)
        );
        assert_eq!(
            OutputFormat::from_str("jsonl"),
            Some(OutputFormat::JsonLines)
        );
        assert_eq!(OutputFormat::from_str("invalid"), None);
    }

    #[test]
    fn test_command_result_output() {
        let result = CommandResult::success("test");
        let mut buffer = Cursor::new(Vec::new());

        result.output(OutputFormat::Human, &mut buffer).unwrap();
        let output = String::from_utf8(buffer.into_inner()).unwrap();
        assert!(output.contains("Command 'test' completed successfully"));
    }

    #[test]
    fn test_command_result_json_output() {
        let result = CommandResult::success("test");
        let mut buffer = Cursor::new(Vec::new());

        result.output(OutputFormat::Json, &mut buffer).unwrap();
        let output = String::from_utf8(buffer.into_inner()).unwrap();

        let parsed: CommandResult = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed.command, "test");
        assert!(parsed.success);
    }
}
