# Sandbox Integration Tests

This document describes the integration tests for the sandbox Docker container using the generated Rust SDK.

## Overview

The integration tests verify that the generated `sandbox-client` SDK can successfully communicate with the `wholelottahoopla/sandbox` Docker container and execute commands.

## Files Created

### 1. Integration Test (`tests/sandbox_integration_test.rs`)

**Purpose**: Tests the SDK's ability to execute commands in the sandbox.

**What it tests**:
- Connects to sandbox at `localhost:8080`
- Executes `echo 'Hello World'` command via SDK
- Asserts:
  - Command execution succeeds
  - Exit code is 0
  - Output contains "Hello World"

**Features**:
- Marked with `#[ignore]` to only run when explicitly requested
- Uses async/await with Tokio
- Clear error messages for debugging

### 2. Test Script (`scripts/test_sandbox_integration.sh`)

**Purpose**: Automated script to run the integration tests with proper setup/teardown.

**What it does**:
1. Verifies Docker is running
2. Starts the sandbox container on port 8080
3. Waits for container to be ready (up to 30 seconds)
4. Runs the integration test using Nix (if available) or system cargo
5. Cleans up the container

**Nix Integration**:
- Detects if Nix is available
- Uses `nix develop --command cargo test` to ensure consistent environment
- Falls back to system cargo if Nix is not available

### 3. GitHub Actions Workflow (`.github/workflows/integration-tests.yml`)

**New job added**: `sandbox-integration-tests`

**Purpose**: Runs integration tests automatically on CI/CD.

**Steps**:
1. Checkout code
2. Install Nix with flakes support
3. Setup Nix cache (optional, requires `CACHIX_AUTH_TOKEN`)
4. Pull sandbox Docker image
5. Run `test_sandbox_integration.sh` script
6. Show logs on failure
7. Clean up containers

**Triggers**: Runs on every push and pull request to any branch.

### 4. Documentation (`tests/README.md`)

**Purpose**: Complete guide for running and understanding the tests.

**Includes**:
- Test description
- Running instructions (automated and manual)
- CI/CD integration details
- Requirements
- Example output

## Usage

### Local Testing

#### Quick Run (Automated)

```bash
./scripts/test_sandbox_integration.sh
```

This will:
- Start the sandbox container
- Run the tests
- Clean up automatically

#### Manual Testing

```bash
# 1. Start sandbox
docker run -d --name sandbox -p 8080:8080 wholelottahoopla/sandbox:latest

# 2. Run tests
nix develop --command cargo test --test sandbox_integration_test -- --ignored --nocapture

# Or without Nix:
cargo test --test sandbox_integration_test -- --ignored --nocapture

# 3. Cleanup
docker stop sandbox && docker rm sandbox
```

### CI/CD

The tests run automatically in GitHub Actions:

1. **Unit Tests** - Runs all unit tests including snapshot tests
2. **CRUD Integration Tests** - Tests the main application with Docker Compose
3. **Sandbox Integration Tests** - Tests the sandbox SDK (NEW)

View results in the GitHub Actions tab of the repository.

## Architecture

```
┌─────────────────────────────────────────────────────┐
│ GitHub Actions Workflow                             │
│ (.github/workflows/integration-tests.yml)           │
└─────────────────┬───────────────────────────────────┘
                  │
                  ├── Unit Tests (cargo test)
                  │
                  ├── CRUD Integration Tests (Docker Compose)
                  │
                  └── Sandbox Integration Tests
                      │
                      ├── Install Nix
                      │
                      └── Run test_sandbox_integration.sh
                          │
                          ├── Start sandbox container
                          │
                          ├── Wait for readiness
                          │
                          └── nix develop --command cargo test
                              │
                              └── tests/sandbox_integration_test.rs
                                  │
                                  └── Use sandbox-client SDK
                                      │
                                      └── Execute commands in sandbox
```

## TDD Approach

This implementation follows Test-Driven Development:

1. **RED**: Test written to fail if sandbox doesn't work
   - Test expects successful command execution
   - Asserts on exit code and output

2. **GREEN**: Test passes when sandbox executes correctly
   - Sandbox container must be running
   - Command execution must succeed
   - Output must contain expected text

3. **REFACTOR**: Documented and automated
   - Helper script for easy execution
   - CI/CD integration
   - Clear documentation

## Requirements

- **Docker**: Must be installed and running
- **Nix** (optional): Recommended for consistent environment
- **Port 8080**: Must be available for sandbox container
- **Image**: `wholelottahoopla/sandbox:latest` must be accessible

## Troubleshooting

### Test fails with "connection refused"

**Cause**: Sandbox container not running or not ready.

**Solution**:
```bash
# Check container status
docker ps | grep sandbox

# Check container logs
docker logs sandbox
```

### Test fails with compilation errors

**Cause**: Rust environment mismatch.

**Solution**: Use Nix to ensure consistent environment:
```bash
nix develop --command cargo test --test sandbox_integration_test -- --ignored --nocapture
```

### Container fails to start

**Cause**: Port 8080 already in use.

**Solution**:
```bash
# Find process using port 8080
lsof -i :8080

# Stop conflicting container
docker stop $(docker ps -q --filter "publish=8080")
```

## Future Enhancements

Potential improvements:

1. **Multiple Commands**: Test various command types (bash, python, etc.)
2. **Error Handling**: Test failure scenarios and error responses
3. **Concurrent Execution**: Test multiple concurrent commands
4. **Timeout Handling**: Test command timeouts
5. **State Management**: Test session persistence across commands
6. **Performance**: Measure execution time and optimize

## Related Files

- `agent-sandbox-sdk/` - Generated SDK source
- `agent-sandbox-sdk/openapi.json` - OpenAPI spec
- `agent-sandbox-sdk/build.rs` - SDK build script
- `.github/workflows/generate-rust-sdk.yml` - SDK update workflow
