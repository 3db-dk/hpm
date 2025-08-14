# Security Audit Command

Performs comprehensive security analysis of the HPM codebase.

## Usage
```
/security-audit [--full] [--deps-only] [--code-only]
```

## Options
- `--full`: Complete security audit (default)
- `--deps-only`: Dependency vulnerability scan only
- `--code-only`: Static code analysis only

## Description
Executes comprehensive security analysis including dependency vulnerability scanning, static code analysis, and security pattern review.

## Process
1. Run cargo audit for dependency vulnerabilities
2. Execute cargo geiger for unsafe code analysis
3. Scan for common security anti-patterns
4. Review cryptographic implementations
5. Analyze input validation patterns
6. Check for information disclosure risks

## Output
- Vulnerability report with severity ratings
- List of unsafe code blocks requiring review
- Security recommendations and remediation steps
- Compliance status for security standards