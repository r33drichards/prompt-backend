# TypeScript SDK Configuration

This directory contains the configuration for the auto-generated TypeScript SDK.

## package.json

The `package.json` file in this directory is the **source of truth** for SDK versioning and package metadata. It is used by:

- The Nix build script (`nix run .#generateTypescriptClient`)
- GitHub Actions workflow for automated publishing

### Versioning Strategy

- **Main branch**: Uses the exact version from `sdk/package.json`
  - Example: `0.1.0` → publishes as `@wholelottahoopla/prompt-backend-client@0.1.0`

- **Pull requests**: Appends beta suffix with PR number and commit SHA
  - Example: `0.1.0` → publishes as `@wholelottahoopla/prompt-backend-client@0.1.0-beta.pr8.abc1234`

- **Manual workflow dispatch**: Can override with custom version

### Updating the Version

To release a new version of the SDK:

1. Update the `version` field in `sdk/package.json`:
   ```bash
   cd sdk
   npm version patch  # or minor, or major
   ```

2. Commit the change:
   ```bash
   git add sdk/package.json
   git commit -m "Bump SDK version to 0.2.0"
   ```

3. Push to main branch:
   ```bash
   git push origin main
   ```

4. GitHub Actions will automatically publish the new version to npm

### Package Metadata

All metadata in `sdk/package.json` is merged into the generated SDK:
- `name`: npm package name
- `version`: package version
- `description`: package description
- `author`: package author
- `license`: package license
- `repository`: git repository info
- `bugs`: issue tracker URL
- `homepage`: package homepage

When you update any of these fields, the changes will be reflected in the next SDK build.
