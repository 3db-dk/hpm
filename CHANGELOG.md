# Changelog

All notable changes to HPM (Houdini Package Manager) will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.2] - 2026-03-26

### Fixed
- Handle platform-specific builds in registry `get_version`
- Make asset uploads idempotent, improve unwrap safety

## [0.3.0] - 2026-03-25

### Added
- `[native]` section in `hpm.toml` for declaring platform-specific files
- `hpm pack --platform` flag for producing per-platform archives
- `Platform` type with support for `linux-x86_64`, `macos-universal`, `windows-x86_64`
- Auto-detection of host platform when packing native packages
- `platform` field on registry entries for future install-time platform selection

## [0.1.0] - Initial Release

### Package Management
- `hpm init` - Package initialization with standard and bare templates
- `hpm add` - Add dependencies (registry, path sources) with version specifications
- `hpm remove` - Remove dependencies from manifest
- `hpm install` - Install all dependencies with lock file support (`--frozen-lockfile` for CI)
- `hpm update` - Update dependencies to latest compatible versions
- `hpm list` - List dependencies with tree view
- `hpm check` - Validate package configuration
- `hpm pack` - Create signed package archives
- `hpm clean` - Remove orphaned packages and virtual environments
- `hpm audit` - Security audit on dependencies
- `hpm completions` - Shell completion generation (bash, zsh, fish, powershell)

### Dependency Resolution
- PubGrub-based resolver with conflict learning and backtracking
- Lock file (`hpm.lock`) with pinned versions and checksums
- Registry, path, and URL dependency sources

### Python Integration
- Virtual environment isolation with content-addressable sharing
- UV-powered dependency resolution
- Automatic Python version mapping based on Houdini version
- PYTHONPATH injection via generated Houdini `package.json`

### Storage
- Global package storage in `~/.hpm/packages/`
- Content-addressable Python venvs in `~/.hpm/venvs/`
- Project-aware cleanup with orphan detection

### Security
- SHA-256 checksums for package verification
- Ed25519 package signing and verification
- Pure Rust TLS (rustls) for all network communication
