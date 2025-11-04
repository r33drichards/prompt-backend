/// Integration test for sandbox execution using the generated SDK
///
/// Prerequisites:
/// 1. Docker must be installed and running
/// 2. Run: docker run -d --name sandbox -p 8080:8080 wholelottahoopla/sandbox:latest
///
/// To run this test:
/// cargo test --test sandbox_integration_test -- --ignored --nocapture
///
/// To clean up after test:
/// docker stop sandbox && docker rm sandbox

use sandbox_client::Client;

#[tokio::test]
#[ignore] // Requires Docker container to be running
async fn test_sandbox_exec_hello_world() {
    // Create SDK client pointing to the sandbox container
    // Container should be running on localhost:8080
    let client = Client::new("http://localhost:8080");

    // Execute "echo 'Hello World'" command
    let request = sandbox_client::types::ShellExecRequest {
        command: "echo 'Hello World'".to_string(),
        async_mode: false,
        exec_dir: None,
        id: None,
        timeout: Some(5.0),
    };

    let response = client
        .exec_command_v1_shell_exec_post(&request)
        .await
        .expect("Failed to execute command - is the sandbox container running on port 8080?");

    // Get the response data
    let result = response.into_inner();

    // Assert the command succeeded
    assert!(
        result.success,
        "Command should succeed. Message: {}",
        result.message
    );

    let data = result
        .data
        .expect("Should have command result data");

    assert_eq!(
        data.exit_code,
        Some(0),
        "Exit code should be 0, got: {:?}",
        data.exit_code
    );

    let output = data.output.expect("Should have output");
    assert!(
        output.contains("Hello World"),
        "Output should contain 'Hello World', got: {}",
        output
    );

    println!("✓ Successfully executed 'echo Hello World' in sandbox");
    println!("  Exit code: {:?}", data.exit_code);
    println!("  Output: {}", output.trim());
}
