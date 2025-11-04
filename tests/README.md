# Integration Tests

## Sandbox Integration Test

The `sandbox_integration_test.rs` file contains an integration test that verifies the generated SDK can communicate with the sandbox Docker container.

### Test Description

The test performs the following:
1. Connects to a running sandbox container at `localhost:8080`
2. Uses the generated SDK (`sandbox-client`) to execute the command: `echo 'Hello World'`
3. Asserts that:
   - The command execution succeeds
   - The exit code is 0
   - The output contains "Hello World"

### Running the Test

#### Option 1: Automated Script (Recommended)

```bash
./scripts/test_sandbox_integration.sh
```

This script will:
- Start the sandbox Docker container
- Wait for it to be ready
- Run the integration test
- Clean up the container

#### Option 2: Manual Execution

1. Start the sandbox container:
```bash
docker run -d --name sandbox -p 8080:8080 wholelottahoopla/sandbox:latest
```

2. Run the test:
```bash
cargo test --test sandbox_integration_test -- --ignored --nocapture
```

3. Clean up:
```bash
docker stop sandbox && docker rm sandbox
```

### Test Structure

The test follows TDD principles:

- **RED**: The test will fail if the sandbox doesn't respond correctly or if the command execution fails
- **GREEN**: The test passes when the sandbox successfully executes the command and returns "Hello World"
- **Assertions**:
  - `result.success` must be true
  - `exit_code` must be 0
  - `output` must contain "Hello World"

### Requirements

- Docker must be installed and running
- The `wholelottahoopla/sandbox:latest` image must be available
- Port 8080 must be available

### Example Output

```
✓ Successfully executed 'echo Hello World' in sandbox
  Exit code: Some(0)
  Output: Hello World
```

## CI/CD Integration

The sandbox integration tests are automatically run in GitHub Actions on every push and pull request.

### GitHub Actions Workflow

The tests are run as part of the `integration-tests.yml` workflow in the `sandbox-integration-tests` job:

- **Trigger**: On push and pull requests to any branch
- **Environment**: Ubuntu latest with Nix installed
- **Steps**:
  1. Checkout code
  2. Install Nix with flakes support
  3. Pull the sandbox Docker image
  4. Run `scripts/test_sandbox_integration.sh`
  5. Show logs on failure
  6. Clean up containers

### Viewing Test Results

You can view the test results in the GitHub Actions tab of the repository. Each workflow run will show:
- Unit tests status
- CRUD integration tests status
- Sandbox integration tests status

### Local Testing with Nix

If you have Nix installed with flakes enabled, the test script will automatically use `nix develop` to run the tests:

```bash
./scripts/test_sandbox_integration.sh
```

This ensures the same Rust toolchain and dependencies are used locally as in CI.
