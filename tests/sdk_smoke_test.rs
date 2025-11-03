use sandbox_client::Client;

#[test]
fn test_sdk_client_instantiation() {
    // Test that we can create a client instance
    // This verifies the SDK is properly linked
    let _client = Client::new("http://localhost:8000");
}

#[test]
fn test_sdk_types_accessible() {
    // This test just needs to compile to verify types are available
    let _type_name = std::any::type_name::<Client>();
}
