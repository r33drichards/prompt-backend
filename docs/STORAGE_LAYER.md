# Storage Layer Architecture

## Overview

The storage layer provides a pluggable abstraction for file storage operations, allowing the application to seamlessly swap between different storage backends (local filesystem, Amazon S3, etc.) without changing application code.

## Architecture

```text
┌─────────────────────────────────────────┐
│         Application Code                │
│    (handlers, background tasks)         │
└──────────────┬──────────────────────────┘
               │
               │ Uses StorageBackend trait
               ▼
┌──────────────────────────────────────────┐
│       StorageBackend Trait               │
│  (put, get, exists, delete, list, etc.)  │
└──────────────┬───────────────────────────┘
               │
    ┌──────────┴──────────┬────────────┐
    ▼                     ▼            ▼
┌─────────┐          ┌─────────┐  ┌─────────┐
│  Local  │          │   S3    │  │  Future │
│ Storage │          │ Storage │  │ Backends│
└─────────┘          └─────────┘  └─────────┘
```

## Key Benefits

1. **Flexibility**: Switch storage backends via configuration
2. **Testability**: Easy to mock for unit tests
3. **Scalability**: Start with local storage, migrate to S3 when needed
4. **Consistency**: Unified interface regardless of backend
5. **Future-proof**: Add new backends without changing application code

## Core Components

### 1. StorageBackend Trait

The main abstraction that all storage backends implement:

```rust
#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn put(&self, path: &str, data: Vec<u8>, options: Option<PutOptions>) -> StorageResult<FileMetadata>;
    async fn get(&self, path: &str, options: Option<GetOptions>) -> StorageResult<Vec<u8>>;
    async fn exists(&self, path: &str) -> StorageResult<bool>;
    async fn delete(&self, path: &str) -> StorageResult<()>;
    async fn list(&self, prefix: &str) -> StorageResult<Vec<String>>;
    async fn metadata(&self, path: &str) -> StorageResult<FileMetadata>;
    async fn get_url(&self, path: &str, expires_in_secs: Option<u64>) -> StorageResult<String>;
    fn backend_name(&self) -> &str;
}
```

### 2. Implementations

#### LocalStorage
- Stores files on the local filesystem
- Perfect for development and small deployments
- Built-in path traversal protection
- No external dependencies

#### S3Storage
- Stores files in Amazon S3 or S3-compatible services
- Supports MinIO, DigitalOcean Spaces, etc.
- Presigned URL generation
- Automatic retry and error handling

### 3. StorageFactory

Convenient factory for creating storage backends:

```rust
// From environment variables
let storage = StorageFactory::from_env()?;

// From explicit configuration
let config = StorageConfig::Local { base_path: "/tmp/storage".to_string() };
let storage = StorageFactory::create(config)?;
```

## Usage Examples

### Basic Usage

```rust
use prompt_backend::storage::{StorageFactory, StorageBackend};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create storage from environment
    let storage = StorageFactory::from_env()?;
    
    // Store a file
    let data = b"Hello, World!".to_vec();
    storage.put("hello.txt", data, None).await?;
    
    // Retrieve it
    let retrieved = storage.get("hello.txt", None).await?;
    println!("Retrieved: {}", String::from_utf8(retrieved)?);
    
    // Check if exists
    if storage.exists("hello.txt").await? {
        println!("File exists!");
    }
    
    // Get metadata
    let meta = storage.metadata("hello.txt").await?;
    println!("Size: {} bytes", meta.size);
    
    // List files
    let files = storage.list("").await?;
    println!("Files: {:?}", files);
    
    // Delete
    storage.delete("hello.txt").await?;
    
    Ok(())
}
```

### Storing Images (Image Input MVP)

```rust
use prompt_backend::storage::{StorageFactory, PutOptions};

async fn store_prompt_image(
    storage: &dyn StorageBackend,
    prompt_id: &str,
    image_data: Vec<u8>,
    media_type: &str,
) -> Result<String, StorageError> {
    // Generate unique path
    let image_id = uuid::Uuid::new_v4();
    let extension = match media_type {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "bin",
    };
    let path = format!("prompts/{}/images/{}.{}", prompt_id, image_id, extension);
    
    // Store with metadata
    let options = PutOptions {
        content_type: Some(media_type.to_string()),
        overwrite: false,
        metadata: None,
    };
    
    storage.put(&path, image_data, Some(options)).await?;
    
    Ok(path)
}

async fn retrieve_image(
    storage: &dyn StorageBackend,
    path: &str,
) -> Result<Vec<u8>, StorageError> {
    storage.get(path, None).await
}
```

### Background Job Integration

```rust
use prompt_backend::storage::{StorageFactory, StorageBackend};
use std::sync::Arc;

pub struct OutboxContext {
    pub db: DatabaseConnection,
    pub storage: Arc<dyn StorageBackend>,
}

async fn process_job(job: OutboxJob, ctx: Data<OutboxContext>) -> Result<(), Error> {
    // Extract images from prompt data
    let images = extract_images(&prompt_model.data)?;
    
    for (idx, image) in images.iter().enumerate() {
        let path = format!("session_{}/prompt_{}/image_{}.png", 
                          session_id, prompt_id, idx);
        
        // Decode base64 and store
        let image_data = base64::decode(&image.data)?;
        ctx.storage.put(&path, image_data, None).await?;
        
        info!("Stored image: {}", path);
    }
    
    Ok(())
}
```

### Testing with Mock Storage

```rust
use prompt_backend::storage::{LocalStorage, StorageBackend};
use tempfile::TempDir;

#[tokio::test]
async fn test_image_storage() {
    // Use temporary directory for tests
    let temp_dir = TempDir::new().unwrap();
    let storage = LocalStorage::new(temp_dir.path()).unwrap();
    
    // Test storage operations
    let image = vec![1, 2, 3, 4];
    storage.put("test.png", image.clone(), None).await.unwrap();
    
    let retrieved = storage.get("test.png", None).await.unwrap();
    assert_eq!(retrieved, image);
}
```

## Configuration

### Environment Variables

#### Local Storage (Default)
```bash
STORAGE_TYPE=local
STORAGE_BASE_PATH=/var/app/storage
```

#### Amazon S3
```bash
STORAGE_TYPE=s3
S3_BUCKET=my-bucket
S3_REGION=us-east-1
# Optional: explicit credentials
S3_ACCESS_KEY=AKIAIOSFODNN7EXAMPLE
S3_SECRET_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY
```

#### S3-Compatible Services (MinIO, DigitalOcean Spaces)
```bash
STORAGE_TYPE=s3
S3_BUCKET=my-spaces-bucket
S3_REGION=nyc3
S3_ENDPOINT=https://nyc3.digitaloceanspaces.com
S3_ACCESS_KEY=your-access-key
S3_SECRET_KEY=your-secret-key
```

### Programmatic Configuration

```rust
use prompt_backend::storage::{StorageFactory, StorageConfig};

// Local storage
let local_config = StorageConfig::Local {
    base_path: "/var/app/storage".to_string(),
};
let local_storage = StorageFactory::create(local_config)?;

// S3 storage
let s3_config = StorageConfig::S3 {
    bucket: "my-bucket".to_string(),
    region: "us-east-1".to_string(),
    access_key: None,  // Uses AWS credentials chain
    secret_key: None,
    endpoint: None,
};
let s3_storage = StorageFactory::create(s3_config)?;
```

## Migration Path

### Phase 1: Start with Local Storage
```bash
# Development
STORAGE_TYPE=local
STORAGE_BASE_PATH=/tmp/storage
```

### Phase 2: Move to S3 in Production
```bash
# Production
STORAGE_TYPE=s3
S3_BUCKET=prod-prompt-images
S3_REGION=us-east-1
```

### Phase 3: Data Migration Script
```rust
use prompt_backend::storage::{LocalStorage, S3Storage, StorageBackend};

async fn migrate_to_s3() -> Result<(), Box<dyn std::error::Error>> {
    let local = LocalStorage::new("/var/app/storage")?;
    let s3 = S3Storage::new(
        "prod-bucket".to_string(),
        "us-east-1".to_string(),
        None, None, None
    )?;
    
    // List all files in local storage
    let files = local.list("").await?;
    
    for file in files {
        println!("Migrating: {}", file);
        
        // Get from local
        let data = local.get(&file, None).await?;
        
        // Put to S3
        s3.put(&file, data, None).await?;
        
        println!("✓ Migrated: {}", file);
    }
    
    Ok(())
}
```

## Advanced Features

### Range Requests
```rust
use prompt_backend::storage::GetOptions;

// Get bytes 0-1023
let options = GetOptions {
    range: Some((0, 1023)),
};
let chunk = storage.get("large-file.bin", Some(options)).await?;
```

### Presigned URLs (S3 only)
```rust
// Generate a URL valid for 1 hour
let url = storage.get_url("image.jpg", Some(3600)).await?;
println!("Download from: {}", url);
```

### Custom Metadata
```rust
use prompt_backend::storage::PutOptions;
use std::collections::HashMap;

let mut metadata = HashMap::new();
metadata.insert("user_id".to_string(), "123".to_string());
metadata.insert("prompt_id".to_string(), "456".to_string());

let options = PutOptions {
    content_type: Some("image/png".to_string()),
    overwrite: true,
    metadata: Some(metadata),
};

storage.put("image.png", data, Some(options)).await?;
```

## Error Handling

```rust
use prompt_backend::storage::StorageError;

match storage.get("file.txt", None).await {
    Ok(data) => println!("Got data: {} bytes", data.len()),
    Err(StorageError::NotFound(msg)) => {
        println!("File not found: {}", msg);
    }
    Err(StorageError::PermissionDenied(msg)) => {
        println!("Access denied: {}", msg);
    }
    Err(StorageError::Network(msg)) => {
        println!("Network error: {}", msg);
    }
    Err(e) => {
        println!("Other error: {}", e);
    }
}
```

## Performance Considerations

### Local Storage
- **Pros**: Fast, no network latency
- **Cons**: Limited by disk I/O, not scalable across servers
- **Best for**: Development, single-server deployments

### S3 Storage
- **Pros**: Scalable, durable, globally distributed
- **Cons**: Network latency, API rate limits, costs
- **Best for**: Production, multi-server deployments

### Optimization Tips

1. **Batch Operations**: Use `list()` to get multiple files, then process in parallel
2. **Caching**: Cache frequently accessed files in memory or Redis
3. **Compression**: Compress large files before storing
4. **CDN**: Use CloudFront or similar CDN in front of S3

## Security

### Local Storage
- Path traversal protection built-in
- Relies on filesystem permissions
- No public URL generation

### S3 Storage
- IAM policies for access control
- Presigned URLs with expiration
- Encryption at rest (server-side)
- Bucket policies for fine-grained control

### Best Practices
1. Never expose storage backend directly to users
2. Validate all file paths before storing
3. Implement size limits to prevent abuse
4. Use presigned URLs for secure temporary access
5. Regularly audit stored files and access patterns

## Monitoring

### Metrics to Track
- Number of storage operations (put/get/delete)
- Storage operation latency
- Storage size and growth rate
- Error rates by operation type
- S3 API costs (if using S3)

### Example Prometheus Metrics
```rust
use prometheus::{Counter, Histogram};

lazy_static! {
    static ref STORAGE_OPS: Counter = Counter::new(
        "storage_operations_total",
        "Total storage operations"
    ).unwrap();
    
    static ref STORAGE_DURATION: Histogram = Histogram::new(
        "storage_operation_duration_seconds",
        "Storage operation duration"
    ).unwrap();
}

// In your code
async fn put_with_metrics(storage: &dyn StorageBackend, path: &str, data: Vec<u8>) {
    let start = std::time::Instant::now();
    let result = storage.put(path, data, None).await;
    STORAGE_DURATION.observe(start.elapsed().as_secs_f64());
    STORAGE_OPS.inc();
    result
}
```

## Troubleshooting

### Common Issues

#### "Storage directory not found"
```bash
# Ensure directory exists
mkdir -p /var/app/storage
chmod 755 /var/app/storage
```

#### "S3 permission denied"
```bash
# Check IAM policy includes:
# - s3:PutObject
# - s3:GetObject
# - s3:DeleteObject
# - s3:ListBucket
```

#### "Network timeout"
```rust
// Increase timeout in AWS SDK configuration
let config = aws_config::from_env()
    .timeout_config(TimeoutConfig::builder()
        .operation_timeout(Duration::from_secs(30))
        .build())
    .load()
    .await;
```

## Future Enhancements

Potential future backends:
- **Azure Blob Storage**
- **Google Cloud Storage**
- **PostgreSQL (BYTEA columns)**
- **Object storage with automatic tiering**
- **Distributed filesystems (Ceph, GlusterFS)**

The trait-based design makes adding new backends straightforward!
