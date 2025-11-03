// tests/sdk_smoke_test.rs

/// Smoke test to verify SDK can be imported and Client can be instantiated
#[test]
fn test_sdk_client_instantiation() {
    use sandbox_client::Client;

    // Just verify we can create a client - don't actually make requests
    let _client = Client::new("http://localhost:8000");

    // If we got here, SDK is properly linked
    assert!(true, "SDK client successfully instantiated");
}

/// Verify SDK types are accessible
#[test]
fn test_sdk_types_accessible() {
    // This test just needs to compile to verify types are available
    // We're not testing the API behavior, just that SDK exports work

    let _ = std::any::type_name::<sandbox_client::Client>();

    assert!(true, "SDK types are accessible");
}
