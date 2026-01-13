# HPM Security Guide

This document covers HPM's security features, best practices, and threat model to help you secure your Houdini package management workflow.

## Security Features

### Package Integrity Verification

HPM uses SHA-256 checksums to verify package integrity:

- **Automatic checksum computation**: When packages are first downloaded, HPM computes a SHA-256 hash of all package contents
- **Stored in lock file**: Checksums are stored in `hpm.lock` for reproducibility across machines and time
- **Pre-install verification**: Before every installation, HPM verifies packages against their stored checksums
- **Tamper detection**: Tampered or corrupted packages are rejected with clear error messages

```bash
# Checksums are stored in hpm.lock like this:
[dependencies.utility-nodes]
version = "1.2.0"
checksum = "sha256:a3f2b8c9..."
```

### Transport Security

- **HTTPS recommended**: HPM warns when HTTP URLs are used for Git dependencies
- **TLS encryption**: All network connections use TLS for secure transport
- **Warning system**: Clear warnings are displayed when insecure URLs are detected

```bash
# This will show a security warning:
hpm add --git http://github.com/user/repo --version 1.0.0
# Warning: Using HTTP instead of HTTPS for Git URL
```

### Reproducible Builds

HPM ensures build reproducibility through several mechanisms:

- **Lock file pinning**: `hpm.lock` pins exact versions and checksums
- **Frozen lockfile mode**: Use `--frozen-lockfile` in CI to prevent drift
- **Staleness detection**: Lock file includes timestamps to detect outdated dependencies

```bash
# CI/CD recommended usage:
hpm install --frozen-lockfile
```

## Security Commands

### hpm audit

The `hpm audit` command scans your project for security issues:

```bash
hpm audit
```

**Checks performed:**

| Check | Description |
|-------|-------------|
| HTTP URLs | Warns about dependencies using insecure HTTP URLs |
| Lock file presence | Verifies `hpm.lock` exists for reproducible builds |
| Lock file staleness | Warns if lock file is older than 90 days |
| Checksum verification | Validates all package checksums against stored values |

**Example output:**

```
HPM Security Audit
========================================

  PASS All Git URLs use HTTPS
  PASS Lock file exists (hpm.lock)
  PASS Lock file is recent
  PASS Package checksums verified

No security issues found.
```

### hpm install --frozen-lockfile

Ensures reproducible builds in CI environments:

```bash
hpm install --frozen-lockfile
```

This command **fails** if:

- Lock file (`hpm.lock`) doesn't exist
- Lock file would need to be updated (dependencies changed)

Use this in CI pipelines to catch dependency drift before deployment.

## Best Practices

### 1. Use HTTPS URLs

Always use HTTPS for Git dependencies to prevent man-in-the-middle attacks:

```toml
# Good - uses HTTPS
[dependencies]
my-package = { git = "https://github.com/studio/my-package", version = "1.0.0" }

# Bad - uses HTTP (will trigger warnings)
my-package = { git = "http://github.com/studio/my-package", version = "1.0.0" }
```

### 2. Commit hpm.lock

Always commit your lock file to version control:

```bash
# Add lock file to git
git add hpm.lock
git commit -m "Add hpm.lock for reproducible builds"
```

This ensures:
- All team members use identical package versions
- CI/CD builds are reproducible
- Checksum verification is possible

### 3. Use --frozen-lockfile in CI

Add `--frozen-lockfile` to your CI pipeline to catch dependency drift:

```yaml
# GitHub Actions example
- name: Install HPM dependencies
  run: hpm install --frozen-lockfile
```

### 4. Run hpm audit Regularly

Include `hpm audit` in your pre-release checklist:

```bash
# Run before major releases
hpm audit
```

Consider adding it to CI pipelines for automated security checks.

### 5. Update Dependencies Regularly

Stale dependencies may contain unpatched vulnerabilities:

```bash
# Check for updates
hpm update

# HPM will warn about lock files older than 90 days
```

### 6. Review Dependencies Before Adding

Before adding a new dependency:

- Verify the repository is legitimate
- Check for recent maintenance activity
- Review the package manifest for suspicious scripts

## Threat Model

### Threats Mitigated by HPM

| Threat | Attack Vector | Mitigation |
|--------|--------------|------------|
| Cache tampering | Attacker modifies cached packages | Pre-install checksum verification |
| Man-in-the-middle | Attacker intercepts network traffic | HTTPS transport + checksum verification |
| Lockfile poisoning | Attacker modifies hpm.lock checksums | Checksum mismatch detection on verification |
| Dependency drift | Different versions installed over time | Lock file pinning + frozen mode |
| Stale dependencies | Old packages with known vulnerabilities | Lock file timestamps + warnings |
| Replay attacks | Serving old vulnerable versions | Version pinning in lock file |

### Threats Not Addressed

HPM cannot protect against:

- **Malicious code in legitimate packages**: If a package author intentionally includes malware, HPM cannot detect it
- **Compromised upstream repositories**: If a repository is compromised at the source, HPM will still trust it
- **Zero-day vulnerabilities**: Unknown vulnerabilities cannot be detected
- **Supply chain attacks at source**: HPM trusts the packages it downloads from configured sources

For these threats, consider:
- Code review of dependencies
- Using only trusted, well-maintained packages
- Monitoring security advisories for your dependencies
- Using additional security scanning tools

## Lock File Security

The `hpm.lock` file is critical for security. Here's what it contains:

```toml
# HPM Lock File
# This file is auto-generated. Do not edit manually.

version = 1

[package]
name = "my-project"
version = "1.0.0"

[metadata]
generated_at = "2025-01-13T10:30:00Z"
hpm_version = "0.1.0"
platform = "windows-x86_64"

[dependencies.utility-nodes]
version = "1.2.0"
checksum = "sha256:a3f2b8c9d4e5f6..."

[dependencies.utility-nodes.source]
type = "git"
url = "https://github.com/studio/utility-nodes"
version = "1.2.0"
```

### Lock File Best Practices

1. **Never edit manually**: Let HPM manage the lock file
2. **Always commit**: Include in version control
3. **Verify after updates**: Review lock file changes in pull requests
4. **Keep up to date**: Regularly run `hpm update` to check for newer versions

## Configuration Security

### Global Configuration

HPM stores configuration in platform-specific locations:

| Platform | Location |
|----------|----------|
| Windows | `%APPDATA%\hpm\config.toml` |
| macOS | `~/Library/Application Support/hpm/config.toml` |
| Linux | `~/.config/hpm/config.toml` |

### Package Storage

Downloaded packages are stored in:

| Platform | Location |
|----------|----------|
| Windows | `%LOCALAPPDATA%\hpm\packages` |
| macOS | `~/Library/Caches/hpm/packages` |
| Linux | `~/.cache/hpm/packages` |

Ensure these directories have appropriate permissions to prevent unauthorized access.

## Reporting Security Issues

If you discover a security vulnerability in HPM:

1. **Do not** open a public issue
2. Report privately via email or security advisory
3. Include steps to reproduce the vulnerability
4. Allow time for a fix before public disclosure

## Security Changelog

| Version | Security Change |
|---------|----------------|
| 0.1.0 | Initial security features |
| - | SHA-256 checksum verification |
| - | HTTPS URL warnings |
| - | Frozen lockfile mode |
| - | Security audit command |
| - | Lock file staleness detection |
