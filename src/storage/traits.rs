//! Core traits and types for the storage abstraction layer

use async_trait::async_trait;
use std::fmt;

/// Result type for storage operations
pub type StorageResult<T> = Result<T, StorageError>;

/// Errors that can occur during storage operations
#[derive(Debug)]
pub enum StorageError {
    /// File or object not found
    NotFound(String),
    /// Permission denied
    PermissionDenied(String),
    /// I/O error
    Io(std::io::Error),
    /// Network or connectivity error
    Network(String),
    /// Configuration error
    Config(String),
    /// Other error
    Other(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::NotFound(msg) => write!(f, "Not found: {}", msg),
            StorageError::PermissionDenied(msg) => write!(f, "Permission denied: {}", msg),
            StorageError::Io(err) => write!(f, "I/O error: {}", err),
            StorageError::Network(msg) => write!(f, "Network error: {}", msg),
            StorageError::Config(msg) => write!(f, "Configuration error: {}", msg),
            StorageError::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => StorageError::NotFound(err.to_string()),
            std::io::ErrorKind::PermissionDenied => {
                StorageError::PermissionDenied(err.to_string())
            }
            _ => StorageError::Io(err),
        }
    }
}

/// Configuration for storage backends
#[derive(Debug, Clone)]
pub enum StorageConfig {
    /// Local filesystem storage
    Local {
        /// Base directory path for storing files
        base_path: String,
    },
    /// Amazon S3 or S3-compatible storage
    S3 {
        /// S3 bucket name
        bucket: String,
        /// AWS region
        region: String,
        /// AWS access key (optional, uses credentials chain if not provided)
        access_key: Option<String>,
        /// AWS secret key (optional)
        secret_key: Option<String>,
        /// Custom endpoint for S3-compatible services (e.g., MinIO, DigitalOcean Spaces)
        endpoint: Option<String>,
    },
}

/// Metadata about a stored file
#[derive(Debug, Clone)]
pub struct FileMetadata {
    /// File path or key
    pub path: String,
    /// File size in bytes
    pub size: u64,
    /// MIME type (if available)
    pub content_type: Option<String>,
    /// Last modified timestamp (Unix timestamp)
    pub last_modified: Option<i64>,
    /// ETag or content hash (if available)
    pub etag: Option<String>,
}

/// Options for storing files
#[derive(Debug, Clone, Default)]
pub struct PutOptions {
    /// MIME type
    pub content_type: Option<String>,
    /// Whether to overwrite existing file
    pub overwrite: bool,
    /// Custom metadata
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

/// Options for retrieving files
#[derive(Debug, Clone, Default)]
pub struct GetOptions {
    /// Byte range to retrieve (start, end)
    pub range: Option<(u64, u64)>,
}

/// Abstract storage backend interface
///
/// This trait defines the common operations that all storage backends must implement.
/// It allows the application to swap between different storage systems (local filesystem,
/// S3, etc.) without changing business logic.
#[async_trait]
pub trait StorageBackend: Send + Sync {
    /// Store a file
    ///
    /// # Arguments
    /// * `path` - File path or object key
    /// * `data` - File contents as bytes
    /// * `options` - Optional storage options
    ///
    /// # Returns
    /// Metadata about the stored file
    async fn put(
        &self,
        path: &str,
        data: Vec<u8>,
        options: Option<PutOptions>,
    ) -> StorageResult<FileMetadata>;

    /// Retrieve a file
    ///
    /// # Arguments
    /// * `path` - File path or object key
    /// * `options` - Optional retrieval options
    ///
    /// # Returns
    /// File contents as bytes
    async fn get(&self, path: &str, options: Option<GetOptions>) -> StorageResult<Vec<u8>>;

    /// Check if a file exists
    ///
    /// # Arguments
    /// * `path` - File path or object key
    ///
    /// # Returns
    /// `true` if the file exists, `false` otherwise
    async fn exists(&self, path: &str) -> StorageResult<bool>;

    /// Delete a file
    ///
    /// # Arguments
    /// * `path` - File path or object key
    async fn delete(&self, path: &str) -> StorageResult<()>;

    /// List files with a given prefix
    ///
    /// # Arguments
    /// * `prefix` - Path prefix to filter by
    ///
    /// # Returns
    /// Vector of file paths matching the prefix
    async fn list(&self, prefix: &str) -> StorageResult<Vec<String>>;

    /// Get metadata about a file without retrieving its contents
    ///
    /// # Arguments
    /// * `path` - File path or object key
    ///
    /// # Returns
    /// Metadata about the file
    async fn metadata(&self, path: &str) -> StorageResult<FileMetadata>;

    /// Get a public URL for a file (if supported)
    ///
    /// # Arguments
    /// * `path` - File path or object key
    /// * `expires_in_secs` - Optional expiration time in seconds
    ///
    /// # Returns
    /// URL string, or None if not supported by this backend
    async fn get_url(&self, path: &str, expires_in_secs: Option<u64>) -> StorageResult<String>;

    /// Get a human-readable name for this storage backend
    fn backend_name(&self) -> &str;
}
