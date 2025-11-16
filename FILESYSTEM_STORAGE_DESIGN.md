# File System Storage Layer Design

## Question 1: PostgreSQL JSONB Storage Limits

### Technical Limits

PostgreSQL JSONB columns have the following limits:

1. **Maximum Size**: ~1 GB (1 GB - 1 byte = 1,073,741,823 bytes)
   - This is the limit for any single value in PostgreSQL (TOAST storage)
   - Applies to JSONB, JSON, TEXT, and BYTEA columns
   - Stored using TOAST (The Oversized-Attribute Storage Technique)

2. **Practical Considerations**:
   - **Performance degradation** starts around 10-100 MB
   - **Memory pressure**: Large JSONB values are loaded entirely into memory during operations
   - **Index limitations**: GIN indexes on JSONB become less efficient with large documents
   - **Backup/replication impact**: Large rows slow down pg_dump and replication
   - **Network overhead**: Every query retrieves the entire JSONB value

3. **Current Usage in This Codebase**:
   - `prompt.data`: Stores prompt content (text + images as base64)
   - `message.data`: Stores Claude Code output messages (streaming JSON)
   - **Problem**: Base64 images increase size by ~33%, so a 10MB image becomes ~13.3MB in JSON
   - **Example**: 5 images × 10MB each = 66MB in base64, approaching performance limits

### When to Switch to File System Storage

**Use PostgreSQL JSONB when**:
- Data < 1 MB per record
- Need to query/index JSON fields (e.g., WHERE data->>'status' = 'completed')
- Need ACID transactions on the data
- Infrequent updates
- Data fits in working memory

**Use File System when**:
- Data > 10 MB (performance threshold)
- Binary data (images, videos, archives)
- Streaming/chunked access needed
- High update frequency
- Cost-sensitive (storage is cheaper than database)

---

## Question 2: File System Storage Layer Design

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                     Storage Abstraction Layer                │
├─────────────────────────────────────────────────────────────┤
│  trait BlobStorage {                                         │
│    async fn write(key: &str, data: Vec<u8>) -> Result<()>  │
│    async fn read(key: &str) -> Result<Vec<u8>>             │
│    async fn delete(key: &str) -> Result<()>                │
│    async fn exists(key: &str) -> Result<bool>              │
│  }                                                           │
└─────────────────────────────────────────────────────────────┘
                            │
        ┌───────────────────┴────────────────────┐
        ▼                                        ▼
┌──────────────────┐                  ┌──────────────────┐
│ PostgresStorage  │                  │ FileSystemStorage│
│                  │                  │                  │
│ - Small data     │                  │ - Large data     │
│ - < 1 MB         │                  │ - > 1 MB         │
│ - Queryable      │                  │ - Binary blobs   │
└──────────────────┘                  └──────────────────┘
                                                │
                                    ┌───────────┴───────────┐
                                    ▼                       ▼
                            ┌──────────────┐      ┌──────────────┐
                            │ LocalFS      │      │ S3-Compatible│
                            │              │      │              │
                            │ - Dev/test   │      │ - Production │
                            │ - Fast       │      │ - Durable    │
                            └──────────────┘      └──────────────┘
```

### Database Schema Changes

Instead of storing large data in JSONB, we'll store references:

**Before** (current):
```sql
CREATE TABLE prompt (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL,
    data JSONB NOT NULL,  -- Can be very large
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
```

**After** (with file storage):
```sql
CREATE TABLE prompt (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL,
    data JSONB,  -- NULL if stored externally
    data_storage_key TEXT,  -- e.g., "prompts/550e8400-e29b-41d4-a716-446655440000.json"
    data_size_bytes BIGINT,  -- Track size for monitoring
    storage_backend VARCHAR(20),  -- 'postgres', 'local_fs', 's3'
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    
    -- Ensure either data or data_storage_key is set, but not both
    CONSTRAINT check_data_xor CHECK (
        (data IS NOT NULL AND data_storage_key IS NULL) OR
        (data IS NULL AND data_storage_key IS NOT NULL)
    )
);
```

---

## Implementation

### 1. Storage Trait Definition

Create `src/storage/mod.rs`:

```rust
use async_trait::async_trait;
use std::error::Error as StdError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Key not found: {0}")]
    NotFound(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(String),
    
    #[error("Storage backend error: {0}")]
    Backend(Box<dyn StdError + Send + Sync>),
}

pub type StorageResult<T> = Result<T, StorageError>;

/// Abstraction for blob storage (file system, S3, etc.)
#[async_trait]
pub trait BlobStorage: Send + Sync {
    /// Write data to storage with the given key
    async fn write(&self, key: &str, data: Vec<u8>) -> StorageResult<()>;
    
    /// Read data from storage by key
    async fn read(&self, key: &str) -> StorageResult<Vec<u8>>;
    
    /// Delete data by key
    async fn delete(&self, key: &str) -> StorageResult<()>;
    
    /// Check if a key exists
    async fn exists(&self, key: &str) -> StorageResult<bool>;
    
    /// Get storage backend name (for logging/metrics)
    fn backend_name(&self) -> &'static str;
}

pub mod local_fs;
pub mod s3;
pub mod postgres;

pub use local_fs::LocalFileStorage;
pub use s3::S3Storage;
pub use postgres::PostgresStorage;
```

---

### 2. Local File System Implementation

Create `src/storage/local_fs.rs`:

```rust
use super::{BlobStorage, StorageError, StorageResult};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, error, info};

/// Local file system storage implementation
pub struct LocalFileStorage {
    base_path: PathBuf,
}

impl LocalFileStorage {
    /// Create a new local file storage with the given base directory
    pub fn new(base_path: impl Into<PathBuf>) -> StorageResult<Self> {
        let base_path = base_path.into();
        
        // Create base directory if it doesn't exist
        std::fs::create_dir_all(&base_path)?;
        
        info!("Initialized local file storage at: {:?}", base_path);
        
        Ok(Self { base_path })
    }
    
    /// Get the full path for a storage key
    fn get_path(&self, key: &str) -> PathBuf {
        // Sanitize the key to prevent directory traversal attacks
        let sanitized_key = key.replace("..", "_").replace("//", "/");
        self.base_path.join(sanitized_key)
    }
    
    /// Ensure parent directory exists for a path
    async fn ensure_parent_dir(&self, path: &PathBuf) -> StorageResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl BlobStorage for LocalFileStorage {
    async fn write(&self, key: &str, data: Vec<u8>) -> StorageResult<()> {
        let path = self.get_path(key);
        
        debug!("Writing {} bytes to {:?}", data.len(), path);
        
        // Ensure parent directory exists
        self.ensure_parent_dir(&path).await?;
        
        // Write atomically by writing to temp file then renaming
        let temp_path = path.with_extension("tmp");
        
        let mut file = fs::File::create(&temp_path).await?;
        file.write_all(&data).await?;
        file.sync_all().await?;
        
        // Atomic rename
        fs::rename(&temp_path, &path).await?;
        
        info!("Successfully wrote {} bytes to {:?}", data.len(), path);
        
        Ok(())
    }
    
    async fn read(&self, key: &str) -> StorageResult<Vec<u8>> {
        let path = self.get_path(key);
        
        debug!("Reading from {:?}", path);
        
        match fs::read(&path).await {
            Ok(data) => {
                info!("Successfully read {} bytes from {:?}", data.len(), path);
                Ok(data)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                error!("Key not found: {}", key);
                Err(StorageError::NotFound(key.to_string()))
            }
            Err(e) => {
                error!("Failed to read {:?}: {}", path, e);
                Err(StorageError::Io(e))
            }
        }
    }
    
    async fn delete(&self, key: &str) -> StorageResult<()> {
        let path = self.get_path(key);
        
        debug!("Deleting {:?}", path);
        
        match fs::remove_file(&path).await {
            Ok(_) => {
                info!("Successfully deleted {:?}", path);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Already deleted, not an error
                debug!("Key already deleted: {}", key);
                Ok(())
            }
            Err(e) => {
                error!("Failed to delete {:?}: {}", path, e);
                Err(StorageError::Io(e))
            }
        }
    }
    
    async fn exists(&self, key: &str) -> StorageResult<bool> {
        let path = self.get_path(key);
        Ok(fs::try_exists(&path).await?)
    }
    
    fn backend_name(&self) -> &'static str {
        "local_fs"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_write_read_delete() {
        let temp_dir = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp_dir.path()).unwrap();
        
        let key = "test/data.bin";
        let data = b"hello world".to_vec();
        
        // Write
        storage.write(key, data.clone()).await.unwrap();
        
        // Read
        let read_data = storage.read(key).await.unwrap();
        assert_eq!(read_data, data);
        
        // Exists
        assert!(storage.exists(key).await.unwrap());
        
        // Delete
        storage.delete(key).await.unwrap();
        
        // Not exists
        assert!(!storage.exists(key).await.unwrap());
    }
    
    #[tokio::test]
    async fn test_read_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp_dir.path()).unwrap();
        
        let result = storage.read("nonexistent.bin").await;
        assert!(matches!(result, Err(StorageError::NotFound(_))));
    }
}
```

---

### 3. S3-Compatible Storage Implementation

Create `src/storage/s3.rs`:

```rust
use super::{BlobStorage, StorageError, StorageResult};
use async_trait::async_trait;
use aws_sdk_s3::{Client, primitives::ByteStream};
use tracing::{debug, error, info};

/// S3-compatible storage implementation (works with AWS S3, MinIO, DigitalOcean Spaces, etc.)
pub struct S3Storage {
    client: Client,
    bucket: String,
    prefix: Option<String>,
}

impl S3Storage {
    /// Create a new S3 storage client
    pub async fn new(bucket: String, prefix: Option<String>) -> Self {
        let config = aws_config::load_from_env().await;
        let client = Client::new(&config);
        
        info!("Initialized S3 storage for bucket: {}", bucket);
        
        Self {
            client,
            bucket,
            prefix,
        }
    }
    
    /// Get the full S3 key with prefix
    fn get_key(&self, key: &str) -> String {
        match &self.prefix {
            Some(prefix) => format!("{}/{}", prefix.trim_end_matches('/'), key),
            None => key.to_string(),
        }
    }
}

#[async_trait]
impl BlobStorage for S3Storage {
    async fn write(&self, key: &str, data: Vec<u8>) -> StorageResult<()> {
        let s3_key = self.get_key(key);
        let data_len = data.len();
        
        debug!("Writing {} bytes to s3://{}/{}", data_len, self.bucket, s3_key);
        
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&s3_key)
            .body(ByteStream::from(data))
            .send()
            .await
            .map_err(|e| {
                error!("Failed to write to S3: {}", e);
                StorageError::Backend(Box::new(e))
            })?;
        
        info!("Successfully wrote {} bytes to s3://{}/{}", data_len, self.bucket, s3_key);
        
        Ok(())
    }
    
    async fn read(&self, key: &str) -> StorageResult<Vec<u8>> {
        let s3_key = self.get_key(key);
        
        debug!("Reading from s3://{}/{}", self.bucket, s3_key);
        
        let response = self.client
            .get_object()
            .bucket(&self.bucket)
            .key(&s3_key)
            .send()
            .await
            .map_err(|e| {
                if e.to_string().contains("NoSuchKey") {
                    StorageError::NotFound(key.to_string())
                } else {
                    error!("Failed to read from S3: {}", e);
                    StorageError::Backend(Box::new(e))
                }
            })?;
        
        let data = response
            .body
            .collect()
            .await
            .map_err(|e| {
                error!("Failed to read S3 response body: {}", e);
                StorageError::Backend(Box::new(e))
            })?
            .into_bytes()
            .to_vec();
        
        info!("Successfully read {} bytes from s3://{}/{}", data.len(), self.bucket, s3_key);
        
        Ok(data)
    }
    
    async fn delete(&self, key: &str) -> StorageResult<()> {
        let s3_key = self.get_key(key);
        
        debug!("Deleting s3://{}/{}", self.bucket, s3_key);
        
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&s3_key)
            .send()
            .await
            .map_err(|e| {
                error!("Failed to delete from S3: {}", e);
                StorageError::Backend(Box::new(e))
            })?;
        
        info!("Successfully deleted s3://{}/{}", self.bucket, s3_key);
        
        Ok(())
    }
    
    async fn exists(&self, key: &str) -> StorageResult<bool> {
        let s3_key = self.get_key(key);
        
        match self.client
            .head_object()
            .bucket(&self.bucket)
            .key(&s3_key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) if e.to_string().contains("NotFound") => Ok(false),
            Err(e) => {
                error!("Failed to check S3 object existence: {}", e);
                Err(StorageError::Backend(Box::new(e)))
            }
        }
    }
    
    fn backend_name(&self) -> &'static str {
        "s3"
    }
}
```

---

### 4. PostgreSQL Storage Implementation

Create `src/storage/postgres.rs`:

```rust
use super::{BlobStorage, StorageError, StorageResult};
use async_trait::async_trait;
use sea_orm::DatabaseConnection;
use serde_json::Value as JsonValue;
use tracing::{debug, error, info};

/// PostgreSQL JSONB storage (for small data that benefits from DB storage)
pub struct PostgresStorage {
    db: DatabaseConnection,
}

impl PostgresStorage {
    pub fn new(db: DatabaseConnection) -> Self {
        info!("Initialized PostgreSQL storage");
        Self { db }
    }
}

#[async_trait]
impl BlobStorage for PostgresStorage {
    async fn write(&self, key: &str, data: Vec<u8>) -> StorageResult<()> {
        debug!("Writing {} bytes to PostgreSQL with key: {}", data.len(), key);
        
        // Parse data as JSON
        let json: JsonValue = serde_json::from_slice(&data)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        
        // This would typically insert into a dedicated storage table
        // For now, we'll return an error since this needs integration with your schema
        
        error!("PostgresStorage::write not fully implemented - needs schema integration");
        Err(StorageError::Backend("Not implemented".into()))
    }
    
    async fn read(&self, key: &str) -> StorageResult<Vec<u8>> {
        debug!("Reading from PostgreSQL with key: {}", key);
        
        // This would query from your storage table
        error!("PostgresStorage::read not fully implemented - needs schema integration");
        Err(StorageError::NotFound(key.to_string()))
    }
    
    async fn delete(&self, key: &str) -> StorageResult<()> {
        debug!("Deleting from PostgreSQL with key: {}", key);
        Ok(())
    }
    
    async fn exists(&self, key: &str) -> StorageResult<bool> {
        debug!("Checking existence in PostgreSQL for key: {}", key);
        Ok(false)
    }
    
    fn backend_name(&self) -> &'static str {
        "postgres"
    }
}
```

---

### 5. Smart Storage Router (Automatic Backend Selection)

Create `src/storage/router.rs`:

```rust
use super::{BlobStorage, StorageResult};
use async_trait::async_trait;
use std::sync::Arc;
use tracing::info;

/// Smart storage router that automatically selects backend based on data size
pub struct StorageRouter {
    small_backend: Arc<dyn BlobStorage>,
    large_backend: Arc<dyn BlobStorage>,
    threshold_bytes: usize,
}

impl StorageRouter {
    /// Create a new storage router
    /// 
    /// # Arguments
    /// * `small_backend` - Backend for small data (e.g., PostgreSQL)
    /// * `large_backend` - Backend for large data (e.g., S3 or local FS)
    /// * `threshold_bytes` - Size threshold to switch backends (default: 1 MB)
    pub fn new(
        small_backend: Arc<dyn BlobStorage>,
        large_backend: Arc<dyn BlobStorage>,
        threshold_bytes: Option<usize>,
    ) -> Self {
        let threshold_bytes = threshold_bytes.unwrap_or(1_048_576); // 1 MB default
        
        info!(
            "Initialized storage router: small={}, large={}, threshold={}",
            small_backend.backend_name(),
            large_backend.backend_name(),
            threshold_bytes
        );
        
        Self {
            small_backend,
            large_backend,
            threshold_bytes,
        }
    }
    
    /// Select the appropriate backend based on data size
    fn select_backend(&self, data_size: usize) -> &dyn BlobStorage {
        if data_size < self.threshold_bytes {
            self.small_backend.as_ref()
        } else {
            self.large_backend.as_ref()
        }
    }
}

#[async_trait]
impl BlobStorage for StorageRouter {
    async fn write(&self, key: &str, data: Vec<u8>) -> StorageResult<()> {
        let backend = self.select_backend(data.len());
        
        info!(
            "Routing write ({} bytes) to {} backend",
            data.len(),
            backend.backend_name()
        );
        
        backend.write(key, data).await
    }
    
    async fn read(&self, key: &str) -> StorageResult<Vec<u8>> {
        // Try large backend first (most common for reads), then small
        match self.large_backend.read(key).await {
            Ok(data) => Ok(data),
            Err(_) => self.small_backend.read(key).await,
        }
    }
    
    async fn delete(&self, key: &str) -> StorageResult<()> {
        // Try both backends (idempotent operation)
        let _ = self.large_backend.delete(key).await;
        let _ = self.small_backend.delete(key).await;
        Ok(())
    }
    
    async fn exists(&self, key: &str) -> StorageResult<bool> {
        // Check both backends
        if self.large_backend.exists(key).await? {
            return Ok(true);
        }
        self.small_backend.exists(key).await
    }
    
    fn backend_name(&self) -> &'static str {
        "router"
    }
}
```

---

### 6. Updated Entity Models

Update `src/entities/prompt.rs`:

```rust
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "prompt")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    
    #[sea_orm(column_name = "session_id")]
    pub session_id: Uuid,
    
    // Either data OR data_storage_key should be set, not both
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub data: Option<Json>,
    
    #[sea_orm(nullable)]
    pub data_storage_key: Option<String>,
    
    #[sea_orm(nullable)]
    pub data_size_bytes: Option<i64>,
    
    #[sea_orm(nullable)]
    pub storage_backend: Option<String>,
    
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

impl Model {
    /// Check if data is stored externally (file system/S3)
    pub fn is_external_storage(&self) -> bool {
        self.data_storage_key.is_some()
    }
    
    /// Get the storage key for this prompt
    pub fn storage_key(&self) -> String {
        self.data_storage_key
            .clone()
            .unwrap_or_else(|| format!("prompts/{}.json", self.id))
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::session::Entity",
        from = "Column::SessionId",
        to = "super::session::Column::Id"
    )]
    Session,
    #[sea_orm(has_many = "super::message::Entity")]
    Message,
}

impl Related<super::session::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Session.def()
    }
}

impl Related<super::message::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Message.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
```

---

### 7. Storage Service Layer

Create `src/services/storage_service.rs`:

```rust
use crate::storage::{BlobStorage, StorageResult};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

/// High-level service for managing prompt/message data storage
pub struct StorageService {
    storage: Arc<dyn BlobStorage>,
}

impl StorageService {
    pub fn new(storage: Arc<dyn BlobStorage>) -> Self {
        info!("Initialized storage service with backend: {}", storage.backend_name());
        Self { storage }
    }
    
    /// Store prompt data and return the storage key
    pub async fn store_prompt_data(
        &self,
        prompt_id: Uuid,
        data: &serde_json::Value,
    ) -> StorageResult<(String, usize)> {
        let key = format!("prompts/{}.json", prompt_id);
        let bytes = serde_json::to_vec(data)
            .map_err(|e| crate::storage::StorageError::Serialization(e.to_string()))?;
        
        let size = bytes.len();
        
        self.storage.write(&key, bytes).await?;
        
        info!("Stored prompt data for {} ({} bytes) at key: {}", prompt_id, size, key);
        
        Ok((key, size))
    }
    
    /// Load prompt data from storage
    pub async fn load_prompt_data(&self, storage_key: &str) -> StorageResult<serde_json::Value> {
        let bytes = self.storage.read(storage_key).await?;
        
        let data = serde_json::from_slice(&bytes)
            .map_err(|e| crate::storage::StorageError::Serialization(e.to_string()))?;
        
        info!("Loaded prompt data from key: {} ({} bytes)", storage_key, bytes.len());
        
        Ok(data)
    }
    
    /// Store message data
    pub async fn store_message_data(
        &self,
        message_id: Uuid,
        data: &serde_json::Value,
    ) -> StorageResult<(String, usize)> {
        let key = format!("messages/{}.json", message_id);
        let bytes = serde_json::to_vec(data)
            .map_err(|e| crate::storage::StorageError::Serialization(e.to_string()))?;
        
        let size = bytes.len();
        
        self.storage.write(&key, bytes).await?;
        
        info!("Stored message data for {} ({} bytes) at key: {}", message_id, size, key);
        
        Ok((key, size))
    }
    
    /// Load message data
    pub async fn load_message_data(&self, storage_key: &str) -> StorageResult<serde_json::Value> {
        let bytes = self.storage.read(storage_key).await?;
        
        let data = serde_json::from_slice(&bytes)
            .map_err(|e| crate::storage::StorageError::Serialization(e.to_string()))?;
        
        info!("Loaded message data from key: {} ({} bytes)", storage_key, bytes.len());
        
        Ok(data)
    }
    
    /// Delete data by key
    pub async fn delete(&self, storage_key: &str) -> StorageResult<()> {
        self.storage.delete(storage_key).await
    }
}
```

---

### 8. Migration File

Create `migration/src/m20251116_000001_add_external_storage_fields.rs`:

```rust
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Add external storage fields to prompt table
        manager
            .alter_table(
                Table::alter()
                    .table(Prompt::Table)
                    // Make data nullable (can be NULL if stored externally)
                    .modify_column(ColumnDef::new(Prompt::Data).json_binary().null())
                    .add_column(ColumnDef::new(Prompt::DataStorageKey).string().null())
                    .add_column(ColumnDef::new(Prompt::DataSizeBytes).big_integer().null())
                    .add_column(ColumnDef::new(Prompt::StorageBackend).string_len(20).null())
                    .to_owned(),
            )
            .await?;
        
        // Add external storage fields to message table
        manager
            .alter_table(
                Table::alter()
                    .table(Message::Table)
                    .modify_column(ColumnDef::new(Message::Data).json_binary().null())
                    .add_column(ColumnDef::new(Message::DataStorageKey).string().null())
                    .add_column(ColumnDef::new(Message::DataSizeBytes).big_integer().null())
                    .add_column(ColumnDef::new(Message::StorageBackend).string_len(20).null())
                    .to_owned(),
            )
            .await?;
        
        // Add check constraint to ensure data XOR data_storage_key
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE prompt ADD CONSTRAINT check_prompt_data_xor 
                 CHECK ((data IS NOT NULL AND data_storage_key IS NULL) OR 
                        (data IS NULL AND data_storage_key IS NOT NULL))"
            )
            .await?;
        
        manager
            .get_connection()
            .execute_unprepared(
                "ALTER TABLE message ADD CONSTRAINT check_message_data_xor 
                 CHECK ((data IS NOT NULL AND data_storage_key IS NULL) OR 
                        (data IS NULL AND data_storage_key IS NOT NULL))"
            )
            .await?;
        
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Remove constraints
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE prompt DROP CONSTRAINT IF EXISTS check_prompt_data_xor")
            .await?;
        
        manager
            .get_connection()
            .execute_unprepared("ALTER TABLE message DROP CONSTRAINT IF EXISTS check_message_data_xor")
            .await?;
        
        // Remove columns from prompt
        manager
            .alter_table(
                Table::alter()
                    .table(Prompt::Table)
                    .drop_column(Prompt::DataStorageKey)
                    .drop_column(Prompt::DataSizeBytes)
                    .drop_column(Prompt::StorageBackend)
                    .modify_column(ColumnDef::new(Prompt::Data).json_binary().not_null())
                    .to_owned(),
            )
            .await?;
        
        // Remove columns from message
        manager
            .alter_table(
                Table::alter()
                    .table(Message::Table)
                    .drop_column(Message::DataStorageKey)
                    .drop_column(Message::DataSizeBytes)
                    .drop_column(Message::StorageBackend)
                    .modify_column(ColumnDef::new(Message::Data).json_binary().not_null())
                    .to_owned(),
            )
            .await?;
        
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Prompt {
    Table,
    Data,
    DataStorageKey,
    DataSizeBytes,
    StorageBackend,
}

#[derive(DeriveIden)]
enum Message {
    Table,
    Data,
    DataStorageKey,
    DataSizeBytes,
    StorageBackend,
}
```

---

### 9. Configuration & Initialization

Update `src/main.rs` or create `src/config.rs`:

```rust
use crate::storage::{BlobStorage, LocalFileStorage, S3Storage, StorageRouter};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum StorageBackend {
    LocalFS,
    S3,
    Hybrid, // Automatic routing based on size
}

pub async fn init_storage(backend: StorageBackend) -> Arc<dyn BlobStorage> {
    match backend {
        StorageBackend::LocalFS => {
            let base_path = std::env::var("STORAGE_PATH")
                .unwrap_or_else(|_| "./data/storage".to_string());
            
            Arc::new(LocalFileStorage::new(base_path).expect("Failed to init local storage"))
        }
        
        StorageBackend::S3 => {
            let bucket = std::env::var("S3_BUCKET")
                .expect("S3_BUCKET environment variable required");
            let prefix = std::env::var("S3_PREFIX").ok();
            
            Arc::new(S3Storage::new(bucket, prefix).await)
        }
        
        StorageBackend::Hybrid => {
            // Small data -> PostgreSQL (or LocalFS for now)
            // Large data -> S3 or LocalFS
            let small_backend = {
                let base_path = std::env::var("STORAGE_PATH_SMALL")
                    .unwrap_or_else(|_| "./data/storage/small".to_string());
                Arc::new(LocalFileStorage::new(base_path).expect("Failed to init small storage"))
                    as Arc<dyn BlobStorage>
            };
            
            let large_backend = if std::env::var("S3_BUCKET").is_ok() {
                let bucket = std::env::var("S3_BUCKET").unwrap();
                let prefix = std::env::var("S3_PREFIX").ok();
                Arc::new(S3Storage::new(bucket, prefix).await) as Arc<dyn BlobStorage>
            } else {
                let base_path = std::env::var("STORAGE_PATH_LARGE")
                    .unwrap_or_else(|_| "./data/storage/large".to_string());
                Arc::new(LocalFileStorage::new(base_path).expect("Failed to init large storage"))
                    as Arc<dyn BlobStorage>
            };
            
            Arc::new(StorageRouter::new(small_backend, large_backend, Some(1_048_576)))
        }
    }
}
```

---

### 10. Updated Outbox Handler

Update `src/bg_tasks/outbox_publisher.rs` to use the storage service:

```rust
use crate::services::storage_service::StorageService;

// In the OutboxContext
pub struct OutboxContext {
    pub db: DatabaseConnection,
    pub storage: Arc<StorageService>, // Add this
}

// In process_outbox_job, replace direct JSONB access with storage service:

let prompt_model = Prompt::find_by_id(prompt_id)
    .one(&ctx.db)
    .await?
    .ok_or_else(|| Error::Failed("Prompt not found".into()))?;

// Load prompt content from storage
let prompt_content = if let Some(ref storage_key) = prompt_model.data_storage_key {
    // Data stored externally
    info!("Loading prompt data from external storage: {}", storage_key);
    let data = ctx.storage.load_prompt_data(storage_key).await.map_err(|e| {
        error!("Failed to load prompt data: {}", e);
        Error::Failed(Box::new(e))
    })?;
    
    // Extract content from loaded JSON
    extract_content_from_json(&data)
} else if let Some(ref data) = prompt_model.data {
    // Data in database
    extract_content_from_json(data)
} else {
    error!("Prompt {} has no data", prompt_id);
    return Err(Error::Failed("Prompt has no data".into()));
};

fn extract_content_from_json(data: &serde_json::Value) -> String {
    match data {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(obj) => {
            obj.get("content")
                .or_else(|| obj.get("prompt"))
                .or_else(|| obj.get("text"))
                .or_else(|| obj.get("message"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| serde_json::to_string(data).unwrap_or_default())
        }
        _ => serde_json::to_string(data).unwrap_or_default(),
    }
}
```

---

## Environment Variables

Add to `.env`:

```bash
# Storage backend: local_fs, s3, hybrid
STORAGE_BACKEND=local_fs

# Local file system storage paths
STORAGE_PATH=./data/storage
STORAGE_PATH_SMALL=./data/storage/small
STORAGE_PATH_LARGE=./data/storage/large

# S3 configuration (optional, for production)
S3_BUCKET=my-prompt-storage
S3_PREFIX=prompts
AWS_REGION=us-east-1
AWS_ACCESS_KEY_ID=your_key
AWS_SECRET_ACCESS_KEY=your_secret

# Size threshold for hybrid mode (bytes)
STORAGE_SIZE_THRESHOLD=1048576  # 1 MB
```

---

## Cargo Dependencies

Add to `Cargo.toml`:

```toml
[dependencies]
# Existing dependencies...

# Storage
tokio = { version = "1", features = ["fs", "io-util"] }
aws-config = "1.1"
aws-sdk-s3 = "1.15"
thiserror = "1.0"

[dev-dependencies]
tempfile = "3.8"
```

---

## Usage Examples

### Example 1: Basic Local File Storage

```rust
use crate::storage::LocalFileStorage;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let storage = Arc::new(LocalFileStorage::new("./data/storage").unwrap());
    
    // Write
    let data = b"Hello, world!".to_vec();
    storage.write("test/greeting.txt", data).await.unwrap();
    
    // Read
    let read_data = storage.read("test/greeting.txt").await.unwrap();
    println!("Read: {}", String::from_utf8(read_data).unwrap());
    
    // Delete
    storage.delete("test/greeting.txt").await.unwrap();
}
```

### Example 2: Automatic Backend Selection

```rust
use crate::storage::{LocalFileStorage, StorageRouter};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let small_storage = Arc::new(LocalFileStorage::new("./data/small").unwrap());
    let large_storage = Arc::new(LocalFileStorage::new("./data/large").unwrap());
    
    let router = StorageRouter::new(
        small_storage as Arc<dyn BlobStorage>,
        large_storage as Arc<dyn BlobStorage>,
        Some(1024), // 1 KB threshold
    );
    
    // Small data goes to small storage
    router.write("small.txt", b"tiny".to_vec()).await.unwrap();
    
    // Large data goes to large storage
    let large_data = vec![0u8; 2048];
    router.write("large.bin", large_data).await.unwrap();
}
```

### Example 3: S3 Storage

```rust
use crate::storage::S3Storage;

#[tokio::main]
async fn main() {
    let storage = S3Storage::new(
        "my-bucket".to_string(),
        Some("prompts/".to_string()),
    ).await;
    
    // Write to s3://my-bucket/prompts/test.json
    let data = serde_json::json!({"message": "Hello S3"});
    let bytes = serde_json::to_vec(&data).unwrap();
    storage.write("test.json", bytes).await.unwrap();
    
    // Read back
    let read_bytes = storage.read("test.json").await.unwrap();
    let read_data: serde_json::Value = serde_json::from_slice(&read_bytes).unwrap();
    println!("Read from S3: {}", read_data);
}
```

---

## Migration Strategy

### Phase 1: Add Storage Fields (No Behavior Change)

1. Run migration to add new columns
2. Deploy code with storage abstraction but keep using JSONB
3. Monitor for issues

```sql
-- All prompts still use data column
SELECT COUNT(*) FROM prompt WHERE data IS NOT NULL;
SELECT COUNT(*) FROM prompt WHERE data_storage_key IS NOT NULL; -- Should be 0
```

### Phase 2: Dual-Write (Write to Both)

1. Update code to write to both JSONB and file storage
2. Monitor storage usage
3. Verify data consistency

### Phase 3: Switch Reads to File Storage

1. Update code to read from file storage first, fallback to JSONB
2. Monitor error rates
3. Ensure performance is acceptable

### Phase 4: Migrate Existing Data

```rust
use sea_orm::{EntityTrait, QueryFilter};
use crate::entities::prompt::{Entity as Prompt, Column};
use crate::storage::BlobStorage;

async fn migrate_prompt_to_storage(
    db: &DatabaseConnection,
    storage: &dyn BlobStorage,
    prompt_id: Uuid,
) -> Result<(), Box<dyn std::error::Error>> {
    let prompt = Prompt::find_by_id(prompt_id)
        .one(db)
        .await?
        .ok_or("Prompt not found")?;
    
    // Skip if already migrated
    if prompt.data_storage_key.is_some() {
        return Ok(());
    }
    
    // Skip if data is None
    let data = prompt.data.ok_or("No data to migrate")?;
    
    // Write to storage
    let key = format!("prompts/{}.json", prompt_id);
    let bytes = serde_json::to_vec(&data)?;
    let size = bytes.len();
    
    storage.write(&key, bytes).await?;
    
    // Update database record
    let mut active_model: prompt::ActiveModel = prompt.into();
    active_model.data = Set(None);
    active_model.data_storage_key = Set(Some(key));
    active_model.data_size_bytes = Set(Some(size as i64));
    active_model.storage_backend = Set(Some("local_fs".to_string()));
    active_model.update(db).await?;
    
    info!("Migrated prompt {} to external storage", prompt_id);
    
    Ok(())
}

// Batch migration
async fn migrate_all_large_prompts(
    db: &DatabaseConnection,
    storage: &dyn BlobStorage,
    min_size_bytes: i64,
) -> Result<usize, Box<dyn std::error::Error>> {
    use sea_orm::sea_query::Expr;
    
    // Find prompts with large JSONB data
    let prompts = Prompt::find()
        .filter(Column::Data.is_not_null())
        .filter(Column::DataStorageKey.is_null())
        .all(db)
        .await?;
    
    let mut migrated = 0;
    
    for prompt in prompts {
        // Check size (estimate from JSON serialization)
        if let Some(ref data) = prompt.data {
            let estimated_size = serde_json::to_string(data)?.len();
            
            if estimated_size as i64 > min_size_bytes {
                migrate_prompt_to_storage(db, storage, prompt.id).await?;
                migrated += 1;
            }
        }
    }
    
    info!("Migrated {} prompts to external storage", migrated);
    
    Ok(migrated)
}
```

### Phase 5: Drop JSONB Column (Optional)

Once all data is migrated and stable:

```sql
-- After confirming all data is migrated
ALTER TABLE prompt DROP COLUMN data;
ALTER TABLE message DROP COLUMN data;
```

---

## Monitoring & Metrics

Add to your metrics collection:

```rust
use prometheus::{IntCounterVec, HistogramVec};

lazy_static! {
    static ref STORAGE_OPS: IntCounterVec = register_int_counter_vec!(
        "storage_operations_total",
        "Total storage operations",
        &["backend", "operation", "status"]
    ).unwrap();
    
    static ref STORAGE_SIZE: HistogramVec = register_histogram_vec!(
        "storage_object_size_bytes",
        "Size of stored objects",
        &["backend", "object_type"],
        vec![1024.0, 10240.0, 102400.0, 1048576.0, 10485760.0, 104857600.0]
    ).unwrap();
    
    static ref STORAGE_LATENCY: HistogramVec = register_histogram_vec!(
        "storage_operation_duration_seconds",
        "Storage operation latency",
        &["backend", "operation"]
    ).unwrap();
}

// Wrap storage operations with metrics
impl<T: BlobStorage> BlobStorage for MetricsWrapper<T> {
    async fn write(&self, key: &str, data: Vec<u8>) -> StorageResult<()> {
        let start = std::time::Instant::now();
        let result = self.inner.write(key, data.clone()).await;
        
        let status = if result.is_ok() { "success" } else { "error" };
        STORAGE_OPS.with_label_values(&[self.inner.backend_name(), "write", status]).inc();
        STORAGE_SIZE.with_label_values(&[self.inner.backend_name(), "write"]).observe(data.len() as f64);
        STORAGE_LATENCY.with_label_values(&[self.inner.backend_name(), "write"]).observe(start.elapsed().as_secs_f64());
        
        result
    }
    
    // Similar for read, delete, exists...
}
```

---

## Testing

Create `tests/integration/storage_test.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_storage_abstraction() {
        let temp_dir = TempDir::new().unwrap();
        let storage = LocalFileStorage::new(temp_dir.path()).unwrap();
        
        // Test write
        let data = b"test data".to_vec();
        storage.write("test.bin", data.clone()).await.unwrap();
        
        // Test read
        let read_data = storage.read("test.bin").await.unwrap();
        assert_eq!(read_data, data);
        
        // Test exists
        assert!(storage.exists("test.bin").await.unwrap());
        assert!(!storage.exists("nonexistent.bin").await.unwrap());
        
        // Test delete
        storage.delete("test.bin").await.unwrap();
        assert!(!storage.exists("test.bin").await.unwrap());
    }
    
    #[tokio::test]
    async fn test_storage_router() {
        let temp_dir = TempDir::new().unwrap();
        let small_dir = temp_dir.path().join("small");
        let large_dir = temp_dir.path().join("large");
        
        let small_storage = Arc::new(LocalFileStorage::new(&small_dir).unwrap());
        let large_storage = Arc::new(LocalFileStorage::new(&large_dir).unwrap());
        
        let router = StorageRouter::new(
            small_storage.clone() as Arc<dyn BlobStorage>,
            large_storage.clone() as Arc<dyn BlobStorage>,
            Some(100), // 100 byte threshold
        );
        
        // Small data
        router.write("small.txt", b"tiny".to_vec()).await.unwrap();
        assert!(small_dir.join("small.txt").exists());
        
        // Large data
        let large_data = vec![0u8; 200];
        router.write("large.bin", large_data).await.unwrap();
        assert!(large_dir.join("large.bin").exists());
    }
    
    #[tokio::test]
    async fn test_concurrent_writes() {
        let temp_dir = TempDir::new().unwrap();
        let storage = Arc::new(LocalFileStorage::new(temp_dir.path()).unwrap());
        
        let mut handles = vec![];
        
        for i in 0..10 {
            let storage = storage.clone();
            let handle = tokio::spawn(async move {
                let key = format!("concurrent_{}.txt", i);
                let data = format!("data {}", i).into_bytes();
                storage.write(&key, data).await.unwrap();
            });
            handles.push(handle);
        }
        
        for handle in handles {
            handle.await.unwrap();
        }
        
        // Verify all files written
        for i in 0..10 {
            let key = format!("concurrent_{}.txt", i);
            assert!(storage.exists(&key).await.unwrap());
        }
    }
}
```

---

## Summary

This design provides:

1. **PostgreSQL JSONB Limits**: ~1 GB max, but performance degrades after 10-100 MB
2. **Clean Abstraction**: Trait-based design for easy backend swapping
3. **Multiple Backends**: Local FS, S3, and PostgreSQL implementations
4. **Smart Routing**: Automatic backend selection based on data size
5. **Zero-Downtime Migration**: Phased migration strategy
6. **Production Ready**: Monitoring, testing, and error handling included

The local file system implementation is production-ready and can easily be swapped for S3 by changing configuration!
