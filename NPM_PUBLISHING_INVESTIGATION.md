# NPM Package Publishing Issue - Investigation Report

## Issue Summary

The GitHub Action failed to publish `@wholelottahoopla/prompt-backend-client` version `0.1.9` with the following error:

```
npm error 404 Not Found - PUT https://registry.npmjs.org/@wholelottahoopla%2fprompt-backend-client - Not found
npm error 404  '@wholelottahoopla/prompt-backend-client@0.1.9' is not in this registry.
```

## Root Cause

**The version 0.1.9 ALREADY EXISTS in the npm registry.** The error message is misleading - npm returns a 404 "Not found" error when attempting to publish a version that already exists, rather than a more descriptive error like "version already exists" or "conflict".

## Investigation Findings

### 1. Package Status on NPM

✅ **Package EXISTS:** `@wholelottahoopla/prompt-backend-client` is published and available on npm
- **Latest version:** 0.1.9 (published 2025-11-08)
- **Total versions published:** 45 versions
- **Maintainer:** wholelottahoopla
- **Registry URL:** https://npm.im/@wholelottahoopla/prompt-backend-client

### 2. All Published Versions

The package has 45 versions published, including:
- Beta versions: 0.1.0-beta.pr9 through 0.1.2-beta.pr23
- Stable versions: 0.1.0, 0.1.1, 0.1.2, 0.1.3, 0.1.4, 0.1.5, 0.1.6, 0.1.7, 0.1.9

**Notable observation:** Version 0.1.8 was SKIPPED - the versions jump from 0.1.7 to 0.1.9

### 3. Current Local Configuration

- **Local package.json version:** 0.1.9
- **Location:** `sdk/package.json`
- **Package name:** `@wholelottahoopla/prompt-backend-client`

### 4. Publishing Workflow

The package is published via GitHub Actions workflow: `.github/workflows/publish-sdk.yml`

**Workflow behavior:**
- Triggered on push to `master` branch or manual workflow_dispatch
- Uses version from `sdk/package.json` for master branch pushes
- Allows custom version input for manual dispatch
- Generates TypeScript client using Nix: `nix run .#generateTypescriptClient`
- Publishes to npm with `--access public --provenance` flags

## Solutions

### Option 1: Bump Version Number (Recommended)

Update `sdk/package.json` to the next version:

\`\`\`json
{
  "name": "@wholelottahoopla/prompt-backend-client",
  "version": "0.1.10",  // Changed from 0.1.9
  ...
}
\`\`\`

### Option 2: Use Beta/Pre-release Version

For testing or PR-specific builds, use a beta version:

\`\`\`json
{
  "version": "0.1.10-beta.pr<number>.<commit-hash>"
}
\`\`\`

### Option 3: Manual Workflow Dispatch

Use the workflow_dispatch trigger with a custom version input to publish a specific version without modifying `sdk/package.json`.

## Recommendations

1. **Implement version checking** in the CI/CD pipeline to detect if a version already exists before attempting to publish
2. **Add npm version bump script** to package.json for easier version management
3. **Consider using automated versioning** based on commit messages (semantic-release or similar)
4. **Update error handling** in the workflow to provide clearer feedback when publishing fails

## Additional Context

- The `@wholelottahoopla` scope contains at least 2 packages:
  - `@wholelottahoopla/prompt-backend-client`
  - `@wholelottahoopla/vkanban`
- The npm 404 error is a known confusing behavior when attempting to republish existing versions
- No authentication issues detected - the scope and package are properly configured

---

**Investigation Date:** 2025-11-15
**Investigated by:** Claude Code Agent
**Session ID:** f2ff27be-816d-44cd-9148-0df51e520e6e
